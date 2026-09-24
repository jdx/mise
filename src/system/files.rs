//! `[dotfiles]` — declarative config files (dotfiles) applied by
//! `mise dot apply` or `mise bootstrap`, and removed by
//! `mise dot unapply`.
//!
//! Entries are keyed by target path and point at a source file or directory,
//! resolved relative to the config file that declares them:
//!
//! ```toml
//! [dotfiles]
//! "~/.config/mise/config.toml" = { source = "config.toml", mode = "symlink" }
//! "~/.zshrc" = { mode = "symlink" }                      # implied source
//! "~/.gitconfig" = "dotfiles/gitconfig"                  # explicit source
//! "~/.config/foo.toml" = { mode = "copy" }               # implied source
//! "~/.ssh/config" = { source = "ssh.tmpl", mode = "template" }
//! "~/.config/nvim" = "dotfiles/nvim"                     # symlink the dir itself
//! "~/.local/bin" = { source = "bin", mode = "symlink-each" }
//! ```
//!
//! Like `[bootstrap.packages]`, entries merge across the config hierarchy
//! (global -> local, local overrides by target key) and are only ever
//! applied by an explicit command, never implicitly.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use eyre::{Result, WrapErr, bail};
use indexmap::IndexMap;
use itertools::Itertools;
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::config::{Config, ConfigMap, Settings};
use crate::dirs;
use crate::file;
use crate::hash::{hash_sha256_to_str, hash_to_str};
use crate::path::PathExt;
use crate::system::history::journal::{self, Capture};
use crate::system::resources::ResourceOrigin;
use crate::system::secrets::SecretValues;
use crate::ui::prompt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FileMode {
    /// symlink the target to the source — a file or the directory itself
    Symlink,
    /// source is a directory: recreate its directory structure under the
    /// target and symlink each file individually, so the target directory
    /// can also hold files mise doesn't manage
    SymlinkEach,
    /// copy the source file (or directory, recursively)
    Copy,
    /// render the source through the mise template engine and write the
    /// result (permissions are taken from the source file unless the entry
    /// sets `permissions`)
    Template,
    /// write literal content declared directly in mise.toml
    Content,
    /// the live file stays where it is; history protects (and shares) it
    Track,
    /// remove the target: a regular file or symlink is deleted, a directory
    /// is refused and never removed recursively
    Absent,
    /// set the permissions of an existing target without managing its
    /// content: an entry with `permissions` and no source, content, or mode
    Permissions,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FileManifest {
    Git,
}

impl FileManifest {
    fn parse(s: &str) -> Option<Self> {
        match s {
            "git" => Some(Self::Git),
            _ => None,
        }
    }
}

impl FileMode {
    pub(crate) fn parse(s: &str) -> Option<Self> {
        match s {
            "symlink" => Some(Self::Symlink),
            "symlink-each" => Some(Self::SymlinkEach),
            "copy" => Some(Self::Copy),
            "template" => Some(Self::Template),
            "track" => Some(Self::Track),
            "absent" => Some(Self::Absent),
            _ => None,
        }
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Symlink => "symlink",
            Self::SymlinkEach => "symlink-each",
            Self::Copy => "copy",
            Self::Template => "template",
            Self::Content => "content",
            Self::Track => "track",
            Self::Permissions => "permissions",
            Self::Absent => "absent",
        }
    }

    /// Whether requests in this mode read a source path. Inline content,
    /// permissions-only entries, and absent targets have none.
    pub(crate) fn has_source(self) -> bool {
        !matches!(self, Self::Content | Self::Permissions | Self::Absent)
    }
}

/// Parse a `permissions` value: an octal string such as `"0600"`, with the
/// same syntax `[bootstrap.files]` accepts for `mode`.
fn parse_permissions(value: &str) -> Result<u32> {
    crate::system::managed_files::parse_mode(Some(value), 0).map_err(|_| {
        eyre::eyre!(
            "permissions must be an octal string between \"0000\" and \"7777\", got {value:?}"
        )
    })
}

/// Why `permissions` cannot be combined with an entry's other keys, if it
/// cannot. `mode` is the declared mode, or `None` for a permissions-only entry.
fn permissions_conflict(
    mode: Option<FileMode>,
    exclude: bool,
    manifest: bool,
    encrypt: bool,
) -> Option<&'static str> {
    match mode {
        Some(FileMode::Track) => Some(
            "permissions is not supported with mode = \"track\"; history records a tracked file's mode itself",
        ),
        Some(FileMode::Symlink | FileMode::SymlinkEach) => Some(
            "permissions requires mode copy or template, or inline content; a symlink has no permissions of its own",
        ),
        Some(_) if manifest => Some("permissions is not supported with a manifest directory copy"),
        Some(_) => None,
        None if exclude || manifest || encrypt => {
            Some("a permissions-only entry takes no exclude, manifest, or encrypt")
        }
        None => None,
    }
}

/// Windows has no Unix permission bits; entries there ignore `permissions`.
#[cfg(not(unix))]
fn warn_permissions_ignored() {
    static WARNED: std::sync::Once = std::sync::Once::new();
    WARNED.call_once(|| warn!("[dotfiles]: permissions is ignored on this platform"));
}

/// How history treats a destination: whether edits are saved automatically,
/// whether the file's saved version is shared with other machines, and
/// whether it enters remote backups.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct FilePolicy {
    pub autosave: bool,
    pub encrypt: bool,
    /// Which fields the declaration wrote, so a later layer repeating it
    /// overrides only what it says and inherits the rest.
    pub explicit: ExplicitFields,
}

/// The fields a `[dotfiles]` declaration wrote explicitly.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ExplicitFields {
    pub autosave: bool,
    pub encrypt: bool,
    pub variants: bool,
    pub enabled: bool,
    pub exclude: bool,
    pub include: bool,
}

impl FilePolicy {
    /// Policy defaults do not enroll a deployment. Only explicit Track
    /// declarations observe files, regardless of how those files are deployed.
    pub(crate) fn for_mode(_mode: FileMode) -> Self {
        Self {
            autosave: true,
            encrypt: false,
            explicit: ExplicitFields::default(),
        }
    }
}

/// A `[dotfiles]` declaration history could not honour, reported instead
/// of silently ignored so failed enrollment is never mistaken for
/// protection.
#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct InvalidDeclaration {
    pub target: String,
    pub config: PathBuf,
    pub reason: String,
    /// Why the declaration is not in force.
    pub cause: Ignored,
}

/// **Two reasons to ignore a declaration, and only one of them is a
/// problem with the declaration.**
///
/// A rewrite has to tell them apart: replacing something mise could not
/// read would discard configuration nobody can see, while replacing
/// nothing — a `mode = "track"` entry in project configuration, which is
/// ignored by policy and always was — loses nothing at all.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Ignored {
    /// mise could not read it: a pattern that is not a glob, a mode it
    /// does not know, an encryption declaration that cannot hold.
    Unreadable,
    /// It reads fine and does not apply here.
    ByPolicy,
}

static INVALID_DECLARATIONS: std::sync::Mutex<Vec<InvalidDeclaration>> =
    std::sync::Mutex::new(Vec::new());

fn record_invalid(target: &str, config: &Path, reason: impl Into<String>) {
    record_ignored(target, config, reason, Ignored::Unreadable);
}

fn record_ignored(target: &str, config: &Path, reason: impl Into<String>, cause: Ignored) {
    let reason = reason.into();
    warn!("[dotfiles].\"{target}\": {reason}, ignoring entry");
    let mut invalid = INVALID_DECLARATIONS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if !invalid
        .iter()
        .any(|existing| existing.target == target && existing.config == config)
    {
        invalid.push(InvalidDeclaration {
            target: target.to_string(),
            config: config.to_path_buf(),
            reason,
            cause,
        });
    }
}

/// The declarations ignored while loading `[dotfiles]` in this process.
pub(crate) fn invalid_declarations() -> Vec<InvalidDeclaration> {
    INVALID_DECLARATIONS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

/// A configuration reload must not retain failures from a previous version.
pub(crate) fn clear_invalid_declarations() {
    INVALID_DECLARATIONS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clear();
}

/// Deployment variants reuse tracking selectors without changing history streams.
#[derive(Debug, Clone)]
pub(crate) struct FileVariant {
    target: Option<String>,
    selector: crate::system::history::select::Variant,
}

impl<'de> Deserialize<'de> for FileVariant {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // Flattening Variant would bypass its deny_unknown_fields check. Remove
        // our one additional field, then use the original strict selector parser.
        let mut table = toml::Table::deserialize(deserializer)?;
        let target = table
            .remove("target")
            .map(toml::Value::try_into)
            .transpose()
            .map_err(serde::de::Error::custom)?;
        let selector = toml::Value::Table(table)
            .try_into()
            .map_err(serde::de::Error::custom)?;
        Ok(Self { target, selector })
    }
}

/// Validate selector combinations and destination syntax before selecting a variant.
/// Returns a `dotfiles.root`-relative implied source for a logical entry key.
/// A permissions-only entry has no source, so none is implied for it.
fn validate_file_variants(
    target: &str,
    source: Option<&str>,
    content: Option<&str>,
    mode: Option<&str>,
    permissions_only: bool,
    variants: &[FileVariant],
) -> Result<Option<PathBuf>> {
    let selectors: Vec<_> = variants.iter().map(|v| v.selector.clone()).collect();
    crate::system::history::select::validate(&selectors)?;
    let has_target_override = variants.iter().any(|v| v.target.is_some());
    if has_target_override && mode == Some("track") {
        bail!("target overrides are not supported with mode = \"track\"");
    }
    if has_target_override && content.is_some() {
        bail!("destination variants with inline content are not supported");
    }
    // an absent entry removes its destination and reads no source
    let implied_source = if has_target_override
        && source.is_none()
        && !permissions_only
        && mode != Some("absent")
    {
        if !variants.iter().all(|v| v.target.is_some()) {
            bail!(
                "destination variants require an explicit source when any variant uses the entry key as its target"
            );
        }
        Some(logical_source_path(target)?)
    } else {
        None
    };
    if variants.is_empty() && resolve_target_arg(target).is_relative() {
        bail!("target must be absolute or start with ~/");
    }
    for variant in variants {
        let destination = variant.target.as_deref().unwrap_or(target);
        if !variant_target_is_absolute(destination) {
            bail!("variant target must be absolute or start with ~/");
        }
    }
    Ok(implied_source)
}

fn logical_source_path(key: &str) -> Result<PathBuf> {
    let path = file::replace_path(key);
    if !path.is_relative() {
        bail!(
            "destination variants require an explicit source unless the entry key is a relative source path"
        );
    }
    let mut relative = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::Normal(component) => relative.push(component),
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                bail!("an implied source entry key must not contain '..'");
            }
            std::path::Component::RootDir | std::path::Component::Prefix(_) => {
                bail!("an implied source entry key must be relative");
            }
        }
    }
    if relative.as_os_str().is_empty() {
        bail!("an implied source entry key must not be empty");
    }
    Ok(relative)
}

/// Inactive variants can contain another platform's absolute path syntax.
/// The selected destination is still checked with native path rules before use.
fn variant_target_is_absolute(target: &str) -> bool {
    let bytes = target.as_bytes();
    resolve_target_arg(target).is_absolute()
        || target.starts_with('/')
        || target.starts_with("\\\\")
        || (bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && matches!(bytes[2], b'/' | b'\\'))
}

/// One `[dotfiles]` whole-file entry as written in mise.toml.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub(crate) enum FileTomlEntry {
    /// `"~/.gitconfig" = "dotfiles/gitconfig"`
    Source(String),
    /// `"~/.gitconfig" = { source = "...", mode = "..." }` — every field is
    /// optional so `{}` can mean implied source/default mode. Mode stays a
    /// string here so configs using modes from newer mise versions still
    /// parse (they warn and are skipped, like unknown package managers)
    Table {
        #[serde(default)]
        source: Option<String>,
        #[serde(default)]
        content: Option<String>,
        #[serde(default)]
        mode: Option<String>,
        #[serde(default)]
        exclude: Option<Vec<String>>,
        /// history: capture only these paths of a tracked directory
        #[serde(default)]
        include: Option<Vec<String>>,
        #[serde(default)]
        manifest: Option<String>,
        /// octal permissions for the target, e.g. `"0600"`; on its own it
        /// manages only the permissions of an existing target
        #[serde(default)]
        permissions: Option<String>,
        /// history: save edits automatically (default true)
        #[serde(default)]
        autosave: Option<bool>,
        #[serde(default)]
        encrypt: Option<bool>,
        /// Platform / profile selectors, with optional deployment destinations
        #[serde(default)]
        variants: Option<Vec<FileVariant>>,
        /// `false` disables an inherited declaration on this machine
        #[serde(default)]
        enabled: Option<bool>,
        /// template only: remove the target when the template renders empty
        #[serde(default)]
        remove_empty: Option<bool>,
        /// directory-walking modes only: deploy a source name like
        /// `dot-bashrc` as `.bashrc`
        #[serde(default)]
        dot_prefix: Option<bool>,
        /// symlink modes only: link with a relative target, overriding
        /// `dotfiles.relative_symlinks`
        #[serde(default)]
        relative: Option<bool>,
    },
}

impl FileRequest {
    /// Takes from `later`, a repeat of this declaration from a later layer
    /// or file, only what it wrote explicitly (`enabled`, the policies, the
    /// variants); what it left unsaid stays inherited.
    fn override_from(&mut self, later: FileRequest) {
        let explicit = later.policy.explicit;
        if explicit.enabled {
            self.enabled = later.enabled;
        }
        if explicit.autosave {
            self.policy.autosave = later.policy.autosave;
        }
        if explicit.encrypt {
            self.policy.encrypt = later.policy.encrypt;
        }
        if explicit.variants {
            self.variants = later.variants;
        }
        if explicit.exclude {
            self.exclude = later.exclude;
        }
        if explicit.include {
            self.include = later.include;
        }
        // the later file is the effective declaration
        self.origin = later.origin;
        let mine = self.policy.explicit;
        self.policy.explicit = ExplicitFields {
            autosave: mine.autosave || explicit.autosave,
            encrypt: mine.encrypt || explicit.encrypt,
            variants: mine.variants || explicit.variants,
            enabled: mine.enabled || explicit.enabled,
            exclude: mine.exclude || explicit.exclude,
            include: mine.include || explicit.include,
        };
    }
}

/// one file entry, resolved against the config file that declared it
#[derive(Debug, Clone)]
pub(crate) struct FileRequest {
    /// target path as written in config (display/merge key)
    pub target_raw: String,
    /// absolute, lexically normalized target path (`~` expanded)
    pub target: PathBuf,
    /// absolute source path (relative sources resolve against the config
    /// file's directory; omitted sources resolve under dotfiles.root)
    pub source: PathBuf,
    /// literal whole-file content; present only for inline content entries
    pub content: Option<String>,
    pub mode: FileMode,
    /// glob patterns, matched against source-relative paths, for files a
    /// directory-walking mode should skip (see [`is_excluded`])
    pub exclude: Vec<glob::Pattern>,
    /// history: the only paths of a tracked directory that are captured,
    /// relative to it and matched like `exclude` (see [`is_excluded`]).
    ///
    /// `None` means no list was declared and the whole tree is captured.
    /// `Some` means one was, and only what it names is — including
    /// `Some([])`, which selects nothing. A declared list that happens to
    /// be empty must not be read as no list at all.
    pub include: Option<Vec<glob::Pattern>>,
    /// optional source manifest limiting which directory entries are managed
    pub manifest: Option<FileManifest>,
    /// permission bits the target must have, overriding what the mode would
    /// otherwise give it (Unix only)
    pub permissions: Option<u32>,
    /// directory of the declaring config file — base dir for template
    /// functions like `exec` and `read_file`
    pub base: PathBuf,
    pub origin: ResourceOrigin,
    /// how history treats the destination
    pub policy: FilePolicy,
    /// platform / profile streams of a tracked file
    pub variants: Vec<crate::system::history::select::Variant>,
    /// `false` when a later layer disabled the declaration
    pub enabled: bool,
    /// template only: an empty (whitespace-only) render removes the target
    /// instead of writing an empty file
    pub remove_empty: bool,
    /// directory-walking modes only: each source path component named
    /// `dot-<name>` is deployed as `.<name>`, like GNU Stow's `--dotfiles`
    pub dot_prefix: bool,
    /// symlink modes only: links point at the source by a path relative to
    /// the link's directory (see [`relative_link_path`])
    pub relative: bool,
}

