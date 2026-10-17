//! The `mise daemons providers` and `mise daemons resource` commands.

use super::providers::*;
use crate::config::Config;
use eyre::{Result, bail};
use std::path::PathBuf;

#[derive(Debug, usage_rs::Args)]
pub struct Exec {
    manifest: PathBuf,
    command: String,
}

impl Exec {
    pub fn run(self) -> Result<()> {
        let manifest: Execution = serde_json::from_slice(&std::fs::read(&self.manifest)?)?;
        let command = manifest
            .commands
            .get(&self.command)
            .ok_or_else(|| eyre::eyre!("unknown provider command"))?;
        let mut process = std::process::Command::new("/bin/sh");
        process
            .arg("-c")
            .arg(command)
            .env_clear()
            .envs(&manifest.env)
            .current_dir(&manifest.root);
        #[cfg(unix)]
        {
            use eyre::Context;
            use std::os::unix::process::CommandExt;
            Err(process.exec()).wrap_err("executing provider command")
        }
        #[cfg(not(unix))]
        {
            let _ = process;
            bail!("providers are not supported on Windows");
        }
    }
}

/// [experimental] Manage shared servers defined in global `[daemon_providers]`
#[derive(Debug, usage_rs::Args)]
pub struct Providers {
    #[usage(subcommand)]
    command: Option<ProviderCommand>,
}

#[derive(Debug, usage_rs::Subcommands)]
enum ProviderCommand {
    /// [experimental] List shared servers
    Ls(ProviderList),
    /// [experimental] Start shared servers
    Start(Names),
    /// [experimental] Stop shared servers (data is kept)
    Stop(Names),
    /// [experimental] Restart shared servers with their current configuration
    Restart(Names),
}
#[derive(Debug, usage_rs::Args)]
struct ProviderList {
    /// Output in JSON format
    #[usage(long)]
    json: bool,
}
#[derive(Debug, usage_rs::Args)]
struct Names {
    names: Vec<String>,
}

impl Providers {
    pub async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        let mut providers = load(&super::glue::view_map(&config.config_files))?;
        let (action, names, json) = match self.command {
            Some(ProviderCommand::Start(n)) => ("start", n.names, false),
            Some(ProviderCommand::Stop(n)) => ("stop", n.names, false),
            Some(ProviderCommand::Restart(n)) => ("restart", n.names, false),
            Some(ProviderCommand::Ls(n)) => ("ls", vec![], n.json),
            None => ("ls", vec![], false),
        };
        if matches!(action, "ls" | "stop") {
            for entry in std::fs::read_dir(crate::dirs::STATE.join("daemon-providers"))
                .into_iter()
                .flatten()
                .flatten()
            {
                let name = entry.file_name().to_string_lossy().into_owned();
                if !name
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
                    || providers.contains_key(&name)
                {
                    continue;
                }
                let path = entry.path().join("definition.json");
                if let Ok(bytes) = std::fs::read(&path) {
                    let declaration = serde_json::from_slice(&bytes).unwrap_or_else(|_| {
                        warn!("invalid saved provider definition {}; lifecycle cleanup remains available", path.display());
                        toml::Table::new()
                    });
                    providers.insert(
                        name.clone(),
                        Provider {
                            name,
                            source: super::host::global_config_path(),
                            declaration,
                        },
                    );
                }
            }
        }
        for name in &names {
            if !providers.contains_key(name) {
                bail!("unknown daemon provider {name:?}");
            }
        }
        if action != "ls" && names.is_empty() {
            bail!("name the providers to {action}");
        }
        let mut rows = vec![];
        for provider in providers
            .values()
            .filter(|p| names.is_empty() || names.contains(&p.name))
        {
            let root = directory(&provider.name);
            // Ahead of the supervisor lookup: installing Pitchfork would not help.
            if action != "ls" && action != "stop" {
                provider.ensure_runnable_as_user()?;
            }
            let rt = provider.runtime().await;
            if action == "ls" {
                let daemon = provider.daemon().ok();
                let status = if let Ok(rt) = &rt {
                    rt.status(&crate::dirs::HOME, &provider.id()).await.ok()
                } else {
                    None
                };
                rows.push(serde_json::json!({"name": provider.name, "id": provider.id(), "source": provider.source, "preset": daemon.as_ref().and_then(|d| d.preset.as_ref()), "port": daemon.as_ref().and_then(|d| d.port.as_ref()).map(|p| p.port), "data_dir": daemon.as_ref().and_then(|d| d.data_dir.as_ref()), "ownership": "provider", "status": status.and_then(|v| v.get("status").cloned()).unwrap_or("available".into())}));
                continue;
            }
            let rt = rt.as_ref().map_err(|e| eyre::eyre!("{e:#}"))?;
            if matches!(action, "stop" | "restart") && root.is_dir() {
                rt.exec(&root, vec!["stop".into(), provider.id()]).await?;
            }
            if action != "stop" {
                provider.install().await?;
                provider.prepare(rt, true).await?;
                rt.exec(&root, vec!["start".into(), provider.id()]).await?;
            }
        }
        if action == "ls" {
            if json {
                miseprintln!("{}", serde_json::to_string_pretty(&rows)?);
            } else {
                for row in rows {
                    miseprintln!(
                        "{}\t{}\t{}",
                        row["name"].as_str().unwrap_or_default(),
                        row["preset"].as_str().unwrap_or_default(),
                        row["status"].as_str().unwrap_or_default()
                    );
                }
            }
        }
        Ok(())
    }
}

