//! User-owned servers. A provider is never a member of a consumer's lifecycle.
use super::{Daemon, DaemonSet, ports, presets, runtime};
use crate::config::config_file::mise_toml::MiseToml;
use crate::config::{Config, ConfigMap};
use crate::env_diff::EnvMap;
use eyre::{Context, Result, bail};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub(crate) struct Provider {
    pub name: String,
    pub source: PathBuf,
    pub declaration: toml::Table,
}

pub(crate) fn directory(name: &str) -> PathBuf {
    crate::dirs::STATE.join("daemon-providers").join(name)
}

pub(crate) fn load(files: &ConfigMap) -> Result<IndexMap<String, Provider>> {
    let mut providers = IndexMap::new();
    for cf in files.values().rev() {
        let declarations = cf.daemon_providers();
        if declarations.is_empty() {
            continue;
        }
        if !crate::config::is_global_config(cf.get_path()) {
            bail!(
                "[daemon_providers] belongs in global mise configuration, not {}",
                cf.get_path().display()
            );
        }
        for (name, declaration) in declarations {
            super::validate_name("provider", &name)?;
            if !name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
            {
                bail!("invalid provider name {name:?}");
            }
            let provider = Provider {
                name: name.clone(),
                source: cf.get_path().into(),
                declaration,
            };
            provider.daemon()?;
            providers.insert(name, provider);
        }
    }
    Ok(providers)
}

impl Provider {
    pub(crate) fn id(&self) -> String {
        format!("mise-provider-{}/server", self.name)
    }

    pub(crate) fn daemon(&self) -> Result<Daemon> {
        let mut table = self.declaration.clone();
        let preset = super::take_string(&mut table, "preset")?
            .ok_or_else(|| eyre::eyre!("provider {} requires preset", self.name))?;
        if !matches!(preset.as_str(), "postgres" | "cockroachdb" | "nats") {
            bail!(
                "provider {}: supported presets are postgres, cockroachdb and nats",
                self.name
            );
        }
        let version = super::take_string(&mut table, "version")?
            .ok_or_else(|| eyre::eyre!("provider {} requires version", self.name))?;
        for key in table.keys() {
            if !matches!(
                key.as_str(),
                "port" | "ports" | "options" | "data_dir" | "tool"
            ) {
                bail!(
                    "provider {} does not accept {key:?}; lifecycle and commands are managed by mise",
                    self.name
                );
            }
        }
        let root = directory(&self.name);
        let request = table.remove("port").unwrap_or_else(|| "auto".into());
        let claim = match ports::parse(&self.name, request)? {
            ports::PortRequest::Fixed(port) => ports::PortClaim::fixed(port),
            ports::PortRequest::Auto { base, stride } => ports::resolve_slot(
                "server",
                ports::hashed_slot(&root),
                base,
                stride,
                Some(presets::default_port(&preset)?),
                runtime::read_state(&root)?.ports.get("server").copied(),
            )?,
            _ => bail!("provider ports must be integers or auto allocations"),
        };
        table
            .entry("data_dir".to_string())
            .or_insert_with(|| root.join("data").to_string_lossy().into_owned().into());
        // Render unwrapped commands: the frozen tool environment below belongs to
        // this provider, never to a consuming project or the supervisor's shell.
        table.insert("mise".into(), false.into());
        // Shared stores can have several consumers' databases to checkpoint.
        // Pitchfork's short default can kill PostgreSQL before that completes.
        table.insert(
            "stop_signal".into(),
            toml::Value::Table(toml::Table::from_iter([
                (
                    "signal".into(),
                    if preset == "postgres" {
                        "SIGINT"
                    } else {
                        "SIGTERM"
                    }
                    .into(),
                ),
                ("timeout".into(), "60s".into()),
            ])),
        );

        table.insert("proxy_idle_timeout".into(), false.into());
        table.insert("auto".into(), toml::Value::Array(vec![]));
        presets::expand(
            "server",
            &preset,
            &version,
            table,
            presets::Extras {
                init: &[],
                port: Some(claim),
                labels: &Default::default(),
                imported: false,
            },
            &self.source,
            &root,
        )
    }

    async fn tool_config(&self, daemon: &Daemon) -> Result<Arc<Config>> {
        let (tool, version) = daemon
            .tool
            .as_ref()
            .ok_or_else(|| eyre::eyre!("provider needs a tool"))?;
        let doc = toml::Table::from_iter([(
            "tools".into(),
            toml::Value::Table(toml::Table::from_iter([(
                tool.clone(),
                version.clone().into(),
            )])),
        )]);
        let cf = MiseToml::from_str(&toml::to_string(&doc)?, &self.source)?;
        let mut config = Config::load_from_config_files(
            IndexMap::from([(
                self.source.clone(),
                Arc::new(cf) as Arc<dyn crate::config::config_file::ConfigFile>,
            )]),
            true,
        )
        .await?;
        Arc::get_mut(&mut config)
            .expect("new provider configuration")
            .project_root = Some(directory(&self.name));
        Ok(config)
    }

