//! `[dotfiles]` and `[dotfile_groups]`.
//!
//! A config file keeps each entry as TOML ([`DotfilesTomlConfig`]), so a malformed entry warns
//! and is skipped instead of failing the whole file. mise-system reads an entry when it expands
//! the dotfiles, as a whole-file entry or an edit depending on its keys. The declaration types
//! below name every key an entry may hold; `schema/mise.json` is generated from them, and
//! mise-system's tests check that its parsers read the same keys.

pub(crate) mod schema;

use indexmap::IndexMap;
use schemars::JsonSchema;
use serde::Deserialize;

/// `[dotfile_groups]` as written in one config file: group name -> table.
/// Values stay TOML until expansion so a malformed group warns and is
/// skipped instead of failing the whole config file.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct DotfileGroupsTomlConfig(pub IndexMap<String, toml::Value>);

/// `[dotfiles]` as parsed from a single mise.toml.
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(transparent)]
pub struct DotfilesTomlConfig(pub IndexMap<String, toml::Value>);

// The doc comments on these types are their descriptions in schema/mise.json, which editors show
// to users. Notes for Rust readers go in plain comments.

// One `[dotfiles]` value.
/// a dotfile: the source path of a whole-file entry, or a table describing a whole-file entry or an edit
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(untagged)]
#[schemars(transform = crate::schema::one_of)]
pub enum DotfileEntryDecl {
    /// source path for a whole-file dotfile entry
    Source(String),
    Table(Box<DotfileTableDecl>),
}

/// whole-file dotfile entry or file edit entry
// Whole-file entries and edits share one table: which one it is follows from its keys. The
// rules between keys are written by hand in `schema.rs`.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(transform = schema::entry_rules)]
pub struct DotfileTableDecl {
    /// encrypt tracked file contents before storing them in Git; the live file remains plaintext
    #[serde(default)]
    pub encrypt: bool,
    /// allow this directly tracked credential-named file to be saved in plaintext history and any connected origin
    #[serde(default)]
    pub allow_plaintext: bool,
    /// source file or directory
    pub source: Option<String>,
    /// literal inline file content; alternative to source
    pub content: Option<String>,
    /// how to apply a whole-file source to the target; omitted uses dotfiles.default_mode. "absent" removes a file or symlink at the target and takes no source. "track" saves the file's history where it is, and "track-local" saves it in this machine's local-only history, which is never shared
    // A string, so a mode from a newer mise warns and is skipped.
    #[schemars(extend("anyOf" = [
        {"enum": ["symlink", "symlink-each", "copy", "template", "absent", "track", "track-local"]},
        {"type": "string"}
    ]))]
    pub mode: Option<String>,
    /// glob patterns to skip when the mode walks a directory (symlink-each, copy with a directory source, or a tracked directory, where they are relative to the tracked path); a pattern without "/" matches any path component, one with "/" is anchored to the root
    pub exclude: Option<Vec<String>>,
    /// history only (mode = "track"): the only paths of the tracked directory that are captured, relative to it and matched like "exclude"; without it the whole tree is captured with the builtin credential filtering, and an explicit "exclude" still wins. A selected file is captured even when it looks like a credential store, in plaintext unless the entry sets encrypt
    pub include: Option<Vec<String>>,
    /// source manifest used to select managed files from a directory
    #[schemars(extend("enum" = ["git"]))]
    pub manifest: Option<String>,
    /// template mode only: remove the target when the template renders empty or whitespace-only; a target that changed since mise last wrote it needs --force
    #[serde(default)]
    pub remove_empty: bool,
    /// symlink-each and directory copy only: deploy each source path component named "dot-<name>" as ".<name>", like GNU Stow's --dotfiles; exclude still matches source names
    #[serde(default)]
    pub dot_prefix: bool,
    /// symlink and symlink-each only: link by a path relative to the link's directory instead of an absolute one; overrides the dotfiles.relative_symlinks setting (ignored on Windows)
    pub relative: Option<bool>,
    /// octal permissions for the target, such as "0600"; overrides what copy, template, and inline content entries set, and on its own manages only the permissions of an existing target (Unix only)
    #[schemars(regex(pattern = r"^(0o)?[0-7]{1,4}$"))]
    pub permissions: Option<String>,
    /// track only: false saves the path only on `mise dotfiles save <path>`
    pub autosave: Option<bool>,
    /// false turns off an entry inherited from another config file
    pub enabled: Option<bool>,
    /// block content, maintained between marker comments
    pub block: Option<String>,
    /// template engine to render block content with; currently only "tera"
    #[schemars(extend("anyOf" = [{"enum": ["tera"]}, {"type": "string"}]))]
    pub template: Option<String>,
    /// exact line to ensure exists
    pub line: Option<String>,
    /// where to insert a missing line
    #[schemars(extend("default" = "append", "anyOf" = [
        {"enum": ["append", "prepend"]},
        {"type": "string"}
    ]))]
    pub position: Option<String>,
    /// comment prefix for edit marker lines; inferred from the file extension when omitted
    pub comment: Option<String>,
    /// edit entry: true sets the keys of source (default: the target's path under dotfiles.root) on a JSON, TOML, or YAML target and leaves the target's other keys to the application that also writes it; "missing" sets only the keys the target has no value for; the format comes from the target's extension
    #[serde(default)]
    #[schemars(schema_with = "schema::merge")]
    pub merge: Option<MergeSetting>,
    /// OS/profile variants: history streams for track mode, or optional destinations for deployment modes
    pub variants: Option<Vec<DotfileVariantDecl>>,
}

