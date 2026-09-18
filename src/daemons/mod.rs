//! Project daemons: custom pitchfork definitions and embedded database presets.
pub(crate) mod hook_env;
pub(crate) mod presets;
pub(crate) mod runtime;

use crate::config::config_file::ConfigFile;
use crate::config::env_directive::EnvDirective;
use crate::config::{ConfigMap, Settings};
use crate::toolset::{ToolRequest, ToolRequestSet, ToolSource};
use eyre::{Result, bail};
use indexmap::IndexMap;
use path_absolutize::Absolutize;
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub(crate) enum Declaration {
    Preset(String),
    Definition(toml::Table),
}

/// Project-wide daemon options declared in `[daemons_settings]`.
///
/// Every field is optional so a higher-precedence file can set one key without
/// discarding the others; see [`DaemonSettings::merge`].
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DaemonSettings {
    /// Fixed pitchfork namespace for this project, replacing the hashed default.
    #[serde(default)]
    pub namespace: Option<String>,
    /// Keep linked git worktrees of one repository in separate namespaces.
    #[serde(default)]
    pub namespace_per_worktree: Option<bool>,
}

impl DaemonSettings {
    /// Overlay a higher-precedence file's table onto this one, key by key. A
    /// `mise.local.toml` that sets only `namespace_per_worktree` must not drop
    /// the `namespace` that `mise.toml` established, because other projects
    /// refer to daemons by the qualified ID that namespace produces.
    fn merge(&mut self, other: Self) {
        if other.namespace.is_some() {
            self.namespace = other.namespace;
        }
        if other.namespace_per_worktree.is_some() {
            self.namespace_per_worktree = other.namespace_per_worktree;
        }
    }

