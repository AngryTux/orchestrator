use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::credentials::CredentialStore;
use crate::engine::PerformanceEngine;
use crate::host::HostInfo;
use crate::metrics::MetricsStore;
use crate::repertoire::ProviderSpec;

// ─── App State ───────────────────────────────────────────

#[derive(Clone)]
pub struct AppState {
    pub credentials: Arc<CredentialStore>,
    pub engine: Arc<PerformanceEngine>,
    pub providers: std::collections::HashMap<String, ProviderSpec>,
    pub metrics: Arc<MetricsStore>,
}

// ─── System handlers (no state needed) ───────────────────

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
}

#[derive(Serialize)]
struct VersionResponse {
    version: &'static str,
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

async fn version() -> Json<VersionResponse> {
    Json(VersionResponse {
        version: env!("CARGO_PKG_VERSION"),
    })
}

async fn info() -> Result<Json<HostInfo>, StatusCode> {
    HostInfo::detect()
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

// ─── Provider handlers (need state) ─────────────────────

#[derive(Deserialize)]
struct AddProviderRequest {
    name: String,
    key: String,
}

#[derive(Serialize)]
struct AddProviderResponse {
    status: &'static str,
    provider: String,
    namespace: String,
}

#[derive(Serialize)]
struct ProviderListResponse {
    namespace: String,
    providers: Vec<String>,
}

async fn add_provider(
    State(state): State<AppState>,
    Path(ns): Path<String>,
    Json(body): Json<AddProviderRequest>,
) -> Result<Json<AddProviderResponse>, StatusCode> {
    validate_ns(&ns)?;
    state
        .credentials
        .store(&ns, &body.name, &body.key)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(AddProviderResponse {
        status: "ok",
        provider: body.name,
        namespace: ns,
    }))
}

async fn list_providers(
    State(state): State<AppState>,
    Path(ns): Path<String>,
) -> Result<Json<ProviderListResponse>, StatusCode> {
    validate_ns(&ns)?;
    let providers = state
        .credentials
        .list(&ns)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(ProviderListResponse {
        namespace: ns,
        providers,
    }))
}

// ─── Performance handlers ────────────────────────────────

#[derive(Deserialize)]
struct PerformRequest {
    prompt: String,
    #[serde(default)]
    provider: Option<String>,
    #[serde(default)]
    formation: Option<crate::contracts::FormationType>,
}

async fn perform(
    State(state): State<AppState>,
    Path(ns): Path<String>,
    Json(body): Json<PerformRequest>,
) -> Result<Json<crate::contracts::CodaContract>, StatusCode> {
    validate_ns(&ns)?;
    let provider_name = body.provider.as_deref().unwrap_or("claude");
    let spec = state
        .providers
        .get(provider_name)
        .ok_or(StatusCode::BAD_REQUEST)?;

    let formation = body
        .formation
        .unwrap_or(crate::contracts::FormationType::Solo);

    let coda = state
        .engine
        .perform(&ns, &body.prompt, spec, formation)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Persist to metrics store (best-effort — don't fail the request)
    let _ = state.metrics.save(&ns, &body.prompt, &coda);

    Ok(Json(coda))
}

async fn list_performances(
    State(state): State<AppState>,
    Path(ns): Path<String>,
) -> Result<Json<Vec<crate::metrics::PerformanceSummary>>, StatusCode> {
    validate_ns(&ns)?;
    state
        .metrics
        .list(&ns)
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn get_performance(
    State(state): State<AppState>,
    Path((_, id)): Path<(String, String)>,
) -> Result<Json<Option<crate::metrics::PerformanceDetail>>, StatusCode> {
    state
        .metrics
        .get(&id)
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn metrics_summary(
    State(state): State<AppState>,
) -> Result<Json<crate::metrics::MetricsSummary>, StatusCode> {
    state
        .metrics
        .summary()
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

// ─── Router ──────────────────────────────────────────────

/// Creates the application router with shared state.
pub fn app(state: AppState) -> Router {
    Router::new()
        .route("/v1/system/health", get(health))
        .route("/v1/system/version", get(version))
        .route("/v1/system/info", get(info))
        .route(
            "/v1/namespaces/{ns}/providers",
            post(add_provider).get(list_providers),
        )
        .route("/v1/namespaces/{ns}/performances", post(perform).get(list_performances))
        .route("/v1/namespaces/{ns}/performances/{id}", get(get_performance))
        .route("/v1/metrics", get(metrics_summary))
        .with_state(state)
}

/// Validate namespace from URL path parameter.
fn validate_ns(ns: &str) -> Result<(), StatusCode> {
    if ns.is_empty()
        || ns.contains('/')
        || ns.contains("..")
        || !ns.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    Ok(())
}

/// Creates a minimal router without state (for tests that don't need credentials).
pub fn app_stateless() -> Router {
    Router::new()
        .route("/v1/system/health", get(health))
        .route("/v1/system/version", get(version))
        .route("/v1/system/info", get(info))
}

/// Serves the application on a pre-existing Unix listener.
pub async fn serve(
    listener: tokio::net::UnixListener,
    app: Router,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> anyhow::Result<()> {
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown)
        .await?;
    Ok(())
}

/// Convenience: bind a new Unix socket at `path` and serve.
pub async fn serve_on_socket(
    path: &std::path::Path,
    app: Router,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> anyhow::Result<()> {
    if path.exists() {
        std::fs::remove_file(path)?;
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let listener = tokio::net::UnixListener::bind(path)?;
    serve(listener, app, shutdown).await
}
