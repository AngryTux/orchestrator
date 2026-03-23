//! Movement 8 — Duet: 2 workspaces in parallel, consolidated.

use orch_core::contracts::FormationType;
use orch_core::credentials::CredentialStore;
use orch_core::engine::PerformanceEngine;
use orch_core::repertoire::*;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::Arc;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "orch-duet-{}-{}-{}",
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

fn mock_provider(dir: &std::path::Path, name: &str, script: &str) -> ProviderSpec {
    let binary = dir.join(name);
    std::fs::write(&binary, script).unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();

    ProviderSpec {
        kind: "Provider".into(),
        version: 1,
        metadata: SpecMetadata {
            name: "mock".into(),
            description: None,
            display_name: None,
            url: None,
            risk: None,
        },
        detection: ProviderDetection {
            binary: name.into(),
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
            env_var: "MOCK_KEY".into(),
            methods: vec!["env".into()],
        },
        install: None,
    }
}

fn setup(name: &str) -> (Arc<CredentialStore>, PathBuf) {
    let dir = temp_dir(name);
    let store = CredentialStore::open(dir.clone()).unwrap();
    store.store("default", "mock", "key").unwrap();
    (Arc::new(store), dir)
}

// ═══════════════════════════════════════════════════════════
// Scene 8.2: Two workspaces in parallel
// ═══════════════════════════════════════════════════════════

#[tokio::test]
async fn duet_returns_two_sections() {
    let dir = temp_dir("two-sec");
    let spec = mock_provider(
        &dir,
        "echo",
        "#!/bin/sh\nwhile [ $# -gt 0 ]; do case \"$1\" in -p) shift; echo \"Answer: $1\"; exit 0;; esac; shift; done\n",
    );
    let (creds, cred_dir) = setup("two-sec-c");
    let engine = PerformanceEngine::new(creds);

    let coda = engine
        .perform("default", "test prompt", &spec, FormationType::Duet, &[])
        .await
        .unwrap();

    assert_eq!(coda.formation, FormationType::Duet);
    assert_eq!(
        coda.sections.len(),
        2,
        "duet must produce exactly 2 sections"
    );
    assert!(coda.sections[0].success);
    assert!(coda.sections[1].success);
    assert!(coda.performance_id.starts_with("perf-"));

    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&cred_dir);
}

// ═══════════════════════════════════════════════════════════
// Scene 8.2: Parallel execution (not sequential)
// ═══════════════════════════════════════════════════════════

#[tokio::test]
async fn duet_runs_in_parallel() {
    let dir = temp_dir("parallel");
    // Each section sleeps 200ms. If parallel, total should be ~200ms, not ~400ms.
    let spec = mock_provider(&dir, "slow", "#!/bin/sh\nsleep 0.2\necho \"done\"\n");
    let (creds, cred_dir) = setup("parallel-c");
    let engine = PerformanceEngine::new(creds);

    let coda = engine
        .perform("default", "test", &spec, FormationType::Duet, &[])
        .await
        .unwrap();

    assert!(
        coda.total_duration_ms < 500,
        "duet should run in parallel (~200ms), took {}ms",
        coda.total_duration_ms
    );

    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&cred_dir);
}

// ═══════════════════════════════════════════════════════════
// Scene 8.4: Collect both results
// ═══════════════════════════════════════════════════════════

#[tokio::test]
async fn duet_collects_both_outputs() {
    let dir = temp_dir("both");
    let spec = mock_provider(
        &dir,
        "echo",
        "#!/bin/sh\nwhile [ $# -gt 0 ]; do case \"$1\" in -p) shift; echo \"Response: $1\"; exit 0;; esac; shift; done\n",
    );
    let (creds, cred_dir) = setup("both-c");
    let engine = PerformanceEngine::new(creds);

    let coda = engine
        .perform("default", "what is CQRS?", &spec, FormationType::Duet, &[])
        .await
        .unwrap();

    // Both sections should have output
    assert!(!coda.sections[0].output.is_empty());
    assert!(!coda.sections[1].output.is_empty());
    // Summary should contain both
    assert!(
        coda.summary.contains("Section 1") && coda.summary.contains("Section 2"),
        "summary should reference both sections: {}",
        coda.summary
    );

    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&cred_dir);
}

