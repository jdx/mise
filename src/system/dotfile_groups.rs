//! `[dotfile_groups]` — named directory trees of dotfiles, and the
//! `[bootstrap] dotfile_groups` selection of which groups a machine applies.
//!
//! ```toml
//! [dotfile_groups.home]
//! root = "home"                          # ~/.dotfiles/home, walked into ~
//! dot_prefix = true
//! exclude = ["README.md"]
//!
//! [dotfile_groups.home.entries]          # whole-file entries, cut out of the walk
//! "~/.config/kitty" = { mode = "symlink" }       # link the directory itself
//! "~/.gitconfig" = { source = "git/config.tmpl", mode = "template" }
//!
//! [bootstrap]
//! dotfile_groups = ["home"]              # unset: every group applies
//! ```
//!
//! An entry without a source finds it under the group root at its path
//! inside the group target, and a relative source starts at the root. A
//! group expands into ordinary [`FileRequest`]s tagged with its name, so
//! apply, status, and unapply treat its files like any other entry. Each
//! apply also records what a group deployed under
//! `$MISE_STATE_DIR/dotfiles/groups/`, so its files can still be found once
//! the group stops deploying them: `mise dot status` reports them as
//! orphaned, `mise dot apply --prune` removes them, and
//! `mise dot unapply --group` removes a group's files whether or not it is
//! still configured.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};

use eyre::{Result, bail};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::config::ConfigMap;
use crate::dirs;
use crate::file;
use crate::path::PathExt;
use crate::system::files::{self, FileMode, FileRequest, FileTomlEntry};
use crate::system::history::journal;
use crate::system::resources::ResourceOrigin;

/// `[dotfile_groups]` as written in one config file: group name -> table.
/// Values stay TOML until expansion so a malformed group warns and is
/// skipped instead of failing the whole config file.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct DotfileGroupsTomlConfig(pub IndexMap<String, toml::Value>);

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct GroupToml {
    /// the group's directory tree (required); relative paths resolve
    /// against `dotfiles.root`
    #[serde(default)]
    root: Option<String>,
    /// target directory, `~` by default
    #[serde(default)]
    target: Option<String>,
    /// `symlink-each` (default), `copy`, or `symlink`
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    exclude: Vec<String>,
    #[serde(default)]
    dot_prefix: bool,
    #[serde(default)]
    manifest: Option<String>,
    #[serde(default)]
    relative: Option<bool>,
    /// whole-file entries keyed by target, cut out of the tree's walk
    #[serde(default)]
    entries: IndexMap<String, toml::Value>,
}

/// Group names become state file names and CLI arguments.
fn valid_group_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// Check a group name given on the command line.
pub fn validate_group_name(name: &str) -> Result<()> {
    if !valid_group_name(name) {
        bail!("invalid dotfile group name {name:?}: use letters, digits, '-', '_', and '.'");
    }
    Ok(())
}

/// The groups `[bootstrap] dotfile_groups` selects, or `None` when no layer
/// sets it and every group applies. `config_files` is ordered local ->
/// global, so the first list found is the most local, which replaces the
/// rest.
pub fn selection(config_files: &ConfigMap) -> Option<Vec<String>> {
    config_files
        .values()
        .find_map(|cf| cf.bootstrap_config().and_then(|b| b.dotfile_groups))
}

/// Whether `req` applies under `selection`. Ungrouped entries always do.
pub(crate) fn is_selected(req: &FileRequest, selection: Option<&[String]>) -> bool {
    match (&req.group, selection) {
        (Some(group), Some(selected)) => selected.iter().any(|s| s == group),
        _ => true,
    }
}

/// Warn about selected names nothing declares: a typo there would otherwise
/// silently deselect every group.
pub(crate) fn warn_unknown_selection(selection: &[String], requests: &[FileRequest]) {
    let known = requests
        .iter()
        .filter_map(|req| req.group.as_deref())
        .collect::<HashSet<_>>();
    for name in selection {
        if !known.contains(name.as_str()) {
            warn_once!(
                "[bootstrap] dotfile_groups selects {name:?}, but no [dotfile_groups] table declares it"
            );
        }
    }
}

/// Groups declared in config that could not be expanded, such as one
/// missing its `root`. Their recorded files are still the group's, so they
/// are never orphaned and pruned for a configuration mistake.
fn unexpanded() -> std::sync::MutexGuard<'static, HashSet<String>> {
    static UNEXPANDED: std::sync::LazyLock<std::sync::Mutex<HashSet<String>>> =
        std::sync::LazyLock::new(Default::default);
    UNEXPANDED.lock().unwrap_or_else(|err| err.into_inner())
}