const SYMLINK_EACH_STATE_VERSION: u8 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ManagedLink {
    source: PathBuf,
    target: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct SymlinkEachState {
    version: u8,
    source: PathBuf,
    target: PathBuf,
    links: Vec<ManagedLink>,
}

#[derive(Debug)]
struct SymlinkEachReconciliation {
    stale_links: Vec<ManagedLink>,
    targets: Vec<PathBuf>,
}

enum LoadedSymlinkEachState {
    Missing,
    Invalid,
    Present(SymlinkEachState),
}

const TARGET_STATE_VERSION: u8 = 1;

/// What mise knows about a single-file target it wrote, keyed by the target
/// path and kept under `$MISE_STATE_DIR/dotfiles/targets/`. It is the
/// ownership evidence for removing a target mise no longer wants: a file is
/// only removed without `--force` when it still holds what mise last wrote.
/// New fields must default, so a record written by an older mise still
/// loads; bump the version only for an incompatible change.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
struct TargetState {
    version: u8,
    target: PathBuf,
    /// sha256 of the content mise last wrote to the target
    content_digest: Option<String>,
    /// directories mise created to hold the target. Only these are removed
    /// with the target, and only once they are empty; a record without them
    /// (or from an older mise) removes none.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    created_dirs: Vec<PathBuf>,
}

/// What an empty template render means for the target currently on disk.
#[derive(Debug, PartialEq, Eq)]
enum EmptyRenderTarget {
    /// nothing there: already converged
    Absent,
    /// an empty file, or the content mise last wrote: safe to remove
    Owned,
    /// anything else needs `--force`; the reason is human-readable
    Conflict(&'static str),
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum FileState {
    Applied,
    Missing,
    /// target exists but doesn't match — the reason is human-readable
    Differs(String),
    SourceMissing,
    /// a tracked file: nothing to apply, history protects it where it is
    Tracked,
}

/// Aggregate whole-file `[dotfiles]` entries across all loaded config files.
/// Keys union global -> local; a more local config overrides an entry for the
/// same target. Malformed entries and unknown modes warn and are skipped.
pub(crate) fn files_from_config(config: &Config) -> Result<Vec<FileRequest>> {
    Ok(composed_files_from_config(config)?
        .into_iter()
        .filter(|request| request.enabled)
        .collect())
}

/// Keep disabled declarations so explicit tracking removal remains observable
/// when the prior enrollment exists in Git rather than local configuration.
pub(crate) fn composed_files_from_config(config: &Config) -> Result<Vec<FileRequest>> {
    let mut composed: IndexMap<PathBuf, Vec<FileRequest>> = IndexMap::new();
    let trusted_roots = global_composed_roots(config);
    for config_files in config.bootstrap_config_maps() {
        for request in
            files_from_config_files_with_tracking_roots(config_files, Some(&trusted_roots))
        {
            let siblings = composed.entry(request.target.clone()).or_default();
            if let Some(existing) = siblings
                .iter_mut()
                .find(|existing| file_requests_match(config, existing, &request))
            {
                // the same declaration from a later layer: what it says
                // about `enabled`, the policies, and the variants wins, so a
                // local `enabled = false` overrides what
                // it inherited instead of being dropped as a duplicate; what
                // it leaves unsaid stays inherited
                existing.override_from(request);
                continue;
            }
            if let Some(existing) = siblings.iter().find(|existing| {
                existing.mode != FileMode::Track
                    && request.mode != FileMode::Track
                    && (existing.mode != FileMode::SymlinkEach
                        || request.mode != FileMode::SymlinkEach)
            }) {
                bail!(
                    "conflicting dotfile declarations for {}\n\n  first:\n    {}\n\n  second:\n    {}",
                    request.target.display(),
                    existing.origin.conflict_description(),
                    request.origin.conflict_description(),
                );
            }
            siblings.push(request);
        }
    }
    Ok(composed.into_values().flatten().collect())
}

/// Whether a declaration comes from the system or global layers (or a root
/// they compose): the only layers history enrolls files from.
pub(crate) fn declaration_is_global(config: &Config, req: &FileRequest) -> bool {
    track_layer_allowed(&req.origin, &global_composed_roots(config))
}

/// Invalid project declarations must not block personal history either.
pub(crate) fn tracking_config_is_global(config: &Config, path: &Path) -> bool {
    crate::config::is_global_config(path)
        || global_composed_roots(config)
            .iter()
            .any(|root| path.starts_with(root))
}

/// Whether a track declaration comes from a layer allowed to enroll files.
fn track_layer_allowed(origin: &ResourceOrigin, trusted_roots: &[PathBuf]) -> bool {
    crate::config::is_global_config(&origin.config)
        || trusted_roots
            .iter()
            .any(|root| origin.config_root == *root || origin.config.starts_with(root))
}

/// The bootstrap config roots composed by system and global configuration.
fn global_composed_roots(config: &Config) -> Vec<PathBuf> {
    let mut roots = vec![];
    for (root, config_files) in config.selected_bootstrap_config_maps() {
        let declared_globally = config_files
            .keys()
            .any(|path| crate::config::is_global_config(path) && !path.starts_with(root));
        if declared_globally {
            roots.push(root.to_path_buf());
        }
    }
    roots
}

/// Validate the complete paths claimed by composed `[dotfiles]` entries.
/// Directory copies and `symlink-each` entries may share directories, but no
/// two entries may own the same leaf or require a directory where another
/// entry places a leaf.
pub(crate) fn validate_composed_file_footprints(requests: &[FileRequest]) -> Result<()> {
    let mut leaves: IndexMap<PathBuf, &FileRequest> = IndexMap::new();
    let mut directories: IndexMap<PathBuf, &FileRequest> = IndexMap::new();
    let mut symlink_each_identities: HashMap<(&Path, &Path), &FileRequest> = HashMap::new();
    let mut permissions_only = vec![];

    for request in requests {
        // Tracking observes native files; it does not own an apply leaf.
        // A tracked parent may contain independently managed destinations.
        if request.mode == FileMode::Track {
            continue;
        }
        // A permissions-only entry creates nothing, so a directory it
        // chmods may hold other entries' files. It still must not fight
        // another entry over the permissions of a file that entry writes.
        if request.mode == FileMode::Permissions {
            permissions_only.push(request);
            continue;
        }
        if request.permissions.is_some() && request.source.is_dir() {
            bail!(
                "[dotfiles].\"{}\": permissions requires a file source, not a directory: {}",
                request.target_raw,
                request.source.display_user()
            );
        }
        if request.manifest.is_some() && request.source.exists() && !request.source.is_dir() {
            bail!(
                "[dotfiles].\"{}\": manifest requires the source to be a directory: {}",
                request.target_raw,
                request.source.display_user()
            );
        }
        if request.dot_prefix && request.source.exists() && !request.source.is_dir() {
            return Err(dot_prefix_file_source(request));
        }
        if request.mode == FileMode::SymlinkEach
            && let Some(existing) = symlink_each_identities.insert(
                (request.source.as_path(), request.target.as_path()),
                request,
            )
        {
            return Err(composed_file_footprint_conflict(
                &request.target,
                existing,
                request,
            ));
        }
        // A missing source has an unknown eventual shape, but it still claims
        // its target. Whole-resource modes reserve a leaf; symlink-each has a
        // known directory-shaped target even before its children are known.
        // An absent entry has no source and claims its target as a leaf, so
        // declaring the same path present elsewhere (or a file beneath it)
        // conflicts.
        let source_unavailable = request.mode.has_source()
            && (!request.source.exists()
                || request.mode == FileMode::SymlinkEach && !request.source.is_dir());
        let directory_walker = !source_unavailable
            && matches!(request.mode, FileMode::Copy | FileMode::SymlinkEach)
            && request.source.is_dir();
        let unresolved_directory = source_unavailable && request.mode == FileMode::SymlinkEach;
        let request_leaves = if unresolved_directory {
            vec![]
        } else if directory_walker {
            walk_source_files(request)?
                .into_iter()
                .map(|(_, target)| target)
                .collect::<Vec<_>>()
        } else {
            vec![request.target.clone()]
        };
        let mut request_directories = indexmap::IndexSet::new();
        for leaf in &request_leaves {
            request_directories.extend(leaf.ancestors().skip(1).map(Path::to_path_buf));
        }
        if directory_walker || unresolved_directory {
            request_directories.insert(request.target.clone());
            request_directories.extend(request.target.ancestors().skip(1).map(Path::to_path_buf));
        }

        for leaf in &request_leaves {
            if let Some(existing) = leaves.get(leaf).or_else(|| directories.get(leaf)) {
                return Err(composed_file_footprint_conflict(leaf, existing, request));
            }
        }
        for directory in &request_directories {
            if let Some(existing) = leaves.get(directory) {
                return Err(composed_file_footprint_conflict(
                    directory, existing, request,
                ));
            }
        }
        for leaf in request_leaves {
            leaves.insert(leaf, request);
        }
        for directory in request_directories {
            directories.entry(directory).or_insert(request);
        }
    }
    for request in permissions_only {
        if let Some(existing) = leaves.get(&request.target) {
            return Err(composed_file_footprint_conflict(
                &request.target,
                existing,
                request,
            ));
        }
    }
    Ok(())
}

/// Describe a footprint collision while preserving the established
/// `symlink-each` diagnostic for two contributors in that mode.
fn composed_file_footprint_conflict(
    path: &Path,
    first: &FileRequest,
    second: &FileRequest,
) -> eyre::Report {
    let kind = if first.mode == FileMode::SymlinkEach && second.mode == FileMode::SymlinkEach {
        "symlink-each"
    } else {
        "dotfile"
    };
    eyre::eyre!(
        "conflicting {kind} declarations for {}\n\n  first:\n    {}\n\n  second:\n    {}",
        path.display(),
        first.origin.conflict_description(),
        second.origin.conflict_description(),
    )
}

/// Returns whether sibling declarations produce the same whole-file resource.
fn file_requests_match(config: &Config, first: &FileRequest, second: &FileRequest) -> bool {
    first.target == second.target
        && first.source == second.source
        && first.content == second.content
        && first.mode == second.mode
        && first.manifest == second.manifest
        && first.remove_empty == second.remove_empty
        && first.dot_prefix == second.dot_prefix
        && first.relative == second.relative
        && first.permissions == second.permissions
        // a track entry's list is a policy a later layer may change, like
        // autosave; a deployment entry's list is part of what it deploys
        && (first.mode == FileMode::Track
            || first
                .exclude
                .iter()
                .map(glob::Pattern::as_str)
                .eq(second.exclude.iter().map(glob::Pattern::as_str)))
        && (first.mode != FileMode::Template
            || first.base == second.base
                && config.bootstrap_tera_ctx(&first.origin.config)
                    == config.bootstrap_tera_ctx(&second.origin.config))
}

/// Incoming configuration must fail preflight rather than silently dropping
/// malformed declarations and applying the rest of the setup.
pub(crate) fn validate_incoming_files(config_files: &ConfigMap) -> Result<()> {
    for (path, config) in config_files {
        let Some(dotfiles) = config.dotfiles_config() else {
            continue;
        };
        for (target, value) in dotfiles.0 {
            if value.as_table().is_some_and(|t| {
                t.contains_key("encrypt")
                    && t.get("encrypt").and_then(toml::Value::as_bool).is_none()
            }) {
                bail!("dotfile {target}: encrypt must be a boolean");
            }
            if value.as_table().is_some_and(|t| {
                t.get("encrypt").and_then(toml::Value::as_bool) == Some(true)
                    && ["content", "block", "line", "template"]
                        .iter()
                        .any(|key| t.contains_key(*key))
            }) {
                bail!(
                    "encrypted dotfile {target} requires an external source, not inline content or edits"
                );
            }
            let Some(entry) = file_entry_from_toml(&target, value.clone()) else {
                // Managed line/block edits are handled by the edit engine,
                // not by this whole-file declaration parser.
                if let Some(table) = value.as_table().filter(|table| {
                    ["block", "line", "template", "comment", "position"]
                        .iter()
                        .any(|key| table.contains_key(*key))
                }) {
                    for key in ["permissions", "relative"] {
                        if table.contains_key(key) {
                            bail!(
                                "dotfile {target}: {key} applies to whole-file entries, not block or line edits"
                            );
                        }
                    }
                    continue;
                }
                bail!("invalid dotfile declaration {target} in {}", path.display());
            };
            if let Some(table) = value.as_table() {
                for key in table.keys() {
                    if !matches!(
                        key.as_str(),
                        "source"
                            | "content"
                            | "mode"
                            | "exclude"
                            | "include"
                            | "manifest"
                            | "permissions"
                            | "autosave"
                            | "encrypt"
                            | "variants"
                            | "enabled"
                            | "remove_empty"
                            | "dot_prefix"
                            | "relative"
                    ) {
                        bail!(
                            "unknown dotfile key {key:?} for {target} in {}",
                            path.display()
                        );
                    }
                }
            }
            if let FileTomlEntry::Source(_) = &entry {
                validate_file_variants(&target, None, None, None, false, &[])?;
            }
            if let FileTomlEntry::Table {
                source,
                content,
                mode,
                manifest,
                exclude,
                encrypt,
                include,
                permissions,
                variants,
                remove_empty,
                dot_prefix,
                relative,
                ..
            } = entry
            {
                let permissions_only = permissions.is_some()
                    && source.is_none()
                    && content.is_none()
                    && mode.is_none();
                let implied_variant_source = validate_file_variants(
                    &target,
                    source.as_deref(),
                    content.as_deref(),
                    mode.as_deref(),
                    permissions_only,
                    variants.as_deref().unwrap_or_default(),
                )?;
                if content.is_some() && (mode.is_some() || exclude.is_some() || manifest.is_some())
                {
                    bail!(
                        "dotfile {target}: inline content does not support mode, exclude, or manifest"
                    );
                }
                let mode = match mode.as_deref() {
                    Some(value) => FileMode::parse(value).ok_or_else(|| {
                        eyre::eyre!("unknown dotfile mode {value:?} for {target}")
                    })?,
                    None => default_mode(),
                };
                if mode == FileMode::Track
                    && (source.is_some() || content.is_some() || manifest.is_some())
                {
                    bail!("tracked file {target} cannot declare source, content, or manifest");
                }
                if mode == FileMode::Absent
                    && (source.is_some()
                        || manifest.is_some()
                        || exclude.is_some()
                        || permissions.is_some()
                        || encrypt == Some(true))
                {
                    bail!(
                        "dotfile {target} with mode = \"absent\" cannot declare source, content, manifest, exclude, permissions, or encrypt"
                    );
                }
                // composition checks whichever destination a variant selects,
                // so every one of them must be a single path
                if mode == FileMode::Absent
                    && std::iter::once(target.as_str())
                        .chain(
                            variants
                                .iter()
                                .flatten()
                                .filter_map(|variant| variant.target.as_deref()),
                        )
                        .any(|path| is_glob_pattern(&resolve_target_arg(path)))
                {
                    bail!("dotfile {target}: an absent target cannot use wildcards");
                }
                if source.is_some() && content.is_some() {
                    bail!("dotfile {target} cannot declare both source and content");
                }
                if let Some(permissions) = &permissions {
                    parse_permissions(permissions)
                        .map_err(|err| eyre::eyre!("dotfile {target}: {err}"))?;
                    let declared = if permissions_only {
                        None
                    } else if content.is_some() {
                        Some(FileMode::Content)
                    } else {
                        Some(mode)
                    };
                    if let Some(reason) = permissions_conflict(
                        declared,
                        exclude.is_some(),
                        manifest.is_some(),
                        encrypt.is_some(),
                    ) {
                        bail!("dotfile {target}: {reason}");
                    }
                    if permissions_only && is_glob_pattern(&resolve_target_arg(&target)) {
                        bail!("dotfile {target}: a permissions-only target cannot use wildcards");
                    }
                }
                if remove_empty == Some(true)
                    && (content.is_some() || permissions_only || mode != FileMode::Template)
                {
                    bail!("dotfile {target}: remove_empty requires mode = \"template\"");
                }
                if dot_prefix == Some(true)
                    && (content.is_some()
                        || permissions_only
                        || !matches!(mode, FileMode::Copy | FileMode::SymlinkEach))
                {
                    bail!("dotfile {target}: dot_prefix requires mode copy or symlink-each");
                }
                if relative == Some(true)
                    && (content.is_some()
                        || permissions_only
                        || !matches!(mode, FileMode::Symlink | FileMode::SymlinkEach))
                {
                    bail!(
                        "dotfile {target}: relative requires mode = \"symlink\" or \"symlink-each\""
                    );
                }
                if !matches!(mode, FileMode::Track | FileMode::Absent)
                    && !permissions_only
                    && source.is_none()
                    && content.is_none()
                    && implied_variant_source.is_none()
                {
                    implied_source(&resolve_target_arg(&target))?;
                }
                if let Some(manifest) = manifest
                    && (FileManifest::parse(&manifest).is_none()
                        || !matches!(mode, FileMode::Copy | FileMode::SymlinkEach))
                {
                    bail!("invalid manifest {manifest:?} for dotfile {target}");
                }
                // **Preflight has to reject what composition would
                // drop, not just what it cannot parse.** An `include` on
                // a deployment entry is refused when the configuration is
                // composed, so accepting it here let a pull report
                // success while quietly leaving that entry out. Two
                // readers of one declaration must not disagree about
                // whether it is usable.
                if include.is_some() && mode != FileMode::Track {
                    bail!("dotfile {target}: include applies only to mode = \"track\"");
                }
                if include.is_some()
                    && std::fs::symlink_metadata(resolve_target_arg(&target))
                        .is_ok_and(|meta| !meta.is_dir())
                {
                    bail!(
                        "dotfile {target}: include selects paths inside a tracked directory; remove it from this file or track its parent directory"
                    );
                }
                // Both selection lists, not just one: an incoming
                // `include` this mise cannot compile must fail preflight
                // rather than be dropped, or the entry silently selects
                // the whole tree on the machine that receives it.
                for pattern in exclude
                    .into_iter()
                    .flatten()
                    .chain(include.into_iter().flatten())
                {
                    glob::Pattern::new(&pattern)?;
                }
            }
        }
    }
    Ok(())
}

/// An edit on a path an `absent` entry removes would recreate the file on
/// every apply, so the two contradict each other.
pub(crate) fn validate_absent_edit_targets(
    files: &[FileRequest],
    edits: &[crate::system::edits::EditRequest],
) -> Result<()> {
    for edit in edits {
        // absent targets are lexically normalized; compare the edit's path
        // the same way so a `..` spelling cannot slip past
        let path = lexical_normalize(&edit.path);
        if let Some(file) = files
            .iter()
            .find(|file| file.mode == FileMode::Absent && file.target == path)
        {
            bail!(
                "conflicting dotfile declarations for {}: mode = \"absent\" removes the file that the edit {} changes\n\n  absent:\n    {}\n\n  edit:\n    {}",
                edit.path.display_user(),
                edit.describe_op(),
                file.origin.conflict_description(),
                edit.origin.conflict_description(),
            );
        }
    }
    Ok(())
}

/// Aggregate `[dotfiles]` across a specific set of config files. This is
/// used by OCI builds, which intentionally scope config to project files by
/// default instead of blindly inheriting global dotfiles.
pub(crate) fn files_from_config_files(config_files: &ConfigMap) -> Vec<FileRequest> {
    files_from_config_files_with_tracking_roots(config_files, None)
}

/// Resolve deployment overrides while limiting history enrollment to trusted roots.
fn files_from_config_files_with_tracking_roots(
    config_files: &ConfigMap,
    tracking_roots: Option<&[PathBuf]>,
) -> Vec<FileRequest> {
    // keyed by the *expanded* target so "~/.gitconfig" in one config and
    // its absolute spelling in another are one entry, not two
    let mut merged: IndexMap<(PathBuf, bool), FileRequest> = IndexMap::new();
    // A logical declaration must be overridden before selecting its destination:
    // otherwise a local override that changes the target would deploy both paths.
    let mut destination_declarations = IndexMap::new();
    let mut resolved_declarations = HashMap::new();
    for (path, cf) in config_files {
        let base = path.parent().unwrap_or(Path::new("."));
        let origin = ResourceOrigin {
            config: path.clone(),
            config_root: cf.config_root(),
            environment: crate::config::environments_for_config_path(path),
            source: None,
        };
        if let Some(dotfiles) = cf.dotfiles_config() {
            for (key, value) in dotfiles.0 {
                if value.get("mode").and_then(toml::Value::as_str) == Some("track") {
                    continue;
                }
                let mut requests = IndexMap::new();
                if let Some(entry) = parse_file_entry(&key, value, path) {
                    let overrides_target = matches!(&entry,
                        FileTomlEntry::Table { variants: Some(vs), .. }
                            if vs.iter().any(|v| v.target.is_some()));
                    merge_file_entry(key.clone(), entry, base, &origin, &mut requests);
                    // An invalid or inactive declaration cannot suppress an
                    // inherited request. Cache resolution so sources are walked once.
                    if !requests.is_empty() {
                        let (_, has_override) = destination_declarations
                            .entry(resolve_target_arg(&key))
                            .or_insert((path, false));
                        *has_override |= overrides_target;
                    }
                }
                resolved_declarations.insert((path, key), requests);
            }
        }
    }
    // config_files is ordered local -> global; reverse for global -> local
    for (path, cf) in config_files.iter().rev() {
        let base = path.parent().unwrap_or(Path::new(".")).to_path_buf();
        let origin = ResourceOrigin {
            config: path.clone(),
            config_root: cf.config_root(),
            environment: crate::config::environments_for_config_path(path),
            source: None,
        };
        let Some(dotfiles) = cf.dotfiles_config() else {
            continue;
        };
        for (target_raw, value) in dotfiles.0 {
            if value.get("mode").and_then(toml::Value::as_str) != Some("track")
                && destination_declarations
                    .get(&resolve_target_arg(&target_raw))
                    .is_some_and(|(winner, has_override)| *has_override && *winner != path)
            {
                continue;
            }
            if tracking_roots.is_some_and(|roots| !track_layer_allowed(&origin, roots))
                && value.get("mode").and_then(toml::Value::as_str) == Some("track")
            {
                record_ignored(
                    &target_raw,
                    &origin.config,
                    "tracking is enrolled from the global configuration only (ignored: project config)",
                    Ignored::ByPolicy,
                );
                continue;
            }
            if let Some(requests) = resolved_declarations.remove(&(path, target_raw.clone())) {
                merged.extend(requests);
            } else if let Some(entry) = parse_file_entry(&target_raw, value, path) {
                merge_file_entry(target_raw, entry, &base, &origin, &mut merged);
            }
        }
    }
    merged.into_values().collect()
}

/// Parse a whole-file declaration and reject unsupported encryption combinations.
fn parse_file_entry(target: &str, value: toml::Value, config: &Path) -> Option<FileTomlEntry> {
    if value.as_table().is_some_and(|t| {
        t.get("encrypt").and_then(toml::Value::as_bool) == Some(true)
            && ["content", "block", "line", "template"]
                .iter()
                .any(|key| t.contains_key(*key))
    }) {
        record_invalid(
            target,
            config,
            "encrypted dotfiles require an external source, not inline content or edits",
        );
        return None;
    }
    // Deserializing a whole-file entry drops the edit keys, so an edit that
    // also says `mode = "absent"` would silently become a removal of the
    // file it meant to edit.
    if value.as_table().is_some_and(|t| {
        t.get("mode").and_then(toml::Value::as_str) == Some("absent")
            && ["block", "line", "template", "comment", "position"]
                .iter()
                .any(|key| t.contains_key(*key))
    }) {
        record_invalid(
            target,
            config,
            "mode = \"absent\" removes the whole file and cannot be combined with block or line edits",
        );
        return None;
    }
    let encryption_declared = value
        .as_table()
        .is_some_and(|table| table.contains_key("encrypt"));
    let entry = file_entry_from_toml(target, value);
    if entry.is_none() && encryption_declared {
        record_invalid(
            target,
            config,
            "invalid encryption declaration; encrypt must be a boolean on a whole-file entry",
        );
    }
    entry
}

fn file_entry_from_toml(target_raw: &str, value: toml::Value) -> Option<FileTomlEntry> {
    match &value {
        toml::Value::String(_) => {}
        toml::Value::Table(table)
            if table.is_empty()
                || table.contains_key("mode")
                || table.contains_key("exclude")
                || table.contains_key("include")
                || table.contains_key("manifest")
                || table.contains_key("autosave")
                || table.contains_key("encrypt")
                || table.contains_key("variants")
                || table.contains_key("enabled")
                || table.contains_key("remove_empty")
                || table.contains_key("dot_prefix")
                || ((table.contains_key("source")
                    || table.contains_key("content")
                    || table.contains_key("permissions")
                    || table.contains_key("relative"))
                    && !table.contains_key("block")
                    && !table.contains_key("line")
                    && !table.contains_key("template")
                    && !table.contains_key("comment")) => {}
        toml::Value::Table(_) => return None,
        _ => {
            warn!("[dotfiles].\"{target_raw}\": expected string or table entry, ignoring entry");
            return None;
        }
    }
    match value.try_into() {
        Ok(entry) => Some(entry),
        Err(err) => {
            warn!("[dotfiles].\"{target_raw}\": invalid file entry: {err}");
            None
        }
    }
}

/// Resolve one declaration, merging explicit tracking policies into earlier layers.
fn merge_file_entry(
    target_raw: String,
    entry: FileTomlEntry,
    base: &Path,
    origin: &ResourceOrigin,
    merged: &mut IndexMap<(PathBuf, bool), FileRequest>,
) {
    let (
        source,
        content,
        mode,
        exclude,
        include,
        manifest,
        permissions,
        autosave,
        encrypt,
        variants,
        enabled,
        remove_empty,
        dot_prefix,
        relative,
    ) = match entry {
        FileTomlEntry::Source(source) => (
            Some(source),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        ),
        FileTomlEntry::Table {
            source,
            content,
            mode,
            exclude,
            include,
            manifest,
            permissions,
            autosave,
            encrypt,
            variants,
            enabled,
            remove_empty,
            dot_prefix,
            relative,
        } => (
            source,
            content,
            mode,
            exclude,
            include,
            manifest,
            permissions,
            autosave,
            encrypt,
            variants,
            enabled,
            remove_empty,
            dot_prefix,
            relative,
        ),
    };
    // `{ permissions = "0600" }` alone manages only an existing target's
    // permissions; it never implies a source under dotfiles.root
    let permissions_only =
        permissions.is_some() && source.is_none() && content.is_none() && mode.is_none();
    let permissions = match permissions.as_deref().map(parse_permissions).transpose() {
        Ok(permissions) => permissions,
        Err(err) => {
            warn!("[dotfiles].\"{target_raw}\": {err}, ignoring entry");
            return;
        }
    };
    let remove_empty = remove_empty.unwrap_or(false);
    let dot_prefix = dot_prefix.unwrap_or(false);
    if encrypt == Some(true) && content.is_some() {
        record_invalid(
            &target_raw,
            &origin.config,
            "encrypted dotfiles require an external source; inline content is shared in configuration",
        );
        return;
    }
    let explicit = ExplicitFields {
        autosave: autosave.is_some(),
        encrypt: encrypt.is_some(),
        variants: variants.is_some(),
        enabled: enabled.is_some(),
        exclude: exclude.is_some(),
        include: include.is_some(),
    };
    let enabled = enabled.unwrap_or(true);
    let variants = variants.unwrap_or_default();
    let implied_variant_source = match validate_file_variants(
        &target_raw,
        source.as_deref(),
        content.as_deref(),
        mode.as_deref(),
        permissions_only,
        &variants,
    ) {
        Ok(Some(relative)) => Some(dotfiles_root().join(relative)),
        Ok(None) => None,
        Err(err) => {
            record_invalid(&target_raw, &origin.config, err.to_string());
            return;
        }
    };
    let selectors: Vec<_> = variants.iter().map(|v| v.selector.clone()).collect();
    let policy_for = |mode: FileMode| {
        let defaults = FilePolicy::for_mode(mode);
        FilePolicy {
            autosave: autosave.unwrap_or(defaults.autosave),
            encrypt: encrypt.unwrap_or(false),
            explicit,
        }
    };
    if mode.as_deref() != Some("track") && include.is_some() {
        record_invalid(
            &target_raw,
            &origin.config,
            "include selects what a tracked directory saves and applies only to mode = \"track\"",
        );
        return;
    }
    if mode.as_deref() == Some("track") {
        if source.is_some()
            || content.is_some()
            || manifest.is_some()
            || remove_empty
            || relative == Some(true)
            || dot_prefix
        {
            record_invalid(
                &target_raw,
                &origin.config,
                "mode = \"track\" leaves the file where it is and takes no source, content, manifest, remove_empty, relative, or dot_prefix",
            );
            return;
        }
        if permissions.is_some()
            && let Some(reason) = permissions_conflict(Some(FileMode::Track), false, false, false)
        {
            record_invalid(&target_raw, &origin.config, reason);
            return;
        }
        let target = resolve_target_arg(&target_raw);
        if target.is_relative() {
            record_invalid(
                &target_raw,
                &origin.config,
                "target must be absolute or start with ~/",
            );
            return;
        }
        // **An `include` list selects paths inside a tracked directory,
        // so a list on an entry that is a file can never select
        // anything.** `"~/.aws/credentials" = { include = ["credentials"]
        // }` is the natural mistake next to the documented directory
        // example, and it would otherwise be a declaration that captures
        // nothing at all. Said at the point it is written, with the fix,
        // rather than left to be worked out from an omission line. A
        // target that does not exist yet is not judged: it is captured
        // once it appears, and what it will be is not knowable here.
        if include.is_some() && std::fs::symlink_metadata(&target).is_ok_and(|meta| !meta.is_dir())
        {
            record_invalid(
                &target_raw,
                &origin.config,
                "include selects paths inside a tracked directory and does nothing on a file: remove it, or track the parent directory and name this file in its include list",
            );
            return;
        }
        let compiled = (
            compile_patterns("exclude", exclude),
            compile_patterns("include", include),
        );
        let (exclude, include) = match compiled {
            (Ok(exclude), Ok(include)) => (exclude.unwrap_or_default(), include),
            // both lists are reported when both are wrong: naming one and
            // dropping the other sends the user back for a second round
            // over a mistake mise had already seen
            (exclude, include) => {
                let reasons = [exclude.err(), include.err()]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    .join("; ");
                record_invalid(&target_raw, &origin.config, &reasons);
                return;
            }
        };
        let request = FileRequest {
            target_raw,
            target: target.clone(),
            source: PathBuf::new(),
            content: None,
            mode: FileMode::Track,
            exclude,
            include,
            manifest: None,
            permissions: None,
            base: base.to_path_buf(),
            origin: origin.clone(),
            policy: policy_for(FileMode::Track),
            variants: selectors,
            enabled,
            remove_empty: false,
            dot_prefix: false,
            relative: false,
        };
        // a later file of the same directory (`config.local.toml` after
        // `config.toml`) repeating a track declaration overrides only what
        // it says
        match merged.get_mut(&(target.clone(), true)) {
            Some(existing) if existing.mode == FileMode::Track => existing.override_from(request),
            _ => {
                merged.insert((target, true), request);
            }
        }
        return;
    }
    use crate::system::history::select::{self, Selection};
    let target_raw = match select::select(&selectors, &select::active_environments()) {
        Selection::Single => target_raw,
        Selection::Variant(selected) => variants
            .iter()
            .find(|v| v.selector == selected)
            .and_then(|v| v.target.clone())
            .unwrap_or(target_raw),
        Selection::NoMatch => return,
        Selection::Ambiguous(_) => {
            record_invalid(&target_raw, &origin.config, "ambiguous dotfile variants");
            return;
        }
    };
    if source.is_some() && content.is_some() {
        warn!(
            "[dotfiles].\"{target_raw}\": source and content are mutually exclusive, ignoring entry"
        );
        return;
    }
    if content.is_some() && (mode.is_some() || exclude.is_some() || manifest.is_some()) {
        warn!(
            "[dotfiles].\"{target_raw}\": inline content does not support mode, exclude, or manifest, ignoring entry"
        );
        return;
    }
    if mode.as_deref() == Some("absent") {
        if source.is_some()
            || exclude.is_some()
            || manifest.is_some()
            || permissions.is_some()
            || encrypt == Some(true)
        {
            warn!(
                "[dotfiles].\"{target_raw}\": mode = \"absent\" removes the target and takes no source, content, exclude, manifest, permissions, or encrypt, ignoring entry"
            );
            return;
        }
        if remove_empty || relative == Some(true) {
            warn!(
                "[dotfiles].\"{target_raw}\": mode = \"absent\" takes no remove_empty or relative, ignoring entry"
            );
            return;
        }
        if dot_prefix {
            warn!(
                "[dotfiles].\"{target_raw}\": dot_prefix requires mode copy or symlink-each, ignoring entry"
            );
            return;
        }
        let target = resolve_target_arg(&target_raw);
        if target.is_relative() {
            warn!(
                "[dotfiles].\"{target_raw}\": target must be absolute or start with ~/, ignoring entry"
            );
            return;
        }
        // a pattern would be checked as a literal path and remove nothing
        if is_glob_pattern(&target) {
            warn!(
                "[dotfiles].\"{target_raw}\": an absent target cannot use wildcards, ignoring entry"
            );
            return;
        }
        merged.insert(
            (target.clone(), false),
            FileRequest {
                target_raw,
                target,
                source: PathBuf::new(),
                content: None,
                mode: FileMode::Absent,
                exclude: vec![],
                include: None,
                manifest: None,
                permissions: None,
                base: base.to_path_buf(),
                origin: origin.clone(),
                policy: policy_for(FileMode::Absent),
                variants: vec![],
                enabled,
                remove_empty: false,
                dot_prefix: false,
                relative: false,
            },
        );
        return;
    }
    // compile once here so a typo is reported against the entry that wrote
    // it, not on every walk of the source.
    //
    // **A deployment entry keeps working with the rest of its list**, as
    // it always has: what an unreadable pattern costs here is a file
    // copied or linked that the user meant to leave behind, which they
    // can see. A tracked entry is refused instead (see the `track`
    // branch above), because what it costs there is a file captured into
    // history and pushed to a remote, which they cannot take back.
    let exclude = exclude
        .unwrap_or_default()
        .into_iter()
        .filter_map(|pattern| match glob::Pattern::new(&pattern) {
            Ok(pattern) => Some(pattern),
            Err(err) => {
                warn!("[dotfiles].\"{target_raw}\": invalid exclude pattern '{pattern}': {err}");
                None
            }
        })
        .collect::<Vec<_>>();
    let mode = match mode.as_deref() {
        None => default_mode(),
        Some(m) => match FileMode::parse(m) {
            Some(m) => m,
            None => {
                warn!("[dotfiles].\"{target_raw}\": unknown mode '{m}', ignoring entry");
                return;
            }
        },
    };
    let manifest = match manifest.as_deref() {
        None => None,
        Some(value) => match FileManifest::parse(value) {
            Some(manifest) => Some(manifest),
            None => {
                warn!("[dotfiles].\"{target_raw}\": unknown manifest '{value}', ignoring entry");
                return;
            }
        },
    };
    if manifest.is_some() && !matches!(mode, FileMode::Copy | FileMode::SymlinkEach) {
        warn!(
            "[dotfiles].\"{target_raw}\": manifest requires mode copy or symlink-each, ignoring entry"
        );
        return;
    }
    if permissions.is_some() {
        let declared = if permissions_only {
            None
        } else if content.is_some() {
            Some(FileMode::Content)
        } else {
            Some(mode)
        };
        if let Some(reason) = permissions_conflict(
            declared,
            !exclude.is_empty(),
            manifest.is_some(),
            encrypt.is_some(),
        ) {
            warn!("[dotfiles].\"{target_raw}\": {reason}, ignoring entry");
            return;
        }
    }
    #[cfg(not(unix))]
    let permissions = {
        if permissions.is_some() {
            warn_permissions_ignored();
            if permissions_only {
                return;
            }
        }
        None::<u32>
    };
    if remove_empty && (content.is_some() || permissions_only || mode != FileMode::Template) {
        warn!(
            "[dotfiles].\"{target_raw}\": remove_empty requires mode = \"template\", ignoring entry"
        );
        return;
    }
    if dot_prefix
        && (content.is_some()
            || permissions_only
            || !matches!(mode, FileMode::Copy | FileMode::SymlinkEach))
    {
        warn!(
            "[dotfiles].\"{target_raw}\": dot_prefix requires mode copy or symlink-each, ignoring entry"
        );
        return;
    }
    if relative == Some(true)
        && (content.is_some()
            || permissions_only
            || !matches!(mode, FileMode::Symlink | FileMode::SymlinkEach))
    {
        warn!(
            "[dotfiles].\"{target_raw}\": relative requires mode = \"symlink\" or \"symlink-each\", ignoring entry"
        );
        return;
    }
    let target = resolve_target_arg(&target_raw);
    if target.is_relative() {
        warn!(
            "[dotfiles].\"{target_raw}\": target must be absolute or start with ~/, ignoring entry"
        );
        return;
    }
    if permissions_only {
        if is_glob_pattern(&target) {
            warn!(
                "[dotfiles].\"{target_raw}\": a permissions-only target cannot use wildcards, ignoring entry"
            );
            return;
        }
        merged.insert(
            (target.clone(), false),
            FileRequest {
                target_raw,
                target,
                source: PathBuf::new(),
                content: None,
                mode: FileMode::Permissions,
                exclude: vec![],
                include: None,
                manifest: None,
                permissions,
                base: base.to_path_buf(),
                origin: origin.clone(),
                policy: policy_for(FileMode::Permissions),
                variants: vec![],
                enabled,
                remove_empty: false,
                dot_prefix: false,
                relative: false,
            },
        );
        return;
    }
    if let Some(content) = content {
        merged.insert(
            (target.clone(), false),
            FileRequest {
                target_raw,
                target,
                source: PathBuf::new(),
                content: Some(content),
                mode: FileMode::Content,
                exclude: vec![],
                include: None,
                manifest: None,
                permissions,
                base: base.to_path_buf(),
                origin: origin.clone(),
                policy: policy_for(FileMode::Content),
                variants: vec![],
                enabled,
                remove_empty: false,
                dot_prefix: false,
                relative: false,
            },
        );
        return;
    }
    let source = match source {
        Some(source) => {
            let source = file::replace_path(&source);
            if source.is_relative() {
                base.join(source)
            } else {
                source
            }
        }
        None => match implied_variant_source
            .map(Ok)
            .unwrap_or_else(|| implied_source(&target))
        {
            Ok(source) => source,
            Err(err) => {
                warn!("[dotfiles].\"{target_raw}\": {err}, ignoring entry");
                return;
            }
        },
    };
    let mut origin = origin.clone();
    origin.source = Some(source.clone());
    for req in expand_request(FileRequest {
        target_raw,
        target,
        source,
        content: None,
        mode,
        exclude,
        // `include` applies only to `mode = "track"`, which returned above
        include: None,
        manifest,
        permissions,
        base: base.to_path_buf(),
        origin,
        policy: policy_for(mode),
        variants: vec![],
        enabled,
        remove_empty,
        dot_prefix,
        relative: relative_symlinks(mode, relative),
    }) {
        merged.insert((req.target.clone(), false), req);
    }
}

/// Whether an entry in `mode` links by relative path: its own `relative` key
/// when set, else `dotfiles.relative_symlinks`. Only symlink modes link, and
/// never on Windows, where a directory link is a junction and has no
/// relative form.
pub(crate) fn relative_symlinks(mode: FileMode, declared: Option<bool>) -> bool {
    cfg!(unix)
        && matches!(mode, FileMode::Symlink | FileMode::SymlinkEach)
        && declared.unwrap_or_else(|| Settings::get().dotfiles.relative_symlinks)
}

/// Resolve the default deployment mode, warning and using symlinks for unsupported values.
pub(crate) fn default_mode() -> FileMode {
    let settings = Settings::get();
    let mode = settings.dotfiles.default_mode.as_str();
    match FileMode::parse(mode) {
        Some(
            mode
            @ (FileMode::Symlink | FileMode::SymlinkEach | FileMode::Copy | FileMode::Template),
        ) => mode,
        _ => {
            warn!("dotfiles.default_mode: unsupported mode '{mode}', using symlink");
            FileMode::Symlink
        }
    }
}

pub(crate) fn dotfiles_root() -> PathBuf {
    file::replace_path(&Settings::get().dotfiles.root)
}

pub(crate) fn implied_source(target: &Path) -> Result<PathBuf> {
    let home: &Path = &dirs::HOME;
    let rel = target.strip_prefix(home).map_err(|_| {
        eyre::eyre!(
            "source is required for targets outside $HOME: {}",
            target.display_user()
        )
    })?;
    if rel.as_os_str().is_empty() {
        bail!("source is required for the home directory itself");
    }
    Ok(dotfiles_root().join(rel))
}

pub(crate) fn source_is_implied(req: &FileRequest) -> bool {
    if !req.mode.has_source() {
        return false;
    }
    match implied_source(&req.target) {
        Ok(source) => source == req.source,
        Err(_) => false,
    }
}

pub(crate) fn resolve_target_arg(target: &str) -> PathBuf {
    lexical_normalize(&file::replace_path(target))
}

fn lexical_normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            component => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

pub(crate) fn matches_target(req_target: &Path, req_raw: &str, filters: &[String]) -> bool {
    filters.is_empty()
        || filters.iter().any(|filter| {
            filter == req_raw || {
                let resolved = resolve_target_arg(filter);
                resolved == req_target
            }
        })
}

pub(crate) fn copy_path(source: &Path, target: &Path) -> Result<()> {
    if let Some(parent) = target.parent() {
        file::create_dir_all(parent)?;
    }
    if source.is_dir() {
        if target.exists() || target.is_symlink() {
            remove_existing(target)?;
        }
        file::create_dir_all(target)?;
        file::copy_dir_all_preserve_symlinks(source, target)?;
    } else {
        if target.is_symlink() {
            file::remove_file(target)?;
        }
        file::copy(source, target)?;
    }
    Ok(())
}

fn expand_request(req: FileRequest) -> Vec<FileRequest> {
    let FileRequest {
        target_raw,
        target,
        source,
        mode,
        exclude,
        include,
        manifest,
        permissions,
        base,
        origin,
        policy,
        enabled,
        remove_empty,
        dot_prefix,
        relative,
        ..
    } = req;
    if !is_glob_pattern(&source) {
        return vec![FileRequest {
            target_raw,
            target,
            source,
            content: None,
            mode,
            exclude,
            include,
            manifest,
            permissions,
            base,
            origin,
            policy,
            variants: vec![],
            enabled,
            remove_empty,
            dot_prefix,
            relative,
        }];
    }

    let source_pattern = source.to_string_lossy().to_string();
    let matches = match glob::glob(&source_pattern) {
        Ok(paths) => paths
            .filter_map(|path| match path {
                Ok(path) => Some(path),
                Err(err) => {
                    warn!(
                        "[dotfiles].\"{target_raw}\": error reading source pattern {source_pattern}: {err}"
                    );
                    None
                }
            })
            .sorted()
            .collect_vec(),
        Err(err) => {
            warn!("[dotfiles].\"{target_raw}\": invalid source pattern: {err}");
            return vec![];
        }
    };
    if matches.is_empty() {
        warn!("[dotfiles].\"{target_raw}\": source pattern matched no files, ignoring entry");
        return vec![];
    }

    let target_pattern = target.to_string_lossy().to_string();
    if !is_glob_pattern(&target) {
        if matches.len() > 1 {
            warn!(
                "[dotfiles].\"{target_raw}\": source pattern matched multiple paths but target has no wildcard, ignoring entry"
            );
            return vec![];
        }
        return vec![FileRequest {
            target_raw,
            target,
            source: matches[0].clone(),
            content: None,
            mode,
            exclude,
            include,
            manifest,
            permissions,
            base,
            origin: ResourceOrigin {
                source: Some(matches[0].clone()),
                ..origin
            },
            policy,
            variants: vec![],
            enabled,
            remove_empty,
            dot_prefix,
            relative,
        }];
    }

    matches
        .into_iter()
        .filter_map(|matched_source| {
            let captures = match wildcard_captures(&source_pattern, &matched_source) {
                Ok(captures) => captures,
                Err(err) => {
                    warn!("[dotfiles].\"{target_raw}\": {err}");
                    return None;
                }
            };
            let Some(target_path) = expand_target_pattern(&target_pattern, &captures) else {
                warn!(
                    "[dotfiles].\"{target_raw}\": target wildcard count does not match source pattern, ignoring {}",
                    matched_source.display_user()
                );
                return None;
            };
            Some(FileRequest {
                target_raw: target_path.display_user().to_string(),
                target: target_path,
                source: matched_source.clone(),
                content: None,
                mode,
                exclude: exclude.clone(),
                include: None,
                manifest,
                permissions,
                base: base.clone(),
                origin: ResourceOrigin {
                    source: Some(matched_source.clone()),
                    ..origin.clone()
                },
                policy,
                variants: vec![],
                enabled,
                remove_empty,
                dot_prefix,
                relative,
            })
        })
        .collect()
}

fn is_glob_pattern(path: &Path) -> bool {
    path.to_string_lossy()
        .chars()
        .any(|c| matches!(c, '*' | '?' | '['))
}

fn wildcard_captures(pattern: &str, path: &Path) -> Result<Vec<String>> {
    let path = normalize_path_separators(&path.to_string_lossy());
    let re = wildcard_regex(pattern)?;
    let Some(captures) = re.captures(&path) else {
        bail!("source pattern did not match {path}");
    };
    Ok((1..captures.len())
        .map(|i| {
            captures
                .get(i)
                .map(|m| m.as_str().to_string())
                .unwrap_or_default()
        })
        .collect())
}

fn wildcard_regex(pattern: &str) -> Result<Regex> {
    let mut re = String::from("^");
    let pattern = normalize_path_separators(pattern);
    let mut chars = pattern.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '*' if chars.peek() == Some(&'*') => {
                chars.next();
                if chars.peek() == Some(&'/') {
                    chars.next();
                    // Glob `**/` can match zero directories. Capture the
                    // directory body without the trailing slash so target
                    // expansion can omit the slash when the capture is empty.
                    re.push_str("(?:(.*)/)?");
                } else {
                    re.push_str("(.*)");
                }
            }
            '*' => re.push_str("([^/]*)"),
            '?' => re.push_str("([^/])"),
            '[' => {
                let Some(class) = read_glob_class(&mut chars) else {
                    re.push_str("\\[");
                    continue;
                };
                re.push('(');
                re.push_str(&class);
                re.push(')');
            }
            _ => re.push_str(&regex::escape(&ch.to_string())),
        }
    }
    re.push('$');
    Ok(Regex::new(&re)?)
}

fn expand_target_pattern(pattern: &str, captures: &[String]) -> Option<PathBuf> {
    let mut out = String::new();
    let pattern = normalize_path_separators(pattern);
    let mut chars = pattern.chars().peekable();
    let mut captures = captures.iter();
    while let Some(ch) = chars.next() {
        match ch {
            '*' if chars.peek() == Some(&'*') => {
                chars.next();
                let capture = normalize_path_separators(captures.next()?);
                if chars.peek() == Some(&'/') {
                    chars.next();
                    if !capture.is_empty() {
                        out.push_str(&capture);
                        out.push('/');
                    }
                } else {
                    out.push_str(&capture);
                }
            }
            '*' => out.push_str(&normalize_path_separators(captures.next()?)),
            '?' => out.push_str(&normalize_path_separators(captures.next()?)),
            '[' => {
                read_glob_class(&mut chars)?;
                out.push_str(&normalize_path_separators(captures.next()?));
            }
            _ => out.push(ch),
        }
    }
    if captures.next().is_some() {
        return None;
    }
    Some(PathBuf::from(native_path_separators(&out)))
}

fn normalize_path_separators(path: &str) -> String {
    path.replace('\\', "/")
}

fn native_path_separators(path: &str) -> String {
    if std::path::MAIN_SEPARATOR == '/' {
        path.to_string()
    } else {
        path.replace('/', std::path::MAIN_SEPARATOR_STR)
    }
}

fn read_glob_class<I>(chars: &mut std::iter::Peekable<I>) -> Option<String>
where
    I: Iterator<Item = char>,
{
    let mut class = String::from("[");
    if chars.peek() == Some(&'!') {
        chars.next();
        class.push('^');
    }
    for ch in chars.by_ref() {
        class.push(ch);
        if ch == ']' {
            return Some(class);
        }
    }
    None
}

/// Current state of one entry on this machine.
///
/// Note: computing a template entry's state requires rendering it, so this
/// runs the template engine — including `exec()` — from
/// `mise dot status`. That's the same trust model as `[env]`
/// templates (which run on
/// every command in a trusted config); only `--dry-run` promises to execute
/// nothing and therefore skips template checks entirely.
pub(crate) fn check(
    config: &Config,
    req: &FileRequest,
    secrets: &SecretValues,
) -> Result<FileState> {
    if req.mode == FileMode::Track {
        return Ok(FileState::Tracked);
    }
    if req.mode.has_source() && !req.source.exists() {
        return Ok(FileState::SourceMissing);
    }
    // render at most once per call — templates may use exec()
    let rendered = match req.mode {
        FileMode::Template => Some(render_template(config, req, secrets)?),
        _ => None,
    };
    check_rendered(req, rendered.as_deref())
}

/// [`check`] with template output already rendered, so callers that go on to
/// write the file render only once (templates may use `exec()`, which must
/// not run more often than necessary)
fn check_rendered(req: &FileRequest, rendered: Option<&str>) -> Result<FileState> {
    match req.mode {
        FileMode::Track => Ok(FileState::Tracked),
        FileMode::Absent => check_absent(&req.target),
        FileMode::Symlink => check_symlink(&req.source, &req.target, req.relative),
        FileMode::SymlinkEach => check_symlink_each(req),
        FileMode::Copy if req.source.is_dir() => {
            if req.permissions.is_some() {
                bail!("permissions requires a file source, not a directory");
            }
            check_copy_dir(req)
        }
        FileMode::Copy => {
            let expected = file::read(&req.source)?;
            check_permissions(req, check_content(&req.target, &expected))
        }
        FileMode::Content => check_permissions(
            req,
            check_content(
                &req.target,
                req.content.as_deref().expect("inline content").as_bytes(),
            ),
        ),
        FileMode::Template if removes_target(req, rendered) => {
            Ok(match empty_render_target(req)? {
                EmptyRenderTarget::Absent => FileState::Applied,
                EmptyRenderTarget::Owned => {
                    FileState::Differs("template renders empty, target will be removed".into())
                }
                EmptyRenderTarget::Conflict(reason) => FileState::Differs(format!(
                    "template renders empty, but {reason}; use --force to remove it"
                )),
            })
        }
        FileMode::Template => check_permissions(
            req,
            check_content(
                &req.target,
                rendered.expect("rendered template content").as_bytes(),
            ),
        ),
        FileMode::Permissions => check_permissions_only(req),
    }
}

/// The permission bits apply must leave on a written target, when the entry
/// promises any: an explicit `permissions`, or a template's source mode.
/// Copies and inline content without `permissions` keep their historical
/// behaviour and are not checked for permission drift.
#[cfg(unix)]
fn desired_permissions(req: &FileRequest) -> Result<Option<u32>> {
    if req.permissions.is_some() {
        return Ok(req.permissions);
    }
    if req.mode == FileMode::Template {
        return Ok(Some(permission_bits(&req.source.metadata()?)));
    }
    Ok(None)
}

#[cfg(unix)]
fn permission_bits(metadata: &std::fs::Metadata) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o7777
}

/// Combine a content check with the target's permissions: an otherwise
/// applied target whose permissions drifted (e.g. a later chmod) is
/// `Differs`, so apply repairs them too. The mode is read without opening the
/// file, so a declared mode that denies its owner read access (`0200`,
/// `0000`) is still compared; such a target is applied once its mode matches,
/// because its content cannot be read back.
fn check_permissions(req: &FileRequest, content: Result<FileState>) -> Result<FileState> {
    #[cfg(unix)]
    if let Some(desired) = desired_permissions(req)? {
        let mode_differs = match std::fs::symlink_metadata(&req.target) {
            Ok(metadata) if metadata.file_type().is_file() => permission_bits(&metadata) != desired,
            _ => false,
        };
        let permissions_differ = || FileState::Differs("permissions differ".into());
        return match content {
            Ok(FileState::Applied) if mode_differs => Ok(permissions_differ()),
            // an unreadable target either drifted to a mode that denies
            // its owner read access (apply rewrites it) or was declared so
            Err(err) if (mode_differs || desired & 0o400 == 0) && is_permission_denied(&err) => {
                Ok(if mode_differs {
                    permissions_differ()
                } else {
                    FileState::Applied
                })
            }
            other => other,
        };
    }
    #[cfg(not(unix))]
    let _ = req;
    content
}

fn is_permission_denied(err: &eyre::Report) -> bool {
    err.chain().any(|cause| {
        cause
            .downcast_ref::<std::io::Error>()
            .is_some_and(|err| err.kind() == std::io::ErrorKind::PermissionDenied)
    })
}

/// A permissions-only entry never creates or rewrites its target and never
/// follows a symlink there: it only compares the bits of what exists. A
/// target that does not exist has nothing to adjust, so it counts as
/// satisfied (see [`permissions_target_absent`]).
fn check_permissions_only(req: &FileRequest) -> Result<FileState> {
    let metadata = match std::fs::symlink_metadata(&req.target) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(FileState::Applied),
        Err(err) => return Err(err.into()),
    };
    if metadata.file_type().is_symlink() {
        return Ok(FileState::Differs(PERMISSIONS_THROUGH_LINK.into()));
    }
    #[cfg(unix)]
    if let Some(desired) = req.permissions
        && permission_bits(&metadata) != desired
    {
        return Ok(FileState::Differs("permissions differ".into()));
    }
    Ok(FileState::Applied)
}

/// Why an applied permissions-only entry changed nothing: its target does not
/// exist. Status shows this next to `applied`.
pub(crate) fn permissions_target_absent(req: &FileRequest) -> Option<&'static str> {
    (req.mode == FileMode::Permissions
        && std::fs::symlink_metadata(&req.target)
            .is_err_and(|err| err.kind() == std::io::ErrorKind::NotFound))
    .then_some("target absent; permissions not applied")
}

