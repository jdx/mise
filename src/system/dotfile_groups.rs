//! `[dotfile_groups]` — named directory trees of dotfiles, and the
//! `[bootstrap] dotfile_groups` selection of which groups a machine applies.
//!
//! ```toml
//! [dotfile_groups.zsh]                   # ~/.dotfiles/zsh/* -> ~/*
//! [dotfile_groups.home]
//! source = "home"
//! target = "~"
//! mode = "symlink-each"
//! dot_prefix = true
//! exclude = ["README.md"]
//! paths.".config/kitty" = { mode = "symlink" } # link the directory itself
//!
//! [dotfiles]
//! "~/.work.gitconfig" = { source = "work/gitconfig", group = "work" }
//!
//! [bootstrap]
//! dotfile_groups = ["home", "zsh"]       # unset: every group applies
//! ```
//!
//! A group expands into ordinary [`FileRequest`]s tagged with its name, so
//! apply, status, and unapply treat its files like any other entry. Each
//! apply also records what a group deployed under
//! `$MISE_STATE_DIR/dotfiles/groups/`, so its files can still be found once
//! the group is deselected or removed from config: `mise dot status` reports
//! them as orphaned, `mise dot apply --prune` removes them, and
//! `mise dot unapply --group` removes a group's files whether or not it is
//! still configured.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Component, Path, PathBuf};

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
    /// source directory; relative paths resolve against `dotfiles.root`,
    /// and the default is the group name
    #[serde(default)]
    source: Option<String>,
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
    /// target-relative path -> override for that part of the tree
    #[serde(default)]
    paths: IndexMap<String, PathOverrideToml>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct PathOverrideToml {
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    exclude: Option<Vec<String>>,
    #[serde(default)]
    permissions: Option<String>,
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

/// Check a `group = "..."` value on an ordinary `[dotfiles]` entry.
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
                "[bootstrap] dotfile_groups selects {name:?}, but no [dotfile_groups] table or [dotfiles] entry declares it"
            );
        }
    }
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
                warn_once!("[dotfile_groups].{name}: {err}, ignoring group");
                continue;
            }
        };
        match expand_group(&name, group, &base, &origin) {
            Ok(requests) => out.extend(requests),
            Err(err) => warn_once!("[dotfile_groups].{name}: {err}, ignoring group"),
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
    let source = group_source(group.source.as_deref().unwrap_or(name));
    let target_raw = group.target.unwrap_or_else(|| "~".to_string());
    if mode == FileMode::Symlink && !group.paths.is_empty() {
        bail!("paths overrides need a mode that walks the tree (symlink-each or copy)");
    }

    // overrides are written as target paths; their source spelling depends
    // on dot_prefix and on what the source tree already holds
    let mut overrides = vec![];
    for (key, value) in group.paths {
        let rel = override_rel(&key)?;
        let source_rel = source_rel_for(&source, &rel, group.dot_prefix);
        overrides.push((key, rel, source_rel, value));
    }

    let entry = |source: &Path,
                 mode: FileMode,
                 exclude: Vec<String>,
                 dot_prefix: bool,
                 walks: bool,
                 permissions: Option<String>| FileTomlEntry::Table {
        source: Some(source.to_string_lossy().to_string()),
        content: None,
        mode: Some(mode.name().to_string()),
        exclude: (!exclude.is_empty()).then_some(exclude),
        include: None,
        manifest: if walks { group.manifest.clone() } else { None },
        permissions,
        autosave: None,
        encrypt: None,
        allow_plaintext: None,
        variants: None,
        enabled: None,
        remove_empty: None,
        dot_prefix: dot_prefix.then_some(true),
        relative: if matches!(mode, FileMode::Symlink | FileMode::SymlinkEach) {
            group.relative
        } else {
            None
        },
        group: Some(name.to_string()),
    };

    let mut merged = IndexMap::new();
    // the tree itself, minus every overridden subtree
    let mut exclude = group.exclude.clone();
    exclude.extend(
        overrides
            .iter()
            .map(|(_, _, source_rel, _)| format!("/{}", glob_escape_rel(source_rel))),
    );
    files::merge_group_entry(
        target_raw.clone(),
        entry(
            &source,
            mode,
            exclude,
            group.dot_prefix && mode != FileMode::Symlink,
            mode != FileMode::Symlink,
            None,
        ),
        base,
        origin,
        &mut merged,
    );
    for (key, rel, source_rel, value) in &overrides {
        let sub_mode = match value.mode.as_deref() {
            None => mode,
            Some(m @ ("symlink" | "symlink-each" | "copy" | "template")) => {
                FileMode::parse(m).expect("validated override mode")
            }
            Some(m) => bail!(
                "paths.{key:?}: mode must be \"symlink\", \"symlink-each\", \"copy\", or \"template\", not {m:?}"
            ),
        };
        let sub_source = source.join(source_rel);
        let walks =
            matches!(sub_mode, FileMode::SymlinkEach | FileMode::Copy) && sub_source.is_dir();
        // an override's own list replaces the group's; otherwise patterns
        // that match any component still apply inside it, while anchored
        // ones name paths relative to the group root
        let mut sub_exclude = match &value.exclude {
            Some(exclude) => exclude.clone(),
            None => group
                .exclude
                .iter()
                .filter(|p| !p.contains('/') && !p.contains('\\'))
                .cloned()
                .collect(),
        };
        // a nested override owns its subtree, not this one
        for (_, other_rel, other_source_rel, _) in &overrides {
            if other_rel != rel
                && other_rel.starts_with(rel)
                && let Ok(nested) = other_source_rel.strip_prefix(source_rel)
            {
                sub_exclude.push(format!("/{}", glob_escape_rel(nested)));
            }
        }
        files::merge_group_entry(
            join_target_raw(&target_raw, rel),
            entry(
                &sub_source,
                sub_mode,
                sub_exclude,
                group.dot_prefix && walks,
                walks,
                value.permissions.clone(),
            ),
            base,
            origin,
            &mut merged,
        );
    }
    Ok(merged.into_values().collect())
}

