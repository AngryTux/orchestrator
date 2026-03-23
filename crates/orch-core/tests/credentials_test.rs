use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use orch_core::credentials::CredentialStore;
use orch_core::engine::PerformanceEngine;
use orch_core::metrics::MetricsStore;
use orch_core::namespace::NamespaceManager;
use orch_core::server::AppState;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Arc;
use tower::ServiceExt;

fn temp_store(name: &str) -> (CredentialStore, PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "orch-cred-{}-{}-{}",
        std::process::id(),
        name,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let store = CredentialStore::open(dir.clone()).unwrap();
    (store, dir)
}

// ---- Scene 5.1: Encrypt + Scene 5.3: Decrypt ----

#[test]
fn encrypt_decrypt_roundtrip() {
    let (store, dir) = temp_store("roundtrip");
    store
        .store("default", "claude", "sk-ant-secret-key-123")
        .unwrap();
    let decrypted = store.get("default", "claude").unwrap();
    assert_eq!(decrypted, "sk-ant-secret-key-123");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn each_encrypt_produces_different_ciphertext() {
    let (store, dir) = temp_store("nonce");
    store.store("default", "provider-a", "same-key").unwrap();
    store.store("default", "provider-b", "same-key").unwrap();

    // Read raw encrypted files — they should differ (random nonce)
    let a =
        std::fs::read_to_string(dir.join("namespaces/default/credentials/provider-a.enc")).unwrap();
    let b =
        std::fs::read_to_string(dir.join("namespaces/default/credentials/provider-b.enc")).unwrap();
    assert_ne!(a, b, "same plaintext must produce different ciphertext");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn decrypt_with_different_store_fails() {
    let (store1, dir1) = temp_store("key1");
    store1.store("default", "claude", "my-secret").unwrap();

    // Open a NEW store with a DIFFERENT master key
    let (store2, dir2) = temp_store("key2");
    // Copy the encrypted file to store2's directory
    let src = dir1.join("namespaces/default/credentials/claude.enc");
    let dst_dir = dir2.join("namespaces/default/credentials");
    std::fs::create_dir_all(&dst_dir).unwrap();
    std::fs::copy(&src, dst_dir.join("claude.enc")).unwrap();

    let result = store2.get("default", "claude");
    assert!(result.is_err(), "decryption with wrong key must fail");
    let _ = std::fs::remove_dir_all(&dir1);
    let _ = std::fs::remove_dir_all(&dir2);
}

// ---- Scene 5.1: Master key management ----

#[test]
fn generates_master_key_on_first_open() {
    let dir = std::env::temp_dir().join(format!("orch-cred-gen-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);

    let _store = CredentialStore::open(dir.clone()).unwrap();
    assert!(dir.join(".master_key").exists());

    // Key file should be 32 bytes
    let key = std::fs::read(dir.join(".master_key")).unwrap();
    assert_eq!(key.len(), 32);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn reuses_existing_master_key() {
    let dir = std::env::temp_dir().join(format!("orch-cred-reuse-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);

    // First open — generates key
    let store1 = CredentialStore::open(dir.clone()).unwrap();
    store1.store("default", "claude", "my-secret").unwrap();

    // Second open — reuses key, can decrypt
    let store2 = CredentialStore::open(dir.clone()).unwrap();
    let decrypted = store2.get("default", "claude").unwrap();
    assert_eq!(decrypted, "my-secret");
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- Scene 5.4: Store/Get/Delete/List ----

#[test]
fn get_nonexistent_credential_fails() {
    let (store, dir) = temp_store("noexist");
    let result = store.get("default", "nonexistent");
    assert!(result.is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn delete_credential() {
    let (store, dir) = temp_store("delete");
    store.store("default", "claude", "key").unwrap();
    assert!(store.get("default", "claude").is_ok());

    store.delete("default", "claude").unwrap();
    assert!(store.get("default", "claude").is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn list_credentials_in_namespace() {
    let (store, dir) = temp_store("list");
    store.store("default", "claude", "key1").unwrap();
    store.store("default", "codex", "key2").unwrap();
    store.store("default", "gemini", "key3").unwrap();

    let providers = store.list("default").unwrap();
    assert_eq!(providers, vec!["claude", "codex", "gemini"]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn list_empty_namespace_returns_empty() {
    let (store, dir) = temp_store("empty");
    let providers = store.list("nonexistent").unwrap();
    assert!(providers.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- Scene 5.7: Credentials scoped by namespace ----

#[test]
fn credentials_scoped_by_namespace() {
    let (store, dir) = temp_store("scoped");
    store.store("default", "claude", "default-key").unwrap();
    store.store("secure", "claude", "secure-key").unwrap();

    assert_eq!(store.get("default", "claude").unwrap(), "default-key");
    assert_eq!(store.get("secure", "claude").unwrap(), "secure-key");

    // Deleting from one namespace doesn't affect another
    store.delete("default", "claude").unwrap();
    assert!(store.get("default", "claude").is_err());
    assert_eq!(store.get("secure", "claude").unwrap(), "secure-key");
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- Path traversal prevention ----

#[test]
fn rejects_path_traversal_in_namespace() {
    let (store, dir) = temp_store("traversal-ns");
    assert!(store.store("../../etc", "claude", "key").is_err());
    assert!(store.get("../secret", "claude").is_err());
    assert!(store.list("../../tmp").is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn rejects_path_traversal_in_provider() {
    let (store, dir) = temp_store("traversal-prov");
    assert!(
        store
            .store("default", "../../../etc/passwd", "key")
            .is_err()
    );
    assert!(store.get("default", "../../shadow").is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn rejects_special_chars_in_names() {
    let (store, dir) = temp_store("special");
    assert!(store.store("def ault", "claude", "key").is_err());
    assert!(store.store("default", "cla/ude", "key").is_err());
    assert!(store.store("default", "claude\0bad", "key").is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- Scene 5.4: Inject as env var ----

#[test]
fn credential_to_env_pair() {
    let (store, dir) = temp_store("env");
    store.store("default", "claude", "sk-ant-key").unwrap();

    let (var, val) = store
        .env_pair("default", "claude", "ANTHROPIC_API_KEY")
        .unwrap();
    assert_eq!(var, "ANTHROPIC_API_KEY");
    assert_eq!(val, "sk-ant-key");
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- Scene 5.5: API endpoints ----

fn test_app(name: &str) -> (axum::Router, PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "orch-api-{}-{}-{}",
        std::process::id(),
        name,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let store = CredentialStore::open(dir.clone()).unwrap();
    let credentials = Arc::new(store);
    let engine = Arc::new(PerformanceEngine::new(credentials.clone()));
    let metrics = Arc::new(MetricsStore::open(&dir.join("metrics.db")).unwrap());
    let state = AppState {
        credentials,
        engine,
        providers: std::collections::HashMap::new(),
        metrics,
        namespaces: Arc::new(NamespaceManager::new(dir.clone())),
    };
    (orch_core::server::app(state), dir)
}

#[tokio::test]
async fn api_add_provider() {
    let (app, dir) = test_app("add");

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/namespaces/default/providers")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"name": "claude", "key": "sk-ant-secret"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["status"], "ok");
    assert_eq!(json["provider"], "claude");
    assert_eq!(json["namespace"], "default");
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn api_list_providers() {
    let (app, dir) = test_app("list-api");
    let state_dir = dir.clone();

    // Pre-store some credentials
    let store = CredentialStore::open(state_dir).unwrap();
    store.store("default", "claude", "key1").unwrap();
    store.store("default", "codex", "key2").unwrap();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/namespaces/default/providers")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["namespace"], "default");

    let providers = json["providers"].as_array().unwrap();
    assert!(providers.contains(&json!("claude")));
    assert!(providers.contains(&json!("codex")));
    let _ = std::fs::remove_dir_all(&dir);
}