/// Expand every `[dotfile_groups]` table in one config hierarchy into file
/// requests. A more local file's definition of a group replaces the whole
/// definition from a more global one.
pub(crate) fn group_requests(config_files: &ConfigMap) -> Vec<FileRequest> {
    let mut groups: IndexMap<String, (PathBuf, ResourceOrigin, toml::Value)> = IndexMap::new();
    for (path, cf) in config_files.iter().rev() {
        let Some(declared) = cf.dotfile_groups_config() else {
            continue;
        };
        let origin = ResourceOrigin {
            config: path.clone(),
            config_root: cf.config_root(),
            environment: crate::config::environments_for_config_path(path),
            source: None,
        };
        let base = path.parent().unwrap_or(Path::new(".")).to_path_buf();
        for (name, value) in declared.0 {
            groups.shift_remove(&name);
            groups.insert(name, (base.clone(), origin.clone(), value));
        }
    }
    let mut out = vec![];
    for (name, (base, origin, value)) in groups {
        if !valid_group_name(&name) {
            warn_once!(
                "[dotfile_groups].{name:?}: group names may contain only letters, digits, '-', '_', and '.', ignoring group"
            );
            continue;
        }
        let group = match value.try_into::<GroupToml>() {
            Ok(group) => group,
            Err(err) => {
                unexpanded().insert(name.clone());
                warn_once!("[dotfile_groups].{name}: {err}, ignoring group");
                continue;
            }
        };
        match expand_group(&name, group, &base, &origin) {
            Ok(requests) => out.extend(requests),
            Err(err) => {
                unexpanded().insert(name.clone());
                warn_once!("[dotfile_groups].{name}: {err}, ignoring group")
            }
        }
    }
    out
}

fn expand_group(
    name: &str,
    group: GroupToml,
    base: &Path,
    origin: &ResourceOrigin,
) -> Result<Vec<FileRequest>> {
    let mode = group.mode.as_deref().unwrap_or("symlink-each");
    if !matches!(mode, "symlink-each" | "copy" | "symlink") {
        bail!("mode must be \"symlink-each\", \"copy\", or \"symlink\", not {mode:?}");
    }
    let mode = FileMode::parse(mode).expect("validated group mode");
    let Some(root) = group.root.as_deref() else {
        bail!("root is required: the directory tree the group deploys");
    };
    let root = group_root(root);
    let target_raw = group.target.unwrap_or_else(|| "~".to_string());
    let target = files::resolve_target_arg(&target_raw);
    if mode == FileMode::Symlink && !group.entries.is_empty() {
        bail!("entries need a mode that walks the tree (symlink-each or copy)");
    }
    let unanchored_exclude = group
        .exclude
        .iter()
        .filter(|p| !p.contains('/') && !p.contains('\\'))
        .cloned()
        .collect::<Vec<_>>();

    let defaults = EntryDefaults {
        name,
        root: &root,
        target: &target,
        mode,
        manifest: group.manifest.as_deref(),
        dot_prefix: group.dot_prefix,
        relative: group.relative,
        unanchored_exclude: &unanchored_exclude,
    };
    let mut entries = vec![];
    for (key, value) in group.entries {
        entries.push(group_entry(&defaults, &key, value)?);
    }
    // an entry beneath a walking entry is cut out of its walk, as every
    // entry is cut out of the group's tree. Beneath anything else, such as
    // a directory linked whole, both claim the same path, and planning
    // reports the conflict.
    let nested = entries
        .iter()
        .map(|outer| {
            let mut cut = vec![];
            if !outer.walks {
                return cut;
            }
            for inner in entries.iter().filter(|inner| inner.target != outer.target) {
                // the nested entry's target, and its source when that lies
                // in this entry's tree, are the nested entry's to deploy
                if let Ok(rel) = inner.target.strip_prefix(&outer.target) {
                    cut.push(format!(
                        "/{}",
                        glob_escape_rel(&source_rel_for(&outer.source, rel, outer.dot_prefix))
                    ));
                }
                if let Ok(rel) = inner.source.strip_prefix(&outer.source)
                    && !rel.as_os_str().is_empty()
                {
                    cut.push(format!("/{}", glob_escape_rel(rel)));
                }
            }
            cut
        })
        .collect::<Vec<_>>();

    let mut merged = IndexMap::new();
    // the tree itself, minus every path an entry describes, and minus a
    // source inside the tree that an entry deploys somewhere else
    let mut exclude = group.exclude.clone();
    for entry in &entries {
        let rel = entry
            .target
            .strip_prefix(&target)
            .expect("entry inside target");
        exclude.push(format!(
            "/{}",
            glob_escape_rel(&source_rel_for(&root, rel, group.dot_prefix))
        ));
        if let Ok(source_rel) = entry.source.strip_prefix(&root)
            && !source_rel.as_os_str().is_empty()
        {
            exclude.push(format!("/{}", glob_escape_rel(source_rel)));
        }
    }
    let walks = mode != FileMode::Symlink;
    files::merge_group_entry(
        target_raw.clone(),
        FileTomlEntry::Table {
            source: Some(root.to_string_lossy().to_string()),
            content: None,
            mode: Some(mode.name().to_string()),
            exclude: (!exclude.is_empty()).then_some(exclude),
            include: None,
            manifest: if walks { group.manifest.clone() } else { None },
            permissions: None,
            autosave: None,
            encrypt: None,
            allow_plaintext: None,
            variants: None,
            enabled: None,
            remove_empty: None,
            dot_prefix: (group.dot_prefix && walks).then_some(true),
            relative: group.relative,
            group: Some(name.to_string()),
        },
        base,
        origin,
        &mut merged,
    );
    for (entry, cut) in entries.into_iter().zip(nested) {
        let mut table = entry.table;
        if !cut.is_empty() {
            let mut exclude = table
                .get("exclude")
                .and_then(toml::Value::as_array)
                .cloned()
                .unwrap_or_default();
            exclude.extend(cut.into_iter().map(toml::Value::String));
            table.insert("exclude".into(), toml::Value::Array(exclude));
        }
        files::merge_group_toml_entry(
            entry.key,
            toml::Value::Table(table),
            base,
            origin,
            &mut merged,
        )?;
    }
    Ok(merged.into_values().collect())
}

