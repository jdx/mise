//! Project daemons: custom pitchfork definitions and embedded database presets.
pub(crate) mod hook_env;
pub(crate) mod ports;
pub(crate) mod presets;
pub(crate) mod runtime;

use crate::config::env_directive::EnvDirective;
use crate::config::{ConfigMap, Settings};
use crate::toolset::{ToolRequest, ToolRequestSet, ToolSource};
use eyre::{Result, bail};
use indexmap::IndexMap;
use ports::{PortClaim, PortRequest};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

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
    /// The resolved allocation for a `port = "auto"` daemon, persisted so it
    /// survives a later change to the slot derivation.
    pub port: Option<PortClaim>,
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
    let mut declarations = IndexMap::new();
    for cf in files.values().rev() {
        let entries = cf.daemon_declarations();
        if entries.is_empty() {
            continue;
        }
        if !Settings::get().experimental {
            warn_once!("[daemons] requires experimental = true; ignoring daemon declarations");
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
    // Previously persisted allocations, read once per project root.
    let mut claims: BTreeMap<PathBuf, BTreeMap<String, PortClaim>> = BTreeMap::new();
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
        let request = table
            .remove("port")
            .map(|value| ports::parse(&name, value))
            .transpose()?;
        // `auto` resolution reuses the allocation recorded by the last start, so
        // a change to the slot derivation cannot move a running daemon's port.
        let persisted = if matches!(request, Some(PortRequest::Auto { .. })) {
            claims
                .entry(root.clone())
                .or_insert_with(|| {
                    runtime::read_state(&root)
                        .map(|s| s.ports)
                        .unwrap_or_default()
                })
                .get(&name)
                .copied()
        } else {
            None
        };
        let daemon = if let Some(preset) = preset {
            let version = version
                .ok_or_else(|| eyre::eyre!("[daemons.{name}] requires version with preset"))?;
            let claim = match request {
                Some(PortRequest::Passthrough(_)) => bail!(
                    "[daemons.{name}].port must be an integer or \"auto\"; \
                     pitchfork's structured port is only available on custom daemons"
                ),
                Some(PortRequest::Fixed(port)) => Some(PortClaim::fixed(port)),
                Some(PortRequest::Auto { base, stride }) => Some(ports::resolve(
                    &name,
                    &root,
                    base,
                    stride,
                    Some(presets::default_port(&preset)?),
                    persisted,
                )?),
                None => None,
            };
            presets::expand(&name, &preset, &version, table, &source, &root, claim)?
        } else {
            if version.is_some() || table.contains_key("options") {
                bail!("[daemons.{name}] requires preset when specifying version or options");
            }
            if table.get("run").and_then(toml::Value::as_str).is_none() {
                bail!("[daemons.{name}] requires run or preset");
            }
            let claim = match request {
                Some(PortRequest::Passthrough(value)) => {
                    table.insert("port".into(), value);
                    None
                }
                Some(PortRequest::Fixed(port)) => Some(PortClaim::fixed(port)),
                Some(PortRequest::Auto { base, stride }) => {
                    Some(ports::resolve(&name, &root, base, stride, None, persisted)?)
                }
                None => None,
            };
            if let Some(claim) = claim {
                table.insert("port".into(), expected_port(claim.port));
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
                // Without this a custom daemon's port would reach pitchfork and
                // nothing else: `mise env` would export nothing, and the process
                // could only discover it through pitchfork's own injection.
                exports: claim
                    .map(|c| IndexMap::from([(port_env_var(&name), c.port.to_string())]))
                    .unwrap_or_default(),
                port: claim,
            }
        };
        set.daemons.insert(name, daemon);
    }
    Ok(set)
}

/// The variable a custom daemon's resolved port is exported as, so `mise env`,
/// `mise x`, and the daemon's own process all see one endpoint. Presets export
/// their tool's conventional variables instead.
fn port_env_var(name: &str) -> String {
    let base: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect();
    format!("{base}_PORT")
}

