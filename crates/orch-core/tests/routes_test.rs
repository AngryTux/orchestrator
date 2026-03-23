//! HTTP route coverage — every endpoint gets at least one request test.
//! Ensures routing, deserialization, and response format work end-to-end.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use orch_core::credentials::CredentialStore;
use orch_core::engine::PerformanceEngine;
use orch_core::metrics::MetricsStore;
use orch_core::server::AppState;
use serde_json::{Value, json};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::Arc;
use tower::ServiceExt;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "orch-routes-{}-{}-{}",
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

fn test_app(name: &str) -> (axum::Router, PathBuf) {
    let dir = temp_dir(name);

    // Create mock provider
    let mock = dir.join("mock-provider");
    std::fs::write(&mock, "#!/bin/sh\nwhile [ $# -gt 0 ]; do case \"$1\" in -p) shift; echo \"Response: $1\"; exit 0;; esac; shift; done\n").unwrap();
    std::fs::set_permissions(&mock, std::fs::Permissions::from_mode(0o755)).unwrap();

    let creds = CredentialStore::open(dir.join("creds")).unwrap();
    creds.store("default", "mock", "test-key").unwrap();
    let creds = Arc::new(creds);
    let engine = Arc::new(PerformanceEngine::new(creds.clone()));
    let metrics = Arc::new(MetricsStore::open(&dir.join("metrics.db")).unwrap());

    let mut providers = std::collections::HashMap::new();
    providers.insert(
        "mock".to_string(),
        serde_yaml::from_str(&format!(
            r#"
kind: Provider
version: 1
metadata:
  name: mock
detection:
  binary: mock
invocation:
  cmd: ["{}"]
  prompt_flag: "-p"
auth:
  env_var: MOCK_KEY
  methods: [env]
"#,
            mock.display()
        ))
        .unwrap(),
    );

    let state = AppState {
        credentials: creds,
        engine,
        providers,
        metrics,
    };
    (orch_core::server::app(state), dir)
}

// ─── POST /v1/namespaces/{ns}/performances ───────────────

#[tokio::test]
async fn route_post_performance() {
    let (app, dir) = test_app("post-perf");

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/namespaces/default/performances")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"prompt": "test", "provider": "mock"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["formation"], "solo");
    assert!(json["performance_id"].is_string());
    assert!(json["sections"].is_array());

    let _ = std::fs::remove_dir_all(&dir);
}

// ─── GET /v1/namespaces/{ns}/performances ────────────────

#[tokio::test]
async fn route_list_performances() {
    let (app, dir) = test_app("list-perf");

    // First create a performance
    let app_clone = app.clone();
    let response = app_clone
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/namespaces/default/performances")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"prompt": "test", "provider": "mock"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    // Then list
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/namespaces/default/performances")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert!(!json.as_array().unwrap().is_empty());

    let _ = std::fs::remove_dir_all(&dir);
}

// ─── GET /v1/namespaces/{ns}/performances/{id} ──────────

#[tokio::test]
async fn route_get_performance_by_id() {
    let (app, dir) = test_app("get-perf");

    // Create a performance
    let app_clone = app.clone();
    let response = app_clone
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/namespaces/default/performances")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"prompt": "hello", "provider": "mock"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let coda: Value = serde_json::from_slice(&body).unwrap();
    let perf_id = coda["performance_id"].as_str().unwrap();

    // Get by ID
    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/v1/namespaces/default/performances/{perf_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let detail: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(detail["performance_id"], perf_id);
    assert_eq!(detail["namespace"], "default");

    let _ = std::fs::remove_dir_all(&dir);
}

// ─── GET /v1/metrics ─────────────────────────────────────

#[tokio::test]
async fn route_metrics_summary() {
    let (app, dir) = test_app("metrics");

    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/metrics")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert!(json["total_performances"].is_u64());
    assert!(json["total_tokens_in"].is_u64());

    let _ = std::fs::remove_dir_all(&dir);
}

// ─── Invalid namespace in URL returns 400 ────────────────

#[tokio::test]
async fn route_rejects_invalid_namespace() {
    let (app, dir) = test_app("bad-ns");

    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/namespaces/../../etc/providers")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // axum rejects path traversal at the router level (404) or our validate_ns rejects it (400)
    // Either way, it must NOT be 200
    assert_ne!(
        response.status(),
        StatusCode::OK,
        "path traversal must not succeed"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