/// One `[dotfile_groups.<name>.entries]` declaration, completed from its
/// group: a missing source is found under the group root at the entry's
/// path inside the group target, a relative one starts at the group root,
/// and a missing mode deploys like the group does.
struct GroupEntry {
    key: String,
    target: PathBuf,
    source: PathBuf,
    walks: bool,
    dot_prefix: bool,
    table: toml::Table,
}

/// What a group's entries inherit from it.
struct EntryDefaults<'a> {
    name: &'a str,
    root: &'a Path,
    target: &'a Path,
    mode: FileMode,
    manifest: Option<&'a str>,
    dot_prefix: bool,
    relative: Option<bool>,
    /// the group's exclusions that match any path component, which apply
    /// inside an entry's tree too
    unanchored_exclude: &'a [String],
}

fn group_entry(defaults: &EntryDefaults, key: &str, value: toml::Value) -> Result<GroupEntry> {
    let EntryDefaults {
        name,
        root,
        target: group_target,
        mode: group_mode,
        manifest,
        dot_prefix,
        relative,
        unanchored_exclude,
    } = *defaults;
    let mut table = match value {
        toml::Value::String(source) => {
            toml::Table::from_iter([("source".to_string(), toml::Value::String(source))])
        }
        toml::Value::Table(table) => table,
        _ => bail!("entries.{key:?}: expected a string or table"),
    };
    if ["block", "line", "template", "comment", "position", "merge"]
        .iter()
        .any(|k| table.contains_key(*k))
    {
        bail!(
            "entries.{key:?}: a group holds whole-file entries; declare block, line, and merge edits in [dotfiles]"
        );
    }
    match table.get("group").map(|g| g.as_str()) {
        None => {}
        Some(Some(g)) if g == name => {}
        Some(_) => bail!("entries.{key:?}: belongs to group {name:?} and cannot name another"),
    }
    table.insert("group".into(), toml::Value::String(name.to_string()));
    let target = files::resolve_target_arg(key);
    let Ok(rel) = target.strip_prefix(group_target) else {
        bail!("entries.{key:?}: must be inside the group's target");
    };
    if rel.as_os_str().is_empty() {
        bail!("entries.{key:?}: must be a path inside the group's target, not the target itself");
    }
    let rel = rel.to_path_buf();
    // a variant may deploy elsewhere; every destination stays in the group
    for variant in table
        .get("variants")
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
    {
        if let Some(variant_target) = variant.get("target").and_then(toml::Value::as_str)
            && !files::resolve_target_arg(variant_target)
                .strip_prefix(group_target)
                .is_ok_and(|rel| !rel.as_os_str().is_empty())
        {
            bail!(
                "entries.{key:?}: variant target {variant_target:?} must be inside the group's target"
            );
        }
    }
    let mode = table
        .get("mode")
        .and_then(toml::Value::as_str)
        .map(str::to_string);
    let has_source = table.contains_key("source");
    let needs_source = !(has_source
        || table.contains_key("content")
        || matches!(mode.as_deref(), Some("absent" | "track" | "track-local"))
        // `{ permissions = "0600" }` alone manages an existing file's mode
        || mode.is_none() && table.contains_key("permissions"));
    let source = if let Some(source) = table.get("source").and_then(toml::Value::as_str) {
        let source = file::replace_path(source);
        Some(if source.is_relative() {
            root.join(source)
        } else {
            source
        })
    } else if needs_source {
        Some(root.join(source_rel_for(root, &rel, dot_prefix)))
    } else {
        None
    };
    let Some(source) = source else {
        return Ok(GroupEntry {
            key: key.to_string(),
            target,
            source: PathBuf::new(),
            walks: false,
            dot_prefix: false,
            table,
        });
    };
    table.insert(
        "source".into(),
        toml::Value::String(source.to_string_lossy().to_string()),
    );
    let mode = match mode {
        Some(mode) => mode,
        // deploy like the group: a directory the same way, a file as one
        // link (or one copy)
        None => {
            let mode = if source.is_dir() {
                group_mode
            } else if group_mode == FileMode::Copy {
                FileMode::Copy
            } else {
                FileMode::Symlink
            };
            table.insert("mode".into(), toml::Value::String(mode.name().into()));
            mode.name().to_string()
        }
    };
    let walks = matches!(mode.as_str(), "symlink-each" | "copy") && source.is_dir();
    // a tree inferred from the group's root reads like the group's tree
    let entry_dot_prefix = if walks && !table.contains_key("dot_prefix") && dot_prefix {
        table.insert("dot_prefix".into(), toml::Value::Boolean(true));
        true
    } else {
        table
            .get("dot_prefix")
            .and_then(toml::Value::as_bool)
            .unwrap_or(false)
    };
    if walks {
        if let Some(manifest) = manifest
            && !table.contains_key("manifest")
        {
            table.insert("manifest".into(), toml::Value::String(manifest.to_string()));
        }
        if !table.contains_key("exclude") && !unanchored_exclude.is_empty() {
            table.insert(
                "exclude".into(),
                toml::Value::Array(
                    unanchored_exclude
                        .iter()
                        .cloned()
                        .map(toml::Value::String)
                        .collect(),
                ),
            );
        }
    }
    if let Some(relative) = relative
        && matches!(mode.as_str(), "symlink" | "symlink-each")
        && !table.contains_key("relative")
    {
        table.insert("relative".into(), toml::Value::Boolean(relative));
    }
    Ok(GroupEntry {
        key: key.to_string(),
        target,
        source,
        walks,
        dot_prefix: entry_dot_prefix,
        table,
    })
}

