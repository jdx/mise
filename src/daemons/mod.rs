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

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub(crate) enum Declaration {
    Preset(String),
    Definition(toml::Table),
}

/// `[daemon_groups]` entry: a bare list of members or a table with `daemons`.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub(crate) enum GroupDeclaration {
    List(Vec<String>),
    Table { daemons: Vec<String> },
}

impl GroupDeclaration {
    fn members(&self) -> &[String] {
        match self {
            GroupDeclaration::List(members) => members,
            GroupDeclaration::Table { daemons } => daemons,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Group {
    pub name: String,
    pub source: PathBuf,
    pub root: PathBuf,
    /// Declared members, which may reference other groups in the same project.
    pub members: Vec<String>,
    /// Members expanded to daemon names declared in the same project root.
    pub daemons: Vec<String>,
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
    /// Group names are project scoped, so nested projects may each declare one
    /// with the same name. They stay in a root-aware list until `for_root`.
    pub groups: Vec<Group>,
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
    let mut group_declarations = IndexMap::new();
    for cf in files.values().rev() {
        let entries = cf.daemon_declarations();
        let groups = cf.daemon_group_declarations();
        if entries.is_empty() && groups.is_empty() {
            continue;
        }
        if !Settings::get().experimental {
            warn_once!("[daemons] requires experimental = true; ignoring daemon declarations");
            continue;
        }
        if Settings::safe_mode() && !crate::config::is_global_config(cf.get_path()) {
            continue;
        }
        let source = cf.get_path().to_path_buf();
        let root = cf.project_root().unwrap_or_else(|| cf.config_root());
        for (name, declaration) in entries {
            declarations.insert(name, (declaration, source.clone(), root.clone()));
        }
        for (name, declaration) in groups {
            // Keyed by root so a child project's group never displaces a parent's.
            group_declarations.insert(
                (root.clone(), name),
                (declaration, source.clone(), root.clone()),
            );
        }
    }
    let mut set = DaemonSet::default();
    for (name, (declaration, source, root)) in declarations {
        validate_name("daemon", &name)?;
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
    load_groups(&mut set, group_declarations)?;
    Ok(set)
}

fn validate_name(kind: &str, name: &str) -> Result<()> {
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
        bail!("invalid {kind} name {name:?}; use letters, numbers, '.', '_' or '-'");
    }
    Ok(())
}

type GroupKey = (PathBuf, String);
type GroupDeclarations = IndexMap<GroupKey, (GroupDeclaration, PathBuf, PathBuf)>;

/// Groups are project scoped: every member resolves to a daemon declared under the
/// same project root, so a group can never select daemons outside the project.
fn load_groups(set: &mut DaemonSet, declarations: GroupDeclarations) -> Result<()> {
    let mut groups: IndexMap<GroupKey, Group> = IndexMap::new();
    for (key, (declaration, source, root)) in declarations {
        let name = key.1.clone();
        validate_name("daemon group", &name)?;
        if let Some(daemon) = set.daemons.get(&name)
            && daemon.root == root
        {
            bail!("[daemon_groups.{name}] conflicts with the daemon of the same name");
        }
        let members = declaration.members().to_vec();
        if members.is_empty() {
            bail!("[daemon_groups.{name}] requires at least one daemon or group");
        }
        groups.insert(
            key,
            Group {
                name,
                source,
                root,
                members,
                daemons: Vec::new(),
            },
        );
    }
    for key in groups.keys().cloned().collect::<Vec<_>>() {
        let daemons = expand_group(set, &groups, &key, &mut Vec::new())?;
        groups[&key].daemons = daemons;
    }
    set.groups = groups.into_values().collect();
    Ok(())
}

fn expand_group(
    set: &DaemonSet,
    groups: &IndexMap<GroupKey, Group>,
    key: &GroupKey,
    seen: &mut Vec<String>,
) -> Result<Vec<String>> {
    let name = &key.1;
    if seen.iter().any(|s| s == name) {
        seen.push(name.clone());
        bail!(
            "[daemon_groups.{name}] references itself: {}",
            seen.join(" -> ")
        );
    }
    seen.push(name.clone());
    let group = &groups[key];
    let mut expanded: Vec<String> = Vec::new();
    for member in &group.members {
        if let Some(daemon) = set.daemons.get(member)
            && daemon.root == group.root
        {
            if !expanded.contains(member) {
                expanded.push(member.clone());
            }
            continue;
        }
        // Nested members resolve within the declaring project only.
        let nested = (group.root.clone(), member.clone());
        if groups.contains_key(&nested) {
            for daemon in expand_group(set, groups, &nested, seen)? {
                if !expanded.contains(&daemon) {
                    expanded.push(daemon);
                }
            }
            continue;
        }
        bail!(
            "{}: [daemon_groups.{name}] member {member:?} is not a daemon or group declared for {}",
            group.source.display(),
            group.root.display()
        );
    }
    seen.pop();
    Ok(expanded)
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
            groups: self
                .groups
                .iter()
                .filter(|g| g.root == root)
                .cloned()
                .collect(),
        }
    }

    /// The group named `name`. Call on a root-scoped set, where names are unique.
    pub(crate) fn group(&self, name: &str) -> Option<&Group> {
        self.groups.iter().find(|g| g.name == name)
    }

    /// Daemon names a declared group expands to, or None when `name` is not a group.
    pub(crate) fn expand(&self, name: &str) -> Option<&[String]> {
        self.group(name).map(|g| g.daemons.as_slice())
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
    fn groups_expand_members_and_nested_groups() {
        let set = load(&files(&[(
            "/project/mise.toml",
            r#"
[daemons.postgres]
run = 'postgres'
[daemons.nats]
run = 'nats'
[daemons.core]
run = 'core'
[daemons.core2]
run = 'core2'
[daemons.node0]
run = 'node0'
[daemon_groups]
default = ["postgres", "nats", "core", "node0"]
two-cluster = ["default", "core2"]
[daemon_groups.explicit]
daemons = ["core", "core2"]
"#,
        )]))
        .unwrap();
        assert_eq!(
            set.group("default").unwrap().daemons,
            ["postgres", "nats", "core", "node0"]
        );
        // A nested group expands in place and members are deduplicated.
        assert_eq!(
            set.group("two-cluster").unwrap().daemons,
            ["postgres", "nats", "core", "node0", "core2"]
        );
        assert_eq!(set.group("explicit").unwrap().daemons, ["core", "core2"]);
        assert_eq!(set.expand("default").unwrap().len(), 4);
        assert!(set.expand("postgres").is_none());
        assert_eq!(set.for_root(Path::new("/project")).groups.len(), 3);
        assert!(set.for_root(Path::new("/other")).groups.is_empty());
    }

    #[test]
    fn same_group_name_in_two_projects_is_kept() {
        let set = load(&files(&[
            (
                "/parent/child/mise.toml",
                "[daemons.web]\nrun = 'web'\n[daemon_groups]\ndefault = ['web']\n",
            ),
            (
                "/parent/mise.toml",
                "[daemons.api]\nrun = 'api'\n[daemon_groups]\ndefault = ['api']\n",
            ),
        ]))
        .unwrap();
        assert_eq!(set.groups.len(), 2);
        assert_eq!(
            set.for_root(Path::new("/parent/child"))
                .group("default")
                .unwrap()
                .daemons,
            ["web"]
        );
        assert_eq!(
            set.for_root(Path::new("/parent"))
                .group("default")
                .unwrap()
                .daemons,
            ["api"]
        );
    }

    #[test]
    fn nested_group_references_stay_inside_one_project() {
        // `shared` exists only in the parent, so the child cannot reference it.
        let err = load(&files(&[
            (
                "/parent/child/mise.toml",
                "[daemons.web]\nrun = 'web'\n[daemon_groups]\nall = ['web', 'shared']\n",
            ),
            (
                "/parent/mise.toml",
                "[daemons.api]\nrun = 'api'\n[daemon_groups]\nshared = ['api']\n",
            ),
        ]))
        .unwrap_err()
        .to_string();
        assert!(
            err.contains("is not a daemon or group declared for"),
            "{err}"
        );
    }

    #[test]
    fn groups_are_scoped_to_one_project_root() {
        // A daemon declared by an outer project is not a valid member of an inner group.
        let err = load(&files(&[
            (
                "/parent/child/mise.toml",
                "[daemons.api]\nrun = 'api'\n[daemon_groups]\nall = ['api', 'outer']\n",
            ),
            ("/parent/mise.toml", "[daemons.outer]\nrun = 'outer'\n"),
        ]))
        .unwrap_err()
        .to_string();
        assert!(
            err.contains("is not a daemon or group declared for"),
            "{err}"
        );
    }

    #[test]
    fn invalid_groups_are_rejected() {
        let cases = [
            (
                "[daemons.api]\nrun = 'api'\n[daemon_groups]\nweb = ['missing']\n",
                "is not a daemon or group",
            ),
            (
                "[daemons.api]\nrun = 'api'\n[daemon_groups]\nweb = []\n",
                "requires at least one daemon or group",
            ),
            (
                "[daemons.api]\nrun = 'api'\n[daemon_groups]\napi = ['api']\n",
                "conflicts with the daemon of the same name",
            ),
            (
                "[daemons.api]\nrun = 'api'\n[daemon_groups]\nweb = ['other']\nother = ['web']\n",
                "references itself",
            ),
            (
                "[daemons.api]\nrun = 'api'\n[daemon_groups]\nweb = ['web']\n",
                "references itself",
            ),
            (
                "[daemons.api]\nrun = 'api'\n[daemon_groups]\n'bad name' = ['api']\n",
                "invalid daemon group name",
            ),
        ];
        for (body, expected) in cases {
            let err = load(&files(&[("/project/mise.toml", body)]))
                .unwrap_err()
                .to_string();
            assert!(err.contains(expected), "{body:?} produced {err}");
        }
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