/// Resolve a group's `source`: relative paths start at `dotfiles.root`.
fn group_source(source: &str) -> PathBuf {
    let source = file::replace_path(source);
    if source.is_relative() {
        files::dotfiles_root().join(source)
    } else {
        source
    }
}

/// A `paths` key as a relative path inside the group's target.
fn override_rel(key: &str) -> Result<PathBuf> {
    let rel = PathBuf::from(key.trim_start_matches("./").trim_end_matches('/'));
    if rel.as_os_str().is_empty() || rel.components().any(|c| !matches!(c, Component::Normal(_))) {
        bail!("paths.{key:?}: must be a relative path inside the group's target");
    }
    Ok(rel)
}

fn join_target_raw(target_raw: &str, rel: &Path) -> String {
    let rel = rel_to_slash(rel);
    if target_raw == "~" {
        format!("~/{rel}")
    } else {
        format!("{}/{rel}", target_raw.trim_end_matches(['/', '\\']))
    }
}

fn rel_to_slash(rel: &Path) -> String {
    rel.components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
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
/// With `group`, only that group is considered. Two groups at the same depth
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
pub(crate) fn record_applied(requests: &[FileRequest]) {
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
        record.paths.retain(|path| !roots.contains(&path.root));
        for req in reqs {
            for path in deployed_paths(req) {
                record.paths.retain(|p| p.target != path.target);
                record.paths.push(path);
            }
        }
        save_record(&record);
    }
}

/// Groups that deployed files but are not active now: deselected by
/// `[bootstrap] dotfile_groups`, or no longer declared.
pub fn orphaned(active: &[FileRequest]) -> Vec<GroupRecord> {
    let active = active
        .iter()
        .filter_map(|req| req.group.as_deref())
        .collect::<HashSet<_>>();
    load_records()
        .into_iter()
        .filter(|record| !active.contains(record.group.as_str()))
        .collect()
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
        file::remove_file(&path.target)?;
        journal::commit_changes(pending);
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
    if !opts.dry_run {
        save_record(&GroupRecord {
            paths: kept,
            ..record.clone()
        });
    }
    Ok(removed)
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
    fn override_keys_must_stay_inside_the_target() {
        assert_eq!(
            override_rel("./.config/kitty/").unwrap(),
            PathBuf::from(".config/kitty")
        );
        for key in ["", "/etc", "../x", ".config/../../x"] {
            assert!(override_rel(key).is_err(), "{key}");
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
    fn target_raw_joins_under_home_and_absolute_targets() {
        assert_eq!(
            join_target_raw("~", Path::new(".config/kitty")),
            "~/.config/kitty"
        );
        assert_eq!(join_target_raw("/etc/", Path::new("x")), "/etc/x");
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