    pub(crate) fn namespace_per_worktree(&self) -> bool {
        self.namespace_per_worktree.unwrap_or(true)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Daemon {
    /// Name inside its own project; the key in the owning project's pitchfork config.
    pub name: String,
    pub source: PathBuf,
    pub root: PathBuf,
    pub table: toml::Table,
    pub preset: Option<String>,
    pub tool: Option<(String, String)>,
    pub exports: IndexMap<String, String>,
    /// True when this daemon was declared by another project and pulled in with
    /// `project =`. Its tools and exported environment belong to that project.
    pub imported: bool,
}

#[derive(Debug, Default, Clone)]
pub(crate) struct DaemonSet {
    /// Keyed by local name for own daemons and by qualified ID for imported ones.
    pub daemons: IndexMap<String, Daemon>,
    /// Resolved pitchfork namespace for every project root in this set.
    pub namespaces: IndexMap<PathBuf, String>,
    /// Local key -> qualified ID for imported daemons, so a daemon renamed on
    /// the way in can still be selected by the name this project gave it.
    pub aliases: IndexMap<String, String>,
}

/// Validate a pitchfork identifier component. Daemon names and namespaces reach
/// pitchfork verbatim, which rejects `--` among other things, and both end up in
/// filesystem paths under the state directory.
pub(crate) fn validate_id(kind: &str, value: &str) -> Result<()> {
    if value.is_empty()
        || value == "."
        || value.contains("..")
        || value.contains("--")
        || value.starts_with('-')
        || value.ends_with('-')
        || !value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
    {
        bail!("invalid daemon {kind} {value:?}; use letters, numbers, '.', '_' or '-'");
    }
    Ok(())
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
    let mut settings: IndexMap<PathBuf, DaemonSettings> = IndexMap::new();
    for cf in files.values().rev() {
        let entries = cf.daemon_declarations();
        let file_settings = cf.daemon_settings();
        if entries.is_empty() && file_settings.is_none() {
            continue;
        }
        if !Settings::get().experimental {
            if !entries.is_empty() {
                warn_once!("[daemons] requires experimental = true; ignoring daemon declarations");
            }
            continue;
        }
        if Settings::safe_mode() && !crate::config::is_global_config(cf.get_path()) {
            continue;
        }
        let project_root = cf.project_root();
        let root = project_root.clone().unwrap_or_else(|| cf.config_root());
        if let Some(file_settings) = file_settings {
            match &project_root {
                // Settings are inherited down a project tree, and a global
                // config's root is the home directory. Honouring one there would
                // hand every project on the machine the same namespace, which is
                // the one thing a namespace must never be. It would also be
                // invisible to another project importing from here, which reads
                // project configuration only.
                None => warn_once!(
                    "[daemons_settings] in {} is ignored; it belongs in a project configuration",
                    crate::file::display_path(cf.get_path())
                ),
                Some(project_root) => settings
                    .entry(project_root.clone())
                    .or_default()
                    .merge(file_settings),
            }
        }
        for (name, declaration) in entries {
            declarations.insert(
                name,
                (declaration, cf.get_path().to_path_buf(), root.clone()),
            );
        }
    }
    let mut set = DaemonSet::default();
    // Local name -> qualified ID, so `depends` can name an imported daemon short.
    let mut imported_ids: IndexMap<String, String> = IndexMap::new();
    for (name, (declaration, source, root)) in declarations {
        validate_id("name", &name)?;
        if let Declaration::Definition(table) = &declaration
            && table.contains_key("project")
        {
            let (key, daemon) = import(&name, table.clone(), &source, &root, &mut set.namespaces)?;
            // Two imports can resolve to one qualified ID when their projects
            // share a namespace. Inserting the second would replace the first
            // and leave both local names pointing at the later project.
            if let Some(existing) = set.daemons.get(&key)
                && existing.root != daemon.root
            {
                bail!(
                    "daemon {key} is imported from both {} and {}; they share a namespace, so one of those projects needs its own [daemons_settings] namespace",
                    existing.root.display(),
                    daemon.root.display()
                );
            }
            imported_ids.insert(name.clone(), key.clone());
            set.aliases.insert(name, key.clone());
            set.daemons.insert(key, daemon);
            continue;
        }
        set.daemons
            .insert(name.clone(), build(&name, declaration, source, root)?);
    }
    for root in set
        .daemons
        .values()
        .filter(|d| !d.imported)
        .map(|d| d.root.clone())
        .collect::<indexmap::IndexSet<_>>()
    {
        let namespace = runtime::resolve_namespace(&root, Some(&settings_for(&settings, &root)))?;
        set.namespaces.insert(root, namespace);
    }
    // An inherited namespace is shared by every project beneath the config that
    // declares it, so two of them can name the same daemon and resolve to one
    // pitchfork ID. Say so instead of letting one silently shadow the other.
    let mut claimed: IndexMap<String, PathBuf> = IndexMap::new();
    for daemon in set.daemons.values() {
        let Some(namespace) = set.namespaces.get(&daemon.root) else {
            continue;
        };
        let id = format!("{namespace}/{}", daemon.name);
        if let Some(other) = claimed.insert(id.clone(), daemon.root.clone())
            && other != daemon.root
        {
            bail!(
                "daemon {id} is declared in both {} and {}; they share a namespace, so give one of them a different name or its own [daemons_settings] namespace",
                other.display(),
                daemon.root.display()
            );
        }
    }
    // Runs even with nothing imported, because it also rejects a `depends` this
    // project could not act on. An imported daemon's own `depends` is relative
    // to its project and is checked when that project loads.
    for daemon in set.daemons.values_mut().filter(|d| !d.imported) {
        rewrite_depends(&mut daemon.table, &imported_ids)?;
    }
    Ok(set)
}

/// Turn one declaration into a daemon owned by `root`.
fn build(name: &str, declaration: Declaration, source: PathBuf, root: PathBuf) -> Result<Daemon> {
    let (preset, version, mut table) = match declaration {
        Declaration::Preset(version) => (Some(name.to_string()), Some(version), toml::Table::new()),
        Declaration::Definition(mut table) => {
            let preset = take_string(&mut table, "preset")?;
            let version = take_string(&mut table, "version")?;
            (preset, version, table)
        }
    };
    if let Some(preset) = preset {
        let version =
            version.ok_or_else(|| eyre::eyre!("[daemons.{name}] requires version with preset"))?;
        return presets::expand(name, &preset, &version, table, &source, &root);
    }
    if version.is_some() || table.contains_key("options") {
        bail!("[daemons.{name}] requires preset when specifying version or options");
    }
    if table.get("run").and_then(toml::Value::as_str).is_none() {
        bail!("[daemons.{name}] requires run, preset, or project");
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
    Ok(Daemon {
        name: name.to_string(),
        source,
        root,
        table,
        preset: None,
        tool: None,
        exports: IndexMap::new(),
        imported: false,
    })
}

/// The `[daemons_settings]` that apply to a project root.
///
/// A root inherits the table from its ancestors, nearest declaration winning,
/// the same way a daemon declared in a parent configuration belongs to that
/// parent. Resolving this identically here and when a referenced project is
/// imported is what keeps one namespace from being computed for a daemon and a
/// different one from being registered for it.
fn settings_for(settings: &IndexMap<PathBuf, DaemonSettings>, root: &Path) -> DaemonSettings {
    let mut resolved = DaemonSettings::default();
    // Farthest ancestor first, so a nearer declaration overrides it.
    for ancestor in root.ancestors().collect::<Vec<_>>().into_iter().rev() {
        if let Some(declared) = settings.get(ancestor) {
            resolved.merge(declared.clone());
        }
    }
    resolved
}

/// Config files that apply to a directory, lowest precedence first.
///
/// A referenced project is later reloaded through its whole configuration
/// hierarchy when its daemons are prepared, so discovery has to walk the same
/// ancestors. Reading only the directory itself would miss a daemon or a
/// `[daemons_settings] namespace` inherited from a parent config, and mise would
/// then compute one daemon ID here and register a different one there.
fn hierarchy_config_paths(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    // `all_dirs` yields the directory first and its ancestors after it; reverse
    // so the nearest config is read last and wins.
    for ancestor in crate::file::all_dirs(dir, &crate::env::MISE_CEILING_PATHS)?
        .into_iter()
        .rev()
    {
        // Within one directory this helper lists the highest-precedence file
        // first, which is the opposite of the order wanted here. It also lists
        // `.tool-versions`, which is not TOML and would fail to parse; only a
        // TOML config can carry [daemons] anyway.
        paths.extend(
            crate::config::config_paths_in_dir(&ancestor)
                .into_iter()
                .rev()
                .filter(|path| path.extension().is_some_and(|ext| ext == "toml")),
        );
    }
    Ok(paths)
}

/// Pull a daemon definition out of another project so it runs with that
/// project's root, state, and `mise x` environment rather than this one's.
fn import(
    local_name: &str,
    mut table: toml::Table,
    source: &Path,
    root: &Path,
    namespaces: &mut IndexMap<PathBuf, String>,
) -> Result<(String, Daemon)> {
    let project = take_string(&mut table, "project")?
        .ok_or_else(|| eyre::eyre!("[daemons.{local_name}].project must be a string"))?;
    let remote_name = take_string(&mut table, "name")?.unwrap_or_else(|| local_name.to_string());
    validate_id("name", &remote_name)?;
    if let Some(unexpected) = table.keys().next() {
        bail!(
            "[daemons.{local_name}] declares {unexpected:?} alongside project; define the daemon in the referenced project instead"
        );
    }
    let expanded = crate::file::replace_path(&project);
    let dir = if expanded.is_absolute() {
        expanded
    } else {
        root.join(expanded)
    };
    // Fold `..` before the path reaches an error message: a missing project is
    // the common case, and `/src/app/../mirror-pipeline` is not a path anyone
    // can act on. Absolutizing works on a path that does not exist yet.
    let dir = dir
        .absolutize()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|_| dir.clone());
    let dir = dir.canonicalize().unwrap_or(dir);
    if !dir.is_dir() {
        bail!(
            "[daemons.{local_name}].project expects a project directory at {}, which {}; check that project out there or point [daemons.{local_name}].project in {} at its directory",
            dir.display(),
            if dir.exists() {
                "is not a directory"
            } else {
                "does not exist"
            },
            crate::file::display_path(source)
        );
    }
    let paths = hierarchy_config_paths(&dir)?;
    if paths.is_empty() {
        bail!(
            "[daemons.{local_name}].project expects a mise configuration in {}, which has none; add a mise.toml declaring [daemons.{remote_name}] there",
            dir.display()
        );
    }
    let mut found: Option<(Declaration, PathBuf, PathBuf)> = None;
    // Keyed by config root, exactly as `load` keys it, so the namespace resolves
    // the same way here as it will when this root is prepared.
    let mut settings: IndexMap<PathBuf, DaemonSettings> = IndexMap::new();
    let mut available: Vec<String> = Vec::new();
    for path in &paths {
        // `MiseToml::from_file` runs the same trust gate as ordinary config
        // loading, so an untrusted sibling project cannot be pulled in silently.
        let cf = crate::config::config_file::mise_toml::MiseToml::from_file(path)?;
        let remote_root = crate::config::config_file::config_root::config_root(path);
        if let Some(file_settings) = cf.daemon_settings() {
            settings
                .entry(remote_root.clone())
                .or_default()
                .merge(file_settings);
        }
        for (name, declaration) in cf.daemon_declarations() {
            if !available.contains(&name) {
                available.push(name.clone());
            }
            if name == remote_name {
                found = Some((declaration, path.clone(), remote_root.clone()));
            }
        }
    }
    let Some((declaration, remote_source, remote_root)) = found else {
        bail!(
            "[daemons.{local_name}].project expects [daemons.{remote_name}] in {}, which declares {}",
            dir.display(),
            if available.is_empty() {
                "no daemons".to_string()
            } else {
                available.join(", ")
            }
        );
    };
    if let Declaration::Definition(table) = &declaration
        && table.contains_key("project")
    {
        bail!(
            "[daemons.{remote_name}] in {} is itself imported with project; reference the project that declares it",
            dir.display()
        );
    }
    let mut daemon = build(
        &remote_name,
        declaration,
        remote_source,
        remote_root.clone(),
    )?;
    daemon.imported = true;
    let namespace =
        runtime::resolve_namespace(&remote_root, Some(&settings_for(&settings, &remote_root)))?;
    let id = format!("{namespace}/{}", daemon.name);
    namespaces.insert(remote_root, namespace);
    Ok((id, daemon))
}

/// Rewrite short `depends` entries that name an imported daemon. Pitchfork
/// resolves bare names inside one namespace, so an imported dependency only
/// works when it is written out in full.
fn rewrite_depends(table: &mut toml::Table, imported: &IndexMap<String, String>) -> Result<()> {
    let Some(depends) = table.get_mut("depends") else {
        return Ok(());
    };
    let rewrite = |value: &mut toml::Value| {
        if let Some(name) = value.as_str()
            && let Some(id) = imported.get(name)
        {
            *value = toml::Value::String(id.clone());
        }
    };
    match depends {
        toml::Value::String(_) => rewrite(depends),
        toml::Value::Array(entries) => {
            if entries.iter().any(|entry| entry.as_str().is_none()) {
                bail!("daemon depends must be a string or an array of strings");
            }
            entries.iter_mut().for_each(rewrite);
        }
        _ => bail!("daemon depends must be a string or an array of strings"),
    }
    Ok(())
}

/// The bare daemon names a table's `depends` refers to.
fn depends_names(table: &toml::Table) -> Vec<String> {
    let Some(depends) = table.get("depends") else {
        return Vec::new();
    };
    let entries: Vec<&toml::Value> = match depends {
        toml::Value::String(_) => vec![depends],
        toml::Value::Array(entries) => entries.iter().collect(),
        _ => Vec::new(),
    };
    entries
        .into_iter()
        .filter_map(|entry| entry.as_str())
        // A qualified ID names another project's daemon, which that project
        // checks for itself.
        .filter(|entry| !entry.contains('/'))
        .map(str::to_string)
        .collect()
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
            // An imported daemon runs out of its own project, which installs and
            // resolves its tool there. Adding it here would force an unrelated
            // version onto this project's toolset.
            let Some((tool, version)) = daemon.tool.as_ref().filter(|_| !daemon.imported) else {
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
            .filter(|daemon| !daemon.imported)
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
            namespaces: self
                .namespaces
                .iter()
                .filter(|(r, _)| r.as_path() == root)
                .map(|(r, n)| (r.clone(), n.clone()))
                .collect(),
            aliases: self.aliases.clone(),
        }
    }

    /// Translate a name typed on the command line. An imported daemon answers to
    /// the key this project gave it as well as to its qualified ID.
    pub(crate) fn resolve_alias(&self, name: &str) -> String {
        self.aliases
            .get(name)
            .cloned()
            .unwrap_or_else(|| name.to_string())
    }

    /// The pitchfork namespace for a project root, when this set declares daemons for it.
    pub(crate) fn namespace_for(&self, root: &Path) -> Option<&str> {
        self.namespaces.get(root).map(String::as_str)
    }

    /// The subset of this set that another set also names.
    ///
    /// Only for deciding what this invocation may act on or show. Never pass the
    /// result to `prepare`: a root's generated pitchfork config is rewritten
    /// whole, so registering a reduced set deletes that project's other daemons.
    pub(crate) fn restricted_to(&self, requested: &Self) -> Self {
        Self {
            daemons: self
                .daemons
                .iter()
                // Match the root too. A name alone is only unique within one
                // project, and nothing here promises both sets hold just one.
                .filter(|(_, d)| {
                    requested
                        .daemons
                        .values()
                        .any(|r| r.name == d.name && r.root == d.root)
                })
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            namespaces: self.namespaces.clone(),
            aliases: self.aliases.clone(),
        }
    }

    /// The daemons these names select, together with everything they depend on.
    ///
    /// Pitchfork starts a daemon's dependencies with it, so a caller checking
    /// what a command is about to run has to include them. A dependency written
    /// as a qualified ID belongs to another project and is checked when that
    /// project's root is prepared, so only bare names are followed here.
    pub(crate) fn with_dependencies(&self, names: &[String]) -> Self {
        let mut keep: indexmap::IndexSet<String> = self
            .daemons
            .values()
            .filter(|d| {
                names.iter().any(|name| {
                    name == &d.name
                        || self
                            .namespace_for(&d.root)
                            .is_some_and(|namespace| name == &format!("{namespace}/{}", d.name))
                })
            })
            .map(|d| d.name.clone())
            .collect();
        let mut queue: Vec<String> = keep.iter().cloned().collect();
        while let Some(name) = queue.pop() {
            let Some(daemon) = self.find(&name) else {
                continue;
            };
            for dependency in depends_names(&daemon.table) {
                if self.find(&dependency).is_some() && keep.insert(dependency.clone()) {
                    queue.push(dependency);
                }
            }
        }
        Self {
            daemons: self
                .daemons
                .iter()
                .filter(|(_, d)| keep.contains(&d.name))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            namespaces: self.namespaces.clone(),
            aliases: self.aliases.clone(),
        }
    }

    /// Look a daemon up by the name it carries inside its own project. Imported
    /// daemons are keyed by qualified ID, so the map key is not always the name.
    pub(crate) fn find(&self, name: &str) -> Option<&Daemon> {
        self.daemons.values().find(|d| d.name == name)
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

    /// A project whose daemons another project can import. The filename has to
    /// be one this build actually looks for; unit tests override the list.
    fn referenced_project(dir: &Path, body: &str) -> PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        let path = dir.join(&*crate::env::MISE_DEFAULT_CONFIG_FILENAME);
        std::fs::write(&path, body).unwrap();
        path
    }

    #[test]
    fn referenced_project_daemons_belong_to_that_project() {
        let tmp = tempfile::tempdir().unwrap();
        let mirror = tmp.path().join("mirror-pipeline");
        referenced_project(
            &mirror,
            "[daemons_settings]\nnamespace = 'mirror'\n[daemons.worker]\nrun = 'exec worker'\n[daemons.idle]\nrun = 'exec idle'\n",
        );
        let app = tmp.path().join("app");
        std::fs::create_dir_all(&app).unwrap();
        let config = files(&[(
            app.join("mise.toml").to_str().unwrap(),
            "[daemons.worker]\nproject = '../mirror-pipeline'\n[daemons.api]\nrun = 'exec api'\ndepends = ['worker']\n",
        )]);
        let set = load(&config).unwrap();
        let worker = &set.daemons["mirror/worker"];
        assert!(worker.imported);
        assert_eq!(worker.name, "worker");
        assert_eq!(worker.root, mirror.canonicalize().unwrap());
        assert_eq!(
            worker.source,
            mirror
                .canonicalize()
                .unwrap()
                .join(&*crate::env::MISE_DEFAULT_CONFIG_FILENAME)
        );
        // Only the named daemon is imported, and it keeps the referenced
        // project's namespace and root rather than this project's.
        assert!(!set.daemons.contains_key("idle"));
        assert!(set.roots().contains(&mirror.canonicalize().unwrap()));
        assert_eq!(
            set.namespace_for(&mirror.canonicalize().unwrap()),
            Some("mirror")
        );
        // A short depends on an imported daemon is rewritten; pitchfork resolves
        // bare names inside one namespace only.
        assert_eq!(
            set.daemons["api"].table["depends"][0].as_str(),
            Some("mirror/worker")
        );
        // The referenced project installs and exports for its own daemon.
        let mut requests = ToolRequestSet::default();
        set.add_tool_requests(&mut requests).unwrap();
        assert!(requests.tools.is_empty());
        assert!(set.env_entries().is_empty());
    }

    #[test]
    fn referenced_daemon_may_be_renamed_locally() {
        let tmp = tempfile::tempdir().unwrap();
        let mirror = tmp.path().join("mirror");
        referenced_project(&mirror, "[daemons.worker]\nrun = 'exec worker'\n");
        let app = tmp.path().join("app");
        std::fs::create_dir_all(&app).unwrap();
        let config = files(&[(
            app.join("mise.toml").to_str().unwrap(),
            &format!(
                "[daemons.pipeline]\nproject = {}\nname = 'worker'\n",
                toml::Value::String(mirror.to_string_lossy().into_owned())
            ),
        )]);
        let set = load(&config).unwrap();
        let daemon = set.daemons.values().next().unwrap();
        assert_eq!(daemon.name, "worker");
        assert!(daemon.imported);
        // The key this project chose still selects it on the command line.
        assert_eq!(
            set.resolve_alias("pipeline"),
            format!("{}/worker", set.namespace_for(&daemon.root).unwrap())
        );
        assert_eq!(set.resolve_alias("other"), "other");
    }

    #[test]
    fn missing_referenced_projects_name_the_path_and_the_setting() {
        let tmp = tempfile::tempdir().unwrap();
        let app = tmp.path().join("app");
        std::fs::create_dir_all(&app).unwrap();
        let source = app.join("mise.toml");
        let config = files(&[(
            source.to_str().unwrap(),
            "[daemons.worker]\nproject = '../mirror-pipeline'\n",
        )]);
        let err = load(&config).unwrap_err().to_string();
        assert!(err.contains(&tmp.path().join("mirror-pipeline").display().to_string()));
        assert!(err.contains("[daemons.worker].project"));

        let empty = tmp.path().join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        let config = files(&[(
            source.to_str().unwrap(),
            "[daemons.worker]\nproject = '../empty'\n",
        )]);
        assert!(
            load(&config)
                .unwrap_err()
                .to_string()
                .contains("mise configuration")
        );

        referenced_project(
            &tmp.path().join("other"),
            "[daemons.build]\nrun = 'exec build'\n",
        );
        let config = files(&[(
            source.to_str().unwrap(),
            "[daemons.worker]\nproject = '../other'\n",
        )]);
        let err = load(&config).unwrap_err().to_string();
        assert!(err.contains("[daemons.worker]"), "{err}");
        assert!(err.contains("build"), "{err}");
    }

    #[test]
    fn referenced_projects_inherit_daemons_and_namespaces_from_parent_configs() {
        // The referenced root is reloaded through its whole hierarchy when its
        // daemons are prepared, so discovery has to agree with that.
        let tmp = tempfile::tempdir().unwrap();
        let group = tmp.path().join("group");
        referenced_project(
            &group,
            "[daemons_settings]\nnamespace = 'shared'\n[daemons.worker]\nrun = 'exec worker'\n",
        );
        let mirror = group.join("mirror-pipeline");
        std::fs::create_dir_all(&mirror).unwrap();
        let app = tmp.path().join("app");
        std::fs::create_dir_all(&app).unwrap();
        let config = files(&[(
            app.join("mise.toml").to_str().unwrap(),
            &format!(
                "[daemons.worker]\nproject = {}\n",
                toml::Value::String(mirror.to_string_lossy().into_owned())
            ),
        )]);
        let set = load(&config).unwrap();
        // Both the daemon and the namespace come from the parent config.
        let daemon = &set.daemons["shared/worker"];
        assert!(daemon.imported);
        assert_eq!(daemon.root, group.canonicalize().unwrap());
        assert_eq!(
            set.namespace_for(&group.canonicalize().unwrap()),
            Some("shared")
        );
    }

    #[test]
    fn global_daemon_settings_do_not_capture_every_project() {
        // A global config's root is the home directory, so honouring a namespace
        // there would give every project on the machine the same one.
        let tmp = tempfile::tempdir().unwrap();
        let global = crate::dirs::HOME.join("config").join("config.toml");
        let config = files(&[
            (
                global.to_str().unwrap(),
                "[daemons_settings]\nnamespace = 'everything'\n",
            ),
            (
                tmp.path().join("app").join("mise.toml").to_str().unwrap(),
                "[daemons.api]\nrun = 'exec api'\n",
            ),
        ]);
        let set = load(&config).unwrap();
        let root = set.daemons["api"].root.clone();
        assert_ne!(set.namespace_for(&root), Some("everything"));
        assert_eq!(
            set.namespace_for(&root),
            Some(runtime::namespace(&root).unwrap().as_str())
        );
    }

    #[test]
    fn two_imports_cannot_claim_one_daemon_id() {
        let tmp = tempfile::tempdir().unwrap();
        for dir in ["one", "two"] {
            referenced_project(
                &tmp.path().join(dir),
                "[daemons_settings]\nnamespace = 'shared'\n[daemons.worker]\nrun = 'exec worker'\n",
            );
        }
        let app = tmp.path().join("app");
        std::fs::create_dir_all(&app).unwrap();
        let config = files(&[(
            app.join("mise.toml").to_str().unwrap(),
            "[daemons.first]\nproject = '../one'\nname = 'worker'\n[daemons.second]\nproject = '../two'\nname = 'worker'\n",
        )]);
        let err = load(&config).unwrap_err().to_string();
        assert!(err.contains("shared/worker"), "{err}");
        assert!(err.contains("share a namespace"), "{err}");
    }

    #[test]
    fn depends_entries_must_be_strings() {
        let config = files(&[(
            "/project/mise.toml",
            "[daemons.api]\nrun = 'exec api'\ndepends = ['db', 3]\n",
        )]);
        assert!(
            load(&config)
                .unwrap_err()
                .to_string()
                .contains("array of strings")
        );
    }

    #[test]
    fn selecting_a_daemon_includes_what_it_depends_on() {
        let config = files(&[(
            "/project/mise.toml",
            "[daemons.api]\nrun = 'exec api'\ndepends = ['cache']\n[daemons.cache]\nrun = 'exec cache'\ndepends = ['db']\n[daemons.db]\nrun = 'exec db'\n[daemons.unrelated]\nrun = 'exec other'\n",
        )]);
        let set = load(&config).unwrap();
        let starting = set.with_dependencies(&["api".to_string()]);
        let mut names: Vec<_> = starting.daemons.values().map(|d| d.name.clone()).collect();
        names.sort();
        assert_eq!(names, ["api", "cache", "db"]);
    }

    #[test]
    fn qualified_selection_requires_the_daemons_namespace() {
        let config = files(&[(
            "/project/mise.toml",
            "[daemons_settings]\nnamespace = 'app'\n[daemons.worker]\nrun = 'exec worker'\n[daemons.api]\nrun = 'exec api'\n",
        )]);
        let set = load(&config).unwrap();
        let selected = set.with_dependencies(&["mirror/worker".into(), "api".into()]);
        assert!(selected.find("worker").is_none());
        assert!(selected.find("api").is_some());
        assert!(
            set.with_dependencies(&["app/worker".into()])
                .find("worker")
                .is_some()
        );
    }

    #[test]
    fn two_projects_cannot_claim_one_daemon_id() {
        // Fixed namespaces make this reachable: an import resolves to the same
        // namespace as a daemon declared here, and both want the same ID.
        let tmp = tempfile::tempdir().unwrap();
        let mirror = tmp.path().join("mirror");
        referenced_project(
            &mirror,
            "[daemons_settings]\nnamespace = 'shared'\n[daemons.worker]\nrun = 'exec worker'\n",
        );
        let app = tmp.path().join("app");
        std::fs::create_dir_all(&app).unwrap();
        let config = files(&[(
            app.join("mise.toml").to_str().unwrap(),
            &format!(
                "[daemons_settings]\nnamespace = 'shared'\n[daemons.worker]\nrun = 'exec local'\n[daemons.pipeline]\nproject = {}\nname = 'worker'\n",
                toml::Value::String(mirror.to_string_lossy().into_owned())
            ),
        )]);
        let err = load(&config).unwrap_err().to_string();
        assert!(err.contains("shared/worker"), "{err}");
        assert!(err.contains("share a namespace"), "{err}");
    }

    #[test]
    fn nested_referenced_projects_resolve_the_same_namespace_both_ways() {
        // The namespace computed when importing has to be the one registered
        // when that root is prepared, or `depends` and `start` name an ID that
        // pitchfork never saw.
        let tmp = tempfile::tempdir().unwrap();
        let group = tmp.path().join("group");
        referenced_project(&group, "[daemons_settings]\nnamespace = 'shared'\n");
        let mirror = group.join("mirror-pipeline");
        referenced_project(&mirror, "[daemons.worker]\nrun = 'exec worker'\n");

        // What the referenced project computes for itself when it is prepared.
        let own = files(&[
            (
                mirror
                    .join(&*crate::env::MISE_DEFAULT_CONFIG_FILENAME)
                    .to_str()
                    .unwrap(),
                "[daemons.worker]\nrun = 'exec worker'\n",
            ),
            (
                group
                    .join(&*crate::env::MISE_DEFAULT_CONFIG_FILENAME)
                    .to_str()
                    .unwrap(),
                "[daemons_settings]\nnamespace = 'shared'\n",
            ),
        ]);
        let own = load(&own).unwrap();
        let own_root = own.daemons["worker"].root.clone();
        assert_eq!(own.namespace_for(&own_root), Some("shared"));

        // What another project computes when it imports that daemon.
        let app = tmp.path().join("app");
        std::fs::create_dir_all(&app).unwrap();
        let config = files(&[(
            app.join("mise.toml").to_str().unwrap(),
            &format!(
                "[daemons.worker]\nproject = {}\n",
                toml::Value::String(mirror.to_string_lossy().into_owned())
            ),
        )]);
        let imported = load(&config).unwrap();
        assert!(imported.daemons.contains_key("shared/worker"));
        assert_eq!(
            imported.namespace_for(&mirror.canonicalize().unwrap()),
            Some("shared")
        );
    }

    #[test]
    fn referenced_projects_with_tool_versions_files_still_import() {
        // `.tool-versions` sits in the same config list but is not TOML; parsing
        // it would fail before the project's mise config is ever read.
        let tmp = tempfile::tempdir().unwrap();
        let mirror = tmp.path().join("mirror");
        referenced_project(&mirror, "[daemons.worker]\nrun = 'exec worker'\n");
        std::fs::write(
            mirror.join(&*crate::env::MISE_DEFAULT_TOOL_VERSIONS_FILENAME),
            "node 20.0.0\n",
        )
        .unwrap();
        let app = tmp.path().join("app");
        std::fs::create_dir_all(&app).unwrap();
        let config = files(&[(
            app.join("mise.toml").to_str().unwrap(),
            "[daemons.worker]\nproject = '../mirror'\n",
        )]);
        let set = load(&config).unwrap();
        assert_eq!(set.find("worker").map(|d| d.imported), Some(true));
    }

    #[test]
    fn daemon_settings_merge_across_files_instead_of_replacing() {
        // A file that sets one key must not discard a namespace another file
        // established; other projects refer to the IDs it produces.
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let config = files(&[
            (
                root.join("mise.local.toml").to_str().unwrap(),
                "[daemons_settings]\nnamespace_per_worktree = false\n",
            ),
            (
                root.join("mise.toml").to_str().unwrap(),
                "[daemons_settings]\nnamespace = 'entiredb'\n[daemons.api]\nrun = 'exec api'\n",
            ),
        ]);
        let set = load(&config).unwrap();
        let declared = set.daemons["api"].root.clone();
        assert_eq!(set.namespace_for(&declared), Some("entiredb"));
        // The higher-precedence file still wins for the key it does set.
        let mut merged = DaemonSettings {
            namespace: Some("entiredb".into()),
            namespace_per_worktree: Some(true),
        };
        merged.merge(DaemonSettings {
            namespace: None,
            namespace_per_worktree: Some(false),
        });
        assert_eq!(merged.namespace.as_deref(), Some("entiredb"));
        assert!(!merged.namespace_per_worktree());
        assert!(DaemonSettings::default().namespace_per_worktree());
    }

    #[test]
    fn referenced_projects_cannot_be_chained_or_mixed_with_a_definition() {
        let tmp = tempfile::tempdir().unwrap();
        let far = tmp.path().join("far");
        referenced_project(&far, "[daemons.worker]\nrun = 'exec worker'\n");
        let middle = tmp.path().join("middle");
        referenced_project(
            &middle,
            &format!(
                "[daemons.worker]\nproject = {}\n",
                toml::Value::String(far.to_string_lossy().into_owned())
            ),
        );
        let app = tmp.path().join("app");
        std::fs::create_dir_all(&app).unwrap();
        let config = files(&[(
            app.join("mise.toml").to_str().unwrap(),
            "[daemons.worker]\nproject = '../middle'\n",
        )]);
        assert!(
            load(&config)
                .unwrap_err()
                .to_string()
                .contains("reference the project that declares it")
        );
        let config = files(&[(
            app.join("mise.toml").to_str().unwrap(),
            "[daemons.worker]\nproject = '../far'\nrun = 'exec local'\n",
        )]);
        assert!(
            load(&config)
                .unwrap_err()
                .to_string()
                .contains("define the daemon in the referenced project")
        );
    }

    #[test]
    fn explicit_namespaces_are_validated() {
        for namespace in ["entiredb", "entire.db", "entire_db", "db-1"] {
            assert!(validate_id("namespace", namespace).is_ok(), "{namespace}");
        }
        for namespace in ["", ".", "..", "a--b", "-a", "a-", "a/b", "a b"] {
            assert!(validate_id("namespace", namespace).is_err(), "{namespace}");
        }
        let tmp = tempfile::tempdir().unwrap();
        let config = files(&[(
            tmp.path().join("mise.toml").to_str().unwrap(),
            "[daemons_settings]\nnamespace = 'entire--db'\n[daemons.api]\nrun = 'exec api'\n",
        )]);
        assert!(
            load(&config)
                .unwrap_err()
                .to_string()
                .contains("invalid daemon namespace")
        );
    }

    #[test]
    fn explicit_namespace_replaces_the_hashed_default() {
        let tmp = tempfile::tempdir().unwrap();
        let config = files(&[(
            tmp.path().join("mise.toml").to_str().unwrap(),
            "[daemons_settings]\nnamespace = 'entiredb'\n[daemons.api]\nrun = 'exec api'\n",
        )]);
        let set = load(&config).unwrap();
        let root = set.daemons["api"].root.clone();
        assert_eq!(set.namespace_for(&root), Some("entiredb"));
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
