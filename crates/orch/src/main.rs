use anyhow::{Result, anyhow};
use clap::{Parser, Subcommand};

mod client;

#[derive(Parser)]
#[command(
    name = "orch",
    about = "Orchestrator — secure LLM workspace manager",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Check daemon health
    Health,
    /// Show version
    Version,
    /// Show system information
    Info,
    /// Manage providers
    #[command(subcommand)]
    Provider(ProviderCommand),
    /// Run a performance
    Run {
        /// The prompt to send
        prompt: String,
        /// Provider to use
        #[arg(short, long, default_value = "claude")]
        provider: String,
        /// Namespace
        #[arg(short, long, default_value = "default")]
        namespace: String,
        /// Formation (solo, duet)
        #[arg(short, long)]
        formation: Option<String>,
        /// Model(s) — for duet: "haiku,opus"
        #[arg(short, long)]
        model: Option<String>,
    },
    /// Manage performances
    #[command(subcommand)]
    Performance(PerformanceCommand),
    /// Show metrics
    Metrics,
    /// Manage namespaces
    #[command(subcommand)]
    Namespace(NamespaceCommand),
    /// Update orchestrator to latest version
    Update,
}

#[derive(Subcommand)]
enum ProviderCommand {
    /// List configured providers
    List {
        #[arg(short, long, default_value = "default")]
        namespace: String,
    },
    /// Add a provider credential
    Add {
        name: String,
        key: String,
        #[arg(short, long, default_value = "default")]
        namespace: String,
    },
    /// Remove a provider
    Rm {
        name: String,
        #[arg(short, long, default_value = "default")]
        namespace: String,
    },
    /// Test a provider
    Test {
        name: String,
        #[arg(short, long, default_value = "default")]
        namespace: String,
    },
}

#[derive(Subcommand)]
enum PerformanceCommand {
    /// List performances
    List {
        #[arg(short, long, default_value = "default")]
        namespace: String,
    },
    /// Inspect a performance
    Inspect {
        id: String,
        #[arg(short, long, default_value = "default")]
        namespace: String,
    },
}

#[derive(Subcommand)]
enum NamespaceCommand {
    /// List namespaces
    List,
    /// Create a namespace
    Create { name: String },
    /// Inspect a namespace
    Inspect { name: String },
    /// Delete a namespace
    Rm { name: String },
}

// ─── Formatting helpers ─────────────────────────────────

fn label(key: &str, val: &str) {
    println!("  {:<14} {}", key, val);
}

fn bool_icon(v: bool) -> &'static str {
    if v { "✓" } else { "✗" }
}

fn status_icon(ok: bool) -> &'static str {
    if ok { "✓" } else { "✗" }
}

fn header(title: &str) {
    println!("{title}");
}

fn divider() {
    println!("{}", "─".repeat(50));
}

fn humanize_bytes(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.1} GB", bytes as f64 / 1024.0 / 1024.0 / 1024.0)
    } else {
        format!("{} MB", bytes / 1024 / 1024)
    }
}