const PERMISSIONS_THROUGH_LINK: &str =
    "exists but is a symlink; permissions are not set through links";

/// Why a permissions-only entry cannot act on its target right now, if it
/// cannot: the target is missing, or it is a symlink mise must not follow.
fn permissions_target_unavailable(req: &FileRequest) -> Result<Option<String>> {
    match std::fs::symlink_metadata(&req.target) {
        Ok(metadata) if metadata.file_type().is_symlink() => Ok(Some(format!(
            "{} is a symlink, which is never followed",
            req.target.display_user()
        ))),
        Ok(_) => Ok(None),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Some(format!(
            "{} does not exist",
            req.target.display_user()
        ))),
        Err(err) => Err(err.into()),
    }
}

/// An absent target is converged once nothing is there. A regular file or a
/// symlink (to anything, even a directory) is removed without comparing its
/// content: the declaration itself says it must not exist. A real directory
/// is an error rather than a `--force`-able conflict, because absent entries
/// never delete recursively.
fn check_absent(target: &Path) -> Result<FileState> {
    // links first: a Windows directory symlink or junction is a directory
    // carrying a reparse point, and is removed as a link, not refused
    if file::is_symlink_or_junction(target) {
        return Ok(FileState::Differs("present (symlink)".into()));
    }
    match std::fs::symlink_metadata(target) {
        // a parent that is a file means the target cannot exist either
        Err(err)
            if matches!(
                err.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
            ) =>
        {
            Ok(FileState::Applied)
        }
        Err(err) => Err(err.into()),
        Ok(meta) if meta.is_dir() => bail!(
            "{} is a directory; mode = \"absent\" only removes files and symlinks",
            target.display_user()
        ),
        Ok(meta) if meta.is_file() => Ok(FileState::Differs("present".into())),
        // a FIFO, socket, or device node is not a configuration file
        Ok(_) => bail!(
            "{} is not a regular file or symlink; mode = \"absent\" only removes files and symlinks",
            target.display_user()
        ),
    }
}

/// With `relative`, a link that reaches the source by an absolute path is not
/// applied: it is re-pointed, so turning the option on converts links an
/// earlier apply made. Without it, any link reaching the source is accepted,
/// relative or not, as it always was.
fn check_symlink(source: &Path, target: &Path, relative: bool) -> Result<FileState> {
    // On Windows a file link is a real symlink when the privilege was available and a copy
    // otherwise (see `link_path`), so which one is on disk decides how to read it. Only fall
    // through to the copy comparison when it is not a symlink.
    if cfg!(windows) && source.is_file() && !target.is_symlink() {
        return check_copy(source, target);
    }
    if target.is_symlink() {
        let dest = std::fs::read_link(target)?;
        if !link_points_to(source, target) {
            Ok(FileState::Differs(format!(
                "symlink points to {}",
                dest.display_user()
            )))
        } else if relative && dest.is_absolute() {
            Ok(FileState::Differs(format!(
                "symlink points to {} by an absolute path; relative requested",
                dest.display_user()
            )))
        } else {
            Ok(FileState::Applied)
        }
    } else if target.exists() {
        Ok(FileState::Differs("exists but is not a symlink".into()))
    } else {
        Ok(FileState::Missing)
    }
}

fn points_at_same_file(target: &Path, source: &Path) -> bool {
    match (target.canonicalize(), source.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

fn check_symlink_each(req: &FileRequest) -> Result<FileState> {
    if !req.source.is_dir() {
        // callers add the entry's context (status row / apply error list)
        bail!(
            "mode symlink-each requires the source to be a directory: {}",
            req.source.display_user()
        );
    }
    let stale = stale_links(req)?;
    let stale_reason = || format!("{} stale link(s)", stale.len());
    let files = walk_source_files(req)?;
    // with no files to link the desired state is just the target directory —
    // a blocking non-directory must still surface (and be --force-able)
    if files.is_empty() {
        return if req.target.is_dir() {
            if stale.is_empty() {
                Ok(FileState::Applied)
            } else {
                Ok(FileState::Differs(stale_reason()))
            }
        } else if req.target.exists() || req.target.is_symlink() {
            Ok(FileState::Differs("exists but is not a directory".into()))
        } else {
            Ok(FileState::Missing)
        };
    }
    let mut applied = 0;
    let mut missing = 0;
    let mut differs: Option<String> = None;
    for (source, target) in files {
        match check_symlink(&source, &target, req.relative)? {
            FileState::Applied => applied += 1,
            FileState::Missing => missing += 1,
            FileState::Differs(reason) => {
                differs.get_or_insert(format!("{}: {reason}", target.display_user()));
            }
            FileState::SourceMissing | FileState::Tracked => {
                unreachable!("walked from source")
            }
        }
    }
    if let Some(reason) = differs {
        Ok(FileState::Differs(reason))
    } else if missing > 0 && applied > 0 {
        Ok(FileState::Differs(format!(
            "{applied} file(s) linked, {missing} missing"
        )))
    } else if missing > 0 {
        Ok(FileState::Missing)
    } else if !stale.is_empty() {
        Ok(FileState::Differs(stale_reason()))
    } else {
        Ok(FileState::Applied)
    }
}

fn check_copy(source: &Path, target: &Path) -> Result<FileState> {
    check_content(target, &file::read(source)?)
}

/// walks through [`walk_source_files`] rather than the source tree directly
/// so `exclude` applies to a directory copy the same way it does to
/// symlink-each
fn check_copy_dir(req: &FileRequest) -> Result<FileState> {
    if !req.target.exists() {
        return Ok(FileState::Missing);
    }
    if !req.target.is_dir() {
        return Ok(FileState::Differs("exists but is not a directory".into()));
    }
    for (source, target) in walk_source_files(req)? {
        match check_content(&target, &file::read(&source)?)? {
            FileState::Applied => {}
            _ => {
                let rel = target.strip_prefix(&req.target).unwrap_or(&target);
                return Ok(FileState::Differs(format!("{} differs", rel.display())));
            }
        }
    }
    Ok(FileState::Applied)
}

fn check_content(target: &Path, expected: &[u8]) -> Result<FileState> {
    // copy/template targets must be real files; a symlink — dangling or
    // live — gets replaced (re-pointing/replacing symlinks needs no --force)
    if target.is_symlink() {
        return Ok(FileState::Differs("exists but is a symlink".into()));
    }
    if !target.exists() {
        return Ok(FileState::Missing);
    }
    if target.is_dir() {
        return Ok(FileState::Differs("exists but is a directory".into()));
    }
    if file::read(target)? == expected {
        Ok(FileState::Applied)
    } else {
        Ok(FileState::Differs("content differs".into()))
    }
}

pub(crate) fn render_template(
    config: &Config,
    req: &FileRequest,
    secrets: &SecretValues,
) -> Result<String> {
    let raw = file::read_to_string(&req.source)?;
    let rendered = secrets
        .render_dotfile(config, &raw, &req.base, &req.origin.config)
        .map_err(|err| {
            eyre::eyre!(
                "[dotfiles].\"{}\": failed to render template {}: {err}",
                req.target_raw,
                req.source.display_user()
            )
        })?;
    Ok(rendered)
}

pub(crate) fn render_template_for_oci(config: &Config, req: &FileRequest) -> Result<String> {
    let raw = file::read_to_string(&req.source)?;
    let rendered =
        SecretValues::render_dotfile_for_oci(config, &raw, &req.base, &req.origin.config).map_err(
            |err| {
                eyre::eyre!(
                    "[dotfiles].\"{}\": failed to render template {}: {err}",
                    req.target_raw,
                    req.source.display_user()
                )
            },
        )?;
    Ok(rendered)
}

/// Render every configured dotfile template before a full bootstrap can
/// mutate anything. Secret values are cached, but templates are rendered again
/// when applied so hooks can update dynamic inputs such as files or commands.
pub(crate) fn preflight_templates(
    config: &Config,
    requests: &[FileRequest],
    secrets: &SecretValues,
) -> Result<()> {
    validate_composed_file_footprints(requests)?;
    let broken = requests
        .iter()
        .filter(|req| req.mode == FileMode::Template)
        .filter_map(|req| render_template(config, req, secrets).err())
        .map(|err| format!("  {err}"))
        .collect::<Vec<_>>();
    if !broken.is_empty() {
        bail!("files: entries with errors:\n{}", broken.join("\n"));
    }
    Ok(())
}

/// directories a symlink-each entry needs: the target itself plus every
/// intermediate directory for nested source files
fn needed_dirs(req: &FileRequest) -> Result<Vec<PathBuf>> {
    let mut out = indexmap::IndexSet::new();
    out.insert(req.target.clone());
    for (_, target) in walk_source_files(req)? {
        let mut dir = target.parent();
        while let Some(d) = dir {
            if d == req.target {
                break;
            }
            out.insert(d.to_path_buf());
            dir = d.parent();
        }
    }
    Ok(out.into_iter().collect())
}

fn symlink_each_state_path(req: &FileRequest) -> PathBuf {
    let source = lexical_normalize(&req.source);
    let target = lexical_normalize(&req.target);
    dirs::STATE.join("dotfiles").join(format!(
        "{}.toml",
        hash_to_str(&(source.as_path(), target.as_path()))
    ))
}

fn desired_symlink_each_state(req: &FileRequest) -> Result<SymlinkEachState> {
    Ok(SymlinkEachState {
        version: SYMLINK_EACH_STATE_VERSION,
        source: lexical_normalize(&req.source),
        target: lexical_normalize(&req.target),
        links: walk_source_files(req)?
            .into_iter()
            .map(|(source, target)| ManagedLink {
                source: lexical_normalize(&source),
                target: lexical_normalize(&target),
            })
            .collect(),
    })
}

fn active_symlink_each_state(req: &FileRequest) -> Result<SymlinkEachState> {
    if req.source.is_dir() {
        desired_symlink_each_state(req)
    } else {
        Ok(SymlinkEachState {
            version: SYMLINK_EACH_STATE_VERSION,
            source: lexical_normalize(&req.source),
            target: lexical_normalize(&req.target),
            links: vec![],
        })
    }
}

fn load_symlink_each_state(req: &FileRequest) -> LoadedSymlinkEachState {
    let path = symlink_each_state_path(req);
    if !path.exists() {
        return LoadedSymlinkEachState::Missing;
    }
    let state = match read_symlink_each_state(&path) {
        Ok(state) => state,
        Err(err) => {
            warn!(
                "files: failed to read dotfiles state {}: {err}",
                path.display_user()
            );
            return LoadedSymlinkEachState::Invalid;
        }
    };
    let valid = state.version == SYMLINK_EACH_STATE_VERSION
        && state.source == lexical_normalize(&req.source)
        && state.target == lexical_normalize(&req.target)
        && state
            .links
            .iter()
            .all(|link| link.target != req.target && link.target.starts_with(&req.target));
    if valid {
        LoadedSymlinkEachState::Present(state)
    } else {
        warn!(
            "files: ignoring invalid dotfiles state {}",
            path.display_user()
        );
        LoadedSymlinkEachState::Invalid
    }
}

fn read_symlink_each_state(path: &Path) -> Result<SymlinkEachState> {
    file::read_to_string(path)
        .and_then(|contents| toml::from_str::<SymlinkEachState>(&contents).map_err(Into::into))
        .map(normalize_symlink_each_state)
}

fn normalize_symlink_each_state(mut state: SymlinkEachState) -> SymlinkEachState {
    state.source = lexical_normalize(&state.source);
    state.target = lexical_normalize(&state.target);
    for link in &mut state.links {
        link.source = lexical_normalize(&link.source);
        link.target = lexical_normalize(&link.target);
    }
    state
}

fn valid_symlink_each_state(state: &SymlinkEachState) -> bool {
    state.version == SYMLINK_EACH_STATE_VERSION
        && state
            .links
            .iter()
            .all(|link| link.target != state.target && link.target.starts_with(&state.target))
}

fn plan_symlink_each_reconciliation(
    active_requests: &[FileRequest],
    selected_requests: &[FileRequest],
) -> Result<SymlinkEachReconciliation> {
    let selected_targets = selected_requests
        .iter()
        .map(|req| lexical_normalize(&req.target))
        .collect::<std::collections::HashSet<_>>();
    let active = active_requests
        .iter()
        .filter(|req| {
            req.mode == FileMode::SymlinkEach
                && selected_targets.contains(&lexical_normalize(&req.target))
        })
        .map(active_symlink_each_state)
        .collect::<Result<Vec<_>>>()?;
    let state_dir = dirs::STATE.join("dotfiles");
    let stored = if state_dir.is_dir() {
        state_dir
            .read_dir()?
            .filter_map(|entry| entry.ok())
            .filter_map(|entry| read_symlink_each_state(&entry.path()).ok())
            .collect()
    } else {
        vec![]
    };
    Ok(reconcile_symlink_each_states(
        &active,
        &selected_targets,
        &stored,
    ))
}

fn reconcile_symlink_each_states(
    active: &[SymlinkEachState],
    selected_targets: &std::collections::HashSet<PathBuf>,
    stored: &[SymlinkEachState],
) -> SymlinkEachReconciliation {
    let active_keys = active
        .iter()
        .map(|state| (&state.source, &state.target))
        .collect::<std::collections::HashSet<_>>();
    let desired_links = active
        .iter()
        .flat_map(|state| state.links.iter())
        .map(|link| (&link.target, &link.source))
        .collect::<std::collections::HashMap<_, _>>();
    if selected_targets.is_empty() {
        return SymlinkEachReconciliation {
            stale_links: vec![],
            targets: vec![],
        };
    }
    let mut stale_links = IndexMap::<PathBuf, ManagedLink>::new();
    let mut targets = indexmap::IndexSet::new();
    for state in stored {
        if !valid_symlink_each_state(state)
            || !selected_targets.contains(&state.target)
            || active_keys.contains(&(&state.source, &state.target))
        {
            continue;
        }
        let stale_before = stale_links.len();
        for link in &state.links {
            if desired_links
                .get(&link.target)
                .is_none_or(|source| **source != link.source)
                && link_points_to(&link.source, &link.target)
            {
                stale_links.insert(link.target.clone(), link.clone());
            }
        }
        if stale_links.len() != stale_before {
            targets.insert(state.target.clone());
        }
    }
    SymlinkEachReconciliation {
        stale_links: stale_links.into_values().collect(),
        targets: targets.into_iter().collect(),
    }
}

fn save_symlink_each_state(req: &FileRequest) {
    if cfg!(windows) {
        return;
    }
    let path = symlink_each_state_path(req);
    let result = (|| -> Result<()> {
        file::create_dir_all(path.parent().expect("dotfiles state parent"))?;
        file::write(
            &path,
            toml::to_string_pretty(&desired_symlink_each_state(req)?)?,
        )
    })();
    if let Err(err) = result {
        warn!(
            "files: failed to write dotfiles state {}: {err}",
            path.display_user()
        );
    }
}

fn remove_symlink_each_state(req: &FileRequest) -> Result<()> {
    let path = symlink_each_state_path(req);
    if path.exists() {
        file::remove_file(path)?;
    }
    Ok(())
}

fn target_state_path(req: &FileRequest) -> PathBuf {
    let target = lexical_normalize(&req.target);
    dirs::STATE
        .join("dotfiles")
        .join("targets")
        .join(format!("{}.toml", hash_to_str(&target.as_path())))
}

/// The record for `req`'s target. A missing, unreadable, or foreign record
/// proves nothing, so it reads as no record at all.
fn load_target_state(req: &FileRequest) -> Option<TargetState> {
    let path = target_state_path(req);
    if !path.exists() {
        return None;
    }
    let state = match file::read_to_string(&path)
        .and_then(|contents| toml::from_str::<TargetState>(&contents).map_err(Into::into))
    {
        Ok(state) => state,
        Err(err) => {
            warn!(
                "files: failed to read dotfiles state {}: {err}",
                path.display_user()
            );
            return None;
        }
    };
    if state.version == TARGET_STATE_VERSION
        && lexical_normalize(&state.target) == lexical_normalize(&req.target)
    {
        Some(state)
    } else {
        warn!(
            "files: ignoring invalid dotfiles state {}",
            path.display_user()
        );
        None
    }
}

/// Record `content` as what mise last wrote to `req`'s target. A record that
/// cannot be written only costs a later `--force`, so it warns.
fn save_target_state(req: &FileRequest, content: &str) {
    update_target_state(req, |state| {
        state.content_digest = Some(content_digest(content));
    });
}

/// Add `created` to the directories recorded as mise's for `req`'s target,
/// keeping those recorded by earlier applies. A record that cannot be written
/// only leaves the directories in place later, so it warns.
fn record_created_dirs(req: &FileRequest, created: &[PathBuf]) {
    if created.is_empty() {
        return;
    }
    update_target_state(req, |state| {
        for dir in created {
            if !state.created_dirs.contains(dir) {
                state.created_dirs.push(dir.clone());
            }
        }
    });
}

fn update_target_state(req: &FileRequest, update: impl FnOnce(&mut TargetState)) {
    let path = target_state_path(req);
    let mut state = load_target_state(req).unwrap_or_default();
    state.version = TARGET_STATE_VERSION;
    state.target = lexical_normalize(&req.target);
    update(&mut state);
    let result = (|| -> Result<()> {
        file::create_dir_all(path.parent().expect("dotfiles state parent"))?;
        file::write_atomic(&path, toml::to_string_pretty(&state)?)
    })();
    if let Err(err) = result {
        warn!(
            "files: failed to write dotfiles state {}: {err}",
            path.display_user()
        );
    }
}

fn remove_target_state(req: &FileRequest) -> Result<()> {
    let path = target_state_path(req);
    if path.exists() {
        file::remove_file(path)?;
    }
    Ok(())
}

/// Whether mise records the directories it creates for `req`: entries whose
/// target is a single file or link. Entries that walk a source directory
/// share their target with unmanaged files, permissions-only entries never
/// create or remove theirs, and absent entries create nothing (they prune
/// from a record an earlier entry for the same target left; see
/// [`removal_created_dirs`]).
fn records_created_dirs(req: &FileRequest) -> bool {
    match req.mode {
        FileMode::Symlink | FileMode::Template | FileMode::Content => true,
        FileMode::Copy => !req.source.is_dir(),
        FileMode::SymlinkEach | FileMode::Track | FileMode::Permissions | FileMode::Absent => false,
    }
}

/// The directories removing `target` may take with it: the `created` ones in
/// an unbroken run upward from its parent, deepest first. A directory that is
/// already gone (an earlier removal took it) holds nothing, so the run passes
/// over it. Only directories strictly inside `home` qualify: `home` itself,
/// everything above it, and everything outside it (such as `/opt/app` for a
/// target `/opt/app/file`) may be shared with other software, so they stay
/// even when mise created them.
fn created_dirs_to_prune(target: &Path, created: &[PathBuf], home: &Path) -> Vec<PathBuf> {
    let mut chain = vec![];
    for dir in target.ancestors().skip(1) {
        if dir == home || !dir.starts_with(home) {
            break;
        }
        if created.iter().any(|c| c == dir) {
            chain.push(dir.to_path_buf());
        } else if dir.exists() || dir.is_symlink() {
            break;
        }
    }
    chain
}

/// Remove the empty directories of `chain` (deepest first), stopping at the
/// first one that holds anything, is not a directory, or is `claimed` by
/// another entry. One already gone, taken by an earlier walk, is passed
/// over. The chain is checked against `home` by path only, so a symlinked
/// ancestor (`~/.config -> /opt/config`) can put a directory physically
/// outside home: the walk stops there and gives the directory up, since
/// mise will never remove it. Returns what was removed or given up, which
/// the records drop.
fn remove_created_dirs(chain: &[PathBuf], claimed: &HashSet<PathBuf>, home: &Path) -> Vec<PathBuf> {
    use std::io::ErrorKind;
    // Best effort: the targets are already gone, so a directory that cannot
    // be removed only stays behind. It stays in the record too, so a later
    // removal retries it.
    let give_up = |dir: &Path, err: std::io::Error| {
        if matches!(
            err.kind(),
            ErrorKind::NotFound | ErrorKind::DirectoryNotEmpty
        ) {
            // something else removed it or wrote into it meanwhile
            debug!("files: keeping {}: {err}", dir.display_user());
        } else {
            warn!(
                "files: cannot remove empty directory {}: {err}",
                dir.display_user()
            );
        }
    };
    let physical_home = std::fs::canonicalize(home).unwrap_or_else(|_| home.to_path_buf());
    let mut settled = vec![];
    for dir in chain {
        match std::fs::symlink_metadata(dir) {
            Err(err) if err.kind() == ErrorKind::NotFound => continue,
            Err(err) => {
                give_up(dir, err);
                break;
            }
            Ok(metadata) if !metadata.is_dir() => break,
            Ok(_) => {}
        }
        let physical = match std::fs::canonicalize(dir) {
            Ok(physical) => physical,
            Err(err) => {
                give_up(dir, err);
                break;
            }
        };
        if physical == physical_home || !physical.starts_with(&physical_home) {
            debug!(
                "files: keeping {}: it is {}, outside the home directory",
                dir.display_user(),
                physical.display()
            );
            settled.push(dir.clone());
            break;
        }
        // Check and remove the resolved path that passed the home check, so
        // an ancestor swapped for a symlink afterwards cannot redirect them.
        // The journal and records keep the path as the user sees it.
        // `remove_dir` only removes an empty directory, so a remaining race
        // can at worst remove an empty directory that is inside home.
        if claimed.contains(dir) {
            break;
        }
        match physical.read_dir() {
            Ok(mut entries) => {
                if entries.next().is_some() {
                    break;
                }
            }
            Err(err) => {
                give_up(dir, err);
                break;
            }
        }
        debug!("files: removing empty directory {}", dir.display_user());
        if let Err(err) = std::fs::remove_dir(&physical) {
            give_up(dir, err);
            break;
        }
        settled.push(dir.clone());
    }
    settled
}

/// The directories recorded as created for `req`'s target, if its kind
/// records them.
fn recorded_created_dirs(req: &FileRequest) -> Vec<PathBuf> {
    if !records_created_dirs(req) {
        return vec![];
    }
    load_target_state(req)
        .map(|state| state.created_dirs)
        .unwrap_or_default()
}

/// The recorded directories an apply that removes `req`'s target may prune.
/// Records are keyed by target, so an absent entry finds the one a copy,
/// symlink, template, or content entry left for the same path.
fn removal_created_dirs(req: &FileRequest) -> Vec<PathBuf> {
    if req.mode == FileMode::Absent {
        load_target_state(req)
            .map(|state| state.created_dirs)
            .unwrap_or_default()
    } else {
        recorded_created_dirs(req)
    }
}

/// Whether the record for `req`'s already-removed target still lists a
/// directory mise created that is there to prune.
fn has_leftover_created_dirs(req: &FileRequest) -> bool {
    created_dirs_to_prune(&req.target, &removal_created_dirs(req), &dirs::HOME)
        .iter()
        .any(|dir| dir.is_dir())
}

/// The pruning pass for a run: the targets it removed, and the converged
/// ones whose leftover directories it retries, each with its record.
fn prune_after_apply<'a>(removed: impl IntoIterator<Item = &'a FileRequest>, plan: &ApplyPlan<'a>) {
    let removals = removed
        .into_iter()
        .chain(plan.prune_leftovers.iter().copied())
        .map(|req| {
            if plan.prune_leftovers.iter().any(|r| std::ptr::eq(*r, req)) {
                debug!(
                    "files: retrying the directories created for {}",
                    req.target.display_user()
                );
            }
            (req, removal_created_dirs(req))
        })
        .collect::<Vec<_>>();
    prune_created_dirs(&removals, &plan.claimed_dirs, &dirs::HOME, true);
}

/// Whether this apply of `req` removes its target.
fn apply_removes_target(req: &FileRequest, rendered: Option<&str>) -> bool {
    req.mode == FileMode::Absent || removes_target(req, rendered)
}

/// Remove the directories mise created for targets it has just removed,
/// each paired with its recorded directories. This runs once every target
/// of the batch is gone, and each walk counts the directories recorded by
/// any of them: only the first entry written into a new directory records
/// it, so a directory shared by sibling targets still goes, whatever order
/// they were removed in. Each walk is journaled on its own. With
/// `update_records`, the removed directories are dropped from the records
/// that held them, which stay for the ownership evidence they hold.
///
/// This never fails: the targets are already removed, and an error here
/// would skip the steps after it while the next apply saw the targets as
/// converged. A directory that cannot be removed is warned about and stays
/// in its record, so a later removal retries it.
fn prune_created_dirs(
    removals: &[(&FileRequest, Vec<PathBuf>)],
    claimed: &HashSet<PathBuf>,
    home: &Path,
    update_records: bool,
) {
    let created = removals
        .iter()
        .flat_map(|(_, dirs)| dirs.iter().cloned())
        .unique()
        .collect::<Vec<_>>();
    if created.is_empty() {
        return;
    }
    for (req, _) in removals
        .iter()
        .sorted_by_key(|(req, _)| std::cmp::Reverse(req.target.components().count()))
    {
        let chain = created_dirs_to_prune(&req.target, &created, home);
        if !chain.iter().any(|dir| dir.is_dir()) {
            continue;
        }
        let holders = removals
            .iter()
            .filter(|(_, dirs)| update_records && dirs.iter().any(|dir| chain.contains(dir)))
            .map(|(holder, _)| *holder)
            .collect::<Vec<_>>();
        // the directories and the records that list them change together
        let paths = chain
            .iter()
            .map(|dir| (dir.clone(), Capture::Shallow))
            .chain(
                holders
                    .iter()
                    .map(|holder| (target_state_path(holder), Capture::Full)),
            )
            .collect::<Vec<_>>();
        // nothing is removed without its write-ahead record
        let pending = match journal::begin_changes_with(DOTFILES_PART, &req.target_raw, paths) {
            Ok(pending) => pending,
            Err(err) => {
                warn!(
                    "files: keeping the directories created for {}: {err:#}",
                    req.target.display_user()
                );
                continue;
            }
        };
        let settled = remove_created_dirs(&chain, claimed, home);
        if !settled.is_empty() {
            for holder in &holders {
                update_target_state(holder, |state| {
                    state.created_dirs.retain(|dir| !settled.contains(dir));
                });
            }
        }
        journal::commit_changes(pending);
    }
}

/// The recorded directories a removal of `req`'s target may take with it,
/// deepest first, for dry runs.
fn prunable_created_dirs(req: &FileRequest) -> Vec<PathBuf> {
    created_dirs_to_prune(&req.target, &recorded_created_dirs(req), &dirs::HOME)
}

/// Directories other entries need to stay: their targets, and for entries
/// that walk a source directory every directory their files go in. An absent
/// entry needs nothing.
fn claimed_dirs<'a>(
    requests: impl IntoIterator<Item = &'a FileRequest>,
) -> Result<HashSet<PathBuf>> {
    let mut out = HashSet::new();
    for req in requests {
        if req.mode == FileMode::Absent {
            continue;
        }
        if matches!(req.mode, FileMode::Copy | FileMode::SymlinkEach) && req.source.is_dir() {
            out.extend(needed_dirs(req)?);
        }
        out.insert(req.target.clone());
    }
    Ok(out)
}

fn content_digest(content: &str) -> String {
    hash_sha256_to_str(content)
}

/// Whether the record already holds `rendered` as the target's content.
fn target_state_matches(req: &FileRequest, rendered: &str) -> bool {
    load_target_state(req)
        .and_then(|state| state.content_digest)
        .is_some_and(|digest| digest == content_digest(rendered))
}

/// Empty means nothing but whitespace, so a template that leaves a stray
/// newline between `{% if %}` blocks still counts.
fn renders_empty(rendered: &str) -> bool {
    rendered.trim().is_empty()
}

/// Whether applying `req` with this render removes its target instead of
/// writing it.
pub(crate) fn removes_target(req: &FileRequest, rendered: Option<&str>) -> bool {
    req.mode == FileMode::Template && req.remove_empty && rendered.is_some_and(renders_empty)
}

/// Classify the target of a template that rendered empty. Only an empty file
/// or one still holding exactly what mise last wrote is mise's to remove;
/// anything else may hold a user's edits.
fn empty_render_target(req: &FileRequest) -> Result<EmptyRenderTarget> {
    let target = &req.target;
    if target.is_symlink() {
        return Ok(EmptyRenderTarget::Conflict("it is a symlink"));
    }
    if !target.exists() {
        return Ok(EmptyRenderTarget::Absent);
    }
    if target.is_dir() {
        return Ok(EmptyRenderTarget::Conflict("it is a directory"));
    }
    // ownership cannot be proven without reading the content (a template
    // written with `permissions = "0200"`, say), so only --force removes it
    let Ok(current) = file::read(target) else {
        return Ok(EmptyRenderTarget::Conflict(
            "it cannot be read to confirm mise wrote it",
        ));
    };
    let owned = match str::from_utf8(&current) {
        Ok(current) => renders_empty(current) || target_state_matches(req, current),
        // mise only writes rendered text, so non-UTF-8 content is not its own
        Err(_) => false,
    };
    Ok(if owned {
        EmptyRenderTarget::Owned
    } else {
        EmptyRenderTarget::Conflict("it changed since mise last wrote it")
    })
}

fn link_points_to(source: &Path, target: &Path) -> bool {
    if !target.is_symlink() {
        return false;
    }
    std::fs::read_link(target).is_ok_and(|dest| {
        dest == source
            || points_at_same_file(target, source)
            || if dest.is_absolute() {
                lexical_normalize(&dest) == lexical_normalize(source)
            } else {
                // the kernel resolves a relative link from the directory the
                // link physically sits in, so its text is read from there too:
                // read from the configured spelling, a link in a symlinked
                // directory could seem to reach a source it does not
                resolve_relative_link(target, &dest)
                    .is_some_and(|resolved| resolved == physical_path(source))
            }
    })
}

/// Where a link at `target` holding the relative `dest` physically leads,
/// stepping through `dest` from the link's canonical directory as the
/// kernel does (see [`walk_physical`]).
fn resolve_relative_link(target: &Path, dest: &Path) -> Option<PathBuf> {
    walk_physical(target.parent()?.canonicalize().ok()?, dest)
}

/// Where `path` physically is, without requiring it to exist (so a deleted
/// file or a dangling link still has a location). Its own last component is
/// not followed: a source that is a symlink is the link, not what it names.
fn physical_path(path: &Path) -> PathBuf {
    walk_physical(PathBuf::new(), path).unwrap_or_else(|| lexical_normalize(path))
}

/// Append `path` to `base` a component at a time, resolving every directory
/// on the way before a later `..` steps out of it: `alias/../x` climbs out
/// of wherever `alias` really leads, so collapsing it lexically first could
/// name a different file. A component that does not exist cannot be a
/// symlink and is kept as written; a dangling one in the middle has no
/// location at all.
fn walk_physical(mut cur: PathBuf, path: &Path) -> Option<PathBuf> {
    use std::path::Component;
    let components = path.components().collect::<Vec<_>>();
    for (i, component) in components.iter().enumerate() {
        match component {
            Component::Prefix(_) | Component::RootDir => cur.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                cur.pop();
            }
            Component::Normal(name) => {
                cur.push(name);
                if i + 1 < components.len() {
                    match cur.canonicalize() {
                        Ok(canonical) => cur = canonical,
                        Err(_) if std::fs::symlink_metadata(&cur).is_err() => {}
                        Err(_) => return None,
                    }
                }
            }
        }
    }
    Some(cur)
}

fn tracked_stale_links(state: &SymlinkEachState, desired: &SymlinkEachState) -> Vec<PathBuf> {
    let desired = desired
        .links
        .iter()
        .map(|link| (&link.target, &link.source))
        .collect::<std::collections::HashMap<_, _>>();
    state
        .links
        .iter()
        .filter(|link| {
            desired
                .get(&link.target)
                .is_none_or(|source| **source != link.source)
                && link_points_to(&link.source, &link.target)
        })
        .map(|link| link.target.clone())
        .collect()
}

/// The source-relative path a symlink found at `rel` under an entry's target
/// would have been deployed from. Without `dot_prefix` the two are the same
/// path. With it, `.bashrc` could come from `dot-bashrc` or `.bashrc`, so the
/// link's own destination decides, and a link into neither is not the entry's.
fn linked_source_rel(req: &FileRequest, link: &Path, rel: &Path, dest: &Path) -> Option<PathBuf> {
    if !req.dot_prefix {
        return Some(rel.to_path_buf());
    }
    // a relative destination is read from where the link physically sits,
    // as the kernel reads it (see `link_points_to`)
    let (dest, source) = if dest.is_absolute() {
        (lexical_normalize(dest), lexical_normalize(&req.source))
    } else {
        (
            resolve_relative_link(link, dest)?,
            physical_path(&req.source),
        )
    };
    let source_rel = dest.strip_prefix(source).ok()?.to_path_buf();
    (target_rel(req, &source_rel) == rel).then_some(source_rel)
}

/// Whether a link's destination names `expected`, even when `expected` no
/// longer exists. Compared as paths, so the `.` in a source like
/// `/dotfiles/.` doesn't have to match character for character; a relative
/// destination (`relative` or `dotfiles.relative_symlinks`) is resolved from
/// where the link physically sits, like [`link_points_to`] does.
fn link_points_at(link: &Path, dest: &Path, expected: &Path) -> bool {
    dest == expected
        || if dest.is_absolute() {
            lexical_normalize(dest) == lexical_normalize(expected)
        } else {
            resolve_relative_link(link, dest)
                .is_some_and(|resolved| resolved == physical_path(expected))
        }
}

/// Legacy ownership discovery for installations that predate persistent
/// symlink-each state. A successful apply records the exact links it owns, so
/// this unbounded target walk happens at most once per target.
fn legacy_stale_links(req: &FileRequest) -> Result<Vec<PathBuf>> {
    if cfg!(windows) || !req.target.is_dir() || req.target.is_symlink() {
        return Ok(vec![]);
    }
    let mut out = vec![];
    for entry in walkdir::WalkDir::new(&req.target).sort_by_file_name() {
        // a path we can't read shouldn't fail the whole entry — the links we
        // can see are still worth pruning
        let entry = match entry {
            Ok(entry) => entry,
            Err(err) => {
                debug!("files: walking {}: {err}", req.target.display_user());
                continue;
            }
        };
        if !entry.file_type().is_symlink() {
            continue;
        }
        let dest = match std::fs::read_link(entry.path()) {
            Ok(dest) => dest,
            Err(err) => {
                debug!("files: reading {}: {err}", entry.path().display_user());
                continue;
            }
        };
        let Ok(rel) = entry.path().strip_prefix(&req.target) else {
            continue;
        };
        let Some(source_rel) = linked_source_rel(req, entry.path(), rel, &dest) else {
            continue;
        };
        let expected = req.source.join(&source_rel);
        if link_points_at(entry.path(), &dest, &expected)
            && (!expected.exists() || is_excluded(&source_rel, &req.exclude))
        {
            out.push(entry.path().to_path_buf());
        }
    }
    Ok(out)
}

/// Links recorded for this target that are no longer desired. Persistent
/// ownership state keeps this proportional to the managed source tree instead
/// of recursively walking a shared target such as the user's home directory.
fn stale_links(req: &FileRequest) -> Result<Vec<PathBuf>> {
    match load_symlink_each_state(req) {
        LoadedSymlinkEachState::Present(state) => Ok(tracked_stale_links(
            &state,
            &desired_symlink_each_state(req)?,
        )),
        LoadedSymlinkEachState::Missing | LoadedSymlinkEachState::Invalid => {
            legacy_stale_links(req)
        }
    }
}

