//! Project daemons: custom pitchfork definitions and embedded database presets.
pub(crate) mod hook_env;
pub(crate) mod presets;
pub(crate) mod runtime;

use crate::config::env_directive::EnvDirective;
use crate::config::{ConfigMap, Settings};
use crate::toolset::{ToolRequest, ToolRequestSet, ToolSource};
use eyre::{Result, bail};
use indexmap::IndexMap;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub(crate) enum Declaration {
    Preset(String),
    Definition(toml::Table),
}

#[derive(Debug, Clone)]
pub(crate) struct Daemon {
    pub name: String,
    pub source: PathBuf,
    pub root: PathBuf,
    pub table: toml::Table,
    pub preset: Option<String>,
    pub tool: Option<(String, String)>,
    pub exports: IndexMap<String, String>,
}

#[derive(Debug, Default, Clone)]
pub(crate) struct DaemonSet {
    pub daemons: IndexMap<String, Daemon>,
}

pub(crate) fn state_dir(root: &Path) -> PathBuf {
    crate::dirs::STATE
        .join("daemons")
        .join(crate::hash::hash_to_str(
            &root.canonicalize().unwrap_or_else(|_| root.to_path_buf()),
        ))
}

pub(crate) fn load(files: &ConfigMap) -> Result<DaemonSet> {
    static WARNED: AtomicBool = AtomicBool::new(false);
    let mut declarations = IndexMap::new();
    for cf in files.values().rev() {
        let entries = cf.daemon_declarations();
        if entries.is_empty() {
            continue;
        }
        if !Settings::get().experimental {
            if !WARNED.swap(true, Ordering::Relaxed) {
                warn!("[daemons] requires experimental = true; ignoring daemon declarations");
            }
            continue;
        }
        if Settings::safe_mode() && !crate::config::is_global_config(cf.get_path()) {
            continue;
        }
        for (name, declaration) in entries {
            declarations.insert(
                name,
                (
                    declaration,
                    cf.get_path().to_path_buf(),
                    cf.project_root().unwrap_or_else(|| cf.config_root()),
                ),
            );
        }
    }
    let mut set = DaemonSet::default();
    for (name, (declaration, source, root)) in declarations {
        if name.is_empty()
            || name == "."
            || name.contains("..")
            || name.contains("--")
            || name.starts_with('-')
            || name.ends_with('-')
            || !name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
        {
            bail!("invalid daemon name {name:?}; use letters, numbers, '.', '_' or '-'");
        }
        let (preset, version, mut table) = match declaration {
            Declaration::Preset(version) => (Some(name.clone()), Some(version), toml::Table::new()),
            Declaration::Definition(mut table) => {
                let preset = take_string(&mut table, "preset")?;
                let version = take_string(&mut table, "version")?;
                (preset, version, table)
            }
        };
        let daemon = if let Some(preset) = preset {
            let version = version
                .ok_or_else(|| eyre::eyre!("[daemons.{name}] requires version with preset"))?;
            presets::expand(&name, &preset, &version, table, &source, &root)?
        } else {
            if version.is_some() || table.contains_key("options") {
                bail!("[daemons.{name}] requires preset when specifying version or options");
            }
            if table.get("run").and_then(toml::Value::as_str).is_none() {
                bail!("[daemons.{name}] requires run or preset");
            }
            if let Some(port) = table.get("port").and_then(toml::Value::as_integer) {
                if !(1..=65535).contains(&port) {
                    bail!("daemon port must be an integer from 1 to 65535");
                }
                table.insert(
                    "port".into(),
                    toml::Value::Table(toml::Table::from_iter([
                        (
                            "expect".into(),
                            toml::Value::Array(vec![toml::Value::Integer(port)]),
                        ),
                        ("bump".into(), toml::Value::Boolean(false)),
                    ])),
                );
            }
            table
                .entry("mise".to_string())
                .or_insert(toml::Value::Boolean(true));
            Daemon {
                name: name.clone(),
                source,
                root,
                table,
                preset: None,
                tool: None,
                exports: IndexMap::new(),
            }
        };
        set.daemons.insert(name, daemon);
    }
    Ok(set)
}

fn take_string(table: &mut toml::Table, key: &str) -> Result<Option<String>> {
    table
        .remove(key)
        .map(|v| {
            v.as_str()
                .map(str::to_string)
                .ok_or_else(|| eyre::eyre!("daemon {key} must be a string"))
        })
        .transpose()
}

impl DaemonSet {
    pub(crate) fn add_tool_requests(&self, trs: &mut ToolRequestSet) -> Result<()> {
        for daemon in self.daemons.values() {
            let Some((tool, version)) = &daemon.tool else {
                continue;
            };
            let ba = crate::cli::args::BackendArg::from(tool.as_str());
            if let Some(existing) = trs.tools.get(&ba) {
                // Explicit declarations are validated against backend resolution before starting.
                if existing.iter().all(|tr| tr.source().is_mise_toml_daemon())
                    && existing.iter().any(|tr| tr.version() != *version)
                {
                    bail!(
                        "daemon {} requests {tool}@{version}, conflicting with another daemon; use one version per tool",
                        daemon.name
                    );
                }
                continue;
            }
            let source = ToolSource::MiseTomlDaemon(daemon.source.clone());
            let request = ToolRequest::new(ba.into(), version, source.clone())?;
            trs.add_version(request, &source);
        }
        Ok(())
    }