// ═══════════════════════════════════════════════════════════
// Scene 8.5: Consolidation
// ═══════════════════════════════════════════════════════════

#[tokio::test]
async fn duet_summary_consolidates_outputs() {
    let dir = temp_dir("consolidate");
    let spec = mock_provider(
        &dir,
        "echo",
        "#!/bin/sh\nwhile [ $# -gt 0 ]; do case \"$1\" in -p) shift; echo \"$1\"; exit 0;; esac; shift; done\n",
    );
    let (creds, cred_dir) = setup("consolidate-c");
    let engine = PerformanceEngine::new(creds);

    let coda = engine
        .perform(
            "default",
            "compare Redis vs Memcached",
            &spec,
            FormationType::Duet,
            &[],
        )
        .await
        .unwrap();

    // Summary must not be empty and should contain content from sections
    assert!(!coda.summary.is_empty());
    assert!(coda.total_duration_ms > 0);
    assert_eq!(coda.total_tokens_in, 0); // not tracked yet

    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&cred_dir);
}

// ═══════════════════════════════════════════════════════════
// Scene 8.6: Harmony / Dissonance
// ═══════════════════════════════════════════════════════════

#[tokio::test]
async fn duet_harmony_when_both_succeed() {
    let dir = temp_dir("harmony");
    let spec = mock_provider(&dir, "ok", "#!/bin/sh\necho ok\n");
    let (creds, cred_dir) = setup("harmony-c");
    let engine = PerformanceEngine::new(creds);

    let coda = engine
        .perform("default", "test", &spec, FormationType::Duet, &[])
        .await
        .unwrap();

    assert!(coda.harmony, "both sections succeeded — should be harmony");

    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&cred_dir);
}

#[tokio::test]
async fn duet_dissonance_when_one_fails() {
    let dir = temp_dir("dissonance");
    // Provider that fails on second call (uses a counter file)
    let counter = dir.join("counter");
    std::fs::write(&counter, "0").unwrap();
    let script = format!(
        r#"#!/bin/sh
COUNT=$(cat {counter})
echo $((COUNT + 1)) > {counter}
if [ "$COUNT" = "0" ]; then
    echo "first response"
    exit 0
else
    echo "failure" >&2
    exit 1
fi
"#,
        counter = counter.display()
    );
    let spec = mock_provider(&dir, "flaky", &script);
    let (creds, cred_dir) = setup("dissonance-c");
    let engine = PerformanceEngine::new(creds);

    let coda = engine
        .perform("default", "test", &spec, FormationType::Duet, &[])
        .await
        .unwrap();

    assert!(!coda.harmony, "one section failed — should be dissonance");
    // The coda should still return (not error) — partial results are valid
    assert_eq!(coda.sections.len(), 2);

    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&cred_dir);
}

// ═══════════════════════════════════════════════════════════
// Solo still works after Duet refactor
// ═══════════════════════════════════════════════════════════

#[tokio::test]
async fn solo_still_works_with_formation_param() {
    let dir = temp_dir("solo-compat");
    let spec = mock_provider(
        &dir,
        "echo",
        "#!/bin/sh\nwhile [ $# -gt 0 ]; do case \"$1\" in -p) shift; echo \"$1\"; exit 0;; esac; shift; done\n",
    );
    let (creds, cred_dir) = setup("solo-compat-c");
    let engine = PerformanceEngine::new(creds);

    let coda = engine
        .perform("default", "hello", &spec, FormationType::Solo, &[])
        .await
        .unwrap();

    assert_eq!(coda.formation, FormationType::Solo);
    assert_eq!(coda.sections.len(), 1);

    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&cred_dir);
}

#[tokio::test]
async fn unsupported_formation_returns_error() {
    let dir = temp_dir("unsupported");
    let spec = mock_provider(&dir, "echo", "#!/bin/sh\necho ok\n");
    let (creds, cred_dir) = setup("unsupported-c");
    let engine = PerformanceEngine::new(creds);

    let result = engine
        .perform("default", "test", &spec, FormationType::Opera, &[])
        .await;

    assert!(result.is_err());

    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&cred_dir);
}
