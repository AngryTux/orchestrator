use crate::contracts::{CodaContract, FormationType, ResultContract};
use crate::credentials::CredentialStore;
use crate::isolation::{self, SpawnConfig};
use crate::repertoire::ProviderSpec;
use std::sync::Arc;

pub struct PerformanceEngine {
    credentials: Arc<CredentialStore>,
}

impl PerformanceEngine {
    pub fn new(credentials: Arc<CredentialStore>) -> Self {
        Self { credentials }
    }

    /// Execute a performance with the given formation and optional models.
    ///
    /// `models` controls which model each section uses:
    /// - `&[]` → provider default (no --model flag)
    /// - `&["haiku"]` → all sections use haiku
    /// - `&["haiku", "opus"]` → section 1 uses haiku, section 2 uses opus
    pub async fn perform(
        &self,
        namespace: &str,
        prompt: &str,
        provider_spec: &ProviderSpec,
        formation: FormationType,
        models: &[String],
    ) -> anyhow::Result<CodaContract> {
        match formation {
            FormationType::Solo => {
                let model = models.first().map(|s| s.as_str());
                self.perform_solo(namespace, prompt, provider_spec, model)
                    .await
            }
            FormationType::Duet => {
                let model_a = models.first().map(|s| s.as_str());
                let model_b = models.get(1).or(models.first()).map(|s| s.as_str());
                self.perform_duet(namespace, prompt, provider_spec, model_a, model_b)
                    .await
            }
            other => Err(anyhow::anyhow!("formation not supported: {other:?}")),
        }
    }

    async fn perform_solo(
        &self,
        namespace: &str,
        prompt: &str,
        provider_spec: &ProviderSpec,
        model: Option<&str>,
    ) -> anyhow::Result<CodaContract> {
        let perf_id = generate_id("perf");
        let section = self
            .spawn_section(namespace, "sec-001", prompt, provider_spec, model)
            .await?;

        let summary = section.output.clone();
        let duration = section.duration_ms;

        Ok(CodaContract {
            performance_id: perf_id,
            summary,
            formation: FormationType::Solo,
            harmony: true,
            sections: vec![section],
            total_tokens_in: 0,
            total_tokens_out: 0,
            total_cost_usd: 0.0,
            total_duration_ms: duration,
        })
    }

    async fn perform_duet(
        &self,
        namespace: &str,
        prompt: &str,
        provider_spec: &ProviderSpec,
        model_a: Option<&str>,
        model_b: Option<&str>,
    ) -> anyhow::Result<CodaContract> {
        let perf_id = generate_id("perf");

        let (r1, r2) = tokio::join!(
            self.spawn_section(namespace, "sec-001", prompt, provider_spec, model_a),
            self.spawn_section(namespace, "sec-002", prompt, provider_spec, model_b),
        );

        let sec1 = r1?;
        let sec2 = r2?;

        let harmony = sec1.success && sec2.success;
        let summary = consolidate(&[&sec1, &sec2]);
        let total_duration = sec1.duration_ms.max(sec2.duration_ms);

        Ok(CodaContract {
            performance_id: perf_id,
            summary,
            formation: FormationType::Duet,
            harmony,
            sections: vec![sec1, sec2],
            total_tokens_in: 0,
            total_tokens_out: 0,
            total_cost_usd: 0.0,
            total_duration_ms: total_duration,
        })
    }

    async fn spawn_section(
        &self,
        namespace: &str,
        section_id: &str,
        prompt: &str,
        provider_spec: &ProviderSpec,
        model: Option<&str>,
    ) -> anyhow::Result<ResultContract> {
        let ws_id = generate_id("ws");
        let api_key = self
            .credentials
            .get(namespace, &provider_spec.metadata.name)?;

        let (binary, args) = build_invocation(provider_spec, prompt, model);

        let uses_detect = provider_spec.auth.methods.contains(&"detect".to_string());

        let env = if uses_detect {
            vec![]
        } else {
            vec![(provider_spec.auth.env_var.clone(), api_key)]
        };

        let result = isolation::spawn(&SpawnConfig {
            binary,
            args,
            env,
            inherit_env: uses_detect,
            ..SpawnConfig::default()
        })
        .await?;

        let success = result.exit_code == 0;
        let model_used = model.unwrap_or("default").to_string();

        Ok(ResultContract {
            workspace_id: ws_id,
            section_id: section_id.to_string(),
            provider: provider_spec.metadata.name.clone(),
            model: model_used,
            output: result.stdout.trim().to_string(),
            tokens_in: 0,
            tokens_out: 0,
            cost_usd: 0.0,
            duration_ms: result.duration_ms,
            success,
            error: if success {
                None
            } else {
                Some(result.stderr.trim().to_string())
            },
        })
    }
}

fn consolidate(sections: &[&ResultContract]) -> String {
    let mut summary = String::new();
    for (i, section) in sections.iter().enumerate() {
        if !summary.is_empty() {
            summary.push_str("\n\n");
        }
        let model_tag = if section.model != "default" {
            format!(" [{}]", section.model)
        } else {
            String::new()
        };
        if section.success {
            summary.push_str(&format!(
                "--- Section {}{} ---\n{}",
                i + 1,
                model_tag,
                section.output
            ));
        } else {
            summary.push_str(&format!(
                "--- Section {}{} (failed) ---\n{}",
                i + 1,
                model_tag,
                section.error.as_deref().unwrap_or("unknown error")
            ));
        }
    }
    summary
}

fn build_invocation(
    spec: &ProviderSpec,
    prompt: &str,
    model: Option<&str>,
) -> (String, Vec<String>) {
    let binary = spec.invocation.cmd[0].clone();
    let mut args: Vec<String> = spec.invocation.cmd[1..].to_vec();

    // Add model flag if specified and provider supports it
    if let (Some(m), Some(flag)) = (model, &spec.invocation.model_flag) {
        args.push(flag.clone());
        args.push(m.to_string());
    }

    args.push(spec.invocation.prompt_flag.clone());
    args.push(prompt.to_string());
    args.extend(spec.invocation.output_format_flag.clone());
    args.extend(spec.invocation.extra_flags.clone());
    (binary, args)
}

fn generate_id(prefix: &str) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis();
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}-{ts:x}-{seq:x}")
}
