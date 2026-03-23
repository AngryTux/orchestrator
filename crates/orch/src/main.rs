use anyhow::{Result, anyhow};
use clap::{Parser, Subcommand};

mod client;

#[derive(Parser)]
#[command(
    name = "orch",
    about = "Orchestrator CLI — secure LLM workspace manager"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Show daemon health status
    Health,
    /// Show daemon version
    Version,
    /// Show host system information (kernel, security, resources)
    Info,

    /// Manage providers
    #[command(subcommand)]
    Provider(ProviderCommand),

    /// Run a performance
    Run {
        /// The prompt to send
        prompt: String,
        /// Provider to use (default: claude)
        #[arg(short, long, default_value = "claude")]
        provider: String,
        /// Namespace (default: default)
        #[arg(short, long, default_value = "default")]
        namespace: String,
        /// Formation (solo, duet)
        #[arg(short, long)]
        formation: Option<String>,
    },

    /// List past performances
    #[command(subcommand)]
    Performance(PerformanceCommand),

    /// Show aggregate metrics
    Metrics,

    /// Manage namespaces
    #[command(subcommand)]
    Namespace(NamespaceCommand),
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
        /// Provider name
        name: String,
        /// API key
        key: String,
        #[arg(short, long, default_value = "default")]
        namespace: String,
    },
    /// Remove a provider credential
    Rm {
        /// Provider name
        name: String,
        #[arg(short, long, default_value = "default")]
        namespace: String,
    },
    /// Test a provider (credential + binary)
    Test {
        /// Provider name
        name: String,
        #[arg(short, long, default_value = "default")]
        namespace: String,
    },
}

