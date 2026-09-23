//! User-owned servers. A provider is never a member of a consumer's lifecycle.
use super::{Daemon, DaemonSet, ports, presets, runtime};
use crate::config::config_file::mise_toml::MiseToml;
use crate::config::{Config, ConfigMap};
use crate::env_diff::EnvMap;
use eyre::{Result, bail};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
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
    load_selected(files, None)
}

pub(crate) fn load_selected(
    files: &ConfigMap,
    names: Option<&std::collections::HashSet<String>>,
) -> Result<IndexMap<String, Provider>> {
    let mut providers = IndexMap::new();
    for cf in files.values().rev() {
        let declarations = cf.daemon_providers();
        if declarations.is_empty() {
            continue;
        }
        for (name, declaration) in declarations {
            if names.is_some_and(|names| !names.contains(&name)) {
                continue;
            }
            if !crate::config::is_global_config(cf.get_path()) {
                bail!(
                    "[daemon_providers] belongs in global mise configuration, not {}",
                    cf.get_path().display()
                );
            }
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
        for value in [
            Some(version.as_str()),
            table.get("tool").and_then(toml::Value::as_str),
        ]
        .into_iter()
        .flatten()
        {
            if value.contains("{{") || value.contains("{%") {
                bail!(
                    "provider versions and tools must be literal, not consumer-dependent templates"
                );
            }
        }
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

    /// Management follows the user's global configuration, never a consumer's
    /// tools or env. Persist only connection identity, not the user's environment.
    pub(crate) async fn runtime(&self) -> Result<runtime::Runtime> {
        let config = Config::get().await?;
        let files = config
            .config_files
            .iter()
            .filter(|(path, _)| crate::config::is_global_config(path))
            .map(|(path, cf)| (path.clone(), cf.clone()))
            .collect();
        let global = Config::load_from_config_files(files, true).await?;
        let (_, ts) = runtime::toolset(&global, false).await?;
        let root = directory(&self.name);
        let previous = runtime::read_state(&root)?;
        let mut rt = if previous.bin.is_file() {
            runtime::Runtime {
                bin: previous.bin,
                env: ts.env_with_path(&global).await?,
            }
        } else {
            runtime::Runtime::from_toolset(&global, &ts, which::which("pitchfork").ok().as_deref())
                .await?
        };
        let path = root.join("supervisor.json");
        if path.is_file() {
            let identity: EnvMap = serde_json::from_slice(&std::fs::read(path)?)?;
            rt.env.extend(identity);
        }
        Ok(rt)
    }

    pub(crate) async fn install(&self) -> Result<()> {
        let daemon = self.daemon()?;
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
        Ok(())
    }

    pub(crate) async fn prepare(
        &self,
        rt: &runtime::Runtime,
        force_registration: bool,
    ) -> Result<()> {
        let root = directory(&self.name);
        std::fs::create_dir_all(&root)?;
        let _lock = crate::lock_file::LockFile::at(&root.join("provider.lock")).lock()?;
        let connection = root.join("supervisor.json");
        let owner = Box::pin(self.runtime()).await?;
        let identity = supervisor_identity(&owner.env);
        if supervisor_identity(&rt.env) != identity {
            bail!(
                "provider {} belongs to the user's supervisor; remove the consumer's Pitchfork directory overrides",
                self.name
            );
        }
        runtime::write_if_changed(&connection, &serde_json::to_vec(&identity)?)?;
        let rt = &owner;
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
        let config = self.tool_config(&daemon).await?;
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
            preset: daemon.preset.clone().expect("provider preset"),
            port: daemon.port.expect("provider port").port,
            env,
            commands,
            root: root.clone(),
        };
        let bytes = serde_json::to_vec_pretty(&execution)?;
        if std::fs::read(&manifest).is_ok_and(|old| !execution.matches_process(&old))
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
        Box::pin(rt.prepare(&root, &set, force_registration, true, &["server".into()])).await?;
        runtime::write_if_changed(&path, &desired)?;
        Ok(())
    }
}

fn supervisor_identity(env: &EnvMap) -> EnvMap {
    let value = |key: &str| {
        env.get(key)
            .cloned()
            .or_else(|| crate::env::PRISTINE_ENV.get(key).cloned())
    };
    let home = value("HOME").unwrap_or_else(|| crate::dirs::HOME.to_string_lossy().into_owned());
    let state = value("PITCHFORK_STATE_DIR").unwrap_or_else(|| {
        let base = value("XDG_STATE_HOME").unwrap_or_else(|| format!("{home}/.local/state"));
        format!("{base}/pitchfork")
    });
    let config =
        value("PITCHFORK_CONFIG_DIR").unwrap_or_else(|| format!("{home}/.config/pitchfork"));
    EnvMap::from_iter([
        ("HOME".into(), home),
        ("PITCHFORK_STATE_DIR".into(), state),
        ("PITCHFORK_CONFIG_DIR".into(), config),
    ])
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
    // Older provider manifests still serve process and probe execution.
    #[serde(default)]
    preset: String,
    #[serde(default)]
    port: u16,
    env: EnvMap,
    commands: IndexMap<String, String>,
    root: PathBuf,
}

impl Execution {
    // Resource metadata may grow across releases without changing the server.
    fn matches_process(&self, previous: &[u8]) -> bool {
        serde_json::from_slice::<Self>(previous).is_ok_and(|old| {
            self.env == old.env && self.commands == old.commands && self.root == old.root
        })
    }
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
                    let declaration = serde_json::from_slice(&bytes).unwrap_or_else(|_| {
                        warn!("invalid saved provider definition {}; lifecycle cleanup remains available", path.display());
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
        let mut rows = vec![];
        for provider in providers
            .values()
            .filter(|p| names.is_empty() || names.contains(&p.name))
        {
            let root = directory(&provider.name);
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

#[derive(Debug, Clone)]
pub(crate) struct Binding {
    pub provider: Provider,
    pub resource: String,
}

fn validate_resource(resource: &str) -> Result<()> {
    if resource.is_empty()
        || resource.len() > 63
        || !resource.starts_with(|c: char| c.is_ascii_lowercase())
        || !resource
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    {
        bail!(
            "resource names must begin with a lowercase letter and contain at most 63 lowercase letters, digits or underscores"
        );
    }
    Ok(())
}

fn resource_name(root: &Path, name: &str) -> String {
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    format!("mise_{}", crate::hash::hash_to_str(&(root, name)))
}

pub(crate) fn binding(
    providers: &IndexMap<String, Provider>,
    name: &str,
    mut table: toml::Table,
    source: PathBuf,
    root: PathBuf,
) -> Result<Daemon> {
    let provider_name = super::take_string(&mut table, "provider")?
        .ok_or_else(|| eyre::eyre!("provider must be a string"))?;
    let resource =
        super::take_string(&mut table, "resource")?.unwrap_or_else(|| resource_name(&root, name));
    validate_resource(&resource)?;
    if let Some(key) = table.keys().next() {
        bail!(
            "[daemons.{name}] cannot override {key:?} alongside provider; configure the server globally"
        );
    }
    let provider = providers
        .get(&provider_name)
        .ok_or_else(|| {
            eyre::eyre!(
                "unknown daemon provider {provider_name:?}; define it in global [daemon_providers]"
            )
        })?
        .clone();
    let mut export_provider = provider.clone();
    let preset = provider
        .declaration
        .get("preset")
        .and_then(toml::Value::as_str)
        .unwrap_or_default();
    if !matches!(preset, "postgres" | "cockroachdb") {
        bail!("{preset} does not support provider resources yet");
    }
    let options = export_provider
        .declaration
        .entry("options".to_string())
        .or_insert_with(|| toml::Value::Table(toml::Table::new()))
        .as_table_mut()
        .ok_or_else(|| eyre::eyre!("provider options must be a table"))?;
    options.insert("database".into(), resource.clone().into());
    let exports = export_provider.daemon()?.exports;
    let command = format!(
        "{} daemons __resource {} {}",
        presets::quote(crate::env::MISE_BIN.to_string_lossy()),
        presets::quote(directory(&provider.name).to_string_lossy()),
        presets::quote(&resource)
    );
    Ok(Daemon {
        name: name.into(),
        source,
        root,
        table: toml::toml! { run = command mise = false proxy = false ready_output = { pattern = "mise shared resource ready", timeout = "120s" } },
        preset: Some(preset.into()),
        data_dir: None,
        task: None,
        tool: None,
        provider: Some(Binding { provider, resource }),
        exports,
        imported: false,
        port: None,
        host: None,
    })
}

pub(crate) async fn install_set(set: &DaemonSet) -> Result<()> {
    let mut seen = std::collections::HashSet::new();
    for binding in set.daemons.values().filter_map(|d| d.provider.as_ref()) {
        if seen.insert(&binding.provider.name) {
            binding.provider.install().await?;
        }
    }
    Ok(())
}

pub(crate) async fn prepare_set(
    rt: &runtime::Runtime,
    set: &DaemonSet,
    force_registration: bool,
) -> Result<()> {
    let mut seen = std::collections::HashSet::new();
    for binding in set.daemons.values().filter_map(|d| d.provider.as_ref()) {
        if seen.insert(&binding.provider.name) {
            Box::pin(binding.provider.prepare(rt, force_registration)).await?;
        }
    }
    Ok(())
}

/// A project-owned readiness process: it never starts or stops the server itself.
#[derive(Debug, usage_rs::Args)]
pub(crate) struct Resource {
    provider: PathBuf,
    resource: String,
}

impl Resource {
    pub(crate) async fn run(self) -> Result<()> {
        validate_resource(&self.resource)?;
        let root = self.provider;
        let execution: Execution =
            serde_json::from_slice(&std::fs::read(root.join("execution.json"))?)?;
        if !matches!(execution.preset.as_str(), "postgres" | "cockroachdb") || execution.port == 0 {
            bail!(
                "provider metadata predates resource support; explicitly restart its provider first"
            );
        }
        {
            let _lock = crate::lock_file::LockFile::at(&root.join("resources.lock")).lock()?;
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
        // Readiness is output only after provisioning succeeded. Pitchfork owns
        // this small process, so stopping a consumer cannot stop the provider.
        println!("mise shared resource ready");
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
    fn process_comparison_ignores_metadata_but_protects_execution() {
        let old = serde_json::json!({"env": {"PATH": "/bin"}, "commands": {"run": "postgres"}, "root": "/tmp/provider"});
        let bytes = serde_json::to_vec(&old).unwrap();
        let mut desired: Execution = serde_json::from_value(old).unwrap();
        desired.preset = "postgres".into();
        desired.port = 5432;
        assert!(desired.matches_process(&bytes));
        desired.env.insert("INJECTED".into(), "value".into());
        assert!(!desired.matches_process(&bytes));
        desired.env.remove("INJECTED");
        desired.commands.insert("run".into(), "other".into());
        assert!(!desired.matches_process(&bytes));
        assert!(!desired.matches_process(b"{broken"));
    }

    #[test]
    fn old_manifests_remain_readable_for_processes_and_probes() {
        let execution: Execution = serde_json::from_value(serde_json::json!({
            "env": {"PATH": "/bin"}, "commands": {"ready_cmd": "true"}, "root": "/tmp/provider"
        }))
        .unwrap();
        assert_eq!(execution.commands["ready_cmd"], "true");
        assert_eq!(execution.env["PATH"], "/bin");
        assert_eq!(execution.root, PathBuf::from("/tmp/provider"));
    }

    #[test]
    fn provider_ports_are_stable_and_use_nonzero_slots() {
        let p = provider("stable", toml::Table::new());
        let first = p.daemon().unwrap().port.unwrap();
        assert_eq!(first, p.daemon().unwrap().port.unwrap());
        assert!(first.port > first.base);
    }
    #[test]
    fn resource_identity_is_per_checkout_and_daemon() {
        let root = tempfile::tempdir().unwrap();
        let one = resource_name(root.path(), "db");
        assert_eq!(one, resource_name(root.path(), "db"));
        assert_ne!(one, resource_name(root.path(), "other"));
        assert_ne!(one, resource_name(&root.path().join("other"), "db"));
        validate_resource(&one).unwrap();
        for invalid in ["", "../db", "a'b", "A", "a-b", &"a".repeat(64)] {
            assert!(validate_resource(invalid).is_err());
        }
    }

    #[cfg(unix)]
    #[test]
    fn resource_identity_follows_symlinks() {
        let root = tempfile::tempdir().unwrap();
        let real = root.path().join("real");
        let link = root.path().join("link");
        std::fs::create_dir(&real).unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert_eq!(resource_name(&real, "db"), resource_name(&link, "db"));
    }
}
