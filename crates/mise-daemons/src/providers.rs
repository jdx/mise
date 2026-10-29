//! User-owned servers. A provider is never a member of a consumer's lifecycle.
use super::host::ConfigMap;
use super::{Daemon, DaemonSet, ports, presets, runtime};
use eyre::{Result, bail};
use indexmap::IndexMap;
use mise_util::env_diff::EnvMap;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Provider {
    pub name: String,
    pub source: PathBuf,
    pub declaration: toml::Table,
}

pub fn directory(name: &str) -> PathBuf {
    mise_util::dirs::STATE.join("daemon-providers").join(name)
}

pub fn load(files: &ConfigMap) -> Result<IndexMap<String, Provider>> {
    load_selected(files, None)
}

pub fn load_selected(
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
            if !super::host::is_global_config(cf.get_path()) {
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
    pub fn id(&self) -> String {
        format!("mise-provider-{}/server", self.name)
    }

    pub fn daemon(&self) -> Result<Daemon> {
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
        if preset == "nats" {
            super::providers_nats::configure(&mut table, &root)?;
        }
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

    /// Management follows the user's global configuration, never a consumer's
    /// tools or env. Persist only connection identity, not the user's environment.
    pub async fn runtime(&self) -> Result<runtime::Runtime> {
        let root = directory(&self.name);
        let previous = runtime::read_state(&root)?;
        let mut rt =
            super::host::global_runtime(previous.bin.is_file().then_some(previous.bin)).await?;
        let path = root.join("supervisor.json");
        if path.is_file() {
            let identity: EnvMap = serde_json::from_slice(&std::fs::read(path)?)?;
            rt.env.extend(identity);
        }
        Ok(rt)
    }

    pub async fn install(&self) -> Result<()> {
        super::host::install_provider_tools(&self.name, &self.source, &self.daemon()?).await
    }

    pub fn ensure_runnable_as_user(&self) -> Result<()> {
        match self.daemon()?.preset {
            Some(preset) => {
                presets::ensure_runnable_as_user(&format!("daemon provider {}", self.name), &preset)
            }
            None => Ok(()),
        }
    }

    pub async fn prepare(&self, rt: &runtime::Runtime, force_registration: bool) -> Result<()> {
        let root = directory(&self.name);
        std::fs::create_dir_all(&root)?;
        let _lock = mise_util::lock_file::LockFile::at(&root.join("provider.lock")).lock()?;
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
        let mut env = base_env();
        let mut paths = super::host::provider_tool_paths(&self.name, &self.source, &daemon).await?;
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
            data_dir: daemon.data_dir.clone().expect("provider data"),
            jetstream: self
                .declaration
                .get("options")
                .and_then(|v| v.get("jetstream"))
                .and_then(toml::Value::as_bool)
                .unwrap_or(true),
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
        if execution.preset == "nats" && !rt.active(&root, &runtime::read_state(&root)?).await? {
            let _resources =
                mise_util::lock_file::LockFile::at(&root.join("resources.lock")).lock()?;
            super::providers_nats::prepare(&execution).await?;
        }
        runtime::write_if_changed(&manifest, &bytes)?;
        for key in execution.commands.keys() {
            daemon.table.insert(
                key.clone(),
                format!(
                    "{} daemons __provider-exec {} {}",
                    presets::quote(mise_util::env::MISE_BIN.to_string_lossy()),
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
            .or_else(|| mise_util::env::PRISTINE_ENV.get(key).cloned())
    };
    let home =
        value("HOME").unwrap_or_else(|| mise_util::dirs::HOME.to_string_lossy().into_owned());
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
        mise_util::dirs::HOME.to_string_lossy().into_owned(),
    );
    env.insert("LANG".into(), "C.UTF-8".into());
    env.insert("MISE_EXPERIMENTAL".into(), "1".into());
    for (key, value) in [
        ("MISE_STATE_DIR", &*mise_util::dirs::STATE),
        ("MISE_DATA_DIR", &*mise_util::dirs::DATA),
        ("MISE_CONFIG_DIR", &*mise_util::dirs::CONFIG),
    ] {
        env.insert(key.into(), value.to_string_lossy().into_owned());
    }
    env
}

#[derive(Serialize, Deserialize)]
pub struct Execution {
    // Older provider manifests still serve process and probe execution.
    #[serde(default)]
    pub preset: String,
    #[serde(default)]
    pub port: u16,
    pub env: EnvMap,
    pub commands: IndexMap<String, String>,
    pub root: PathBuf,
    #[serde(default)]
    pub data_dir: PathBuf,
    #[serde(default)]
    pub jetstream: bool,
}

impl Execution {
    // Resource metadata may grow across releases without changing the server.
    pub fn matches_process(&self, previous: &[u8]) -> bool {
        serde_json::from_slice::<Self>(previous).is_ok_and(|old| {
            self.env == old.env && self.commands == old.commands && self.root == old.root
        })
    }
}

#[derive(Debug, Clone)]
pub struct Binding {
    pub provider: Provider,
    pub resource: String,
}

pub fn validate_resource(resource: &str) -> Result<()> {
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
    format!("mise_{}", mise_util::hash::hash_to_str(&(root, name)))
}

pub fn binding(
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
    let mut exports = if preset == "nats" {
        export_provider.daemon()?.exports
    } else {
        let options = export_provider
            .declaration
            .entry("options".to_string())
            .or_insert_with(|| toml::Value::Table(toml::Table::new()))
            .as_table_mut()
            .ok_or_else(|| eyre::eyre!("provider options must be a table"))?;
        options.insert("database".into(), resource.clone().into());
        export_provider.daemon()?.exports
    };
    if preset == "nats" {
        let port = provider.daemon()?.port.expect("provider port").port;
        exports.insert(
            "NATS_URL".into(),
            super::providers_nats::url(&directory(&provider.name), &resource, port)?,
        );
    }
    let command = format!(
        "{} daemons __resource {} {}",
        presets::quote(mise_util::env::MISE_BIN.to_string_lossy()),
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

pub async fn install_set(set: &DaemonSet) -> Result<()> {
    let mut seen = std::collections::HashSet::new();
    for binding in set.daemons.values().filter_map(|d| d.provider.as_ref()) {
        if seen.insert(&binding.provider.name) {
            binding.provider.install().await?;
        }
    }
    Ok(())
}

pub async fn prepare_set(
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

#[cfg(test)]
mod tests {
    use super::*;

    fn provider(name: &str, extra: toml::Table) -> Provider {
        let mut declaration = toml::toml! { preset = "postgres" version = "18" };
        declaration.extend(extra);
        Provider {
            name: name.into(),
            source: crate::host::global_config_path(),
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