/// A project-owned readiness process: it never starts or stops the server itself.
#[derive(Debug, usage_rs::Args)]
pub struct Resource {
    provider: PathBuf,
    resource: String,
}

impl Resource {
    pub async fn run(self) -> Result<()> {
        validate_resource(&self.resource)?;
        let root = self.provider;
        let execution: Execution =
            serde_json::from_slice(&std::fs::read(root.join("execution.json"))?)?;
        if !matches!(
            execution.preset.as_str(),
            "postgres" | "cockroachdb" | "nats"
        ) || execution.port == 0
            || (execution.preset == "nats" && execution.data_dir.as_os_str().is_empty())
        {
            bail!(
                "provider metadata predates resource support; explicitly restart its provider first"
            );
        }
        {
            let _lock = crate::lock_file::LockFile::at(&root.join("resources.lock")).lock()?;
            if execution.preset == "nats" {
                super::providers_nats::provision(&execution, &self.resource).await?;
            } else {
                let port = execution.port;
                let postgres = execution.preset == "postgres";
                // Concurrent starts can encounter a provider whose first start is
                // still in flight. Check SQL readiness before provisioning.
                let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(60);
                loop {
                    match sql(&execution, postgres, port, "SELECT 1").await {
                        Ok(_) => break,
                        Err(err) if tokio::time::Instant::now() >= deadline => return Err(err),
                        Err(_) => tokio::time::sleep(std::time::Duration::from_millis(200)).await,
                    }
                }

                let query = format!(
                    "SELECT 1 FROM pg_database WHERE datname = '{}'",
                    self.resource
                );
                if sql(&execution, postgres, port, &query).await?.trim() != "1" {
                    sql(
                        &execution,
                        postgres,
                        port,
                        &format!("CREATE DATABASE \"{}\"", self.resource),
                    )
                    .await?;
                }
            }
        }
        // Readiness is output only after provisioning succeeded. Pitchfork owns
        // this small process, so stopping a consumer cannot stop the provider.
        miseprintln!("mise shared resource ready");
        use std::io::Write;
        std::io::stdout().flush()?;
        std::future::pending::<()>().await;
        Ok(())
    }
}

async fn sql(execution: &Execution, postgres: bool, port: u16, query: &str) -> Result<String> {
    let mut cmd = tokio::process::Command::new(if postgres { "psql" } else { "cockroach" });
    cmd.env_clear()
        .envs(&execution.env)
        .env("PGCONNECT_TIMEOUT", "5")
        .current_dir(&execution.root)
        .kill_on_drop(true);
    if postgres {
        cmd.env("PGOPTIONS", "-c statement_timeout=30000").args([
            "-X",
            "-A",
            "-t",
            "-v",
            "ON_ERROR_STOP=1",
            "-h",
            "127.0.0.1",
            "-p",
            &port.to_string(),
            "-U",
            "postgres",
            "-d",
            "postgres",
            "-c",
            query,
        ]);
    } else {
        cmd.args([
            "sql",
            "--insecure",
            &format!("--host=127.0.0.1:{port}"),
            "--database=defaultdb",
            "--format=tsv",
            "--execute",
            query,
        ]);
    }
    let output = tokio::time::timeout(std::time::Duration::from_secs(40), cmd.output()).await??;
    if !output.status.success() {
        bail!(
            "resource provisioning failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let output = String::from_utf8(output.stdout)?;
    // Cockroach's TSV includes a column heading, unlike psql's tuples-only mode.
    Ok(if postgres {
        output
    } else {
        output.lines().skip(1).collect::<Vec<_>>().join("\n")
    })
}
