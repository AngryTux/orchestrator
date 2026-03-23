mod formation;
mod integration;
mod isolation;
mod provider;

pub use formation::*;
pub use integration::*;
pub use isolation::*;
pub use provider::*;

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpecMetadata {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub risk: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum RepertoireError {
    #[error("spec not found: {category}/{name}")]
    NotFound { name: String, category: String },
    #[error("failed to read spec: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed to parse spec: {0}")]
    Parse(#[from] serde_yaml::Error),
}

/// Loads specs from user custom directory or repertoire.
///
/// Resolution order: user custom → repertoire → error.
pub struct Repertoire {
    user_dir: PathBuf,
    repertoire_dir: PathBuf,
}

impl Repertoire {
    pub fn new(user_dir: PathBuf, repertoire_dir: PathBuf) -> Self {
        Self {
            user_dir,
            repertoire_dir,
        }
    }

    pub fn load_provider(&self, name: &str) -> Result<ProviderSpec, RepertoireError> {
        self.resolve("providers", name)
    }

    pub fn load_integration(&self, name: &str) -> Result<IntegrationSpec, RepertoireError> {
        self.resolve("integrations", name)
    }

    pub fn load_formation(&self, name: &str) -> Result<FormationSpec, RepertoireError> {
        self.resolve("formations", name)
    }

    pub fn load_isolation(&self, name: &str) -> Result<IsolationProfileSpec, RepertoireError> {
        self.resolve("isolation", name)
    }

    fn resolve<T: DeserializeOwned>(
        &self,
        category: &str,
        name: &str,
    ) -> Result<T, RepertoireError> {
        // Reject path traversal attempts
        if name.contains('/') || name.contains('\\') || name.contains('\0') || name.contains("..") {
            return Err(RepertoireError::NotFound {
                name: name.to_string(),
                category: category.to_string(),
            });
        }
        let filename = format!("{}.yaml", name);

        // 1. User custom (priority)
        let user_path = self.user_dir.join(category).join(&filename);
        if user_path.exists() {
            let content = std::fs::read_to_string(&user_path)?;
            return Ok(serde_yaml::from_str(&content)?);
        }

        // 2. Repertoire
        let repo_path = self.repertoire_dir.join(category).join(&filename);
        if repo_path.exists() {
            let content = std::fs::read_to_string(&repo_path)?;
            return Ok(serde_yaml::from_str(&content)?);
        }

        Err(RepertoireError::NotFound {
            name: name.to_string(),
            category: category.to_string(),
        })
    }
}
