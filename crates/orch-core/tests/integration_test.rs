//! Integration tests for the full performance pipeline.
//!
//! These tests validate the stack: YAML spec loading → credentials → engine → isolation → Coda.
//! They use mock provider scripts (shell), not real LLMs.
//! Any valid provider spec + mock binary should produce a correct Coda.

use orch_core::contracts::FormationType;
use orch_core::credentials::CredentialStore;
use orch_core::engine::PerformanceEngine;
use orch_core::repertoire::ProviderSpec;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

const FIXTURES: &str = env!("CARGO_MANIFEST_DIR");

// ─── Helpers ─────────────────────────────────────────────

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "orch-integ-{}-{}-{}",
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

/// Load a provider spec from fixtures and patch the binary path.
fn load_spec(fixture_name: &str, binary_path: &Path) -> ProviderSpec {
    let yaml_path = Path::new(FIXTURES)
        .join("tests/fixtures/providers")
        .join(fixture_name);
    let yaml = std::fs::read_to_string(&yaml_path)
        .unwrap_or_else(|e| panic!("failed to read fixture {}: {e}", yaml_path.display()));

    // Replace placeholder with actual binary path
    let yaml = yaml.replace("__BINARY_PATH__", &binary_path.to_string_lossy());
    serde_yaml::from_str(&yaml)
        .unwrap_or_else(|e| panic!("failed to parse fixture {}: {e}", yaml_path.display()))
}

/// Create a mock provider script and return its path.
fn create_mock_binary(dir: &Path, name: &str, script: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, script).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

/// Set up a CredentialStore with a pre-stored key.
fn setup_credentials(name: &str, provider: &str, key: &str) -> (Arc<CredentialStore>, PathBuf) {
    let dir = temp_dir(name);
    let store = CredentialStore::open(dir.clone()).unwrap();
    store.store("default", provider, key).unwrap();
    (Arc::new(store), dir)
}

// ═══════════════════════════════════════════════════════════
// Pipeline: spec loaded from YAML → credential → engine → Coda
// ═══════════════════════════════════════════════════════════

#[tokio::test]
async fn pipeline_echo_provider_returns_prompt() {
    let dir = temp_dir("echo");
    let binary = create_mock_binary(
        &dir,
        "echo-provider",
        r#"#!/bin/sh
while [ $# -gt 0 ]; do
    case "$1" in -p) shift; echo "Echo: $1"; exit 0 ;; esac
    shift
done
echo "no prompt"; exit 1
"#,
    );

    let spec = load_spec("echo-provider.yaml", &binary);
    assert_eq!(spec.metadata.name, "echo");
    assert_eq!(spec.auth.env_var, "ECHO_API_KEY");

    let (creds, cred_dir) = setup_credentials("echo-creds", "echo", "test-key-echo");
    let engine = PerformanceEngine::new(creds);

    let coda = engine
        .perform("default", "what is CQRS?", &spec, orch_core::contracts::FormationType::Solo)
        .await
        .unwrap();

    assert_eq!(coda.formation, FormationType::Solo);
    assert_eq!(coda.sections.len(), 1);
    assert!(coda.sections[0].success);
    assert!(
        coda.summary.contains("Echo: what is CQRS?"),
        "prompt not echoed: {}",
        coda.summary
    );
    assert!(coda.total_duration_ms > 0);
    assert!(coda.performance_id.starts_with("perf-"));

    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&cred_dir);
}

#[tokio::test]
async fn pipeline_failing_provider_captures_error() {
    let dir = temp_dir("fail");
    let binary = create_mock_binary(
        &dir,
        "failing-provider",
        "#!/bin/sh\necho 'rate limit exceeded' >&2\nexit 1\n",
    );

    let spec = load_spec("failing-provider.yaml", &binary);
    let (creds, cred_dir) = setup_credentials("fail-creds", "failing", "key");
    let engine = PerformanceEngine::new(creds);

    let coda = engine.perform("default", "test", &spec, orch_core::contracts::FormationType::Solo).await.unwrap();

    assert_eq!(coda.formation, FormationType::Solo);
    assert!(!coda.sections[0].success);
    assert_eq!(
        coda.sections[0].error.as_deref(),
        Some("rate limit exceeded")
    );

    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&cred_dir);
}

