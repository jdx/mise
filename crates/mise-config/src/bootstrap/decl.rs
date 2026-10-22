//! The declaration types behind the schema for the `[bootstrap]` fields that stay raw TOML.
//!
//! Hooks, shell activation, the macOS defaults domains and the apt and dnf repositories are kept
//! as TOML values so a shape from a newer mise warns instead of failing the whole file, so
//! nothing parses into these types: they describe what the code that reads those values accepts.
//! The derived `[bootstrap]` schema points at them through [`refs`](crate::schema::refs). The doc
//! comments are the descriptions in `schema/mise.json`; notes for Rust readers go in plain
//! comments.

use std::collections::BTreeMap;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};

use crate::schema::refs;

/// When to apply the managed path during bootstrap; pre-packages runs before the pre-packages hook
#[derive(JsonSchema)]
#[serde(rename_all = "kebab-case")]
#[schemars(extend("default" = "post-packages"))]
pub enum BootstrapFilePhase {
    PrePackages,
    PostPackages,
}

/// command or commands to run for a bootstrap phase
#[derive(JsonSchema)]
#[serde(untagged)]
#[schemars(transform = crate::schema::one_of)]
pub enum BootstrapHook {
    One(String),
    Many(Vec<String>),
    Table(BootstrapHookTable),
}

// Extra keys are tolerated, so a hook from a newer mise still loads.
#[derive(JsonSchema)]
#[schemars(extend("additionalProperties" = true))]
pub struct BootstrapHookTable {
    /// shell command or commands to run in order, with the default inline shell; Tera templates are rendered first
    pub run: BootstrapHookRun,
}

#[derive(JsonSchema)]
#[serde(untagged)]
#[schemars(transform = crate::schema::one_of)]
pub enum BootstrapHookRun {
    One(String),
    Many(Vec<String>),
}

// The phases a hook can run at, each spelled with `-` or `_`. mise-system's `BootstrapHookPhase`
// is the list the code reads, and a test there checks this one against it.
pub const BOOTSTRAP_HOOK_PHASES: &[&str] = &[
    "pre-packages",
    "post-packages",
    "pre-repos",
    "post-repos",
    "pre-dotfiles",
    "post-dotfiles",
    "pre-defaults",
    "post-defaults",
    "pre-user",
    "post-user",
    "pre-tools",
    "post-tools",
    "final",
];

/// commands to run before and after bootstrap phases
// The phases are properties, written by `named_phases` from the list above, so every phase is
// offered in both spellings.
#[derive(JsonSchema)]
#[schemars(transform = named_phases)]
pub struct BootstrapHooksDecl {}

fn named_phases(schema: &mut Schema) {
    let hook = serde_json::json!({"$ref": "#/$defs/bootstrap_hook"});
    let mut properties = serde_json::Map::new();
    for phase in BOOTSTRAP_HOOK_PHASES {
        properties.insert((*phase).into(), hook.clone());
        let underscored = phase.replace('-', "_");
        if underscored != *phase {
            properties.insert(underscored, hook.clone());
        }
    }
    schema.insert("properties".into(), properties.into());
    schema.insert("additionalProperties".into(), false.into());
}

// How mise manages the activation block of one shell or shell file.
#[derive(JsonSchema)]
#[serde(untagged)]
#[schemars(transform = crate::schema::one_of)]
pub enum BootstrapShellActivation {
    /// enable the target with its default mode, or disable it
    Enabled(bool),
    /// enable the target with the selected activation mode
    Mode(ActivationMode),
    /// shell activation settings for the target
    Table(ShellActivationTable),
}

#[derive(JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ActivationMode {
    Activate,
    Shims,
    Auto,
}

#[derive(JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ShellActivationTable {
    /// whether to manage the activation block
    pub enabled: bool,
    /// activation mode; defaults according to the target
    pub mode: Option<ActivationMode>,
}

