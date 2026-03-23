use http_body_util::{BodyExt, Empty};
use hyper::body::Bytes;
use hyper_util::rt::TokioIo;
use serde_json::Value;
use std::path::PathBuf;
use std::time::Duration;
use tokio::net::UnixStream;

fn temp_socket_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("orch-test-{}-{}.sock", std::process::id(), name))
}

/// Scene 1.5 — Server shuts down gracefully when the shutdown signal fires.
///
/// The server must:
/// 1. Respond to requests before shutdown
/// 2. Stop when the signal fires
/// 3. Complete the server task (not hang forever)
#[tokio::test]
async fn server_shuts_down_on_signal() {
    let path = temp_socket_path("shutdown");
    let _ = std::fs::remove_file(&path);

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

    let app = orch_core::server::app_stateless();
    let server = tokio::spawn({
        let p = path.clone();
        async move {
            orch_core::server::serve_on_socket(&p, app, async {
                let _ = shutdown_rx.await;
            })
            .await
            .unwrap();
        }
    });

    // Wait for server to be ready
    for _ in 0..100 {
        if path.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(path.exists(), "socket file was not created");

    // Verify server responds before shutdown
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

    // Fire shutdown signal
    shutdown_tx.send(()).unwrap();

    // Server task must complete within 5 seconds (not hang)
    let result = tokio::time::timeout(Duration::from_secs(5), server).await;
    assert!(result.is_ok(), "server did not shut down within 5 seconds");
    assert!(
        result.unwrap().is_ok(),
        "server task panicked during shutdown"
    );

    let _ = std::fs::remove_file(&path);
}

/// Scene 1.5 — Server stops accepting new connections after shutdown signal.
#[tokio::test]
async fn no_new_connections_after_shutdown() {
    let path = temp_socket_path("no-conn");
    let _ = std::fs::remove_file(&path);

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

    let app = orch_core::server::app_stateless();
    let server = tokio::spawn({
        let p = path.clone();
        async move {
            orch_core::server::serve_on_socket(&p, app, async {
                let _ = shutdown_rx.await;
            })
            .await
            .unwrap();
        }
    });

    for _ in 0..100 {
        if path.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    // Fire shutdown signal
    shutdown_tx.send(()).unwrap();

    // Wait for server to finish
    let _ = tokio::time::timeout(Duration::from_secs(5), server).await;

    // New connection should fail — server is gone
    let result = UnixStream::connect(&path).await;
    assert!(
        result.is_err(),
        "connection should be refused after shutdown"
    );

    let _ = std::fs::remove_file(&path);
}
