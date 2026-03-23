//! Provider discovery tests — validates REAL providers on the system.
//!
//! These tests scan the repertoire for provider specs, check which
//! binaries are actually installed, and validate they respond correctly.
//!
//! No mock providers. No fake binaries. Real system validation.
//!
//! Tests are non-failing by default: a missing provider is reported
//! but not a test failure (CI machines may not have providers installed).
//! The test FAILS only if a detected provider is broken.

use orch_core::host::find_in_path;
use orch_core::repertoire::ProviderSpec;
use std::path::Path;
use std::process::Command;

const PROJECT_ROOT: &str = env!("CARGO_MANIFEST_DIR");

/// Load all provider specs from the project repertoire.
fn load_repertoire_specs() -> Vec<ProviderSpec> {
    let providers_dir = Path::new(PROJECT_ROOT)
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("repertoire/providers");

    if !providers_dir.exists() {
        return vec![];
    }

    let mut specs = vec![];
    for entry in std::fs::read_dir(&providers_dir).unwrap() {
        let entry = entry.unwrap();
        if entry.path().extension().is_some_and(|e| e == "yaml") {
            let content = std::fs::read_to_string(entry.path()).unwrap();
            if let Ok(spec) = serde_yaml::from_str::<ProviderSpec>(&content) {
                specs.push(spec);
            }
        }
    }
    specs.sort_by(|a, b| a.metadata.name.cmp(&b.metadata.name));
    specs
}

// ═══════════════════════════════════════════════════════════
// Discovery: which providers are installed on this system?
// ═══════════════════════════════════════════════════════════

#[test]
fn discover_real_providers() {
    let specs = load_repertoire_specs();
    assert!(
        !specs.is_empty(),
        "no provider specs found in repertoire/providers/"
    );

    println!("\n  Provider Discovery:");
    println!("  {:-<55}", "");

    let mut found = 0;
    let mut missing = 0;

    for spec in &specs {
        let binary = &spec.detection.binary;
        match find_in_path(binary) {
            Some(path) => {
                println!("  {:<20} FOUND    {}", spec.metadata.name, path.display());
                found += 1;
            }
            None => {
                println!("  {:<20} MISSING  (not in PATH)", spec.metadata.name);
                missing += 1;
            }
        }
    }

    println!("  {:-<55}", "");
    println!("  {} found, {} missing\n", found, missing);

    // This test reports, it doesn't fail on missing providers.
    // It only fails if the repertoire itself is broken.
}

// ═══════════════════════════════════════════════════════════
// Version check: installed providers respond to --version
// ═══════════════════════════════════════════════════════════

#[test]
fn installed_providers_respond_to_version_cmd() {
    let specs = load_repertoire_specs();

    println!("\n  Provider Version Check:");
    println!("  {:-<55}", "");

    let mut tested = 0;
    let mut failures = vec![];

    for spec in &specs {
        // Skip if binary not installed
        if find_in_path(&spec.detection.binary).is_none() {
            continue;
        }

        // Skip if no version_cmd defined
        if spec.detection.version_cmd.is_empty() {
            println!(
                "  {:<20} SKIP     (no version_cmd in spec)",
                spec.metadata.name
            );
            continue;
        }

        let cmd = &spec.detection.version_cmd;
        let result = Command::new(&cmd[0])
            .args(&cmd[1..])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output();

        match result {
            Ok(output) if output.status.success() => {
                let version = String::from_utf8_lossy(&output.stdout);
                let version = version.lines().next().unwrap_or("(empty)").trim();
                println!("  {:<20} OK       {}", spec.metadata.name, version);
                tested += 1;
            }
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                println!(
                    "  {:<20} FAIL     exit={}, stderr={}",
                    spec.metadata.name,
                    output.status.code().unwrap_or(-1),
                    stderr.lines().next().unwrap_or("(empty)").trim()
                );
                failures.push(spec.metadata.name.clone());
            }
            Err(e) => {
                println!("  {:<20} ERROR    {}", spec.metadata.name, e);
                failures.push(spec.metadata.name.clone());
            }
        }
    }

    println!("  {:-<55}", "");
    println!(
        "  {} tested, {} failed\n",
        tested + failures.len(),
        failures.len()
    );

    // A provider that IS installed but fails --version is a real problem
    assert!(
        failures.is_empty(),
        "installed providers failed version check: {:?}",
        failures
    );
}