/// declarative mise shell activation snippets, applied with `mise bootstrap mise-shell-activate apply`
#[derive(JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BootstrapMiseShellActivate {
    #[serde(default)]
    /// manage the activation blocks in ~/.bash_profile (shims) and ~/.bashrc (activate); a mode applies to both files
    #[schemars(schema_with = "refs::bootstrap_shell_activation")]
    pub bash: Option<BootstrapShellActivation>,
    #[serde(default)]
    /// manage the activation blocks in ~/.zprofile (shims) and ~/.zshrc (activate); a mode applies to both files, and ~/.zshenv is never written
    #[schemars(schema_with = "refs::bootstrap_shell_activation")]
    pub zsh: Option<BootstrapShellActivation>,
    #[serde(default)]
    /// manage the activation block in ~/.config/fish/config.fish; activate by default, and auto activates interactive shells and uses shims otherwise
    #[schemars(schema_with = "refs::bootstrap_shell_activation")]
    pub fish: Option<BootstrapShellActivation>,
    #[serde(default)]
    /// manage the activation block in ~/.bash_profile; shims by default
    #[schemars(schema_with = "refs::bootstrap_shell_activation")]
    pub bash_profile: Option<BootstrapShellActivation>,
    #[serde(default)]
    /// manage the activation block in ~/.bashrc; activate by default
    #[schemars(schema_with = "refs::bootstrap_shell_activation")]
    pub bashrc: Option<BootstrapShellActivation>,
    #[serde(default)]
    /// manage the activation block in ~/.zshenv, which zsh reads for every script; shims by default
    #[schemars(schema_with = "refs::bootstrap_shell_activation")]
    pub zshenv: Option<BootstrapShellActivation>,
    #[serde(default)]
    /// manage the activation block in ~/.zprofile; shims by default
    #[schemars(schema_with = "refs::bootstrap_shell_activation")]
    pub zprofile: Option<BootstrapShellActivation>,
    #[serde(default)]
    /// manage the activation block in ~/.zshrc; activate by default
    #[schemars(schema_with = "refs::bootstrap_shell_activation")]
    pub zshrc: Option<BootstrapShellActivation>,
}

/// apt repositories added before apt packages are installed, keyed by a name used in the files mise writes
#[derive(JsonSchema)]
#[schemars(extend("propertyNames" = {"pattern": "^[A-Za-z0-9_-][A-Za-z0-9_.-]*$"}))]
pub struct BootstrapAptSources(pub BTreeMap<String, AptSourceDecl>);

/// apt source: a Launchpad PPA, or repository URIs with the key that signs them
// A PPA stands alone; any other source names its URIs and the key that signs them.
#[derive(JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("oneOf" = [
    {
        "required": ["ppa"],
        "properties": {"fingerprint": {"type": "string"}},
        "not": {"anyOf": [
            {"required": ["uris"]},
            {"required": ["suites"]},
            {"required": ["components"]},
            {"required": ["architectures"]},
            {"required": ["key"]}
        ]}
    },
    {"required": ["uris", "key", "fingerprint"], "not": {"required": ["ppa"]}}
]))]
pub struct AptSourceDecl {
    /// Launchpad PPA as owner/archive, such as git-core/ppa; added on Ubuntu and Ubuntu-based releases only
    pub ppa: Option<String>,
    /// repository URL(s), space-separated
    pub uris: Option<String>,
    /// suite(s), space-separated; defaults to the release codename
    pub suites: Option<String>,
    /// component(s), space-separated; defaults to main, or none when suites is a path ending in /
    pub components: Option<String>,
    /// architecture(s) to fetch, space-separated
    pub architectures: Option<String>,
    /// http(s) URL of the repository's signing key, armored or binary
    pub key: Option<String>,
    /// full primary key fingerprint(s) the signing key must have; required with key
    pub fingerprint: Option<OneOrManyStrings>,
}

#[derive(JsonSchema)]
#[serde(untagged)]
#[schemars(transform = crate::schema::one_of)]
pub enum OneOrManyStrings {
    One(String),
    Many(Vec<String>),
}

/// dnf repositories enabled before dnf packages are installed, keyed by a name for the entry
#[derive(JsonSchema)]
#[schemars(extend("propertyNames" = {"pattern": "^[A-Za-z0-9_-][A-Za-z0-9_.-]*$"}))]
pub struct BootstrapDnfRepos(pub BTreeMap<String, DnfRepoDecl>);

/// dnf repository to enable, given as a COPR project
#[derive(JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DnfRepoDecl {
    /// COPR project as owner/project or @group/project, such as scottames/ghostty; enabled as dnf copr enable would
    pub copr: String,
}

