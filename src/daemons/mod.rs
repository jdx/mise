//! Project daemons: custom pitchfork definitions and embedded database presets.
pub(crate) mod hook_env;
pub(crate) mod ports;
pub(crate) mod presets;
pub(crate) mod runtime;
pub(crate) mod tasks;

use crate::config::config_file::ConfigFile;
use crate::config::env_directive::EnvDirective;
use crate::config::{Config, ConfigMap, Settings};
use crate::toolset::{ToolRequest, ToolRequestSet, ToolSource};
use eyre::{Result, bail};
use indexmap::IndexMap;
use path_absolutize::Absolutize;
use ports::{PortClaim, PortRequest};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Gate shared by `[daemons]` loading and by tasks that require daemons, so
/// both report the same requirement.
pub(crate) const EXPERIMENTAL: &str = "[daemons] requires experimental = true";

/// Set on a daemon that runs a task, so the `mise run` it starts does not start
/// daemons of its own and recurse.
pub(crate) const DAEMON_TASK_MARKER: &str = "MISE_DAEMON_TASK";

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
    /// Task this daemon runs, when declared with `task = "..."`. Retained so
    /// the reference can be checked against the loaded task list, which is not
    /// available while configuration is still being parsed.
    pub task: Option<String>,
    pub tool: Option<(String, String)>,
    pub exports: IndexMap<String, String>,
    /// True when this daemon was declared by another project and pulled in with
    /// `project =`. Its tools and exported environment belong to that project.
    pub imported: bool,
    /// The resolved allocation for a `port = "auto"` daemon, persisted so it
    /// survives a later change to the slot derivation.
    pub port: Option<PortClaim>,
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
    /// Daemons that depend on an import which could not be resolved, mapping the
    /// daemon's key to the import's local key. Starting one would run it without
    /// something it declared it needs, so the command refuses instead.
    pub blocked: IndexMap<String, String>,
    /// Imports that could not be resolved, by local key.
    ///
    /// Daemons load on every command, so a sibling project that is missing or
    /// not trusted must not take `mise x`, `mise run` or the activation hook
    /// down with it. The failure is carried here and reported by `mise daemons`,
    /// which is the command that can act on it.
    pub import_errors: IndexMap<String, String>,
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
            // A file may carry only `[daemons_settings]`, which declares nothing.
            if !entries.is_empty() {
                warn_once!("{EXPERIMENTAL}; ignoring daemon declarations");
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
    let mut ports = PortState::default();
    for (name, (declaration, source, root)) in declarations {
        validate_id("name", &name)?;
        if let Declaration::Definition(table) = &declaration
            && table.contains_key("project")
        {
            let spec = parse_import(&name, table.clone())?;
            let (key, daemon) = match import(
                &name,
                &spec,
                &source,
                &root,
                &mut set.namespaces,
                &mut ports,
            ) {
                Ok(imported) => imported,
                Err(err) => {
                    debug!("[daemons.{name}] import failed: {err:#}");
                    set.import_errors.insert(name, format!("{err:#}"));
                    continue;
                }
            };
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
        set.daemons.insert(
            name.clone(),
            build(&name, declaration, source, root, &mut ports)?,
        );
    }
    // The first claimant of an ambiguous key kept its export while it looked
    // unique; drop it now so neither side is handed the other's endpoint.
    for daemon in set.daemons.values_mut() {
        daemon.exports.retain(|key, _| !ports.ambiguous.contains(key));
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
    let unresolved = set.import_errors.clone();
    let mut blocked: IndexMap<String, String> = IndexMap::new();
    for (key, daemon) in set.daemons.iter_mut().filter(|(_, d)| !d.imported) {
        if let Some(missing) = rewrite_depends(&mut daemon.table, &imported_ids, &unresolved)? {
            blocked.insert(key.clone(), missing);
        }
    }
    set.blocked = blocked;
    Ok(set)
}

/// Port allocation state shared across one `load`, so two daemons cannot claim
/// one slot or one exported variable without it being noticed.
#[derive(Default)]
struct PortState {
    /// Previously persisted allocations, read once per project root.
    claims: BTreeMap<PathBuf, BTreeMap<String, PortClaim>>,
    /// Port variables already taken, so two names cannot normalize onto one key.
    keys: BTreeMap<String, String>,
    ambiguous: std::collections::BTreeSet<String>,
}

/// Turn one declaration into a daemon owned by `root`.
fn build(
    name: &str,
    declaration: Declaration,
    source: PathBuf,
    root: PathBuf,
    ports: &mut PortState,
) -> Result<Daemon> {
    let (preset, version, mut table) = match declaration {
        Declaration::Preset(version) => (Some(name.to_string()), Some(version), toml::Table::new()),
        Declaration::Definition(mut table) => {
            let preset = take_string(&mut table, "preset")?;
            let version = take_string(&mut table, "version")?;
            (preset, version, table)
        }
    };
    let request = table
        .remove("port")
        .map(|value| ports::parse(name, value))
        .transpose()?;
    // `auto` resolution reuses the allocation recorded by the last start, so
    // a change to the slot derivation cannot move a running daemon's port.
    let persisted = if matches!(request, Some(PortRequest::Auto { .. })) {
        ports
            .claims
            .entry(root.clone())
            .or_insert_with(|| {
                runtime::read_state(&root)
                    .map(|s| s.ports)
                    .unwrap_or_default()
            })
            .get(name)
            .copied()
    } else {
        None
    };
    // `init`, `task` and `args` are mise concepts; pitchfork never sees them.
    let init = take_init(&mut table, name)?;
    let task = take_string(&mut table, "task")?;
    let args = take_args(&mut table, name)?;
    if preset.is_some() {
        // Name the key actually present; `args` without `task` is a
        // different mistake from `preset` with `task`.
        if task.is_some() {
            bail!("[daemons.{name}] cannot combine preset with task");
        }
        if args.is_some() {
            bail!("[daemons.{name}] args requires task, which a preset cannot use");
        }
    }
    if let Some(preset) = preset {
        let version =
            version.ok_or_else(|| eyre::eyre!("[daemons.{name}] requires version with preset"))?;
        let claim = match request {
            Some(PortRequest::Passthrough(_)) => bail!(
                "[daemons.{name}].port must be an integer or \"auto\"; pitchfork's structured port is only available on custom daemons"
            ),
            Some(PortRequest::Fixed(port)) => Some(PortClaim::fixed(port)),
            Some(PortRequest::Auto { base, stride }) => Some(ports::resolve(
                name,
                &root,
                base,
                stride,
                Some(presets::default_port(&preset)?),
                persisted,
            )?),
            None => None,
        };
        return presets::expand(
            name,
            &preset,
            &version,
            table,
            presets::Extras {
                init: &init,
                port: claim,
            },
            &source,
            &root,
        );
    }
    if version.is_some() || table.contains_key("options") {
        bail!("[daemons.{name}] requires preset when specifying version or options");
    }
    if task.is_some() && table.contains_key("run") {
        bail!("[daemons.{name}] cannot set both run and task");
    }
    if args.is_some() && task.is_none() {
        bail!("[daemons.{name}] args requires task");
    }
    if let Some(task) = &task {
        if task.is_empty() {
            bail!("[daemons.{name}] task must not be empty");
        }
        let mut run = format!(
            "exec {} run {}",
            presets::quote(crate::env::MISE_BIN.to_string_lossy()),
            presets::quote(task)
        );
        let args = args.unwrap_or_default();
        if !args.is_empty() {
            run.push_str(" --");
            for arg in args {
                run.push(' ');
                run.push_str(&presets::quote(arg));
            }
        }
        table.insert("run".into(), toml::Value::String(run));
        // The task runs through `mise run`, which would start this very
        // daemon again. This marker breaks that cycle in `tasks::start`
        // instead of `--skip-deps`, which would also have discarded the
        // task's own `depends`, leaving the daemon to supervise a stale
        // build.
        table
            .entry("env".to_string())
            .or_insert_with(|| toml::Value::Table(toml::Table::new()))
            .as_table_mut()
            .ok_or_else(|| eyre::eyre!("[daemons.{name}] env must be a table"))?
            .insert(
                DAEMON_TASK_MARKER.into(),
                toml::Value::String("1".to_string()),
            );
        // mise is already the entry point, so pitchfork does not need
        // to wrap a bare task daemon in `mise x`. With `init` it does:
        // the setup steps and the task then share one shell inside the
        // project's tool environment, so a step can export a variable
        // or change directory for the ones after it, exactly as it can
        // for a daemon declared with `run`.
        if init.is_empty() {
            table
                .entry("mise".to_string())
                .or_insert(toml::Value::Boolean(false));
        }
    }
    if table.get("run").and_then(toml::Value::as_str).is_none() {
        bail!("[daemons.{name}] requires run, task, preset, or project");
    }
    let claim = match request {
        Some(PortRequest::Passthrough(value)) => {
            table.insert("port".into(), value);
            None
        }
        Some(PortRequest::Fixed(port)) => Some(PortClaim::fixed(port)),
        Some(PortRequest::Auto { base, stride }) => {
            Some(ports::resolve(name, &root, base, stride, None, persisted)?)
        }
        None => None,
    };
    if let Some(claim) = claim {
        table.insert("port".into(), expected_port(claim.port));
    }
    // Without this a custom daemon's port would reach pitchfork and
    // nothing else: `mise env` would export nothing, and the process
    // could only discover it through pitchfork's own injection.
    let mut port_export = IndexMap::new();
    if let Some(c) = claim
        && let Some(key) = port_env_var(name)
    {
        match ports.keys.insert(key.clone(), name.to_string()) {
            // Two names collapsing onto one key is ambiguous, and
            // picking a winner would hand somebody the wrong endpoint.
            // Neither is exported and both daemons still run, because
            // this convenience must not break a working project.
            Some(other) => {
                warn_once!(
                    "[daemons] {other} and {name} both map to {key}; their names differ only by punctuation, so neither port is exported. Rename one of them."
                );
                ports.ambiguous.insert(key);
            }
            None => {
                port_export.insert(key, c.port.to_string());
            }
        }
    }
    table
        .entry("mise".to_string())
        .or_insert(toml::Value::Boolean(true));
    if !init.is_empty() {
        let run = table["run"].as_str().unwrap().to_string();
        table.insert(
            "run".into(),
            toml::Value::String(presets::with_init(&init, &run)),
        );
    }
    Ok(Daemon {
        name: name.to_string(),
        source,
        root,
        table,
        preset: None,
        task,
        tool: None,
        exports: port_export,
        imported: false,
        port: claim,
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
        // Honour the same ignore rules ordinary loading does, so a config the
        // user has told mise to skip is skipped here too rather than read and
        // reported as an error.
        if crate::config::config_dir_is_ignored(&ancestor, false) {
            continue;
        }
        // Within one directory this helper lists the highest-precedence file
        // first, which is the opposite of the order wanted here. It also lists
        // `.tool-versions`, which is not TOML and would fail to parse; only a
        // TOML config can carry [daemons] anyway.
        paths.extend(
            crate::config::config_paths_in_dir(&ancestor)
                .into_iter()
                .rev()
                .filter(|path| path.extension().is_some_and(|ext| ext == "toml"))
                .filter(|path| !crate::config::config_path_is_ignored(path, false)),
        );
    }
    // Two paths can reach one file through a symlinked prefix; reading it twice
    // would merge its settings onto themselves.
    let mut seen = std::collections::HashSet::new();
    paths.retain(|path| seen.insert(crate::file::desymlink_path(path)));
    Ok(paths)
}

/// Pull a daemon definition out of another project so it runs with that
/// project's root, state, and `mise x` environment rather than this one's.
/// What a `project =` declaration asks for, before anything is read from disk.
struct Import {
    project: String,
    remote_name: String,
}

/// Check the declaration itself. This is this project's own configuration, so a
/// mistake here is fatal like any other config error; only what has to be read
/// from the other project is allowed to fail softly.
fn parse_import(local_name: &str, mut table: toml::Table) -> Result<Import> {
    let project = take_string(&mut table, "project")?
        .ok_or_else(|| eyre::eyre!("[daemons.{local_name}].project must be a string"))?;
    let remote_name = take_string(&mut table, "name")?.unwrap_or_else(|| local_name.to_string());
    validate_id("name", &remote_name)?;
    if let Some(unexpected) = table.keys().next() {
        bail!(
            "[daemons.{local_name}] declares {unexpected:?} alongside project; define the daemon in the referenced project instead"
        );
    }
    Ok(Import {
        project,
        remote_name,
    })
}

fn import(
    local_name: &str,
    spec: &Import,
    source: &Path,
    root: &Path,
    namespaces: &mut IndexMap<PathBuf, String>,
    ports: &mut PortState,
) -> Result<(String, Daemon)> {
    let Import {
        project,
        remote_name,
    } = spec;
    let remote_name = remote_name.as_str();
    let expanded = crate::file::replace_path(project);
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
        // Require trust that already exists rather than letting the parse
        // establish it. `mise x`, `mise run`, `mise install`, `mise watch` and
        // `mise daemons start` mark the active config implicitly trusted, and
        // that branch would grant durable trust to whatever directory `project`
        // names, with no prompt, so a later `cd` into it would run its env,
        // hooks and templates. Safe mode makes config inert, so it needs no
        // trust of its own.
        if !Settings::safe_mode() && !crate::config::config_file::is_path_trusted(path) {
            // The untrusted file can be an ancestor of the referenced project,
            // so name the root that actually needs trusting; trusting the
            // project directory would not cover it. Ask the same question
            // `is_path_trusted` does: under `paranoid` that is the file itself,
            // and naming its directory would send the user nowhere.
            let untrusted = crate::config::config_file::config_trust_root(path);
            bail!(
                "[daemons.{local_name}].project needs {}, which is not trusted; run `mise trust {}` after reviewing it",
                crate::file::display_path(path),
                untrusted.display()
            );
        }
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
        remote_name,
        declaration,
        remote_source,
        remote_root.clone(),
        ports,
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
///
/// A dependency on an import that could not be resolved is dropped rather than
/// written out, because there is no ID to write. Leaving the bare name would
/// register a definition naming a daemon pitchfork cannot resolve; the import
/// failure itself is reported separately, so the reason is not lost.
fn rewrite_depends(
    table: &mut toml::Table,
    imported: &IndexMap<String, String>,
    unresolved: &IndexMap<String, String>,
) -> Result<Option<String>> {
    let Some(depends) = table.get_mut("depends") else {
        return Ok(None);
    };
    let mut missing: Option<String> = None;
    let rewrite = |value: &mut toml::Value| {
        if let Some(name) = value.as_str()
            && let Some(id) = imported.get(name)
        {
            *value = toml::Value::String(id.clone());
        }
    };
    let dangling = |value: &toml::Value| {
        value
            .as_str()
            .filter(|name| unresolved.contains_key(*name))
            .map(str::to_string)
    };
    let mut empty = false;
    match depends {
        toml::Value::String(_) => {
            if let Some(name) = dangling(depends) {
                missing = Some(name);
                empty = true;
            } else {
                rewrite(depends);
            }
        }
        toml::Value::Array(entries) => {
            if entries.iter().any(|entry| entry.as_str().is_none()) {
                bail!("daemon depends must be a string or an array of strings");
            }
            missing = entries.iter().find_map(dangling);
            entries.retain(|entry| dangling(entry).is_none());
            entries.iter_mut().for_each(rewrite);
            empty = entries.is_empty();
        }
        _ => bail!("daemon depends must be a string or an array of strings"),
    }
    // Dropping the last entry leaves nothing to depend on, so drop the key too
    // rather than registering an empty list.
    if empty {
        table.remove("depends");
    }
    Ok(missing)
}

/// The daemon names and qualified IDs a table's `depends` refers to.
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
        .map(str::to_string)
        .collect()
}

/// The variable a custom daemon's resolved port is exported as, so `mise env`,
/// `mise x`, and the daemon's own process all see one endpoint. Presets export
/// their tool's conventional variables instead.
fn port_env_var(name: &str) -> Option<String> {
    // A shell cannot export a name starting with a digit: `export 9API_PORT=1`
    // is an invalid identifier and would break the whole activation, not just
    // that variable. Such a name was legal before this export existed, so it
    // keeps working and only goes without the variable.
    if !name.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_') {
        warn_once!(
            "[daemons] {name} starts with a digit, so its port cannot be exported as a shell variable; rename it to start with a letter to get one"
        );
        return None;
    }
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
    Some(format!("{base}_PORT"))
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

/// Remove `init`, accepting one command or an ordered list of them.
fn take_init(table: &mut toml::Table, name: &str) -> Result<Vec<String>> {
    let Some(value) = table.remove("init") else {
        return Ok(vec![]);
    };
    let steps = match value {
        toml::Value::String(step) => vec![step],
        toml::Value::Array(steps) => steps
            .into_iter()
            .map(|step| match step {
                toml::Value::String(step) => Ok(step),
                _ => bail!("[daemons.{name}] init entries must be strings"),
            })
            .collect::<Result<Vec<_>>>()?,
        _ => bail!("[daemons.{name}] init must be a string or an array of strings"),
    };
    if steps.iter().any(|step| step.trim().is_empty()) {
        bail!("[daemons.{name}] init entries must not be empty");
    }
    Ok(steps)
}

fn take_args(table: &mut toml::Table, name: &str) -> Result<Option<Vec<String>>> {
    table
        .remove("args")
        .map(|value| match value {
            toml::Value::Array(args) => args
                .into_iter()
                .map(|arg| match arg {
                    toml::Value::String(arg) => Ok(arg),
                    _ => bail!("[daemons.{name}] args entries must be strings"),
                })
                .collect::<Result<Vec<_>>>(),
            _ => bail!("[daemons.{name}] args must be an array of strings"),
        })
        .transpose()
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
            import_errors: self.import_errors.clone(),
            blocked: self.blocked.clone(),
        }
    }

    /// Resolve a local declaration or import alias to its qualified ID.
    /// A local name must not also select a same-named daemon from another root.
    pub(crate) fn resolve_alias(&self, name: &str) -> String {
        self.aliases
            .get(name)
            .cloned()
            .or_else(|| {
                let daemon = self.daemons.get(name)?;
                let namespace = self.namespace_for(&daemon.root)?;
                Some(format!("{namespace}/{}", daemon.name))
            })
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
            import_errors: self.import_errors.clone(),
            blocked: self.blocked.clone(),
        }
    }

    /// The daemons these names select, together with their dependency closure.
    /// Bare dependencies resolve within the declaring daemon's namespace;
    /// qualified dependencies can select another loaded project's daemon.
    pub(crate) fn with_dependencies(&self, names: &[String]) -> Self {
        let qualified = |d: &Daemon| match self.namespace_for(&d.root) {
            Some(namespace) => format!("{namespace}/{}", d.name),
            None => d.name.clone(),
        };
        let by_id: IndexMap<_, _> = self.daemons.values().map(|d| (qualified(d), d)).collect();
        let mut keep: indexmap::IndexSet<String> = by_id
            .iter()
            .filter(|(id, d)| names.iter().any(|name| name == *id || name == &d.name))
            .map(|(id, _)| id.clone())
            .collect();
        let mut queue: Vec<String> = keep.iter().cloned().collect();
        while let Some(id) = queue.pop() {
            let Some(daemon) = by_id.get(&id) else {
                continue;
            };
            for dependency in depends_names(&daemon.table) {
                let dependency = if dependency.contains('/') {
                    dependency
                } else if let Some(namespace) = self.namespace_for(&daemon.root) {
                    format!("{namespace}/{dependency}")
                } else {
                    dependency
                };
                if by_id.contains_key(&dependency) && keep.insert(dependency.clone()) {
                    queue.push(dependency);
                }
            }
        }
        Self {
            daemons: self
                .daemons
                .iter()
                .filter(|(_, d)| keep.contains(&qualified(d)))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            namespaces: self.namespaces.clone(),
            aliases: self.aliases.clone(),
            import_errors: self.import_errors.clone(),
            blocked: self.blocked.clone(),
        }
    }

    /// Look a daemon up by the name it carries inside its own project. Imported
    /// daemons are keyed by qualified ID, so the map key is not always the name.
    pub(crate) fn find(&self, name: &str) -> Option<&Daemon> {
        self.daemons.values().find(|d| d.name == name)
    }

    /// Check `task = "..."` references against the loaded task list. Task
    /// loading is asynchronous and reads the filesystem, so this cannot run
    /// while `[daemons]` is parsed. The three paths that register a generated
    /// pitchfork configuration call it first: `mise daemons start`, a task that
    /// requires daemons, and the shell auto-lifecycle hook.
    pub(crate) async fn validate_tasks(&self, config: &Arc<Config>) -> Result<()> {
        if self.daemons.values().all(|d| d.task.is_none()) {
            return Ok(());
        }
        let tasks = config.tasks_with_aliases().await?;
        for daemon in self.daemons.values() {
            let Some(task) = &daemon.task else {
                continue;
            };
            if !tasks.contains_key(task) {
                bail!("daemon {} runs unknown task {task:?}", daemon.name);
            }
        }
        Ok(())
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
    fn task_daemons_run_mise_without_a_mise_wrapper() {
        let config = files(&[(
            "/project/mise.toml",
            "[daemons.core]\ntask = 'dev:core'\nargs = ['--port', \"it's 3000\"]\nready_port = 3000\n",
        )]);
        let set = load(&config).unwrap();
        let daemon = &set.daemons["core"];
        let mise = presets::quote(crate::env::MISE_BIN.to_string_lossy());
        assert_eq!(
            daemon.table["run"].as_str(),
            Some(format!("exec {mise} run 'dev:core' -- '--port' 'it'\\''s 3000'").as_str())
        );
        // The task keeps its own `depends`; a marker in the daemon environment
        // is what stops the nested run from starting this daemon again.
        assert_eq!(daemon.table["env"][DAEMON_TASK_MARKER].as_str(), Some("1"));
        // mise is the entry point already, so pitchfork must not re-enter it.
        assert_eq!(daemon.table["mise"].as_bool(), Some(false));
        assert_eq!(daemon.task.as_deref(), Some("dev:core"));
        // Task plumbing stays in mise; pitchfork only sees its own keys.
        assert!(!daemon.table.contains_key("task"));
        assert!(!daemon.table.contains_key("args"));
        assert_eq!(daemon.table["ready_port"].as_integer(), Some(3000));
        // A task daemon with no args does not emit a dangling separator.
        let config = files(&[("/project/mise.toml", "[daemons.core]\ntask = 'dev'\n")]);
        assert_eq!(
            load(&config).unwrap().daemons["core"].table["run"].as_str(),
            Some(format!("exec {mise} run 'dev'").as_str())
        );
    }

    #[test]
    fn conflicting_daemon_process_declarations_are_rejected() {
        for body in [
            "[daemons.core]\ntask = 'dev'\nrun = 'server'\n",
            "[daemons.core]\npreset = 'postgres'\nversion = '18'\ntask = 'dev'\n",
            "[daemons.core]\nargs = ['--port']\nrun = 'server'\n",
            "[daemons.core]\ntask = ''\n",
            "[daemons.core]\nready_port = 3000\n",
        ] {
            assert!(
                load(&files(&[("/project/mise.toml", body)])).is_err(),
                "{body}"
            );
        }
    }

    #[test]
    fn conflict_errors_name_the_key_that_is_set() {
        // `args` without `task` is a different mistake from `preset` with
        // `task`, so the message must not blame a key the user never wrote.
        let err = load(&files(&[(
            "/project/mise.toml",
            "[daemons.db]\npreset = 'postgres'\nversion = '18'\nargs = ['--flag']\n",
        )]))
        .unwrap_err()
        .to_string();
        assert!(err.contains("args requires task"), "{err}");
        let err = load(&files(&[(
            "/project/mise.toml",
            "[daemons.db]\npreset = 'postgres'\nversion = '18'\ntask = 'dev'\n",
        )]))
        .unwrap_err()
        .to_string();
        assert!(err.contains("cannot combine preset with task"), "{err}");
    }

    #[test]
    fn init_steps_run_before_the_long_running_process() {
        let config = files(&[(
            "/project/mise.toml",
            "[daemons.api]\ninit = ['npm ci', 'npm run migrate']\nrun = 'exec npm start'\n",
        )]);
        let set = load(&config).unwrap();
        assert_eq!(
            set.daemons["api"].table["run"].as_str(),
            Some("npm ci && npm run migrate && exec npm start")
        );
        // A single string is the same as a one-entry list.
        let config = files(&[(
            "/project/mise.toml",
            "[daemons.api]\ninit = 'npm ci'\nrun = 'exec npm start'\n",
        )]);
        assert_eq!(
            load(&config).unwrap().daemons["api"].table["run"].as_str(),
            Some("npm ci && exec npm start")
        );
        // Preset initialization still comes first, before user setup.
        let config = files(&[(
            "/project/mise.toml",
            "[daemons.db]\npreset = 'postgres'\nversion = '18'\ninit = 'echo ready'\n",
        )]);
        let run = load(&config).unwrap().daemons["db"].table["run"]
            .as_str()
            .unwrap()
            .to_string();
        let init = run.find(" daemons __init ").unwrap();
        assert!(init < run.find("echo ready").unwrap());
        assert!(run.contains("&& echo ready && exec "));
        // A task daemon with init keeps pitchfork's `mise x` wrapper, so the
        // steps and the task share one shell that has the project's tools.
        let wrapped = files(&[(
            "/project/mise.toml",
            "[daemons.core]\ntask = 'dev'\ninit = ['npm ci', 'npm run migrate']\n",
        )]);
        let daemon = &load(&wrapped).unwrap().daemons["core"];
        let run = daemon.table["run"].as_str().unwrap().to_string();
        assert!(
            run.starts_with("npm ci && npm run migrate && exec "),
            "{run}"
        );
        assert!(run.ends_with("run 'dev'"), "{run}");
        assert_eq!(daemon.table["mise"].as_bool(), Some(true));
        // An explicit `mise` value stays the user's call.
        let explicit = files(&[(
            "/project/mise.toml",
            "[daemons.core]\ntask = 'dev'\ninit = 'npm ci'\nmise = false\n",
        )]);
        assert_eq!(
            load(&explicit).unwrap().daemons["core"].table["mise"].as_bool(),
            Some(false)
        );
        // `init` is consumed by mise and never reaches pitchfork.
        assert!(
            !load(&config).unwrap().daemons["db"]
                .table
                .contains_key("init")
        );
        for body in [
            "[daemons.api]\ninit = 1\nrun = 'server'\n",
            "[daemons.api]\ninit = [1]\nrun = 'server'\n",
            "[daemons.api]\ninit = ' '\nrun = 'server'\n",
        ] {
            assert!(
                load(&files(&[("/project/mise.toml", body)])).is_err(),
                "{body}"
            );
        }
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
        let path = untrusted_project(dir, body);
        // Importing requires trust that already exists. These fixtures stand in
        // for projects the developer has reviewed and trusted.
        crate::config::config_file::trust(dir).unwrap();
        path
    }

    /// Every test that imports takes this lock. The paranoid-mode test changes a
    /// global setting, and under `paranoid` a fixture's directory-level trust no
    /// longer covers its config file, so an overlapping import would fail.
    static IMPORT_TESTS: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn import_lock() -> std::sync::MutexGuard<'static, ()> {
        crate::test::lock_ignoring_poison(&IMPORT_TESTS)
    }

    /// A referenced project the developer has never trusted.
    fn untrusted_project(dir: &Path, body: &str) -> PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        let path = dir.join(&*crate::env::MISE_DEFAULT_CONFIG_FILENAME);
        std::fs::write(&path, body).unwrap();
        path
    }

    #[test]
    fn referenced_project_daemons_belong_to_that_project() {
        let _serial = import_lock();
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
        let starting = set.with_dependencies(&["api".into()]);
        assert!(starting.daemons.contains_key("mirror/worker"));
        assert!(starting.daemons.contains_key("api"));
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
        let _serial = import_lock();
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
    fn an_unresolvable_import_does_not_break_other_commands() {
        let _serial = import_lock();
        // Daemons load on every command. A sibling that is not checked out must
        // leave `mise x`, `mise run` and the activation hook working.
        let tmp = tempfile::tempdir().unwrap();
        let app = tmp.path().join("app");
        std::fs::create_dir_all(&app).unwrap();
        let config = files(&[(
            app.join("mise.toml").to_str().unwrap(),
            "[daemons.pipeline]\nproject = '../mirror-pipeline'\n[daemons.api]\nrun = 'exec api'\n",
        )]);
        let set = load(&config).unwrap();
        // Nothing depends on the unresolved import here, so nothing is blocked.
        assert!(set.blocked.is_empty());
        // The local daemon still loads, so env and tool resolution are unaffected.
        assert!(set.daemons.contains_key("api"));
        assert!(!set.daemons.contains_key("pipeline"));
        // The failure is kept so `mise daemons` can report it with the path.
        let err = &set.import_errors["pipeline"];
        assert!(err.contains("mirror-pipeline"), "{err}");
        assert!(err.contains("[daemons.pipeline].project"), "{err}");
        assert!(
            set.add_tool_requests(&mut ToolRequestSet::default())
                .is_ok(),
            "tool resolution must survive an unresolvable import"
        );
    }

    #[test]
    fn missing_referenced_projects_name_the_path_and_the_setting() {
        let _serial = import_lock();
        let tmp = tempfile::tempdir().unwrap();
        let app = tmp.path().join("app");
        std::fs::create_dir_all(&app).unwrap();
        let source = app.join("mise.toml");
        let config = files(&[(
            source.to_str().unwrap(),
            "[daemons.worker]\nproject = '../mirror-pipeline'\n",
        )]);
        let err = load(&config).unwrap().import_errors["worker"].clone();
        assert!(err.contains(&tmp.path().join("mirror-pipeline").display().to_string()));
        assert!(err.contains("[daemons.worker].project"));

        let empty = tmp.path().join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        let config = files(&[(
            source.to_str().unwrap(),
            "[daemons.worker]\nproject = '../empty'\n",
        )]);
        assert!(load(&config).unwrap().import_errors["worker"].contains("mise configuration"));

        referenced_project(
            &tmp.path().join("other"),
            "[daemons.build]\nrun = 'exec build'\n",
        );
        let config = files(&[(
            source.to_str().unwrap(),
            "[daemons.worker]\nproject = '../other'\n",
        )]);
        let err = load(&config).unwrap().import_errors["worker"].clone();
        assert!(err.contains("[daemons.worker]"), "{err}");
        assert!(err.contains("build"), "{err}");
    }

    #[test]
    fn referenced_projects_inherit_daemons_and_namespaces_from_parent_configs() {
        let _serial = import_lock();
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
        let _serial = import_lock();
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
        let _serial = import_lock();
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
        let _serial = import_lock();
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
    fn an_untrusted_referenced_project_is_not_imported() {
        let _serial = import_lock();
        // This exercises the real trust gate. `mise x`, `mise run` and
        // `mise daemons start` mark the active config implicitly trusted, and
        // reading a sibling through that branch would grant it durable trust
        // with no prompt.
        let tmp = tempfile::tempdir().unwrap();
        let mirror = tmp.path().join("mirror");
        untrusted_project(&mirror, "[daemons.worker]\nrun = 'exec worker'\n");
        let app = tmp.path().join("app");
        std::fs::create_dir_all(&app).unwrap();
        let config = files(&[(
            app.join("mise.toml").to_str().unwrap(),
            "[daemons.worker]\nproject = '../mirror'\n[daemons.api]\nrun = 'exec api'\ndepends = ['worker']\n",
        )]);
        let config_path = mirror.join(&*crate::env::MISE_DEFAULT_CONFIG_FILENAME);
        // `is_trusted` trusts everything under `cfg!(test)`, except in paranoid
        // mode, where trust is bound to file contents and checked first. That is
        // the only way to exercise this gate without the bypass.
        let _paranoid = Paranoid::on();
        let set = load(&config).unwrap();
        assert!(set.find("worker").is_none());
        let err = &set.import_errors["worker"];
        assert!(err.contains("not trusted"), "{err}");
        assert!(err.contains("mise trust"), "{err}");
        // Trying did not trust it as a side effect, which is the whole point:
        // the commands that reach here mark the active config implicitly
        // trusted, and that branch would have granted it durably.
        assert!(!crate::config::config_file::is_path_trusted(&config_path));
        // A dependency on an import that never resolved would name a daemon
        // pitchfork cannot find, so it is dropped rather than registered, and
        // the daemon that needed it is recorded so starting it can refuse.
        assert!(!set.daemons["api"].table.contains_key("depends"));
        assert_eq!(set.blocked.get("api").map(String::as_str), Some("worker"));

        // Trusting it makes the same configuration import.
        crate::config::config_file::trust(&config_path).unwrap();
        let set = load(&config).unwrap();
        assert_eq!(set.find("worker").map(|d| d.imported), Some(true));
    }

    /// Turns on `paranoid` for one test and restores the settings on drop, even
    /// if the test panics.
    struct Paranoid;

    impl Paranoid {
        fn on() -> Self {
            use confique::Layer;
            let mut settings = crate::config::settings::SettingsPartial::empty();
            settings.paranoid = Some(true);
            crate::config::Settings::reset(Some(settings));
            Self
        }
    }

    impl Drop for Paranoid {
        fn drop(&mut self) {
            crate::config::Settings::reset(None);
        }
    }

    #[test]
    fn referenced_projects_with_tool_versions_files_still_import() {
        let _serial = import_lock();
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
        let _serial = import_lock();
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
            load(&config).unwrap().import_errors["worker"]
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
        // Two names that normalize onto one variable are ambiguous, so
        // neither is exported and both daemons keep working. Failing the load
        // would take `mise env` down for the whole project over a convenience.
        let set = load(&files(&[(
            root.join("mise.toml").to_str().unwrap(),
            "[daemons.web-ui]\nrun = 'a'\nport = 3000\n[daemons.web_ui]\nrun = 'b'\nport = 3001\n",
        )]))
        .unwrap();
        assert!(set.daemons["web-ui"].exports.is_empty());
        assert!(set.daemons["web_ui"].exports.is_empty());
        // Both still have their ports; only the variable is withheld.
        assert_eq!(set.daemons["web-ui"].port.unwrap().port, 3000);
        assert_eq!(set.daemons["web_ui"].port.unwrap().port, 3001);

        // A shell cannot export a name starting with a digit, but that name was
        // legal before this export existed, so it keeps working without one.
        let set = load(&files(&[(
            root.join("mise.toml").to_str().unwrap(),
            "[daemons.9api]\nrun = 'a'\nport = 3000\n",
        )]))
        .unwrap();
        assert!(set.daemons["9api"].exports.is_empty());
        assert_eq!(set.daemons["9api"].port.unwrap().port, 3000);
        assert_eq!(
            set.daemons["9api"].table["port"]["expect"][0].as_integer(),
            Some(3000)
        );

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