    pub(crate) async fn prepare(&self, rt: &runtime::Runtime) -> Result<()> {
        let root = directory(&self.name);
        std::fs::create_dir_all(&root)?;
        let _lock = crate::lock_file::LockFile::at(&root.join("provider.lock")).lock()?;
        let path = root.join("definition.json");
        let desired = serde_json::to_vec(&self.declaration)?;
        if std::fs::read(&path).is_ok_and(|old| old != desired)
            && rt.active(&root, &runtime::read_state(&root)?).await?
        {
            bail!(
                "provider {} configuration changed while running; use `mise daemons providers restart {}`",
                self.name,
                self.name
            );
        }
        let mut daemon = self.daemon()?;
        let mut config = self.tool_config(&daemon).await?;
        let mut ts = crate::toolset::Toolset::default();
        for cf in config.config_files.values() {
            ts.merge(cf.to_toolset()?);
        }
        ts.resolve_with_opts(
            &config,
            &crate::toolset::ResolveOptions {
                offline: true,
                ..Default::default()
            },
        )
        .await?;
        let (_, missing) = ts
            .install_missing_versions(
                &mut config,
                &crate::toolset::InstallOptions {
                    missing_args_only: false,
                    reload_config: false,
                    ..Default::default()
                },
            )
            .await?;
        ts.notify_missing_versions(missing);
        runtime::validate_tools(
            &DaemonSet {
                daemons: IndexMap::from([("server".into(), daemon.clone())]),
                ..Default::default()
            },
            &config,
            &ts,
        )
        .await?;
        let mut env = base_env();
        let mut paths = ts.list_paths(&config).await;
        paths.extend([
            PathBuf::from("/usr/bin"),
            PathBuf::from("/bin"),
            PathBuf::from("/usr/sbin"),
            PathBuf::from("/sbin"),
        ]);
        env.insert(
            "PATH".into(),
            std::env::join_paths(paths)?.to_string_lossy().into_owned(),
        );
        let mut commands = IndexMap::new();
        for key in ["run", "ready_cmd", "health_cmd"] {
            if let Some(command) = daemon.table.get(key).and_then(toml::Value::as_str) {
                commands.insert(key.to_string(), command.to_string());
            }
        }
        let manifest = root.join("execution.json");
        let execution = Execution {
            env,
            commands,
            root: root.clone(),
        };
        let bytes = serde_json::to_vec_pretty(&execution)?;
        if std::fs::read(&manifest).is_ok_and(|old| old != bytes)
            && rt.active(&root, &runtime::read_state(&root)?).await?
        {
            bail!(
                "provider {} execution environment changed; restart it explicitly",
                self.name
            );
        }
        runtime::write_if_changed(&manifest, &bytes)?;
        for key in execution.commands.keys() {
            daemon.table.insert(
                key.clone(),
                format!(
                    "{} daemons __provider-exec {} {}",
                    presets::quote(crate::env::MISE_BIN.to_string_lossy()),
                    presets::quote(manifest.to_string_lossy()),
                    key
                )
                .into(),
            );
        }
        let set = DaemonSet {
            daemons: IndexMap::from([("server".into(), daemon)]),
            namespaces: IndexMap::from([(root.clone(), format!("mise-provider-{}", self.name))]),
            ..Default::default()
        };
        rt.prepare(&root, &set, true, true, &["server".into()])
            .await?;
        runtime::write_if_changed(&path, &desired)?;
        Ok(())
    }
}

fn base_env() -> EnvMap {
    let mut env = EnvMap::new();
    env.insert(
        "HOME".into(),
        crate::dirs::HOME.to_string_lossy().into_owned(),
    );
    env.insert("LANG".into(), "C.UTF-8".into());
    env.insert("MISE_EXPERIMENTAL".into(), "1".into());
    for (key, value) in [
        ("MISE_STATE_DIR", &*crate::dirs::STATE),
        ("MISE_DATA_DIR", &*crate::dirs::DATA),
        ("MISE_CONFIG_DIR", &*crate::dirs::CONFIG),
    ] {
        env.insert(key.into(), value.to_string_lossy().into_owned());
    }
    env
}

#[derive(Serialize, Deserialize)]
struct Execution {
    env: EnvMap,
    commands: IndexMap<String, String>,
    root: PathBuf,
}

#[derive(Debug, usage_rs::Args)]
pub(crate) struct Exec {
    manifest: PathBuf,
    command: String,
}

