use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PerformanceState {
    Arranging,
    Conducting,
    Performing,
    Consolidating,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FormationType {
    Solo,
    Duet,
    Quartet,
    Chamber,
    Symphonic,
    Opera,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntentContract {
    pub prompt: String,
    pub namespace: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub formation: Option<FormationType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub isolation_profile: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
}

impl IntentContract {
    pub fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty("prompt", &self.prompt)?;
        require_non_empty("namespace", &self.namespace)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Section {
    pub id: String,
    pub provider: String,
    pub model: String,
    pub prompt: String,
    #[serde(default)]
    pub depends_on: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoreContract {
    pub performance_id: String,
    pub formation: FormationType,
    pub sections: Vec<Section>,
}

impl ScoreContract {
    pub fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty("performance_id", &self.performance_id)?;
        if self.sections.is_empty() {
            return Err(ValidationError::new("sections", "must not be empty"));
        }
        for (i, section) in self.sections.iter().enumerate() {
            let prefix = format!("sections[{}]", i);
            require_non_empty(&format!("{prefix}.id"), &section.id)?;
            require_non_empty(&format!("{prefix}.provider"), &section.provider)?;
            require_non_empty(&format!("{prefix}.model"), &section.model)?;
            require_non_empty(&format!("{prefix}.prompt"), &section.prompt)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResultContract {
    pub workspace_id: String,
    pub section_id: String,
    pub provider: String,
    pub model: String,
    pub output: String,
    pub tokens_in: u64,
    pub tokens_out: u64,
    pub cost_usd: f64,
    pub duration_ms: u64,
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl ResultContract {
    pub fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty("workspace_id", &self.workspace_id)?;
        require_non_empty("section_id", &self.section_id)?;
        require_non_empty("provider", &self.provider)?;
        require_non_empty("model", &self.model)?;
        if !self.success && self.error.is_none() {
            return Err(ValidationError::new(
                "error",
                "must be present when success is false",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodaContract {
    pub performance_id: String,
    pub summary: String,
    pub formation: FormationType,
    pub harmony: bool,
    pub sections: Vec<ResultContract>,
    pub total_tokens_in: u64,
    pub total_tokens_out: u64,
    pub total_cost_usd: f64,
    pub total_duration_ms: u64,
}

impl CodaContract {
    pub fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty("performance_id", &self.performance_id)?;
        require_non_empty("summary", &self.summary)?;
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
#[error("{field}: {message}")]
pub struct ValidationError {
    pub field: String,
    pub message: String,
}

impl ValidationError {
    fn new(field: &str, message: &str) -> Self {
        Self {
            field: field.to_string(),
            message: message.to_string(),
        }
    }
}

fn require_non_empty(field: &str, value: &str) -> Result<(), ValidationError> {
    if value.is_empty() {
        return Err(ValidationError::new(field, "must not be empty"));
    }
    Ok(())
}
