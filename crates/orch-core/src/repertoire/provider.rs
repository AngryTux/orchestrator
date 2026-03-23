use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::SpecMetadata;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderSpec {
    pub kind: String,
    pub version: u32,
    pub metadata: SpecMetadata,
    pub detection: ProviderDetection,
    pub invocation: ProviderInvocation,
    pub auth: ProviderAuth,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub install: Option<ProviderInstall>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderDetection {
    pub binary: String,
    #[serde(default)]
    pub version_cmd: Vec<String>,
    #[serde(default)]
    pub auth_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderInvocation {
    pub cmd: Vec<String>,
    pub prompt_flag: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_flag: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_prompt_flag: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub json_schema_flag: Option<String>,
    #[serde(default)]
    pub output_format_flag: Vec<String>,
    #[serde(default)]
    pub extra_flags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderAuth {
    pub env_var: String,
    pub methods: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderInstall {
    pub hint: String,
    #[serde(default)]
    pub commands: HashMap<String, Vec<String>>,
}