/// macOS user defaults to apply with `mise bootstrap macos defaults apply`, keyed by preferences domain (e.g. "com.apple.dock", "NSGlobalDomain")
#[derive(JsonSchema)]
pub struct BootstrapMacosDefaults(pub BTreeMap<String, MacosDefaultsDomain>);

/// defaults keys and their desired values for this domain
#[derive(JsonSchema)]
pub struct MacosDefaultsDomain(pub BTreeMap<String, MacosDefaultsEntry>);

/// desired value, written with the matching `defaults write` type; arrays and tables become property list arrays and dictionaries
#[derive(JsonSchema)]
pub struct MacosDefaultsEntry(#[schemars(schema_with = "refs::macos_defaults_value")] ());

/// a property list value: boolean, integer, real, string, or an array or dictionary of these
// Recursive, so the nested values point at this definition by name.
#[derive(JsonSchema)]
#[serde(untagged)]
pub enum MacosDefaultsValue {
    Boolean(bool),
    Integer(i64),
    Real(f64),
    String(String),
    Array(#[schemars(schema_with = "macos_values_array")] ()),
    Table(#[schemars(schema_with = "macos_values_table")] ()),
}

fn macos_values_array(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "array", "items": {"$ref": "#/$defs/macos_defaults_value"}})
}

fn macos_values_table(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "type": "object",
        "additionalProperties": {"$ref": "#/$defs/macos_defaults_value"}
    })
}

/// curated Dock preferences that compile into macOS defaults
#[derive(JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MacosDock {
    /// Pinned application paths in order; absolute or ~/ paths ending in .app. An empty array removes application tiles.
    #[schemars(inner(regex(pattern = r"^(?:/|~/(?!/)).+\.[aA][pP][pP]/*$")), extend("uniqueItems" = true))]
    pub apps: Option<Vec<String>>,
    /// delay before showing the Dock
    pub autohide_delay: Option<f64>,
    /// Dock autohide animation time modifier
    pub autohide_time_modifier: Option<f64>,
    /// hide and show the Dock automatically
    pub autohide: Option<bool>,
    /// Dock screen edge
    pub orientation: Option<DockOrientation>,
    /// Dock icon size
    pub tilesize: Option<i64>,
    /// enable Dock magnification
    pub magnification: Option<bool>,
    /// Dock magnified icon size
    pub largesize: Option<i64>,
    /// show recent applications in the Dock
    pub show_recents: Option<bool>,
    /// automatically rearrange Spaces based on most recent use
    pub mru_spaces: Option<bool>,
}

#[derive(JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum DockOrientation {
    Bottom,
    Left,
    Right,
}

/// curated Finder preferences that compile into macOS defaults
#[derive(JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MacosFinder {
    /// keep folders first when sorting by name
    pub sort_folders_first: Option<bool>,
    /// save new documents to iCloud by default (applies globally to application save dialogs)
    pub save_new_documents_to_cloud: Option<bool>,
    /// show hidden files in Finder
    pub show_all_files: Option<bool>,
    /// show the Finder path bar
    pub show_pathbar: Option<bool>,
    /// show the Finder status bar
    pub show_status_bar: Option<bool>,
    /// show the warning when changing file extensions
    pub show_extensions_warning: Option<bool>,
    /// Finder preferred view style
    pub preferred_view_style: Option<FinderViewStyle>,
}

#[derive(JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum FinderViewStyle {
    Icon,
    List,
    Column,
    Gallery,
}

/// curated keyboard preferences that compile into macOS defaults
#[derive(JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MacosKeyboard {
    /// automatically capitalize words
    pub automatic_capitalization: Option<bool>,
    /// automatically correct spelling
    pub automatic_spelling_correction: Option<bool>,
    /// keyboard repeat interval
    pub key_repeat: Option<i64>,
    /// delay before key repeat starts
    pub initial_key_repeat: Option<i64>,
    /// enable press-and-hold accent picker
    pub press_and_hold: Option<bool>,
    /// use F1, F2, etc. as standard function keys
    pub fn_state: Option<bool>,
}

/// curated trackpad preferences that compile into macOS defaults
#[derive(JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MacosTrackpad {
    /// enable tap to click for built-in and Bluetooth trackpads
    pub tap_to_click: Option<bool>,
    /// enable three finger drag for built-in and Bluetooth trackpads
    pub three_finger_drag: Option<bool>,
}