/// Resolve a group's `root`: relative paths start at `dotfiles.root`.
fn group_root(root: &str) -> PathBuf {
    let root = file::replace_path(root);
    if root.is_relative() {
        files::dotfiles_root().join(root)
    } else {
        root
    }
}

fn glob_escape_rel(rel: &Path) -> String {
    rel.components()
        .map(|c| glob::Pattern::escape(&c.as_os_str().to_string_lossy()))
        .collect::<Vec<_>>()
        .join("/")
}

/// The source path, relative to a group's source, that deploys to the
/// target-relative path `rel`. With `dot_prefix`, a `.name` component is
/// stored as `dot-name`, unless the source tree already has it as `.name`
/// (and not as `dot-name`), since both deploy to the same place.
pub fn source_rel_for(source: &Path, rel: &Path, dot_prefix: bool) -> PathBuf {
    if !dot_prefix {
        return rel.to_path_buf();
    }
    let mut dir = source.to_path_buf();
    let mut out = PathBuf::new();
    for component in rel.components() {
        let name = component.as_os_str().to_string_lossy().to_string();
        let chosen = match name.strip_prefix('.') {
            Some(rest) if !rest.is_empty() && rest != "." => {
                let prefixed = format!("dot-{rest}");
                if dir.join(&name).symlink_metadata().is_ok()
                    && dir.join(&prefixed).symlink_metadata().is_err()
                {
                    name
                } else {
                    prefixed
                }
            }
            _ => name,
        };
        dir.push(&chosen);
        out.push(&chosen);
    }
    out
}