fn symlink_each_state_needs_update(req: &FileRequest) -> Result<bool> {
    if cfg!(windows) {
        return Ok(false);
    }
    let desired = desired_symlink_each_state(req)?;
    Ok(!matches!(
        load_symlink_each_state(req),
        LoadedSymlinkEachState::Present(state) if state == desired
    ))
}

/// Compiled once here so a typo is reported against the entry that wrote
/// it, not on every walk of the source (or of a tracked directory).
/// Compiles a per-entry `exclude` or `include` list, or names the first
/// pattern that will not parse.
///
/// **A list that does not compile is an error, never a shorter list.**
/// Dropping a bad pattern fails open in both directions: a shorter
/// `exclude` captures files the user asked to leave out, and a shorter
/// `include` — or an empty one — captures the whole tree the user asked
/// to narrow. Neither is something to warn about and carry on from.
fn compile_patterns(
    key: &str,
    patterns: Option<Vec<String>>,
) -> std::result::Result<Option<Vec<glob::Pattern>>, String> {
    let Some(patterns) = patterns else {
        return Ok(None);
    };
    let mut compiled = vec![];
    for pattern in patterns {
        match glob::Pattern::new(&pattern) {
            Ok(pattern) => compiled.push(pattern),
            Err(err) => return Err(format!("invalid {key} pattern '{pattern}': {err}")),
        }
    }
    Ok(Some(compiled))
}

/// Whether a source-relative (or entry-relative) path is dropped by the
/// entry's `exclude` patterns. A pattern without `/` matches any single
/// path component, so `exclude = ["mise.toml"]` drops that file wherever
/// it sits in the tree and `["*.md"]` drops every markdown file; a pattern
/// containing `/` is anchored to the source root. Either kind matching a
/// directory takes everything under it, which is why ancestors are tested
/// too. Track entries use the same rules relative to the tracked path.
pub(crate) fn is_excluded(rel: &Path, patterns: &[glob::Pattern]) -> bool {
    patterns.iter().any(|pattern| {
        if pattern.as_str().contains('/') {
            rel.ancestors().any(|a| pattern.matches_path(a))
        } else {
            rel.components()
                .any(|c| pattern.matches(&c.as_os_str().to_string_lossy()))
        }
    })
}

/// every (source file, target path) pair of a directory-walking entry —
/// `symlink-each`, and `copy` with a directory source
fn walk_source_files(req: &FileRequest) -> Result<Vec<(PathBuf, PathBuf)>> {
    let files = walk_source_files_unchecked(req)?;
    if req.dot_prefix {
        check_dot_prefix_collisions(req, &files)?;
    }
    Ok(files)
}

/// Every (source file, target path) pair of a directory-walking entry, for
/// builds that skip apply's footprint validation: a `dot_prefix` source must
/// be a directory, and no two of its paths may deploy to the same place.
pub(crate) fn directory_source_files(req: &FileRequest) -> Result<Vec<(PathBuf, PathBuf)>> {
    if req.dot_prefix && !req.source.is_dir() {
        return Err(dot_prefix_file_source(req));
    }
    walk_source_files(req)
}

/// `dot_prefix` renames paths inside a directory; a single file keeps the
/// target its entry names, so the option would silently do nothing.
fn dot_prefix_file_source(req: &FileRequest) -> eyre::Report {
    eyre::eyre!(
        "[dotfiles].\"{}\": dot_prefix requires the source to be a directory: {}",
        req.target_raw,
        req.source.display_user()
    )
}

/// The path under an entry's target that a source-relative path deploys to.
pub(crate) fn target_rel(req: &FileRequest, rel: &Path) -> PathBuf {
    if !req.dot_prefix {
        return rel.to_path_buf();
    }
    rel.components()
        .map(|component| match component {
            std::path::Component::Normal(name) => {
                match name.to_str().and_then(|name| name.strip_prefix("dot-")) {
                    // `dot-` and `dot-.` would name `.` and `..`
                    Some(rest) if !rest.is_empty() && rest != "." => {
                        std::ffi::OsString::from(format!(".{rest}"))
                    }
                    _ => name.to_os_string(),
                }
            }
            other => other.as_os_str().to_os_string(),
        })
        .collect()
}

/// With `dot_prefix`, `dot-bashrc` and `.bashrc` in one source both deploy
/// to `.bashrc`, and a `dot-config/` directory beside a `.config` file puts
/// a directory where a file goes. Neither has a right answer to pick.
fn check_dot_prefix_collisions(req: &FileRequest, files: &[(PathBuf, PathBuf)]) -> Result<()> {
    let by_target: HashMap<&Path, &Path> = files
        .iter()
        .map(|(source, target)| (target.as_path(), source.as_path()))
        .collect();
    for (source, target) in files {
        if let Some(other) = by_target.get(target.as_path())
            && *other != source.as_path()
        {
            bail!(
                "[dotfiles].\"{}\": {} and {} both deploy to {} with dot_prefix",
                req.target_raw,
                source.display_user(),
                other.display_user(),
                target.display_user()
            );
        }
        for ancestor in target.ancestors().skip(1) {
            if !ancestor.starts_with(&req.target) {
                break;
            }
            if let Some(other) = by_target.get(ancestor) {
                bail!(
                    "[dotfiles].\"{}\": {} deploys to {}, but {} needs it to be a directory with dot_prefix",
                    req.target_raw,
                    other.display_user(),
                    ancestor.display_user(),
                    source.display_user()
                );
            }
        }
    }
    Ok(())
}

fn walk_source_files_unchecked(req: &FileRequest) -> Result<Vec<(PathBuf, PathBuf)>> {
    if req.manifest == Some(FileManifest::Git) {
        return git_tracked_paths(&req.source)?.into_iter().try_fold(
            vec![],
            |mut out, entry| -> Result<_> {
                if entry.is_gitlink || is_excluded(&entry.path, &req.exclude) {
                    return Ok(out);
                }
                let source = req.source.join(&entry.path);
                match std::fs::symlink_metadata(&source) {
                    Ok(metadata) if !metadata.file_type().is_dir() => {
                        out.push((source, req.target.join(target_rel(req, &entry.path))));
                    }
                    Ok(_) => {}
                    Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                    Err(err) => return Err(err.into()),
                }
                Ok(out)
            },
        );
    }
    let mut out = vec![];
    let mut walk = walkdir::WalkDir::new(&req.source)
        .sort_by_file_name()
        .into_iter();
    while let Some(entry) = walk.next() {
        let entry = entry?;
        let rel = entry.path().strip_prefix(&req.source)?;
        if !rel.as_os_str().is_empty() && is_excluded(rel, &req.exclude) {
            // don't descend into an excluded directory — its children are
            // excluded by the same pattern, just more slowly
            if entry.file_type().is_dir() {
                walk.skip_current_dir();
            }
            continue;
        }
        if entry.file_type().is_dir() {
            continue;
        }
        out.push((
            entry.path().to_path_buf(),
            req.target.join(target_rel(req, rel)),
        ));
    }
    Ok(out)
}

struct GitTrackedPath {
    path: PathBuf,
    is_gitlink: bool,
    is_symlink: bool,
}

fn git_tracked_paths(source: &Path) -> Result<Vec<GitTrackedPath>> {
    let mut root_command = Command::new("git");
    root_command
        .arg("-C")
        .arg(source)
        .args(["-c", "safe.directory=*", "rev-parse", "--show-toplevel"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    crate::git::sanitize_git_command(&mut root_command);
    let root_output = root_command.output().wrap_err_with(|| {
        format!(
            "failed to locate Git repository for {}",
            source.display_user()
        )
    })?;
    if !root_output.status.success() {
        let stderr = String::from_utf8_lossy(&root_output.stderr)
            .trim()
            .to_string();
        bail!(
            "failed to locate Git repository for {}: {}",
            source.display_user(),
            if stderr.is_empty() {
                root_output.status.to_string()
            } else {
                stderr
            }
        );
    }
    let root = root_output
        .stdout
        .strip_suffix(b"\n")
        .unwrap_or(&root_output.stdout);
    let root = root.strip_suffix(b"\r").unwrap_or(root);
    let safe = format!("safe.directory={}", path_buf_from_git_bytes(root).display());
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(source)
        .arg("-c")
        .arg(safe)
        .arg("-c")
        .arg("core.autocrlf=false")
        .args(["ls-files", "-z", "--cached", "--stage", "--", "."])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    crate::git::sanitize_git_command(&mut command);
    let output = command.output().wrap_err_with(|| {
        format!(
            "failed to list Git-tracked files in {}",
            source.display_user()
        )
    })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        bail!(
            "failed to list Git-tracked files in {}: {}",
            source.display_user(),
            if stderr.is_empty() {
                output.status.to_string()
            } else {
                stderr
            }
        );
    }
    output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
        .map(|record| {
            let tab = record
                .iter()
                .position(|byte| *byte == b'\t')
                .ok_or_else(|| {
                    eyre::eyre!(
                        "unexpected git ls-files output for {}",
                        source.display_user()
                    )
                })?;
            let metadata = &record[..tab];
            let path = path_buf_from_git_bytes(&record[tab + 1..]);
            if !metadata.ends_with(b" 0") {
                bail!(
                    "unresolved Git index entry {} in {}",
                    path.display(),
                    source.display_user()
                );
            }
            Ok(GitTrackedPath {
                is_gitlink: metadata.starts_with(b"160000 "),
                is_symlink: metadata.starts_with(b"120000 "),
                path,
            })
        })
        .collect()
}

/// Capture only the files selected by a Git manifest, preserving the source
/// repository and any untracked files around them.
pub(crate) fn capture_git_manifest(req: &FileRequest) -> Result<()> {
    for entry in git_tracked_paths(&req.source)? {
        if entry.is_gitlink || entry.is_symlink || is_excluded(&entry.path, &req.exclude) {
            continue;
        }
        let from = req.target.join(target_rel(req, &entry.path));
        let to = req.source.join(entry.path);
        if from.exists() || from.is_symlink() {
            if !file::same_file(&from, &to) {
                copy_path(&from, &to)?;
            }
        } else if to.exists() || to.is_symlink() {
            remove_existing(&to)?;
        }
    }
    Ok(())
}

#[cfg(unix)]
fn path_buf_from_git_bytes(path: &[u8]) -> PathBuf {
    use std::os::unix::ffi::OsStringExt;
    std::ffi::OsString::from_vec(path.to_vec()).into()
}

#[cfg(not(unix))]
fn path_buf_from_git_bytes(path: &[u8]) -> PathBuf {
    String::from_utf8_lossy(path).into_owned().into()
}

pub(crate) struct ApplyOpts {
    pub dry_run: bool,
    pub verbose: bool,
    /// replace conflicting targets (existing real files where a symlink
    /// should go, or type mismatches) instead of erroring
    pub force: bool,
    pub force_hint: &'static str,
    pub yes: bool,
}

pub(crate) struct ApplyPlan<'a> {
    todo: Vec<(&'a FileRequest, Option<String>)>,
    record_symlink_each: Vec<&'a FileRequest>,
    /// converged templates whose ownership record is missing or stale, with
    /// the content to record
    record_templates: Vec<(&'a FileRequest, String)>,
    /// converged entries whose target is already gone but whose record
    /// still lists directories mise created that are there: an earlier
    /// prune could not remove them, so this run retries. Not a file change,
    /// so never shown as one.
    prune_leftovers: Vec<&'a FileRequest>,
    /// directories active entries need, which a removed target never takes
    /// along; only computed when a removal has directories it could prune
    claimed_dirs: HashSet<PathBuf>,
    reconciliation: SymlinkEachReconciliation,
}

/// Apply all entries that aren't already in the desired state. Conflicting
/// targets (a real file where a symlink should go, a directory where a file
/// should go) are an error unless `force` is set — content updates for
/// copy/template entries are not conflicts, overwriting is their job. Returns
/// `false` when the user declines the confirmation prompt. The target paths
/// written or removed are appended to `written` as each entry is applied,
/// so a caller still sees what changed when a later entry fails; nothing is
/// appended on a dry run.
pub(crate) fn apply(
    config: &Config,
    requests: &[FileRequest],
    opts: &ApplyOpts,
    secrets: &SecretValues,
    written: &mut Vec<PathBuf>,
) -> Result<bool> {
    execute_apply(
        config,
        plan_apply(config, requests, opts, secrets)?,
        opts,
        written,
    )
}

pub(crate) fn execute_apply(
    config: &Config,
    plan: ApplyPlan<'_>,
    opts: &ApplyOpts,
    written: &mut Vec<PathBuf>,
) -> Result<bool> {
    let has_reconciliation = !plan.reconciliation.stale_links.is_empty();
    if plan.todo.is_empty() && !has_reconciliation {
        if !opts.dry_run {
            prune_after_apply([], &plan);
            for req in plan.record_symlink_each {
                let pending = journal::begin_changes(
                    DOTFILES_PART,
                    &req.target_raw,
                    [symlink_each_state_path(req)],
                )?;
                save_symlink_each_state(req);
                journal::commit_changes(pending);
            }
            record_template_states(&plan.record_templates)?;
        }
        info!("files: all files are applied");
        return Ok(true);
    }
    if opts.dry_run {
        for link in &plan.reconciliation.stale_links {
            miseprintln!("rm {}", link.target.display_user());
        }
        for (req, rendered) in &plan.todo {
            // template state wasn't computed (no rendering on dry runs), so
            // the entry may already be converged
            let conditional = req.mode == FileMode::Template && rendered.is_none();
            let suffix = if conditional { " (if changed)" } else { "" };
            miseprintln!("{}{suffix}", describe(req)?);
            if opts.verbose && !conditional {
                print_diff(config, req, rendered.as_deref())?;
            }
        }
        return Ok(true);
    }
    if !opts.yes && console::user_attended_stderr() {
        let list = plan
            .todo
            .iter()
            .map(|(r, _)| r.target_raw.clone())
            .chain(
                plan.reconciliation
                    .targets
                    .iter()
                    .map(|target| target.display_user()),
            )
            .unique()
            .collect::<Vec<_>>()
            .join(", ");
        if !prompt::confirm(format!("files: apply {list}?"))?.is_yes() {
            info!("files: skipped");
            return Ok(false);
        }
    }
    for link in &plan.reconciliation.stale_links {
        if link_points_to(&link.source, &link.target) {
            let item = link.target.display_user().to_string();
            // parents up to the entry target may be pruned once empty
            let root = plan
                .reconciliation
                .targets
                .iter()
                .find(|target| link.target.starts_with(target));
            let mut paths = vec![(link.target.clone(), Capture::Full)];
            if let Some(root) = root {
                paths.extend(
                    dirs_between(&link.target, root)
                        .into_iter()
                        .map(|dir| (dir, Capture::Shallow)),
                );
            }
            let pending = journal::begin_changes_with(DOTFILES_PART, &item, paths)?;
            file::remove_file(&link.target)?;
            written.push(link.target.clone());
            journal::commit_changes(pending);
        }
    }
    let mut removals = vec![];
    for (req, rendered) in &plan.todo {
        recheck_removal(req, rendered.as_deref(), opts.force)?;
        let pending =
            journal::begin_changes_with(DOTFILES_PART, &req.target_raw, touched_paths(req)?)?;
        apply_one(req, rendered.as_deref(), written)?;
        if apply_removes_target(req, rendered.as_deref()) {
            removals.push(*req);
        }
        if req.mode == FileMode::SymlinkEach {
            save_symlink_each_state(req);
        }
        journal::commit_changes(pending);
        if removes_target(req, rendered.as_deref()) {
            info!(
                "files: removed {} (template rendered empty)",
                req.target.display_user()
            );
        } else {
            info!("files: {}", describe_applied(req)?);
        }
    }
    prune_after_apply(removals, &plan);
    record_template_states(&plan.record_templates)?;
    for req in plan.record_symlink_each {
        if !plan.todo.iter().any(|(todo, _)| std::ptr::eq(*todo, req)) {
            let pending = journal::begin_changes(
                DOTFILES_PART,
                &req.target_raw,
                [symlink_each_state_path(req)],
            )?;
            save_symlink_each_state(req);
            journal::commit_changes(pending);
        }
    }
    cleanup_reconciled_directories(&plan.reconciliation)?;
    let applied = plan
        .todo
        .iter()
        .map(|(r, _)| r.target_raw.clone())
        .chain(
            plan.reconciliation
                .targets
                .iter()
                .map(|target| target.display_user()),
        )
        .unique()
        .collect::<Vec<_>>();
    info!("files: applied {}", applied.join(", "));
    Ok(true)
}

/// Plan and validate an apply without changing targets. Templates are rendered
/// here so execution writes exactly the content that was validated.
pub(crate) fn plan_apply<'a>(
    config: &Config,
    requests: &'a [FileRequest],
    opts: &ApplyOpts,
    secrets: &SecretValues,
) -> Result<ApplyPlan<'a>> {
    let active_requests = files_from_config(config)?;
    plan_apply_with_active(config, requests, &active_requests, opts, secrets)
}

/// Plan an apply against the requests that will be active when it executes.
/// This is used by transactional config updates that apply before saving the
/// prospective configuration.
pub(crate) fn plan_apply_with_active<'a>(
    config: &Config,
    requests: &'a [FileRequest],
    active_requests: &[FileRequest],
    opts: &ApplyOpts,
    secrets: &SecretValues,
) -> Result<ApplyPlan<'a>> {
    validate_composed_file_footprints(requests)?;
    // pre-rendered template output rides along so it's written as compared,
    // and exec() in templates runs once per apply
    let mut todo: Vec<(&FileRequest, Option<String>)> = vec![];
    let mut missing_sources = vec![];
    let mut broken = vec![];
    let mut conflicts = vec![];
    let mut record_symlink_each = vec![];
    let mut record_templates = vec![];
    let mut prune_leftovers = vec![];
    let mut missing_permission_targets = vec![];
    for req in requests {
        // a tracked file is never written: history captures it as it is
        if req.mode == FileMode::Track {
            continue;
        }
        // report every problem in one pass instead of fix-and-retry — a
        // render or check failure on one entry must not hide the rest
        if req.mode.has_source() && !req.source.exists() {
            missing_sources.push(format!(
                "  [dotfiles].\"{}\": {}",
                req.target_raw,
                req.source.display_user()
            ));
            continue;
        }
        // a permissions-only entry never creates its target and never
        // follows a link there: nothing to do is not an error. A missing
        // directory another entry of this apply creates is decided once
        // every entry is planned.
        if req.mode == FileMode::Permissions
            && let Some(reason) = permissions_target_unavailable(req)?
        {
            if std::fs::symlink_metadata(&req.target).is_err() {
                missing_permission_targets.push((req, reason));
            } else {
                warn!(
                    "[dotfiles].\"{}\": {reason}; permissions not set",
                    req.target_raw
                );
            }
            continue;
        }
        // rendering can run exec() — a dry run must not execute anything,
        // so list template entries without computing their current state
        if opts.dry_run && req.mode == FileMode::Template {
            conflicts.extend(find_conflicts(req)?);
            todo.push((req, None));
            continue;
        }
        let rendered = match req.mode {
            FileMode::Template => match render_template(config, req, secrets) {
                Ok(rendered) => Some(rendered),
                // already carries the entry's context
                Err(err) => {
                    broken.push(format!("  {err}"));
                    continue;
                }
            },
            _ => None,
        };
        match check_rendered(req, rendered.as_deref()) {
            Ok(FileState::Applied) => {
                // a target already gone can still leave directories an
                // earlier prune could not remove; retry them after the run
                if apply_removes_target(req, rendered.as_deref()) && has_leftover_created_dirs(req)
                {
                    prune_leftovers.push(req);
                }
                if req.mode == FileMode::SymlinkEach && symlink_each_state_needs_update(req)? {
                    record_symlink_each.push(req);
                }
                if let Some(update) = template_state_update(req, rendered) {
                    record_templates.push((req, update));
                }
                continue;
            }
            Ok(_) => {}
            Err(err) => {
                broken.push(format!("  [dotfiles].\"{}\": {err}", req.target_raw));
                continue;
            }
        }
        if removes_target(req, rendered.as_deref()) {
            if matches!(empty_render_target(req)?, EmptyRenderTarget::Conflict(_)) {
                conflicts.push(req.target.clone());
            }
        } else {
            conflicts.extend(find_conflicts(req)?);
        }
        todo.push((req, rendered));
    }
    let mut problems = vec![];
    if !missing_sources.is_empty() {
        problems.push(format!(
            "sources do not exist:\n{}",
            missing_sources.join("\n")
        ));
    }
    if !broken.is_empty() {
        problems.push(format!("entries with errors:\n{}", broken.join("\n")));
    }
    if !conflicts.is_empty() && !opts.force {
        problems.push(format!(
            "refusing to overwrite existing files ({}):\n{}",
            opts.force_hint,
            conflicts
                .iter()
                .map(|p| format!("  {}", p.display_user()))
                .collect::<Vec<_>>()
                .join("\n")
        ));
    }
    if !problems.is_empty() {
        bail!("files: {}", problems.join("\nfiles: "));
    }
    // entries run in order, so one that creates a directory a
    // permissions-only entry names has made it by the time the chmod runs
    let mut deferred = vec![];
    for (req, reason) in missing_permission_targets {
        if todo.iter().any(|(other, _)| {
            // an absent entry removes rather than creates
            !matches!(other.mode, FileMode::Permissions | FileMode::Absent)
                && other.target.starts_with(&req.target)
        }) {
            deferred.push((req, None));
        } else {
            warn!(
                "[dotfiles].\"{}\": {reason}; permissions not set",
                req.target_raw
            );
        }
    }
    todo.extend(deferred);
    let claimed_dirs = if !prune_leftovers.is_empty()
        || todo.iter().any(|(req, rendered)| {
            apply_removes_target(req, rendered.as_deref()) && !removal_created_dirs(req).is_empty()
        }) {
        claimed_dirs(active_requests)?
    } else {
        HashSet::new()
    };
    Ok(ApplyPlan {
        todo,
        record_symlink_each,
        record_templates,
        prune_leftovers,
        claimed_dirs,
        reconciliation: plan_symlink_each_reconciliation(active_requests, requests)?,
    })
}

/// The plan classified a removal's target before the confirmation prompt; an
/// edit made since then must not be removed without `--force`.
fn recheck_removal(req: &FileRequest, rendered: Option<&str>, force: bool) -> Result<()> {
    if !force
        && removes_target(req, rendered)
        && let EmptyRenderTarget::Conflict(reason) = empty_render_target(req)?
    {
        bail!(
            "files: {} changed during apply: {reason}; use --force to remove it",
            req.target.display_user()
        );
    }
    Ok(())
}

/// The ownership-record change a converged template needs, if any. Recording
/// every template, not just `remove_empty` ones, means turning `remove_empty`
/// on later still finds the evidence that the current file is mise's.
fn template_state_update(req: &FileRequest, rendered: Option<String>) -> Option<String> {
    if req.mode != FileMode::Template {
        return None;
    }
    let rendered = rendered?;
    // a converged removal wrote nothing; the record keeps the last write
    if removes_target(req, Some(&rendered)) || target_state_matches(req, &rendered) {
        None
    } else {
        Some(rendered)
    }
}

/// Journal and write the records of converged templates.
fn record_template_states(updates: &[(&FileRequest, String)]) -> Result<()> {
    for (req, content) in updates {
        let pending =
            journal::begin_changes(DOTFILES_PART, &req.target_raw, [target_state_path(req)])?;
        save_target_state(req, content);
        journal::commit_changes(pending);
    }
    Ok(())
}

fn cleanup_reconciled_directories(reconciliation: &SymlinkEachReconciliation) -> Result<()> {
    for target in &reconciliation.targets {
        remove_empty_dirs_upward(
            reconciliation
                .stale_links
                .iter()
                .filter(|link| link.target.starts_with(target))
                .filter_map(|link| link.target.parent()),
            |dir| dir != target && dir.starts_with(target),
        )?;
    }
    Ok(())
}

pub(crate) struct UnapplyOpts {
    pub dry_run: bool,
    pub verbose: bool,
    /// remove targets whose ownership cannot be verified from their current
    /// state (for example a modified copy)
    pub force: bool,
    pub yes: bool,
}

#[derive(Debug)]
pub(crate) struct UnapplyPlan<'a> {
    req: &'a FileRequest,
    paths: Vec<PathBuf>,
    /// directory-walking modes share their target with unmanaged files, so
    /// only remove directories after their managed children are gone and only
    /// while they are empty
    cleanup_empty_dirs: bool,
    /// template dry-runs do not render, so the removal is conditional
    conditional: bool,
    /// remove persistent symlink-each ownership after successful cleanup
    clear_symlink_each_state: bool,
}

/// Remove configured whole-file entries without recursively deleting
/// directories that may contain unmanaged files. Symlinks carry their own
/// ownership evidence. Copies and templates must still match their source
/// unless `--force` was given.
pub(crate) fn plan_unapply<'a>(
    requests: &'a [FileRequest],
    opts: &UnapplyOpts,
) -> Result<Vec<UnapplyPlan<'a>>> {
    let mut todo = vec![];
    let mut problems = vec![];
    for req in requests {
        match plan_unapply_one(req, opts) {
            Ok(Some(plan)) => todo.push(plan),
            Ok(None) => {}
            Err(err) => problems.push(format!("  [dotfiles].\"{}\": {err}", req.target_raw)),
        }
    }
    if !problems.is_empty() {
        bail!(
            "files: cannot unapply these entries:\n{}",
            problems.join("\n")
        );
    }
    Ok(todo)
}

