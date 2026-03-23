use serde::{Deserialize, Serialize};

use super::SpecMetadata;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntegrationSpec {
    pub kind: String,
    pub version: u32,
    pub metadata: SpecMetadata,
    pub role: IntegrationRole,
    pub provider: IntegrationProvider,
    pub phases: Vec<IntegrationPhase>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntegrationRole {
    Arranger,
    Maestro,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntegrationProvider {
    pub default: String,
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntegrationPhase {
    pub name: String,
    pub system_prompt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub json_schema: Option<serde_json::Value>,
}