// ─── Main ───────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let client = client::DaemonClient::new()?;

    match cli.command {
        Command::Health => {
            let body = client.get("/v1/system/health").await?;
            let json: serde_json::Value = serde_json::from_str(&body)?;
            let status = json["status"].as_str().unwrap_or("unknown");
            println!("{} daemon is {status}", status_icon(status == "ok"));
        }

        Command::Version => {
            let body = client.get("/v1/system/version").await?;
            let json: serde_json::Value = serde_json::from_str(&body)?;
            println!("orch {}", json["version"].as_str().unwrap_or("unknown"));
        }

        Command::Info => {
            let body = client.get("/v1/system/info").await?;
            let j: serde_json::Value = serde_json::from_str(&body)?;

            header("System");
            label(
                "Kernel",
                &format!(
                    "{} (Landlock ABI v{})",
                    j["kernel"]["release"].as_str().unwrap_or("?"),
                    j["security"]["landlock_abi"]
                ),
            );
            label("CPU", &format!("{} cores", j["resources"]["cpu_count"]));
            label(
                "Memory",
                &humanize_bytes(j["resources"]["memory_total_bytes"].as_u64().unwrap_or(0)),
            );
            println!();

            header("Security");
            label(
                "Seccomp",
                bool_icon(j["security"]["seccomp"].as_bool().unwrap_or(false)),
            );
            label(
                "cgroup v2",
                bool_icon(j["security"]["cgroup_v2"].as_bool().unwrap_or(false)),
            );
            label(
                "SELinux",
                bool_icon(j["security"]["selinux"].as_bool().unwrap_or(false)),
            );
            label(
                "AppArmor",
                bool_icon(j["security"]["apparmor"].as_bool().unwrap_or(false)),
            );
            label(
                "pidfd",
                bool_icon(j["security"]["pidfd"].as_bool().unwrap_or(false)),
            );
            label(
                "User NS",
                bool_icon(j["security"]["user_namespaces"].as_bool().unwrap_or(false)),
            );
        }

        Command::Provider(sub) => match sub {
            ProviderCommand::List { namespace } => {
                let body = client
                    .get(&format!("/v1/namespaces/{namespace}/providers"))
                    .await?;
                let json: serde_json::Value = serde_json::from_str(&body)?;
                if let Some(providers) = json["providers"].as_array() {
                    if providers.is_empty() {
                        println!("No providers in namespace '{namespace}'");
                    } else {
                        header(&format!("Providers ({namespace})"));
                        for p in providers {
                            println!("  • {}", p.as_str().unwrap_or("?"));
                        }
                    }
                }
            }
            ProviderCommand::Add {
                name,
                key,
                namespace,
            } => {
                let body = serde_json::json!({"name": name, "key": key});
                client
                    .post(
                        &format!("/v1/namespaces/{namespace}/providers"),
                        &body.to_string(),
                    )
                    .await?;
                println!("✓ Provider '{name}' added to '{namespace}'");
            }
            ProviderCommand::Rm { name, namespace } => {
                client
                    .delete(&format!("/v1/namespaces/{namespace}/providers/{name}"))
                    .await?;
                println!("✓ Provider '{name}' removed from '{namespace}'");
            }
            ProviderCommand::Test { name, namespace } => {
                let resp = client
                    .post(
                        &format!("/v1/namespaces/{namespace}/providers/{name}/test"),
                        "{}",
                    )
                    .await?;
                let json: serde_json::Value = serde_json::from_str(&resp)?;
                let cred = json["credential"].as_str().unwrap_or("?");
                let bin = json["binary"].as_str().unwrap_or("?");
                println!("Provider '{name}'");
                label(
                    "Credential",
                    &format!("{} {cred}", status_icon(cred == "valid")),
                );
                label("Binary", &format!("{} {bin}", status_icon(bin == "found")));
            }
        },

        Command::Run {
            prompt,
            provider,
            namespace,
            formation,
            model,
        } => {
            let mut body = serde_json::json!({
                "prompt": prompt,
                "provider": provider,
            });
            if let Some(f) = &formation {
                body["formation"] = serde_json::Value::String(f.clone());
            }
            if let Some(m) = &model {
                let models: Vec<&str> = m.split(',').map(|s| s.trim()).collect();
                body["models"] = serde_json::json!(models);
            }

            let resp = client
                .post(
                    &format!("/v1/namespaces/{namespace}/performances"),
                    &body.to_string(),
                )
                .await?;

            let j: serde_json::Value = serde_json::from_str(&resp)?;

            // Output
            if let Some(summary) = j["summary"].as_str() {
                println!("{summary}");
            }

            // Metadata
            eprintln!();
            divider();
            eprintln!(
                "  {} {} │ {} │ {}ms",
                status_icon(true),
                j["performance_id"].as_str().unwrap_or("?"),
                j["formation"].as_str().unwrap_or("?"),
                j["total_duration_ms"]
            );
            if let Some(sections) = j["sections"].as_array() {
                for s in sections {
                    let ok = s["success"].as_bool().unwrap_or(false);
                    let model_str = s["model"].as_str().unwrap_or("default");
                    eprintln!(
                        "    {} {} │ {}ms │ {}",
                        status_icon(ok),
                        s["section_id"].as_str().unwrap_or("?"),
                        s["duration_ms"],
                        model_str
                    );
                }
            }
        }

        Command::Performance(sub) => match sub {
            PerformanceCommand::List { namespace } => {
                let body = client
                    .get(&format!("/v1/namespaces/{namespace}/performances"))
                    .await?;
                let json: serde_json::Value = serde_json::from_str(&body)?;
                if let Some(perfs) = json.as_array() {
                    if perfs.is_empty() {
                        println!("No performances in '{namespace}'");
                    } else {
                        header(&format!("Performances ({namespace})"));
                        divider();
                        for p in perfs {
                            let ok = p["harmony"].as_bool().unwrap_or(false);
                            let prompt = p["prompt"].as_str().unwrap_or("?");
                            let truncated = if prompt.len() > 40 {
                                format!("{}...", &prompt[..37])
                            } else {
                                prompt.to_string()
                            };
                            println!(
                                "  {} {:<22} {:>6}ms  {}",
                                status_icon(ok),
                                p["performance_id"].as_str().unwrap_or("?"),
                                p["duration_ms"],
                                truncated
                            );
                        }
                    }
                }
            }
            PerformanceCommand::Inspect { id, namespace } => {
                let body = client
                    .get(&format!("/v1/namespaces/{namespace}/performances/{id}"))
                    .await?;
                let json: serde_json::Value = serde_json::from_str(&body)?;
                if json.is_null() {
                    return Err(anyhow!("performance not found: {id}"));
                }
                println!("{}", serde_json::to_string_pretty(&json)?);
            }
        },

        Command::Metrics => {
            let body = client.get("/v1/metrics").await?;
            let j: serde_json::Value = serde_json::from_str(&body)?;
            header("Metrics");
            label("Performances", &j["total_performances"].to_string());
            label("Tokens in", &j["total_tokens_in"].to_string());
            label("Tokens out", &j["total_tokens_out"].to_string());
            label(
                "Cost",
                &format!("${:.4}", j["total_cost_usd"].as_f64().unwrap_or(0.0)),
            );
        }

        Command::Namespace(sub) => match sub {
            NamespaceCommand::List => {
                let body = client.get("/v1/namespaces").await?;
                let json: serde_json::Value = serde_json::from_str(&body)?;
                if let Some(names) = json.as_array() {
                    if names.is_empty() {
                        println!("No namespaces");
                    } else {
                        header("Namespaces");
                        for n in names {
                            println!("  • {}", n.as_str().unwrap_or("?"));
                        }
                    }
                }
            }
            NamespaceCommand::Create { name } => {
                let body = serde_json::json!({"name": name});
                client.post("/v1/namespaces", &body.to_string()).await?;
                println!("✓ Namespace '{name}' created");
            }
            NamespaceCommand::Inspect { name } => {
                let body = client.get(&format!("/v1/namespaces/{name}")).await?;
                let json: serde_json::Value = serde_json::from_str(&body)?;
                if json.is_null() {
                    return Err(anyhow!("namespace not found: {name}"));
                }
                println!("{}", serde_json::to_string_pretty(&json)?);
            }
            NamespaceCommand::Rm { name } => {
                client.delete(&format!("/v1/namespaces/{name}")).await?;
                println!("✓ Namespace '{name}' deleted");
            }
        },

        Command::Update => {
            update_self()?;
        }
    }

    Ok(())
}