/// Resolve checks that may execute user-authored template functions. This runs
/// only after interactive confirmation, but still before any mutation.
pub(crate) fn resolve_unapply(
    config: &Config,
    plans: &mut Vec<UnapplyPlan<'_>>,
    opts: &UnapplyOpts,
    secrets: &SecretValues,
) -> Result<()> {
    if opts.dry_run {
        return Ok(());
    }
    let mut problems = vec![];
    let rendered = plans
        .iter()
        .map(|plan| {
            if plan.conditional {
                match render_template(config, plan.req, secrets) {
                    Ok(rendered) => Some(rendered),
                    Err(err) => {
                        problems.push(format!("  [dotfiles].\"{}\": {err}", plan.req.target_raw));
                        None
                    }
                }
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    if !problems.is_empty() {
        bail!(
            "files: cannot unapply these entries:\n{}",
            problems.join("\n")
        );
    }

    // Template functions can change any selected target. Refresh every plan
    // only after all templates have rendered so no stale validation is used.
    let mut refreshed = Vec::with_capacity(plans.len());
    for (plan, rendered) in std::mem::take(plans).into_iter().zip(rendered) {
        let result = if let Some(rendered) = rendered {
            let mut paths = IndexMap::new();
            plan_expected_content(rendered.as_bytes(), &plan.req.target, false, &mut paths).map(
                |_| {
                    Some(UnapplyPlan {
                        req: plan.req,
                        paths: paths.into_keys().collect(),
                        cleanup_empty_dirs: false,
                        conditional: false,
                        clear_symlink_each_state: false,
                    })
                },
            )
        } else {
            plan_unapply_one(plan.req, opts)
        };
        match result {
            Ok(Some(plan)) => refreshed.push(plan),
            Ok(None) => {}
            Err(err) => problems.push(format!("  [dotfiles].\"{}\": {err}", plan.req.target_raw)),
        }
    }
    if !problems.is_empty() {
        bail!(
            "files: cannot unapply these entries:\n{}",
            problems.join("\n")
        );
    }
    *plans = refreshed;
    Ok(())
}

pub(crate) fn execute_unapply(
    config: &Config,
    plans: &[UnapplyPlan<'_>],
    opts: &UnapplyOpts,
) -> Result<()> {
    let todo = plans;
    if todo.is_empty() {
        info!("files: all files are unapplied");
        return Ok(());
    }
    if opts.dry_run {
        for plan in todo {
            for path in &plan.paths {
                let suffix = if plan.conditional {
                    " (if unchanged)"
                } else {
                    ""
                };
                miseprintln!("rm {}{suffix}", path.display_user());
            }
            for dir in prunable_created_dirs(plan.req) {
                miseprintln!("rmdir {} (if empty)", dir.display_user());
            }
            if plan.cleanup_empty_dirs {
                miseprintln!("rmdir {} (if empty)", plan.req.target.display_user());
            }
            if opts.verbose && plan.cleanup_empty_dirs {
                miseprintln!(
                    "  preserve unmanaged files under {}",
                    plan.req.target.display_user()
                );
            }
        }
        return Ok(());
    }
    if !opts.yes && console::user_attended_stderr() {
        let list = todo
            .iter()
            .map(|plan| plan.req.target_raw.clone())
            .join(", ");
        if !prompt::confirm(format!("files: unapply {list}?"))?.is_yes() {
            info!("files: skipped");
            return Ok(());
        }
    }
    // The records go with their targets, so they are read up front. A plan
    // with no paths only clears the record of a target that is already gone;
    // the directories mise created for it may still be there and empty.
    let removals = todo
        .iter()
        .map(|plan| (plan.req, recorded_created_dirs(plan.req)))
        .filter(|(_, dirs)| !dirs.is_empty())
        .collect::<Vec<_>>();
    // Directories mise created go only when no entry that stays needs them.
    // Loaded before anything changes, so a config that cannot be read fails
    // the unapply rather than leaving it half done.
    let claimed = if !removals.is_empty() {
        let unapplied = todo
            .iter()
            .map(|plan| lexical_normalize(&plan.req.target))
            .collect::<HashSet<_>>();
        claimed_dirs(
            files_from_config(config)?
                .iter()
                .filter(|req| !unapplied.contains(&lexical_normalize(&req.target))),
        )?
    } else {
        HashSet::new()
    };
    for plan in todo {
        let mut paths: Vec<(PathBuf, Capture)> = plan
            .paths
            .iter()
            .map(|path| (path.clone(), Capture::Full))
            .collect();
        if plan.clear_symlink_each_state {
            paths.push((symlink_each_state_path(plan.req), Capture::Full));
        }
        let clear_target_state = target_state_path(plan.req).exists();
        if clear_target_state {
            paths.push((target_state_path(plan.req), Capture::Full));
        }
        if plan.cleanup_empty_dirs {
            // the upward walk removes directories that end up empty
            for path in &plan.paths {
                paths.extend(
                    dirs_between(path, &plan.req.target)
                        .into_iter()
                        .map(|dir| (dir, Capture::Shallow)),
                );
            }
            paths.push((plan.req.target.clone(), Capture::Shallow));
        }
        let pending = journal::begin_changes_with(DOTFILES_PART, &plan.req.target_raw, paths)?;
        unapply_one(plan)?;
        if plan.clear_symlink_each_state {
            remove_symlink_each_state(plan.req)?;
        }
        if clear_target_state {
            remove_target_state(plan.req)?;
        }
        journal::commit_changes(pending);
    }
    // once every target is gone, so siblings no longer hold their parents
    prune_created_dirs(&removals, &claimed, &dirs::HOME, false);
    info!(
        "files: unapplied {}",
        todo.iter()
            .map(|plan| plan.req.target_raw.clone())
            .join(", ")
    );
    Ok(())
}

fn plan_unapply_one<'a>(
    req: &'a FileRequest,
    opts: &UnapplyOpts,
) -> Result<Option<UnapplyPlan<'a>>> {
    let mut paths = IndexMap::<PathBuf, ()>::new();
    let mut cleanup_empty_dirs = false;
    let mut conditional = false;
    let mut clear_symlink_each_state = false;
    if records_created_dirs(req) && !req.target.exists() && !req.target.is_symlink() {
        // Nothing to remove, but a record left by an earlier removal (a
        // `remove_empty` apply, or the user deleting the file) still claims
        // the path and the directories mise created for it; execute clears
        // the record and prunes those directories with the (empty) plan.
        return Ok(target_state_path(req).exists().then_some(UnapplyPlan {
            req,
            paths: vec![],
            cleanup_empty_dirs: false,
            conditional: false,
            clear_symlink_each_state: false,
        }));
    }
    match req.mode {
        // A Windows file link that came out as a copy is planned by content further down; one
        // that is a real symlink belongs here, where the link target is what identifies it as
        // ours. `plan_expected_content` requires a non-symlink, so routing a symlink there
        // would make unapply demand `--force`.
        FileMode::Symlink
            if !(cfg!(windows) && req.source.is_file() && !req.target.is_symlink()) =>
        {
            if req.target.is_symlink() {
                let dest = std::fs::read_link(&req.target)?;
                // `link_points_to` also resolves a relative link whose source
                // is gone, which `canonicalize` cannot
                if opts.force || link_points_to(&req.source, &req.target) {
                    paths.insert(req.target.clone(), ());
                } else {
                    bail!(
                        "target symlink points to {}, use --force to remove it",
                        dest.display_user()
                    );
                }
            } else if req.target.exists() {
                if opts.force {
                    paths.insert(req.target.clone(), ());
                } else {
                    bail!("target is not the managed symlink, use --force to remove it");
                }
            }
        }
        FileMode::SymlinkEach if !cfg!(windows) => {
            clear_symlink_each_state = symlink_each_state_path(req).exists();
            if req.target.is_symlink() || (req.target.exists() && !req.target.is_dir()) {
                if opts.force {
                    paths.insert(req.target.clone(), ());
                } else {
                    bail!("target is not the managed directory, use --force to remove it");
                }
            } else if req.target.is_dir() {
                for path in owned_links(req)? {
                    paths.insert(path, ());
                }
                cleanup_empty_dirs = true;
            }
        }
        FileMode::Copy | FileMode::SymlinkEach if req.source.is_dir() => {
            if req.target.is_symlink() || (req.target.exists() && !req.target.is_dir()) {
                if opts.force {
                    paths.insert(req.target.clone(), ());
                } else {
                    bail!("target is not the managed directory, use --force to remove it");
                }
            } else if req.target.is_dir() {
                for (source, target) in walk_source_files(req)? {
                    plan_regular_file(&source, &target, opts.force, &mut paths)?;
                }
                cleanup_empty_dirs = true;
            }
        }
        FileMode::Copy => {
            if req.target.is_dir() && !req.source.exists() {
                bail!("source directory is missing, so managed children cannot be identified");
            }
            plan_single_file(req, opts, &mut paths)?;
        }
        // a tracked file was never written by mise: stop tracking it with
        // `mise dot untrack`, the file itself stays
        FileMode::Track => {
            info!(
                "files: {} is tracked in place; nothing to remove (use `mise dot untrack` to stop tracking it)",
                req.target.display_user()
            );
            return Ok(None);
        }
        // undoing an absence would mean recreating a file mise never wrote
        FileMode::Absent => {
            debug!(
                "files: {} is declared absent; nothing to unapply",
                req.target.display_user()
            );
            return Ok(None);
        }
        // mise only changed the permissions of a file it does not own, so
        // unapplying never removes it, not even with --force
        FileMode::Permissions => {
            debug!(
                "files: {} has only its permissions managed; nothing to remove",
                req.target.display_user()
            );
            return Ok(None);
        }
        // an absent target was handled above
        FileMode::Content => {
            if opts.force {
                paths.insert(req.target.clone(), ());
            } else {
                plan_inline_file(req, &mut paths)?;
            }
        }
        FileMode::Symlink => {
            plan_single_file(req, opts, &mut paths)?;
        }
        FileMode::SymlinkEach => {
            bail!("mode symlink-each requires the source to be a directory");
        }
        FileMode::Template => {
            if opts.force {
                paths.insert(req.target.clone(), ());
            } else if opts.dry_run {
                // Rendering may execute commands. Keep dry-run inert, matching
                // apply/status policy, and describe the removal as conditional.
                paths.insert(req.target.clone(), ());
                conditional = true;
            } else {
                if !req.source.exists() {
                    bail!("source is missing; use --force to remove the target");
                }
                // Rendering may execute user-authored commands. Defer it until
                // after the complete unapply plan has been confirmed.
                conditional = true;
            }
        }
    }
    if paths.is_empty() && !cleanup_empty_dirs && !conditional && !clear_symlink_each_state {
        Ok(None)
    } else {
        Ok(Some(UnapplyPlan {
            req,
            paths: paths.into_keys().collect(),
            cleanup_empty_dirs,
            conditional,
            clear_symlink_each_state,
        }))
    }
}

fn plan_inline_file(req: &FileRequest, paths: &mut IndexMap<PathBuf, ()>) -> Result<()> {
    if !req.target.is_symlink()
        && req.target.is_file()
        && file::read(&req.target)? == req.content.as_deref().expect("inline content").as_bytes()
    {
        paths.insert(req.target.clone(), ());
    } else if req.target.exists() || req.target.is_symlink() {
        bail!("target differs from the managed content, use --force to remove it");
    }
    Ok(())
}

fn plan_single_file(
    req: &FileRequest,
    opts: &UnapplyOpts,
    paths: &mut IndexMap<PathBuf, ()>,
) -> Result<()> {
    if !req.source.exists() && req.target.exists() && !opts.force {
        bail!("source is missing; use --force to remove the target");
    }
    if opts.force && (req.target.exists() || req.target.is_symlink()) {
        paths.insert(req.target.clone(), ());
    } else if req.source.exists() {
        plan_regular_file(&req.source, &req.target, false, paths)?;
    }
    Ok(())
}

fn plan_regular_file(
    source: &Path,
    target: &Path,
    force: bool,
    paths: &mut IndexMap<PathBuf, ()>,
) -> Result<()> {
    if !target.exists() && !target.is_symlink() {
        return Ok(());
    }
    if source.is_file() {
        plan_expected_content(&file::read(source)?, target, force, paths)
    } else if force {
        paths.insert(target.to_path_buf(), ());
        Ok(())
    } else {
        bail!(
            "cannot verify {}; use --force to remove it",
            target.display_user()
        )
    }
}

fn plan_expected_content(
    expected: &[u8],
    target: &Path,
    force: bool,
    paths: &mut IndexMap<PathBuf, ()>,
) -> Result<()> {
    if force || (target.is_file() && !target.is_symlink() && file::read(target)? == expected) {
        paths.insert(target.to_path_buf(), ());
        Ok(())
    } else {
        bail!(
            "{} differs from its managed source; use --force to remove it",
            target.display_user()
        )
    }
}

/// All symlinks under a symlink-each target that point exactly where this
/// entry maps that relative path. Unlike stale-link pruning this includes
/// both current and deleted source files because unapply removes the entry's
/// complete observable footprint.
fn legacy_owned_links(req: &FileRequest) -> Result<Vec<PathBuf>> {
    if !req.target.is_dir() || req.target.is_symlink() {
        return Ok(vec![]);
    }
    let mut out = vec![];
    for entry in walkdir::WalkDir::new(&req.target).sort_by_file_name() {
        let entry = match entry {
            Ok(entry) => entry,
            Err(err) => {
                debug!("files: walking {}: {err}", req.target.display_user());
                continue;
            }
        };
        if !entry.file_type().is_symlink() {
            continue;
        }
        let Ok(rel) = entry.path().strip_prefix(&req.target) else {
            continue;
        };
        let dest = match std::fs::read_link(entry.path()) {
            Ok(dest) => dest,
            Err(err) => {
                debug!("files: reading {}: {err}", entry.path().display_user());
                continue;
            }
        };
        let Some(source_rel) = linked_source_rel(req, entry.path(), rel, &dest) else {
            continue;
        };
        // Excluded paths are outside this entry's managed footprint. Even an
        // exact source-shaped link there may have been created by the user.
        if is_excluded(&source_rel, &req.exclude) {
            continue;
        }
        let expected = req.source.join(&source_rel);
        if link_points_at(entry.path(), &dest, &expected)
            || points_at_same_file(entry.path(), &expected)
        {
            out.push(entry.path().to_path_buf());
        }
    }
    Ok(out)
}

fn owned_links(req: &FileRequest) -> Result<Vec<PathBuf>> {
    let state = match load_symlink_each_state(req) {
        LoadedSymlinkEachState::Present(state) => state,
        LoadedSymlinkEachState::Missing | LoadedSymlinkEachState::Invalid => {
            return legacy_owned_links(req);
        }
    };
    let mut out = IndexMap::<PathBuf, ()>::new();
    for link in state.links {
        if link_points_to(&link.source, &link.target) {
            out.insert(link.target, ());
        }
    }
    Ok(out.into_keys().collect())
}

fn unapply_one(plan: &UnapplyPlan<'_>) -> Result<()> {
    let mut parents = vec![];
    for path in &plan.paths {
        debug!("files: removing {}", path.display_user());
        if path.is_symlink() || path.is_file() {
            file::remove_file(path)?;
        } else if path.is_dir() {
            // Only reached for a forced type conflict. The user explicitly
            // asked to remove the declared whole target.
            file::remove_all(path)?;
        }
        if let Some(parent) = path.parent() {
            parents.push(parent.to_path_buf());
        }
    }
    if plan.cleanup_empty_dirs && plan.req.target.is_dir() {
        parents.push(plan.req.target.clone());
        // the walk ends at the target: its parent does not start with it
        remove_empty_dirs_upward(parents.iter().map(PathBuf::as_path), |dir| {
            dir.starts_with(&plan.req.target)
        })?;
    }
    Ok(())
}

/// existing paths this entry would have to delete or replace — not counting
/// content overwrites by copy/template (those are the declared intent) or
/// re-pointing symlinks (always mise-owned territory)
fn find_conflicts(req: &FileRequest) -> Result<Vec<PathBuf>> {
    // A symlink at the target is one mise made, so it is re-pointable. Anything else that is
    // there — a regular file or a directory — belongs to someone else and needs `--force`.
    //
    // Windows used to exempt regular files here, on the grounds that a file link becomes a copy
    // on that platform so an existing file is only a content update. That reasoning holds for
    // `copy` mode, which is declared as overwriting; for a `symlink` entry it meant the copy
    // silently destroyed a file the user wrote, with no `--force` and no message — while unix
    // refused the same apply.
    let file_link_conflicts =
        |target: &Path| -> Result<bool> { Ok(target.exists() && !target.is_symlink()) };
    let mut out = vec![];
    match req.mode {
        FileMode::Symlink => {
            if file_link_conflicts(&req.target)? {
                out.push(req.target.clone());
            }
        }
        FileMode::SymlinkEach => {
            // a regular file where the target directory (or a nested one)
            // should go blocks the whole entry
            for dir in needed_dirs(req)? {
                if dir.exists() && !dir.is_dir() {
                    out.push(dir);
                }
            }
            for (_source, target) in walk_source_files(req)? {
                if file_link_conflicts(&target)? {
                    out.push(target);
                }
            }
        }
        FileMode::Copy | FileMode::Template => {
            // a dir where a file should go (or vice versa) must be removed;
            // file-over-file is an ordinary overwrite
            if req.target.exists() && req.target.is_dir() != req.source.is_dir() {
                out.push(req.target.clone());
            }
        }
        FileMode::Content => {
            if req.target.is_dir() {
                out.push(req.target.clone());
            }
        }
        // removal is the declared intent; a directory is refused by
        // `check_absent` instead, even with --force
        FileMode::Track | FileMode::Absent | FileMode::Permissions => {}
    }
    Ok(out)
}

fn describe(req: &FileRequest) -> Result<String> {
    let src = req.source.display_user();
    let tgt = req.target.display_user();
    // `ln -r` (GNU) is the familiar spelling of a relative link
    let ln = if req.relative { "ln -sfr" } else { "ln -sf" };
    Ok(match req.mode {
        FileMode::Track => format!("track {tgt} in place"),
        FileMode::Absent => format!("rm {tgt}"),
        FileMode::Symlink => format!("{ln} {src} {tgt}"),
        FileMode::SymlinkEach => {
            let stale = stale_links(req)?.len();
            let removals = match stale {
                0 => String::new(),
                n => format!(", rm {n} stale link(s)"),
            };
            format!(
                "{ln} {src}/* into {tgt}/ ({} files){removals}",
                walk_source_files(req)?.len()
            )
        }
        FileMode::Copy if req.source.is_dir() => format!("cp -r {src} {tgt}"),
        FileMode::Copy => format!("cp {src} {tgt}"),
        FileMode::Template => format!("render {src} -> {tgt}"),
        FileMode::Content => format!("write inline content to {tgt}"),
        FileMode::Permissions => format!("chmod {:04o} {tgt}", req.permissions.unwrap_or_default()),
    })
}

fn describe_applied(req: &FileRequest) -> Result<String> {
    let src = req.source.display_user();
    let tgt = req.target.display_user();
    Ok(match req.mode {
        FileMode::Track => format!("tracked {tgt} in place"),
        FileMode::Absent => format!("removed {tgt}"),
        FileMode::Symlink => format!("created symlink {tgt} -> {src}"),
        FileMode::SymlinkEach => format!(
            "created {} symlink(s) from {src} in {tgt}",
            walk_source_files(req)?.len()
        ),
        FileMode::Copy => format!("copied {src} to {tgt}"),
        FileMode::Template => format!("rendered {src} to {tgt}"),
        FileMode::Content => format!("wrote inline content to {tgt}"),
        FileMode::Permissions => format!(
            "set permissions of {tgt} to {:04o}",
            req.permissions.unwrap_or_default()
        ),
    })
}

fn print_diff(config: &Config, req: &FileRequest, rendered: Option<&str>) -> Result<()> {
    if removes_target(req, rendered) {
        match empty_render_target(req)? {
            EmptyRenderTarget::Absent => {}
            EmptyRenderTarget::Owned => {
                miseprintln!(
                    "  template renders empty: remove {}",
                    req.target.display_user()
                );
                if let Some(current) = current_regular_file_for_diff(req)?
                    && !current.is_empty()
                {
                    print_content_diff(config, req, &current, &[])?;
                }
            }
            // apply refuses these without --force, so they are not removals
            EmptyRenderTarget::Conflict(reason) => miseprintln!(
                "  template renders empty, but {reason}: {} is kept unless applied with --force",
                req.target.display_user()
            ),
        }
        return Ok(());
    }
    match req.mode {
        FileMode::Track => {}
        FileMode::Absent => {
            if req.target.is_symlink() {
                let dest = std::fs::read_link(&req.target)?;
                miseprintln!(
                    "  current symlink: {} -> {}",
                    req.target.display_user(),
                    dest.display_user()
                );
            } else {
                miseprintln!("  current: {} exists", req.target.display_user());
            }
            miseprintln!("  desired: {} absent", req.target.display_user());
        }
        FileMode::Symlink => {
            if req.target.is_symlink() {
                let dest = std::fs::read_link(&req.target)?;
                miseprintln!(
                    "  current symlink: {} -> {}",
                    req.target.display_user(),
                    dest.display_user()
                );
            } else if req.target.exists() {
                miseprintln!("  current: {} exists", req.target.display_user());
            } else {
                miseprintln!("  current: {} missing", req.target.display_user());
            }
            miseprintln!(
                "  desired symlink: {} -> {}",
                req.target.display_user(),
                req.source.display_user()
            );
        }
        FileMode::SymlinkEach => {
            miseprintln!(
                "  desired symlink-each: {} files from {}",
                walk_source_files(req)?.len(),
                req.source.display_user()
            );
            for path in stale_links(req)? {
                miseprintln!("  stale link to remove: {}", path.display_user());
            }
        }
        FileMode::Copy | FileMode::Template if req.source.is_file() => {
            let desired = match req.mode {
                FileMode::Template => rendered.unwrap_or_default().as_bytes().to_vec(),
                _ => file::read(&req.source)?,
            };
            if let Some(current) = current_regular_file_for_diff(req)?
                && current != desired
            {
                print_content_diff(config, req, &current, &desired)?;
            }
            print_permissions_diff(req)?;
        }
        FileMode::Copy | FileMode::Template => {
            miseprintln!(
                "  desired directory contents: {} -> {}",
                req.source.display_user(),
                req.target.display_user()
            );
        }
        FileMode::Content => {
            let desired = req.content.as_deref().expect("inline content").as_bytes();
            if let Some(current) = current_regular_file_for_diff(req)?
                && current != desired
            {
                print_content_diff(config, req, &current, desired)?;
            }
            print_permissions_diff(req)?;
        }
        FileMode::Permissions => match permissions_target_unavailable(req)? {
            Some(reason) => miseprintln!("  current: {reason}"),
            None => print_permissions_diff(req)?,
        },
    }
    Ok(())
}

/// Print the permission change apply would make to an existing regular file
/// or directory (never through a symlink).
fn print_permissions_diff(req: &FileRequest) -> Result<()> {
    #[cfg(unix)]
    if !req.target.is_symlink()
        && (req.target.is_file() || req.mode == FileMode::Permissions && req.target.exists())
        && let Some(desired) = desired_permissions(req)?
    {
        let current = permission_bits(&std::fs::symlink_metadata(&req.target)?);
        if current != desired {
            miseprintln!(
                "  permissions differ: {current:04o} (current) -> {desired:04o} (desired)"
            );
        }
    }
    #[cfg(not(unix))]
    let _ = req;
    Ok(())
}

/// Read a regular target without following a symlink. Non-file targets are
/// described structurally and return no bytes to compare.
fn current_regular_file_for_diff(req: &FileRequest) -> Result<Option<Vec<u8>>> {
    if req.target.is_symlink() {
        let dest = std::fs::read_link(&req.target)?;
        miseprintln!(
            "  file type differs: current symlink {} -> {}; desired regular file",
            req.target.display_user(),
            dest.display_user()
        );
        return Ok(None);
    }
    if req.target.is_file() {
        return match file::read(&req.target) {
            Ok(current) => Ok(Some(current)),
            // a mode such as 0200 can deny even the owner read access
            Err(err) if is_permission_denied(&err) => {
                miseprintln!("  current: {} is not readable", req.target.display_user());
                Ok(None)
            }
            Err(err) => Err(err),
        };
    }
    if req.target.exists() {
        miseprintln!(
            "  file type differs: current directory {}; desired regular file",
            req.target.display_user()
        );
        return Ok(None);
    }
    miseprintln!("  current: {} missing", req.target.display_user());
    Ok(Some(vec![]))
}

fn print_content_diff(
    config: &Config,
    req: &FileRequest,
    current: &[u8],
    desired: &[u8],
) -> Result<()> {
    let source = match req.mode {
        FileMode::Content => "inline".to_string(),
        _ => req.source.display_user(),
    };
    miseprintln!(
        "  content differs: {} -> {}",
        source,
        req.target.display_user()
    );
    let mut opts = diffy::DiffOptions::new();
    opts.set_original_filename(format!("{} (current)", req.target.display_user()))
        .set_modified_filename(match req.mode {
            FileMode::Content => format!("{} (desired)", req.target.display_user()),
            _ => format!("{} (desired)", req.source.display_user()),
        });
    match (str::from_utf8(current), str::from_utf8(desired)) {
        (Ok(current), Ok(desired)) => {
            let current = config.redact(current);
            let desired = config.redact(desired);
            let patch = opts.create_patch(&current, &desired);
            miseprint!("{}", diffy::PatchFormatter::new().fmt_patch(&patch))?;
        }
        _ => miseprintln!("  binary content differs"),
    }
    Ok(())
}

/// Print the changes required to converge whole-file dotfile entries.
/// Templates are rendered because a meaningful diff requires their desired
/// content, matching the trust and execution semantics of dotfiles status.
pub(crate) fn print_diffs(
    config: &Config,
    requests: &[FileRequest],
    secrets: &SecretValues,
) -> Result<()> {
    let mut changed = false;
    let mut problems = vec![];
    for req in requests {
        if req.mode == FileMode::Track {
            continue;
        }
        if req.mode.has_source() && !req.source.exists() {
            miseprintln!("{}: source missing", req.target_raw);
            changed = true;
            continue;
        }
        let rendered = match req.mode {
            FileMode::Template => match render_template(config, req, secrets) {
                Ok(rendered) => Some(rendered),
                Err(err) => {
                    problems.push(format!("  \"{}\": {err}", req.target_raw));
                    continue;
                }
            },
            _ => None,
        };
        match check_rendered(req, rendered.as_deref()) {
            Ok(FileState::Applied) => continue,
            Ok(_) => {}
            Err(err) => {
                problems.push(format!("  \"{}\": {err}", req.target_raw));
                continue;
            }
        }
        changed = true;
        miseprintln!("dotfile differs: {}", req.target.display_user());
        if let Err(err) = print_diff(config, req, rendered.as_deref()) {
            problems.push(format!("  \"{}\": {err}", req.target_raw));
        }
    }
    if !problems.is_empty() {
        bail!(
            "files: cannot diff these entries, fix them manually:\n{}",
            problems.join("\n")
        );
    }
    if !changed {
        info!("files: all files are applied");
    }
    Ok(())
}

const DOTFILES_PART: &str = "dotfiles";

/// Every path `apply_one` may create, replace, or remove for `req`, with how
/// deeply to capture it first: a path that gets replaced is captured whole,
/// a directory that stays a directory only by existence.
fn touched_paths(req: &FileRequest) -> Result<Vec<(PathBuf, Capture)>> {
    let mut paths: IndexMap<PathBuf, Capture> = IndexMap::new();
    for dir in missing_ancestors(&req.target) {
        paths.insert(dir, Capture::Shallow);
    }
    // a directory that will keep being a directory is never walked; a file
    // or link in the way of one is replaced and captured whole
    let dir_capture = |dir: &Path| {
        if dir.is_dir() {
            Capture::Shallow
        } else {
            Capture::Full
        }
    };
    match req.mode {
        FileMode::Track => {}
        // only the mode changes; a directory's contents stay untouched
        FileMode::Permissions => {
            paths.insert(req.target.clone(), dir_capture(&req.target));
        }
        // captured whole, so rollback restores a removed file or link
        FileMode::Absent | FileMode::Symlink | FileMode::Content => {
            paths.insert(req.target.clone(), Capture::Full);
        }
        FileMode::Template => {
            paths.insert(req.target.clone(), Capture::Full);
            // the ownership record changes with the target, so a rollback
            // restores the two together
            paths.insert(target_state_path(req), Capture::Full);
        }
        FileMode::Copy => {
            if req.source.is_dir() {
                paths.insert(req.target.clone(), dir_capture(&req.target));
                for (_, target) in walk_source_files(req)? {
                    // intermediate directories the copy creates on the way
                    // down; an existing one keeps being a directory
                    for dir in missing_ancestors(&target) {
                        paths.entry(dir).or_insert(Capture::Shallow);
                    }
                    paths.insert(target, Capture::Full);
                }
            } else {
                paths.insert(req.target.clone(), Capture::Full);
            }
        }
        FileMode::SymlinkEach => {
            for dir in needed_dirs(req)? {
                let capture = dir_capture(&dir);
                paths.insert(dir, capture);
            }
            for (_, target) in walk_source_files(req)? {
                paths.insert(target, Capture::Full);
            }
            let stale = stale_links(req)?;
            for path in &stale {
                paths.insert(path.clone(), Capture::Full);
            }
            // Journal removed parents after their children, so crash recovery
            // recreates directories (including their modes) before the links.
            for dir in stale
                .iter()
                .flat_map(|path| dirs_between(path, &req.target))
                .sorted_by_key(|path| std::cmp::Reverse(path.components().count()))
                .unique()
            {
                paths.entry(dir).or_insert(Capture::Shallow);
            }
            paths.insert(symlink_each_state_path(req), Capture::Full);
        }
    }
    // Directories a removal empties are journaled by `prune_created_dirs`,
    // which runs after the whole batch.
    if records_created_dirs(req) {
        // the record gains the directories created on the way: the only
        // shallow paths of a single-file entry are the missing ancestors
        if paths.values().any(|capture| *capture == Capture::Shallow) {
            paths.entry(target_state_path(req)).or_insert(Capture::Full);
        }
    }
    Ok(paths.into_iter().collect())
}

/// Directories strictly between `path` and `root`, deepest first: the ones an
/// upward cleanup may remove once they are empty.
fn dirs_between(path: &Path, root: &Path) -> Vec<PathBuf> {
    let mut dirs = vec![];
    let mut dir = path.parent();
    while let Some(d) = dir {
        if d == root || !d.starts_with(root) {
            break;
        }
        dirs.push(d.to_path_buf());
        dir = d.parent();
    }
    dirs
}

/// Remove each of `starts` and then its parents while `may_remove` allows it
/// and the directory is empty, stopping each walk at the first directory that
/// stays. Deepest starts go first, so emptying a nested directory can empty
/// its parent too. Returns the removed directories.
fn remove_empty_dirs_upward<'a>(
    starts: impl IntoIterator<Item = &'a Path>,
    may_remove: impl Fn(&Path) -> bool,
) -> Result<Vec<PathBuf>> {
    let mut removed = vec![];
    for start in starts
        .into_iter()
        .sorted_by_key(|dir| std::cmp::Reverse(dir.components().count()))
        .unique()
    {
        let mut dir = Some(start);
        while let Some(d) = dir {
            if !may_remove(d) || !d.is_dir() || d.read_dir()?.next().is_some() {
                break;
            }
            debug!("files: removing empty directory {}", d.display_user());
            file::remove_dir(d)?;
            removed.push(d.to_path_buf());
            dir = d.parent();
        }
    }
    Ok(removed)
}

/// Ancestors of `path` that do not exist yet, outermost first.
pub(crate) fn missing_ancestors(path: &Path) -> Vec<PathBuf> {
    let mut missing = vec![];
    let mut dir = path.parent();
    while let Some(d) = dir {
        if d.exists() || d.as_os_str().is_empty() {
            break;
        }
        missing.push(d.to_path_buf());
        dir = d.parent();
    }
    missing.reverse();
    missing
}

/// Write one entry. Each path is appended to `written` at the point it is
/// first mutated — after the removal of what was there, or else after its
/// own write lands — so a caller sees exactly the files that changed when a
/// later write fails: the target of a whole-file entry, each file a
/// directory copy or symlink-each places, anything cleared to make room, and
/// each stale link symlink-each prunes. A symlink to a directory also lists
/// the files it exposes, so a `[history.reload]` glob under the target
/// matches. Directories created on the way are not listed.
fn apply_one(req: &FileRequest, rendered: Option<&str>, written: &mut Vec<PathBuf>) -> Result<()> {
    if removes_target(req, rendered) {
        // conflicts were vetted (or --force given) when the plan was made
        debug!(
            "files: rm {} (template rendered empty)",
            req.target.display_user()
        );
        if remove_existing(&req.target)? {
            written.push(req.target.clone());
        }
        // The record keeps what mise last wrote, so a file brought back by
        // `mise dot undo` is still recognised as mise's own.
        return Ok(());
    }
    debug!("files: {}", describe(req)?);
    if req.mode == FileMode::Absent {
        // checked again here, not only when planning: a directory that
        // appeared since must still never be removed
        if check_absent(&req.target)? != FileState::Applied {
            if file::is_symlink_or_junction(&req.target) {
                // removes the link itself by handle; a Windows directory
                // link needs this (`remove_file` refuses it), and the
                // directory it points to is never entered
                file::remove_symlink_or_junction(&req.target)?;
            } else {
                file::remove_file(&req.target)?;
            }
            written.push(req.target.clone());
        }
        return Ok(());
    }
    // what is missing now is what mise creates, and so may remove again
    let created_dirs = if records_created_dirs(req) {
        missing_ancestors(&req.target)
    } else {
        vec![]
    };
    if req.mode != FileMode::Permissions
        && let Some(parent) = req.target.parent()
    {
        file::create_dir_all(parent)?;
    }
    match req.mode {
        FileMode::Symlink => {
            replace_recorded(&req.target, written, || {
                link_path(&req.source, &req.target, req.relative, true)
            })?;
            // the link is in place; listing what it exposes only feeds
            // reload matching, so a walk that fails must not fail the apply
            if req.source.is_dir() {
                match walk_source_files(req) {
                    Ok(files) => written.extend(files.into_iter().map(|(_, target)| target)),
                    Err(err) => warn!(
                        "files: cannot list {} for reload matching: {err:#}",
                        req.source.display_user()
                    ),
                }
            }
        }
        FileMode::SymlinkEach => {
            // conflicts were vetted (or --force given): clear anything
            // blocking a directory we need
            for dir in needed_dirs(req)? {
                if dir.exists() && !dir.is_dir() && remove_existing(&dir)? {
                    written.push(dir);
                }
            }
            // even an empty source dir must produce the target dir, or the
            // entry would never converge
            file::create_dir_all(&req.target)?;
            for (source, target) in walk_source_files(req)? {
                if check_symlink(&source, &target, req.relative)? == FileState::Applied {
                    continue;
                }
                if let Some(parent) = target.parent() {
                    file::create_dir_all(parent)?;
                }
                replace_recorded(&target, written, || {
                    link_path(&source, &target, req.relative, false)
                })?;
            }
            prune_stale_links(req, written)?;
        }
        FileMode::Copy => {
            if req.source.is_dir() {
                // additive: overwrite matching files, leave files mise
                // doesn't manage in place — only a type mismatch (vetted
                // as a conflict) removes the target
                if req.target.exists() && !req.target.is_dir() && remove_existing(&req.target)? {
                    written.push(req.target.clone());
                }
                // even an empty source dir must produce the target dir,
                // or the entry would never converge
                file::create_dir_all(&req.target)?;
                // per-file instead of a directory copy so a symlink at a
                // destination is replaced, not written through
                for (source, target) in walk_source_files(req)? {
                    if let Some(parent) = target.parent() {
                        file::create_dir_all(parent)?;
                    }
                    if target.is_symlink() {
                        // a link is replaced: recorded once it is gone. The
                        // source is opened and checked first and the copy
                        // reads from that handle, so one that cannot be
                        // copied leaves the link alone and one that changes
                        // meanwhile cannot leave the target missing
                        let mut from = CopySource::open(&source, &target)?;
                        file::remove_file(&target)?;
                        written.push(target.clone());
                        let to = std::fs::File::create(&target)
                            .wrap_err_with(copy_failure(&source, &target))?;
                        from.copy_into(to)?;
                    } else if target.is_file() {
                        overwrite_recorded(&source, &target, written)?;
                    } else {
                        match std::fs::symlink_metadata(&target) {
                            // absent: recorded once it exists, even if the
                            // copy that created it then failed
                            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                                create_recorded(&target, written, || file::copy(&source, &target))?;
                            }
                            // something else is there (a directory, say):
                            // a copy that fails leaves it as it was, so it
                            // is recorded only once the copy succeeded
                            Ok(_) => {
                                file::copy(&source, &target)?;
                                written.push(target.clone());
                            }
                            Err(err) => return Err(err.into()),
                        }
                    }
                }
            } else {
                replace_recorded(&req.target, written, || {
                    file::copy(&req.source, &req.target)
                })?;
                // the copy took the source's permissions; an explicit
                // `permissions` overrides them
                #[cfg(unix)]
                if let Some(permissions) = req.permissions {
                    set_mode(&req.target, permissions)?;
                }
            }
        }
        FileMode::Template => {
            let rendered = rendered.expect("rendered template content");
            replace_recorded(&req.target, written, || file::write(&req.target, rendered))?;
            #[cfg(unix)]
            match req.permissions {
                Some(permissions) => set_mode(&req.target, permissions)?,
                None => {
                    std::fs::set_permissions(&req.target, req.source.metadata()?.permissions())?
                }
            }
            save_target_state(req, rendered);
        }
        FileMode::Track => unreachable!("tracked files are never written"),
        FileMode::Absent => unreachable!("absent targets are removed above"),
        FileMode::Content => {
            replace_recorded(&req.target, written, || {
                file::write(&req.target, req.content.as_deref().expect("inline content"))
            })?;
            #[cfg(unix)]
            set_mode(&req.target, req.permissions.unwrap_or(0o600))?;
        }
        FileMode::Permissions => {
            // planning skipped a missing target or a symlink; this check only
            // gives the clearer message, the chmod itself never follows a
            // link that appeared since
            if let Some(reason) = permissions_target_unavailable(req)? {
                bail!("[dotfiles].\"{}\": {reason}", req.target_raw);
            }
            #[cfg(unix)]
            if let Some(permissions) = req.permissions {
                chmod_no_follow(&req.target, permissions)
                    .wrap_err_with(|| format!("[dotfiles].\"{}\"", req.target_raw))?;
                written.push(req.target.clone());
            }
        }
    }
    record_created_dirs(req, &created_dirs);
    Ok(())
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))?;
    Ok(())
}

/// Set the mode of a file mise does not own without ever following a symlink
/// at `path`: the check and the chmod act on one descriptor, so a link
/// swapped in after planning is refused instead of redirecting the change.
#[cfg(unix)]
fn chmod_no_follow(path: &Path, mode: u32) -> Result<()> {
    use nix::errno::Errno;
    use nix::fcntl::{OFlag, open};
    use nix::sys::stat::{Mode, fchmod};

    let mode = Mode::from_bits_truncate(mode as nix::libc::mode_t);
    let flags = OFlag::O_NOFOLLOW | OFlag::O_NONBLOCK | OFlag::O_CLOEXEC;
    // a write-only file cannot be opened for reading, nor a directory for
    // writing; either descriptor is enough for fchmod
    for access in [OFlag::O_RDONLY, OFlag::O_WRONLY] {
        match open(path, flags | access, Mode::empty()) {
            Ok(fd) => {
                fchmod(&fd, mode).wrap_err_with(|| {
                    format!("failed to set permissions of {}", path.display_user())
                })?;
                return Ok(());
            }
            Err(Errno::EACCES | Errno::EISDIR) => continue,
            Err(Errno::ELOOP) => bail!(
                "{} is a symlink, which is never followed",
                path.display_user()
            ),
            Err(err) => {
                return Err(err)
                    .wrap_err_with(|| format!("failed to open {}", path.display_user()));
            }
        }
    }
    // a target its owner can neither read nor write (mode 0000) cannot be
    // opened for either, but its owner may still change its mode
    chmod_unopenable_no_follow(path, mode)
}

/// Linux: an `O_PATH` descriptor needs no read or write permission, and with
/// `O_NOFOLLOW` it refers to a final symlink itself, which the type check
/// then refuses. Linux has no `fchmod` for such a descriptor, so the change
/// goes through its `/proc/self/fd` entry, which resolves to the opened inode
/// rather than to the path again. (`fchmodat` with `AT_SYMLINK_NOFOLLOW`
/// fails with `ENOTSUP` on older kernels and C libraries.)
#[cfg(target_os = "linux")]
fn chmod_unopenable_no_follow(path: &Path, mode: nix::sys::stat::Mode) -> Result<()> {
    use nix::fcntl::{AT_FDCWD, OFlag, open};
    use nix::sys::stat::{FchmodatFlags, Mode, SFlag, fchmodat, fstat};
    use std::os::fd::AsRawFd;

    let fd = open(
        path,
        OFlag::O_PATH | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
        Mode::empty(),
    )
    .wrap_err_with(|| format!("failed to open {}", path.display_user()))?;
    let kind = SFlag::from_bits_truncate(fstat(&fd)?.st_mode) & SFlag::S_IFMT;
    if kind == SFlag::S_IFLNK {
        bail!(
            "{} is a symlink, which is never followed",
            path.display_user()
        );
    }
    if kind != SFlag::S_IFREG && kind != SFlag::S_IFDIR {
        bail!("{} is not a file or directory", path.display_user());
    }
    let descriptor = PathBuf::from(format!("/proc/self/fd/{}", fd.as_raw_fd()));
    fchmodat(AT_FDCWD, &descriptor, mode, FchmodatFlags::FollowSymlink)
        .wrap_err_with(|| format!("failed to set permissions of {}", path.display_user()))
}

/// Other Unix systems (macOS, the BSDs) implement `fchmodat` with
/// `AT_SYMLINK_NOFOLLOW` directly. There it changes a symlink's own mode
/// rather than failing, so a link is refused first; one swapped in after that
/// check only has its own mode changed, never its target's.
#[cfg(all(unix, not(target_os = "linux")))]
fn chmod_unopenable_no_follow(path: &Path, mode: nix::sys::stat::Mode) -> Result<()> {
    use nix::errno::Errno;
    use nix::fcntl::AT_FDCWD;
    use nix::sys::stat::{FchmodatFlags, fchmodat};

    let file_type = std::fs::symlink_metadata(path)
        .wrap_err_with(|| format!("failed to inspect {}", path.display_user()))?
        .file_type();
    if file_type.is_symlink() {
        bail!(
            "{} is a symlink, which is never followed",
            path.display_user()
        );
    }
    if !file_type.is_file() && !file_type.is_dir() {
        bail!("{} is not a file or directory", path.display_user());
    }
    match fchmodat(AT_FDCWD, path, mode, FchmodatFlags::NoFollowSymlink) {
        Ok(()) => Ok(()),
        // one errno on some systems, two on others
        Err(err) if err == Errno::ENOTSUP || err == Errno::EOPNOTSUPP => bail!(
            "cannot set permissions of {} without following symlinks on this system; make it readable or writable by its owner first",
            path.display_user()
        ),
        Err(err) => Err(err)
            .wrap_err_with(|| format!("failed to set permissions of {}", path.display_user())),
    }
}

/// delete this entry's leftover links (see [`stale_links`]) and any directory
/// they emptied out. A directory only goes when the links we just removed were
/// all that was in it and the entry has no source file left that needs it, so
/// user content — and the target directory itself — always survives.
fn prune_stale_links(req: &FileRequest, written: &mut Vec<PathBuf>) -> Result<()> {
    let stale = stale_links(req)?;
    if stale.is_empty() {
        return Ok(());
    }
    for path in &stale {
        debug!("files: removing stale link {}", path.display_user());
        file::remove_file(path)?;
        written.push(path.clone());
    }
    let needed: HashSet<PathBuf> = needed_dirs(req)?.into_iter().collect();
    remove_empty_dirs_upward(stale.iter().filter_map(|p| p.parent()), |dir| {
        dir != req.target && dir.starts_with(&req.target) && !needed.contains(dir)
    })?;
    Ok(())
}

/// remove whatever sits at `path` so it can be replaced — conflicts have
/// already been vetted (or --force given) by the time this runs
fn remove_existing(path: &Path) -> Result<bool> {
    if path.is_symlink() || path.is_file() {
        file::remove_file(path)?;
    } else if path.is_dir() {
        file::remove_all(path)?;
    } else {
        return Ok(false);
    }
    Ok(true)
}

/// A copy source held open, so the destination is touched only once the
/// source has been opened and checked, and the copy reads from that same
/// handle — a source replaced or removed meanwhile cannot leave the
/// destination cleared with nothing to put in its place.
struct CopySource<'a> {
    file: std::fs::File,
    metadata: std::fs::Metadata,
    source: &'a Path,
    target: &'a Path,
}

