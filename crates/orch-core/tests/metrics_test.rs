//! Movement 9 — Metrics: SQLite persistence + query endpoints.

use orch_core::contracts::{CodaContract, FormationType, ResultContract};
use orch_core::metrics::MetricsStore;
use std::path::PathBuf;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "orch-metrics-{}-{}-{}",
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

fn sample_coda(id: &str, formation: FormationType, success: bool) -> CodaContract {
    CodaContract {
        performance_id: id.into(),
        summary: format!("Summary for {id}"),
        formation,
        harmony: success,
        sections: vec![ResultContract {
            workspace_id: format!("ws-{id}"),
            section_id: "sec-001".into(),
            provider: "mock".into(),
            model: "test".into(),
            output: "output".into(),
            tokens_in: 100,
            tokens_out: 200,
            cost_usd: 0.01,
            duration_ms: 500,
            success,
            error: if success {
                None
            } else {
                Some("failed".into())
            },
        }],
        total_tokens_in: 100,
        total_tokens_out: 200,
        total_cost_usd: 0.01,
        total_duration_ms: 500,
    }
}

// ═══════════════════════════════════════════════════════════
// Scene 9.1: Schema creation
// ═══════════════════════════════════════════════════════════

#[test]
fn opens_and_creates_schema() {
    let dir = temp_dir("schema");
    let _store = MetricsStore::open(&dir.join("test.db")).unwrap();
    assert!(dir.join("test.db").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

// ═══════════════════════════════════════════════════════════
// Scene 9.2: Save and retrieve performances
// ═══════════════════════════════════════════════════════════

#[test]
fn save_and_get_performance() {
    let dir = temp_dir("save-get");
    let store = MetricsStore::open(&dir.join("test.db")).unwrap();
    let coda = sample_coda("perf-001", FormationType::Solo, true);

    store.save("default", "what is CQRS?", &coda).unwrap();

    let retrieved = store.get("perf-001").unwrap();
    assert!(retrieved.is_some());
    let detail = retrieved.unwrap();
    assert_eq!(detail.performance_id, "perf-001");
    assert_eq!(detail.namespace, "default");
    assert_eq!(detail.prompt, "what is CQRS?");
    assert_eq!(detail.formation, "solo");
    assert!(detail.harmony);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn get_nonexistent_returns_none() {
    let dir = temp_dir("noexist");
    let store = MetricsStore::open(&dir.join("test.db")).unwrap();

    assert!(store.get("nonexistent").unwrap().is_none());

    let _ = std::fs::remove_dir_all(&dir);
}

// ═══════════════════════════════════════════════════════════
// Scene 9.2: List performances by namespace
// ═══════════════════════════════════════════════════════════

#[test]
fn list_performances_by_namespace() {
    let dir = temp_dir("list");
    let store = MetricsStore::open(&dir.join("test.db")).unwrap();

    store
        .save("default", "prompt 1", &sample_coda("p1", FormationType::Solo, true))
        .unwrap();
    store
        .save("default", "prompt 2", &sample_coda("p2", FormationType::Duet, true))
        .unwrap();
    store
        .save("secure", "prompt 3", &sample_coda("p3", FormationType::Solo, false))
        .unwrap();

    let default_list = store.list("default").unwrap();
    assert_eq!(default_list.len(), 2);

    let secure_list = store.list("secure").unwrap();
    assert_eq!(secure_list.len(), 1);
    assert_eq!(secure_list[0].performance_id, "p3");
    assert!(!secure_list[0].harmony);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn list_empty_namespace_returns_empty() {
    let dir = temp_dir("empty-ns");
    let store = MetricsStore::open(&dir.join("test.db")).unwrap();

    let list = store.list("nonexistent").unwrap();
    assert!(list.is_empty());

    let _ = std::fs::remove_dir_all(&dir);
}

// ═══════════════════════════════════════════════════════════
// Scene 9.3: Metrics summary
// ═══════════════════════════════════════════════════════════

#[test]
fn metrics_summary() {
    let dir = temp_dir("summary");
    let store = MetricsStore::open(&dir.join("test.db")).unwrap();

    store
        .save("default", "p1", &sample_coda("p1", FormationType::Solo, true))
        .unwrap();
    store
        .save("default", "p2", &sample_coda("p2", FormationType::Duet, true))
        .unwrap();
    store
        .save("default", "p3", &sample_coda("p3", FormationType::Solo, false))
        .unwrap();

    let summary = store.summary().unwrap();
    assert_eq!(summary.total_performances, 3);
    assert_eq!(summary.total_tokens_in, 300);
    assert_eq!(summary.total_tokens_out, 600);

    let _ = std::fs::remove_dir_all(&dir);
}

// ═══════════════════════════════════════════════════════════
// Persistence: survives reopen
// ═══════════════════════════════════════════════════════════

#[test]
fn data_persists_across_reopens() {
    let dir = temp_dir("persist");
    let db_path = dir.join("test.db");

    {
        let store = MetricsStore::open(&db_path).unwrap();
        store
            .save("default", "test", &sample_coda("p1", FormationType::Solo, true))
            .unwrap();
    } // store dropped, connection closed

    {
        let store = MetricsStore::open(&db_path).unwrap();
        let list = store.list("default").unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].performance_id, "p1");
    }

    let _ = std::fs::remove_dir_all(&dir);
}
