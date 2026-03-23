use anyhow::{Context, Result};
use sd_notify::NotifyState;
use std::os::unix::io::FromRawFd;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let listener = match sd_notify::listen_fds().ok().and_then(|mut fds| fds.next()) {
        Some(fd) => {
            tracing::info!("socket-activated by systemd (fd={})", fd);
            let std_listener = unsafe { std::os::unix::net::UnixListener::from_raw_fd(fd) };
            std_listener.set_nonblocking(true)?;
            tokio::net::UnixListener::from_std(std_listener)?
        }
        None => {
            let runtime_dir = std::env::var("XDG_RUNTIME_DIR")
                .context("XDG_RUNTIME_DIR not set — systemd required")?;
            let socket_path = std::path::PathBuf::from(runtime_dir)
                .join("orchestrator")
                .join("orchestrator.sock");

            if socket_path.exists() {
                std::fs::remove_file(&socket_path)?;
            }
            if let Some(parent) = socket_path.parent() {
                std::fs::create_dir_all(parent)?;
            }

            let listener = tokio::net::UnixListener::bind(&socket_path)?;
            tracing::info!("listening on {}", socket_path.display());
            listener
        }
    };

    let data_dir = std::env::var("XDG_DATA_HOME").unwrap_or_else(|_| {
        let home = std::env::var("HOME").context("neither XDG_DATA_HOME nor HOME is set");
        format!(
            "{}/.local/share",
            home.unwrap_or_else(|e| {
                tracing::error!("{e}");
                std::process::exit(1);
            })
        )
    });
    let data_dir = std::path::PathBuf::from(data_dir).join("orchestrator");

    let credentials = std::sync::Arc::new(
        orch_core::credentials::CredentialStore::open(data_dir.clone())
            .context("failed to open credential store")?,
    );
    let engine = std::sync::Arc::new(orch_core::engine::PerformanceEngine::new(
        credentials.clone(),
    ));

    // Load provider specs from repertoire
    let mut providers = std::collections::HashMap::new();
    let providers_dir = data_dir.join("repertoire/providers");
    if let Ok(entries) = std::fs::read_dir(&providers_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|e| e != "yaml") {
                continue;
            }
            let Ok(content) = std::fs::read_to_string(&path) else {
                continue;
            };
            let Ok(spec) = serde_yaml::from_str::<orch_core::repertoire::ProviderSpec>(&content)
            else {
                continue;
            };
            tracing::info!("loaded provider: {}", spec.metadata.name);
            providers.insert(spec.metadata.name.clone(), spec);
        }
    }
    tracing::info!("providers loaded: {}", providers.len());

    let metrics = std::sync::Arc::new(
        orch_core::metrics::MetricsStore::open(&data_dir.join("db/orchestrator.db"))
            .context("failed to open metrics database")?,
    );

    let namespaces = std::sync::Arc::new(orch_core::namespace::NamespaceManager::new(
        data_dir.clone(),
    ));

    let state = orch_core::server::AppState {
        credentials,
        engine,
        providers,
        metrics,
        namespaces,
    };

    let _ = sd_notify::notify(&[NotifyState::Ready]);

    let app = orch_core::server::app(state);
    orch_core::server::serve(listener, app, shutdown_signal()).await?;

    tracing::info!("orchestratord stopped");
    Ok(())
}

async fn shutdown_signal() {
    use tokio::signal::unix::{SignalKind, signal};

    let mut sigterm = signal(SignalKind::terminate()).expect("failed to register SIGTERM handler");

    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            tracing::info!("received SIGINT, shutting down");
        }
        _ = sigterm.recv() => {
            tracing::info!("received SIGTERM, shutting down");
        }
    }

    let _ = sd_notify::notify(&[NotifyState::Stopping]);
}