/// Pitchfork's structured `port`, pinned to the port mise already rendered into
/// the daemon's command line and `[env]` exports.
pub(crate) fn expected_port(port: u16) -> toml::Value {
    toml::Value::Table(toml::Table::from_iter([
        (
            "expect".into(),
            toml::Value::Array(vec![toml::Value::Integer(i64::from(port))]),
        ),
        ("bump".into(), toml::Value::Boolean(false)),
    ]))
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
    fn preset_run_override_keeps_initialization() {
        let config = files(&[(
            "/project/mise.toml",
            "[daemons.db]\npreset = 'postgres'\nversion = '18'\nrun = 'echo starting && exec postgres -D /data'\n",
        )]);
        let set = load(&config).unwrap();
        let run = set.daemons["db"].table["run"].as_str().unwrap();
        assert!(run.contains("daemons __init"));
        assert!(run.ends_with("&& echo starting && exec postgres -D /data"));
        let invalid = files(&[(
            "/project/mise.toml",
            "[daemons.db]\npreset = 'postgres'\nversion = '18'\nrun = 42\n",
        )]);
        assert!(load(&invalid).is_err());
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
    fn auto_ports_separate_worktrees_but_leave_the_primary_checkout_alone() {
        let tmp = tempfile::tempdir().unwrap();
        let primary = tmp.path().join("project");
        std::fs::create_dir_all(primary.join(".git")).unwrap();
        let linked = tmp.path().join("worktree");
        std::fs::create_dir_all(&linked).unwrap();
        // A linked worktree's marker points into the main checkout's worktrees
        // directory, which carries a commondir pointer; that is what
        // distinguishes it from a submodule.
        let private = primary.join(".git").join("worktrees").join("worktree");
        std::fs::create_dir_all(&private).unwrap();
        std::fs::write(primary.join(".git").join("HEAD"), "ref: refs/heads/main\n").unwrap();
        std::fs::write(private.join("commondir"), "../..\n").unwrap();
        std::fs::write(
            linked.join(".git"),
            format!("gitdir: {}\n", private.display()),
        )
        .unwrap();

        let body = "[daemons.db]\npreset = 'postgres'\nversion = '18'\nport = 'auto'\n[daemons.api]\nrun = 'server'\n[daemons.api.port]\nauto = true\nbase = 3000\n";
        let ports = |root: &Path| {
            let set = load(&files(&[(root.join("mise.toml").to_str().unwrap(), body)])).unwrap();
            (
                set.daemons["db"].port.unwrap().port,
                set.daemons["api"].port.unwrap().port,
            )
        };

        // The single well-known checkout keeps the well-known ports.
        assert_eq!(ports(&primary), (5432, 3000));
        let (db, api) = ports(&linked);
        assert!(db > 5432 && db <= 5432 + ports::SLOTS);
        assert!(api > 3000 && api <= 3000 + ports::SLOTS);
        // Same root, same ports; and the port reaches the exports and the command.
        assert_eq!(ports(&linked), (db, api));
        let set = load(&files(&[(
            linked.join("mise.toml").to_str().unwrap(),
            body,
        )]))
        .unwrap();
        assert_eq!(set.daemons["db"].exports["PGPORT"], db.to_string());
        assert!(set.daemons["db"].exports["DATABASE_URL"].contains(&format!(":{db}/")));
        assert!(
            set.daemons["db"].table["run"]
                .as_str()
                .unwrap()
                .contains(&format!("-p {db}"))
        );
        assert_eq!(
            set.daemons["api"].table["port"]["expect"][0].as_integer(),
            Some(i64::from(api))
        );
    }

    #[test]
    fn custom_daemons_export_their_resolved_port() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("project");
        std::fs::create_dir_all(root.join(".git")).unwrap();
        let set = load(&files(&[(
            root.join("mise.toml").to_str().unwrap(),
            "[daemons.api]\nrun = 'server'\nport = 3000\n[daemons.web-ui]\nrun = 'ui'\n[daemons.web-ui.port]\nauto = true\nbase = 4000\n",
        )]))
        .unwrap();
        // Without an export the port would reach pitchfork only.
        assert_eq!(set.daemons["api"].exports["API_PORT"], "3000");
        // Punctuation is not valid in a variable name.
        assert_eq!(set.daemons["web-ui"].exports["WEB_UI_PORT"], "4000");
        // A daemon with no port mise resolved exports nothing.
        let set = load(&files(&[(
            root.join("mise.toml").to_str().unwrap(),
            "[daemons.api]\nrun = 'server'\n",
        )]))
        .unwrap();
        assert!(set.daemons["api"].exports.is_empty());
        // The exports reach the environment mise renders.
        let set = load(&files(&[(
            root.join("mise.toml").to_str().unwrap(),
            "[daemons.api]\nrun = 'server'\nport = 3000\n",
        )]))
        .unwrap();
        assert!(set.env_entries().iter().any(
            |(d, _)| matches!(d, EnvDirective::Val(k, v, _) if k == "API_PORT" && v == "3000")
        ));
    }

    #[test]
    fn a_persisted_allocation_wins_over_a_fresh_derivation() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("wt");
        std::fs::create_dir_all(&root).unwrap();
        let private = tmp.path().join(".git").join("worktrees").join("wt");
        std::fs::create_dir_all(&private).unwrap();
        std::fs::write(
            tmp.path().join(".git").join("HEAD"),
            "ref: refs/heads/main\n",
        )
        .unwrap();
        std::fs::write(private.join("commondir"), "../..\n").unwrap();
        std::fs::write(
            root.join(".git"),
            format!("gitdir: {}\n", private.display()),
        )
        .unwrap();

        let body = "[daemons.api]\nrun = 'server'\n[daemons.api.port]\nauto = true\nbase = 3000\n";
        let cfg = || files(&[(root.join("mise.toml").to_str().unwrap(), body)]);
        let derived = load(&cfg()).unwrap().daemons["api"].port.unwrap();
        assert_ne!(derived.port, 3000, "a worktree is offset");

        // A recorded claim for the same base and stride is what the next load
        // uses, so a started daemon cannot move when derivation changes.
        let state = runtime::State {
            root: root.clone(),
            ports: std::collections::BTreeMap::from([(
                "api".to_string(),
                PortClaim {
                    port: 3456,
                    base: 3000,
                    stride: 1,
                },
            )]),
            ..Default::default()
        };
        let dir = state_dir(&root);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("state.json"),
            serde_json::to_vec_pretty(&state).unwrap(),
        )
        .unwrap();
        let set = load(&cfg()).unwrap();
        assert_eq!(set.daemons["api"].port.unwrap().port, 3456);
        assert_eq!(set.daemons["api"].exports["API_PORT"], "3456");
        assert_eq!(
            set.daemons["api"].table["port"]["expect"][0].as_integer(),
            Some(3456)
        );
    }

    #[test]
    fn invalid_auto_port_declarations_are_rejected() {
        // A custom daemon has no default port to offset.
        let config = files(&[(
            "/project/mise.toml",
            "[daemons.api]\nrun = 'server'\nport = 'auto'\n",
        )]);
        assert!(
            load(&config)
                .unwrap_err()
                .to_string()
                .contains("needs a base port")
        );
        // Presets do not take pitchfork's structured port.
        let config = files(&[(
            "/project/mise.toml",
            "[daemons.db]\npreset = 'postgres'\nversion = '18'\n[daemons.db.port]\nexpect = [5432]\n",
        )]);
        assert!(load(&config).is_err());
        // Custom daemons still forward it verbatim.
        let config = files(&[(
            "/project/mise.toml",
            "[daemons.api]\nrun = 'server'\n[daemons.api.port]\nexpect = [3000, 3001]\nbump = true\n",
        )]);
        let set = load(&config).unwrap();
        assert_eq!(
            set.daemons["api"].table["port"]["bump"].as_bool(),
            Some(true)
        );
        assert!(set.daemons["api"].port.is_none());
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
