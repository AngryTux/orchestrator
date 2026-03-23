use serde::{Deserialize, Serialize};

use super::SpecMetadata;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FormationSpec {
    pub kind: String,
    pub version: u32,
    pub metadata: SpecMetadata,
    pub min_sections: u32,
    pub max_sections: u32,
    #[serde(default)]
    pub parallel: bool,
    pub consolidation: ConsolidationType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConsolidationType {
    Passthrough,
    Required,
    Optional,
}
