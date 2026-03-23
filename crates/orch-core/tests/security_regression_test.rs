//! Security regression tests — validates fixes from the security audit.
//! Each test maps to a specific finding. If any regresses, the audit fix broke.

use orch_core::credentials::CredentialStore;
use orch_core::isolation::*;
use orch_core::repertoire::Repertoire;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "orch-sec-{}-{}-{}",
        std::process::id(),
        name,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

// ─── Credential path traversal ───────────────────────────

#[test]
fn credential_rejects_dot_dot_namespace() {
    let dir = temp_dir("cred-dotdot");
    let store = CredentialStore::open(dir.clone()).unwrap();
    assert!(store.store("../../etc", "claude", "key").is_err());
    assert!(store.get("../secret", "claude").is_err());
    assert!(store.list("../../tmp").is_err());
    assert!(store.delete("../x", "claude").is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn credential_rejects_slash_in_provider() {
    let dir = temp_dir("cred-slash");
    let store = CredentialStore::open(dir.clone()).unwrap();
    assert!(store.store("default", "../../../shadow", "key").is_err());
    assert!(store.store("default", "a/b", "key").is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

// ─── Repertoire path traversal ───────────────────────────

#[test]
fn repertoire_rejects_path_traversal() {
    let dir = temp_dir("rep-traverse");
    let repo = Repertoire::new(dir.join("custom"), dir.join("repo"));
    let result = repo.load_provider("../../etc/passwd");
    assert!(result.is_err());
    let result = repo.load_formation("../secret");
    assert!(result.is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

// ─── Credential nonce validation ─────────────────────────

#[test]
fn credential_handles_corrupt_encrypted_file() {
    let dir = temp_dir("corrupt");
    let store = CredentialStore::open(dir.clone()).unwrap();
    store.store("default", "test", "secret").unwrap();

    // Corrupt the encrypted file
    let enc_path = dir.join("namespaces/default/credentials/test.enc");
    std::fs::write(&enc_path, "not-valid-base64").unwrap();
    assert!(store.get("default", "test").is_err());

    // Truncated nonce (too short base64)
    std::fs::write(&enc_path, "YQ==.AAAA").unwrap(); // nonce=1 byte, not 12
    let result = store.get("default", "test");
    assert!(result.is_err());
    assert!(
        result.unwrap_err().to_string().contains("nonce"),
        "should mention nonce in error"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

// ─── Master key permissions ──────────────────────────────

#[test]
fn master_key_created_with_0600() {
    let dir = temp_dir("key-perms");
    let _store = CredentialStore::open(dir.clone()).unwrap();
    let key_path = dir.join(".master_key");
    let metadata = std::fs::metadata(&key_path).unwrap();
    let mode = metadata.permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "master key should be 0600, got {:o}", mode);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn base_dir_created_with_0700() {
    let dir = temp_dir("dir-perms");
    let store_dir = dir.join("store");
    let _store = CredentialStore::open(store_dir.clone()).unwrap();
    let metadata = std::fs::metadata(&store_dir).unwrap();
    let mode = metadata.permissions().mode() & 0o777;
    assert_eq!(mode, 0o700, "base dir should be 0700, got {:o}", mode);
    let _ = std::fs::remove_dir_all(&dir);
}

// ─── Isolation: stdin is null ────────────────────────────

#[tokio::test]
async fn isolated_process_cannot_read_stdin() {
    let result = spawn(&SpawnConfig {
        binary: "/bin/sh".into(),
        args: vec!["-c".into(), "read -t 1 input 2>/dev/null; echo ${input:-empty}".into()],
        ..SpawnConfig::default()
    })
    .await
    .unwrap();

    assert_eq!(result.stdout.trim(), "empty", "stdin should be /dev/null");
}

// ─── Isolation: no_new_privs is set ──────────────────────

#[tokio::test]
async fn no_new_privs_is_always_set() {
    // /proc/self/status contains NoNewPrivs line
    let result = spawn(&SpawnConfig {
        binary: "/bin/grep".into(),
        args: vec!["NoNewPrivs".into(), "/proc/self/status".into()],
        // No landlock, no seccomp — just checking no_new_privs
        ..SpawnConfig::default()
    })
    .await
    .unwrap();

    assert_eq!(result.exit_code, 0);
    assert!(
        result.stdout.contains("NoNewPrivs:\t1"),
        "no_new_privs not set: {}",
        result.stdout
    );
}

// ─── Isolation: write_paths cannot execute ───────────────

#[tokio::test]
async fn write_path_cannot_execute_binaries() {
    if !landlock_available() {
        return;
    }

    let write_dir = temp_dir("no-exec");
    let read_dir = temp_dir("no-exec-r");

    // Create an executable in the write dir
    let script = write_dir.join("malicious.sh");
    std::fs::write(&script, "#!/bin/sh\necho pwned\n").unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();

    let result = spawn(&SpawnConfig {
        binary: "/bin/sh".into(),
        args: vec!["-c".into(), format!("{}", script.display())],
        read_paths: vec![read_dir.clone()],
        write_paths: vec![write_dir.clone()],
        enable_landlock: true,
        ..SpawnConfig::default()
    })
    .await
    .unwrap();

    assert_ne!(
        result.exit_code, 0,
        "should not be able to execute from write-only dir"
    );

    let _ = std::fs::remove_dir_all(&write_dir);
    let _ = std::fs::remove_dir_all(&read_dir);
}