impl<'a> CopySource<'a> {
    /// Open `source` for copying to `target`, with the check `fs::copy`
    /// performs before it touches its destination: the source must be a
    /// regular file (or a link to one).
    fn open(source: &'a Path, target: &'a Path) -> Result<Self> {
        let failed = copy_failure(source, target);
        let file = std::fs::File::open(source).wrap_err_with(&failed)?;
        let metadata = file.metadata().wrap_err_with(&failed)?;
        if !metadata.is_file() {
            bail!(
                "{}: the source path is neither a regular file nor a symlink to a regular file",
                failed()
            );
        }
        Ok(Self {
            file,
            metadata,
            source,
            target,
        })
    }

    /// Write the source's content and permission bits into the open `to`,
    /// as `fs::copy` would.
    fn copy_into(&mut self, mut to: std::fs::File) -> Result<()> {
        let failed = copy_failure(self.source, self.target);
        std::io::copy(&mut self.file, &mut to).wrap_err_with(&failed)?;
        to.set_permissions(self.metadata.permissions())
            .wrap_err_with(&failed)?;
        Ok(())
    }
}

fn copy_failure<'a>(source: &'a Path, target: &'a Path) -> impl Fn() -> String + 'a {
    move || {
        format!(
            "failed copy: {} -> {}",
            source.display_user(),
            target.display_user()
        )
    }
}

/// Overwrite the existing regular file at `target` with `source` in place,
/// as `file::copy` would (content and permission bits), recording the target
/// in `written` once it has been opened for truncation — the first mutation.
/// The source is opened and checked first, so a source that is not a regular
/// file (a directory behind a link, say) fails before the target is touched.
/// An open that fails (a read-only target file or filesystem) changes
/// nothing and records nothing.
fn overwrite_recorded(source: &Path, target: &Path, written: &mut Vec<PathBuf>) -> Result<()> {
    let mut from = CopySource::open(source, target)?;
    let to = std::fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(target)
        .wrap_err_with(copy_failure(source, target))?;
    written.push(target.to_path_buf());
    from.copy_into(to)
}

/// Run `write` against a `target` that does not exist yet, recording the
/// target in `written` if it exists afterwards: after a successful write,
/// and after one that failed only once it had created the file (its on-disk
/// state changed either way). A write that failed before creating anything
/// records nothing.
pub(crate) fn create_recorded(
    target: &Path,
    written: &mut Vec<PathBuf>,
    write: impl FnOnce() -> Result<()>,
) -> Result<()> {
    let result = write();
    if result.is_ok() || std::fs::symlink_metadata(target).is_ok() {
        written.push(target.to_path_buf());
    }
    result
}

/// Clear `target` and run `write` in its place, recording the target in
/// `written` at its first mutation: right after the removal when something
/// was there (a write that then fails still leaves the old content gone),
/// otherwise as [`create_recorded`] does.
fn replace_recorded(
    target: &Path,
    written: &mut Vec<PathBuf>,
    write: impl FnOnce() -> Result<()>,
) -> Result<()> {
    if remove_existing(target)? {
        written.push(target.to_path_buf());
        return write();
    }
    create_recorded(target, written, write)
}

/// `allow_windows_symlink` is false for `symlink-each`, which stays on the Windows copy path:
/// its unapply planner is `#[cfg(not(windows))]`-guarded and falls through to the content
/// comparison, which rejects a symlink — creating one there would make unapply demand `--force`.
fn link_path(
    source: &Path,
    target: &Path,
    relative: bool,
    allow_windows_symlink: bool,
) -> Result<()> {
    #[cfg(windows)]
    if source.is_file() {
        // Windows grants SYMBOLIC_LINK_FLAG_ALLOW_UNPRIVILEGED_CREATE when Developer Mode is
        // on, so a file symlink is often creatable without elevation -- `windows_shim_mode`
        // already relies on that. Try it rather than assume it fails. Junctions only cover
        // directories, so a copy stays the fallback: it is what this did unconditionally
        // before, and it keeps working where the privilege really is absent.
        if allow_windows_symlink && std::os::windows::fs::symlink_file(source, target).is_ok() {
            return Ok(());
        }
        file::copy(source, target)?;
        return Ok(());
    }
    #[cfg(not(windows))]
    let _ = allow_windows_symlink;
    if relative {
        file::make_symlink(&relative_link_path(source, target), target)?;
    } else {
        file::make_symlink(source, target)?;
    }
    Ok(())
}

