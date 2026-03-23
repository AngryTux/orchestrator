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

    /// Execute a performance with the given formation.
    pub async fn perform(
        &self,
        namespace: &str,
        prompt: &str,
        provider_spec: &ProviderSpec,
        formation: FormationType,
    ) -> anyhow::Result<CodaContract> {
        match formation {
            FormationType::Solo => self.perform_solo(namespace, prompt, provider_spec).await,
            FormationType::Duet => self.perform_duet(namespace, prompt, provider_spec).await,
            other => Err(anyhow::anyhow!("formation not supported: {other:?}")),
        }
    }

    /// Solo: one prompt → one provider → one Coda.
    async fn perform_solo(
        &self,
        namespace: &str,
        prompt: &str,
        provider_spec: &ProviderSpec,
    ) -> anyhow::Result<CodaContract> {
        let perf_id = generate_id("perf");
        let section = self
            .spawn_section(namespace, "sec-001", prompt, provider_spec)
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

    /// Duet: same prompt → 2 parallel workspaces → consolidation → Coda.
    async fn perform_duet(
        &self,
        namespace: &str,
        prompt: &str,
        provider_spec: &ProviderSpec,
    ) -> anyhow::Result<CodaContract> {
        let perf_id = generate_id("perf");

        // Spawn 2 sections in parallel
        let (r1, r2) = tokio::join!(
            self.spawn_section(namespace, "sec-001", prompt, provider_spec),
            self.spawn_section(namespace, "sec-002", prompt, provider_spec),
        );

        let sec1 = r1?;
        let sec2 = r2?;

        // Harmony: both sections succeeded
        let harmony = sec1.success && sec2.success;

        // Consolidation (Foundation: simple merge; future: Maestro LLM call)
        let summary = consolidate(&[&sec1, &sec2]);
        let total_duration = sec1.duration_ms.max(sec2.duration_ms); // parallel = max, not sum

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

    /// Spawn a single section: invoke the provider and capture the result.
    async fn spawn_section(
        &self,
        namespace: &str,
        section_id: &str,
        prompt: &str,
        provider_spec: &ProviderSpec,
    ) -> anyhow::Result<ResultContract> {
        let ws_id = generate_id("ws");
        let api_key = self
            .credentials
            .get(namespace, &provider_spec.metadata.name)?;

        let (binary, args) = build_invocation(provider_spec, prompt);

        let result = isolation::spawn(&SpawnConfig {
            binary,
            args,
            env: vec![(provider_spec.auth.env_var.clone(), api_key)],
            ..SpawnConfig::default()
        })
        .await?;

        let success = result.exit_code == 0;
        Ok(ResultContract {
            workspace_id: ws_id,
            section_id: section_id.to_string(),
            provider: provider_spec.metadata.name.clone(),
            model: String::new(),
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

/// Foundation consolidation: merge section outputs into a summary.
/// Future: this becomes a Maestro LLM call.
fn consolidate(sections: &[&ResultContract]) -> String {
    let mut summary = String::new();
    for (i, section) in sections.iter().enumerate() {
        if !summary.is_empty() {
            summary.push_str("\n\n");
        }
        if section.success {
            summary.push_str(&format!("--- Section {} ---\n{}", i + 1, section.output));
        } else {
            summary.push_str(&format!(
                "--- Section {} (failed) ---\n{}",
                i + 1,
                section.error.as_deref().unwrap_or("unknown error")
            ));
        }
    }
    summary
}

fn build_invocation(spec: &ProviderSpec, prompt: &str) -> (String, Vec<String>) {
    let binary = spec.invocation.cmd[0].clone();
    let mut args: Vec<String> = spec.invocation.cmd[1..].to_vec();
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