/// The group request a new file at `target` belongs in: the deepest grouped
/// tree whose target contains it and that would deploy it (not excluded).
/// With `group`, only that group is considered. Among groups at the same
/// depth, the one whose source already holds the file wins; otherwise they
/// are ambiguous and need `--group`.
pub fn route_add<'a>(
    requests: &'a [FileRequest],
    target: &Path,
    group: Option<&str>,
) -> Result<Option<&'a FileRequest>> {
    let mut candidates = requests
        .iter()
        .filter(|req| {
            req.group.is_some()
                && group.is_none_or(|g| req.group.as_deref() == Some(g))
                && matches!(req.mode, FileMode::SymlinkEach | FileMode::Copy)
                && (req.source.is_dir() || !req.source.exists())
                && target != req.target
                && target.starts_with(&req.target)
        })
        .filter(|req| {
            let rel = target
                .strip_prefix(&req.target)
                .expect("filtered by prefix");
            let source_rel = source_rel_for(&req.source, rel, req.dot_prefix);
            !files::is_excluded(&source_rel, &req.exclude)
        })
        .collect::<Vec<_>>();
    let Some(depth) = candidates
        .iter()
        .map(|req| req.target.components().count())
        .max()
    else {
        if let Some(group) = group {
            bail!(
                "{}: no tree of dotfile group {group:?} covers this path",
                target.display_user()
            );
        }
        return Ok(None);
    };
    candidates.retain(|req| req.target.components().count() == depth);
    // a file one group's source already holds belongs to that group
    let in_source = |req: &&FileRequest| {
        let rel = target
            .strip_prefix(&req.target)
            .expect("filtered by prefix");
        req.source
            .join(source_rel_for(&req.source, rel, req.dot_prefix))
            .symlink_metadata()
            .is_ok()
    };
    if candidates.len() > 1 && candidates.iter().any(in_source) {
        candidates.retain(in_source);
    }
    if candidates.len() > 1 {
        let names = candidates
            .iter()
            .filter_map(|req| req.group.as_deref())
            .collect::<BTreeSet<_>>();
        bail!(
            "{}: dotfile groups {} all cover this path; choose one with --group",
            target.display_user(),
            names.into_iter().collect::<Vec<_>>().join(", ")
        );
    }
    Ok(candidates.pop())
}

const GROUP_RECORD_VERSION: u8 = 1;

/// What one group deployed, kept under `$MISE_STATE_DIR/dotfiles/groups/`:
/// the ownership evidence that lets mise find and remove the group's files
/// after it leaves the config or the selection.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct GroupRecord {
    version: u8,
    pub group: String,
    pub paths: Vec<RecordedPath>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordedPath {
    pub target: PathBuf,
    /// the entry target the path was deployed under; emptied directories
    /// are removed up to it, never beyond
    pub root: PathBuf,
    /// for a link: the source it points at
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<PathBuf>,
    /// for a file: the sha256 of what mise wrote
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
}

impl RecordedPath {
    /// Whether the path still holds what the group deployed.
    pub fn is_owned(&self) -> bool {
        if !self.safe_to_remove() {
            return false;
        }
        if let Some(source) = &self.link {
            files::link_points_to(source, &self.target)
        } else if let Some(digest) = &self.digest {
            !self.target.is_symlink()
                && self.target.is_file()
                && crate::hash::file_hash_sha256(&self.target, None).is_ok_and(|d| &d == digest)
        } else {
            false
        }
    }

    fn exists(&self) -> bool {
        self.target.symlink_metadata().is_ok()
    }

    /// Whether removing the path deletes only what sits at it. A symlinked
    /// directory at or above it, up to and including the entry target it
    /// was deployed under, makes the path resolve somewhere else, such as
    /// into a dotfiles source after a tree was folded into one directory
    /// link. A path that physically lies inside `dotfiles.root` is a source
    /// file, whatever it was recorded as.
    fn safe_to_remove(&self) -> bool {
        !behind_symlink(&self.target, &self.root) && !inside_dotfiles_root(&self.target)
    }
}

fn records_dir() -> PathBuf {
    dirs::STATE.join("dotfiles").join("groups")
}

fn record_path(group: &str) -> PathBuf {
    records_dir().join(format!("{group}.toml"))
}

pub fn load_record(group: &str) -> Option<GroupRecord> {
    let path = record_path(group);
    if !path.exists() {
        return None;
    }
    match file::read_to_string(&path)
        .and_then(|body| toml::from_str::<GroupRecord>(&body).map_err(Into::into))
    {
        Ok(record) if record.version == GROUP_RECORD_VERSION && record.group == group => {
            Some(record)
        }
        Ok(_) => {
            warn!(
                "files: ignoring invalid dotfile group state {}",
                path.display_user()
            );
            None
        }
        Err(err) => {
            warn!(
                "files: failed to read dotfile group state {}: {err}",
                path.display_user()
            );
            None
        }
    }
}

/// Every group with a deployment record.
pub fn load_records() -> Vec<GroupRecord> {
    let Ok(entries) = std::fs::read_dir(records_dir()) else {
        return vec![];
    };
    entries
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let path = entry.path();
            (path.extension().is_some_and(|e| e == "toml"))
                .then(|| path.file_stem()?.to_str().map(str::to_string))
                .flatten()
        })
        .filter(|name| valid_group_name(name))
        .filter_map(|name| load_record(&name).map(|record| (name, record)))
        .collect::<BTreeMap<_, _>>()
        .into_values()
        .collect()
}

