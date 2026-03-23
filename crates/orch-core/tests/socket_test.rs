use http_body_util::{BodyExt, Empty};
use hyper::body::Bytes;
use hyper_util::rt::TokioIo;
use serde_json::Value;
use std::path::PathBuf;
use tokio::net::UnixStream;

fn temp_socket_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("orch-test-{}-{}.sock", std::process::id(), name))
}

/// Scene 1.2 — Health endpoint accessible via Unix socket.
///
/// RED: The daemon must serve HTTP over a Unix socket, not TCP.
/// This is the fundamental IPC mechanism for orchestratord.
#[tokio::test]
async fn health_via_unix_socket() {
    let path = temp_socket_path("health");
    let _ = std::fs::remove_file(&path);

    let app = orch_core::server::app_stateless();
    let server = tokio::spawn({
        let p = path.clone();
        async move {
            orch_core::server::serve_on_socket(&p, app, std::future::pending())
                .await
                .unwrap();
        }
    });

    // Poll until the socket file appears (server is ready)
    for _ in 0..100 {
        if path.exists() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert!(path.exists(), "socket file was not created");

    let stream = UnixStream::connect(&path).await.unwrap();
    let io = TokioIo::new(stream);
    let (mut sender, conn) = hyper::client::conn::http1::handshake(io).await.unwrap();
    tokio::spawn(conn);

    let req = hyper::Request::builder()
        .uri("/v1/system/health")
        .body(Empty::<Bytes>::new())
        .unwrap();

    let resp = sender.send_request(req).await.unwrap();
    assert_eq!(resp.status(), 200);

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["status"], "ok");

    server.abort();
    let _ = std::fs::remove_file(&path);
}

/// Scene 1.2 — Version endpoint accessible via Unix socket.
#[tokio::test]
async fn version_via_unix_socket() {
    let path = temp_socket_path("version");
    let _ = std::fs::remove_file(&path);

    let app = orch_core::server::app_stateless();
    let server = tokio::spawn({
        let p = path.clone();
        async move {
            orch_core::server::serve_on_socket(&p, app, std::future::pending())
                .await
                .unwrap();
        }
    });

    for _ in 0..100 {
        if path.exists() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert!(path.exists(), "socket file was not created");

    let stream = UnixStream::connect(&path).await.unwrap();
    let io = TokioIo::new(stream);
    let (mut sender, conn) = hyper::client::conn::http1::handshake(io).await.unwrap();
    tokio::spawn(conn);

    let req = hyper::Request::builder()
        .uri("/v1/system/version")
        .body(Empty::<Bytes>::new())
        .unwrap();

    let resp = sender.send_request(req).await.unwrap();
    assert_eq!(resp.status(), 200);

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert!(json["version"].is_string());
    assert!(!json["version"].as_str().unwrap().is_empty());

    server.abort();
    let _ = std::fs::remove_file(&path);
}

/// Scene 1.2 — Stale socket file is cleaned up before binding.
///
/// If orchestratord crashes, a stale .sock file may remain.
/// The server must remove it and bind successfully.
#[tokio::test]
async fn removes_stale_socket_file() {
    let path = temp_socket_path("stale");

    // Simulate stale socket file left by a crashed daemon
    std::fs::write(&path, "stale").unwrap();
    assert!(path.exists());

    let app = orch_core::server::app_stateless();
    let server = tokio::spawn({
        let p = path.clone();
        async move {
            orch_core::server::serve_on_socket(&p, app, std::future::pending())
                .await
                .unwrap();
        }
    });

    // Poll until we can actually connect (not just file exists)
    for _ in 0..100 {
        if UnixStream::connect(&path).await.is_ok() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }

    let stream = UnixStream::connect(&path).await.unwrap();
    let io = TokioIo::new(stream);
    let (mut sender, conn) = hyper::client::conn::http1::handshake(io).await.unwrap();
    tokio::spawn(conn);

    let req = hyper::Request::builder()
        .uri("/v1/system/health")
        .body(Empty::<Bytes>::new())
        .unwrap();

    let resp = sender.send_request(req).await.unwrap();
    assert_eq!(resp.status(), 200);

    server.abort();
    let _ = std::fs::remove_file(&path);
}