#[derive(Subcommand)]
enum PerformanceCommand {
    /// List past performances
    List {
        #[arg(short, long, default_value = "default")]
        namespace: String,
    },
    /// Show performance details
    Inspect {
        /// Performance ID
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
    Create {
        /// Namespace name
        name: String,
    },
    /// Inspect a namespace
    Inspect {
        /// Namespace name
        name: String,
    },
    /// Delete a namespace
    Rm {
        /// Namespace name
        name: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let client = client::DaemonClient::new()?;

    match cli.command {
        Command::Health => {
            let body = client.get("/v1/system/health").await?;
            let json: serde_json::Value = serde_json::from_str(&body)?;
            println!("{}", json["status"].as_str().unwrap_or("unknown"));
        }

        Command::Version => {
            let body = client.get("/v1/system/version").await?;
            let json: serde_json::Value = serde_json::from_str(&body)?;
            println!("{}", json["version"].as_str().unwrap_or("unknown"));
        }

        Command::Info => {
            let body = client.get("/v1/system/info").await?;
            let json: serde_json::Value = serde_json::from_str(&body)?;
            println!(
                "Kernel:     {} (Landlock ABI v{})",
                json["kernel"]["release"].as_str().unwrap_or("?"),
                json["security"]["landlock_abi"]
            );
            println!("Seccomp:    {}", json["security"]["seccomp"]);
            println!("cgroup v2:  {}", json["security"]["cgroup_v2"]);
            println!("SELinux:    {}", json["security"]["selinux"]);
            println!("AppArmor:   {}", json["security"]["apparmor"]);
            println!("CPU:        {} cores", json["resources"]["cpu_count"]);
            println!(
                "Memory:     {} MB",
                json["resources"]["memory_total_bytes"]
                    .as_u64()
                    .unwrap_or(0)
                    / 1024
                    / 1024
            );
        }

        Command::Provider(sub) => match sub {
            ProviderCommand::List { namespace } => {
                let body = client
                    .get(&format!("/v1/namespaces/{namespace}/providers"))
                    .await?;
                let json: serde_json::Value = serde_json::from_str(&body)?;
                let providers = json["providers"].as_array();
                if let Some(providers) = providers {
                    if providers.is_empty() {
                        println!("No providers configured in namespace '{namespace}'");
                    } else {
                        for p in providers {
                            println!("{}", p.as_str().unwrap_or("?"));
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
                let resp = client
                    .post(
                        &format!("/v1/namespaces/{namespace}/providers"),
                        &body.to_string(),
                    )
                    .await?;
                let json: serde_json::Value = serde_json::from_str(&resp)?;
                println!(
                    "Provider '{}' added to namespace '{}'",
                    json["provider"].as_str().unwrap_or(&name),
                    json["namespace"].as_str().unwrap_or(&namespace)
                );
            }
            ProviderCommand::Rm { name, namespace } => {
                client
                    .delete(&format!("/v1/namespaces/{namespace}/providers/{name}"))
                    .await?;
                println!("Provider '{name}' removed from namespace '{namespace}'");
            }
            ProviderCommand::Test { name, namespace } => {
                let resp = client
                    .post(
                        &format!("/v1/namespaces/{namespace}/providers/{name}/test"),
                        "{}",
                    )
                    .await?;
                let json: serde_json::Value = serde_json::from_str(&resp)?;
                println!(
                    "Provider '{}': credential {}, binary {}",
                    name,
                    json["credential"].as_str().unwrap_or("?"),
                    json["binary"].as_str().unwrap_or("?")
                );
            }
        },

        Command::Run {
            prompt,
            provider,
            namespace,
            formation,
        } => {
            let mut body = serde_json::json!({
                "prompt": prompt,
                "provider": provider,
            });
            if let Some(f) = &formation {
                body["formation"] = serde_json::Value::String(f.clone());
            }

            let resp = client
                .post(
                    &format!("/v1/namespaces/{namespace}/performances"),
                    &body.to_string(),
                )
                .await?;

            let json: serde_json::Value = serde_json::from_str(&resp)?;

            // Print summary
            if let Some(summary) = json["summary"].as_str() {
                println!("{summary}");
            }

            // Print metadata to stderr
            eprintln!("\n---");
            eprintln!(
                "performance: {}",
                json["performance_id"].as_str().unwrap_or("?")
            );
            eprintln!("formation:   {}", json["formation"].as_str().unwrap_or("?"));
            eprintln!("duration:    {}ms", json["total_duration_ms"]);
            if let Some(sections) = json["sections"].as_array() {
                eprintln!("sections:    {}", sections.len());
                for s in sections {
                    let status = if s["success"].as_bool().unwrap_or(false) {
                        "ok"
                    } else {
                        "FAILED"
                    };
                    eprintln!(
                        "  {} [{}] {}ms",
                        s["section_id"].as_str().unwrap_or("?"),
                        status,
                        s["duration_ms"]
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
                        println!("No performances in namespace '{namespace}'");
                    } else {
                        println!(
                            "{id:<24} {form:<8} {status:<7} {dur:<10} PROMPT",
                            id = "ID",
                            form = "FORM",
                            status = "STATUS",
                            dur = "DURATION"
                        );
                        for p in perfs {
                            let status = if p["harmony"].as_bool().unwrap_or(false) {
                                "ok"
                            } else {
                                "fail"
                            };
                            let prompt = p["prompt"].as_str().unwrap_or("?");
                            let truncated = if prompt.len() > 40 {
                                format!("{}...", &prompt[..37])
                            } else {
                                prompt.to_string()
                            };
                            println!(
                                "{:<24} {:<8} {:<7} {:<10} {}",
                                p["performance_id"].as_str().unwrap_or("?"),
                                p["formation"].as_str().unwrap_or("?"),
                                status,
                                format!("{}ms", p["duration_ms"]),
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
            let json: serde_json::Value = serde_json::from_str(&body)?;
            println!("Performances: {}", json["total_performances"]);
            println!("Tokens in:    {}", json["total_tokens_in"]);
            println!("Tokens out:   {}", json["total_tokens_out"]);
            println!(
                "Cost:         ${:.4}",
                json["total_cost_usd"].as_f64().unwrap_or(0.0)
            );
        }

        Command::Namespace(sub) => match sub {
            NamespaceCommand::List => {
                let body = client.get("/v1/namespaces").await?;
                let json: serde_json::Value = serde_json::from_str(&body)?;
                if let Some(names) = json.as_array() {
                    if names.is_empty() {
                        println!("No namespaces created yet");
                    } else {
                        for n in names {
                            println!("{}", n.as_str().unwrap_or("?"));
                        }
                    }
                }
            }
            NamespaceCommand::Create { name } => {
                let body = serde_json::json!({"name": name});
                client.post("/v1/namespaces", &body.to_string()).await?;
                println!("Namespace '{name}' created");
            }
            NamespaceCommand::Inspect { name } => {
                let body = client.get(&format!("/v1/namespaces/{name}")).await?;
                let json: serde_json::Value = serde_json::from_str(&body)?;
                if json.is_null() {
                    println!("Namespace '{name}' not found");
                } else {
                    println!("{}", serde_json::to_string_pretty(&json)?);
                }
            }
            NamespaceCommand::Rm { name } => {
                client.delete(&format!("/v1/namespaces/{name}")).await?;
                println!("Namespace '{name}' deleted");
            }
        },
    }

    Ok(())
}
