use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;

/// Scene 1.3 — GET /v1/system/health returns {"status": "ok"}
///
/// RED: This test defines the contract before any implementation exists.
/// The health endpoint is the first sign of life from the daemon.
#[tokio::test]
async fn health_endpoint_returns_ok() {
    let app = orch_core::server::app_stateless();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/system/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["status"], "ok");
}

/// Scene 1.3 — GET /v1/system/version returns version info
#[tokio::test]
async fn version_endpoint_returns_version() {
    let app = orch_core::server::app_stateless();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/system/version")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap();

    assert!(json["version"].is_string());
    assert!(!json["version"].as_str().unwrap().is_empty());
}

/// Unknown routes return 404
#[tokio::test]
async fn unknown_route_returns_not_found() {
    let app = orch_core::server::app_stateless();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/nonexistent")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