fn save_record(record: &GroupRecord) {
    let path = record_path(&record.group);
    let result = if record.paths.is_empty() {
        if path.exists() {
            file::remove_file(&path)
        } else {
            Ok(())
        }
    } else {
        (|| -> Result<()> {
            file::create_dir_all(records_dir())?;
            file::write_atomic(&path, toml::to_string_pretty(record)?)
        })()
    };
    if let Err(err) = result {
        warn!(
            "files: failed to write dotfile group state {}: {err}",
            path.display_user()
        );
    }
}

/// Paths `req` has deployed and still owns.
fn deployed_paths(req: &FileRequest) -> Vec<RecordedPath> {
    let link = |source: PathBuf, target: PathBuf| {
        files::link_points_to(&source, &target).then(|| RecordedPath {
            target,
            root: req.target.clone(),
            link: Some(source),
            digest: None,
        })
    };
    let written = |target: PathBuf| {
        (!target.is_symlink() && target.is_file())
            .then(|| crate::hash::file_hash_sha256(&target, None).ok())
            .flatten()
            .map(|digest| RecordedPath {
                target,
                root: req.target.clone(),
                link: None,
                digest: Some(digest),
            })
    };
    let walked = || files::directory_source_files(req).unwrap_or_default();
    match req.mode {
        FileMode::Symlink => link(req.source.clone(), req.target.clone())
            .into_iter()
            .collect(),
        FileMode::SymlinkEach => walked()
            .into_iter()
            .filter_map(|(source, target)| link(source, target))
            .collect(),
        FileMode::Copy if req.source.is_dir() => walked()
            .into_iter()
            .filter_map(|(_, target)| written(target))
            .collect(),
        FileMode::Copy | FileMode::Template | FileMode::Content => {
            written(req.target.clone()).into_iter().collect()
        }
        FileMode::Track | FileMode::Absent | FileMode::Permissions => vec![],
    }
}

/// After an apply, record what each group among `requests` now has on disk.
/// Paths under an applied entry's target are replaced by what it deployed
/// this time; the rest of the record (other entries of a partially applied
/// group) is kept.
pub(crate) fn record_applied<'a>(requests: impl IntoIterator<Item = &'a FileRequest>) {
    let mut by_group: IndexMap<&str, Vec<&FileRequest>> = IndexMap::new();
    for req in requests {
        if let Some(group) = &req.group {
            by_group.entry(group).or_default().push(req);
        }
    }
    for (group, reqs) in by_group {
        let mut record = load_record(group).unwrap_or_else(|| GroupRecord {
            group: group.to_string(),
            ..Default::default()
        });
        record.version = GROUP_RECORD_VERSION;
        let roots = reqs.iter().map(|req| &req.target).collect::<HashSet<_>>();
        // a path reached through a linked directory is not one mise can
        // ever remove safely, so it is neither recorded nor kept
        let deployed = reqs
            .iter()
            .flat_map(|req| deployed_paths(req))
            .filter(RecordedPath::safe_to_remove)
            .collect::<Vec<_>>();
        // a path the group no longer deploys but still owns, such as the
        // copy of a source file since deleted, stays recorded until removed
        record.paths.retain(|path| {
            !deployed.iter().any(|p| p.target == path.target)
                && path.safe_to_remove()
                && (!roots.contains(&path.root) || path.is_owned())
        });
        record.paths.extend(deployed);
        save_record(&record);
    }
}

/// After an entry failed partway, record the files of `req` it did write,
/// replacing what the record held for them, and nothing it did not write.
pub(crate) fn record_written(req: &FileRequest, written: &[PathBuf]) {
    let Some(group) = &req.group else {
        return;
    };
    let paths = deployed_paths(req)
        .into_iter()
        .filter(|path| written.contains(&path.target) && path.safe_to_remove())
        .collect::<Vec<_>>();
    if paths.is_empty() {
        return;
    }
    let mut record = load_record(group).unwrap_or_else(|| GroupRecord {
        group: group.to_string(),
        ..Default::default()
    });
    record.version = GROUP_RECORD_VERSION;
    record
        .paths
        .retain(|recorded| !paths.iter().any(|p| p.target == recorded.target));
    record.paths.extend(paths);
    save_record(&record);
}

/// What groups deployed that no active entry deploys now. That is every
/// file of a group deselected by `[bootstrap] dotfile_groups` or no longer
/// declared, and every file a still-active group stopped deploying: one its
/// `exclude` now skips, an entry removed from it, or the copy of a source
/// file since deleted. Each returned record holds only those paths.
///
/// A path an active entry deploys is never orphaned, whichever group
/// recorded it: a declaration moved from one group to another keeps its
/// file.
pub fn orphaned(active: &[FileRequest]) -> Vec<GroupRecord> {
    let records = load_records();
    if records.is_empty() {
        return vec![];
    }
    let claimed = claimed_paths(active);
    records
        .into_iter()
        // a declared group that failed to expand still owns its files
        .filter(|record| !unexpanded().contains(&record.group))
        .filter_map(|mut record| {
            record.paths.retain(|path| !claimed.contains(&path.target));
            (!record.paths.is_empty()).then_some(record)
        })
        .collect()
}

