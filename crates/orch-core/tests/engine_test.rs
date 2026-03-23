use orch_core::contracts::FormationType;
use orch_core::credentials::CredentialStore;
use orch_core::engine::PerformanceEngine;
use orch_core::repertoire::*;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::Arc;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "orch-engine-{}-{}-{}",
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

/// Create a mock provider script that echoes the prompt back.
fn mock_provider(dir: &std::path::Path) -> (PathBuf, ProviderSpec) {
    let binary = dir.join("mock-provider");
    std::fs::write(
        &binary,
        r#"#!/bin/sh
# Mock provider: find the prompt after -p flag and echo it
while [ $# -gt 0 ]; do
    case "$1" in
        -p) shift; echo "Answer to: $1"; exit 0 ;;
    esac
    shift
done
echo "no prompt given"
exit 1
"#,
    )
    .unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();

    let spec = ProviderSpec {
        kind: "Provider".into(),
        version: 1,
        metadata: SpecMetadata {
            name: "mock".into(),
            description: Some("Mock provider for testing".into()),
            display_name: None,
            url: None,
            risk: None,
        },
        detection: ProviderDetection {
            binary: "mock-provider".into(),
            version_cmd: vec![],
            auth_paths: vec![],
        },
        invocation: ProviderInvocation {
            cmd: vec![binary.to_string_lossy().into()],
            prompt_flag: "-p".into(),
            model_flag: None,
            system_prompt_flag: None,
            json_schema_flag: None,
            output_format_flag: vec![],
            extra_flags: vec![],
        },
        auth: ProviderAuth {
            env_var: "MOCK_API_KEY".into(),
            methods: vec!["env".into()],
        },
        install: None,
    };

    (binary, spec)
}

// ═══════════════════════════════════════════════════════════
// Scene 7.3-7.5: Solo performance end-to-end
// ═══════════════════════════════════════════════════════════

#[tokio::test]
async fn solo_performance_returns_coda() {
    let dir = temp_dir("solo");
    let (_, spec) = mock_provider(&dir);

    let cred_dir = temp_dir("solo-creds");
    let store = CredentialStore::open(cred_dir.clone()).unwrap();
    store.store("default", "mock", "fake-key-123").unwrap();

    let engine = PerformanceEngine::new(Arc::new(store));
    let coda = engine
        .perform("default", "what is CQRS?", &spec, orch_core::contracts::FormationType::Solo)
        .await
        .unwrap();

    assert_eq!(coda.formation, FormationType::Solo);
    assert!(
        coda.summary.contains("Answer to: what is CQRS?"),
        "expected prompt echo, got: {}",
        coda.summary
    );
    assert_eq!(coda.sections.len(), 1);
    assert!(coda.sections[0].success);
    assert!(coda.total_duration_ms > 0);

    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&cred_dir);
}

#[tokio::test]
async fn solo_performance_captures_provider_name() {
    let dir = temp_dir("prov-name");
    let (_, spec) = mock_provider(&dir);

    let cred_dir = temp_dir("prov-name-creds");
    let store = CredentialStore::open(cred_dir.clone()).unwrap();
    store.store("default", "mock", "key").unwrap();

    let engine = PerformanceEngine::new(Arc::new(store));
    let coda = engine.perform("default", "test", &spec, orch_core::contracts::FormationType::Solo).await.unwrap();

    assert_eq!(coda.sections[0].provider, "mock");

    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&cred_dir);
}

#[tokio::test]
async fn solo_performance_injects_credential() {
    let dir = temp_dir("cred-inject");
    // Provider that prints the env var value
    let binary = dir.join("env-provider");
    std::fs::write(
        &binary,
        "#!/bin/sh\necho \"key=$MOCK_API_KEY\"\n",
    )
    .unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();

    let mut spec = mock_provider(&dir).1;
    spec.invocation.cmd = vec![binary.to_string_lossy().into()];

    let cred_dir = temp_dir("cred-inject-creds");
    let store = CredentialStore::open(cred_dir.clone()).unwrap();
    store.store("default", "mock", "sk-secret-456").unwrap();

    let engine = PerformanceEngine::new(Arc::new(store));
    let coda = engine.perform("default", "test", &spec, orch_core::contracts::FormationType::Solo).await.unwrap();

    assert!(
        coda.summary.contains("key=sk-secret-456"),
        "credential not injected: {}",
        coda.summary
    );

    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&cred_dir);
}

// ═══════════════════════════════════════════════════════════
// Error cases
// ═══════════════════════════════════════════════════════════

#[tokio::test]
async fn solo_performance_fails_without_credential() {
    let dir = temp_dir("no-cred");
    let (_, spec) = mock_provider(&dir);

    let cred_dir = temp_dir("no-cred-creds");
    let store = CredentialStore::open(cred_dir.clone()).unwrap();
    // NOT storing any credential for "mock"

    let engine = PerformanceEngine::new(Arc::new(store));
    let result = engine.perform("default", "test", &spec, orch_core::contracts::FormationType::Solo).await;

    assert!(result.is_err());

    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&cred_dir);
}

#[tokio::test]
async fn solo_performance_captures_provider_failure() {
    let dir = temp_dir("fail");
    let binary = dir.join("failing-provider");
    std::fs::write(
        &binary,
        "#!/bin/sh\necho 'rate limit exceeded' >&2\nexit 1\n",
    )
    .unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();

    let mut spec = mock_provider(&dir).1;
    spec.invocation.cmd = vec![binary.to_string_lossy().into()];

    let cred_dir = temp_dir("fail-creds");
    let store = CredentialStore::open(cred_dir.clone()).unwrap();
    store.store("default", "mock", "key").unwrap();

    let engine = PerformanceEngine::new(Arc::new(store));
    let coda = engine.perform("default", "test", &spec, orch_core::contracts::FormationType::Solo).await.unwrap();

    assert!(!coda.sections[0].success);
    assert_eq!(coda.sections[0].error.as_deref(), Some("rate limit exceeded"));

    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&cred_dir);
}

// ═══════════════════════════════════════════════════════════
// Scene 7.7: Performance ID generation
// ═══════════════════════════════════════════════════════════

#[tokio::test]
async fn solo_performance_generates_unique_ids() {
    let dir = temp_dir("ids");
    let (_, spec) = mock_provider(&dir);

    let cred_dir = temp_dir("ids-creds");
    let store = CredentialStore::open(cred_dir.clone()).unwrap();
    store.store("default", "mock", "key").unwrap();

    let engine = PerformanceEngine::new(Arc::new(store));
    let coda1 = engine.perform("default", "test1", &spec, orch_core::contracts::FormationType::Solo).await.unwrap();
    let coda2 = engine.perform("default", "test2", &spec, orch_core::contracts::FormationType::Solo).await.unwrap();

    assert_ne!(coda1.performance_id, coda2.performance_id);
    assert!(coda1.performance_id.starts_with("perf-"));

    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&cred_dir);
}