fn update_self() -> Result<()> {
    use std::process::Command as Cmd;

    let current_version = env!("CARGO_PKG_VERSION");
    let api_url = "https://api.github.com/repos/AngryTux/orchestrator/releases/latest";
    let install_dir = format!("{}/.local/bin", std::env::var("HOME")?);

    // 1. Check latest release
    println!("→ Checking for updates...");
    let output = Cmd::new("curl")
        .args([
            "-fsSL",
            "-H",
            "Accept: application/vnd.github.v3+json",
            api_url,
        ])
        .output()?;

    if !output.status.success() {
        return Err(anyhow!("failed to check for updates (no releases found?)"));
    }

    let release: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    let latest_tag = release["tag_name"]
        .as_str()
        .ok_or_else(|| anyhow!("no tag_name in release"))?;
    let latest_version = latest_tag.trim_start_matches('v');
    let tarball_url = release["tarball_url"]
        .as_str()
        .ok_or_else(|| anyhow!("no tarball_url in release"))?;
    let changelog = release["body"].as_str().unwrap_or("No release notes.");

    // 2. Compare versions
    if latest_version == current_version {
        println!("✓ Already up to date (v{current_version})");
        return Ok(());
    }

    println!("  Current: v{current_version}");
    println!("  Latest:  {latest_tag}");
    println!();

    // 3. Show changelog
    header("Changelog");
    divider();
    println!("{changelog}");
    divider();
    println!();

    // 4. Download source tarball (not git clone — lighter, versioned)
    println!("→ Downloading {latest_tag}...");
    let tmp = std::env::temp_dir().join(format!("orch-update-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp)?;

    let tarball = tmp.join("source.tar.gz");
    let status = Cmd::new("curl")
        .args(["-fsSL", "-o"])
        .arg(&tarball)
        .arg(tarball_url)
        .status()?;
    if !status.success() {
        let _ = std::fs::remove_dir_all(&tmp);
        return Err(anyhow!("failed to download release tarball"));
    }

    // Extract
    let status = Cmd::new("tar")
        .args(["xzf"])
        .arg(&tarball)
        .arg("-C")
        .arg(&tmp)
        .arg("--strip-components=1")
        .status()?;
    if !status.success() {
        let _ = std::fs::remove_dir_all(&tmp);
        return Err(anyhow!("failed to extract tarball"));
    }
    println!("✓ Source downloaded");

    // 5. Backup current binaries
    let dst_daemon = format!("{install_dir}/orchestratord");
    let dst_cli = format!("{install_dir}/orch");

    if std::path::Path::new(&dst_daemon).exists() {
        let _ = std::fs::copy(&dst_daemon, format!("{dst_daemon}.bak"));
    }
    if std::path::Path::new(&dst_cli).exists() {
        let _ = std::fs::copy(&dst_cli, format!("{dst_cli}.bak"));
    }

    // 6. Build
    println!("→ Building {latest_tag}...");
    let status = Cmd::new("cargo")
        .args(["build", "--release"])
        .current_dir(&tmp)
        .status()?;
    if !status.success() {
        // Restore backup
        let _ = std::fs::copy(format!("{dst_daemon}.bak"), &dst_daemon);
        let _ = std::fs::copy(format!("{dst_cli}.bak"), &dst_cli);
        let _ = std::fs::remove_dir_all(&tmp);
        return Err(anyhow!("build failed — previous version restored"));
    }
    println!("✓ Build complete");

    // 7. Install (atomic rename)
    let src_daemon = tmp.join("target/release/orchestratord");
    let src_cli = tmp.join("target/release/orch");
    std::fs::copy(&src_daemon, &dst_daemon)?;
    std::fs::copy(&src_cli, &dst_cli)?;

    // Cleanup backups
    let _ = std::fs::remove_file(format!("{dst_daemon}.bak"));
    let _ = std::fs::remove_file(format!("{dst_cli}.bak"));
    println!("✓ Binaries installed");

    // 8. Restart daemon
    let restart = Cmd::new("systemctl")
        .args(["--user", "restart", "orchestratord.socket"])
        .status();
    if restart.is_ok_and(|s| s.success()) {
        println!("✓ Daemon restarted");
    }

    // Cleanup
    let _ = std::fs::remove_dir_all(&tmp);

    println!();
    println!("✓ Updated to {latest_tag}");

    Ok(())
}