/// The path a link at `target` should hold to reach `source` relatively.
///
/// The kernel resolves `..` in a link against the directory the link
/// physically sits in, so a path worked out from the configured spelling is
/// only used when it resolves to the source from there (it does not when
/// the link's directory is itself reached through a symlink). Otherwise the
/// path runs between the canonical locations, and if even that cannot be
/// worked out the link stays absolute.
fn relative_link_path(source: &Path, target: &Path) -> PathBuf {
    let Some(parent) = target.parent() else {
        return source.to_path_buf();
    };
    // `physical_path` rather than `canonicalize`: a `symlink-each` source may
    // be a dangling link, which must still get a relative link or the entry
    // would never converge
    let physical_source = physical_path(source);
    let source = lexical_normalize(source);
    let resolves =
        |rel: &Path| resolve_relative_link(target, rel).is_some_and(|p| p == physical_source);
    if let Some(rel) = pathdiff::diff_paths(&source, lexical_normalize(parent))
        && resolves(&rel)
    {
        return rel;
    }
    if let Ok(canonical_parent) = parent.canonicalize()
        && let Some(rel) = pathdiff::diff_paths(&physical_source, canonical_parent)
        && resolves(&rel)
    {
        return rel;
    }
    source
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn destination_variants_survive_incoming_config_preflight() -> Result<()> {
        use crate::config::config_file::mise_toml::MiseToml;
        use std::sync::Arc;

        let path = dirs::HOME.join(".config/mise/config.toml");
        let body = r#"
[dotfiles."vscode/settings.json"]
mode = "copy"
variants = [
    { os = "macos", target = "~/Library/Application Support/Code/User/settings.json" },
    { os = "linux", profile = "work", target = "/etc/example/settings.json" },
    { os = "windows", target = 'C:\Users\example\settings.json' },
]
"#;
        let mut configs = ConfigMap::new();
        configs.insert(
            path.clone(),
            Arc::new(MiseToml::for_history_preflight(body, &path)?),
        );
        validate_incoming_files(&configs)?;

        // Validate inactive destinations too, before accepting shared config.
        let invalid = body.replace(
            "~/Library/Application Support/Code/User/settings.json",
            "relative/settings.json",
        );
        configs.insert(
            path.clone(),
            Arc::new(MiseToml::for_history_preflight(&invalid, &path)?),
        );
        assert!(validate_incoming_files(&configs).is_err());
        Ok(())
    }

    #[test]
    fn destination_variants_infer_safe_root_relative_sources() -> Result<()> {
        let entry: FileTomlEntry = toml::from_str(
            r#"
mode = "copy"
variants = [
    { os = "macos", target = "~/Library/Application Support/Code/User/settings.json" },
    { os = "linux", target = "~/.config/Code/User/settings.json" },
]
"#,
        )?;
        let FileTomlEntry::Table {
            source,
            content,
            mode,
            variants: Some(variants),
            ..
        } = entry
        else {
            bail!("expected a table entry with variants");
        };
        assert_eq!(
            validate_file_variants(
                "vscode/settings.json",
                source.as_deref(),
                content.as_deref(),
                mode.as_deref(),
                false,
                &variants,
            )?,
            Some(PathBuf::from("vscode/settings.json"))
        );
        for key in [
            "",
            ".",
            "../settings.json",
            "vscode/../settings.json",
            "~/.settings.json",
        ] {
            assert!(logical_source_path(key).is_err(), "{key}");
        }
        Ok(())
    }

    #[test]
    fn destination_variants_reject_unknown_fields_in_all_modes() {
        for mode in ["copy", "track"] {
            for field in ["oss", "profle", "targte"] {
                let input = format!(
                    r#"mode = "{mode}"
variants = [{{ {field} = "linux" }}]"#
                );
                assert!(toml::from_str::<FileTomlEntry>(&input).is_err(), "{input}");
            }
        }
    }

    #[test]
    fn destination_variant_paths_accept_foreign_absolute_syntax() {
        for target in [
            "~/settings.json",
            "/etc/example/settings.json",
            "C:/Users/example/settings.json",
            r"C:\Users\example\settings.json",
            r"\\server\share\settings.json",
        ] {
            assert!(variant_target_is_absolute(target), "{target}");
        }
        for target in ["settings.json", "./settings.json", "C:settings.json", ""] {
            assert!(!variant_target_is_absolute(target), "{target}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn stale_link_parent_directories_have_recovery_preimages() -> Result<()> {
        use crate::system::history::journal::{JournalEntry, PathSnapshot, PathState};
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir()?;
        let root = temp.path().canonicalize()?;
        let source = root.join("source");
        let target = root.join("target");
        std::fs::create_dir_all(&source)?;
        let nested = target.join("nested");
        std::fs::create_dir_all(&nested)?;
        std::fs::set_permissions(&nested, std::fs::Permissions::from_mode(0o700))?;
        for name in ["a", "b"] {
            std::os::unix::fs::symlink(source.join("nested").join(name), nested.join(name))?;
        }
        let req = link_req(&source, &target, FileMode::SymlinkEach);
        let state = temp.path().join("recovery");
        let mut journal = touched_paths(&req)?
            .into_iter()
            .map(|(path, capture)| {
                let prior = PathSnapshot::capture_with(&state, &path, capture);
                JournalEntry::PathChanged {
                    part: "dotfiles".into(),
                    item: "test".into(),
                    path,
                    prior,
                }
            })
            .collect::<Vec<_>>();
        let mut written = vec![];
        prune_stale_links(&req, &mut written)?;
        assert!(!nested.exists());
        // the removed links are listed, the pruned directory is not
        assert_eq!(written, vec![nested.join("a"), nested.join("b")]);
        let committed = journal
            .iter()
            .enumerate()
            .filter_map(|(seq, entry)| {
                let JournalEntry::PathChanged { path, .. } = entry else {
                    return None;
                };
                Some(JournalEntry::Committed {
                    seq: seq as u32,
                    after: PathState::observe(path),
                })
            })
            .collect::<Vec<_>>();
        journal.extend(committed);
        crate::system::history::recovery::recover(&state, &journal)?;
        assert_eq!(
            std::fs::metadata(&nested)?.permissions().mode() & 0o777,
            0o700
        );
        for name in ["a", "b"] {
            assert_eq!(
                std::fs::read_link(nested.join(name))?,
                source.join("nested").join(name)
            );
        }
        Ok(())
    }

    #[test]
    fn configuration_reload_discards_old_declaration_diagnostics() {
        let target = "~/.mise-diagnostic-reset-test";
        record_invalid(target, Path::new("diagnostic-reset.toml"), "invalid mode");
        assert!(
            invalid_declarations()
                .iter()
                .any(|item| item.target == target)
        );
        clear_invalid_declarations();
        assert!(
            !invalid_declarations()
                .iter()
                .any(|item| item.target == target)
        );
    }

    #[test]
    fn test_file_mode_parse() {
        assert_eq!(FileMode::parse("symlink"), Some(FileMode::Symlink));
        assert_eq!(FileMode::parse("symlink-each"), Some(FileMode::SymlinkEach));
        assert_eq!(FileMode::parse("copy"), Some(FileMode::Copy));
        assert_eq!(FileMode::parse("template"), Some(FileMode::Template));
        assert_eq!(FileMode::parse("absent"), Some(FileMode::Absent));
        assert_eq!(FileMode::Absent.name(), "absent");
        assert!(!FileMode::Absent.has_source());
        assert_eq!(FileMode::parse("hardlink"), None);
    }

    fn absent_req(target: &Path) -> FileRequest {
        FileRequest {
            target_raw: target.to_string_lossy().to_string(),
            target: target.to_path_buf(),
            source: PathBuf::new(),
            content: None,
            mode: FileMode::Absent,
            exclude: vec![],
            include: None,
            manifest: None,
            permissions: None,
            base: PathBuf::from("/"),
            origin: ResourceOrigin {
                config: PathBuf::from("/mise.toml"),
                config_root: PathBuf::from("/"),
                environment: vec![],
                source: None,
            },
            policy: FilePolicy::for_mode(FileMode::Absent),
            variants: vec![],
            enabled: true,
            remove_empty: false,
            dot_prefix: false,
            relative: false,
        }
    }

    fn validate_incoming_body(body: &str) -> Result<()> {
        use crate::config::config_file::mise_toml::MiseToml;
        use std::sync::Arc;

        let path = dirs::HOME.join(".config/mise/config.toml");
        let mut configs = ConfigMap::new();
        configs.insert(
            path.clone(),
            Arc::new(MiseToml::for_history_preflight(body, &path)?),
        );
        validate_incoming_files(&configs)
    }

    #[test]
    fn absent_entries_take_no_source() -> Result<()> {
        validate_incoming_body(
            r#"
[dotfiles]
"~/.oldrc" = { mode = "absent" }
"#,
        )?;
        for extra in [
            r#"source = "oldrc""#,
            r#"content = "x""#,
            r#"manifest = "git""#,
            r#"exclude = ["*.bak"]"#,
            r#"permissions = "0600""#,
            "encrypt = true",
        ] {
            let body = format!(
                r#"
[dotfiles."~/.oldrc"]
mode = "absent"
{extra}
"#
            );
            assert!(validate_incoming_body(&body).is_err(), "{extra}");
        }
        Ok(())
    }

    /// A destination override needs no source when the entry removes it.
    #[test]
    fn absent_entries_accept_destination_variants() -> Result<()> {
        validate_incoming_body(
            r#"
[dotfiles."~/.oldrc"]
mode = "absent"
variants = [
    { os = "windows", target = 'C:\Users\example\oldrc' },
    { os = ["linux", "macos"] },
]
"#,
        )
    }

    #[test]
    fn merged_absent_entries_have_no_source() -> Result<()> {
        let origin = absent_req(Path::new("/unused")).origin;
        let mut merged = IndexMap::new();
        let entry: FileTomlEntry = toml::from_str(r#"mode = "absent""#)?;
        merge_file_entry(
            "~/.oldrc".into(),
            entry,
            Path::new("/"),
            &origin,
            &mut merged,
        );
        let [request] = merged.values().collect::<Vec<_>>()[..] else {
            bail!("expected one absent request");
        };
        assert_eq!(request.mode, FileMode::Absent);
        assert_eq!(request.target, dirs::HOME.join(".oldrc"));
        assert_eq!(request.source, PathBuf::new());

        // a pattern is rejected rather than checked as a literal path
        assert!(
            validate_incoming_body(
                r#"
[dotfiles]
"~/.old*" = { mode = "absent" }
"#
            )
            .is_err()
        );
        // so is one a variant selects as its destination
        let err = validate_incoming_body(
            r#"
[dotfiles."~/.oldrc"]
mode = "absent"
variants = [{ os = "windows", target = "~/.old[0-9]" }, { default = true }]
"#,
        )
        .unwrap_err();
        assert!(err.to_string().contains("cannot use wildcards"), "{err}");
        let mut merged = IndexMap::new();
        let entry: FileTomlEntry = toml::from_str(r#"mode = "absent""#)?;
        merge_file_entry(
            "~/.old*".into(),
            entry,
            Path::new("/"),
            &origin,
            &mut merged,
        );
        assert!(merged.is_empty());

        let mut merged = IndexMap::new();
        let entry: FileTomlEntry = toml::from_str(
            r#"mode = "absent"
source = "oldrc""#,
        )?;
        merge_file_entry(
            "~/.oldrc".into(),
            entry,
            Path::new("/"),
            &origin,
            &mut merged,
        );
        assert!(merged.is_empty());
        Ok(())
    }

    #[test]
    fn absent_claims_its_target_in_the_footprint() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let source = dir.path().join("source");
        let target = dir.path().join("target");
        file::write(&source, "content")?;

        let err = validate_composed_file_footprints(&[
            absent_req(&target),
            link_req(&source, &target, FileMode::Copy),
        ])
        .unwrap_err();
        assert!(err.to_string().contains("conflicting dotfile declarations"));
        // a permissions-only entry cannot chmod a file an absent entry removes
        let mut permissions_only = absent_req(&target);
        permissions_only.mode = FileMode::Permissions;
        permissions_only.permissions = Some(0o600);
        let err = validate_composed_file_footprints(&[absent_req(&target), permissions_only])
            .unwrap_err();
        assert!(err.to_string().contains("conflicting dotfile declarations"));
        // a file beneath an absent target would need it as a directory
        let err = validate_composed_file_footprints(&[
            absent_req(&target),
            link_req(&source, &target.join("nested"), FileMode::Copy),
        ])
        .unwrap_err();
        assert!(err.to_string().contains("conflicting dotfile declarations"));
        Ok(())
    }

    #[test]
    fn absent_removes_a_file_without_a_content_check() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let target = dir.path().join("oldrc");
        let req = absent_req(&target);
        assert_eq!(check_rendered(&req, None)?, FileState::Applied);

        file::write(&target, "anything")?;
        assert_eq!(
            check_rendered(&req, None)?,
            FileState::Differs("present".into())
        );
        assert!(find_conflicts(&req)?.is_empty());
        assert_eq!(touched_paths(&req)?, vec![(target.clone(), Capture::Full)]);

        let mut written = vec![];
        apply_one(&req, None, &mut written)?;
        assert!(!target.exists());
        assert_eq!(written, vec![target.clone()]);
        assert_eq!(check_rendered(&req, None)?, FileState::Applied);

        // converged: nothing is removed or recorded
        let mut written = vec![];
        apply_one(&req, None, &mut written)?;
        assert!(written.is_empty());
        Ok(())
    }

    /// Not `#[cfg(unix)]`: `make_symlink` writes a junction on Windows, a
    /// directory link that must be removed as a link, not refused as a
    /// directory, and never entered.
    #[test]
    fn absent_removes_a_symlink_but_not_what_it_points_at() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let pointee = dir.path().join("pointee");
        let target = dir.path().join("link");
        file::create_dir_all(&pointee)?;
        file::write(pointee.join("keep"), "keep")?;
        file::make_symlink(&pointee, &target)?;
        let req = absent_req(&target);
        assert!(matches!(check_rendered(&req, None)?, FileState::Differs(_)));

        // the journal captures the link itself (std reports a junction as a
        // symlink), never walking into the directory it points to; undo
        // restores it with `make_symlink`, which writes a junction again
        let state = tempfile::tempdir()?;
        for (path, capture) in touched_paths(&req)? {
            let snapshot = journal::PathSnapshot::capture_with(state.path(), &path, capture);
            assert!(
                matches!(snapshot, journal::PathSnapshot::Symlink { .. }),
                "{}",
                snapshot.describe()
            );
        }

        let mut written = vec![];
        apply_one(&req, None, &mut written)?;
        assert!(!target.is_symlink());
        assert!(pointee.join("keep").is_file());
        Ok(())
    }

    #[test]
    fn absent_under_a_file_is_already_applied() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let parent = dir.path().join("oldrc");
        file::write(&parent, "a file, not a directory")?;
        let req = absent_req(&parent.join("x"));
        assert_eq!(check_rendered(&req, None)?, FileState::Applied);
        let mut written = vec![];
        apply_one(&req, None, &mut written)?;
        assert!(written.is_empty());
        assert!(parent.is_file());
        Ok(())
    }

    #[test]
    fn absent_refuses_a_directory() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let target = dir.path().join("olddir");
        file::create_dir_all(&target)?;
        file::write(target.join("keep"), "keep")?;
        let req = absent_req(&target);

        let err = check_rendered(&req, None).unwrap_err();
        assert!(err.to_string().contains("is a directory"), "{err}");
        let mut written = vec![];
        assert!(apply_one(&req, None, &mut written).is_err());
        assert!(written.is_empty());
        assert!(target.join("keep").is_file());
        Ok(())
    }

    /// Deserializing a whole-file entry would drop the edit keys, turning an
    /// edit into a removal of the file it meant to edit.
    #[test]
    fn absent_entries_reject_edit_keys() {
        for extra in [
            r#"block = "x""#,
            r#"line = "x""#,
            r#"template = "tera""#,
            r#"comment = ";""#,
            r#"position = "prepend""#,
        ] {
            let value: toml::Value =
                toml::from_str(&format!("mode = \"absent\"\n{extra}")).expect("toml");
            assert!(
                parse_file_entry("~/.oldrc", value, Path::new("/mise.toml")).is_none(),
                "{extra}"
            );
        }
    }

    /// Only regular files and symlinks are removed: a socket (like a FIFO or
    /// device node) may belong to a running service.
    #[cfg(unix)]
    #[test]
    fn absent_refuses_special_files() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let target = dir.path().join("socket");
        let _listener = std::os::unix::net::UnixListener::bind(&target)?;
        let req = absent_req(&target);

        let err = check_rendered(&req, None).unwrap_err();
        assert!(
            err.to_string().contains("not a regular file or symlink"),
            "{err}"
        );
        let mut written = vec![];
        assert!(apply_one(&req, None, &mut written).is_err());
        assert!(written.is_empty());
        assert!(std::fs::symlink_metadata(&target).is_ok());
        Ok(())
    }

    #[test]
    fn absent_rejects_an_edit_on_the_same_file() -> Result<()> {
        use crate::system::edits::{EditOp, EditRequest, LinePosition};
        let target = dirs::HOME.join(".oldrc");
        let origin = absent_req(&target).origin;
        let edit = |path: &Path| EditRequest {
            path_raw: path.display().to_string(),
            path: path.to_path_buf(),
            id: "x".into(),
            op: EditOp::Line {
                line: "x".into(),
                position: LinePosition::Append,
            },
            base: PathBuf::from("/"),
            config_path: PathBuf::from("/mise.toml"),
            origin: origin.clone(),
        };
        let err =
            validate_absent_edit_targets(&[absent_req(&target)], &[edit(&target)]).unwrap_err();
        assert!(err.to_string().contains("conflicting dotfile declarations"));
        // another spelling of the same path
        let dotted = dirs::HOME.join(".dir").join("..").join(".oldrc");
        assert!(validate_absent_edit_targets(&[absent_req(&target)], &[edit(&dotted)]).is_err());
        validate_absent_edit_targets(&[absent_req(&target)], &[edit(&target.with_extension("x"))])?;
        // a whole-file entry of another mode may still be edited
        let copy = link_req(&target, &target, FileMode::Copy);
        validate_absent_edit_targets(&[copy], &[edit(&target)])?;
        Ok(())
    }

    #[test]
    fn absent_has_nothing_to_unapply() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let target = dir.path().join("oldrc");
        file::write(&target, "put back by hand")?;
        let req = absent_req(&target);
        let opts = UnapplyOpts {
            dry_run: false,
            verbose: false,
            force: true,
            yes: true,
        };
        assert!(plan_unapply_one(&req, &opts)?.is_none());
        Ok(())
    }

    fn patterns(patterns: &[&str]) -> Vec<glob::Pattern> {
        patterns
            .iter()
            .map(|p| glob::Pattern::new(p).unwrap())
            .collect()
    }

    /// A list mise cannot read in full is an error naming the entry and
    /// the pattern, never a shorter list. A shorter `exclude` captures
    /// files the user asked to leave out; a shorter — or empty —
    /// `include` captures the whole tree they asked to narrow.
    #[test]
    fn an_unparsable_pattern_list_is_an_error_naming_the_pattern() {
        for key in ["exclude", "include"] {
            let error = compile_patterns(key, Some(vec!["fine/**".into(), "[".into()]))
                .expect_err("an unparsable pattern is an error");
            assert!(error.contains(key), "{error}");
            assert!(error.contains('['), "{error}");
        }
        // a list that reads in full is kept exactly, empty or not
        assert_eq!(
            compile_patterns("include", Some(vec![]))
                .unwrap()
                .map(|patterns| patterns.len()),
            Some(0),
            "a declared empty list stays a declared empty list"
        );
        assert!(compile_patterns("include", None).unwrap().is_none());
    }

    #[test]
    fn test_exclude_bare_pattern_matches_any_component() {
        let pats = patterns(&["mise.toml"]);
        assert!(is_excluded(Path::new("mise.toml"), &pats));
        assert!(is_excluded(Path::new("nested/mise.toml"), &pats));
        assert!(!is_excluded(Path::new("mise.toml.bak"), &pats));
        assert!(!is_excluded(Path::new("other.toml"), &pats));
    }

    #[test]
    fn test_exclude_wildcard_matches_by_component() {
        let pats = patterns(&["*.md"]);
        assert!(is_excluded(Path::new("README.md"), &pats));
        assert!(is_excluded(Path::new("docs/guide.md"), &pats));
        assert!(!is_excluded(Path::new("README.txt"), &pats));
    }

    #[test]
    fn test_exclude_slash_pattern_is_anchored() {
        let pats = patterns(&["nvim/spell"]);
        assert!(is_excluded(Path::new("nvim/spell"), &pats));
        // matching a directory takes everything under it
        assert!(is_excluded(Path::new("nvim/spell/en.add"), &pats));
        // but the same name elsewhere in the tree is untouched
        assert!(!is_excluded(Path::new("config/nvim/spell"), &pats));
        assert!(!is_excluded(Path::new("spell"), &pats));
    }

    #[test]
    fn test_exclude_directory_component_takes_children() {
        let pats = patterns(&[".git"]);
        assert!(is_excluded(Path::new(".git"), &pats));
        assert!(is_excluded(Path::new(".git/config"), &pats));
        assert!(!is_excluded(Path::new("git/config"), &pats));
    }

    #[test]
    fn test_exclude_empty_matches_nothing() {
        assert!(!is_excluded(Path::new("mise.toml"), &[]));
    }

    #[test]
    fn a_later_layer_overrides_a_track_entry_exclude_list() {
        let request = |exclude: Vec<&str>, explicit: bool| FileRequest {
            target_raw: "~/.codex".into(),
            target: PathBuf::from("/home/test/.codex"),
            source: PathBuf::new(),
            content: None,
            mode: FileMode::Track,
            exclude: exclude
                .into_iter()
                .map(|p| glob::Pattern::new(p).unwrap())
                .collect(),
            include: None,
            manifest: None,
            permissions: None,
            base: PathBuf::from("/home/test"),
            origin: crate::system::resources::ResourceOrigin {
                config: PathBuf::from("/home/test/.config/mise/config.toml"),
                config_root: PathBuf::from("/home/test/.config/mise"),
                environment: vec![],
                source: None,
            },
            policy: FilePolicy {
                explicit: ExplicitFields {
                    exclude: explicit,
                    ..Default::default()
                },
                ..FilePolicy::for_mode(FileMode::Track)
            },
            variants: vec![],
            enabled: true,
            remove_empty: false,
            dot_prefix: false,
            relative: false,
        };
        let mut first = request(vec!["sessions"], true);
        first.override_from(request(vec!["cache"], true));
        assert_eq!(first.exclude[0].as_str(), "cache");
        assert!(first.policy.explicit.exclude);
        let mut first = request(vec!["sessions"], true);
        first.override_from(request(vec![], false));
        assert_eq!(first.exclude[0].as_str(), "sessions");
    }

    #[test]
    fn test_wildcard_target_expansion() {
        let captures = wildcard_captures(
            "/repo/dotfiles/config/*.toml",
            Path::new("/repo/dotfiles/config/starship.toml"),
        )
        .unwrap();
        let target = expand_target_pattern("/home/me/.config/*.toml", &captures).unwrap();
        assert_eq!(target, PathBuf::from("/home/me/.config/starship.toml"));
    }

    #[test]
    fn test_recursive_wildcard_target_expansion() {
        let captures = wildcard_captures(
            "/repo/dotfiles/config/**/*.toml",
            Path::new("/repo/dotfiles/config/a/b/tool.toml"),
        )
        .unwrap();
        let target = expand_target_pattern("/home/me/.config/**/*.toml", &captures).unwrap();
        assert_eq!(target, PathBuf::from("/home/me/.config/a/b/tool.toml"));
    }

    #[test]
    fn test_recursive_wildcard_matches_zero_directories() {
        let captures = wildcard_captures(
            "/repo/dotfiles/config/**/*.toml",
            Path::new("/repo/dotfiles/config/tool.toml"),
        )
        .unwrap();
        let target = expand_target_pattern("/home/me/.config/**/*.toml", &captures).unwrap();
        assert_eq!(target, PathBuf::from("/home/me/.config/tool.toml"));
    }

    #[test]
    fn test_question_mark_target_expansion() {
        let captures = wildcard_captures(
            "/repo/dotfiles/config/app?.toml",
            Path::new("/repo/dotfiles/config/app1.toml"),
        )
        .unwrap();
        let target = expand_target_pattern("/home/me/.config/app?.toml", &captures).unwrap();
        assert_eq!(target, PathBuf::from("/home/me/.config/app1.toml"));
    }

    #[test]
    fn test_character_class_target_expansion() {
        let captures = wildcard_captures(
            "/repo/dotfiles/config/theme-[ab].toml",
            Path::new("/repo/dotfiles/config/theme-a.toml"),
        )
        .unwrap();
        let target = expand_target_pattern("/home/me/.config/theme-[ab].toml", &captures).unwrap();
        assert_eq!(target, PathBuf::from("/home/me/.config/theme-a.toml"));
    }

    #[test]
    fn test_windows_separator_wildcard_expansion() {
        let captures = wildcard_captures(
            r"C:\repo\dotfiles\config\*.toml",
            Path::new(r"C:\repo\dotfiles\config\starship.toml"),
        )
        .unwrap();
        let target = expand_target_pattern(r"C:\Users\me\.config\*.toml", &captures).unwrap();
        assert_eq!(
            target,
            PathBuf::from(native_path_separators("C:/Users/me/.config/starship.toml"))
        );
    }

    #[test]
    fn test_windows_separator_recursive_wildcard_expansion() {
        let captures = wildcard_captures(
            r"C:\repo\dotfiles\config\**\*.toml",
            Path::new(r"C:\repo\dotfiles\config\tool.toml"),
        )
        .unwrap();
        let target = expand_target_pattern(r"C:\Users\me\.config\**\*.toml", &captures).unwrap();
        assert_eq!(
            target,
            PathBuf::from(native_path_separators("C:/Users/me/.config/tool.toml"))
        );
    }

    fn link_req(source: &Path, target: &Path, mode: FileMode) -> FileRequest {
        FileRequest {
            target_raw: target.to_string_lossy().to_string(),
            target: target.to_path_buf(),
            source: source.to_path_buf(),
            // These tests are about link and copy modes, which read the file at `source`. Inline
            // content is the other kind of entry and has nothing to do with what they assert.
            content: None,
            mode,
            exclude: vec![],
            include: None,
            manifest: None,
            permissions: None,
            base: source.parent().expect("source parent").to_path_buf(),
            origin: ResourceOrigin {
                config: PathBuf::from("/mise.toml"),
                config_root: PathBuf::from("/"),
                environment: vec![],
                source: Some(source.to_path_buf()),
            },
            policy: FilePolicy::for_mode(mode),
            variants: vec![],
            enabled: true,
            remove_empty: false,
            dot_prefix: false,
            relative: false,
        }
    }

    fn symlink_req(source: &Path, target: &Path) -> FileRequest {
        link_req(source, target, FileMode::Symlink)
    }

    #[test]
    fn apply_one_lists_only_the_files_it_wrote() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let source = dir.path().join("source");
        file::create_dir_all(source.join("sub"))?;
        file::write(source.join("a.toml"), "a")?;
        file::write(source.join("sub/b.toml"), "b")?;
        let file_source = dir.path().join("file");
        file::write(&file_source, "file")?;

        // a single-file copy lists its target once it is written, not the
        // directories created on the way
        let target = dir.path().join("one/settings.toml");
        let mut written = vec![];
        apply_one(
            &link_req(&file_source, &target, FileMode::Copy),
            None,
            &mut written,
        )?;
        assert_eq!(written, vec![target]);

        // a directory copy lists each file as it lands
        let target = dir.path().join("all");
        let mut written = vec![];
        apply_one(
            &link_req(&source, &target, FileMode::Copy),
            None,
            &mut written,
        )?;
        written.sort();
        assert_eq!(
            written,
            vec![target.join("a.toml"), target.join("sub/b.toml")]
        );

        // a directory copy that fails part-way lists the files written
        // before the failure and nothing after it: a file where `sub` must
        // become a directory stops the walk after `a.toml`
        let target = dir.path().join("partial");
        file::create_dir_all(&target)?;
        file::write(target.join("sub"), "in the way")?;
        let mut written = vec![];
        assert!(
            apply_one(
                &link_req(&source, &target, FileMode::Copy),
                None,
                &mut written
            )
            .is_err()
        );
        assert_eq!(written, vec![target.join("a.toml")]);

        // a write that fails before touching its target lists nothing
        let target = dir.path().join("partial/sub/settings.toml");
        let mut written = vec![];
        assert!(
            apply_one(
                &link_req(&file_source, &target, FileMode::Copy),
                None,
                &mut written
            )
            .is_err()
        );
        assert!(written.is_empty());

        // an existing target that was cleared before the write failed is
        // mutated (its old content is gone), so it is listed
        let target = dir.path().join("replaced");
        file::write(&target, "old")?;
        let missing_source = dir.path().join("missing");
        let mut written = vec![];
        assert!(
            apply_one(
                &link_req(&missing_source, &target, FileMode::Copy),
                None,
                &mut written
            )
            .is_err()
        );
        assert!(!target.exists());
        assert_eq!(written, vec![target]);
        // the same failed write against an absent target lists nothing
        let target = dir.path().join("never");
        let mut written = vec![];
        assert!(
            apply_one(
                &link_req(&missing_source, &target, FileMode::Copy),
                None,
                &mut written
            )
            .is_err()
        );
        assert!(written.is_empty());
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn directory_copy_leaves_an_existing_file_alone_when_the_source_is_not_a_file() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let source = dir.path().join("source");
        file::create_dir_all(source.join("real"))?;
        // a link to a directory walks as a file-like entry but is not one
        std::os::unix::fs::symlink(source.join("real"), source.join("entry"))?;
        let target = dir.path().join("target");
        file::create_dir_all(&target)?;
        file::write(target.join("entry"), "keep me")?;

        let mut written = vec![];
        assert!(
            apply_one(
                &link_req(&source, &target, FileMode::Copy),
                None,
                &mut written
            )
            .is_err()
        );
        assert_eq!(file::read_to_string(target.join("entry"))?, "keep me");
        assert!(written.is_empty());
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn directory_copy_leaves_an_existing_link_alone_when_the_source_is_not_a_file() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let source = dir.path().join("source");
        file::create_dir_all(source.join("real"))?;
        std::os::unix::fs::symlink(source.join("real"), source.join("entry"))?;
        let elsewhere = dir.path().join("elsewhere");
        file::write(&elsewhere, "keep me")?;
        let target = dir.path().join("target");
        file::create_dir_all(&target)?;
        std::os::unix::fs::symlink(&elsewhere, target.join("entry"))?;

        // the source cannot be copied, so the link it would replace stays
        let mut written = vec![];
        assert!(
            apply_one(
                &link_req(&source, &target, FileMode::Copy),
                None,
                &mut written
            )
            .is_err()
        );
        assert_eq!(std::fs::read_link(target.join("entry"))?, elsewhere);
        assert!(written.is_empty());
        Ok(())
    }

    #[test]
    fn directory_copy_does_not_record_an_existing_directory_the_copy_left_alone() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let source = dir.path().join("source");
        file::create_dir_all(&source)?;
        file::write(source.join("entry"), "file")?;
        // a directory where the copy wants a file: the copy fails without
        // touching it, so nothing changed and nothing is recorded
        let target = dir.path().join("target");
        file::create_dir_all(target.join("entry"))?;
        let mut written = vec![];
        assert!(
            apply_one(
                &link_req(&source, &target, FileMode::Copy),
                None,
                &mut written
            )
            .is_err()
        );
        assert!(target.join("entry").is_dir());
        assert!(written.is_empty());
        Ok(())
    }

    #[test]
    fn create_recorded_lists_a_target_the_failed_write_created() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let target = dir.path().join("created");
        // the write created the file before failing: its state changed
        let mut written = vec![];
        let result = create_recorded(&target, &mut written, || {
            file::write(&target, "partial")?;
            bail!("disk full")
        });
        assert!(result.is_err());
        assert_eq!(written, vec![target.clone()]);
        // the write failed before creating anything: nothing to reload
        let target = dir.path().join("never");
        let mut written = vec![];
        let result = create_recorded(&target, &mut written, || bail!("permission denied"));
        assert!(result.is_err());
        assert!(written.is_empty());
        // and a successful write is recorded as before
        let mut written = vec![];
        create_recorded(&target, &mut written, || file::write(&target, "ok"))?;
        assert_eq!(written, vec![target]);
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn directory_copy_records_an_existing_file_only_once_it_is_opened_for_writing() -> Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir()?;
        let source = dir.path().join("source");
        file::create_dir_all(&source)?;
        file::write(source.join("a.toml"), "new")?;
        let target = dir.path().join("target");
        file::create_dir_all(&target)?;
        file::write(target.join("a.toml"), "old")?;

        // an existing writable file is overwritten in place and recorded
        let mut written = vec![];
        apply_one(
            &link_req(&source, &target, FileMode::Copy),
            None,
            &mut written,
        )?;
        assert_eq!(written, vec![target.join("a.toml")]);
        assert_eq!(file::read_to_string(target.join("a.toml"))?, "new");

        // a read-only existing file cannot be opened for truncation: nothing
        // changes and nothing is recorded (root can open it regardless, so
        // the case is skipped there)
        file::write(target.join("a.toml"), "old")?;
        std::fs::set_permissions(
            target.join("a.toml"),
            std::fs::Permissions::from_mode(0o444),
        )?;
        if std::fs::OpenOptions::new()
            .write(true)
            .open(target.join("a.toml"))
            .is_ok()
        {
            return Ok(());
        }
        let mut written = vec![];
        assert!(
            apply_one(
                &link_req(&source, &target, FileMode::Copy),
                None,
                &mut written
            )
            .is_err()
        );
        assert!(written.is_empty());
        assert_eq!(file::read_to_string(target.join("a.toml"))?, "old");
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn directory_symlink_listing_is_best_effort() -> Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir()?;
        let source = dir.path().join("source");
        file::create_dir_all(source.join("sub"))?;
        file::write(source.join("sub/hidden.toml"), "x")?;
        // an unreadable subdirectory makes the walk fail (unless running as
        // root, where the walk simply succeeds); the link still lands and is
        // recorded either way
        std::fs::set_permissions(source.join("sub"), std::fs::Permissions::from_mode(0o000))?;
        let target = dir.path().join("target");
        let mut written = vec![];
        let result = apply_one(&symlink_req(&source, &target), None, &mut written);
        std::fs::set_permissions(source.join("sub"), std::fs::Permissions::from_mode(0o755))?;
        result?;
        assert!(target.is_symlink());
        assert_eq!(written.first(), Some(&target));
        Ok(())
    }

    #[test]
    fn composed_symlink_each_allows_disjoint_leaves() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let source_a = dir.path().join("a");
        let source_b = dir.path().join("b");
        let target = dir.path().join("target");
        file::create_dir_all(source_a.join("conf.d"))?;
        file::create_dir_all(source_b.join("conf.d"))?;
        file::write(source_a.join("conf.d/a.toml"), "a")?;
        file::write(source_b.join("conf.d/b.toml"), "b")?;

        validate_composed_file_footprints(&[
            link_req(&source_a, &target, FileMode::SymlinkEach),
            link_req(&source_b, &target, FileMode::SymlinkEach),
        ])?;
        Ok(())
    }

    #[test]
    fn reconciliation_preserves_every_active_contributor_for_selected_target() {
        let target = PathBuf::from("/home/example");
        let state = |source: &str, leaf: &str| SymlinkEachState {
            version: SYMLINK_EACH_STATE_VERSION,
            source: PathBuf::from(source),
            target: target.clone(),
            links: vec![ManagedLink {
                source: PathBuf::from(source).join(leaf),
                target: target.join(leaf),
            }],
        };
        let home = state("/repo/home", ".zshrc");
        let work = state("/repo/work", ".gitconfig");
        let selected_targets = [target].into_iter().collect();

        let reconciliation =
            reconcile_symlink_each_states(&[home, work.clone()], &selected_targets, &[work]);

        assert!(reconciliation.stale_links.is_empty());
        assert!(reconciliation.targets.is_empty());
    }

    #[test]
    fn reconciliation_preserves_ownership_for_missing_active_source() {
        let target = PathBuf::from("/home/example");
        let stored = SymlinkEachState {
            version: SYMLINK_EACH_STATE_VERSION,
            source: PathBuf::from("/repo/home"),
            target: target.clone(),
            links: vec![ManagedLink {
                source: PathBuf::from("/repo/home/.zshrc"),
                target: target.join(".zshrc"),
            }],
        };
        let active = SymlinkEachState {
            links: vec![],
            ..stored.clone()
        };
        let selected_targets = [target].into_iter().collect();

        let reconciliation = reconcile_symlink_each_states(&[active], &selected_targets, &[stored]);

        assert!(reconciliation.stale_links.is_empty());
        assert!(reconciliation.targets.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn normalized_source_matches_dangling_managed_link() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let source_dir = dir.path().join("profile");
        let source = source_dir.join("../profile/.zshrc");
        let target = dir.path().join("target");
        file::create_dir_all(&source_dir)?;
        file::write(&source, "managed")?;
        std::os::unix::fs::symlink(&source, &target)?;
        file::remove_file(&source)?;

        assert!(link_points_to(&lexical_normalize(&source), &target));
        Ok(())
    }

    #[test]
    fn legacy_symlink_each_state_paths_are_normalized() {
        let state = SymlinkEachState {
            version: SYMLINK_EACH_STATE_VERSION,
            source: PathBuf::from("/repo/./home"),
            target: PathBuf::from("/home/example/../example"),
            links: vec![ManagedLink {
                source: PathBuf::from("/repo/home/config/../.zshrc"),
                target: PathBuf::from("/home/example/./.zshrc"),
            }],
        };

        assert_eq!(
            normalize_symlink_each_state(state),
            SymlinkEachState {
                version: SYMLINK_EACH_STATE_VERSION,
                source: PathBuf::from("/repo/home"),
                target: PathBuf::from("/home/example"),
                links: vec![ManagedLink {
                    source: PathBuf::from("/repo/home/.zshrc"),
                    target: PathBuf::from("/home/example/.zshrc"),
                }],
            }
        );
    }

    #[test]
    fn composed_symlink_each_rejects_duplicate_state_identity() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let source = dir.path().join("source");
        let target = dir.path().join("target");
        file::create_dir_all(&source)?;
        let ordinary = link_req(&source, &target, FileMode::SymlinkEach);
        let mut git_manifest = ordinary.clone();
        git_manifest.manifest = Some(FileManifest::Git);

        let err = validate_composed_file_footprints(&[ordinary, git_manifest]).unwrap_err();
        assert!(err.to_string().contains("conflicting symlink-each"));
        Ok(())
    }

    #[test]
    fn composed_symlink_each_rejects_duplicate_leaves() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let source_a = dir.path().join("a");
        let source_b = dir.path().join("b");
        let target = dir.path().join("target");
        file::create_dir_all(&source_a)?;
        file::create_dir_all(&source_b)?;
        file::write(source_a.join("shared"), "a")?;
        file::write(source_b.join("shared"), "b")?;

        let err = validate_composed_file_footprints(&[
            link_req(&source_a, &target, FileMode::SymlinkEach),
            link_req(&source_b, &target, FileMode::SymlinkEach),
        ])
        .unwrap_err();
        assert!(
            err.to_string()
                .contains(&target.join("shared").to_string_lossy().to_string())
        );
        Ok(())
    }

    #[test]
    fn composed_symlink_each_rejects_file_directory_collisions() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let source_a = dir.path().join("a");
        let source_b = dir.path().join("b");
        let target = dir.path().join("target");
        file::create_dir_all(&source_a)?;
        file::create_dir_all(source_b.join("shared"))?;
        file::write(source_a.join("shared"), "a")?;
        file::write(source_b.join("shared/nested"), "b")?;

        let err = validate_composed_file_footprints(&[
            link_req(&source_a, &target, FileMode::SymlinkEach),
            link_req(&source_b, &target, FileMode::SymlinkEach),
        ])
        .unwrap_err();
        assert!(
            err.to_string()
                .contains(&target.join("shared").to_string_lossy().to_string())
        );
        Ok(())
    }

    /// Nested declarations may share directories when their concrete leaves
    /// remain disjoint.
    #[test]
    fn tracking_does_not_claim_an_apply_footprint() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let source = dir.path().join("source");
        let target = dir.path().join("target");
        file::write(&source, "managed")?;
        for requests in [
            vec![
                link_req(&source, &target, FileMode::Track),
                link_req(&source, &target.join("nested"), FileMode::Copy),
            ],
            vec![
                link_req(&source, &target.join("nested"), FileMode::Copy),
                link_req(&source, &target, FileMode::Track),
            ],
        ] {
            validate_composed_file_footprints(&requests)?;
        }
        Ok(())
    }

    #[test]
    fn managed_directory_can_contain_explicitly_tracked_children() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let source = dir.path().join("source");
        let target = dir.path().join("target");
        file::create_dir_all(&source)?;
        file::write(source.join("native"), "managed")?;
        for mode in [FileMode::Copy, FileMode::SymlinkEach] {
            let requests = vec![
                link_req(&source, &target, mode),
                link_req(&source, &target.join("native"), FileMode::Track),
            ];
            validate_composed_file_footprints(&requests)?;
            let reversed = requests.into_iter().rev().collect::<Vec<_>>();
            validate_composed_file_footprints(&reversed)?;
        }
        Ok(())
    }

    #[test]
    fn composed_file_footprints_allow_disjoint_nested_leaves() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let source_tree = dir.path().join("tree");
        let source_file = dir.path().join("nested");
        let target = dir.path().join("target");
        file::create_dir_all(&source_tree)?;
        file::write(source_tree.join("owned-by-tree"), "tree")?;
        file::write(&source_file, "nested")?;

        validate_composed_file_footprints(&[
            link_req(&source_tree, &target, FileMode::Copy),
            link_req(
                &source_file,
                &target.join("owned-separately"),
                FileMode::Copy,
            ),
        ])?;
        Ok(())
    }

    /// A nested whole-file declaration cannot also be owned by a directory
    /// copy, regardless of declaration order.
    #[test]
    fn composed_file_footprints_reject_directory_copy_nested_leaf() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let source_tree = dir.path().join("tree");
        let source_file = dir.path().join("nested");
        let target = dir.path().join("target");
        file::create_dir_all(&source_tree)?;
        file::write(source_tree.join("shared"), "tree")?;
        file::write(&source_file, "nested")?;
        let tree = link_req(&source_tree, &target, FileMode::Copy);
        let nested = link_req(&source_file, &target.join("shared"), FileMode::Copy);

        for requests in [
            [tree.clone(), nested.clone()],
            [nested.clone(), tree.clone()],
        ] {
            let err = validate_composed_file_footprints(&requests).unwrap_err();
            assert!(err.to_string().contains("conflicting dotfile declarations"));
            assert!(
                err.to_string()
                    .contains(&target.join("shared").to_string_lossy().to_string())
            );
        }
        Ok(())
    }

    /// A whole-resource leaf cannot occupy a path another declaration needs
    /// as a directory.
    #[test]
    fn composed_file_footprints_reject_leaf_required_as_directory() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let source_tree = dir.path().join("tree");
        let source_file = dir.path().join("nested");
        let target = dir.path().join("target");
        file::create_dir_all(&source_tree)?;
        file::write(&source_file, "nested")?;

        let err = validate_composed_file_footprints(&[
            link_req(&source_tree, &target, FileMode::Symlink),
            link_req(&source_file, &target.join("nested"), FileMode::Copy),
        ])
        .unwrap_err();
        assert!(err.to_string().contains("conflicting dotfile declarations"));
        assert!(
            err.to_string()
                .contains(&target.to_string_lossy().to_string())
        );
        Ok(())
    }

    /// A declaration whose source is unavailable still reserves its target,
    /// preventing a filtered apply from writing a nested declaration there.
    #[test]
    fn composed_file_footprints_reserve_missing_source_target() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let missing = dir.path().join("missing");
        let source_file = dir.path().join("nested");
        let target = dir.path().join("target");
        file::write(&source_file, "nested")?;

        let err = validate_composed_file_footprints(&[
            link_req(&missing, &target, FileMode::Copy),
            link_req(&source_file, &target.join("nested"), FileMode::Copy),
        ])
        .unwrap_err();
        assert!(err.to_string().contains("conflicting dotfile declarations"));
        assert!(
            err.to_string()
                .contains(&target.to_string_lossy().to_string())
        );
        Ok(())
    }

    /// An unavailable symlink-each contributor reserves the shared target as
    /// a directory, allowing a sibling's known leaves to remain composable.
    #[test]
    fn composed_file_footprints_keep_missing_symlink_each_directory_shaped() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let missing = dir.path().join("missing");
        let source_tree = dir.path().join("tree");
        let target = dir.path().join("target");
        file::create_dir_all(&source_tree)?;
        file::write(source_tree.join("known"), "known")?;

        validate_composed_file_footprints(&[
            link_req(&missing, &target, FileMode::SymlinkEach),
            link_req(&source_tree, &target, FileMode::SymlinkEach),
        ])?;
        Ok(())
    }

    /// The fix: a file the user wrote is not mise's to replace. This used to pass on Windows,
    /// where the entry applied and overwrote it with no `--force` and no message.
    #[test]
    fn an_unmanaged_file_at_the_target_is_a_conflict() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let source = dir.path().join("source");
        let target = dir.path().join("target");
        file::write(&source, "managed")?;
        file::write(&target, "the user's own file")?;

        assert_eq!(
            find_conflicts(&symlink_req(&source, &target))?,
            vec![target]
        );
        Ok(())
    }

    /// The control for the test above: the two cases that must *not* start blocking. A symlink
    /// is one mise made, so re-pointing it is the normal path and would be a regression to
    /// refuse; a missing target has nothing to protect.
    #[test]
    fn a_managed_symlink_and_a_missing_target_are_not_conflicts() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let source = dir.path().join("source");
        file::write(&source, "managed")?;

        let absent = dir.path().join("absent");
        assert!(find_conflicts(&symlink_req(&source, &absent))?.is_empty());

        // Created directly rather than through `link_path`, which copies on Windows: the point
        // here is what `find_conflicts` does when a symlink *is* present.
        let link = dir.path().join("link");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&source, &link)?;
        #[cfg(windows)]
        let created = std::os::windows::fs::symlink_file(&source, &link).is_ok();
        #[cfg(unix)]
        let created = true;
        if created {
            assert!(link.is_symlink(), "precondition: target must be a symlink");
            assert!(find_conflicts(&symlink_req(&source, &link))?.is_empty());
        }
        Ok(())
    }

    /// `symlink-each` shares the predicate, so it gains the same protection: a file the user
    /// wrote inside the target directory is not mise's to replace either. Asserted separately
    /// because it reaches `file_link_conflicts` through `walk_source_files` rather than
    /// directly, and the mode is the half of this change most likely to be overlooked.
    #[test]
    fn symlink_each_also_treats_an_unmanaged_file_as_a_conflict() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let source = dir.path().join("source");
        let target = dir.path().join("target");
        // `file::write` does not create parents, and the target directory must already exist or
        // `needed_dirs` would report it as its own conflict and drown the assertion.
        file::create_dir_all(&source)?;
        file::create_dir_all(&target)?;
        file::write(source.join("one"), "managed")?;
        file::write(source.join("two"), "managed")?;
        // Only `one` is squatted on; `two` has nothing in its way.
        file::write(target.join("one"), "the user's own file")?;

        assert_eq!(
            find_conflicts(&link_req(&source, &target, FileMode::SymlinkEach))?,
            vec![target.join("one")]
        );
        Ok(())
    }

    /// A directory where a file belongs blocked before this change and still does — the
    /// type-mismatch case is untouched.
    #[test]
    fn a_directory_at_the_target_is_still_a_conflict() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let source = dir.path().join("source");
        let target = dir.path().join("target");
        file::write(&source, "managed")?;
        file::create_dir_all(&target)?;

        assert_eq!(
            find_conflicts(&symlink_req(&source, &target))?,
            vec![target]
        );
        Ok(())
    }

    /// `link_path` produces either a symlink or a copy on Windows depending on a privilege the
    /// test runner may or may not have, so these assert the property that has to hold for both:
    /// whichever form lands, `check_symlink` reads it as Applied. Branching on what actually
    /// happened rather than on an assumed privilege keeps this meaningful on a runner without
    /// Developer Mode, where only the copy path is exercised.
    #[test]
    fn link_path_result_is_recognised_whichever_form_it_takes() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let source = dir.path().join("dotfile");
        let target = dir.path().join("linked");
        file::write(&source, "contents")?;

        link_path(&source, &target, false, true)?;

        assert!(
            target.exists() || target.is_symlink(),
            "link_path produced nothing"
        );
        assert_eq!(check_symlink(&source, &target, false)?, FileState::Applied);
        Ok(())
    }

    /// `symlink-each` opts out of the Windows symlink attempt, so it keeps producing a copy
    /// there. Pinned because the two modes share `link_path`: making it symlink for everyone
    /// would route `symlink-each` unapply through a planner that rejects symlinks.
    #[cfg(windows)]
    #[test]
    fn symlink_each_still_copies_on_windows() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let source = dir.path().join("dotfile");
        let target = dir.path().join("copied");
        file::write(&source, "contents")?;

        link_path(&source, &target, false, false)?;

        assert!(
            !target.is_symlink(),
            "symlink-each must not create a symlink"
        );
        assert_eq!(file::read_to_string(&target)?, "contents");
        Ok(())
    }

    /// The control for the test above: a target that is neither a copy of the source nor a link
    /// to it must not read as Applied, or the assertion there would hold for the wrong reason.
    #[test]
    fn check_symlink_rejects_an_unrelated_file() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let source = dir.path().join("dotfile");
        let target = dir.path().join("unrelated");
        file::write(&source, "contents")?;
        file::write(&target, "something else")?;

        assert!(!matches!(
            check_symlink(&source, &target, false)?,
            FileState::Applied
        ));
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn relative_link_reaches_the_source_and_survives_a_move() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let home = dir.path().join("home");
        let source = home.join("dotfiles/foo");
        let target = home.join(".config/foo");
        file::create_dir_all(source.parent().unwrap())?;
        file::write(&source, "contents")?;
        file::create_dir_all(target.parent().unwrap())?;

        link_path(&source, &target, true, true)?;

        assert_eq!(
            std::fs::read_link(&target)?,
            PathBuf::from("../dotfiles/foo")
        );
        assert_eq!(check_symlink(&source, &target, true)?, FileState::Applied);
        // either setting accepts a relative link that reaches the source
        assert_eq!(check_symlink(&source, &target, false)?, FileState::Applied);

        let moved = dir.path().join("moved");
        std::fs::rename(&home, &moved)?;
        assert_eq!(file::read_to_string(moved.join(".config/foo"))?, "contents");
        Ok(())
    }

    /// Turning `relative` on must convert a link an earlier apply made.
    #[cfg(unix)]
    #[test]
    fn relative_rejects_an_absolute_link_to_the_source() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let source = dir.path().join("dotfile");
        let target = dir.path().join("linked");
        file::write(&source, "contents")?;
        link_path(&source, &target, false, true)?;

        assert!(matches!(
            check_symlink(&source, &target, true)?,
            FileState::Differs(_)
        ));
        Ok(())
    }

    /// `..` in a link resolves against the directory the link physically
    /// sits in, so a link directory reached through a symlink must not get
    /// a path computed from its configured spelling.
    #[cfg(unix)]
    #[test]
    fn relative_link_resolves_from_a_symlinked_link_directory() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let source = dir.path().join("home/dotfiles/foo");
        let real_config = dir.path().join("elsewhere/config");
        let config = dir.path().join("home/.config");
        file::create_dir_all(source.parent().unwrap())?;
        file::write(&source, "contents")?;
        file::create_dir_all(&real_config)?;
        file::make_symlink(&real_config, &config)?;
        let target = config.join("foo");

        link_path(&source, &target, true, true)?;

        assert!(std::fs::read_link(&target)?.is_relative());
        assert_eq!(file::read_to_string(&target)?, "contents");
        assert_eq!(check_symlink(&source, &target, true)?, FileState::Applied);
        Ok(())
    }

    /// Builds `home/dotfiles/foo` and a `home/.config` that is a symlink to
    /// `elsewhere/config`, returning (source, link directory, root).
    #[cfg(unix)]
    fn symlinked_link_dir() -> Result<(tempfile::TempDir, PathBuf, PathBuf)> {
        let dir = tempfile::tempdir()?;
        let source = dir.path().join("home/dotfiles/foo");
        let real_config = dir.path().join("elsewhere/config");
        let config = dir.path().join("home/.config");
        file::create_dir_all(source.parent().unwrap())?;
        file::write(&source, "contents")?;
        file::create_dir_all(&real_config)?;
        file::make_symlink(&real_config, &config)?;
        Ok((dir, source, config))
    }

    /// Unapply and `symlink-each` pruning must still own a link written from
    /// the canonical link directory once its source file is deleted.
    #[cfg(unix)]
    #[test]
    fn relative_link_in_a_symlinked_directory_is_owned_after_its_source_goes() -> Result<()> {
        let (_dir, source, config) = symlinked_link_dir()?;
        let target = config.join("foo");
        link_path(&source, &target, true, true)?;
        std::fs::remove_file(&source)?;

        assert!(link_points_to(&source, &target));
        Ok(())
    }

    /// Read from the configured spelling, `../dotfiles/foo` in `~/.config`
    /// names `~/dotfiles/foo`, but physically it leads somewhere else. That
    /// link is not mise's to remove.
    #[cfg(unix)]
    #[test]
    fn relative_link_matching_only_as_text_is_not_owned() -> Result<()> {
        let (dir, source, config) = symlinked_link_dir()?;
        let other = dir.path().join("elsewhere/dotfiles/foo");
        file::create_dir_all(other.parent().unwrap())?;
        file::write(&other, "someone else's")?;
        let target = config.join("foo");
        file::make_symlink(Path::new("../dotfiles/foo"), &target)?;

        assert!(!link_points_to(&source, &target));
        assert!(!matches!(
            check_symlink(&source, &target, true)?,
            FileState::Applied
        ));
        Ok(())
    }

    /// A dangling symlink in a `symlink-each` source still gets a relative
    /// link, or the entry would be rewritten on every apply.
    #[cfg(unix)]
    #[test]
    fn relative_link_to_a_dangling_source_converges() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let source = dir.path().join("dotfiles/dangling");
        let target = dir.path().join("home/dangling");
        file::create_dir_all(source.parent().unwrap())?;
        file::create_dir_all(target.parent().unwrap())?;
        file::make_symlink(&dir.path().join("nowhere"), &source)?;

        link_path(&source, &target, true, false)?;

        assert_eq!(
            std::fs::read_link(&target)?,
            PathBuf::from("../dotfiles/dangling")
        );
        assert_eq!(check_symlink(&source, &target, true)?, FileState::Applied);
        Ok(())
    }

    /// `alias/../dotfile` leaves wherever `alias` really leads, not the
    /// directory the link sits in, so it names a different file here.
    #[cfg(unix)]
    #[test]
    fn relative_link_through_a_symlink_then_parent_is_not_owned() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let source = dir.path().join("dotfile");
        let other = dir.path().join("real/dotfile");
        file::write(&source, "mine")?;
        file::create_dir_all(dir.path().join("real/sub"))?;
        file::write(&other, "someone else's")?;
        file::make_symlink(&dir.path().join("real/sub"), &dir.path().join("alias"))?;
        let target = dir.path().join("link");
        file::make_symlink(Path::new("alias/../dotfile"), &target)?;
        assert_eq!(file::read_to_string(&target)?, "someone else's");

        assert!(!link_points_to(&source, &target));
        std::fs::remove_file(&other)?;
        assert!(!link_points_to(&source, &target));
        Ok(())
    }

    fn permissions_req(target: &Path, permissions: u32) -> FileRequest {
        FileRequest {
            source: PathBuf::new(),
            mode: FileMode::Permissions,
            permissions: Some(permissions),
            policy: FilePolicy::for_mode(FileMode::Permissions),
            ..link_req(Path::new("/unused"), target, FileMode::Copy)
        }
    }

    fn incoming(body: &str) -> Result<()> {
        use crate::config::config_file::mise_toml::MiseToml;
        use std::sync::Arc;

        let path = dirs::HOME.join(".config/mise/config.toml");
        let mut configs = ConfigMap::new();
        configs.insert(
            path.clone(),
            Arc::new(MiseToml::for_history_preflight(body, &path)?),
        );
        validate_incoming_files(&configs)
    }

    #[test]
    fn permissions_parse_as_octal_strings() {
        assert_eq!(parse_permissions("0600").unwrap(), 0o600);
        assert_eq!(parse_permissions("0o750").unwrap(), 0o750);
        assert_eq!(parse_permissions("600").unwrap(), 0o600);
        for invalid in ["", "0800", "rw-------", "17777"] {
            assert!(parse_permissions(invalid).is_err(), "{invalid:?}");
        }
    }

    /// A table holding only `permissions` is a whole-file entry, not an
    /// edit, and it infers no source.
    #[test]
    fn a_permissions_only_table_is_a_whole_file_entry() {
        let value: toml::Value = toml::from_str(r#"permissions = "0600""#).unwrap();
        let Some(FileTomlEntry::Table {
            source,
            permissions,
            ..
        }) = file_entry_from_toml("~/.ssh/config", value)
        else {
            panic!("expected a whole-file table entry");
        };
        assert_eq!(source, None);
        assert_eq!(permissions.as_deref(), Some("0600"));

        // an edit key keeps the table an edit entry
        let value: toml::Value = toml::from_str("block = \"x\"\npermissions = \"0600\"").unwrap();
        assert!(file_entry_from_toml("~/.bashrc/id", value).is_none());
    }

    #[test]
    fn incoming_permissions_accept_file_writing_entries() -> Result<()> {
        incoming(
            r#"
[dotfiles]
"~/.ssh/config" = { permissions = "0600" }
"~/.netrc" = { source = "netrc.tera", mode = "template", permissions = "0600" }
"~/.config/app.toml" = { source = "app.toml", mode = "copy", permissions = "0640" }
"~/.config/token" = { content = "secret\n", permissions = "0400" }
"#,
        )
    }

    #[test]
    fn incoming_permissions_reject_unsupported_combinations() {
        for (entry, expected) in [
            (r#"{ permissions = "0600", mode = "track" }"#, "track"),
            (
                r#"{ source = "x", mode = "symlink", permissions = "0600" }"#,
                "mode copy or template",
            ),
            (
                r#"{ source = "x", mode = "symlink-each", permissions = "0600" }"#,
                "mode copy or template",
            ),
            (r#"{ permissions = "0999" }"#, "octal"),
            (
                r#"{ permissions = "0600", exclude = ["*.bak"] }"#,
                "permissions-only",
            ),
            (
                r#"{ permissions = "0600", encrypt = false }"#,
                "permissions-only",
            ),
            (
                r#"{ source = "x", mode = "copy", manifest = "git", permissions = "0600" }"#,
                "manifest",
            ),
            (r#"{ permissions = "0600" }"#, "wildcards"),
        ] {
            let key = if expected == "wildcards" {
                "~/.ssh/id_*"
            } else {
                "~/.ssh/config"
            };
            let err = incoming(&format!("[dotfiles]\n\"{key}\" = {entry}\n"))
                .expect_err(entry)
                .to_string();
            assert!(err.contains(expected), "{entry}: {err}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_permissions_only_entry_changes_only_the_mode() -> Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir()?;
        let target = dir.path().join("config");
        file::write(&target, "user content")?;
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o644))?;
        let req = permissions_req(&target, 0o600);

        assert_eq!(
            check_rendered(&req, None)?,
            FileState::Differs("permissions differ".into())
        );
        assert!(find_conflicts(&req)?.is_empty());
        let mut written = vec![];
        apply_one(&req, None, &mut written)?;
        assert_eq!(written, vec![target.clone()]);
        assert_eq!(
            std::fs::metadata(&target)?.permissions().mode() & 0o7777,
            0o600
        );
        assert_eq!(file::read_to_string(&target)?, "user content");
        assert_eq!(check_rendered(&req, None)?, FileState::Applied);

        // unapply never removes a file mise only chmods, even with --force
        let opts = UnapplyOpts {
            dry_run: false,
            verbose: false,
            force: true,
            yes: true,
        };
        assert!(plan_unapply_one(&req, &opts)?.is_none());
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn a_permissions_only_entry_skips_missing_targets_and_links() -> Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir()?;

        let missing = dir.path().join("missing/config");
        let req = permissions_req(&missing, 0o600);
        // nothing to adjust counts as satisfied; status names the reason
        assert_eq!(check_rendered(&req, None)?, FileState::Applied);
        assert!(permissions_target_absent(&req).is_some());
        assert!(permissions_target_unavailable(&req)?.is_some());
        // applying is refused rather than creating the file or its parent
        assert!(apply_one(&req, None, &mut vec![]).is_err());
        assert!(!missing.parent().unwrap().exists());

        let real = dir.path().join("real");
        file::write(&real, "linked")?;
        std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o644))?;
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&real, &link)?;
        let req = permissions_req(&link, 0o600);
        assert!(matches!(
            check_rendered(&req, None)?,
            FileState::Differs(reason) if reason.contains("symlink")
        ));
        assert!(apply_one(&req, None, &mut vec![]).is_err());
        assert_eq!(
            std::fs::metadata(&real)?.permissions().mode() & 0o7777,
            0o644,
            "the link must not be followed"
        );
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn permissions_override_what_copies_and_templates_would_set() -> Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir()?;
        let source = dir.path().join("source");
        file::write(&source, "managed")?;
        std::fs::set_permissions(&source, std::fs::Permissions::from_mode(0o644))?;
        let mode_of = |path: &Path| -> Result<u32> {
            Ok(std::fs::metadata(path)?.permissions().mode() & 0o7777)
        };

        for mode in [FileMode::Copy, FileMode::Template, FileMode::Content] {
            let target = dir.path().join(mode.name());
            let mut req = link_req(&source, &target, mode);
            req.permissions = Some(0o600);
            if mode == FileMode::Content {
                req.content = Some("managed".into());
            }
            let rendered = (mode == FileMode::Template).then_some("managed");
            apply_one(&req, rendered, &mut vec![])?;
            assert_eq!(mode_of(&target)?, 0o600, "{}", mode.name());
            assert_eq!(check_rendered(&req, rendered)?, FileState::Applied);

            // drift is reported, and applying again repairs it
            std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o644))?;
            assert_eq!(
                check_rendered(&req, rendered)?,
                FileState::Differs("permissions differ".into()),
                "{}",
                mode.name()
            );
            apply_one(&req, rendered, &mut vec![])?;
            assert_eq!(mode_of(&target)?, 0o600, "{}", mode.name());
        }
        Ok(())
    }

    #[test]
    fn composed_file_footprints_scope_permissions_only_entries() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let source_file = dir.path().join("file");
        let source_tree = dir.path().join("tree");
        file::write(&source_file, "file")?;
        file::create_dir_all(&source_tree)?;
        file::write(source_tree.join("config"), "tree")?;
        let target = dir.path().join("target");

        // chmodding a directory another entry writes into claims nothing
        let copy = link_req(&source_file, &target.join("config"), FileMode::Copy);
        let directory = permissions_req(&target, 0o700);
        validate_composed_file_footprints(&[directory.clone(), copy.clone()])?;
        validate_composed_file_footprints(&[copy, directory])?;

        // but it must not fight another entry over a file that entry writes
        let tree = link_req(&source_tree, &target, FileMode::Copy);
        let leaf = permissions_req(&target.join("config"), 0o600);
        for requests in [[tree.clone(), leaf.clone()], [leaf, tree.clone()]] {
            let err = validate_composed_file_footprints(&requests).unwrap_err();
            assert!(err.to_string().contains("conflicting dotfile declarations"));
        }

        // permissions on a directory copy apply to no single file
        let mut tree = tree;
        tree.permissions = Some(0o600);
        let err = validate_composed_file_footprints(&[tree]).unwrap_err();
        assert!(err.to_string().contains("requires a file source"));
        Ok(())
    }

    #[test]
    fn incoming_permissions_reject_edit_entries() {
        let err =
            incoming("[dotfiles]\n\"~/.bashrc/id\" = { block = \"x\", permissions = \"0600\" }\n")
                .unwrap_err()
                .to_string();
        assert!(err.contains("whole-file entries"), "{err}");
    }

    /// The chmod acts on a descriptor opened without following a link, so a
    /// symlink swapped in after planning is refused, and a target its owner
    /// cannot read or write is still reachable.
    #[cfg(unix)]
    #[test]
    fn chmod_no_follow_refuses_links_and_reaches_unreadable_targets() -> Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir()?;
        let mode_of =
            |path: &Path| -> Result<u32> { Ok(permission_bits(&std::fs::symlink_metadata(path)?)) };

        let real = dir.path().join("real");
        file::write(&real, "real")?;
        std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o644))?;
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&real, &link)?;
        let err = chmod_no_follow(&link, 0o600).unwrap_err().to_string();
        assert!(err.contains("symlink"), "{err}");
        assert_eq!(mode_of(&real)?, 0o644);

        for from in [0o000, 0o200, 0o400] {
            let target = dir.path().join(format!("file{from:o}"));
            file::write(&target, "content")?;
            std::fs::set_permissions(&target, std::fs::Permissions::from_mode(from))?;
            chmod_no_follow(&target, 0o600)?;
            assert_eq!(mode_of(&target)?, 0o600, "from {from:o}");
        }

        let directory = dir.path().join("directory");
        file::create_dir_all(&directory)?;
        chmod_no_follow(&directory, 0o700)?;
        assert_eq!(mode_of(&directory)?, 0o700);
        Ok(())
    }

    /// A declared mode that denies the owner read access leaves content that
    /// cannot be compared; the mode is still checked instead of failing.
    #[cfg(unix)]
    #[test]
    fn a_write_only_copy_is_checked_by_its_mode() -> Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir()?;
        let source = dir.path().join("source");
        file::write(&source, "managed")?;
        let target = dir.path().join("target");
        let mut req = link_req(&source, &target, FileMode::Copy);
        req.permissions = Some(0o200);

        apply_one(&req, None, &mut vec![])?;
        assert_eq!(permission_bits(&std::fs::metadata(&target)?), 0o200);
        assert_eq!(check_rendered(&req, None)?, FileState::Applied);

        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o000))?;
        assert_eq!(
            check_rendered(&req, None)?,
            FileState::Differs("permissions differ".into())
        );
        apply_one(&req, None, &mut vec![])?;
        assert_eq!(permission_bits(&std::fs::metadata(&target)?), 0o200);
        Ok(())
    }

    /// Drift to a mode that denies the owner read access makes the content
    /// unreadable; it must read as a permission difference apply repairs, not
    /// as a broken entry. Root reads any file, so the case needs a non-root
    /// user to mean anything.
    #[cfg(unix)]
    #[test]
    fn drift_to_an_unreadable_mode_is_repaired() -> Result<()> {
        use std::os::unix::fs::PermissionsExt;
        if nix::unistd::geteuid().is_root() {
            return Ok(());
        }
        let dir = tempfile::tempdir()?;
        let source = dir.path().join("source");
        file::write(&source, "managed")?;
        for mode in [FileMode::Copy, FileMode::Template, FileMode::Content] {
            let target = dir.path().join(mode.name());
            let mut req = link_req(&source, &target, mode);
            req.permissions = Some(0o600);
            if mode == FileMode::Content {
                req.content = Some("managed".into());
            }
            let rendered = (mode == FileMode::Template).then_some("managed");
            apply_one(&req, rendered, &mut vec![])?;

            for drifted in [0o000, 0o200] {
                std::fs::set_permissions(&target, std::fs::Permissions::from_mode(drifted))?;
                assert_eq!(
                    check_rendered(&req, rendered)?,
                    FileState::Differs("permissions differ".into()),
                    "{} at {drifted:o}",
                    mode.name()
                );
                apply_one(&req, rendered, &mut vec![])?;
                assert_eq!(permission_bits(&std::fs::metadata(&target)?), 0o600);
                assert_eq!(file::read_to_string(&target)?, "managed");
                assert_eq!(check_rendered(&req, rendered)?, FileState::Applied);
            }
        }
        Ok(())
    }

    /// The fallback for a target that cannot be opened for reading or
    /// writing: it works on files and directories and never follows a link.
    #[cfg(unix)]
    #[test]
    fn chmod_of_an_unopenable_target_never_follows_links() -> Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir()?;
        let mode = |bits| nix::sys::stat::Mode::from_bits_truncate(bits);

        let target = dir.path().join("file");
        file::write(&target, "content")?;
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o000))?;
        chmod_unopenable_no_follow(&target, mode(0o600))?;
        assert_eq!(permission_bits(&std::fs::symlink_metadata(&target)?), 0o600);

        let directory = dir.path().join("directory");
        file::create_dir_all(&directory)?;
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o000))?;
        chmod_unopenable_no_follow(&directory, mode(0o700))?;
        assert_eq!(
            permission_bits(&std::fs::symlink_metadata(&directory)?),
            0o700
        );

        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&target, &link)?;
        assert!(chmod_unopenable_no_follow(&link, mode(0o644)).is_err());
        assert_eq!(permission_bits(&std::fs::symlink_metadata(&target)?), 0o600);
        Ok(())
    }

    fn template_req(dir: &Path, remove_empty: bool) -> Result<FileRequest> {
        let source = dir.join("source.tera");
        file::write(&source, "")?;
        let mut req = link_req(&source, &dir.join("target"), FileMode::Template);
        req.remove_empty = remove_empty;
        Ok(req)
    }

    #[test]
    fn relative_is_rejected_outside_symlink_modes() -> Result<()> {
        use crate::config::config_file::mise_toml::MiseToml;
        use std::sync::Arc;

        let path = dirs::HOME.join(".config/mise/config.toml");
        let validate = |entry: &str| -> Result<()> {
            let body = format!("[dotfiles]\n\"~/.relative-test\" = {entry}\n");
            let mut configs = ConfigMap::new();
            configs.insert(
                path.clone(),
                Arc::new(MiseToml::for_history_preflight(&body, &path)?),
            );
            validate_incoming_files(&configs)
        };
        validate(r#"{ source = "a", mode = "symlink", relative = true }"#)?;
        validate(r#"{ source = "a", mode = "symlink-each", relative = true }"#)?;
        validate(r#"{ source = "a", mode = "copy", relative = false }"#)?;
        for entry in [
            r#"{ source = "a", mode = "copy", relative = true }"#,
            r#"{ source = "a.tera", mode = "template", relative = true }"#,
            r#"{ content = "x", relative = true }"#,
            r#"{ permissions = "0600", relative = true }"#,
            r#"{ mode = "absent", relative = true }"#,
            r#"{ mode = "track", relative = true }"#,
            r#"{ block = "x", relative = true }"#,
            r#"{ line = "x", relative = false }"#,
        ] {
            assert!(validate(entry).is_err(), "{entry} should be rejected");
        }
        Ok(())
    }

    #[test]
    fn remove_empty_is_rejected_outside_template_mode() -> Result<()> {
        use crate::config::config_file::mise_toml::MiseToml;
        use std::sync::Arc;

        let path = dirs::HOME.join(".config/mise/config.toml");
        let validate = |entry: &str| -> Result<()> {
            let body = format!("[dotfiles]\n\"~/.remove-empty-test\" = {entry}\n");
            let mut configs = ConfigMap::new();
            configs.insert(
                path.clone(),
                Arc::new(MiseToml::for_history_preflight(&body, &path)?),
            );
            validate_incoming_files(&configs)
        };
        validate(r#"{ source = "a.tera", mode = "template", remove_empty = true }"#)?;
        validate(
            r#"{ source = "a.tera", mode = "template", permissions = "0600", remove_empty = true }"#,
        )?;
        for entry in [
            r#"{ permissions = "0600", remove_empty = true }"#,
            r#"{ mode = "absent", remove_empty = true }"#,
            r#"{ source = "a", mode = "copy", remove_empty = true }"#,
            r#"{ source = "a", mode = "symlink", remove_empty = true }"#,
            r#"{ content = "x", remove_empty = true }"#,
            r#"{ mode = "track", remove_empty = true }"#,
        ] {
            assert!(validate(entry).is_err(), "{entry} should be rejected");
        }

        let origin = ResourceOrigin {
            config: PathBuf::from("/mise.toml"),
            config_root: PathBuf::from("/"),
            environment: vec![],
            source: None,
        };
        let merge = |entry: &str| {
            let entry: FileTomlEntry = toml::from_str(entry).unwrap();
            let mut merged = IndexMap::new();
            merge_file_entry(
                "~/.remove-empty-test".into(),
                entry,
                Path::new("/"),
                &origin,
                &mut merged,
            );
            merged.into_values().collect::<Vec<_>>()
        };
        let accepted = merge("source = \"a.tera\"\nmode = \"template\"\nremove_empty = true");
        assert!(accepted[0].remove_empty);
        assert!(merge("source = \"a\"\nmode = \"copy\"\nremove_empty = true").is_empty());
        assert!(merge("content = \"x\"\nremove_empty = true").is_empty());
        assert!(merge("permissions = \"0600\"\nremove_empty = true").is_empty());
        assert!(merge("mode = \"absent\"\nremove_empty = true").is_empty());
        Ok(())
    }

    #[test]
    fn dot_prefix_is_rejected_outside_directory_walking_modes() -> Result<()> {
        use crate::config::config_file::mise_toml::MiseToml;
        use std::sync::Arc;

        let path = dirs::HOME.join(".config/mise/config.toml");
        let validate = |entry: &str| -> Result<()> {
            let body = format!("[dotfiles]\n\"~/.dot-prefix-test\" = {entry}\n");
            let mut configs = ConfigMap::new();
            configs.insert(
                path.clone(),
                Arc::new(MiseToml::for_history_preflight(&body, &path)?),
            );
            validate_incoming_files(&configs)
        };
        validate(r#"{ source = "a", mode = "symlink-each", dot_prefix = true }"#)?;
        validate(r#"{ source = "a", mode = "copy", dot_prefix = true }"#)?;
        validate(r#"{ source = "a", mode = "symlink", dot_prefix = false }"#)?;
        for entry in [
            r#"{ source = "a", mode = "symlink", dot_prefix = true }"#,
            r#"{ source = "a", mode = "template", dot_prefix = true }"#,
            r#"{ content = "x", dot_prefix = true }"#,
            r#"{ permissions = "0600", dot_prefix = true }"#,
            r#"{ mode = "absent", dot_prefix = true }"#,
            r#"{ mode = "track", dot_prefix = true }"#,
        ] {
            assert!(validate(entry).is_err(), "{entry} should be rejected");
        }

        let origin = ResourceOrigin {
            config: PathBuf::from("/mise.toml"),
            config_root: PathBuf::from("/"),
            environment: vec![],
            source: None,
        };
        let merge = |entry: &str| {
            let entry: FileTomlEntry = toml::from_str(entry).unwrap();
            let mut merged = IndexMap::new();
            merge_file_entry(
                "~/.dot-prefix-test".into(),
                entry,
                Path::new("/"),
                &origin,
                &mut merged,
            );
            merged.into_values().collect::<Vec<_>>()
        };
        let accepted = merge("source = \"a\"\nmode = \"symlink-each\"\ndot_prefix = true");
        assert!(accepted[0].dot_prefix);
        assert!(merge("source = \"a\"\nmode = \"symlink\"\ndot_prefix = true").is_empty());
        assert!(merge("content = \"x\"\ndot_prefix = true").is_empty());
        assert!(merge("mode = \"absent\"\ndot_prefix = true").is_empty());
        assert!(merge("mode = \"track\"\ndot_prefix = true").is_empty());
        Ok(())
    }

    #[test]
    fn dot_prefix_maps_each_dot_component() {
        let mut req = link_req(Path::new("/src"), Path::new("/home"), FileMode::SymlinkEach);
        let rel = Path::new("dot-config/foo/dot-rc");
        assert_eq!(target_rel(&req, rel), rel);
        req.dot_prefix = true;
        for (source, target) in [
            ("dot-bashrc", ".bashrc"),
            ("dot-config/foo/config.toml", ".config/foo/config.toml"),
            ("dot-config/foo/dot-rc", ".config/foo/.rc"),
            (".already", ".already"),
            ("plain/dot-", "plain/dot-"),
            ("dot-.", "dot-."),
            ("my-dot-file", "my-dot-file"),
        ] {
            assert_eq!(target_rel(&req, Path::new(source)), Path::new(target));
        }
    }

    #[test]
    fn dot_prefix_walks_and_rejects_colliding_names() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let source = dir.path().join("src");
        let target = dir.path().join("home");
        file::create_dir_all(source.join("dot-config/app"))?;
        file::write(source.join("dot-bashrc"), "")?;
        file::write(source.join("dot-config/app/config.toml"), "")?;
        file::write(source.join("dot-git.md"), "")?;
        let mut req = link_req(&source, &target, FileMode::SymlinkEach);
        req.dot_prefix = true;
        // exclude still matches source names
        req.exclude = vec![glob::Pattern::new("dot-git.md")?];
        assert_eq!(
            walk_source_files(&req)?,
            vec![
                (source.join("dot-bashrc"), target.join(".bashrc")),
                (
                    source.join("dot-config/app/config.toml"),
                    target.join(".config/app/config.toml")
                ),
            ]
        );

        file::write(source.join(".bashrc"), "")?;
        let err = walk_source_files(&req).unwrap_err().to_string();
        assert!(err.contains("both deploy to"), "{err}");
        std::fs::remove_file(source.join(".bashrc"))?;

        file::write(source.join(".config"), "")?;
        let err = walk_source_files(&req).unwrap_err().to_string();
        assert!(err.contains("to be a directory"), "{err}");
        // builds that skip apply's footprint validation still check it
        assert!(directory_source_files(&req).is_err());
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn legacy_cleanup_recognizes_relative_links() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let source = dir.path().join("src");
        let target = dir.path().join("home");
        file::create_dir_all(source.join("dot-config"))?;
        file::create_dir_all(target.join(".config"))?;
        file::write(source.join("kept"), "")?;
        let plain = target.join("gone");
        let dotted = target.join(".config/gone");
        let kept = target.join("kept");
        // dangling: their sources were removed before any state was recorded
        file::make_symlink(&relative_link_path(&source.join("gone"), &plain), &plain)?;
        file::make_symlink(
            &relative_link_path(&source.join("dot-config/gone"), &dotted),
            &dotted,
        )?;
        file::make_symlink(&relative_link_path(&source.join("kept"), &kept), &kept)?;
        assert!(std::fs::read_link(&plain)?.is_relative());

        let mut req = link_req(&source, &target, FileMode::SymlinkEach);
        req.relative = true;
        assert_eq!(legacy_stale_links(&req)?, vec![plain.clone()]);
        assert_eq!(legacy_owned_links(&req)?, vec![plain.clone(), kept.clone()]);

        req.dot_prefix = true;
        assert_eq!(
            legacy_stale_links(&req)?,
            vec![dotted.clone(), plain.clone()]
        );
        assert_eq!(legacy_owned_links(&req)?, vec![dotted, plain, kept]);
        Ok(())
    }

    #[test]
    fn dot_prefix_requires_a_directory_source() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let source = dir.path().join("dot-bashrc");
        file::write(&source, "")?;
        let mut req = link_req(&source, &dir.path().join(".bashrc"), FileMode::Copy);
        req.dot_prefix = true;
        let err = validate_composed_file_footprints(std::slice::from_ref(&req))
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("dot_prefix requires the source to be a directory"),
            "{err}"
        );
        assert!(directory_source_files(&req).is_err());
        Ok(())
    }

    #[test]
    fn an_empty_render_classifies_the_target_by_ownership() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let req = template_req(dir.path(), true)?;
        assert_eq!(empty_render_target(&req)?, EmptyRenderTarget::Absent);
        assert_eq!(check_rendered(&req, Some(" \n"))?, FileState::Applied);

        // an empty or whitespace-only file holds nothing to lose
        file::write(&req.target, " \n\t")?;
        assert_eq!(empty_render_target(&req)?, EmptyRenderTarget::Owned);

        // content with no record is someone else's
        file::write(&req.target, "work = true\n")?;
        assert!(matches!(
            empty_render_target(&req)?,
            EmptyRenderTarget::Conflict(_)
        ));
        assert!(matches!(
            check_rendered(&req, Some(""))?,
            FileState::Differs(reason) if reason.contains("--force")
        ));

        // what mise last wrote is its own to remove
        save_target_state(&req, "work = true\n");
        assert_eq!(empty_render_target(&req)?, EmptyRenderTarget::Owned);
        assert!(matches!(
            check_rendered(&req, Some("\n"))?,
            FileState::Differs(reason) if reason.contains("will be removed")
        ));

        // a later edit takes it back
        file::write(&req.target, "work = true\nedited = 1\n")?;
        assert!(matches!(
            empty_render_target(&req)?,
            EmptyRenderTarget::Conflict(_)
        ));

        file::remove_file(&req.target)?;
        file::create_dir_all(&req.target)?;
        assert_eq!(
            empty_render_target(&req)?,
            EmptyRenderTarget::Conflict("it is a directory")
        );
        remove_target_state(&req)?;
        Ok(())
    }

    #[test]
    fn an_empty_render_without_remove_empty_still_writes_the_file() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let req = template_req(dir.path(), false)?;
        assert!(!removes_target(&req, Some("")));
        assert_eq!(check_rendered(&req, Some(""))?, FileState::Missing);
        Ok(())
    }

    #[test]
    fn apply_one_records_what_it_wrote_and_removes_it_when_it_renders_empty() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let req = template_req(dir.path(), true)?;
        let mut written = vec![];
        apply_one(&req, Some("work = true\n"), &mut written)?;
        assert_eq!(file::read_to_string(&req.target)?, "work = true\n");
        assert!(target_state_matches(&req, "work = true\n"));
        // converged with a current record: nothing to record
        assert_eq!(
            template_state_update(&req, Some("work = true\n".into())),
            None
        );

        written.clear();
        apply_one(&req, Some("  \n"), &mut written)?;
        assert!(!req.target.exists());
        assert_eq!(written, vec![req.target.clone()]);
        // removed: converged, and the record still holds the last write so
        // a file brought back by undo is recognised
        assert_eq!(check_rendered(&req, Some(""))?, FileState::Applied);
        assert_eq!(template_state_update(&req, Some(String::new())), None);
        assert!(target_state_matches(&req, "work = true\n"));
        remove_target_state(&req)?;
        Ok(())
    }

    #[test]
    fn a_converged_template_without_a_record_gets_one() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let req = template_req(dir.path(), false)?;
        file::write(&req.target, "work = true\n")?;
        assert_eq!(
            template_state_update(&req, Some("work = true\n".into())),
            Some("work = true\n".into())
        );
        save_target_state(&req, "work = true\n");
        assert!(target_state_matches(&req, "work = true\n"));
        remove_target_state(&req)?;
        Ok(())
    }

    #[test]
    fn target_state_ignores_unknown_fields_and_defaults_missing_ones() -> Result<()> {
        let state: TargetState = toml::from_str("version = 1\ntarget = \"/x\"\nfuture = [1]\n")?;
        assert_eq!(
            state,
            TargetState {
                version: 1,
                target: PathBuf::from("/x"),
                content_digest: None,
                created_dirs: vec![],
            }
        );
        Ok(())
    }

    /// `home/.config` exists; `newapp/sub` under it is what mise created.
    fn created_dirs_fixture(home: &Path) -> Result<(PathBuf, Vec<PathBuf>)> {
        let newapp = home.join(".config/newapp");
        let sub = newapp.join("sub");
        file::create_dir_all(&sub)?;
        Ok((sub.join("work.toml"), vec![newapp, sub]))
    }

    /// Prune after the targets of `removals` are gone, each paired with the
    /// directories its record lists.
    fn prune_after(
        removals: &[(&Path, Vec<PathBuf>)],
        claimed: &HashSet<PathBuf>,
        home: &Path,
    ) -> Result<()> {
        let reqs = removals
            .iter()
            .map(|(target, _)| link_req(Path::new("/source"), target, FileMode::Copy))
            .collect::<Vec<_>>();
        let pairs = reqs
            .iter()
            .zip(removals)
            .map(|(req, (_, dirs))| (req, dirs.clone()))
            .collect::<Vec<_>>();
        prune_created_dirs(&pairs, claimed, home, false);
        Ok(())
    }

    #[test]
    fn removing_created_dirs_keeps_pre_existing_ones() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let home = dir.path().join("home");
        let (target, created) = created_dirs_fixture(&home)?;
        prune_after(&[(&target, created.clone())], &HashSet::new(), &home)?;
        assert!(!created[0].exists());
        assert!(home.join(".config").is_dir());
        Ok(())
    }

    #[test]
    fn removing_created_dirs_stops_at_one_holding_anything() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let home = dir.path().join("home");
        let (target, created) = created_dirs_fixture(&home)?;
        file::write(created[0].join("mine.toml"), "")?;
        prune_after(&[(&target, created.clone())], &HashSet::new(), &home)?;
        assert!(!created[1].exists());
        assert!(created[0].join("mine.toml").exists());

        // a directory still holding the target is not touched at all
        file::create_dir_all(&created[1])?;
        file::write(&target, "")?;
        prune_after(&[(&target, created.clone())], &HashSet::new(), &home)?;
        assert!(target.exists());
        Ok(())
    }

    #[test]
    fn removing_created_dirs_stops_at_one_not_recorded() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let home = dir.path().join("home");
        let (target, created) = created_dirs_fixture(&home)?;
        // only the outer one recorded: the walk never reaches it
        prune_after(&[(&target, created[..1].to_vec())], &HashSet::new(), &home)?;
        assert!(created[1].is_dir());
        // an old record with none removes nothing
        prune_after(&[(&target, vec![])], &HashSet::new(), &home)?;
        assert!(created[1].is_dir());
        Ok(())
    }

    #[test]
    fn removing_created_dirs_passes_over_one_already_gone() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let home = dir.path().join("home");
        let (target, created) = created_dirs_fixture(&home)?;
        // an earlier removal took `sub` and dropped it from the record, and
        // the user's file kept `newapp`; now that file is gone too
        file::remove_dir(&created[1])?;
        prune_after(&[(&target, created[..1].to_vec())], &HashSet::new(), &home)?;
        assert!(!created[0].exists());
        assert!(home.join(".config").is_dir());
        Ok(())
    }

    #[test]
    fn a_created_dir_that_cannot_be_checked_stays_recorded_without_failing() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let home = dir.path().join("home");
        let (target, created) = created_dirs_fixture(&home)?;
        // `sub` became a file, so looking inside it fails (ENOTDIR, even as
        // root) for a recorded directory beneath it
        file::remove_dir(&created[1])?;
        file::write(&created[1], "")?;
        let below = created[1].join("inner");
        let target_below = below.join("file");
        let mut recorded = created.clone();
        recorded.push(below.clone());
        let chain = created_dirs_to_prune(&target_below, &recorded, &home);
        assert_eq!(chain.first(), Some(&below));
        assert!(remove_created_dirs(&chain, &HashSet::new(), &home).is_empty());

        // the whole prune is best effort: nothing fails, the record keeps
        // what could not be removed, and the file in the way stays
        let source = dir.path().join("source");
        file::write(&source, "")?;
        let req = link_req(&source, &target_below, FileMode::Copy);
        record_created_dirs(&req, &recorded);
        prune_created_dirs(&[(&req, recorded.clone())], &HashSet::new(), &home, true);
        assert_eq!(recorded_created_dirs(&req), recorded);
        assert!(created[1].is_file());
        assert!(!target.exists());
        remove_target_state(&req)?;
        Ok(())
    }

    #[test]
    fn removing_created_dirs_never_removes_home_or_above() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let home = dir.path().join("home");
        file::create_dir_all(&home)?;
        let created = vec![dir.path().to_path_buf(), home.clone()];
        let target = home.join(".rc");
        assert!(created_dirs_to_prune(&target, &created, &home).is_empty());
        prune_after(&[(&target, created)], &HashSet::new(), &home)?;
        assert!(home.is_dir());
        Ok(())
    }

    #[test]
    fn removing_created_dirs_keeps_directories_outside_home() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let home = dir.path().join("home");
        file::create_dir_all(&home)?;
        // an absolute target outside home, such as /opt/newapp/file
        let newapp = dir.path().join("opt/newapp");
        file::create_dir_all(&newapp)?;
        let target = newapp.join("file");
        let created = vec![dir.path().join("opt"), newapp.clone()];
        assert!(created_dirs_to_prune(&target, &created, &home).is_empty());
        prune_after(&[(&target, created)], &HashSet::new(), &home)?;
        assert!(newapp.is_dir());
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn removing_created_dirs_keeps_one_a_symlinked_ancestor_puts_outside_home() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let home = dir.path().join("home");
        file::create_dir_all(&home)?;
        // ~/.config -> <tmp>/opt/config: ~/.config/app is really outside home
        let outside = dir.path().join("opt/config");
        file::create_dir_all(outside.join("app"))?;
        std::os::unix::fs::symlink(&outside, home.join(".config"))?;
        let app = home.join(".config/app");
        let target = app.join("file");
        prune_after(&[(&target, vec![app.clone()])], &HashSet::new(), &home)?;
        assert!(outside.join("app").is_dir());

        // a symlinked ancestor that resolves inside home: the directory is
        // checked and removed at its resolved location
        let real = home.join("real-config");
        file::create_dir_all(real.join("app"))?;
        std::os::unix::fs::symlink(&real, home.join(".linked"))?;
        let linked = home.join(".linked/app");
        prune_after(
            &[(&linked.join("file"), vec![linked.clone()])],
            &HashSet::new(),
            &home,
        )?;
        assert!(!real.join("app").exists());
        assert!(real.is_dir());

        // the same layout with a real ~/.config inside home goes
        let dir = tempfile::tempdir()?;
        let home = dir.path().join("home");
        let app = home.join(".config/app");
        file::create_dir_all(&app)?;
        prune_after(
            &[(&app.join("file"), vec![app.clone()])],
            &HashSet::new(),
            &home,
        )?;
        assert!(!app.exists());
        Ok(())
    }

    #[test]
    fn removing_sibling_targets_prunes_their_shared_parent_in_any_order() -> Result<()> {
        for owner_first in [true, false] {
            let dir = tempfile::tempdir()?;
            let home = dir.path().join("home");
            let newapp = home.join(".config/newapp");
            file::create_dir_all(&newapp)?;
            // only the first entry written into newapp recorded it
            let owner = (newapp.join("a.toml"), vec![newapp.clone()]);
            let sibling = (newapp.join("b.toml"), vec![]);
            let mut removals = vec![
                (owner.0.as_path(), owner.1),
                (sibling.0.as_path(), sibling.1),
            ];
            if !owner_first {
                removals.reverse();
            }
            prune_after(&removals, &HashSet::new(), &home)?;
            assert!(!newapp.exists(), "owner first: {owner_first}");
            assert!(home.join(".config").is_dir());
        }

        // a sibling in a deeper directory it created itself, and a shallower
        // one whose parent another record lists
        let dir = tempfile::tempdir()?;
        let home = dir.path().join("home");
        let newapp = home.join(".config/newapp");
        let deep = newapp.join("p/q");
        file::create_dir_all(&deep)?;
        file::create_dir_all(newapp.join("t"))?;
        let s = deep.join("s");
        let t = newapp.join("t/u");
        let removals = [
            (
                s.as_path(),
                vec![newapp.clone(), newapp.join("p"), deep.clone()],
            ),
            (t.as_path(), vec![newapp.join("t")]),
        ];
        prune_after(&removals, &HashSet::new(), &home)?;
        assert!(!newapp.exists());
        Ok(())
    }

    #[test]
    fn removing_created_dirs_keeps_one_another_entry_needs() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let home = dir.path().join("home");
        let (target, created) = created_dirs_fixture(&home)?;
        let claimed = HashSet::from([created[0].clone()]);
        prune_after(&[(&target, created.clone())], &claimed, &home)?;
        assert!(!created[1].exists());
        assert!(created[0].is_dir());

        // a symlink-each entry claims its target and every directory below it
        let source = dir.path().join("links");
        file::create_dir_all(source.join("nested"))?;
        file::write(source.join("nested/file"), "")?;
        let links = link_req(&source, &created[0], FileMode::SymlinkEach);
        let claimed = claimed_dirs([&links])?;
        assert!(claimed.contains(&created[0]));
        assert!(claimed.contains(&created[0].join("nested")));

        // a permissions-only entry naming a directory claims it, and never
        // records directories of its own
        let perms = permissions_req(&created[0], 0o700);
        assert!(!records_created_dirs(&perms));
        let claimed = claimed_dirs([&perms])?;
        file::create_dir_all(&created[1])?;
        prune_after(&[(&target, created.clone())], &claimed, &home)?;
        assert!(!created[1].exists());
        assert!(created[0].is_dir());
        Ok(())
    }

    #[test]
    fn an_absent_entry_prunes_what_an_earlier_entry_for_its_target_created() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let source = dir.path().join("source");
        file::write(&source, "copied")?;
        let newapp = dir.path().join("newapp");
        let target = newapp.join("sub/file");
        let copy = link_req(&source, &target, FileMode::Copy);
        apply_one(&copy, None, &mut vec![])?;

        // the entry is changed to mode = "absent": it records nothing, needs
        // nothing, and finds the copy's record under the same target
        let absent = absent_req(&target);
        assert!(!records_created_dirs(&absent));
        assert!(claimed_dirs([&absent])?.is_empty());
        assert!(apply_removes_target(&absent, None));
        let created = removal_created_dirs(&absent);
        assert_eq!(created, vec![newapp.clone(), newapp.join("sub")]);
        apply_one(&absent, None, &mut vec![])?;
        assert!(!target.exists());
        // the tempdir stands in for home
        prune_created_dirs(&[(&absent, created)], &HashSet::new(), dir.path(), true);
        assert!(!newapp.exists());
        assert!(removal_created_dirs(&absent).is_empty());
        remove_target_state(&copy)?;
        Ok(())
    }

    #[test]
    fn a_converged_removal_retries_leftover_created_dirs() -> Result<()> {
        // under the test home, since pruning never leaves it
        let dir = tempfile::Builder::new().tempdir_in(*dirs::HOME)?;
        let source = dir.path().join("source");
        file::write(&source, "copied")?;
        let newapp = dir.path().join("newapp");
        let target = newapp.join("sub/file");
        apply_one(
            &link_req(&source, &target, FileMode::Copy),
            None,
            &mut vec![],
        )?;
        // the target went but an earlier prune left the directories
        file::remove_file(&target)?;
        let absent = absent_req(&target);
        assert_eq!(check_rendered(&absent, None)?, FileState::Applied);
        assert!(apply_removes_target(&absent, None));
        assert!(has_leftover_created_dirs(&absent));

        let plan = ApplyPlan {
            todo: vec![],
            record_symlink_each: vec![],
            record_templates: vec![],
            prune_leftovers: vec![&absent],
            claimed_dirs: HashSet::new(),
            reconciliation: SymlinkEachReconciliation {
                stale_links: vec![],
                targets: vec![],
            },
        };
        prune_after_apply([], &plan);
        assert!(!newapp.exists());
        assert!(removal_created_dirs(&absent).is_empty());
        assert!(!has_leftover_created_dirs(&absent));
        remove_target_state(&absent)?;
        Ok(())
    }

    #[test]
    fn apply_one_records_the_directories_it_creates() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let source = dir.path().join("source");
        file::write(&source, "copied")?;
        let newapp = dir.path().join("newapp");
        let req = link_req(&source, &newapp.join("sub/file"), FileMode::Copy);
        apply_one(&req, None, &mut vec![])?;
        let recorded = load_target_state(&req).expect("record").created_dirs;
        assert_eq!(recorded, vec![newapp.clone(), newapp.join("sub")]);

        // a second apply creates nothing and keeps what was recorded
        apply_one(&req, None, &mut vec![])?;
        assert_eq!(
            load_target_state(&req).expect("record").created_dirs,
            recorded
        );
        assert!(
            touched_paths(&req)?
                .iter()
                .all(|(path, _)| path != &target_state_path(&req))
        );

        // a symlink-each entry shares its target with unmanaged files
        let links = dir.path().join("links");
        file::create_dir_all(&links)?;
        let each = link_req(
            &links,
            &dir.path().join("each/target"),
            FileMode::SymlinkEach,
        );
        apply_one(&each, None, &mut vec![])?;
        assert!(load_target_state(&each).is_none());

        // pruning drops what it removed from the record; the tempdir stands
        // in for home
        file::remove_file(&req.target)?;
        prune_created_dirs(
            &[(&req, recorded_created_dirs(&req))],
            &HashSet::new(),
            dir.path(),
            true,
        );
        assert!(!newapp.exists());
        assert!(
            load_target_state(&req)
                .expect("record")
                .created_dirs
                .is_empty()
        );
        remove_target_state(&req)?;
        Ok(())
    }

    #[test]
    fn a_removal_rechecks_ownership_right_before_it_runs() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let req = template_req(dir.path(), true)?;
        let mut written = vec![];
        apply_one(&req, Some("work = true\n"), &mut written)?;
        // planned as owned, then edited while the prompt waited
        assert_eq!(empty_render_target(&req)?, EmptyRenderTarget::Owned);
        file::write(&req.target, "work = true\nmine = 1\n")?;
        assert!(recheck_removal(&req, Some(""), false).is_err());
        recheck_removal(&req, Some(""), true)?;
        // a write, or an unchanged owned target, needs no recheck
        recheck_removal(&req, Some("work = true\n"), false)?;
        file::write(&req.target, "work = true\n")?;
        recheck_removal(&req, Some(""), false)?;
        remove_target_state(&req)?;
        Ok(())
    }

    #[test]
    fn creating_a_parent_journals_it_with_the_record() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let source = dir.path().join("source.tera");
        file::write(&source, "")?;
        let newapp = dir.path().join("newapp");
        let req = link_req(&source, &newapp.join("work.toml"), FileMode::Content);
        // the missing parent before the target, and the record it goes in
        let paths = touched_paths(&req)?;
        assert_eq!(paths[0], (newapp.clone(), Capture::Shallow));
        assert!(paths.contains(&(target_state_path(&req), Capture::Full)));
        Ok(())
    }

    #[test]
    fn unapply_clears_the_record_of_an_already_removed_target() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let req = template_req(dir.path(), true)?;
        let opts = UnapplyOpts {
            dry_run: false,
            verbose: false,
            force: false,
            yes: true,
        };
        assert!(plan_unapply_one(&req, &opts)?.is_none());
        save_target_state(&req, "work = true\n");
        let plan = plan_unapply_one(&req, &opts)?.expect("a plan that clears the record");
        assert!(plan.paths.is_empty());
        remove_target_state(&req)?;
        Ok(())
    }

    #[test]
    fn unapply_plans_every_single_file_kind_whose_target_is_gone_by_its_record() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let source = dir.path().join("source");
        file::write(&source, "copied")?;
        let links = dir.path().join("links");
        file::create_dir_all(&links)?;
        let opts = UnapplyOpts {
            dry_run: false,
            verbose: false,
            force: false,
            yes: true,
        };
        for mode in [FileMode::Copy, FileMode::Symlink, FileMode::Content] {
            let newapp = dir.path().join(format!("{}-app", mode.name()));
            let mut req = link_req(&source, &newapp.join("sub/file"), mode);
            if mode == FileMode::Content {
                req.content = Some("inline".into());
            }
            apply_one(&req, None, &mut vec![])?;
            assert!(!recorded_created_dirs(&req).is_empty(), "{mode:?}");
            // the user deletes the file by hand
            file::remove_file(&req.target)?;
            let plan = plan_unapply_one(&req, &opts)?.expect("a plan that clears the record");
            assert!(plan.paths.is_empty(), "{mode:?}");
            remove_target_state(&req)?;
            // without a record there is nothing to do
            assert!(plan_unapply_one(&req, &opts)?.is_none(), "{mode:?}");
        }
        // a directory copy keeps no record and plans as before
        let each = link_req(&links, &dir.path().join("each"), FileMode::Copy);
        assert!(plan_unapply_one(&each, &opts)?.is_none());
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn an_unreadable_target_is_a_conflict_force_can_clear() -> Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir()?;
        let req = template_req(dir.path(), true)?;
        file::write(&req.target, "work = true\n")?;
        save_target_state(&req, "work = true\n");
        std::fs::set_permissions(&req.target, std::fs::Permissions::from_mode(0o000))?;
        // root reads any file, so ownership is still provable there
        if std::fs::read(&req.target).is_err() {
            assert_eq!(
                empty_render_target(&req)?,
                EmptyRenderTarget::Conflict("it cannot be read to confirm mise wrote it")
            );
            assert!(matches!(
                check_rendered(&req, Some(""))?,
                FileState::Differs(reason) if reason.contains("cannot be read")
            ));
            assert!(recheck_removal(&req, Some(""), false).is_err());
            recheck_removal(&req, Some(""), true)?;
        }
        // --force removes it: removal needs only the directory to be writable
        let mut written = vec![];
        apply_one(&req, Some(""), &mut written)?;
        assert!(std::fs::symlink_metadata(&req.target).is_err());
        remove_target_state(&req)?;
        Ok(())
    }
}
