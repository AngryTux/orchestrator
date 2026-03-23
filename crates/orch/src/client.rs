use anyhow::{Context, Result, anyhow};
use http_body_util::{BodyExt, Full};
use hyper::Request;
use hyper::body::Bytes;
use hyper_util::rt::TokioIo;
use std::path::PathBuf;
use tokio::net::UnixStream;

pub struct DaemonClient {
    socket_path: PathBuf,
}

impl DaemonClient {
    pub fn new() -> Result<Self> {
        let runtime_dir = std::env::var("XDG_RUNTIME_DIR").context("XDG_RUNTIME_DIR not set")?;
        let socket_path = PathBuf::from(runtime_dir)
            .join("orchestrator")
            .join("orchestrator.sock");
        Ok(Self { socket_path })
    }

    pub async fn get(&self, path: &str) -> Result<String> {
        self.request("GET", path, None).await
    }

    pub async fn post(&self, path: &str, body: &str) -> Result<String> {
        self.request("POST", path, Some(body)).await
    }

    async fn request(&self, method: &str, path: &str, body: Option<&str>) -> Result<String> {
        let stream = UnixStream::connect(&self.socket_path)
            .await
            .context(format!(
                "cannot connect to daemon at {}. Is orchestratord running?",
                self.socket_path.display()
            ))?;

        let io = TokioIo::new(stream);
        let (mut sender, conn) = hyper::client::conn::http1::handshake(io)
            .await
            .context("HTTP handshake failed")?;

        tokio::spawn(conn);

        let req = match body {
            Some(b) => Request::builder()
                .method(method)
                .uri(path)
                .header("content-type", "application/json")
                .body(Full::new(Bytes::from(b.to_string())))
                .context("building request")?,
            None => Request::builder()
                .method(method)
                .uri(path)
                .body(Full::new(Bytes::new()))
                .context("building request")?,
        };

        let response = sender
            .send_request(req)
            .await
            .context("sending request to daemon")?;

        let status = response.status();
        let body_bytes = response
            .into_body()
            .collect()
            .await
            .context("reading response body")?
            .to_bytes();

        let body_str = String::from_utf8_lossy(&body_bytes).to_string();

        if !status.is_success() {
            return Err(anyhow!("daemon returned {}: {}", status, body_str));
        }

        Ok(body_str)
    }
}
