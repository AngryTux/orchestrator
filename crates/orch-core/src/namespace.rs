use anyhow::{anyhow, Result};
use serde::Serialize;
use std::path::PathBuf;

pub struct NamespaceManager {
    base_dir: PathBuf,
}

impl NamespaceManager {
    pub fn new(base_dir: PathBuf) -> Self {
        Self { base_dir }
    }

    /// Create the 3 default namespaces.
    pub fn init_defaults(&self) -> Result<()> {
        self.create("default")?;
        self.create("secure")?;
        self.create("lab")?;
        Ok(())
    }

    pub fn create(&self, name: &str) -> Result<()> {
        validate_name(name)?;
        let dir = self.ns_dir(name);
        std::fs::create_dir_all(&dir)?;
        Ok(())
    }

    pub fn list(&self) -> Result<Vec<String>> {
        let dir = self.base_dir.join("namespaces");
        if !dir.exists() {
            return Ok(vec![]);
        }
        let mut names = vec![];
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() && entry.file_name().to_str().is_some() {
                names.push(entry.file_name().to_string_lossy().to_string());
            }
        }
        names.sort();
        Ok(names)
    }

    pub fn inspect(&self, name: &str) -> Result<Option<NamespaceInfo>> {
        validate_name(name)?;
        let dir = self.ns_dir(name);
        if !dir.exists() {
            return Ok(None);
        }
        Ok(Some(NamespaceInfo {
            name: name.to_string(),
            path: dir,
        }))
    }

    pub fn delete(&self, name: &str) -> Result<()> {
        validate_name(name)?;
        let dir = self.ns_dir(name);
        if !dir.exists() {
            return Err(anyhow!("namespace not found: {name}"));
        }
        std::fs::remove_dir_all(&dir)?;
        Ok(())
    }

    fn ns_dir(&self, name: &str) -> PathBuf {
        self.base_dir.join("namespaces").join(name)
    }
}

fn validate_name(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(anyhow!("namespace name must not be empty"));
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(anyhow!("namespace name must match [a-zA-Z0-9_-]: {name:?}"));
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
pub struct NamespaceInfo {
    pub name: String,
    pub path: PathBuf,
}