/// `record` without the paths an active entry outside its group deploys
/// now: what another group or an ungrouped entry took over is theirs, even
/// when it still matches what this group wrote.
pub fn without_claimed(record: &GroupRecord, active: &[FileRequest]) -> GroupRecord {
    let others = active
        .iter()
        .filter(|req| req.group.as_deref() != Some(record.group.as_str()));
    let claimed = claimed_paths(others);
    GroupRecord {
        paths: record
            .paths
            .iter()
            .filter(|path| !claimed.contains(&path.target))
            .cloned()
            .collect(),
        ..record.clone()
    }
}

/// Every target path an active entry deploys: its target, and each file of
/// a tree it walks. Tracking only observes a file, a permissions-only entry
/// only adjusts one, and an absent entry removes one, so none of them takes
/// a file over from the group that deployed it.
fn claimed_paths<'a>(active: impl IntoIterator<Item = &'a FileRequest>) -> HashSet<PathBuf> {
    let mut claimed = HashSet::new();
    for req in active {
        if matches!(
            req.mode,
            FileMode::Track | FileMode::Permissions | FileMode::Absent
        ) {
            continue;
        }
        claimed.insert(req.target.clone());
        if matches!(req.mode, FileMode::SymlinkEach | FileMode::Copy) && req.source.is_dir() {
            claimed.extend(
                files::directory_source_files(req)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|(_, target)| target),
            );
        }
    }
    claimed
}

/// Whether `target` is reached through a symlinked directory below `root`:
/// removing it would delete a file wherever that link points, such as in a
/// dotfiles source.
fn behind_symlink(target: &Path, root: &Path) -> bool {
    target
        .ancestors()
        .skip(1)
        .take_while(|dir| dir.starts_with(root))
        // a junction redirects a Windows directory the way a symlink does
        .any(file::is_symlink_or_junction)
}

/// Whether `target`'s directory physically lies inside `dotfiles.root`.
fn inside_dotfiles_root(target: &Path) -> bool {
    let (Some(parent), Ok(root)) = (target.parent(), files::dotfiles_root().canonicalize()) else {
        return false;
    };
    parent
        .canonicalize()
        .is_ok_and(|parent| parent.starts_with(root))
}

pub struct RemoveOpts {
    pub dry_run: bool,
    pub force: bool,
}

/// Remove what `record` deployed: links still pointing at their source and
/// files still holding what mise wrote (any file with `force`). Anything
/// else is left with a warning, and the record keeps it so a later
/// `--force` can still find it. Directories emptied on the way are removed
/// up to the entry target. Returns the removed paths.
pub fn remove_recorded(record: &GroupRecord, opts: &RemoveOpts) -> Result<Vec<PathBuf>> {
    let mut removed = vec![];
    let mut kept = vec![];
    for path in &record.paths {
        if !path.exists() {
            continue;
        }
        if !path.safe_to_remove() {
            warn!(
                "files: group {}: {} resolves through a linked directory or into dotfiles.root now, leaving it",
                record.group,
                path.target.display_user()
            );
            kept.push(path.clone());
            continue;
        }
        let removable =
            path.is_owned() || opts.force && path.digest.is_some() && !path.target.is_dir();
        if !removable {
            warn!(
                "files: group {}: {} changed since mise deployed it, leaving it{}",
                record.group,
                path.target.display_user(),
                if path.digest.is_some() {
                    " (use --force to remove it)"
                } else {
                    ""
                }
            );
            kept.push(path.clone());
            continue;
        }
        if opts.dry_run {
            miseprintln!("rm {}", path.target.display_user());
            continue;
        }
        let pending = journal::begin_changes(
            "dotfiles",
            &path.target.display_user(),
            [path.target.clone()],
        )?;
        let removal = remove_no_follow(&path.root, &path.target);
        // nothing changed on a failure, which closes the change as well
        journal::commit_changes(pending);
        if let Err(err) = removal {
            // one path that cannot be removed safely does not stop the rest
            warn!("files: group {}: {err:#}", record.group);
            kept.push(path.clone());
            continue;
        }
        info!(
            "files: removed {} (group {})",
            path.target.display_user(),
            record.group
        );
        removed.push(path.target.clone());
        if let Some(parent) = path.target.parent() {
            remove_empty_dirs(parent, &path.root)?;
        }
    }
    // `record` may be only part of what the group deployed (see
    // [`orphaned`]); the rest of the stored record stays
    if !opts.dry_run
        && let Some(mut stored) = load_record(&record.group)
    {
        let handled = record
            .paths
            .iter()
            .filter(|path| !kept.contains(path))
            .map(|path| &path.target)
            .collect::<HashSet<_>>();
        stored.paths.retain(|path| !handled.contains(&path.target));
        save_record(&stored);
    }
    Ok(removed)
}

