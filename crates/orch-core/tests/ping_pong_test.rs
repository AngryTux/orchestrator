//! Ping-pong tests: the simplest possible end-to-end validation.
//!
//! For every provider spec in fixtures/, creates a mock binary that
//! responds to "ping" with "pong", runs a Solo performance, and
//! verifies the response. If ANY spec fails, the pipeline is broken.
//!
//! This is the "smoke detector" of the integration test suite.

use orch_core::contracts::FormationType;
use orch_core::credentials::CredentialStore;
use orch_core::engine::PerformanceEngine;
use orch_core::repertoire::ProviderSpec;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

const FIXTURES_DIR: &str = env!("CARGO_MANIFEST_DIR");

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "orch-ping-{}-{}-{}",
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

/// Universal mock: for ANY provider spec, responds to "ping" with "pong".
/// Handles prompt_flag dynamically (reads -p, --prompt, etc).
fn create_pong_binary(dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(
        &path,
        r#"#!/bin/sh
# Universal pong responder — works with any prompt_flag
for arg in "$@"; do
    if [ "$prev" = "PROMPT_FLAG" ]; then
        echo "pong"
        exit 0
    fi
    case "$arg" in
        -*) prev="PROMPT_FLAG" ;;
        *)  prev="" ;;
    esac
done
echo "pong"
exit 0
"#,
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

/// Discover all provider spec fixtures.
fn discover_specs() -> Vec<(String, PathBuf)> {
    let fixtures = Path::new(FIXTURES_DIR).join("tests/fixtures/providers");
    let mut specs = vec![];
    for entry in std::fs::read_dir(&fixtures).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "yaml") {
            let name = path
                .file_stem()
                .unwrap()
                .to_string_lossy()
                .to_string();
            specs.push((name, path));
        }
    }
    specs.sort_by(|a, b| a.0.cmp(&b.0));
    specs
}

fn load_spec(yaml_path: &Path, binary_path: &Path) -> ProviderSpec {
    let yaml = std::fs::read_to_string(yaml_path).unwrap();
    let yaml = yaml.replace("__BINARY_PATH__", &binary_path.to_string_lossy());
    serde_yaml::from_str(&yaml).unwrap()
}

// ═══════════════════════════════════════════════════════════
// Ping-pong: every spec gets a "ping", must return "pong"
// ═══════════════════════════════════════════════════════════

#[tokio::test]
async fn ping_pong_all_provider_specs() {
    let specs = discover_specs();
    assert!(!specs.is_empty(), "no provider specs found in fixtures/");

    let dir = temp_dir("ping-pong");
    let cred_dir = temp_dir("ping-pong-creds");
    let store = CredentialStore::open(cred_dir.clone()).unwrap();

    let mut results: Vec<(String, Result<(), String>)> = vec![];

    for (name, yaml_path) in &specs {
        // Skip the failing provider — it's designed to fail
        if name == "failing-provider" {
            continue;
        }

        let binary = create_pong_binary(&dir, name);
        let spec = load_spec(yaml_path, &binary);

        // Store credential for this provider
        store
            .store("default", &spec.metadata.name, "ping-pong-key")
            .unwrap();

        let engine = PerformanceEngine::new(Arc::new(
            CredentialStore::open(cred_dir.clone()).unwrap(),
        ));

        let result = engine.perform("default", "ping", &spec, orch_core::contracts::FormationType::Solo).await;

        match result {
            Ok(coda) => {
                if coda.sections[0].success && coda.summary.contains("pong") {
                    results.push((name.clone(), Ok(())));
                } else {
                    results.push((
                        name.clone(),
                        Err(format!(
                            "success={}, summary={:?}",
                            coda.sections[0].success, coda.summary
                        )),
                    ));
                }
            }
            Err(e) => {
                results.push((name.clone(), Err(format!("engine error: {e}"))));
            }
        }
    }

    // Report
    println!("\n  Ping-Pong Results:");
    println!("  {:-<50}", "");
    let mut failures = 0;
    for (name, result) in &results {
        match result {
            Ok(()) => println!("  {name:<35} PASS"),
            Err(e) => {
                println!("  {name:<35} FAIL: {e}");
                failures += 1;
            }
        }
    }
    println!("  {:-<50}", "");
    println!(
        "  {} specs tested, {} passed, {} failed\n",
        results.len(),
        results.len() - failures,
        failures
    );

    assert_eq!(failures, 0, "{failures} provider specs failed ping-pong");

    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&cred_dir);
}

// ═══════════════════════════════════════════════════════════
// All specs parse correctly
// ═══════════════════════════════════════════════════════════

#[test]
fn all_fixture_specs_are_valid_yaml() {
    let specs = discover_specs();
    assert!(!specs.is_empty());

    for (name, yaml_path) in &specs {
        let yaml = std::fs::read_to_string(yaml_path).unwrap();
        let yaml = yaml.replace("__BINARY_PATH__", "/usr/bin/echo");
        let result = serde_yaml::from_str::<ProviderSpec>(&yaml);
        assert!(
            result.is_ok(),
            "fixture {name}.yaml failed to parse: {}",
            result.unwrap_err()
        );
    }
}

// ═══════════════════════════════════════════════════════════
// Formation is always Solo
// ═══════════════════════════════════════════════════════════

#[tokio::test]
async fn all_specs_produce_solo_formation() {
    let specs = discover_specs();
    let dir = temp_dir("formation");
    let cred_dir = temp_dir("formation-creds");
    let store = CredentialStore::open(cred_dir.clone()).unwrap();

    for (name, yaml_path) in &specs {
        if name == "failing-provider" {
            continue;
        }

        let binary = create_pong_binary(&dir, name);
        let spec = load_spec(yaml_path, &binary);
        store
            .store("default", &spec.metadata.name, "key")
            .unwrap();

        let engine = PerformanceEngine::new(Arc::new(
            CredentialStore::open(cred_dir.clone()).unwrap(),
        ));
        let coda = engine.perform("default", "test", &spec, orch_core::contracts::FormationType::Solo).await.unwrap();

        assert_eq!(
            coda.formation,
            FormationType::Solo,
            "spec {name} produced {:?} instead of Solo",
            coda.formation
        );
        assert_eq!(coda.sections.len(), 1, "spec {name} has {} sections", coda.sections.len());
    }

    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&cred_dir);
}