    pub(crate) fn env_entries(&self) -> Vec<(EnvDirective, PathBuf)> {
        self.daemons
            .values()
            .flat_map(|daemon| {
                daemon.exports.iter().map(|(k, v)| {
                    (
                        EnvDirective::Val(k.clone(), v.clone(), Default::default()),
                        daemon.source.clone(),
                    )
                })
            })
            .collect()
    }

    pub(crate) fn roots(&self) -> Vec<PathBuf> {
        self.daemons
            .values()
            .map(|d| d.root.clone())
            .collect::<indexmap::IndexSet<_>>()
            .into_iter()
            .collect()
    }

    pub(crate) fn for_root(&self, root: &Path) -> Self {
        Self {
            daemons: self
                .daemons
                .iter()
                .filter(|(_, d)| d.root == root)
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        }
    }

    pub(crate) fn auto(&self) -> bool {
        self.daemons.values().any(|d| {
            d.table
                .get("auto")
                .and_then(toml::Value::as_array)
                .is_some_and(|a| !a.is_empty())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::config_file::mise_toml::MiseToml;
    use std::sync::Arc;

    fn files(entries: &[(&str, &str)]) -> ConfigMap {
        entries
            .iter()
            .map(|(path, body)| {
                let path = PathBuf::from(path);
                let cf: Arc<dyn crate::config::config_file::ConfigFile> =
                    Arc::new(MiseToml::from_str(body, &path).unwrap());
                (path, cf)
            })
            .collect()
    }

    #[tokio::test]
    async fn daemons_merge_before_expanding_tools_and_exports() {
        crate::toolset::install_state::init().await.unwrap();
        let config = files(&[
            (
                "/parent/child/mise.toml",
                "[daemons.postgres]\nrun = 'echo custom'\n",
            ),
            (
                "/parent/mise.toml",
                "[daemons]\npostgres = '18'\nredis = '8'\n",
            ),
        ]);
        let set = load(&config).unwrap();
        assert!(set.daemons["postgres"].tool.is_none());
        assert!(set.daemons["postgres"].exports.is_empty());
        assert_eq!(
            set.daemons["postgres"].table["run"].as_str(),
            Some("echo custom")
        );
        let mut requests = ToolRequestSet::default();
        set.add_tool_requests(&mut requests).unwrap();
        assert_eq!(requests.tools.len(), 1);
        assert!(
            requests
                .sources
                .values()
                .next()
                .unwrap()
                .is_mise_toml_daemon()
        );
    }

    #[tokio::test]
    async fn preset_names_and_version_conflicts_are_reported() {
        crate::toolset::install_state::init().await.unwrap();
        let config = files(&[("/project/mise.toml", "[daemons]\nunknown = '1'\n")]);
        assert!(
            load(&config)
                .unwrap_err()
                .to_string()
                .contains("available presets")
        );
        let config = files(&[(
            "/project/mise.toml",
            "[daemons]\npostgres = '18'\n[daemons.analytics]\npreset = 'postgres'\nversion = '17'\n",
        )]);
        let set = load(&config).unwrap();
        assert!(
            set.add_tool_requests(&mut ToolRequestSet::default())
                .is_err()
        );
    }

    #[test]
    fn preset_tool_override_is_used_and_not_forwarded() {
        let config = files(&[(
            "/project/mise.toml",
            "[daemons.cache]\npreset = 'redis'\nversion = '8'\ntool = 'github:example/redis'\n",
        )]);
        let set = load(&config).unwrap();
        assert_eq!(
            set.daemons["cache"].tool.as_ref().unwrap().0,
            "github:example/redis"
        );
        assert!(!set.daemons["cache"].table.contains_key("tool"));
        let config = files(&[(
            "/project/mise.toml",
            "[daemons.cache]\npreset = 'redis'\nversion = '8'\ntool = 42\n",
        )]);
        assert!(
            load(&config)
                .unwrap_err()
                .to_string()
                .contains("tool must be a string")
        );
    }

    #[test]
    fn custom_daemons_accept_integer_ports() {
        let config = files(&[(
            "/project/mise.toml",
            "[daemons.api]\nrun = 'server'\nport = 3000\n",
        )]);
        let set = load(&config).unwrap();
        assert_eq!(
            set.daemons["api"].table["port"]["expect"][0].as_integer(),
            Some(3000)
        );
    }

    #[test]
    fn daemon_names_cannot_escape_persistent_data_directory() {
        for name in [".", "..", "../outside", "a/b", "a\\b"] {
            let config = files(&[(
                "/project/mise.toml",
                &format!(
                    "[daemons.{}]\npreset = 'postgres'\nversion = '18'",
                    toml::Value::String(name.into())
                ),
            )]);
            assert!(load(&config).is_err());
        }
    }
}
