use aes_gcm::aead::rand_core::RngCore;
use aes_gcm::aead::{Aead, KeyInit, OsRng};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use anyhow::{Context, anyhow};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;

pub struct CredentialStore {
    cipher: Aes256Gcm,
    base_dir: PathBuf,
}

impl CredentialStore {
    /// Open or create a credential store.
    ///
    /// Loads the master key from `base_dir/.master_key`.
    /// If the key file doesn't exist, generates a new 256-bit key.
    pub fn open(base_dir: PathBuf) -> anyhow::Result<Self> {
        std::fs::create_dir_all(&base_dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&base_dir, std::fs::Permissions::from_mode(0o700))?;
        }

        let key_path = base_dir.join(".master_key");

        let key = if key_path.exists() {
            let bytes = std::fs::read(&key_path).context("reading master key")?;
            if bytes.len() != 32 {
                return Err(anyhow!(
                    "corrupt master key: expected 32 bytes, got {}",
                    bytes.len()
                ));
            }
            *Key::<Aes256Gcm>::from_slice(&bytes)
        } else {
            // Fix: create file with 0600 atomically (no TOCTOU window)
            let key = Aes256Gcm::generate_key(OsRng);
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&key_path)
                .context("creating master key file")?;
            file.write_all(key.as_slice())
                .context("writing master key")?;
            key
        };

        Ok(Self {
            cipher: Aes256Gcm::new(&key),
            base_dir,
        })
    }

    /// Encrypt and store a credential for a provider in a namespace.
    pub fn store(&self, namespace: &str, provider: &str, key: &str) -> anyhow::Result<()> {
        validate_name(namespace, "namespace")?;
        validate_name(provider, "provider")?;
        let dir = self.credential_dir(namespace);
        std::fs::create_dir_all(&dir)?;
        let encrypted = self.encrypt(key.as_bytes())?;
        std::fs::write(dir.join(format!("{provider}.enc")), encrypted)?;
        Ok(())
    }

    /// Decrypt and return a stored credential.
    pub fn get(&self, namespace: &str, provider: &str) -> anyhow::Result<String> {
        validate_name(namespace, "namespace")?;
        validate_name(provider, "provider")?;
        let path = self.credential_dir(namespace).join(format!("{provider}.enc"));
        let encrypted = std::fs::read_to_string(&path)
            .map_err(|_| anyhow!("credential not found: {namespace}/{provider}"))?;
        let decrypted = self.decrypt(&encrypted)?;
        Ok(String::from_utf8(decrypted)?)
    }

    /// Delete a stored credential.
    pub fn delete(&self, namespace: &str, provider: &str) -> anyhow::Result<()> {
        validate_name(namespace, "namespace")?;
        validate_name(provider, "provider")?;
        let path = self.credential_dir(namespace).join(format!("{provider}.enc"));
        std::fs::remove_file(&path)
            .map_err(|_| anyhow!("credential not found: {namespace}/{provider}"))
    }

    /// List providers with stored credentials in a namespace.
    pub fn list(&self, namespace: &str) -> anyhow::Result<Vec<String>> {
        validate_name(namespace, "namespace")?;
        let dir = self.credential_dir(namespace);
        if !dir.exists() {
            return Ok(vec![]);
        }
        let mut providers = vec![];
        for entry in std::fs::read_dir(dir)? {
            let name = entry?.file_name();
            if let Some(provider) = name.to_str().and_then(|n| n.strip_suffix(".enc")) {
                providers.push(provider.to_string());
            }
        }
        providers.sort();
        Ok(providers)
    }

    /// Decrypt a credential and return it as an (env_var, value) pair
    /// ready for injection into a workspace process.
    pub fn env_pair(
        &self,
        namespace: &str,
        provider: &str,
        env_var: &str,
    ) -> anyhow::Result<(String, String)> {
        let value = self.get(namespace, provider)?;
        Ok((env_var.to_string(), value))
    }

    fn credential_dir(&self, namespace: &str) -> PathBuf {
        self.base_dir
            .join("namespaces")
            .join(namespace)
            .join("credentials")
    }

    fn encrypt(&self, plaintext: &[u8]) -> anyhow::Result<String> {
        let mut nonce_bytes = [0u8; 12];
        OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = self
            .cipher
            .encrypt(nonce, plaintext)
            .map_err(|e| anyhow!("encryption failed: {e}"))?;

        Ok(format!(
            "{}.{}",
            BASE64.encode(nonce_bytes),
            BASE64.encode(ciphertext)
        ))
    }

    fn decrypt(&self, encrypted: &str) -> anyhow::Result<Vec<u8>> {
        let (nonce_b64, ct_b64) = encrypted
            .split_once('.')
            .ok_or_else(|| anyhow!("invalid encrypted format"))?;

        let nonce_bytes = BASE64.decode(nonce_b64).context("decoding nonce")?;
        if nonce_bytes.len() != 12 {
            return Err(anyhow!(
                "invalid nonce length: expected 12, got {}",
                nonce_bytes.len()
            ));
        }
        let ciphertext = BASE64.decode(ct_b64).context("decoding ciphertext")?;

        let nonce = Nonce::from_slice(&nonce_bytes);
        self.cipher
            .decrypt(nonce, ciphertext.as_ref())
            .map_err(|e| anyhow!("decryption failed (wrong key?): {e}"))
    }
}

/// Validate that a name is safe for use in filesystem paths.
/// Rejects empty strings, directory traversal, and special characters.
fn validate_name(name: &str, field: &str) -> anyhow::Result<()> {
    if name.is_empty() {
        return Err(anyhow!("{field} must not be empty"));
    }
    if name.contains('/') || name.contains('\\') || name.contains('\0') || name.contains("..") {
        return Err(anyhow!("{field} contains invalid characters: {name:?}"));
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(anyhow!(
            "{field} must match [a-zA-Z0-9_-]: {name:?}"
        ));
    }
    Ok(())
}