impl Exec {
    pub(crate) fn run(self) -> Result<()> {
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

/// Manage shared servers defined in global [daemon_providers].
#[derive(Debug, usage_rs::Args)]
pub(crate) struct Providers {
    #[usage(subcommand)]
    command: Option<ProviderCommand>,
}

#[derive(Debug, usage_rs::Subcommands)]
enum ProviderCommand {
    /// List configured and previously managed shared servers.
    Ls(ProviderList),
    /// Start the named shared servers.
    Start(Names),
    /// Stop the named shared servers without deleting their data.
    Stop(Names),
    /// Restart the named shared servers with their current configuration.
    Restart(Names),
}
#[derive(Debug, usage_rs::Args)]
struct ProviderList {
    #[usage(long)]
    json: bool,
}
#[derive(Debug, usage_rs::Args)]
struct Names {
    names: Vec<String>,
}

impl Providers {
    pub(crate) async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        let mut providers = load(&config.config_files)?;
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
                    // Stopping needs only the name, so a damaged definition must
                    // not block cleanup of this or any other provider.
                    let declaration = serde_json::from_slice(&bytes).unwrap_or_else(|err| {
                        warn!("reading saved provider {}: {err}", path.display());
                        toml::Table::new()
                    });
                    providers.insert(
                        name.clone(),
                        Provider {
                            name,
                            source: crate::config::global_config_path(),
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
        // Providers are managed identically from every directory: resolve
        // pitchfork and its environment from global configuration only, never
        // from the project the command happens to run in.
        let global = Config::load_from_config_files(
            config
                .config_files
                .iter()
                .filter(|(path, _)| crate::config::is_global_config(path))
                .map(|(path, cf)| (path.clone(), cf.clone()))
                .collect(),
            true,
        )
        .await?;
        let (global, ts) = runtime::toolset(&global, false).await?;
        let mut rows = vec![];
        for provider in providers
            .values()
            .filter(|p| names.is_empty() || names.contains(&p.name))
        {
            let root = directory(&provider.name);
            let fallback = runtime::read_state(&root)
                .ok()
                .map(|state| state.bin)
                .filter(|bin| bin.is_file())
                .or_else(|| which::which("pitchfork").ok());
            let rt = runtime::Runtime::from_toolset(&global, &ts, fallback.as_deref()).await;
            if action == "ls" {
                let daemon = provider.daemon().ok();
                let status = if let Ok(rt) = &rt {
                    rt.status(&crate::dirs::HOME, &provider.id()).await.ok()
                } else {
                    None
                };
                rows.push(serde_json::json!({"name": provider.name, "id": provider.id(), "source": provider.source, "preset": daemon.as_ref().and_then(|d| d.preset.clone()), "port": daemon.as_ref().and_then(|d| d.port.map(|p| p.port)), "data_dir": daemon.and_then(|d| d.data_dir), "ownership": "provider", "status": status.and_then(|v| v.get("status").cloned()).unwrap_or("available".into())}));
                continue;
            }
            let rt = rt.as_ref().map_err(|e| eyre::eyre!("{e:#}"))?;
            if matches!(action, "stop" | "restart") && root.is_dir() {
                rt.exec(&root, vec!["stop".into(), provider.id()]).await?;
            }
            if action != "stop" {
                provider.prepare(rt).await?;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn provider(name: &str, extra: toml::Table) -> Provider {
        let mut declaration = toml::toml! { preset = "postgres" version = "18" };
        declaration.extend(extra);
        Provider {
            name: name.into(),
            source: crate::config::global_config_path(),
            declaration,
        }
    }

    #[test]
    fn providers_own_their_process_environment_and_lifecycle() {
        let p = provider("test-postgres", toml::Table::new());
        let d = p.daemon().unwrap();
        assert_eq!(d.root, directory("test-postgres"));
        assert_eq!(d.data_dir.unwrap(), directory("test-postgres").join("data"));
        assert_eq!(d.table["mise"].as_bool(), Some(false));
        assert_eq!(d.table["proxy_idle_timeout"].as_bool(), Some(false));
        assert!(d.table["auto"].as_array().unwrap().is_empty());
        assert!(!base_env().contains_key("MISE_ENV"));
        assert!(!base_env().contains_key("DATABASE_URL"));
        assert!(!base_env().contains_key("GITHUB_TOKEN"));
    }

    #[test]
    fn provider_options_cannot_override_ownership() {
        for key in [
            "run",
            "depends",
            "auto",
            "env",
            "task",
            "project",
            "proxy_idle_timeout",
        ] {
            let p = provider(
                "test",
                toml::Table::from_iter([(key.into(), "unexpected".into())]),
            );
            assert!(
                p.daemon()
                    .unwrap_err()
                    .to_string()
                    .contains("does not accept")
            );
        }
    }

    #[test]
    fn provider_ports_are_stable_and_use_nonzero_slots() {
        let p = provider("stable", toml::Table::new());
        let first = p.daemon().unwrap().port.unwrap();
        assert_eq!(first, p.daemon().unwrap().port.unwrap());
        assert!(first.port > first.base);
    }
}