// ═══════════════════════════════════════════════════════════
// Spec consistency: all required fields present
// ═══════════════════════════════════════════════════════════

#[test]
fn repertoire_specs_have_required_fields() {
    let specs = load_repertoire_specs();

    for spec in &specs {
        assert!(!spec.metadata.name.is_empty(), "spec missing name");
        assert!(
            !spec.detection.binary.is_empty(),
            "spec {} missing detection.binary",
            spec.metadata.name
        );
        assert!(
            !spec.invocation.cmd.is_empty(),
            "spec {} missing invocation.cmd",
            spec.metadata.name
        );
        assert!(
            !spec.invocation.prompt_flag.is_empty(),
            "spec {} missing invocation.prompt_flag",
            spec.metadata.name
        );
        assert!(
            !spec.auth.env_var.is_empty(),
            "spec {} missing auth.env_var",
            spec.metadata.name
        );
        assert!(
            !spec.auth.methods.is_empty(),
            "spec {} missing auth.methods",
            spec.metadata.name
        );
        assert_eq!(
            spec.kind, "Provider",
            "spec {} has wrong kind: {}",
            spec.metadata.name, spec.kind
        );
        assert!(
            spec.version > 0,
            "spec {} has version 0",
            spec.metadata.name
        );
    }
}

// ═══════════════════════════════════════════════════════════
// CLI spawn: if provider binary exists and is authenticated
// (e.g., Claude Code session), do a real ping-pong.
//
// No API key needed — uses the CLI's own auth (session, config).
// Uses isolation::spawn directly, bypassing credential store.
//
// Opt-in: set ORCH_TEST_REAL_PROVIDERS=1 to enable.
// ═══════════════════════════════════════════════════════════

#[tokio::test]
async fn real_cli_ping_pong() {
    if std::env::var("ORCH_TEST_REAL_PROVIDERS").is_err() {
        println!("\n  Skipping real CLI test (set ORCH_TEST_REAL_PROVIDERS=1 to enable)\n");
        return;
    }

    let specs = load_repertoire_specs();

    println!("\n  Real CLI Ping-Pong (no API key needed — uses CLI auth):");
    println!("  {:-<60}", "");

    let mut tested = 0;
    let mut failures = vec![];

    for spec in &specs {
        if find_in_path(&spec.detection.binary).is_none() {
            println!("  {:<20} SKIP     (not installed)", spec.metadata.name);
            continue;
        }

        // Build invocation from spec — same logic the engine uses
        let binary = spec.invocation.cmd[0].clone();
        let mut args: Vec<String> = spec.invocation.cmd[1..].to_vec();
        args.push(spec.invocation.prompt_flag.clone());
        args.push("Respond with exactly one word: pong".into());
        args.extend(spec.invocation.extra_flags.clone());

        // Spawn directly (no clean env) — the goal is testing
        // the CLI works, not testing isolation. CLIs like Claude Code
        // need their full auth environment (session tokens, node paths, etc).
        let result = Command::new(&binary)
            .args(&args)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output();

        match result {
            Ok(output) if output.status.success() => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let has_pong = stdout.to_lowercase().contains("pong");
                if has_pong {
                    println!(
                        "  {:<20} PASS     ({} chars)",
                        spec.metadata.name,
                        stdout.trim().len()
                    );
                } else {
                    println!(
                        "  {:<20} WARN     responded but no 'pong': {:?}",
                        spec.metadata.name,
                        &stdout[..stdout.len().min(80)]
                    );
                    // Not a failure — LLM may not follow instructions exactly
                }
                tested += 1;
            }
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                println!(
                    "  {:<20} FAIL     exit={}, stderr={:?}",
                    spec.metadata.name,
                    output.status.code().unwrap_or(-1),
                    stderr.lines().next().unwrap_or("(empty)")
                );
                failures.push(spec.metadata.name.clone());
            }
            Err(e) => {
                println!("  {:<20} ERROR    {}", spec.metadata.name, e);
                failures.push(spec.metadata.name.clone());
            }
        }
    }

    println!("  {:-<60}", "");
    println!(
        "  {} tested, {} passed, {} failed\n",
        tested + failures.len(),
        tested,
        failures.len()
    );

    assert!(
        failures.is_empty(),
        "providers failed real CLI ping-pong: {:?}",
        failures
    );
}