/// The value of an entry's `merge` key: `true`, or `"missing"` to set only absent keys.
// A string other than "missing" parses, so a newer mode warns instead of failing.
#[derive(Debug, Clone, Eq, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum MergeSetting {
    Bool(bool),
    Mode(String),
}

// One `variants = [{ … }]` element of a `[dotfiles]` entry.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DotfileVariantDecl {
    /// operating system (linux, macos, windows, or unix), optionally with an architecture such as linux/arm64; a list matches any of its values
    #[serde(default)]
    #[schemars(schema_with = "schema::one_or_many_strings")]
    pub os: Option<Vec<String>>,
    /// mise environment selected with -E or MISE_ENV
    pub profile: Option<String>,
    /// true marks the fallback for machines that match no other variant; takes no os or profile
    pub default: Option<bool>,
    /// track mode only: a separate stream on every machine, never applied on another machine; must be the only variant and cannot be combined with encrypt
    pub machine: Option<bool>,
    /// deployment destination override; requires either an explicit source or a safe relative entry key when every variant has a target, and is not supported in track mode
    pub target: Option<String>,
}

// One `[dotfile_groups.<name>]` table.
/// a directory tree of dotfiles deployed into a target directory
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DotfileGroupDecl {
    /// the directory tree the group deploys; a relative path resolves against dotfiles.root
    pub root: String,
    /// target directory, absolute or starting with ~/
    #[schemars(extend("default" = "~"))]
    pub target: Option<String>,
    /// how the tree is deployed
    #[schemars(extend("enum" = ["symlink-each", "copy", "symlink"], "default" = "symlink-each"))]
    pub mode: Option<String>,
    /// glob patterns of source paths to skip, as for [dotfiles] exclude
    pub exclude: Option<Vec<String>>,
    /// deploy each source path component named "dot-<name>" as ".<name>", like GNU Stow's --dotfiles
    #[serde(default)]
    pub dot_prefix: bool,
    /// source manifest used to select managed files from the tree
    #[schemars(extend("enum" = ["git"]))]
    pub manifest: Option<String>,
    /// link by a path relative to the link's directory; overrides the dotfiles.relative_symlinks setting (ignored on Windows)
    pub relative: Option<bool>,
    /// whole-file entries for parts of the tree, keyed by target path inside the group's target and written like [dotfiles] entries; each is cut out of the walk. A missing source is found under the root at the entry's path inside the target, and a relative source starts at the root
    pub entries: Option<IndexMap<String, DotfileGroupEntryDecl>>,
}

// One `[dotfile_groups.<name>.entries]` value.
/// a whole-file entry for a path inside the group's target: its source path, or a table as in [dotfiles]
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(untagged)]
#[schemars(transform = crate::schema::one_of)]
pub enum DotfileGroupEntryDecl {
    /// source path, relative to the group root
    Source(String),
    /// whole-file entry, as in [dotfiles]
    Table(#[schemars(schema_with = "schema::any_table")] toml::Table),
}