/// Remove `target` without following a symlink on the way from `root`: each
/// directory is opened from the one above it with `O_NOFOLLOW`, and the file
/// is unlinked from the last, so a directory swapped for a link after the
/// safety check fails the removal instead of redirecting it into a source
/// tree.
#[cfg(unix)]
fn remove_no_follow(root: &Path, target: &Path) -> Result<()> {
    use eyre::WrapErr;
    use nix::fcntl::{OFlag, open, openat};
    use nix::sys::stat::Mode;
    use nix::unistd::{UnlinkatFlags, unlinkat};

    let failed = || format!("failed to remove {}", target.display_user());
    // a single-file entry's target is its own root: start from its parent
    let start = if target == root {
        root.parent().unwrap_or(root)
    } else {
        root
    };
    let rel = target.strip_prefix(start).wrap_err_with(failed)?;
    let mut components = rel.components().collect::<Vec<_>>();
    let Some(name) = components.pop() else {
        bail!("{}: nothing to remove", target.display_user());
    };
    let flags = OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC;
    // only directories at or below the root are never followed; the parent
    // of a single-file root, such as a symlinked ~/.config, may be a link
    let start_flags = if start == root {
        flags
    } else {
        flags - OFlag::O_NOFOLLOW
    };
    let mut dir = open(start, start_flags, Mode::empty()).wrap_err_with(failed)?;
    for component in components {
        dir = openat(&dir, component.as_os_str(), flags, Mode::empty()).wrap_err_with(failed)?;
    }
    unlinkat(&dir, name.as_os_str(), UnlinkatFlags::NoRemoveDir).wrap_err_with(failed)?;
    Ok(())
}

/// Windows has no descriptor-relative unlink to pin the directories down,
/// so the safety checks run again right before the removal: a directory
/// swapped for a link or junction since planning is refused, and only the
/// moment between this check and the removal remains.
#[cfg(not(unix))]
fn remove_no_follow(root: &Path, target: &Path) -> Result<()> {
    if behind_symlink(target, root) || inside_dotfiles_root(target) {
        bail!(
            "{} now resolves through a linked directory or into dotfiles.root",
            target.display_user()
        );
    }
    file::remove_file(target)
}

/// Remove empty directories from `dir` up to, but not including, `root`,
/// and never the home directory.
fn remove_empty_dirs(dir: &Path, root: &Path) -> Result<()> {
    let mut dir = Some(dir);
    while let Some(d) = dir {
        if d == root
            || !d.starts_with(root)
            || d == *dirs::HOME
            || !d.is_dir()
            || d.read_dir()?.next().is_some()
        {
            break;
        }
        file::remove_dir(d)?;
        dir = d.parent();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_names_are_path_safe() {
        for name in ["home", "work-laptop", "zsh_2", "a.b"] {
            assert!(valid_group_name(name), "{name}");
        }
        for name in ["", ".", "..", "a/b", "a b", "a\\b"] {
            assert!(!valid_group_name(name), "{name}");
        }
    }

    #[test]
    fn source_names_follow_dot_prefix_and_existing_spelling() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path();
        let rel = Path::new(".config/kitty/kitty.conf");
        assert_eq!(source_rel_for(source, rel, false), rel);
        assert_eq!(
            source_rel_for(source, rel, true),
            PathBuf::from("dot-config/kitty/kitty.conf")
        );
        // a tree that already keeps `.config` as written stays that way
        file::create_dir_all(source.join(".config")).unwrap();
        assert_eq!(
            source_rel_for(source, rel, true),
            PathBuf::from(".config/kitty/kitty.conf")
        );
        // the prefixed spelling wins when both exist
        file::create_dir_all(source.join("dot-config")).unwrap();
        assert_eq!(
            source_rel_for(source, rel, true),
            PathBuf::from("dot-config/kitty/kitty.conf")
        );
    }

    #[test]
    fn escaped_exclusions_match_literal_names() {
        let pattern = glob::Pattern::new(&format!(
            "/{}",
            glob_escape_rel(Path::new("dot-config/k[1]"))
        ))
        .unwrap();
        assert!(files::is_excluded(
            Path::new("dot-config/k[1]/x"),
            std::slice::from_ref(&pattern)
        ));
        assert!(!files::is_excluded(Path::new("dot-config/k1"), &[pattern]));
    }
}