#[tokio::test]
async fn pipeline_credential_injected_as_env_var() {
    let dir = temp_dir("env-check");
    let binary = create_mock_binary(
        &dir,
        "env-check-provider",
        "#!/bin/sh\necho \"injected=$ENVCHECK_API_KEY\"\n",
    );

    let spec = load_spec("env-check-provider.yaml", &binary);
    let (creds, cred_dir) = setup_credentials("env-creds", "env-check", "sk-secret-789");
    let engine = PerformanceEngine::new(creds);

    let coda = engine.perform("default", "test", &spec, orch_core::contracts::FormationType::Solo).await.unwrap();

    assert!(coda.sections[0].success);
    assert!(
        coda.summary.contains("injected=sk-secret-789"),
        "credential not injected: {}",
        coda.summary
    );

    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&cred_dir);
}

// ═══════════════════════════════════════════════════════════
// Spec validation: loaded from YAML matches expected structure
// ═══════════════════════════════════════════════════════════

#[tokio::test]
async fn pipeline_spec_fields_drive_invocation() {
    let dir = temp_dir("spec-fields");
    // Provider that prints all received arguments
    let binary = create_mock_binary(
        &dir,
        "args-provider",
        "#!/bin/sh\necho \"args: $*\"\n",
    );

    let spec = load_spec("echo-provider.yaml", &binary);

    // The spec says prompt_flag = "-p", so the engine should pass: -p "the prompt"
    let (creds, cred_dir) = setup_credentials("spec-creds", "echo", "key");
    let engine = PerformanceEngine::new(creds);

    let coda = engine
        .perform("default", "my prompt here", &spec, orch_core::contracts::FormationType::Solo)
        .await
        .unwrap();

    assert!(
        coda.summary.contains("-p") && coda.summary.contains("my prompt here"),
        "spec fields not used in invocation: {}",
        coda.summary
    );

    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&cred_dir);
}

// ═══════════════════════════════════════════════════════════
// Error: no credential → clear failure
// ═══════════════════════════════════════════════════════════

#[tokio::test]
async fn pipeline_missing_credential_returns_error() {
    let dir = temp_dir("no-cred");
    let binary = create_mock_binary(&dir, "echo-provider", "#!/bin/sh\necho ok\n");
    let spec = load_spec("echo-provider.yaml", &binary);

    let cred_dir = temp_dir("no-cred-store");
    let creds = Arc::new(CredentialStore::open(cred_dir.clone()).unwrap());
    // NOT storing any credential for "echo"

    let engine = PerformanceEngine::new(creds);
    let result = engine.perform("default", "test", &spec, orch_core::contracts::FormationType::Solo).await;

    assert!(result.is_err(), "should fail without credential");

    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&cred_dir);
}

// ═══════════════════════════════════════════════════════════
// Multiple namespaces: same provider, different credentials
// ═══════════════════════════════════════════════════════════

#[tokio::test]
async fn pipeline_namespace_scoped_credentials() {
    let dir = temp_dir("ns-scope");
    let binary = create_mock_binary(
        &dir,
        "env-provider",
        "#!/bin/sh\necho \"key=$ENVCHECK_API_KEY\"\n",
    );

    let spec = load_spec("env-check-provider.yaml", &binary);
    let cred_dir = temp_dir("ns-scope-creds");
    let store = CredentialStore::open(cred_dir.clone()).unwrap();
    store.store("default", "env-check", "default-key").unwrap();
    store.store("secure", "env-check", "secure-key").unwrap();
    let creds = Arc::new(store);
    let engine = PerformanceEngine::new(creds);

    let coda_default = engine.perform("default", "test", &spec, orch_core::contracts::FormationType::Solo).await.unwrap();
    let coda_secure = engine.perform("secure", "test", &spec, orch_core::contracts::FormationType::Solo).await.unwrap();

    assert!(coda_default.summary.contains("key=default-key"));
    assert!(coda_secure.summary.contains("key=secure-key"));

    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&cred_dir);
}
