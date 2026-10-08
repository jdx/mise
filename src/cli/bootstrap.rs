use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use eyre::{Result, bail};
use futures_util::future::LocalBoxFuture;
use heck::ToKebabCase;
use serde::Serialize;
use serde_json::{Value, json};
use usage_rs::spec::ValueEnum;

use super::dotfiles::{Dotfiles, DotfilesApply, write_and_reload};
use super::install::Install;
use super::plugins::install::install_plugin;
use super::run;
use super::system::{export, import, install, prune, status, upgrade, r#use};
use crate::config::{self, Config, Settings, SettingsExt};
use crate::dirs;
use crate::path::PathExt;
use crate::system;
use crate::system::defaults::DefaultsState;
use crate::system::driver::{self, Action, DriverOpts};
use crate::system::files::{FileMode, FileRequest, FileState};
use crate::system::history::store::Summary;
use crate::system::history::{OperationScope, journal};
use crate::system::hooks::{self, BootstrapHookPhase};
use crate::system::launchd::LaunchdState;
use crate::system::login_shell::LoginShellState;
use crate::system::packages::{PackageDesiredState, PackageState};
use crate::system::repos::RepoState;
use crate::system::resources::{ResourceAction, ResourceId};
use crate::toolset::ResolveOptions;
use crate::ui::prompt::Confirmation;
use crate::ui::table::MiseTable;

/// Set up this machine from your mise config
///
/// Applies each part your config declares, in this order. Each step names its
/// parts as `--only` and `--skip` accept them, followed by any condition.
///
/// 1. `accounts`, then `plugins` (package manager plugins)
/// 2. `files` with `phase = "pre-packages"`, then built-in `packages`
/// 3. The remaining `files`, then `services`, `firewall`, and `compose`
/// 4. `repos`, then `dotfiles`
/// 5. `mise-shell-activate`, `macos-defaults`, and `macos-launchd-agents`
/// 6. `linux-systemd-units`, then `user` (the login shell)
/// 7. `tools`
/// 8. Plugin `packages`, then user `services` with `requires_tools = true`
/// 9. `task` (the `bootstrap` task, if defined), then `final-hook`
///
/// Hooks run before and after packages, repos, dotfiles, macOS defaults, the
/// login shell, and tools. The post-packages hook runs right after built-in
/// packages, or after plugin packages when any are configured. See
/// https://mise.jdx.dev/bootstrap.html#how-it-runs.
///
/// Parts that already match the config are left alone, but hooks and the
/// `bootstrap` task run every time, so make them safe to repeat. Checking
/// dotfile state can render templates, which may run `exec()`.
///
/// Preview with `--dry-run`. To inspect without applying, use
/// `mise bootstrap status` for current state or `mise bootstrap plan` for
/// resource-level changes.
#[derive(Debug, usage_rs::Args)]
#[usage(
    verbatim_doc_comment,
    example("mise bootstrap --dry-run", help = "Preview every configured part"),
    example("mise bootstrap", help = "Apply every configured part"),
    example(
        "mise bootstrap --only dotfiles,tools",
        help = "Apply only dotfiles and tools"
    ),
    example(
        "mise bootstrap --skip tools,task",
        help = "Apply everything except tools and the bootstrap task"
    ),
    example(
        "mise bootstrap --force-dotfiles",
        help = "Replace files that conflict with whole-file dotfile entries"
    ),
    example(
        "mise bootstrap --from git@github.com:example/workstation.git",
        help = "Clone a bootstrap project and apply it"
    ),
    example(
        "mise -E work bootstrap --from git@github.com:example/workstation.git",
        help = "Do the same and also load the project's mise.work.toml"
    ),
    example(
        "mise bootstrap --adopt example/mise-config",
        help = "Clone global config into ~/.config/mise, then apply it"
    ),
    example(
        "mise bootstrap --adopt example/setup --replace-history --yes",
        help = "Use a setup repository's dotfile history instead of this machine's"
    ),
    example(
        "mise bootstrap --update",
        help = "Refresh package metadata and fast-forward repositories first"
    )
)]
pub(crate) struct Bootstrap {
    #[usage(subcommand)]
    command: Option<Commands>,

    /// Clone a bootstrap project (a Git repository with a mise.toml) and apply it
    ///
    /// The checkout is reused on later runs; add `--update` to fast-forward it
    /// first. Append `?ref=<branch|tag|commit>` to check out a ref instead of the
    /// default branch, for example
    /// `https://github.com/example/workstation.git?ref=v1`. See
    /// https://mise.jdx.dev/bootstrap/from-repository.html.
    #[usage(long, value_name = "GIT_URL")]
    from: Option<String>,

    /// Clone a repository of global mise config or dotfile history, then apply it
    ///
    /// A repository of global config (`config.toml`, `conf.d/`, `tasks/`) is
    /// cloned into the global config directory, normally ~/.config/mise. A setup
    /// repository shared with `mise dotfiles origin set` restores its tracked
    /// files instead. `OWNER/REPO` is short for
    /// `https://github.com/OWNER/REPO.git`.
    #[usage(long, value_name = "GIT_URL|OWNER/REPO", conflicts = "from")]
    adopt: Option<String>,

    /// Discard this machine's dotfile history and adopt the repository's
    ///
    /// Use it with `--adopt` when the two histories are unrelated. Files that
    /// differ from the repository still stop the adoption until you resolve them,
    /// or add `--take-remote-all` to take the repository's version of each.
    #[usage(long, requires = "adopt")]
    replace_history: bool,

    /// Take the setup repository's version of every existing file that differs while adopting
    ///
    /// The replaced versions are saved first, so `mise dot undo` restores them.
    #[usage(long, requires = "adopt")]
    take_remote_all: bool,

    /// Directory for the `--from` checkout (default: `$MISE_DATA_DIR/bootstrap-repo`)
    #[usage(long, value_name = "DIR", requires = "from")]
    from_dir: Option<PathBuf>,

    /// Show what would change without changing anything
    ///
    /// Hooks and the `bootstrap` task are printed instead of run.
    #[usage(long, short = 'n')]
    dry_run: bool,

    /// Skip confirmation prompts
    #[usage(long, short = 'y')]
    yes: bool,

    /// Skip configured repositories with local changes instead of failing
    #[usage(long)]
    skip_dirty: bool,

    /// Overwrite existing files that conflict with whole-file dotfile entries
    #[usage(long)]
    force_dotfiles: bool,

    /// Run only these parts
    ///
    /// Repeat the flag or separate parts with commas. Cannot be combined with
    /// `--skip`.
    #[usage(
        long,
        value_enum,
        value_name = "PART",
        delimiter = ',',
        conflicts = "skip"
    )]
    only: Vec<BootstrapPartArg>,

    /// Prompt securely for missing bootstrap secret inputs
    #[usage(long)]
    prompt_secrets: bool,

    /// Ask for `[vars]` entries that declare a `prompt` and have no saved answer
    ///
    /// mise saves each answer under `$MISE_STATE_DIR`, not in any config file,
    /// and never asks for it again. Without a terminal, a var keeps its default.
    #[usage(long)]
    prompt_vars: bool,

    /// Skip these parts
    ///
    /// Repeat the flag or separate parts with commas. Cannot be combined with
    /// `--only`.
    #[usage(long, value_enum, value_name = "PART", delimiter = ',')]
    skip: Vec<BootstrapPartArg>,

    /// Refresh package metadata and update repositories before applying
    ///
    /// Also fast-forwards a checkout that `--from` or `--adopt` reuses. Without
    /// `--update`, a reused checkout stays at its current commit.
    #[usage(long)]
    update: bool,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, usage_rs::ValueEnum)]
enum BootstrapPart {
    Plugins,
    Packages,
    Accounts,
    Files,
    Services,
    Firewall,
    Compose,
    Repos,
    Dotfiles,
    #[usage(name = "mise-shell-activate", alias = "shell")]
    Shell,
    #[usage(name = "macos-defaults", alias = "defaults")]
    Defaults,
    #[usage(name = "macos-launchd-agents", alias = "launchd")]
    Launchd,
    #[usage(name = "linux-systemd-units", alias = "systemd")]
    Systemd,
    User,
    Tools,
    Task,
    FinalHook,
}

impl BootstrapPart {
    // Keep this in sync with every enum variant. `--only` computes a
    // complement from ALL, so an omitted variant would always run.
    const ALL: [Self; 17] = [
        Self::Plugins,
        Self::Packages,
        Self::Accounts,
        Self::Files,
        Self::Services,
        Self::Firewall,
        Self::Compose,
        Self::Repos,
        Self::Dotfiles,
        Self::Shell,
        Self::Defaults,
        Self::Launchd,
        Self::Systemd,
        Self::User,
        Self::Tools,
        Self::Task,
        Self::FinalHook,
    ];
}

/// One `--only`/`--skip` word: the part it names, and the legacy alias it
/// was spelled with, if any. The aliases parse to the same part, so the
/// word is kept here to warn about the old spelling.
#[derive(Clone, Copy, Debug)]
struct BootstrapPartArg {
    part: BootstrapPart,
    alias: Option<&'static str>,
}

impl ValueEnum for BootstrapPartArg {
    const CHOICES: &'static [&'static str] = BootstrapPart::CHOICES;
    const ACCEPTED_CHOICES: &'static [&'static str] = BootstrapPart::ACCEPTED_CHOICES;
    const ALIASES: &'static [(&'static str, &'static str)] = BootstrapPart::ALIASES;
    const DETAILS: &'static [usage_rs::spec::ChoiceMeta<'static>] = BootstrapPart::DETAILS;
    const IGNORE_CASE: bool = BootstrapPart::IGNORE_CASE;

    fn from_choice(value: &str) -> Option<Self> {
        let part = BootstrapPart::from_choice(value)?;
        let alias = BootstrapPart::ALIASES
            .iter()
            .find(|(_, alias)| *alias == value)
            .map(|&(_, alias)| alias);
        Some(Self { part, alias })
    }
}

/// Warn about the part names `--only` and `--skip` accepted before the
/// parts were renamed, such as `launchd` for `macos-launchd-agents`.
fn warn_legacy_part_aliases(only: &[BootstrapPartArg], skip: &[BootstrapPartArg]) {
    let renames = only
        .iter()
        .chain(skip)
        .filter_map(|arg| {
            let alias = arg.alias?;
            Some(format!(
                "`{alias}` with `{}`",
                bootstrap_part_name(&arg.part)
            ))
        })
        .collect::<indexmap::IndexSet<_>>();
    if renames.is_empty() {
        return;
    }
    deprecated_at!(
        "2026.10.4",
        "2027.10.4",
        "bootstrap.part_aliases",
        "Legacy bootstrap part names in --only/--skip are deprecated. Replace {}.",
        renames.into_iter().collect::<Vec<_>>().join(", ")
    );
}

/// Warn about `mise bootstrap launchd`, `systemd` and `macos-defaults`, the
/// spellings from before the commands moved under `macos` and `linux`.
fn warn_legacy_bootstrap_command(id: &'static str, legacy: &str, replacement: &str) {
    deprecated_at!(
        "2026.10.4",
        "2027.10.4",
        id,
        "`mise bootstrap {legacy}` is deprecated. Use `mise bootstrap {replacement}` instead."
    );
}

type BootstrapPredictionGraph = HashMap<ResourceId, (ResourceAction, Vec<ResourceId>)>;

fn run_bootstrap_git<const N: usize>(checkout: &Path, args: [&str; N]) -> Result<()> {
    let mut command = Command::new("git");
    command.arg("-C").arg(checkout).args(args);
    crate::git::sanitize_git_command(&mut command);
    let status = command.status()?;
    if !status.success() {
        bail!("git command failed with {status}");
    }
    Ok(())
}

fn validate_bootstrap_checkout(checkout: &Path, url: &str) -> Result<()> {
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(checkout)
        .args(["config", "--get", "remote.origin.url"]);
    crate::git::sanitize_git_command(&mut command);
    let output = command.output()?;
    if !output.status.success() {
        bail!(
            "{} exists but is not a git checkout with an origin remote",
            checkout.display_user()
        );
    }
    let origin = String::from_utf8(output.stdout)?.trim().to_string();
    if origin != url {
        bail!(
            "{} has origin {origin:?}, expected {url:?}",
            checkout.display_user()
        );
    }
    Ok(())
}

fn bootstrap_resource_is_skipped(resource: &ResourceId, skip: &HashSet<BootstrapPart>) -> bool {
    let part = match resource.kind.as_str() {
        "package" => BootstrapPart::Packages,
        "file" | "directory" => BootstrapPart::Files,
        "service" => BootstrapPart::Services,
        "firewall" | "firewall-rule" => BootstrapPart::Firewall,
        "user" | "group" => BootstrapPart::Accounts,
        _ => return false,
    };
    skip.contains(&part)
}

fn bootstrap_prediction_has_skipped_change(
    resource: &ResourceId,
    resources: &BootstrapPredictionGraph,
    skip: &HashSet<BootstrapPart>,
    visited: &mut HashSet<ResourceId>,
) -> bool {
    if !visited.insert(resource.clone()) {
        return false;
    }
    let Some((_, dependencies)) = resources.get(resource) else {
        return false;
    };
    dependencies.iter().any(|dependency| {
        let dependency_changes = resources.get(dependency).is_some_and(|(action, _)| {
            matches!(
                action,
                ResourceAction::Create | ResourceAction::Update | ResourceAction::Remove
            )
        });
        (dependency_changes && bootstrap_resource_is_skipped(dependency, skip))
            || bootstrap_prediction_has_skipped_change(dependency, resources, skip, visited)
    })
}

impl Bootstrap {
    /// `mise bootstrap dotfiles watch`, the watcher under its other name.
    ///
    /// A setup source alongside a subcommand is rejected by `run`, so an
    /// invocation carrying one is not a watcher starting, whatever it names.
    /// The dotfile watcher, and the launcher a Windows user service's task
    /// starts. Both are started by a service manager with nobody reading
    /// their output, and on Windows both are handed a console whose window
    /// has to go before anything slow happens — the window can be closed,
    /// and closing it kills the process behind it.
    pub(crate) fn runs_unattended(&self) -> bool {
        self.from.is_none()
            && self.adopt.is_none()
            && match &self.command {
                Some(Commands::Dotfiles(cmd)) => cmd.is_watch(),
                Some(Commands::ServiceExec(_)) => true,
                _ => false,
            }
    }
}

#[derive(Debug, usage_rs::Subcommands)]
enum Commands {
    #[usage(name = "__apply-account-plan", hide = true)]
    ApplyAccountPlan(BootstrapApplyAccountPlan),
    #[usage(name = "__apply-service-plan", hide = true)]
    ApplyServicePlan(BootstrapApplyServicePlan),
    #[usage(name = "__apply-firewall-plan", hide = true)]
    ApplyFirewallPlan(BootstrapApplyFirewallPlan),
    #[usage(name = "__apply-system-plan", hide = true)]
    ApplySystemPlan(BootstrapApplySystemPlan),
    #[usage(name = "__inspect-system-files", hide = true)]
    InspectSystemFiles(BootstrapInspectSystemFiles),
    #[usage(name = "__inspect-firewall-plan", hide = true)]
    InspectFirewallPlan(BootstrapInspectFirewallPlan),
    #[usage(name = "__service-exec", hide = true)]
    ServiceExec(BootstrapServiceExec),
    Accounts(BootstrapAccounts),
    #[usage(hide = true)]
    ConfigRoots(BootstrapConfigRoots),
    Compose(BootstrapCompose),
    // The same commands as `mise dotfiles`, which is the documented spelling.
    #[usage(hide = true)]
    Dotfiles(Dotfiles),
    Files(BootstrapFiles),
    Firewall(BootstrapFirewall),
    #[usage(hide = true)]
    Launchd(BootstrapLaunchdCompat),
    Linux(BootstrapLinux),
    Macos(BootstrapMacos),
    #[usage(hide = true)]
    MacosDefaults(BootstrapMacosDefaultsCompat),
    #[usage(name = "mise-shell-activate", alias = "shell")]
    MiseShellActivate(BootstrapShell),
    Packages(BootstrapPackages),
    Plan(BootstrapPlan),
    Plugins(BootstrapPlugins),
    Remote(Box<BootstrapRemote>),
    Repos(BootstrapRepos),
    Secrets(BootstrapSecrets),
    Services(BootstrapServices),
    Status(BootstrapStatus),
    #[usage(hide = true)]
    Systemd(BootstrapSystemdCompat),
    Unapply(BootstrapUnapply),
    User(BootstrapUser),
}

/// Show the state of every configured bootstrap part
///
/// Lists secret inputs, packages, accounts, files and directories, services,
/// the firewall, Compose projects, repositories, dotfiles, shell activation,
/// macOS defaults, LaunchAgents, systemd user units, the login shell, `[tools]`,
/// and the system dependencies of those tools, without changing anything.
/// Checking dotfiles can render trusted templates, which may run `exec()`.
///
/// `--missing` exits with status 1 if anything differs from the config; it
/// still lists every entry.
#[derive(Debug, usage_rs::Args)]
#[usage(
    visible_alias = "ls",
    verbatim_doc_comment,
    example(
        "mise bootstrap status",
        help = "List every configured part and its state"
    ),
    example(
        "mise bootstrap status --missing",
        help = "Exit with status 1 if anything differs from the config"
    ),
    example("mise bootstrap status --json", help = "Print the state as JSON")
)]
struct BootstrapStatus {
    /// Output in JSON format
    #[usage(long, short = 'J')]
    json: bool,

    /// Exit with status 1 if anything differs from the config (all entries are still listed)
    #[usage(long, verbatim_doc_comment)]
    missing: bool,

    /// Prompt securely for missing bootstrap secret inputs
    #[usage(long)]
    prompt_secrets: bool,
}

/// Show what applying bootstrap resources would change
///
/// Covers accounts, packages, files and directories, system and user services,
/// the firewall, and Compose projects, in dependency order. Repositories,
/// dotfiles, shell activation, macOS defaults, LaunchAgents, systemd user
/// units, the login shell, tools, hooks, and the `bootstrap` task are not
/// planned; preview the whole run with `mise bootstrap --dry-run`.
#[derive(Debug, usage_rs::Args)]
#[usage(
    verbatim_doc_comment,
    example("mise bootstrap plan", help = "Show the planned changes"),
    example("mise bootstrap plan --json", help = "Print the plan as JSON"),
    example(
        "mise bootstrap plan --detailed-exitcode",
        help = "Exit with status 2 if anything would change"
    )
)]
struct BootstrapPlan {
    /// Output a stable machine-readable plan in JSON format
    #[usage(long, short = 'J')]
    json: bool,

    /// Exit 0 if nothing would change, 2 if something would, 1 on errors or unknown state
    ///
    /// A resource's state is unknown when mise cannot reach its declared state
    /// on its own, for example a package whose manager is not available on this
    /// machine or cannot install the pinned version, or a service whose unit
    /// does not exist.
    #[usage(long)]
    detailed_exitcode: bool,

    /// Prompt securely for missing bootstrap secret inputs
    #[usage(long)]
    prompt_secrets: bool,
}

/// Remove what a config environment's bootstrap sections created
///
/// Removes the files, directories, user services, systemd user units, and
/// dotfile entries and edits that the named environments declare. Each named
/// environment's config is loaded for this command even when `-E` or
/// `MISE_ENV` does not select it.
///
/// mise plans the removal from the current config, not from a record of past
/// runs, so keep the environment's config files until cleanup is done. Anything
/// another selected config still declares is kept, and so is anything changed
/// since it was applied unless you pass `--force`. A directory is removed only
/// if it is empty afterward. Dotfile sources and config entries are left in
/// place.
///
/// It asks before removing anything unless `--yes`, `MISE_YES`, or the `yes`
/// setting is set, and fails when there is no terminal to ask. mise turns on
/// the `yes` setting when `CI` is set.
///
/// Packages, repositories, and Compose projects are not removed; the output
/// says how to clean them up. Accounts, system services, the firewall, and
/// other sections are left alone.
#[derive(Debug, usage_rs::Args)]
#[usage(
    verbatim_doc_comment,
    example(
        "mise bootstrap unapply ssh --dry-run",
        help = "Preview what removing the `ssh` environment's resources would do"
    ),
    example(
        "mise bootstrap unapply ssh",
        help = "Remove them after a confirmation prompt"
    ),
    example(
        "mise bootstrap unapply ssh gpg --yes",
        help = "Remove the resources of two environments without a prompt"
    )
)]
struct BootstrapUnapply {
    /// Config environments whose resources should be removed
    #[usage(value_name = "ENV", required = true)]
    environment: Vec<String>,

    /// Remove targets that changed since they were applied
    #[usage(long, short)]
    force: bool,

    /// Show what would be removed without removing anything
    #[usage(long, short = 'n')]
    dry_run: bool,

    /// Skip the confirmation prompt
    #[usage(long, short)]
    yes: bool,

    /// Prompt securely for missing bootstrap secret inputs
    #[usage(long)]
    prompt_secrets: bool,
}

/// Show non-composed bootstrap declarations in each selected configuration root (deprecated)
///
/// Use this to locate the origin of declarations before composition. For the
/// combined desired state and its changes, use `mise bootstrap plan`.
#[derive(Debug, usage_rs::Args)]
#[usage(verbatim_doc_comment)]
struct BootstrapConfigRoots {
    /// Output in JSON format
    #[usage(long, short = 'J')]
    json: bool,
}

#[derive(Debug, Serialize)]
struct BootstrapConfigRootsOutput {
    roots: Vec<BootstrapConfigRootOutput>,
}

#[derive(Debug, Serialize)]
struct BootstrapConfigRootOutput {
    #[serde(serialize_with = "system::resources::serialize_path")]
    config_root: PathBuf,
    environments: Vec<String>,
    declares: BootstrapConfigRootDeclarations,
}

#[derive(Debug, Default, Serialize)]
struct BootstrapConfigRootDeclarations {
    packages: BootstrapDeclarationSurface,
    repos: BootstrapDeclarationSurface,
    accounts: BootstrapDeclarationSurface,
    hooks: BootstrapHookDeclarationSurface,
}

#[derive(Debug, Default, Serialize)]
struct BootstrapDeclarationSurface {
    count: usize,
    provenance: Vec<BootstrapDeclarationOrigin>,
}

#[derive(Debug, Default, Serialize)]
struct BootstrapHookDeclarationSurface {
    count: usize,
    phases: Vec<String>,
    provenance: Vec<BootstrapDeclarationOrigin>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct BootstrapDeclarationOrigin {
    #[serde(serialize_with = "system::resources::serialize_path")]
    config: PathBuf,
    environment: Vec<String>,
}

#[derive(Debug, usage_rs::Args)]
struct BootstrapApplySystemPlan {}

#[derive(Debug, usage_rs::Args)]
struct BootstrapApplyAccountPlan {}

#[derive(Debug, usage_rs::Args)]
struct BootstrapApplyServicePlan {}

#[derive(Debug, usage_rs::Args)]
struct BootstrapApplyFirewallPlan {}

#[derive(Debug, usage_rs::Args)]
struct BootstrapInspectFirewallPlan {}

#[derive(Debug, usage_rs::Args)]
struct BootstrapInspectSystemFiles {}

/// Run a user service that carries an environment (Windows, internal)
///
/// Task Scheduler's task XML has no environment block, so a service that
/// sets `environment` registers this as its action instead of naming its
/// program directly. It applies the stored environment, starts the service,
/// and stays for its lifetime as the process Task Scheduler tracks.
#[derive(Debug, usage_rs::Args)]
#[usage(verbatim_doc_comment)]
struct BootstrapServiceExec {
    /// The user service to run
    name: String,

    /// The stored launch to run it from
    #[usage(long, value_name = "PATH")]
    launch: String,

    /// The digest of the launch the task was registered with
    #[usage(long, value_name = "HASH")]
    digest: String,
}

/// Manage Linux users and groups
///
/// Creates, updates, or removes the local accounts declared in
/// `[bootstrap.users]` and `[bootstrap.groups]` on Linux. Run `status` or
/// `apply --dry-run` before changing user IDs, group memberships, or account
/// state.
#[derive(Debug, usage_rs::Args)]
#[usage(verbatim_doc_comment)]
struct BootstrapAccounts {
    #[usage(subcommand)]
    command: BootstrapAccountsCommands,
}

#[derive(Debug, usage_rs::Subcommands)]
enum BootstrapAccountsCommands {
    Apply(BootstrapAccountsApply),
    Status(BootstrapAccountsStatus),
}

/// Create, update, or remove the configured Linux users and groups
#[derive(Debug, usage_rs::Args)]
struct BootstrapAccountsApply {
    /// Show what would change without changing anything
    #[usage(long, short = 'n')]
    dry_run: bool,

    /// Skip the confirmation prompt
    #[usage(long, short)]
    yes: bool,
}

/// Show the state of the configured Linux users and groups
#[derive(Debug, usage_rs::Args)]
struct BootstrapAccountsStatus {
    /// Output in JSON format
    #[usage(long, short = 'J')]
    json: bool,

    /// Exit with status 1 if any account differs from the config (all are still listed)
    #[usage(long)]
    missing: bool,
}

/// Manage system files and directories declared in `[bootstrap]`
///
/// Applies `[bootstrap.files]` and `[bootstrap.directories]`: paths that need a
/// specific owner, group, or mode, or that must exist or be absent system-wide,
/// such as configuration under /etc. For files in your home directory that you
/// edit, use `[dotfiles]` and `mise dotfiles`.
#[derive(Debug, usage_rs::Args)]
#[usage(
    verbatim_doc_comment,
    example(
        "mise bootstrap files status --missing",
        help = "Exit with status 1 if any file or directory differs from the config"
    ),
    example(
        "mise bootstrap files apply --dry-run",
        help = "Show what would change"
    ),
    example("mise bootstrap files apply --yes", help = "Apply without a prompt")
)]
struct BootstrapFiles {
    #[usage(subcommand)]
    command: BootstrapFilesCommands,
}

#[derive(Debug, usage_rs::Subcommands)]
enum BootstrapFilesCommands {
    Apply(BootstrapFilesApply),
    Status(BootstrapFilesStatus),
}

/// Create, update, or remove the configured system files and directories
#[derive(Debug, usage_rs::Args)]
struct BootstrapFilesApply {
    /// Show what would change without changing anything
    #[usage(long, short = 'n')]
    dry_run: bool,

    /// Skip the confirmation prompt
    #[usage(long, short)]
    yes: bool,

    /// Prompt securely for missing bootstrap secret inputs
    #[usage(long)]
    prompt_secrets: bool,
}

/// Show the state of the configured system files and directories
#[derive(Debug, usage_rs::Args)]
struct BootstrapFilesStatus {
    /// Output in JSON format
    #[usage(long, short = 'J')]
    json: bool,

    /// Exit with status 1 if any file or directory differs from the config (all are still listed)
    #[usage(long)]
    missing: bool,

    /// Prompt securely for missing bootstrap secret inputs
    #[usage(long)]
    prompt_secrets: bool,
}

/// Manage services from `[bootstrap.services]`
///
/// System-scope entries, the default for entries without `builtin`, start,
/// stop, enable, disable, or mask systemd units that already exist on Linux.
/// User-scope entries (`scope = "user"`, or any `builtin` service) define a
/// service for the current user on every platform: a systemd user unit on
/// Linux, a LaunchAgent on macOS, or a Scheduled Task on Windows. For options
/// that only one platform has, see `mise bootstrap linux systemd-units` and
/// `mise bootstrap macos launchd-agents`.
#[derive(Debug, usage_rs::Args)]
#[usage(
    verbatim_doc_comment,
    example(
        "mise bootstrap services status",
        help = "Show the state of every declared service"
    ),
    example(
        "mise bootstrap services apply --dry-run",
        help = "Show what would change"
    ),
    example(
        "mise bootstrap services remove mise-history",
        help = "Uninstall a user service"
    )
)]
struct BootstrapServices {
    #[usage(subcommand)]
    command: BootstrapServicesCommands,
}

#[derive(Debug, usage_rs::Subcommands)]
enum BootstrapServicesCommands {
    Apply(BootstrapServicesApply),
    Remove(BootstrapServicesRemove),
    Status(BootstrapServicesStatus),
}

/// Apply the service state declared in `[bootstrap.services]`
#[derive(Debug, usage_rs::Args)]
struct BootstrapServicesApply {
    /// Show what would change without changing anything
    #[usage(long, short = 'n')]
    dry_run: bool,

    /// Skip the confirmation prompt
    #[usage(long, short)]
    yes: bool,
}

/// Uninstall a user-scope service
///
/// Removing a user-scope entry from config does not uninstall its systemd
/// unit, LaunchAgent, or Scheduled Task. Use this command to uninstall it,
/// whether or not it is still declared. If the entry is still declared, the
/// next `mise bootstrap` installs it again.
///
/// It removes the dev.mise.NAME.service unit or dev.mise.NAME.plist agent, so it
/// also removes one that [bootstrap.linux.systemd.units] or
/// [bootstrap.macos.launchd.agents] wrote under that name. It does not remove
/// .timer units.
#[derive(Debug, usage_rs::Args)]
#[usage(
    verbatim_doc_comment,
    example(
        "mise bootstrap services remove mise-history --dry-run",
        help = "Preview uninstalling the user service named mise-history"
    )
)]
struct BootstrapServicesRemove {
    /// Name of the installed user service
    name: String,

    /// Show what would be removed without removing it
    #[usage(long, short = 'n')]
    dry_run: bool,
}

/// Show the state of services from `[bootstrap.services]`
#[derive(Debug, usage_rs::Args)]
struct BootstrapServicesStatus {
    /// Output in JSON format
    #[usage(long, short = 'J')]
    json: bool,

    /// Exit with status 1 if any service differs from the config (all are still listed)
    #[usage(long)]
    missing: bool,
}

/// Manage the Linux host firewall from `[bootstrap.linux.firewall]`
///
/// Works with nftables, firewalld, or UFW, and keeps mise's rules separate from
/// other host rules. Over SSH, mise refuses a default incoming policy of deny
/// or reject unless a rule allows the current connection or the config sets
/// `allow_lockout = true`. Check `apply --dry-run` before applying a policy to
/// a remote machine.
#[derive(Debug, usage_rs::Args)]
#[usage(
    verbatim_doc_comment,
    example("mise bootstrap firewall status", help = "Show the firewall's state"),
    example(
        "mise bootstrap firewall apply --dry-run",
        help = "Show the policy and rules that would change"
    )
)]
struct BootstrapFirewall {
    #[usage(subcommand)]
    command: BootstrapFirewallCommands,
}

#[derive(Debug, usage_rs::Subcommands)]
enum BootstrapFirewallCommands {
    Apply(BootstrapFirewallApply),
    Status(BootstrapFirewallStatus),
}

/// Apply the firewall from `[bootstrap.linux.firewall]`
#[derive(Debug, usage_rs::Args)]
struct BootstrapFirewallApply {
    /// Show what would change without changing anything
    #[usage(long, short = 'n')]
    dry_run: bool,

    /// Skip the confirmation prompt
    #[usage(long, short)]
    yes: bool,
}

/// Show the state of the firewall from `[bootstrap.linux.firewall]`
#[derive(Debug, usage_rs::Args)]
struct BootstrapFirewallStatus {
    /// Output in JSON format
    #[usage(long, short = 'J')]
    json: bool,

    /// Exit with status 1 if the firewall differs from the config
    #[usage(long)]
    missing: bool,
}

/// Manage Docker Compose projects from `[bootstrap.compose]`
///
/// Needs a running Docker engine and the Docker Compose command on this
/// machine.
#[derive(Debug, usage_rs::Args)]
#[usage(
    verbatim_doc_comment,
    example(
        "mise bootstrap compose status",
        help = "Show the state of every declared project"
    ),
    example(
        "mise bootstrap compose apply --dry-run",
        help = "Show what would change"
    )
)]
struct BootstrapCompose {
    #[usage(subcommand)]
    command: BootstrapComposeCommands,
}

#[derive(Debug, usage_rs::Subcommands)]
enum BootstrapComposeCommands {
    Apply(BootstrapComposeApply),
    Status(BootstrapComposeStatus),
}

/// Apply Docker Compose projects from `[bootstrap.compose]`
#[derive(Debug, usage_rs::Args)]
struct BootstrapComposeApply {
    /// Show what would change without changing anything
    #[usage(long, short = 'n')]
    dry_run: bool,

    /// Skip the confirmation prompt
    #[usage(long, short)]
    yes: bool,
}

/// Show the state of Docker Compose projects from `[bootstrap.compose]`
#[derive(Debug, usage_rs::Args)]
struct BootstrapComposeStatus {
    /// Output in JSON format
    #[usage(long, short = 'J')]
    json: bool,

    /// Exit with status 1 if any Compose project differs from the config (all are still listed)
    #[usage(long)]
    missing: bool,
}

/// Bootstrap one or more machines over SSH
///
/// Packs a local project directory, copies it to each target, stages mise
/// there, and runs `mise bootstrap`. The directory is `--source`. Without it,
/// an inventory host uses its own `source`, then the one in
/// `[bootstrap.remote]`, then the current directory; a `--host` target uses
/// the current directory. Pick targets by name, `--tag`, or `--all` from
/// `[bootstrap.remote.hosts]`, or give any SSH destination with `--host`.
///
/// Targets need a POSIX shell plus `tar`, `mktemp`, `cksum`, and `uname`;
/// Windows targets are not supported. `--dry-run` still connects, uploads, and
/// inspects each target, but changes nothing there. With `--adopt`, each target
/// adopts the repository as `mise bootstrap --adopt` does, instead of receiving
/// a project directory.
///
/// The `--github-relay-*` flags let targets read private GitHub repositories
/// through this machine's credentials for the length of the run, without
/// copying a token to them. See https://mise.jdx.dev/bootstrap/github-relay.html.
#[derive(Debug, usage_rs::Args)]
#[usage(
    verbatim_doc_comment,
    example(
        "mise bootstrap remote --host devbox --dry-run",
        help = "Preview a run on a host that is not in the inventory"
    ),
    example("mise bootstrap remote cache", help = "Bootstrap one inventory host"),
    example(
        "mise bootstrap remote --tag canary --yes",
        help = "Bootstrap every inventory host tagged canary, without prompts"
    ),
    example(
        "mise bootstrap remote --all --fail-fast",
        help = "Bootstrap every inventory host and stop at the first failure"
    ),
    example(
        "mise bootstrap remote --host ubuntu@cache.example.com -i ~/.ssh/mise-cache --install-mise",
        help = "Leave mise installed on the host after the run"
    ),
    example(
        "mise bootstrap remote --host devbox --github-relay-read-only --github-relay-repo example/setup",
        help = "Let the host read one private GitHub repository during the run"
    )
)]
struct BootstrapRemote {
    /// Adopt global configuration or shared dotfile history on each target
    #[usage(long, value_name = "GIT_URL|OWNER/REPO", conflicts = ["source", "copy_link", "copy_links", "exclude"])]
    adopt: Option<String>,

    /// Inventory host names from `[bootstrap.remote.hosts]`
    #[usage(value_name = "TARGET")]
    targets: Vec<String>,

    /// Select every inventory host
    #[usage(long)]
    all: bool,

    /// Shell command, run on each target, that installs mise and puts it on PATH
    #[usage(
        long,
        value_name = "COMMAND",
        conflicts = ["mise_bin", "remote_mise", "install_mise"]
    )]
    bootstrap_command: Option<String>,

    /// SSH connection timeout in seconds
    #[usage(long, value_name = "SECONDS", default_value_t = 10, default = "10")]
    connect_timeout: u16,

    /// Copy the target of this source-relative symbolic link instead of the link; repeat for more
    #[usage(long, value_name = "PATH", value_hint = usage_rs::ValueHint::AnyPath)]
    copy_link: Vec<std::path::PathBuf>,

    /// Copy the targets of every symbolic link in the source instead of the links
    #[usage(long)]
    copy_links: bool,

    /// Another pattern to leave out of the archive; repeat for more
    #[usage(long, value_name = "PATTERN")]
    exclude: Vec<String>,

    /// Stop after the first failed target
    #[usage(long)]
    fail_fast: bool,

    /// Overwrite files on each target that conflict with whole-file dotfile entries
    #[usage(long)]
    force_dotfiles: bool,

    /// SSH destination that is not in the inventory (`[user@]host`); repeat for more
    #[usage(long, value_name = "[USER@]HOST")]
    host: Vec<String>,

    /// SSH identity file, overriding the inventory and SSH config
    #[usage(long, short = 'i', value_name = "PATH", value_hint = usage_rs::ValueHint::FilePath)]
    identity_file: Option<std::path::PathBuf>,

    /// Show what would change on each target without changing it
    ///
    /// It still connects to each target, uploads the project, stages mise, and
    /// inspects the target.
    #[usage(long, short = 'n')]
    dry_run: bool,

    /// Install the provisioned mise on each host instead of only staging it
    ///
    /// Defaults to `~/.local/bin/mise`; pass `--install-mise=<PATH>` for another
    /// path.
    #[usage(
        long,
        value_name = "PATH",
        num_args = 0..=1,
        require_equals = true,
        default_missing = "~/.local/bin/mise",
        conflicts = ["remote_mise", "bootstrap_command", "no_install_mise"]
    )]
    install_mise: Option<String>,

    /// Keep the remote staging directory for debugging
    #[usage(long)]
    keep_staging: bool,

    /// Upload this local mise executable, for platforms without an official release binary
    #[usage(
        long,
        value_name = "PATH",
        value_hint = usage_rs::ValueHint::FilePath,
        conflicts = ["remote_mise", "bootstrap_command"]
    )]
    mise_bin: Option<std::path::PathBuf>,

    /// Do not install mise on the host, even when the selected hosts configure it
    #[usage(long)]
    no_install_mise: bool,

    /// Run only these parts on each target
    ///
    /// Repeat the flag or separate parts with commas. Cannot be combined with
    /// `--skip`.
    #[usage(
        long,
        value_enum,
        value_name = "PART",
        delimiter = ',',
        conflicts = "skip"
    )]
    only: Vec<BootstrapPartArg>,

    /// SSH port, overriding the inventory and SSH config
    #[usage(long)]
    port: Option<u16>,

    /// Prompt securely for missing secret inputs on the remote host
    #[usage(long)]
    prompt_secrets: bool,

    /// Config environments to load on each target; repeat or separate with commas (for example, ci,dotfiles)
    #[usage(long, value_name = "ENV", delimiter = ',')]
    remote_env: Option<Vec<String>>,

    /// Existing mise executable name or path; relative paths use the staged project
    #[usage(
        long,
        value_name = "COMMAND",
        conflicts = ["mise_bin", "bootstrap_command", "install_mise"]
    )]
    remote_mise: Option<String>,

    /// Skip these parts on each target
    ///
    /// Repeat the flag or separate parts with commas. Cannot be combined with
    /// `--only`.
    #[usage(long, value_enum, value_name = "PART", delimiter = ',')]
    skip: Vec<BootstrapPartArg>,

    /// Local directory to archive and send to each target
    #[usage(long, value_name = "DIR", value_hint = usage_rs::ValueHint::DirPath)]
    source: Option<std::path::PathBuf>,

    /// OpenSSH `-o` option; repeat for multiple options
    #[usage(long, value_name = "OPTION")]
    ssh_option: Vec<String>,

    /// Select configured hosts with this tag; repeat to match any tag
    #[usage(long, value_name = "TAG")]
    tag: Vec<String>,

    /// Refresh package metadata and update repositories on each target
    #[usage(long)]
    update: bool,

    /// Skip confirmation prompts on each target
    #[usage(long, short = 'y')]
    yes: bool,

    /// Borrow read-only GitHub access for this run (Linux and macOS only)
    ///
    /// Needs exactly one of `--github-relay-repo` or `--github-relay-all-repos`.
    #[usage(long)]
    github_relay_read_only: bool,

    /// Repository the targets may read through the relay; repeat for more (needs `--github-relay-read-only`)
    #[usage(long, value_name = "OWNER/REPO")]
    github_relay_repo: Vec<String>,

    /// Let targets read every repository your local credentials can read (needs `--github-relay-read-only`)
    #[usage(long)]
    github_relay_all_repos: bool,

    /// Log sanitized relay requests on local stderr (needs `--github-relay-read-only`)
    #[usage(long, conflicts = "github_relay_no_log_requests")]
    github_relay_log_requests: bool,

    /// Turn off request logging, overriding the `github_relay` settings (needs `--github-relay-read-only`)
    #[usage(long)]
    github_relay_no_log_requests: bool,

    /// Relay log and summary format, `text` or `jsonl` (needs `--github-relay-read-only`)
    #[usage(long, value_name = "FORMAT")]
    github_relay_log_format: Option<String>,

    /// End borrowed access after a duration such as `1h`; `0s` lasts the whole run (needs `--github-relay-read-only`)
    #[usage(long, value_name = "DURATION")]
    github_relay_max_duration: Option<String>,
}

/// Inspect bootstrap secret inputs without revealing their values
///
/// Secret inputs are values declared in `[bootstrap.secrets]` that file and
/// dotfile templates read with `secret()`. mise reads them from environment
/// variables, or prompts for missing ones with `--prompt-secrets`.
#[derive(Debug, usage_rs::Args)]
#[usage(verbatim_doc_comment)]
struct BootstrapSecrets {
    #[usage(subcommand)]
    command: BootstrapSecretsCommands,
}

#[derive(Debug, usage_rs::Subcommands)]
enum BootstrapSecretsCommands {
    Status(BootstrapSecretsStatus),
}

/// Show whether declared bootstrap secret inputs are available
#[derive(Debug, usage_rs::Args)]
struct BootstrapSecretsStatus {
    /// Output in JSON format
    #[usage(long, short = 'J')]
    json: bool,

    /// Exit with status 1 if a declared secret input is unset, empty, or not valid Unicode
    #[usage(long)]
    missing: bool,
}

/// Manage packages from `[bootstrap.packages]`
///
/// Declare packages as `"manager:package" = "version"`. `apply` installs what
/// is missing and removes what is declared absent, `use` adds an entry and
/// installs it, and `upgrade` updates configured packages that are installed.
/// `prune` uninstalls Homebrew formulae, casks, or package plugin packages that
/// no config declares. `import` records installed Homebrew formulae, and
/// `status` shows what differs from the config. See
/// https://mise.jdx.dev/bootstrap/packages/ for each package manager.
#[derive(Debug, usage_rs::Args)]
#[usage(verbatim_doc_comment)]
struct BootstrapPackages {
    #[usage(subcommand)]
    command: BootstrapPackagesCommands,
}

/// Manage package manager plugins declared in `[bootstrap.plugins]`
///
/// A package plugin adds a package manager, such as one for VS Code extensions,
/// that `[bootstrap.packages]` entries can use. `mise bootstrap` installs
/// plugins before packages; when you run the narrower commands, run
/// `plugins apply` before `packages apply`. Installing a plugin does not
/// install its packages.
#[derive(Debug, usage_rs::Args)]
#[usage(verbatim_doc_comment)]
struct BootstrapPlugins {
    #[usage(subcommand)]
    command: BootstrapPluginsCommands,
}

#[derive(Debug, usage_rs::Subcommands)]
enum BootstrapPluginsCommands {
    Apply(BootstrapPluginsApply),
    Status(BootstrapPluginsStatus),
}

/// Install package manager plugins declared in `[bootstrap.plugins]`
///
/// It does not ask for confirmation.
#[derive(Debug, usage_rs::Args)]
struct BootstrapPluginsApply {
    /// Show which plugins would be installed without installing them
    #[usage(long, short = 'n')]
    dry_run: bool,
}

/// Show whether declared package manager plugins are installed
#[derive(Debug, usage_rs::Args)]
struct BootstrapPluginsStatus {
    /// Exit with status 1 if a declared plugin is not installed
    #[usage(long)]
    missing: bool,
}

#[derive(Debug, usage_rs::Subcommands)]
enum BootstrapPackagesCommands {
    #[usage(alias = "install")]
    Apply(install::SystemInstall),
    #[cfg(unix)]
    Brew(super::system::brew::SystemBrew),
    Export(export::SystemExport),
    Import(import::SystemImport),
    Prune(prune::SystemPrune),
    Status(status::SystemStatus),
    Upgrade(upgrade::SystemUpgrade),
    Use(r#use::SystemUse),
    Where(super::system::r#where::SystemWhere),
}

/// Manage Git repositories from `[bootstrap.repos]`
///
/// A repository with uncommitted changes, a different origin, or something
/// other than a Git checkout at its path stops `apply` and `update` before
/// anything changes. Pass `--skip-dirty` to skip repositories with uncommitted
/// changes and continue with the rest.
#[derive(Debug, usage_rs::Args)]
#[usage(verbatim_doc_comment)]
struct BootstrapRepos {
    #[usage(subcommand)]
    command: BootstrapReposCommands,
}

#[derive(Debug, usage_rs::Subcommands)]
enum BootstrapReposCommands {
    Apply(BootstrapReposApply),
    Exec(BootstrapReposExec),
    Status(BootstrapReposStatus),
    Update(BootstrapReposUpdate),
}

/// Clone missing Git repositories and check out configured refs
#[derive(Debug, usage_rs::Args)]
struct BootstrapReposApply {
    /// Show what would change without changing anything
    #[usage(long, short = 'n')]
    dry_run: bool,

    /// Skip the confirmation prompt
    #[usage(long, short)]
    yes: bool,

    /// Skip repositories with local changes instead of failing
    #[usage(long)]
    skip_dirty: bool,
}

/// Clone missing Git repositories and fast-forward the rest
///
/// For a repository without a `ref`, fetches and fast-forwards the current
/// branch; one with a detached HEAD is skipped with a warning. A repository
/// with a `ref` is checked out at that ref.
#[derive(Debug, usage_rs::Args)]
#[usage(
    verbatim_doc_comment,
    example(
        "mise bootstrap repos update --dry-run",
        help = "Show the Git commands that would run"
    ),
    example(
        "mise bootstrap repos update ~/src/mise",
        help = "Update one repository"
    )
)]
struct BootstrapReposUpdate {
    /// Only update repositories at these paths, as written in config or expanded (for example ~/src/mise)
    #[usage(value_name = "PATH")]
    paths: Vec<String>,

    /// Show what would change without changing anything
    #[usage(long, short = 'n')]
    dry_run: bool,

    /// Skip the confirmation prompt
    #[usage(long, short)]
    yes: bool,

    /// Skip repositories with local changes instead of failing
    #[usage(long)]
    skip_dirty: bool,
}

/// Run a command in each configured Git repository
///
/// Put the command and its arguments after `--`. Paths before `--` select
/// repositories. The command runs directly, without a shell, so pipes and `&&`
/// do not work. Missing or conflicting repositories are skipped with a
/// warning. Use `--continue-on-error` to visit the rest after a failure.
#[derive(Debug, usage_rs::Args)]
#[usage(
    verbatim_doc_comment,
    example(
        "mise bootstrap repos exec -- git status --short",
        help = "Show uncommitted changes in every repository"
    ),
    example(
        "mise bootstrap repos exec ~/src/mise -- git log -1",
        help = "Run a command in one repository"
    ),
    example(
        "mise bootstrap repos exec --continue-on-error -- git fetch",
        help = "Fetch every repository, even after one fails"
    )
)]
struct BootstrapReposExec {
    /// Run only in repositories at these paths, as written in config or expanded
    #[usage(value_name = "PATH")]
    paths: Vec<String>,

    /// Continue running in other repositories after a command fails
    #[usage(long, short = 'c')]
    continue_on_error: bool,

    /// Print the commands that would run without running them
    #[usage(long, short = 'n')]
    dry_run: bool,

    /// Command and arguments to run in each repository
    #[usage(double_dash = "required", required = true)]
    command: Vec<String>,
}

/// Show the state of Git repositories from `[bootstrap.repos]`
#[derive(Debug, usage_rs::Args)]
struct BootstrapReposStatus {
    /// Output in JSON format
    #[usage(long, short = 'J')]
    json: bool,

    /// Exit with status 1 if any repository differs from the config (all are still listed)
    #[usage(long, verbatim_doc_comment)]
    missing: bool,
}

/// Manage macOS preferences and LaunchAgents from `[bootstrap.macos]`
#[derive(Debug, usage_rs::Args)]
#[usage(verbatim_doc_comment)]
struct BootstrapMacos {
    #[usage(subcommand)]
    command: BootstrapMacosCommands,
}

#[derive(Debug, usage_rs::Subcommands)]
enum BootstrapMacosCommands {
    Defaults(BootstrapMacosDefaults),
    #[usage(name = "launchd-agents", alias = "launchd")]
    LaunchdAgents(BootstrapLaunchd),
}

/// Manage systemd user units from `[bootstrap.linux.systemd.units]`
///
/// The host firewall in `[bootstrap.linux.firewall]` is managed by
/// `mise bootstrap firewall`.
#[derive(Debug, usage_rs::Args)]
#[usage(verbatim_doc_comment)]
struct BootstrapLinux {
    #[usage(subcommand)]
    command: BootstrapLinuxCommands,
}

#[derive(Debug, usage_rs::Subcommands)]
enum BootstrapLinuxCommands {
    #[usage(name = "systemd-units", alias = "systemd")]
    SystemdUnits(BootstrapSystemd),
}

/// Manage macOS preferences such as Dock, Finder, and keyboard settings
///
/// Applies `[bootstrap.macos.defaults]`, `[[bootstrap.macos.defaults_entries]]`,
/// and the `[bootstrap.macos.dock]`, `finder`, `keyboard`, and `trackpad`
/// sections. Preferences are written the way `defaults write` writes them.
/// mise does not restart apps: Dock, Finder, and some others show a change
/// only after they relaunch.
#[derive(Debug, usage_rs::Args)]
#[usage(verbatim_doc_comment)]
struct BootstrapMacosDefaults {
    #[usage(subcommand)]
    command: BootstrapMacosDefaultsCommands,
}

/// Manage macOS preferences (use `mise bootstrap macos defaults`)
///
/// This older spelling keeps working.
#[derive(Debug, usage_rs::Args)]
struct BootstrapMacosDefaultsCompat {
    #[usage(subcommand)]
    command: BootstrapMacosDefaultsCompatCommands,
}

// The children of a hidden compatibility command are hidden too, so their
// reference pages are not generated a second time.
#[derive(Debug, usage_rs::Subcommands)]
enum BootstrapMacosDefaultsCompatCommands {
    #[usage(hide = true)]
    Apply(BootstrapMacosDefaultsApply),
    #[usage(hide = true)]
    Status(BootstrapMacosDefaultsStatus),
}

#[derive(Debug, usage_rs::Subcommands)]
enum BootstrapMacosDefaultsCommands {
    Apply(BootstrapMacosDefaultsApply),
    Status(BootstrapMacosDefaultsStatus),
}

/// Write the configured macOS preferences
#[derive(Debug, usage_rs::Args)]
struct BootstrapMacosDefaultsApply {
    /// Show what would change without changing anything
    #[usage(long, short = 'n')]
    dry_run: bool,

    /// Skip the confirmation prompt
    #[usage(long, short)]
    yes: bool,
}

/// Show the state of the configured macOS preferences
#[derive(Debug, usage_rs::Args)]
struct BootstrapMacosDefaultsStatus {
    /// Output in JSON format
    #[usage(long, short = 'J')]
    json: bool,

    /// Exit with status 1 if any preference differs from the config (all are still listed)
    #[usage(long, verbatim_doc_comment)]
    missing: bool,
}

/// Manage macOS LaunchAgents from `[bootstrap.macos.launchd.agents]`
///
/// Writes each agent to ~/Library/LaunchAgents/dev.mise.<name>.plist and loads
/// it into your GUI login session. Run it as the user who owns the agents; an
/// SSH session without a GUI login may not be able to load them. For a user
/// service that also works on Linux and Windows, use `[bootstrap.services]`
/// with `scope = "user"`.
#[derive(Debug, usage_rs::Args)]
#[usage(verbatim_doc_comment)]
struct BootstrapLaunchd {
    #[usage(subcommand)]
    command: BootstrapLaunchdCommands,
}

/// Manage macOS LaunchAgents (use `mise bootstrap macos launchd-agents`)
///
/// This older spelling keeps working.
#[derive(Debug, usage_rs::Args)]
struct BootstrapLaunchdCompat {
    #[usage(subcommand)]
    command: BootstrapLaunchdCompatCommands,
}

// The children of a hidden compatibility command are hidden too, so their
// reference pages are not generated a second time.
#[derive(Debug, usage_rs::Subcommands)]
enum BootstrapLaunchdCompatCommands {
    #[usage(hide = true)]
    Apply(BootstrapLaunchdApply),
    #[usage(hide = true)]
    Status(BootstrapLaunchdStatus),
}

#[derive(Debug, usage_rs::Subcommands)]
enum BootstrapLaunchdCommands {
    Apply(BootstrapLaunchdApply),
    Status(BootstrapLaunchdStatus),
}

/// Install and load LaunchAgents from `[bootstrap.macos.launchd.agents]`
#[derive(Debug, usage_rs::Args)]
struct BootstrapLaunchdApply {
    /// Show what would change without changing anything
    #[usage(long, short = 'n')]
    dry_run: bool,

    /// Skip the confirmation prompt
    #[usage(long, short)]
    yes: bool,
}

/// Show the state of the configured LaunchAgents
#[derive(Debug, usage_rs::Args)]
struct BootstrapLaunchdStatus {
    /// Output in JSON format
    #[usage(long, short = 'J')]
    json: bool,

    /// Exit with status 1 if any LaunchAgent differs from the config (all are still listed)
    #[usage(long, verbatim_doc_comment)]
    missing: bool,
}

/// Manage systemd user units from `[bootstrap.linux.systemd.units]`
///
/// Writes unit files to ~/.config/systemd/user, then enables and starts them
/// as configured. An entry with `state = "absent"` is stopped, disabled, and
/// deleted instead. Units run with your permissions and need a reachable
/// systemd user manager. For system units, or a user service that also works
/// on macOS and Windows, use `[bootstrap.services]`.
#[derive(Debug, usage_rs::Args)]
#[usage(verbatim_doc_comment)]
struct BootstrapSystemd {
    #[usage(subcommand)]
    command: BootstrapSystemdCommands,
}

/// Manage systemd user units (use `mise bootstrap linux systemd-units`)
///
/// This older spelling keeps working.
#[derive(Debug, usage_rs::Args)]
struct BootstrapSystemdCompat {
    #[usage(subcommand)]
    command: BootstrapSystemdCompatCommands,
}

// The children of a hidden compatibility command are hidden too, so their
// reference pages are not generated a second time.
#[derive(Debug, usage_rs::Subcommands)]
enum BootstrapSystemdCompatCommands {
    #[usage(hide = true)]
    Apply(BootstrapSystemdApply),
    #[usage(hide = true)]
    Status(BootstrapSystemdStatus),
}

#[derive(Debug, usage_rs::Subcommands)]
enum BootstrapSystemdCommands {
    Apply(BootstrapSystemdApply),
    Status(BootstrapSystemdStatus),
}

/// Install and start the configured systemd user units
#[derive(Debug, usage_rs::Args)]
struct BootstrapSystemdApply {
    /// Show what would change without changing anything
    #[usage(long, short = 'n')]
    dry_run: bool,

    /// Skip the confirmation prompt
    #[usage(long, short)]
    yes: bool,
}

/// Show the state of the configured systemd user units
#[derive(Debug, usage_rs::Args)]
struct BootstrapSystemdStatus {
    /// Output in JSON format
    #[usage(long, short = 'J')]
    json: bool,

    /// Exit with status 1 if any unit differs from the config (all are still listed)
    #[usage(long, verbatim_doc_comment)]
    missing: bool,
}

/// Manage mise shell activation from `[bootstrap.mise_shell_activate]`
///
/// Writes a managed activation block into each configured shell startup file.
/// It does not activate mise in the shell you run it from; open a new shell
/// afterward.
#[derive(Debug, usage_rs::Args)]
#[usage(verbatim_doc_comment)]
struct BootstrapShell {
    #[usage(subcommand)]
    command: BootstrapShellCommands,
}

#[derive(Debug, usage_rs::Subcommands)]
enum BootstrapShellCommands {
    Apply(BootstrapShellApply),
    Status(BootstrapShellStatus),
}

/// Write mise activation into the configured shell startup files
#[derive(Debug, usage_rs::Args)]
struct BootstrapShellApply {
    /// Show what would change without changing anything
    #[usage(long, short = 'n')]
    dry_run: bool,

    /// Skip the confirmation prompt
    #[usage(long, short)]
    yes: bool,
}

/// Show the state of the configured shell activation
#[derive(Debug, usage_rs::Args)]
struct BootstrapShellStatus {
    /// Output in JSON format
    #[usage(long, short = 'J')]
    json: bool,

    /// Exit with status 1 if any shell activation differs from the config (all are still listed)
    #[usage(long, verbatim_doc_comment)]
    missing: bool,
}

/// Manage your login shell from `[bootstrap.user]`
///
/// Run it as the user whose shell should change. The new shell applies to
/// future logins, not to the shell running this command.
#[derive(Debug, usage_rs::Args)]
#[usage(verbatim_doc_comment)]
struct BootstrapUser {
    #[usage(subcommand)]
    command: BootstrapUserCommands,
}

#[derive(Debug, usage_rs::Subcommands)]
enum BootstrapUserCommands {
    Apply(BootstrapUserApply),
    Status(BootstrapUserStatus),
}

/// Set your login shell from `[bootstrap.user]`
#[derive(Debug, usage_rs::Args)]
struct BootstrapUserApply {
    /// Show what would change without changing anything
    #[usage(long, short = 'n')]
    dry_run: bool,

    /// Skip the confirmation prompt
    #[usage(long, short)]
    yes: bool,
}

/// Show whether your login shell matches `[bootstrap.user]`
#[derive(Debug, usage_rs::Args)]
struct BootstrapUserStatus {
    /// Output in JSON format
    #[usage(long, short = 'J')]
    json: bool,

    /// Exit with status 1 if your login shell differs from the config
    #[usage(long, verbatim_doc_comment)]
    missing: bool,
}

impl Bootstrap {
    /// Identify local lookup so dispatch can bypass bootstrap configuration and hooks.
    pub(super) fn is_packages_where(&self) -> bool {
        matches!(
            self.command,
            Some(Commands::Packages(BootstrapPackages {
                command: BootstrapPackagesCommands::Where(_),
            }))
        )
    }

    pub(super) fn is_dry_run(&self) -> bool {
        self.dry_run
    }

    pub(super) fn inherit_root_flags(&mut self, dry_run: bool, yes: bool) {
        self.dry_run |= dry_run;
        self.yes |= yes;
    }

    #[cfg(test)]
    pub(super) fn inherited_root_flags(&self) -> (bool, bool) {
        (self.dry_run, self.yes)
    }

    pub(crate) async fn run(self) -> Result<()> {
        // Dotfiles subcommands handle their own notices; in particular,
        // a background watcher must leave them for a foreground command.
        let deliver_notices = self.command.is_none();
        if deliver_notices {
            system::history::notices::drain();
        }
        let result = self.run_with_notices().await;
        if deliver_notices {
            system::history::notices::drain();
        }
        result
    }

    async fn run_with_notices(mut self) -> Result<()> {
        if self.prompt_vars {
            crate::config::env_directive::prompt::enable();
            // Anything that ran before this point (such as tool purgatory cleanup)
            // may have cached config whose vars never had the chance to prompt.
            Config::reset().await?;
        }
        // Every subcommand, not just the full run, applies less than project
        // config declares in safe mode; say so up front.
        if Settings::safe_mode()
            && let Ok(config) = Config::get().await
        {
            super::dotfiles::warn_if_ignored_in_safe_mode(&config);
        }
        warn_legacy_part_aliases(&self.only, &self.skip);
        if self.from.is_some() || self.adopt.is_some() {
            if self.command.is_some() {
                let flag = if self.adopt.is_some() {
                    "--adopt"
                } else {
                    "--from"
                };
                bail!("{flag} cannot be used with a bootstrap subcommand");
            }
            return self.run_from().await;
        }
        if let Some(command) = self.command.take() {
            return command.run().await;
        }
        let generation = OperationScope::begin("bootstrap", self.dry_run).await?;
        let result = self.run_phases().await;
        generation.refresh_tracked().await;
        let error = result.as_ref().err().map(|err| format!("{err:#}"));
        generation.finish(error, result.as_ref().ok().cloned());
        // a complete run applied the declarations that arrived through
        // sync; a declined, partial, or dry run did not
        if let Ok(summary) = &result
            && !self.dry_run
            && !is_declined(summary)
            && self.only.is_empty()
            && self.skip.is_empty()
        {
            system::history::sync::run::bootstrap_completed();
        }
        result.map(|_| ())
    }

    /// Every bootstrap part in order. Returns what ran for the generation
    /// record; declined prompts end the run early with a note.
    async fn run_phases(&self) -> Result<Summary> {
        let mut config = Config::get().await?;
        let mut hooks = system::hooks_from_config(&config);
        let skip = self.skip_parts();
        let summary = Summary { message: None };
        let accounts_enabled = !skip.contains(&BootstrapPart::Accounts);
        let files_enabled = !skip.contains(&BootstrapPart::Files);
        let configured_accounts =
            if accounts_enabled || (cfg!(target_os = "linux") && files_enabled) {
                Some(system::accounts::prepare_requests_from_config(&config)?)
            } else {
                None
            };
        let managed_accounts = accounts_enabled.then(|| {
            configured_accounts
                .as_ref()
                .expect("enabled accounts were prepared")
        });
        let secrets = system::secrets::resolve(&config, self.prompt_secrets)?;
        if !self.dry_run && !skip.contains(&BootstrapPart::Dotfiles) {
            let files = system::files::files_from_config(&config)?;
            system::files::preflight_templates(&config, &files, &secrets)?;
        }
        let managed_system_files = if !files_enabled {
            None
        } else {
            Some(system::managed_files::prepare_requests_from_config(
                &config, &secrets,
            )?)
        };
        if let Some((files, directories)) = &managed_system_files {
            system::managed_files::validate_principals(
                files,
                directories,
                configured_accounts.as_ref(),
                accounts_enabled,
            )?;
        }
        let services_enabled = !skip.contains(&BootstrapPart::Services);
        let notifications_configured =
            managed_system_files
                .as_ref()
                .is_some_and(|(files, directories)| {
                    files.iter().any(|file| !file.notify.is_empty())
                        || directories
                            .iter()
                            .any(|directory| !directory.notify.is_empty())
                });
        let configured_services = if services_enabled || notifications_configured {
            Some(system::services::prepare_requests_from_config(&config)?)
        } else {
            None
        };
        if notifications_configured {
            let (files, directories) = managed_system_files
                .as_ref()
                .expect("configured notifications came from managed files");
            let services = configured_services
                .as_ref()
                .expect("configured notifications prepared services");
            let user_services = system::services_common::user_service_names(&config)?;
            system::services::validate_notifications(files, directories, services, &user_services)?;
        }
        let mut managed_services =
            services_enabled.then_some(configured_services.unwrap_or_default());
        let user_services = if services_enabled {
            system::user_services::requests_from_config(&config)?
        } else {
            vec![]
        };
        let mut managed_firewall = if skip.contains(&BootstrapPart::Firewall) {
            None
        } else {
            system::firewall::prepare_request_from_config(&config)?
        };
        // Refuse a lockout-prone firewall before applying anything else.
        // A dry run reports it as unknown instead.
        if !self.dry_run
            && let Some(firewall) = &managed_firewall
        {
            system::firewall::validate_request(firewall)?;
        }
        let mut managed_compose = if skip.contains(&BootstrapPart::Compose) {
            None
        } else {
            Some(system::compose::prepare_requests_from_config(&config)?)
        };
        let dry_run_compose_actions = if self.dry_run
            && managed_compose
                .as_ref()
                .is_some_and(|projects| !projects.is_empty())
        {
            let plan = system::resources::plan(
                &config,
                &secrets,
                !skip.contains(&BootstrapPart::Firewall),
            )
            .await?;
            let output = plan.output()?;
            let resources = output
                .resources
                .iter()
                .map(|resource| {
                    (
                        resource.id.clone(),
                        (resource.action, resource.depends_on.clone()),
                    )
                })
                .collect::<HashMap<_, _>>();
            output
                .resources
                .into_iter()
                .filter(|resource| resource.id.kind == "compose")
                .filter(|resource| {
                    !bootstrap_prediction_has_skipped_change(
                        &resource.id,
                        &resources,
                        &skip,
                        &mut HashSet::new(),
                    )
                })
                .map(|resource| (resource.id.name.clone(), resource.action))
                .collect()
        } else {
            HashMap::new()
        };
        let mut follow_up = BootstrapFollowUp::new(self.dry_run);
        let mut notified_services = system::services::ServiceNotifications::default();
        let mut dry_run_config_files = None;
        let mut post_packages_ran = false;

        let allow_pending_accounts = if let Some(accounts) = managed_accounts {
            if accounts.groups.is_empty() && accounts.users.is_empty() {
                debug!("bootstrap: no [bootstrap.groups] or [bootstrap.users] configured");
                true
            } else {
                info!("bootstrap: accounts");
                system::accounts::apply(accounts, self.dry_run, self.yes)?
            }
        } else {
            debug!("bootstrap: accounts skipped");
            false
        };

        if skip.contains(&BootstrapPart::Plugins) {
            debug!("bootstrap: package plugins skipped");
        } else {
            apply_bootstrap_plugins(&config, self.dry_run).await?;
        }

        if let Some((files, directories)) = &managed_system_files {
            let mut files = files
                .iter()
                .filter(|file| file.phase == system::managed_files::ManagedFilePhase::PrePackages)
                .cloned()
                .collect::<Vec<_>>();
            let mut directories = directories
                .iter()
                .filter(|directory| {
                    directory.phase == system::managed_files::ManagedFilePhase::PrePackages
                })
                .cloned()
                .collect::<Vec<_>>();
            if !files.is_empty() || !directories.is_empty() {
                info!("bootstrap: system files (pre-packages)");
                system::managed_files::inspect_requests(&mut files, &mut directories)?;
                let report = system::managed_files::apply_with_accounts(
                    &files,
                    &directories,
                    configured_accounts.as_ref(),
                    allow_pending_accounts,
                    self.dry_run,
                    self.yes,
                )?;
                notified_services.extend(report.notified_services);
            }
        }

        if skip.contains(&BootstrapPart::Packages) {
            debug!("bootstrap: system packages skipped");
        } else {
            self.run_hooks(&config, &hooks, BootstrapHookPhase::PrePackages)
                .await?;
            let all_mgrs = system::packages_from_config(&config)?;
            let has_plugin_packages = all_mgrs
                .iter()
                .any(|mp| mp.manager.is_plugin() && !mp.disabled)
                || (self.dry_run
                    && !system::pending_plugin_packages_from_config(&config).is_empty());
            let mgrs = all_mgrs
                .into_iter()
                .filter(|mp| !mp.manager.is_plugin())
                .collect::<Vec<_>>();
            if mgrs.is_empty() {
                debug!("bootstrap: no [bootstrap.packages] configured, skipping");
            } else {
                info!("bootstrap: system packages");
                follow_up.add_package_skips(&mgrs).await;
                let opts = DriverOpts {
                    manager: None,
                    explicit: false,
                    allow_unavailable_manager: false,
                    dry_run: self.dry_run,
                    update: self.update,
                    yes: self.yes,
                };
                driver::run(mgrs, Action::Install, &opts).await?;
            }
            if !has_plugin_packages {
                self.run_hooks(&config, &hooks, BootstrapHookPhase::PostPackages)
                    .await?;
                post_packages_ran = true;
            }
        }

        if skip.contains(&BootstrapPart::Files) {
            debug!("bootstrap: system files skipped");
        } else {
            let (mut files, mut directories) = managed_system_files
                .as_ref()
                .expect("system files were preflighted when not skipped")
                .clone();
            files
                .retain(|file| file.phase == system::managed_files::ManagedFilePhase::PostPackages);
            directories.retain(|directory| {
                directory.phase == system::managed_files::ManagedFilePhase::PostPackages
            });
            system::managed_files::inspect_requests(&mut files, &mut directories)?;
            if files.is_empty() && directories.is_empty() {
                debug!("bootstrap: no [bootstrap.files] or [bootstrap.directories] configured");
            } else {
                info!("bootstrap: system files");
                let report = system::managed_files::apply_with_accounts(
                    &files,
                    &directories,
                    configured_accounts.as_ref(),
                    allow_pending_accounts,
                    self.dry_run,
                    self.yes,
                )?;
                notified_services.extend(report.notified_services);
            }
        }

        if let Some(services) = &mut managed_services {
            system::services::inspect_requests(services);
            if services.is_empty() {
                debug!("bootstrap: no [bootstrap.services] configured");
            } else {
                info!("bootstrap: system services");
                system::services::apply_with_notifications(
                    services,
                    &notified_services,
                    self.dry_run,
                    self.yes,
                )?;
            }
            let early = user_services
                .iter()
                .filter(|request| !request.requires_tools)
                .cloned()
                .collect::<Vec<_>>();
            apply_user_services(&early, self.dry_run, self.yes, Some(&mut follow_up)).await?;
        } else {
            debug!("bootstrap: services skipped");
        }

        if skip.contains(&BootstrapPart::Firewall) {
            debug!("bootstrap: firewall skipped");
        } else if let Some(firewall) = &mut managed_firewall {
            system::firewall::inspect_request(firewall)?;
            info!("bootstrap: firewall");
            system::firewall::apply(firewall, self.dry_run, self.yes)?;
        } else {
            debug!("bootstrap: no [bootstrap.linux.firewall] configured");
        }

        if let Some(projects) = &mut managed_compose {
            system::compose::inspect_requests(projects);
            if projects.is_empty() {
                debug!("bootstrap: no [bootstrap.compose] configured");
            } else {
                info!("bootstrap: compose projects");
                system::compose::apply_with_dry_run_actions(
                    projects,
                    &dry_run_compose_actions,
                    self.dry_run,
                    self.yes,
                )?;
            }
        } else {
            debug!("bootstrap: compose projects skipped");
        }

        if skip.contains(&BootstrapPart::Repos) {
            debug!("bootstrap: repos skipped");
        } else {
            self.run_hooks(&config, &hooks, BootstrapHookPhase::PreRepos)
                .await?;
            let repos = system::repos_from_config(&config);
            if repos.is_empty() {
                debug!("bootstrap: no [bootstrap.repos] configured, skipping");
            } else {
                info!("bootstrap: repos");
                if self.update {
                    install::update_repos(repos, self.dry_run, self.yes, self.skip_dirty).await?;
                } else {
                    install::apply_repos(repos, self.dry_run, self.yes, self.skip_dirty).await?;
                }
            }
            self.run_hooks(&config, &hooks, BootstrapHookPhase::PostRepos)
                .await?;
        }

        if skip.contains(&BootstrapPart::Dotfiles) {
            debug!("bootstrap: dotfiles skipped");
            if !self.dry_run {
                config = Config::reset().await?;
                hooks = system::hooks_from_config(&config);
            }
        } else {
            self.run_hooks(&config, &hooks, BootstrapHookPhase::PreDotfiles)
                .await?;
            let files = system::files::files_from_config(&config)?;
            // loaded before any file is written: this also refuses an edit on
            // a file an absent entry removes
            let edits = system::edits::edits_from_config(&config)?;
            if files.is_empty() {
                debug!("bootstrap: no whole-file [dotfiles] entries configured, skipping");
            }
            if edits.is_empty() {
                debug!("bootstrap: no edit [dotfiles] entries configured, skipping");
            }
            // the same [history.reload] commands `mise dot apply` runs, for
            // the targets this phase writes
            if (!files.is_empty() || !edits.is_empty())
                && !write_and_reload(self.dry_run, |written| {
                    self.apply_dotfiles(&config, &files, &edits, &secrets, written)
                })?
            {
                return Ok(declined());
            }
            if self.dry_run {
                let config_files = config_files_after_dotfiles_dry_run(&config, &files, &edits)?;
                config = config.with_bootstrap_dry_run_config_files(config_files.clone())?;
                hooks = system::hooks_from_config(&config);
                dry_run_config_files = Some(config_files);
            } else {
                config = Config::reset().await?;
                hooks = system::hooks_from_config(&config);
            }
            self.run_hooks(&config, &hooks, BootstrapHookPhase::PostDotfiles)
                .await?;
        }

        if skip.contains(&BootstrapPart::Shell) {
            debug!("bootstrap: shell activation skipped");
        } else {
            let activations = dry_run_config_files
                .as_ref()
                .map(system::shell_activation_from_config_files)
                .unwrap_or_else(|| system::shell_activation_from_config(&config));
            if activations.is_empty() {
                debug!("bootstrap: no [bootstrap.mise_shell_activate] configured, skipping");
            } else {
                info!("bootstrap: shell activation");
                install::apply_shell_activation(&config, activations, self.dry_run, self.yes)?;
            }
        }

        if skip.contains(&BootstrapPart::Defaults) {
            debug!("bootstrap: system defaults skipped");
        } else {
            self.run_hooks(&config, &hooks, BootstrapHookPhase::PreDefaults)
                .await?;
            let defaults = system::defaults_from_config(&config);
            if defaults.is_empty() {
                debug!("bootstrap: no [bootstrap.macos.defaults] configured, skipping");
            } else {
                info!("bootstrap: system defaults");
                let requested = defaults.len();
                let report =
                    install::apply_defaults_with_report(defaults, self.dry_run, self.yes, false)
                        .await?;
                if report.needs_follow_up {
                    follow_up.add_macos_defaults();
                }
                if let Some(reason) = report.skipped_reason {
                    follow_up.add_skipped(format!(
                        "macOS defaults: {requested} entry(ies) skipped ({reason})"
                    ));
                }
            }
            self.run_hooks(&config, &hooks, BootstrapHookPhase::PostDefaults)
                .await?;
        }

        if skip.contains(&BootstrapPart::Launchd) {
            debug!("bootstrap: launchd agents skipped");
        } else {
            let agents = system::launchd_from_config(&config);
            if agents.is_empty() {
                debug!("bootstrap: no [bootstrap.macos.launchd.agents] configured, skipping");
            } else {
                info!("bootstrap: launchd agents");
                let requested = agents.len();
                let report =
                    install::apply_launchd_with_report(agents, self.dry_run, self.yes).await?;
                if let Some(reason) = report.skipped_reason {
                    follow_up
                        .add_skipped(format!("launchd: {requested} agent(s) skipped ({reason})"));
                }
            }
        }

        if skip.contains(&BootstrapPart::Systemd) {
            debug!("bootstrap: systemd user services skipped");
        } else {
            let units = system::systemd_from_config(&config);
            if units.is_empty() {
                debug!("bootstrap: no [bootstrap.linux.systemd.units] configured, skipping");
            } else {
                info!("bootstrap: systemd user services");
                let requested = units.len();
                let report =
                    install::apply_systemd_with_report(units, self.dry_run, self.yes).await?;
                if let Some(reason) = report.skipped_reason {
                    follow_up
                        .add_skipped(format!("systemd: {requested} unit(s) skipped ({reason})"));
                }
            }
        }

        if skip.contains(&BootstrapPart::User) {
            debug!("bootstrap: login shell skipped");
        } else {
            self.run_hooks(&config, &hooks, BootstrapHookPhase::PreUser)
                .await?;
            let login_shell = system::login_shell_from_config(&config);
            if login_shell.is_none() {
                debug!("bootstrap: no [bootstrap.user].login_shell configured, skipping");
            } else {
                let shell = login_shell.as_ref().map(|r| r.shell.clone());
                info!("bootstrap: login shell");
                let report = install::apply_login_shell_with_report(
                    login_shell,
                    self.dry_run,
                    self.yes,
                    false,
                )?;
                if report.needs_follow_up
                    && let Some(shell) = shell.as_ref()
                {
                    follow_up.add_login_shell(shell);
                }
                if let Some(reason) = report.skipped_reason
                    && let Some(shell) = shell
                {
                    follow_up.add_skipped(format!("login shell {shell}: skipped ({reason})"));
                }
            }
            self.run_hooks(&config, &hooks, BootstrapHookPhase::PostUser)
                .await?;
        }

        if skip.contains(&BootstrapPart::Tools) {
            debug!("bootstrap: tools skipped");
        } else {
            self.run_hooks(&config, &hooks, BootstrapHookPhase::PreTools)
                .await?;
            info!("bootstrap: tools");
            Install::new_bare(self.dry_run, self.yes).run().await?;
            if !self.dry_run {
                config = Config::reset().await?;
                hooks = system::hooks_from_config(&config);
            }
            self.run_hooks(&config, &hooks, BootstrapHookPhase::PostTools)
                .await?;
        }

        if !skip.contains(&BootstrapPart::Packages) {
            let mgrs = system::packages_from_config(&config)?
                .into_iter()
                .filter(|mp| mp.manager.is_plugin())
                .collect::<Vec<_>>();
            if !mgrs.is_empty() {
                info!("bootstrap: plugin packages");
                follow_up.add_package_skips(&mgrs).await;
                driver::run(
                    mgrs,
                    Action::Install,
                    &DriverOpts {
                        manager: None,
                        explicit: false,
                        allow_unavailable_manager: false,
                        dry_run: self.dry_run,
                        update: self.update,
                        yes: self.yes,
                    },
                )
                .await?;
            }
            if self.dry_run {
                for (name, requests) in system::pending_plugin_packages_from_config(&config) {
                    let packages = requests
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join(", ");
                    info!("{name}: would install {packages}");
                }
            }
            if !post_packages_ran {
                self.run_hooks(&config, &hooks, BootstrapHookPhase::PostPackages)
                    .await?;
            }
        }

        // resolved again: the run may have installed a durable mise since
        // the requests were first built (a remote-staged bootstrap)
        let late = if services_enabled {
            system::user_services::requests_from_config(&config)?
                .into_iter()
                .filter(|request| request.requires_tools)
                .collect::<Vec<_>>()
        } else {
            vec![]
        };
        if !late.is_empty() {
            if skip.contains(&BootstrapPart::Tools) {
                info!("bootstrap: tools skipped; user services with requires_tools still converge");
            }
            apply_user_services(&late, self.dry_run, self.yes, Some(&mut follow_up)).await?;
        }

        if skip.contains(&BootstrapPart::Task) {
            debug!("bootstrap: `bootstrap` task skipped");
        } else {
            let tasks = config.tasks().await?;
            if tasks.iter().any(|(_, t)| t.is_match("bootstrap")) {
                info!("bootstrap: running `bootstrap` task");
                self.run_task("bootstrap", skip.contains(&BootstrapPart::Tools))
                    .await?;
            } else {
                debug!("bootstrap: no `bootstrap` task defined, skipping");
            }
        }
        if skip.contains(&BootstrapPart::FinalHook) {
            debug!("bootstrap: final hook skipped");
        } else {
            self.run_hooks(&config, &hooks, BootstrapHookPhase::Final)
                .await?;
        }
        follow_up.print()?;
        Ok(summary)
    }

    async fn run_from(&self) -> Result<()> {
        let expanded = self
            .adopt
            .as_deref()
            .map(crate::github_relay::expand_repository)
            .transpose()?;
        // A setup repository must not bypass the released checkout-origin
        // guard and install its files inside another repository's checkout.
        if let Some(url) = expanded.as_deref() {
            let config_dir = system::history::tracked::global_config_dir();
            if config_dir.join(".git").exists() {
                validate_bootstrap_checkout(&config_dir, url)?;
            }
        }
        // a history-managed setup repository is not cloned into the
        // configuration directory: its branch goes into mise's own store and
        // its files are written by the same recoverable pull as any other
        // incoming change; the ordinary bootstrap then runs from them
        if let Some(url) = expanded.as_deref()
            && let Some(outcome) = system::history::sync::onboard::from_git(
                url,
                self.yes,
                self.dry_run,
                self.replace_history,
                self.take_remote_all,
            )
            .await?
        {
            if self.dry_run {
                if let Some(preview) = outcome.preview_config.as_ref() {
                    self.run_child_bootstrap(preview.path().to_path_buf())
                        .await?;
                }
                return Ok(());
            }
            // the configuration that arrived is what to bootstrap from; one
            // held for a decision leaves the existing one, whose tasks and
            // installations are not what was asked for
            if outcome.setup_held {
                bail!(
                    "the setup from {url} is paused; nothing was bootstrapped. `mise dot status` lists the paths that need attention. Existing files that differ are kept until you decide: `mise dot pull --take-remote-all` chooses the repository's version for every conflict, or `mise dot pull --take-remote <path>` and `mise dot pull --keep-local <path>` decide one at a time. A path held for another reason, such as a directory where the repository has a file, says so in `mise dot status` and needs that fix instead. Then run `mise bootstrap`"
                );
            }
            let config_dir = system::history::tracked::global_config_dir();
            self.run_child_bootstrap(config_dir).await?;
            if !outcome.durable_access {
                warn!("ongoing synchronization still needs credentials on this host (see above)");
            }
            return Ok(());
        }
        let (url, git_ref, checkout) = if let Some(url) = expanded.as_deref() {
            let checkout = crate::env::MISE_GLOBAL_CONFIG_FILE
                .as_deref()
                .map(|path| {
                    path.parent()
                        .filter(|parent| !parent.as_os_str().is_empty())
                        .unwrap_or_else(|| Path::new("."))
                })
                .unwrap_or(*dirs::CONFIG)
                .to_path_buf();
            (url.to_string(), None, checkout)
        } else {
            let (url, git_ref) = mise_util::remote_source::RemoteSource::parse_git_repo(
                self.from.as_deref().expect("--from was provided"),
            )?;
            (
                url,
                git_ref,
                self.from_dir
                    .clone()
                    .unwrap_or_else(|| dirs::DATA.join("bootstrap-repo")),
            )
        };

        let checkout_is_empty = checkout.is_dir() && checkout.read_dir()?.next().is_none();
        let reuse_checkout = checkout.exists() && !checkout_is_empty;
        if reuse_checkout {
            validate_bootstrap_checkout(&checkout, &url)?;
        }
        // The clone or pull changes the config checkout before the child
        // process records the bootstrap itself, so it is a generation of its
        // own. A checkout reused as-is changes nothing and records nothing.
        let mutates_checkout = !reuse_checkout || self.update;
        let generation = if mutates_checkout {
            Some(OperationScope::begin("bootstrap --from", self.dry_run).await?)
        } else {
            None
        };
        let checked_out = checkout_bootstrap_repository(
            &url,
            git_ref.as_deref(),
            &checkout,
            reuse_checkout,
            self.update,
            self.dry_run,
        );
        // the record says why a pull or clone failed, not just that it did
        if let Some(generation) = generation {
            match &checked_out {
                Ok(_) => generation.finish(
                    None,
                    Some(Summary {
                        message: Some(format!("checkout of {url}")),
                    }),
                ),
                Err(err) => generation.finish(Some(format!("{err:#}")), None),
            }
        }
        if !checked_out? {
            return Ok(());
        }

        self.run_child_bootstrap(checkout).await
    }

    /// Runs the bootstrap itself as a child process from `checkout` (the
    /// global configuration directory for `--adopt`), trusted for it.
    async fn run_child_bootstrap(&self, checkout: PathBuf) -> Result<()> {
        let checkout = dunce::canonicalize(&checkout)?;
        let mut command = Command::new(std::env::current_exe()?);
        command.args(bootstrap_from_child_args(
            &checkout,
            &crate::env::ARGS.read().unwrap(),
        ));
        if self.adopt.is_some() {
            if self.dry_run {
                command.env("MISE_CONFIG_DIR", &checkout);
                let name = crate::env::MISE_GLOBAL_CONFIG_FILE
                    .as_deref()
                    .and_then(Path::file_name)
                    .unwrap_or(std::ffi::OsStr::new("config.toml"));
                command.env("MISE_GLOBAL_CONFIG_FILE", checkout.join(name));
            } else {
                let config_dir = crate::env::MISE_CONFIG_DIR.as_path();
                let config_dir = if config_dir.is_absolute() {
                    config_dir.to_path_buf()
                } else {
                    std::env::current_dir()?.join(config_dir)
                };
                command.env("MISE_CONFIG_DIR", config_dir);

                if let Some(file_name) = crate::env::MISE_GLOBAL_CONFIG_FILE
                    .as_deref()
                    .and_then(Path::file_name)
                {
                    command.env("MISE_GLOBAL_CONFIG_FILE", checkout.join(file_name));
                }
            }
        }

        let mut trusted = std::env::split_paths(
            &std::env::var_os("MISE_TRUSTED_CONFIG_PATHS").unwrap_or_default(),
        )
        .collect::<Vec<_>>();
        trusted.push(checkout);
        command.env("MISE_TRUSTED_CONFIG_PATHS", std::env::join_paths(trusted)?);
        let status = command.status()?;
        if !status.success() {
            bail!("bootstrap from repository failed with {status}");
        }
        Ok(())
    }

    /// The dotfiles phase's whole-file entries, then its edits, appending
    /// each written target to `written`. Returns `false` when a prompt was
    /// declined.
    fn apply_dotfiles(
        &self,
        config: &Config,
        files: &[system::files::FileRequest],
        edits: &[system::edits::EditRequest],
        secrets: &system::secrets::SecretValues,
        written: &mut Vec<PathBuf>,
    ) -> Result<bool> {
        if !files.is_empty() {
            info!("bootstrap: dotfiles");
            let opts = system::files::ApplyOpts {
                dry_run: self.dry_run,
                verbose: false,
                force: self.force_dotfiles,
                force_hint: "use --force-dotfiles or run `mise dot apply --force`",
                yes: self.yes,
            };
            if !system::files::apply(config, files, &opts, secrets, written)? {
                return Ok(false);
            }
        }
        if !edits.is_empty() {
            info!("bootstrap: dotfile edits");
            let opts = system::edits::ApplyOpts {
                part: "dotfiles",
                dry_run: self.dry_run,
                verbose: false,
                yes: self.yes,
            };
            if !system::edits::apply(config, edits, &opts, written)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    async fn run_hooks(
        &self,
        config: &Config,
        hooks: &[hooks::BootstrapHook],
        phase: BootstrapHookPhase,
    ) -> Result<()> {
        // Recorded before the hooks run: a hook that fails may still have
        // changed the machine.
        run_bootstrap_hooks(config, hooks, phase, self.dry_run).await
    }

    fn skip_parts(&self) -> HashSet<BootstrapPart> {
        if self.only.is_empty() {
            self.skip.iter().map(|arg| arg.part).collect()
        } else {
            let only = self.only.iter().map(|arg| arg.part).collect::<HashSet<_>>();
            BootstrapPart::ALL
                .into_iter()
                .filter(|part| !only.contains(part))
                .collect()
        }
    }

    async fn run_task(&self, task: &str, skip_tools: bool) -> Result<()> {
        run::Run {
            task: Some(task.into()),
            args: vec![],
            args_last: vec![],
            all: false,
            affected: false,
            affected_base: None,
            affected_head: None,
            affected_explain: false,
            affected_json: false,
            cd: None,
            continue_on_error: false,
            dry_run: self.dry_run,
            force: false,
            is_linear: false,
            jobs: None,
            no_timings: false,
            output: None,
            shell: None,
            quiet: false,
            silent: false,
            raw: false,
            timings: false,
            tmpdir: Default::default(),
            tool: Default::default(),
            output_handler: None,
            context_builder: Default::default(),
            executor: None,
            telemetry: None,
            secrets_denied: Some(crate::secrets::SecretsDenied::Bootstrap),
            cli_secrets: None,
            secrets: vec![],
            secrets_all: false,
            no_cache: Default::default(),
            task_cache: crate::task::TaskCacheMode::from_env()?,
            task_cache_explain: false,
            task_cache_explain_json: false,
            task_cache_stats: false,
            timeout: None,
            skip_deps: false,
            // a dry run must not auto-install tools before the (not actually
            // run) task, and --skip tools must keep the task runner from
            // installing them implicitly before bootstrap tasks
            skip_tools: self.dry_run || skip_tools,
            no_deps: false,
            fresh_env: false,
            deny_all: false,
            deny_read: false,
            deny_write: false,
            deny_net: false,
            deny_env: false,
            allow_read: vec![],
            allow_write: vec![],
            allow_net: vec![],
            allow_env: vec![],
        }
        .run()
        .await
    }
}

/// Re-run the original bootstrap invocation from the checkout, preserving
/// global controls such as `--no-hooks`. Remove only arguments that describe
/// the parent checkout operation and replace any original working directory.
fn bootstrap_from_child_args(checkout: &Path, args: &[String]) -> Vec<OsString> {
    let mut forwarded = vec![OsString::from("--cd"), checkout.as_os_str().to_owned()];
    let mut args = args.iter().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--replace-history" | "--take-remote-all" => {}
            "--from" | "--adopt" | "--from-dir" | "--cd" | "-C" => {
                args.next();
            }
            _ if arg.starts_with("--from=")
                || arg.starts_with("--adopt=")
                || arg.starts_with("--from-dir=")
                || arg.starts_with("--cd=")
                || arg.starts_with("-C") && arg.len() > 2 => {}
            _ => {
                forwarded.push(arg.into());
                if global_option_takes_value(arg)
                    && let Some(value) = args.next()
                {
                    forwarded.push(value.into());
                }
            }
        }
    }
    forwarded
}

/// Re-run this invocation with `environments` selected, preserving every other
/// argument so options such as `--cd` and `--log-level` still apply.
fn unapply_child_args(environments: &str, args: &[String]) -> Vec<OsString> {
    let mut forwarded = vec![OsString::from("--env"), OsString::from(environments)];
    let mut args = args.iter().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            // The selection is replaced, and `--cd` already took effect in this
            // process: the child inherits that directory, and a relative path
            // would resolve a second time from it.
            "--env" | "-E" | "--profile" | "-P" | "--cd" | "-C" => {
                args.next();
            }
            _ if arg.starts_with("--env=")
                || arg.starts_with("--profile=")
                || arg.starts_with("--cd=")
                || (arg.starts_with("-E") || arg.starts_with("-P") || arg.starts_with("-C"))
                    && arg.len() > 2 => {}
            _ => {
                forwarded.push(arg.into());
                if global_option_takes_value(arg)
                    && let Some(value) = args.next()
                {
                    forwarded.push(value.into());
                }
            }
        }
    }
    forwarded
}

/// Whether a global option consumes the argument after it. Both child-argument
/// builders drop `--cd`/`-C` and the selection options before consulting this,
/// so those are listed for correctness rather than for a current caller.
fn global_option_takes_value(arg: &str) -> bool {
    matches!(
        arg,
        "--cd"
            | "-C"
            | "--env"
            | "-E"
            | "--jobs"
            | "-j"
            | "--profile"
            | "-P"
            | "--shell"
            | "-s"
            | "--tool"
            | "-t"
            | "--log-level"
            | "--output"
    )
}

struct BootstrapFollowUp {
    dry_run: bool,
    items: Vec<String>,
    printed: bool,
}

impl BootstrapFollowUp {
    fn new(dry_run: bool) -> Self {
        Self {
            dry_run,
            items: vec![],
            printed: false,
        }
    }

    async fn add_package_skips(&mut self, mgrs: &[system::ManagerPackages]) {
        for mp in mgrs {
            let name = mp.manager.name();
            let reason = if mp.disabled {
                Some("excluded by the system_packages.managers setting".to_string())
            } else {
                mp.manager.unavailable_reason_async().await
            };
            if let Some(reason) = reason {
                self.add_skipped(format!(
                    "{name}: {} package(s) skipped ({reason})",
                    mp.requests.len()
                ));
            }
        }
    }

    fn add_macos_defaults(&mut self) {
        self.items.push(
            "relaunch apps that read changed macOS defaults (for example: `killall Dock`, \
             `killall Finder`, `killall SystemUIServer`)"
                .to_string(),
        );
    }

    fn add_login_shell(&mut self, shell: &str) {
        self.items.push(format!(
            "start a new login session for {shell} to take effect"
        ));
    }

    fn add_skipped(&mut self, message: String) {
        self.items.push(message);
    }

    fn print(&mut self) -> Result<()> {
        self.printed = true;
        self.print_inner()
    }

    fn print_best_effort(&mut self) {
        if self.printed {
            return;
        }
        self.printed = true;
        if let Err(err) = self.print_inner() {
            debug!("bootstrap: failed to print follow-up after error: {err}");
        }
    }

    fn print_inner(&self) -> Result<()> {
        if self.items.is_empty() {
            return Ok(());
        }
        if self.dry_run {
            miseprintln!("bootstrap: follow-up if applied");
        } else {
            miseprintln!("bootstrap: follow-up");
        }
        for item in &self.items {
            miseprintln!("  - {item}");
        }
        Ok(())
    }
}

impl Drop for BootstrapFollowUp {
    fn drop(&mut self) {
        self.print_best_effort();
    }
}

fn config_files_after_dotfiles_dry_run(
    config: &Config,
    files: &[FileRequest],
    edits: &[system::edits::EditRequest],
) -> Result<config::ConfigMap> {
    let mut config_files = config.config_files.clone();
    let mut bodies = indexmap::IndexMap::new();
    let mut unavailable_bodies = HashSet::new();
    for file in files {
        if !is_mise_config_target(&file.target) {
            continue;
        }
        if file.mode == FileMode::Absent {
            // removed by the apply; a later edit starts from an empty file
            config_files.shift_remove(&file.target);
            bodies.insert(file.target.clone(), String::new());
            continue;
        }
        if file.mode != FileMode::Content && !file.source.is_file() {
            continue;
        }
        if file.mode == FileMode::Template {
            config_files.shift_remove(&file.target);
            unavailable_bodies.insert(file.target.clone());
            debug!(
                "bootstrap: template config target {} skipped in dry-run config simulation \
                 because template rendering may execute commands",
                file.target.display()
            );
            continue;
        }
        let contents = if file.mode == system::files::FileMode::Content {
            Ok(file.content.clone().expect("inline content"))
        } else {
            crate::file::read_to_string(&file.source)
        };
        match contents {
            Ok(body) => match parse_mise_config_body(&file.target, &body) {
                Ok(cf) => {
                    bodies.insert(file.target.clone(), body);
                    config_files.insert(file.target.clone(), cf);
                }
                Err(err) => {
                    warn!(
                        "[dotfiles].\"{}\": failed to parse config source {}: {err}",
                        file.target_raw,
                        file.source.display()
                    );
                }
            },
            Err(err) => {
                warn!(
                    "[dotfiles].\"{}\": failed to read config source {}: {err}",
                    file.target_raw,
                    file.source.display()
                );
            }
        }
    }
    for edit in edits {
        if !is_mise_config_target(&edit.path) {
            continue;
        }
        if unavailable_bodies.contains(&edit.path) {
            debug!(
                "bootstrap: edit for config target {} skipped in dry-run config simulation \
                 because the preceding file template was not rendered",
                edit.path.display()
            );
            continue;
        }
        let body = match bodies.get(&edit.path) {
            Some(body) => body.clone(),
            None if edit.path.exists() => match crate::file::read_to_string(&edit.path) {
                Ok(body) => body,
                Err(err) => {
                    warn!(
                        "[dotfiles].\"{}\": failed to read config target {}: {err}",
                        edit.config_key(),
                        edit.path.display()
                    );
                    continue;
                }
            },
            None => String::new(),
        };
        match system::edits::apply_dry_run_to_string(config, edit, &body) {
            Ok(Some(body)) => match parse_mise_config_body(&edit.path, &body) {
                Ok(cf) => {
                    bodies.insert(edit.path.clone(), body);
                    config_files.insert(edit.path.clone(), cf);
                }
                Err(err) => {
                    warn!(
                        "[dotfiles].\"{}\": failed to parse edited config target {}: {err}",
                        edit.config_key(),
                        edit.path.display()
                    );
                }
            },
            Ok(None) => {
                debug!(
                    "bootstrap: edited config target {} skipped in dry-run config simulation \
                     because the edit requires template rendering",
                    edit.path.display()
                );
            }
            Err(err) => {
                warn!(
                    "[dotfiles].\"{}\": failed to simulate config edit for {}: {err}",
                    edit.config_key(),
                    edit.path.display()
                );
            }
        }
    }
    Ok(config_files)
}

fn parse_mise_config_body(
    path: &std::path::Path,
    body: &str,
) -> Result<Arc<dyn config::config_file::ConfigFile>> {
    Ok(Arc::new(
        config::config_file::mise_toml::MiseToml::from_str(body, path)?,
    ))
}

fn is_mise_config_target(path: &std::path::Path) -> bool {
    path.starts_with(*dirs::CONFIG)
        || path.starts_with(*dirs::SYSTEM_CONFIG)
        || config::DEFAULT_CONFIG_FILENAMES.iter().any(|filename| {
            filename.ends_with(".toml") && !filename.contains('*') && path.ends_with(filename)
        })
        || (path.extension().is_some_and(|ext| ext == "toml")
            && path
                .parent()
                .is_some_and(|parent| parent.ends_with(".config/mise/conf.d")))
}

const DECLINED: &str = "dotfiles apply declined";

fn declined() -> Summary {
    Summary {
        message: Some(DECLINED.into()),
    }
}

fn is_declined(summary: &Summary) -> bool {
    summary.message.as_deref() == Some(DECLINED)
}

impl Commands {
    /// Boxed rather than `async` to keep debug builds' main stack small;
    /// see `cli::Commands::run`.
    fn run(self) -> LocalBoxFuture<'static, Result<()>> {
        match self {
            Self::ApplyAccountPlan(cmd) => Box::pin(async move { cmd.run() }),
            Self::ApplyServicePlan(cmd) => Box::pin(async move { cmd.run() }),
            Self::ApplyFirewallPlan(cmd) => Box::pin(async move { cmd.run() }),
            Self::ApplySystemPlan(cmd) => Box::pin(async move { cmd.run() }),
            Self::InspectSystemFiles(cmd) => Box::pin(async move { cmd.run() }),
            Self::InspectFirewallPlan(cmd) => Box::pin(async move { cmd.run() }),
            Self::ServiceExec(cmd) => Box::pin(cmd.run()),
            Self::Accounts(cmd) => Box::pin(cmd.run()),
            Self::ConfigRoots(cmd) => Box::pin(cmd.run()),
            Self::Compose(cmd) => Box::pin(cmd.run()),
            Self::Dotfiles(cmd) => Box::pin(cmd.run()),
            Self::Files(cmd) => Box::pin(cmd.run()),
            Self::Firewall(cmd) => Box::pin(cmd.run()),
            Self::Launchd(cmd) => {
                warn_legacy_bootstrap_command(
                    "bootstrap.launchd",
                    "launchd",
                    "macos launchd-agents",
                );
                Box::pin(cmd.run())
            }
            Self::Linux(cmd) => Box::pin(cmd.run()),
            Self::Macos(cmd) => Box::pin(cmd.run()),
            Self::MacosDefaults(cmd) => {
                warn_legacy_bootstrap_command(
                    "bootstrap.macos-defaults",
                    "macos-defaults",
                    "macos defaults",
                );
                Box::pin(cmd.run())
            }
            Self::MiseShellActivate(cmd) => Box::pin(cmd.run()),
            Self::Packages(cmd) => Box::pin(cmd.run()),
            Self::Plan(cmd) => Box::pin(cmd.run()),
            Self::Plugins(cmd) => Box::pin(cmd.run()),
            Self::Remote(cmd) => Box::pin(cmd.run()),
            Self::Repos(cmd) => Box::pin(cmd.run()),
            Self::Secrets(cmd) => Box::pin(cmd.run()),
            Self::Services(cmd) => Box::pin(cmd.run()),
            Self::Status(cmd) => Box::pin(cmd.run()),
            Self::Systemd(cmd) => {
                warn_legacy_bootstrap_command(
                    "bootstrap.systemd",
                    "systemd",
                    "linux systemd-units",
                );
                Box::pin(cmd.run())
            }
            Self::Unapply(cmd) => Box::pin(cmd.run()),
            Self::User(cmd) => Box::pin(cmd.run()),
        }
    }
}

impl BootstrapPlan {
    async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        let secrets = system::secrets::resolve(&config, self.prompt_secrets)?;
        let plan = system::resources::plan(&config, &secrets, true).await?;
        let output = plan.output()?;
        if self.json {
            miseprintln!("{}", serde_json::to_string_pretty(&output)?);
        } else if output.resources.is_empty() {
            info!("nothing configured for bootstrap planning");
        } else {
            let mut table = MiseTable::new(
                false,
                &[
                    "Action", "Resource", "Phase", "Current", "Desired", "Config",
                ],
            );
            for resource in &output.resources {
                table.add_row(vec![
                    resource.action.to_string(),
                    resource.id.to_string(),
                    resource
                        .phase
                        .map(|phase| phase.as_str())
                        .unwrap_or_default()
                        .to_string(),
                    resource.current.clone(),
                    resource.desired.clone(),
                    resource
                        .origin
                        .as_ref()
                        .map(|origin| origin.config.display_user())
                        .unwrap_or_default(),
                ]);
            }
            table.print()?;
            miseprintln!(
                "Plan: {} create, {} update, {} unchanged, {} remove, {} unknown",
                output.summary.create,
                output.summary.update,
                output.summary.unchanged,
                output.summary.remove,
                output.summary.unknown,
            );
        }
        if self.detailed_exitcode {
            if output.summary.has_unknown() {
                bail!("bootstrap plan contains resources with unknown state");
            }
            if output.summary.has_changes() {
                return Err(crate::request_exit(2));
            }
        }
        Ok(())
    }
}

impl BootstrapUnapply {
    async fn run(self) -> Result<()> {
        if let Some(status) = self.run_with_environments_selected()? {
            if !status.success() {
                bail!("bootstrap unapply failed with {status}");
            }
            return Ok(());
        }
        OperationScope::wrap("bootstrap unapply", self.dry_run, self.run_inner()).await
    }

    /// Re-run this command with the requested environments selected.
    ///
    /// Their config files are only loaded when they are part of the selection,
    /// and the rest of the selection has to stay in place so a resource another
    /// module still declares is visible. Returns `None` once nothing is missing.
    fn run_with_environments_selected(&self) -> Result<Option<std::process::ExitStatus>> {
        let selected = &*crate::env::MISE_ENV;
        if self
            .environment
            .iter()
            .all(|environment| selected.contains(environment))
        {
            return Ok(None);
        }
        let mut environments = selected.clone();
        for environment in &self.environment {
            if !environments.contains(environment) {
                environments.push(environment.clone());
            }
        }
        let mut command = Command::new(std::env::current_exe()?);
        command.args(unapply_child_args(
            &environments.join(","),
            &crate::env::ARGS.read().unwrap(),
        ));
        // The directory has already been entered, by `--cd` or by this variable.
        // The child inherits it, and a relative value would be applied a second
        // time, landing one level deeper or failing outright.
        command.env_remove("MISE_CD");
        Ok(Some(command.status()?))
    }

    async fn run_inner(self) -> Result<()> {
        let config = Config::get().await?;
        let secrets = system::secrets::resolve(&config, self.prompt_secrets)?;
        let opts = system::unapply::UnapplyOpts {
            dry_run: self.dry_run,
            force: self.force,
            verbose: config::Settings::get().verbose,
        };
        let unapply = system::unapply::plan(&config, &self.environment, &secrets, &opts).await?;
        for skip in &unapply.skipped {
            warn!("{} {}: keeping it, {}", skip.kind, skip.name, skip.reason);
        }
        for uncovered in &unapply.uncovered {
            info!(
                "{} declaration(s) in [{}] are not removed by unapply: {}",
                uncovered.count, uncovered.section, uncovered.command
            );
        }
        if unapply.is_empty() {
            info!("nothing to remove for {}", self.environment.join(", "));
            return Ok(());
        }
        let mut table = MiseTable::new(false, &["Action", "Resource"]);
        for removal in &unapply.removals {
            table.add_row(vec![
                "remove".to_string(),
                format!("{}:{}", removal.kind, removal.name),
            ]);
        }
        table.print()?;
        // The global `--yes`, `MISE_YES`, and the `yes` setting answer this
        // question as much as the subcommand flag does.
        if !self.dry_run && !self.yes && !config::Settings::get().yes {
            let message = format!(
                "bootstrap: remove {} resource(s) contributed by {}?",
                unapply.removals.len(),
                self.environment.join(", ")
            );
            // Defaults to no: this removes resources, so neither an unanswered
            // prompt nor one nobody saw may be read as consent.
            match crate::ui::prompt::confirm_with_default(message, false)? {
                Confirmation::Yes => {}
                Confirmation::No | Confirmation::Unanswered => {
                    info!("bootstrap unapply: skipped");
                    return Ok(());
                }
                Confirmation::Unavailable => bail!(
                    "mise bootstrap unapply requires confirmation but there was nobody to ask; pass --yes to remove non-interactively"
                ),
            }
        }
        system::unapply::execute(&config, &unapply, &secrets, &opts).await
    }
}

impl BootstrapConfigRoots {
    async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        let roots = config
            .selected_bootstrap_config_maps()
            .map(|(config_root, config_files)| {
                let mut environments = vec![];
                let mut declares = BootstrapConfigRootDeclarations::default();
                for cf in config_files.values().rev() {
                    let environment = config::environments_for_config_path(cf.get_path());
                    for name in &environment {
                        if !environments.contains(name) {
                            environments.push(name.clone());
                        }
                    }
                    let Some(bootstrap) = cf.bootstrap_config() else {
                        continue;
                    };
                    let origin = BootstrapDeclarationOrigin {
                        config: cf.get_path().to_path_buf(),
                        environment,
                    };
                    declares.packages.add(bootstrap.packages.len(), &origin);
                    declares.repos.add(bootstrap.repos.len(), &origin);
                    declares
                        .accounts
                        .add(bootstrap.users.len() + bootstrap.groups.len(), &origin);
                    declares.hooks.add(bootstrap.hooks.keys(), &origin);
                }
                BootstrapConfigRootOutput {
                    config_root: config_root.to_path_buf(),
                    environments,
                    declares,
                }
            })
            .collect();
        let output = BootstrapConfigRootsOutput { roots };

        if self.json {
            miseprintln!("{}", serde_json::to_string_pretty(&output)?);
        } else if output.roots.is_empty() {
            info!("no [bootstrap].config_roots configured");
        } else {
            let mut table = MiseTable::new(
                false,
                &[
                    "Config Root",
                    "Environments",
                    "Packages",
                    "Repos",
                    "Accounts",
                    "Hooks",
                    "Hook Phases",
                ],
            );
            for root in output.roots {
                table.add_row(vec![
                    root.config_root.display_user().to_string(),
                    root.environments.join(", "),
                    root.declares.packages.count.to_string(),
                    root.declares.repos.count.to_string(),
                    root.declares.accounts.count.to_string(),
                    root.declares.hooks.count.to_string(),
                    root.declares.hooks.phases.join(", "),
                ]);
            }
            table.print()?;
        }
        Ok(())
    }
}

impl BootstrapDeclarationSurface {
    fn add(&mut self, count: usize, origin: &BootstrapDeclarationOrigin) {
        if count == 0 {
            return;
        }
        self.count += count;
        if !self.provenance.contains(origin) {
            self.provenance.push(origin.clone());
        }
    }
}

impl BootstrapHookDeclarationSurface {
    fn add<'a>(
        &mut self,
        phases: impl IntoIterator<Item = &'a String>,
        origin: &BootstrapDeclarationOrigin,
    ) {
        let mut count = 0;
        for phase in phases {
            count += 1;
            let phase = BootstrapHookPhase::parse(phase)
                .map(|phase| phase.to_string())
                .unwrap_or_else(|| phase.clone());
            if !self.phases.contains(&phase) {
                self.phases.push(phase);
            }
        }
        if count > 0 {
            self.count += count;
            if !self.provenance.contains(origin) {
                self.provenance.push(origin.clone());
            }
        }
    }
}

impl BootstrapApplySystemPlan {
    fn run(self) -> Result<()> {
        system::managed_files::apply_privileged_plan_from_stdin()
    }
}

impl BootstrapApplyAccountPlan {
    fn run(self) -> Result<()> {
        system::accounts::apply_privileged_plan_from_stdin()
    }
}

impl BootstrapApplyServicePlan {
    fn run(self) -> Result<()> {
        system::services::apply_privileged_plan_from_stdin()
    }
}

impl BootstrapApplyFirewallPlan {
    fn run(self) -> Result<()> {
        system::firewall::apply_privileged_plan_from_stdin()
    }
}

impl BootstrapInspectFirewallPlan {
    fn run(self) -> Result<()> {
        system::firewall::inspect_privileged_plan_from_stdin()
    }
}

impl BootstrapInspectSystemFiles {
    fn run(self) -> Result<()> {
        system::managed_files::inspect_privileged_files_from_stdin()
    }
}

impl BootstrapAccounts {
    async fn run(self) -> Result<()> {
        match self.command {
            BootstrapAccountsCommands::Apply(command) => command.run().await,
            BootstrapAccountsCommands::Status(command) => command.run().await,
        }
    }
}

impl BootstrapAccountsApply {
    async fn run(self) -> Result<()> {
        OperationScope::wrap("bootstrap accounts apply", self.dry_run, self.run_inner()).await
    }

    async fn run_inner(self) -> Result<()> {
        let config = Config::get().await?;
        let requests = system::accounts::requests_from_config(&config)?;
        system::accounts::apply(&requests, self.dry_run, self.yes)?;
        Ok(())
    }
}

impl BootstrapAccountsStatus {
    async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        let requests = system::accounts::requests_from_config(&config)?;
        let resources = system::accounts::plans(&requests);
        let missing = resources
            .iter()
            .any(|resource| resource.action != system::resources::ResourceAction::Noop);
        if self.json {
            miseprintln!("{}", serde_json::to_string_pretty(&resources)?);
        } else if resources.is_empty() {
            info!("no bootstrap users or groups configured");
        } else {
            let mut table = MiseTable::new(false, &["Action", "Resource", "Current", "Desired"]);
            for resource in resources {
                table.add_row(vec![
                    resource.action.to_string(),
                    resource.id.to_string(),
                    resource.current,
                    resource.desired,
                ]);
            }
            table.print()?;
        }
        if self.missing && missing {
            return Err(crate::request_exit(1));
        }
        Ok(())
    }
}

impl BootstrapFiles {
    async fn run(self) -> Result<()> {
        match self.command {
            BootstrapFilesCommands::Apply(command) => command.run().await,
            BootstrapFilesCommands::Status(command) => command.run().await,
        }
    }
}

impl BootstrapFilesApply {
    async fn run(self) -> Result<()> {
        OperationScope::wrap("bootstrap files apply", self.dry_run, self.run_inner()).await
    }

    async fn run_inner(self) -> Result<()> {
        let config = Config::get().await?;
        let secrets = system::secrets::resolve(&config, self.prompt_secrets)?;
        let (files, directories) = system::managed_files::requests_from_config(&config, &secrets)?;
        let mut services = if files.iter().any(|file| !file.notify.is_empty())
            || directories
                .iter()
                .any(|directory| !directory.notify.is_empty())
        {
            let services = system::services::prepare_requests_from_config(&config)?;
            let user_services = system::services_common::user_service_names(&config)?;
            system::services::validate_notifications(
                &files,
                &directories,
                &services,
                &user_services,
            )?;
            Some(services)
        } else {
            None
        };
        let accounts = if cfg!(target_os = "linux") {
            Some(system::accounts::requests_from_config(&config)?)
        } else {
            None
        };
        let report = system::managed_files::apply_with_accounts(
            &files,
            &directories,
            accounts.as_ref(),
            false,
            self.dry_run,
            self.yes,
        )?;
        if let Some(services) = &mut services {
            system::services::inspect_requests(services);
            system::services::apply_with_notifications(
                services,
                &report.notified_services,
                self.dry_run,
                self.yes,
            )?;
        }
        Ok(())
    }
}

impl BootstrapFilesStatus {
    async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        let secrets = system::secrets::resolve(&config, self.prompt_secrets)?;
        let (files, directories, mut unavailable) =
            system::managed_files::status_requests_from_config(&config, &secrets)?;
        let accounts = system::accounts::prepare_requests_from_config(&config)?;
        system::managed_files::validate_principals(
            &files,
            &directories,
            cfg!(target_os = "linux").then_some(&accounts),
            false,
        )?;
        let mut resources = directories
            .into_iter()
            .map(|request| request.plan())
            .collect::<Result<Vec<_>>>()?;
        resources.extend(
            files
                .into_iter()
                .map(|request| request.plan())
                .collect::<Result<Vec<_>>>()?,
        );
        resources.append(&mut unavailable);
        let missing = resources
            .iter()
            .any(|resource| resource.action != system::resources::ResourceAction::Noop);
        if self.json {
            miseprintln!("{}", serde_json::to_string_pretty(&resources)?);
        } else if resources.is_empty() {
            info!("no system files or directories configured");
        } else {
            let mut table = MiseTable::new(
                false,
                &["Action", "Resource", "Current", "Desired", "Config"],
            );
            for resource in resources {
                table.add_row(vec![
                    resource.action.to_string(),
                    resource.id.to_string(),
                    resource.current,
                    resource.desired,
                    resource
                        .origin
                        .map(|origin| origin.config.display_user())
                        .unwrap_or_default(),
                ]);
            }
            table.print()?;
        }
        if self.missing && missing {
            return Err(crate::request_exit(1));
        }
        Ok(())
    }
}

impl BootstrapServiceExec {
    async fn run(self) -> Result<()> {
        // The service outlives every other thing this process has to do, so
        // it is waited for off the runtime's workers rather than on one.
        let code = tokio::task::spawn_blocking(move || {
            system::service_exec::run(&self.name, std::path::Path::new(&self.launch), &self.digest)
        })
        .await??;
        match code {
            0 => Ok(()),
            code => Err(crate::exit::request(code)),
        }
    }
}

impl BootstrapServices {
    async fn run(self) -> Result<()> {
        match self.command {
            BootstrapServicesCommands::Apply(command) => command.run().await,
            BootstrapServicesCommands::Remove(command) => command.run().await,
            BootstrapServicesCommands::Status(command) => command.run().await,
        }
    }
}

impl BootstrapServicesRemove {
    async fn run(self) -> Result<()> {
        OperationScope::wrap("bootstrap services remove", self.dry_run, self.run_inner()).await
    }

    async fn run_inner(self) -> Result<()> {
        let config = Config::get().await?;
        // best effort: a broken declaration must not block removal, which is
        // the recovery path for exactly that state
        let declared = system::user_services::requests_from_config(&config)
            .map(|requests| requests.iter().any(|request| request.name == self.name))
            .unwrap_or(false);
        let removed = system::user_services::remove_named(&self.name, self.dry_run).await?;
        let manager = system::user_services::manager_name();
        if !removed {
            info!("user service {}: no {manager} installed", self.name);
        } else if self.dry_run {
            info!("user service {}: would remove its {manager}", self.name);
        } else {
            info!("user service {}: removed its {manager}", self.name);
        }
        if declared {
            info!(
                "user service {} is still declared in [bootstrap.services]; the next `mise bootstrap` recreates it",
                self.name
            );
        }
        Ok(())
    }
}

impl BootstrapServicesApply {
    async fn run(self) -> Result<()> {
        OperationScope::wrap("bootstrap services apply", self.dry_run, self.run_inner()).await
    }

    async fn run_inner(self) -> Result<()> {
        let config = Config::get().await?;
        let requests = system::services::requests_from_config(&config)?;
        let user_requests = system::user_services::requests_from_config(&config)?;
        system::services::apply(&requests, self.dry_run, self.yes)?;
        apply_user_services(&user_requests, self.dry_run, self.yes, None).await
    }
}

/// Converge user-scope services, reporting an unavailable service manager as
/// a skipped follow-up instead of a failure.
async fn apply_user_services(
    requests: &[system::user_services::UserServiceRequest],
    dry_run: bool,
    yes: bool,
    follow_up: Option<&mut BootstrapFollowUp>,
) -> Result<()> {
    if requests.is_empty() {
        return Ok(());
    }
    info!("bootstrap: user services");
    if let Some(reason) = system::user_services::apply(requests, dry_run, yes).await? {
        let message = format!(
            "user services: {} service(s) skipped ({reason})",
            requests.len()
        );
        match follow_up {
            Some(follow_up) => follow_up.add_skipped(message),
            None => warn!("{message}"),
        }
    }
    Ok(())
}

impl BootstrapServicesStatus {
    async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        let requests = system::services::requests_from_config(&config)?;
        let mut resources = system::services::plans_with_notifications(
            &requests,
            &system::services::ServiceNotifications::default(),
        );
        let user_requests = system::user_services::requests_from_config(&config)?;
        resources.extend(
            system::user_services::status(&user_requests)
                .await?
                .iter()
                .map(|status| status.plan()),
        );
        let missing = resources
            .iter()
            .any(|resource| resource.action != system::resources::ResourceAction::Noop);
        if self.json {
            miseprintln!("{}", serde_json::to_string_pretty(&resources)?);
        } else if resources.is_empty() {
            info!("no bootstrap services configured");
        } else {
            let mut table = MiseTable::new(false, &["Action", "Resource", "Current", "Desired"]);
            for resource in resources {
                table.add_row(vec![
                    resource.action.to_string(),
                    resource.id.to_string(),
                    resource.current,
                    resource.desired,
                ]);
            }
            table.print()?;
        }
        if self.missing && missing {
            return Err(crate::request_exit(1));
        }
        Ok(())
    }
}

impl BootstrapFirewall {
    async fn run(self) -> Result<()> {
        match self.command {
            BootstrapFirewallCommands::Apply(command) => command.run().await,
            BootstrapFirewallCommands::Status(command) => command.run().await,
        }
    }
}

impl BootstrapFirewallApply {
    async fn run(self) -> Result<()> {
        OperationScope::wrap("bootstrap firewall apply", self.dry_run, self.run_inner()).await
    }

    async fn run_inner(self) -> Result<()> {
        let config = Config::get().await?;
        let Some(request) = system::firewall::request_from_config(&config)? else {
            info!("no bootstrap firewall configured");
            return Ok(());
        };
        system::firewall::apply(&request, self.dry_run, self.yes)
    }
}

impl BootstrapFirewallStatus {
    async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        let Some(request) = system::firewall::status_request_from_config(&config)? else {
            info!("no bootstrap firewall configured");
            return Ok(());
        };
        let resources = request.plans();
        let missing = resources
            .iter()
            .any(|resource| resource.action != system::resources::ResourceAction::Noop);
        if self.json {
            miseprintln!("{}", serde_json::to_string_pretty(&resources)?);
        } else {
            let mut table = MiseTable::new(false, &["Action", "Resource", "Current", "Desired"]);
            for resource in resources {
                table.add_row(vec![
                    resource.action.to_string(),
                    resource.id.to_string(),
                    resource.current,
                    resource.desired,
                ]);
            }
            table.print()?;
        }
        if self.missing && missing {
            return Err(crate::request_exit(1));
        }
        Ok(())
    }
}

impl BootstrapCompose {
    async fn run(self) -> Result<()> {
        match self.command {
            BootstrapComposeCommands::Apply(command) => command.run().await,
            BootstrapComposeCommands::Status(command) => command.run().await,
        }
    }
}

impl BootstrapComposeApply {
    async fn run(self) -> Result<()> {
        OperationScope::wrap("bootstrap compose apply", self.dry_run, self.run_inner()).await
    }

    async fn run_inner(self) -> Result<()> {
        let config = Config::get().await?;
        let requests = system::compose::requests_from_config(&config)?;
        system::compose::apply(&requests, self.dry_run, self.yes)
    }
}

impl BootstrapComposeStatus {
    async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        let requests = system::compose::requests_from_config(&config)?;
        let resources = system::compose::plans(&requests, false);
        let missing = resources
            .iter()
            .any(|resource| resource.action != system::resources::ResourceAction::Noop);
        if self.json {
            miseprintln!("{}", serde_json::to_string_pretty(&resources)?);
        } else if resources.is_empty() {
            info!("no bootstrap compose projects configured");
        } else {
            let mut table = MiseTable::new(false, &["Action", "Resource", "Current", "Desired"]);
            for resource in resources {
                table.add_row(vec![
                    resource.action.to_string(),
                    resource.id.to_string(),
                    resource.current,
                    resource.desired,
                ]);
            }
            table.print()?;
        }
        if self.missing && missing {
            return Err(crate::request_exit(1));
        }
        Ok(())
    }
}

impl BootstrapSecrets {
    async fn run(self) -> Result<()> {
        match self.command {
            BootstrapSecretsCommands::Status(command) => command.run().await,
        }
    }
}

impl BootstrapRemote {
    async fn run(self) -> Result<()> {
        warn_legacy_part_aliases(&self.only, &self.skip);
        crate::ui::ctrlc::exit_on_ctrl_c(false);
        let relay = crate::github_relay::Scope::from_flags(
            self.github_relay_read_only,
            &self.github_relay_repo,
            self.github_relay_all_repos,
        )?;
        let relay = crate::github_relay::configure(
            relay,
            self.github_relay_log_requests,
            self.github_relay_no_log_requests,
            self.github_relay_log_format.as_deref(),
            self.github_relay_max_duration.as_deref(),
        )?;
        if relay.is_some() && !cfg!(unix) {
            bail!("GitHub relay requires Linux or macOS");
        }
        if self.connect_timeout == 0 {
            bail!("--connect-timeout must be greater than zero");
        }
        let config = Config::get().await?;
        let config_excludes = system::remote::excludes_from_config(&config);
        let inventory = system::remote::hosts_from_config(&config, &config_excludes)?;
        let mut selected = select_remote_inventory(&inventory, &self.targets, self.all, &self.tag)?;
        let ad_hoc_source = self.source.clone().unwrap_or(std::env::current_dir()?);
        for destination in &self.host {
            let host =
                system::remote::ad_hoc_host(destination, ad_hoc_source.clone(), &config_excludes)?;
            if selected.insert(host.name.clone(), host).is_some() {
                bail!("remote target '{destination}' was selected more than once");
            }
        }
        if selected.is_empty() {
            if inventory.is_empty() {
                bail!(
                    "no remote targets configured; pass --host [user@]host or add [bootstrap.remote.hosts]"
                );
            }
            bail!(
                "select a remote target by name, --tag, or --all (configured: {})",
                inventory.keys().cloned().collect::<Vec<_>>().join(", ")
            );
        }

        let overrides = system::remote::RemoteOverrides {
            from_git: self.adopt.is_some(),
            source: self.source,
            mise_env: self.remote_env,
            copy_links: self.copy_links,
            copy_link: self.copy_link,
            port: self.port,
            identity_file: self.identity_file,
            exclude: self.exclude,
            ssh_options: self.ssh_option,
            install_mise: self.install_mise,
            no_install_mise: self.no_install_mise,
            mise_bin: self.mise_bin,
            remote_mise: self.remote_mise,
            bootstrap_command: self.bootstrap_command,
        };
        let options = system::remote::RemoteRunOptions {
            relay,
            dry_run: self.dry_run,
            yes: self.yes,
            update: self.update,
            prompt_secrets: self.prompt_secrets,
            force_dotfiles: self.force_dotfiles,
            skip: self
                .skip
                .iter()
                .map(|arg| bootstrap_part_name(&arg.part))
                .collect(),
            only: self
                .only
                .iter()
                .map(|arg| bootstrap_part_name(&arg.part))
                .collect(),
            keep_staging: self.keep_staging,
            connect_timeout: self.connect_timeout,
        };
        let mut configuration_errors = vec![];
        for host in selected.values_mut() {
            if let Err(error) = host.apply_overrides(&overrides) {
                configuration_errors.push(format!("{}: {error:#}", host.name));
            }
        }
        if !configuration_errors.is_empty() {
            bail!(
                "remote bootstrap configuration is invalid for {} target(s):\n  {}",
                configuration_errors.len(),
                configuration_errors.join("\n  ")
            );
        }
        let mut failures = vec![];
        let repository = self
            .adopt
            .as_deref()
            .map(crate::github_relay::expand_repository)
            .transpose()?;
        if let Some(origin) = &repository {
            system::remote_repository::validate_origin(origin)?;
        }
        // a dry run fetches and transfers like a real one: the preview comes
        // from the target, which inspects itself and the repository
        let repository = if let Some(origin) = repository {
            Some(
                system::remote::interruptible(system::remote_repository::Source::fetch(origin))
                    .await?,
            )
        } else {
            None
        };
        let mut artifacts = system::remote::RemoteArtifactResolver::default();
        for host in selected.values() {
            if let Err(error) =
                system::remote::run(host, &options, &mut artifacts, repository.as_ref()).await
            {
                if matches!(
                    crate::exit::requested_exit_code(&error),
                    Some(129 | 130 | 143)
                ) {
                    return Err(error);
                }
                error!("remote bootstrap failed on {}: {error:#}", host.name);
                failures.push(format!("{}: {error:#}", host.name));
                if self.fail_fast {
                    break;
                }
            }
        }
        if !failures.is_empty() {
            bail!(
                "remote bootstrap failed on {} target(s):\n  {}",
                failures.len(),
                failures.join("\n  ")
            );
        }
        info!("remote bootstrap completed on {} target(s)", selected.len());
        Ok(())
    }
}

fn select_remote_inventory(
    inventory: &indexmap::IndexMap<String, system::remote::RemoteHost>,
    targets: &[String],
    all: bool,
    tags: &[String],
) -> Result<indexmap::IndexMap<String, system::remote::RemoteHost>> {
    let mut selected = indexmap::IndexMap::new();
    for target in targets {
        let host = inventory.get(target).ok_or_else(|| {
            eyre::eyre!(
                "remote inventory target '{target}' not found; configured targets: {}",
                inventory.keys().cloned().collect::<Vec<_>>().join(", ")
            )
        })?;
        selected
            .entry(target.clone())
            .or_insert_with(|| host.clone());
    }
    if all {
        for (name, host) in inventory {
            selected.entry(name.clone()).or_insert_with(|| host.clone());
        }
    }
    if !tags.is_empty() {
        for (name, host) in inventory {
            if tags.iter().any(|tag| host.tags.contains(tag)) {
                selected.entry(name.clone()).or_insert_with(|| host.clone());
            }
        }
    }
    Ok(selected)
}

fn bootstrap_part_name(part: &BootstrapPart) -> String {
    match part {
        BootstrapPart::Shell => "mise-shell-activate".to_string(),
        BootstrapPart::Defaults => "macos-defaults".to_string(),
        BootstrapPart::Launchd => "macos-launchd-agents".to_string(),
        BootstrapPart::Systemd => "linux-systemd-units".to_string(),
        part => format!("{part:?}").to_kebab_case(),
    }
}

impl BootstrapSecretsStatus {
    async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        let statuses = system::secrets::statuses(&config)?;
        let unavailable = statuses
            .iter()
            .any(|status| status.state != system::secrets::SecretState::Available);
        if self.json {
            miseprintln!("{}", serde_json::to_string_pretty(&statuses)?);
        } else if statuses.is_empty() {
            info!("no bootstrap secret inputs configured");
        } else {
            let mut table = MiseTable::new(false, &["Secret", "Environment", "State"]);
            for status in statuses {
                table.add_row(vec![status.name, status.env, status.state.to_string()]);
            }
            table.print()?;
        }
        if self.missing && unavailable {
            return Err(crate::request_exit(1));
        }
        Ok(())
    }
}

struct BootstrapStatusReport {
    rows: Vec<Vec<String>>,
    json: serde_json::Map<String, Value>,
    any_missing: bool,
}

impl BootstrapStatusReport {
    fn new() -> Self {
        Self {
            rows: vec![],
            json: serde_json::Map::new(),
            any_missing: false,
        }
    }

    fn row(
        &mut self,
        part: impl Into<String>,
        item: impl Into<String>,
        current: impl Into<String>,
        state: impl Into<String>,
        missing: bool,
    ) {
        self.any_missing |= missing;
        self.rows
            .push(vec![part.into(), item.into(), current.into(), state.into()]);
    }

    fn append(&mut self, mut other: Self) {
        self.any_missing |= other.any_missing;
        self.rows.append(&mut other.rows);
        self.json.append(&mut other.json);
    }
}

impl BootstrapStatus {
    async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        let secrets = system::secrets::resolve(&config, self.prompt_secrets)?;
        let report = self.collect(&config, &secrets).await?;
        if self.json {
            miseprintln!("{}", serde_json::to_string_pretty(&report.json)?);
        } else if report.rows.is_empty() {
            info!("nothing configured for bootstrap");
        } else {
            let mut table = MiseTable::new(false, &["Part", "Item", "Current", "State"]);
            for row in report.rows {
                table.add_row(row);
            }
            table.print()?;
        }
        if self.missing && report.any_missing {
            return Err(crate::request_exit(1));
        }
        Ok(())
    }

    async fn collect(
        &self,
        config: &Arc<Config>,
        secrets: &system::secrets::SecretValues,
    ) -> Result<BootstrapStatusReport> {
        let mut report = BootstrapStatusReport::new();
        let (files, directories, unavailable_files) =
            system::managed_files::status_requests_from_config(config, secrets)?;
        let accounts = system::accounts::prepare_requests_from_config(config)?;
        system::managed_files::validate_principals(
            &files,
            &directories,
            cfg!(target_os = "linux").then_some(&accounts),
            false,
        )?;
        let service_requests = system::services::status_requests_from_config(config)?;
        let firewall_request = system::firewall::status_request_from_config(config)?;
        let user_services = system::services_common::user_service_names(config)?;
        system::services::validate_notifications(
            &files,
            &directories,
            &service_requests,
            &user_services,
        )?;
        let notified_services = system::managed_files::pending_notifications(&files, &directories)?;
        let compose_requests = system::compose::requests_from_config(config)?;
        // Dotfile templates may consume bootstrap secrets. Inspect them before
        // reporting used secrets, but append their rows in the usual order.
        let mut dotfiles_report = BootstrapStatusReport::new();
        self.collect_dotfiles(config, secrets, &mut dotfiles_report)?;
        self.collect_secrets(&secrets.used_statuses()?, &mut report);
        self.collect_packages(config, &mut report).await?;
        self.collect_accounts(&accounts, &mut report);
        self.collect_files(files, directories, unavailable_files, &mut report)?;
        self.collect_services(&service_requests, &notified_services, &mut report);
        self.collect_user_services(config, &mut report).await?;
        self.collect_firewall(firewall_request.as_ref(), &mut report);
        self.collect_compose(&compose_requests, &mut report);
        self.collect_repos(config, &mut report).await?;
        report.append(dotfiles_report);
        self.collect_shell(config, &mut report)?;
        self.collect_defaults(config, &mut report).await?;
        self.collect_launchd(config, &mut report).await?;
        self.collect_systemd(config, &mut report).await?;
        self.collect_user(config, &mut report)?;
        self.collect_tools(config, &mut report).await?;
        self.collect_plugin_deps(config, &mut report).await?;
        Ok(report)
    }

    fn collect_secrets(
        &self,
        statuses: &[system::secrets::SecretStatus],
        report: &mut BootstrapStatusReport,
    ) {
        for status in statuses {
            report.row(
                "secret",
                &status.name,
                status.state.to_string(),
                &status.env,
                status.state != system::secrets::SecretState::Available,
            );
        }
        report.json.insert("secrets".to_string(), json!(statuses));
    }

    fn collect_accounts(
        &self,
        requests: &system::accounts::AccountRequests,
        report: &mut BootstrapStatusReport,
    ) {
        let resources = system::accounts::plans(requests);
        for resource in &resources {
            report.row(
                resource.id.kind.clone(),
                resource.id.name.clone(),
                resource.current.clone(),
                resource.action.to_string(),
                resource.action != system::resources::ResourceAction::Noop,
            );
        }
        report.json.insert("accounts".to_string(), json!(resources));
    }

    fn collect_files(
        &self,
        files: Vec<system::managed_files::ManagedFileRequest>,
        directories: Vec<system::managed_files::ManagedDirectoryRequest>,
        mut unavailable: Vec<system::resources::ResourcePlan>,
        report: &mut BootstrapStatusReport,
    ) -> Result<()> {
        let mut resources = directories
            .into_iter()
            .map(|request| request.plan())
            .collect::<Result<Vec<_>>>()?;
        resources.extend(
            files
                .into_iter()
                .map(|request| request.plan())
                .collect::<Result<Vec<_>>>()?,
        );
        resources.append(&mut unavailable);
        for resource in &resources {
            report.row(
                resource.id.kind.clone(),
                resource.id.name.clone(),
                resource.current.clone(),
                resource.action.to_string(),
                resource.action != system::resources::ResourceAction::Noop,
            );
        }
        report.json.insert("files".to_string(), json!(resources));
        Ok(())
    }

    fn collect_services(
        &self,
        requests: &[system::services::ServiceRequest],
        notified_services: &system::services::ServiceNotifications,
        report: &mut BootstrapStatusReport,
    ) {
        let resources = system::services::plans_with_notifications(requests, notified_services);
        for resource in &resources {
            report.row(
                resource.id.kind.clone(),
                resource.id.name.clone(),
                resource.current.clone(),
                resource.action.to_string(),
                resource.action != system::resources::ResourceAction::Noop,
            );
        }
        report.json.insert("services".to_string(), json!(resources));
    }

    async fn collect_user_services(
        &self,
        config: &Arc<Config>,
        report: &mut BootstrapStatusReport,
    ) -> Result<()> {
        let requests = system::user_services::requests_from_config(config)?;
        let statuses = system::user_services::status(&requests).await?;
        for status in &statuses {
            let missing = status.action != system::resources::ResourceAction::Noop;
            report.row(
                "user-service",
                status.name.clone(),
                status.current.clone(),
                status.action.to_string(),
                missing,
            );
        }
        report
            .json
            .insert("user_services".to_string(), json!(statuses));
        Ok(())
    }

    fn collect_firewall(
        &self,
        request: Option<&system::firewall::FirewallRequest>,
        report: &mut BootstrapStatusReport,
    ) {
        let resources = request.map(|request| request.plans()).unwrap_or_default();
        for resource in &resources {
            report.row(
                resource.id.kind.clone(),
                resource.id.name.clone(),
                resource.current.clone(),
                resource.action.to_string(),
                resource.action != system::resources::ResourceAction::Noop,
            );
        }
        report.json.insert("firewall".to_string(), json!(resources));
    }

    fn collect_compose(
        &self,
        requests: &[system::compose::ComposeRequest],
        report: &mut BootstrapStatusReport,
    ) {
        let resources = system::compose::plans(requests, false);
        for resource in &resources {
            report.row(
                resource.id.kind.clone(),
                resource.id.name.clone(),
                resource.current.clone(),
                resource.action.to_string(),
                resource.action != system::resources::ResourceAction::Noop,
            );
        }
        report.json.insert("compose".to_string(), json!(resources));
    }

    async fn collect_plugin_deps(
        &self,
        config: &Arc<Config>,
        report: &mut BootstrapStatusReport,
    ) -> Result<()> {
        let trs = config.get_tool_request_set().await?;
        let mut seen = HashSet::new();
        let mut json_entries = vec![];
        for tr in trs.tools.values().flatten() {
            if !tr.is_os_supported() {
                continue;
            }
            let ba = tr.ba();
            if !seen.insert(ba.short.clone()) {
                continue;
            }
            let Some(backend) = crate::backend::get(ba) else {
                continue;
            };
            let deps = backend.system_dependencies();
            if deps.is_empty() {
                continue;
            }
            for status in crate::system::deps::detect(&deps).await {
                let optional = status.dep.optional.is_some();
                let (state, missing) = if status.satisfied {
                    ("satisfied".to_string(), false)
                } else if optional {
                    ("optional missing".to_string(), false)
                } else {
                    ("missing".to_string(), true)
                };
                report.row(
                    "plugin-deps",
                    format!("{}: {}", ba.tool_name, status.dep.label()),
                    status.found.clone().unwrap_or_default(),
                    state.clone(),
                    missing,
                );
                json_entries.push(json!({
                    "tool": ba.tool_name,
                    "dependency": status.dep.label(),
                    "found": status.found,
                    "optional": optional,
                    "state": state.replace(' ', "_"),
                }));
            }
        }
        report
            .json
            .insert("plugin_deps".to_string(), json!(json_entries));
        Ok(())
    }

    async fn collect_packages(
        &self,
        config: &Arc<Config>,
        report: &mut BootstrapStatusReport,
    ) -> Result<()> {
        let mut json_out = serde_json::Map::new();
        for mp in system::packages_from_config(config)? {
            let name = mp.manager.name();
            let reason = if mp.disabled {
                Some("excluded by the system_packages.managers setting".to_string())
            } else {
                mp.manager.unavailable_reason_async().await
            };
            if let Some(reason) = reason {
                for req in &mp.requests {
                    report.row(
                        "packages",
                        format!("{name}:{req}"),
                        "",
                        format!("skipped ({reason})"),
                        false,
                    );
                }
                json_out.insert(
                    name.to_string(),
                    json!({
                        "available": false,
                        "reason": reason,
                        "packages": mp.requests.iter().map(|req| {
                            json!({
                                "package": req.name,
                                "requested_version": req.version.clone().unwrap_or_else(|| "latest".to_string()),
                                "desired_state": match req.desired {
                                    PackageDesiredState::Present => "present",
                                    PackageDesiredState::Absent => "absent",
                                },
                                "state": "skipped",
                            })
                        }).collect::<Vec<_>>(),
                    }),
                );
                continue;
            }
            let statuses = mp
                .manager
                .installed_with_options(&mp.requests, &mp.options)
                .await?;
            let mut json_pkgs = vec![];
            for s in statuses {
                let auto_updates = s.state.auto_updates();
                let desired_absent = s.request.desired == PackageDesiredState::Absent;
                let (installed_version, state, reason, missing) = match (&s.state, desired_absent) {
                    (PackageState::Missing, true) => {
                        ("".to_string(), "absent", None::<&str>, false)
                    }
                    (PackageState::Installed { version }, true)
                    | (PackageState::NeedsRepair { installed: version }, true)
                    | (PackageState::VersionMismatch { installed: version }, true) => {
                        (version.clone(), "unexpectedly installed", None, true)
                    }
                    #[cfg(unix)]
                    (PackageState::InstalledAutoUpdates { version }, true) => {
                        (version.clone(), "unexpectedly installed", None, true)
                    }
                    (PackageState::Installed { version }, false) => {
                        (version.clone(), "installed", None::<&str>, false)
                    }
                    #[cfg(unix)]
                    (PackageState::InstalledAutoUpdates { version }, false) => {
                        (version.clone(), "installed", None::<&str>, false)
                    }
                    (PackageState::Missing, false) => ("".to_string(), "missing", None, true),
                    (PackageState::NeedsRepair { installed }, false) => {
                        (installed.clone(), "needs repair", None, true)
                    }
                    (PackageState::VersionMismatch { installed }, false) => {
                        (installed.clone(), "version mismatch", None, true)
                    }
                    #[cfg(unix)]
                    (PackageState::Unavailable { reason }, _) => {
                        ("".to_string(), "skipped", Some(reason.as_str()), false)
                    }
                };
                report.row(
                    "packages",
                    format!("{name}:{}", s.request),
                    installed_version.clone(),
                    if auto_updates {
                        format!("{state} (auto-updates)")
                    } else {
                        reason.map_or_else(
                            || state.to_string(),
                            |reason| format!("{state} ({reason})"),
                        )
                    },
                    missing,
                );
                let mut package = json!({
                    "package": s.request.name,
                    "requested_version": s.request.version.clone().unwrap_or_else(|| "latest".to_string()),
                    "desired_state": if desired_absent { "absent" } else { "present" },
                    "state": state.replace(' ', "_"),
                    "installed_version": installed_version,
                });
                if let Some(reason) = reason {
                    package["reason"] = json!(reason);
                }
                if auto_updates {
                    package["auto_updates"] = json!(true);
                }
                json_pkgs.push(package);
            }
            json_out.insert(
                name.to_string(),
                json!({ "available": true, "packages": json_pkgs }),
            );
        }
        report.json.insert("packages".to_string(), json!(json_out));
        Ok(())
    }

    async fn collect_repos(
        &self,
        config: &Arc<Config>,
        report: &mut BootstrapStatusReport,
    ) -> Result<()> {
        let repos = system::repos_from_config(config);
        let mut json_entries = vec![];
        for s in system::repos::status(&repos).await? {
            let state = s.state.as_str();
            let (row_state, reason, missing) = match &s.state {
                RepoState::Current => ("current".to_string(), "".to_string(), false),
                RepoState::Missing => ("missing".to_string(), "".to_string(), true),
                RepoState::Differs => ("differs".to_string(), "".to_string(), true),
                RepoState::Dirty => (
                    "dirty (local changes)".to_string(),
                    "local changes".to_string(),
                    true,
                ),
                RepoState::Conflict(reason) => {
                    (format!("conflict ({reason})"), reason.clone(), true)
                }
            };
            report.row(
                "repos",
                s.request.path_raw.clone(),
                s.current_ref.clone().unwrap_or_default(),
                row_state,
                missing,
            );
            json_entries.push(json!({
                "path": s.request.path,
                "path_raw": s.request.path_raw,
                "url": s.request.url,
                "ref": s.request.git_ref,
                "origin": s.origin,
                "current_ref": s.current_ref,
                "current_sha": s.current_sha,
                "state": state,
                "reason": reason,
            }));
        }
        report.json.insert("repos".to_string(), json!(json_entries));
        Ok(())
    }

    fn collect_dotfiles(
        &self,
        config: &Arc<Config>,
        secrets: &system::secrets::SecretValues,
        report: &mut BootstrapStatusReport,
    ) -> Result<()> {
        let mut json_files = vec![];
        let files = system::files::files_from_config(config)?;
        system::files::validate_composed_file_footprints(&files)?;
        for req in files {
            // an absent entry that cannot be checked (a directory at the
            // target, say) is an error, not a pending removal
            let (state, removable) = match system::files::check(config, &req, secrets) {
                Ok(state) => (state, true),
                Err(err) => (system::files::FileState::Differs(format!("{err}")), false),
            };
            let absent = req.mode == FileMode::Absent && removable;
            let (state_str, state_json, missing) = match &state {
                system::files::FileState::Applied if absent => {
                    ("absent".to_string(), "applied", false)
                }
                system::files::FileState::Applied => (
                    match system::files::permissions_target_absent(&req) {
                        Some(reason) => format!("applied ({reason})"),
                        None => "applied".to_string(),
                    },
                    "applied",
                    false,
                ),
                system::files::FileState::Missing => ("missing".to_string(), "missing", true),
                system::files::FileState::SourceMissing => {
                    ("source missing".to_string(), "source_missing", true)
                }
                system::files::FileState::Differs(reason) if absent => {
                    (format!("would remove ({reason})"), "differs", true)
                }
                system::files::FileState::Differs(reason) => {
                    (format!("differs ({reason})"), "differs", true)
                }
                system::files::FileState::Tracked => ("tracked".to_string(), "tracked", false),
            };
            report.row(
                "dotfiles",
                req.target_raw.clone(),
                match req.mode {
                    system::files::FileMode::Content => "content inline".to_string(),
                    system::files::FileMode::Permissions => {
                        format!("permissions {:04o}", req.permissions.unwrap_or_default())
                    }
                    system::files::FileMode::Absent => "absent".to_string(),
                    _ => format!("{} {}", req.mode.name(), req.source.display_user()),
                },
                state_str,
                missing,
            );
            let mut entry = json!({
                "target": req.target_raw,
                "source": req.mode.has_source()
                    .then(|| req.source.display_user()),
                "mode": req.mode.name(),
                "state": state_json,
            });
            if let system::files::FileState::Differs(reason) = &state {
                entry["reason"] = json!(if absent {
                    format!("{reason}; will be removed")
                } else {
                    reason.clone()
                });
            }
            json_files.push(entry);
        }

        let mut json_edits = vec![];
        for req in system::edits::edits_from_config(config)? {
            let state = match system::edits::check(config, &req) {
                Ok(state) => state,
                Err(err) => system::files::FileState::Differs(format!("{err}")),
            };
            let (state_str, state_json, missing) = match &state {
                system::files::FileState::Applied => ("applied".to_string(), "applied", false),
                system::files::FileState::Missing => ("missing".to_string(), "missing", true),
                system::files::FileState::SourceMissing => {
                    ("source missing".to_string(), "source_missing", true)
                }
                system::files::FileState::Differs(reason) => {
                    (format!("differs ({reason})"), "differs", true)
                }
                system::files::FileState::Tracked => ("tracked".to_string(), "tracked", false),
            };
            report.row(
                "dotfiles",
                req.path_raw.clone(),
                req.describe_op(),
                state_str,
                missing,
            );
            json_edits.push(json!({
                "path": req.path_raw,
                "edit": req.describe_op(),
                "state": state_json,
            }));
        }
        report.json.insert(
            "dotfiles".to_string(),
            json!({ "files": json_files, "edits": json_edits }),
        );
        Ok(())
    }

    fn collect_shell(
        &self,
        config: &Arc<Config>,
        report: &mut BootstrapStatusReport,
    ) -> Result<()> {
        let activations = system::shell_activation_from_config(config);
        let mut json_entries = vec![];
        for request in &activations {
            let state = match system::edits::check(config, &request.edit) {
                Ok(state) => state,
                Err(err) => FileState::Differs(format!("{err}")),
            };
            let missing = !matches!(state, FileState::Applied | FileState::Tracked);
            report.row(
                "shell",
                request.target.name(),
                format!(
                    "{} {} {}",
                    request.shell.name(),
                    request.edit.path_raw,
                    request.mode.name()
                ),
                file_state_display(&state),
                missing,
            );
            let mut entry = json!({
                "target": request.target.name(),
                "shell": request.shell.name(),
                "path": request.edit.path_raw,
                "mode": request.mode.name(),
                "state": file_state_json(&state),
            });
            if let FileState::Differs(reason) = &state {
                entry["reason"] = json!(reason);
            }
            json_entries.push(entry);
        }
        report
            .json
            .insert("mise_shell_activate".to_string(), json!(json_entries));
        Ok(())
    }

    async fn collect_defaults(
        &self,
        config: &Arc<Config>,
        report: &mut BootstrapStatusReport,
    ) -> Result<()> {
        let defaults = system::defaults_from_config(config);
        if defaults.is_empty() {
            report
                .json
                .insert("macos_defaults".to_string(), json!({ "entries": [] }));
            return Ok(());
        }
        if !system::defaults::is_available() {
            let reason = system::defaults::unavailable_reason();
            for req in &defaults {
                report.row(
                    "defaults",
                    format!("{} {}", req.display_domain(), req.display_key()),
                    "",
                    format!("skipped ({reason})"),
                    false,
                );
            }
            report.json.insert(
                "macos_defaults".to_string(),
                unavailable_defaults_json(&defaults, &reason),
            );
            return Ok(());
        }

        let mut json_entries = vec![];
        for s in system::defaults::status(&defaults).await? {
            let (current, state, missing) = match &s.state {
                DefaultsState::Set => (s.request.value.to_string(), "set", false),
                DefaultsState::Differs { current } => (current.clone(), "differs", true),
                DefaultsState::Unset => ("".to_string(), "unset", true),
            };
            report.row(
                "defaults",
                format!("{} {}", s.request.display_domain(), s.request.display_key()),
                current.clone(),
                state,
                missing,
            );
            json_entries.push(json!({
                "domain": s.request.domain,
                "host": s.request.host,
                "path": s.request.path,
                "key": s.request.key,
                "value": s.request.value.to_json(),
                "current": current,
                "state": state,
            }));
        }
        report.json.insert(
            "macos_defaults".to_string(),
            json!({ "available": true, "entries": json_entries }),
        );
        Ok(())
    }

    async fn collect_launchd(
        &self,
        config: &Arc<Config>,
        report: &mut BootstrapStatusReport,
    ) -> Result<()> {
        let agents = system::launchd_from_config(config);
        if agents.is_empty() {
            report
                .json
                .insert("launchd".to_string(), json!({ "agents": [] }));
            return Ok(());
        }
        if !system::launchd::is_available() {
            let reason = system::launchd::unavailable_reason();
            for req in &agents {
                report.row(
                    "launchd",
                    req.name.clone(),
                    req.label.clone(),
                    format!("skipped ({reason})"),
                    false,
                );
            }
            report.json.insert(
                "launchd".to_string(),
                json!({
                    "available": false,
                    "reason": reason,
                    "agents": agents.iter().map(|req| {
                        json!({
                            "name": req.name,
                            "label": req.label,
                            "state": "skipped",
                        })
                    }).collect::<Vec<_>>(),
                }),
            );
            return Ok(());
        }

        let mut json_entries = vec![];
        for s in system::launchd::status(&agents).await? {
            let (state, missing) = match &s.state {
                LaunchdState::Loaded => ("loaded", false),
                LaunchdState::Unloaded => ("unloaded", true),
                LaunchdState::Differs => ("differs", true),
                LaunchdState::Missing => ("missing", true),
            };
            report.row(
                "launchd",
                s.request.name.clone(),
                s.path.display().to_string(),
                state,
                missing,
            );
            json_entries.push(json!({
                "name": s.request.name,
                "label": s.request.label,
                "path": s.path,
                "loaded": s.loaded,
                "state": state,
            }));
        }
        report.json.insert(
            "launchd".to_string(),
            json!({ "available": true, "agents": json_entries }),
        );
        Ok(())
    }

    async fn collect_systemd(
        &self,
        config: &Arc<Config>,
        report: &mut BootstrapStatusReport,
    ) -> Result<()> {
        let units = system::systemd_from_config(config);
        if units.is_empty() {
            report
                .json
                .insert("systemd".to_string(), json!({ "units": [] }));
            return Ok(());
        }
        if !system::systemd::is_available() {
            let reason = system::systemd::unavailable_reason();
            let units = system::systemd::resolve_absent(&units);
            for req in &units {
                report.row(
                    "systemd",
                    req.name.clone(),
                    req.unit.clone(),
                    format!("skipped ({reason})"),
                    false,
                );
            }
            report.json.insert(
                "systemd".to_string(),
                json!({
                    "available": false,
                    "reason": reason,
                    "units": units.iter().map(|req| {
                        json!({
                            "name": req.name,
                            "unit": req.unit,
                            "state": "skipped",
                        })
                    }).collect::<Vec<_>>(),
                }),
            );
            return Ok(());
        }

        let mut json_entries = vec![];
        for s in system::systemd::status(&units).await? {
            let desired = s.is_desired();
            let state = s.label();
            let missing = !desired;
            report.row(
                "systemd",
                s.request.name.clone(),
                s.path.display().to_string(),
                state,
                missing,
            );
            json_entries.push(json!({
                "name": s.request.name,
                "unit": s.request.unit,
                "path": s.path,
                "active": s.active,
                "enabled": s.enabled,
                "desired": desired,
                "state": state,
            }));
        }
        report.json.insert(
            "systemd".to_string(),
            json!({ "available": true, "units": json_entries }),
        );
        Ok(())
    }

    fn collect_user(&self, config: &Arc<Config>, report: &mut BootstrapStatusReport) -> Result<()> {
        let Some(req) = system::login_shell_from_config(config) else {
            report.json.insert("login_shell".to_string(), json!(null));
            return Ok(());
        };
        if !system::login_shell::is_available() {
            let reason = system::login_shell::unavailable_reason();
            report.row(
                "user",
                "login_shell",
                "",
                format!("skipped ({reason})"),
                false,
            );
            report.json.insert(
                "login_shell".to_string(),
                json!({
                    "available": false,
                    "reason": reason,
                    "shell": req.shell,
                    "state": "skipped",
                }),
            );
            return Ok(());
        }

        let status = system::login_shell::status(&req)?;
        let (row_state, json_state, missing) = match &status.state {
            LoginShellState::Set => ("set", "set", false),
            LoginShellState::Differs { .. } => ("differs", "differs", true),
            LoginShellState::MissingFromShells { .. } => {
                ("missing from /etc/shells", "missing_from_shells", true)
            }
        };
        report.row(
            "user",
            "login_shell",
            status.current.clone(),
            row_state,
            missing,
        );
        report.json.insert(
            "login_shell".to_string(),
            json!({
                "available": true,
                "shell": status.request.shell,
                "user": status.user,
                "current": status.current,
                "shell_listed": status.shell_listed,
                "state": json_state,
            }),
        );
        Ok(())
    }

    async fn collect_tools(
        &self,
        config: &Arc<Config>,
        report: &mut BootstrapStatusReport,
    ) -> Result<()> {
        let trs = config.get_tool_request_set().await?;
        let mut json_tools = vec![];
        for ba in &trs.unknown_tools {
            report.row("tools", ba.to_string(), "", "unknown", true);
            json_tools.push(json!({
                "tool": ba.to_string(),
                "requested_version": null,
                "resolved_version": null,
                "state": "unknown",
                "installed": false,
            }));
        }
        for tr in trs.tools.values().flatten() {
            if !tr.is_os_supported() {
                continue;
            }
            let item = tr.to_string();
            let resolved = match tr.resolve(config, &ResolveOptions::default()).await {
                Ok(tv) => tv,
                Err(err) => {
                    let err = format!("{err:#}");
                    report.row(
                        "tools",
                        item.clone(),
                        "",
                        format!("resolve error ({err})"),
                        true,
                    );
                    json_tools.push(json!({
                        "tool": tr.ba().to_string(),
                        "requested_version": tr.version(),
                        "resolved_version": null,
                        "state": "resolve_error",
                        "installed": false,
                        "error": err,
                    }));
                    continue;
                }
            };
            let installed = {
                crate::backend::get(tr.ba())
                    .is_some_and(|backend| backend.is_version_installed(config, &resolved, true))
            };
            let resolved_version = resolved.version;
            let state = if installed { "installed" } else { "missing" };
            report.row(
                "tools",
                item.clone(),
                resolved_version.clone(),
                state,
                !installed,
            );
            json_tools.push(json!({
                "tool": tr.ba().to_string(),
                "requested_version": tr.version(),
                "resolved_version": resolved_version,
                "state": state,
                "installed": installed,
            }));
        }
        report.json.insert("tools".to_string(), json!(json_tools));
        Ok(())
    }
}

pub(crate) async fn run_dotfiles_apply(cmd: DotfilesApply) -> Result<()> {
    // one operation around both hook phases and the apply itself, so a
    // hook that edits a tracked file is inside the checkpoint pair
    let dry_run = cmd.dry_run();
    OperationScope::wrap("dotfiles apply", dry_run, async move {
        let mut config = Config::get().await?;
        let (files, edits) = cmd.requests(&config)?;
        let hooks = system::hooks_from_config(&config);
        run_bootstrap_hooks(&config, &hooks, BootstrapHookPhase::PreDotfiles, dry_run).await?;
        if !cmd.run_inner().await? {
            return Ok(());
        }
        let hooks = if dry_run {
            let config_files = config_files_after_dotfiles_dry_run(&config, &files, &edits)?;
            config = config.with_bootstrap_dry_run_config_files(config_files)?;
            system::hooks_from_config(&config)
        } else {
            config = Config::reset().await?;
            system::hooks_from_config(&config)
        };
        run_bootstrap_hooks(&config, &hooks, BootstrapHookPhase::PostDotfiles, dry_run).await
    })
    .await
}

/// Updates or clones the bootstrap repository. `Ok(false)` is a dry run
/// that stops here because there is no checkout to continue from.
fn bootstrap_git_succeeds<const N: usize>(checkout: &Path, args: [&str; N]) -> Result<bool> {
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(checkout)
        .args(args)
        .stdout(std::process::Stdio::null());
    crate::git::sanitize_git_command(&mut command);
    Ok(command.status()?.success())
}

/// Switches to the local branch `git_ref`, creating it from `origin/<ref>` when
/// it does not exist, even next to a same-named tag.
fn switch_to_bootstrap_branch(checkout: &Path, git_ref: &str) -> Result<()> {
    let branch = format!("refs/heads/{git_ref}");
    if bootstrap_git_succeeds(checkout, ["show-ref", "--verify", "--quiet", &branch])? {
        run_bootstrap_git(checkout, ["switch", git_ref])
    } else {
        // fully qualified so a local ref named `origin/<ref>` cannot shadow it
        let remote_branch = format!("refs/remotes/origin/{git_ref}");
        run_bootstrap_git(
            checkout,
            ["switch", "--create", git_ref, "--track", &remote_branch],
        )
    }
}

/// Checks out `git_ref` in a fresh clone, using the refs the clone fetched. A
/// branch wins over a tag of the same name.
fn checkout_bootstrap_ref(checkout: &Path, git_ref: &str) -> Result<()> {
    let remote_branch = format!("refs/remotes/origin/{git_ref}");
    if bootstrap_git_succeeds(
        checkout,
        ["show-ref", "--verify", "--quiet", &remote_branch],
    )? {
        switch_to_bootstrap_branch(checkout, git_ref)
    } else {
        run_bootstrap_git(checkout, ["checkout", git_ref, "--"])
    }
}

/// Moves an existing checkout to `git_ref` as origin now has it.
///
/// The ref is looked up on origin so that a name is never resolved from a
/// stale local copy: a branch on origin, which wins over a tag of the same
/// name, is switched to and fast-forwarded from `origin/<ref>`, and a tag on
/// origin is checked out as that tag. A branch or tag that only survives
/// locally, since fetching does not prune tags, is rejected. Anything else,
/// such as a commit, is checked out as given. Other local tags are left
/// alone, and a failing `ls-remote` is an error, not a deletion.
fn update_bootstrap_ref(checkout: &Path, git_ref: &str) -> Result<()> {
    // force tags so a moved one is not resolved from the stale local copy;
    // pruning removes only the remote-tracking branches deleted on origin
    run_bootstrap_git(
        checkout,
        ["fetch", "--force", "--tags", "--prune", "origin"],
    )?;
    let tag = format!("refs/tags/{git_ref}");
    let branch = format!("refs/heads/{git_ref}");
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(checkout)
        .args(["ls-remote", "origin", &tag, &branch]);
    crate::git::sanitize_git_command(&mut command);
    let output = command.output()?;
    if !output.status.success() {
        bail!(
            "could not look up {git_ref:?} on origin: git ls-remote failed with {}",
            output.status
        );
    }
    let listed = String::from_utf8_lossy(&output.stdout);
    let on_origin = |name: &str| listed.lines().any(|l| l.split('\t').nth(1) == Some(name));
    if on_origin(&branch) {
        switch_to_bootstrap_branch(checkout, git_ref)?;
        // from origin itself, whatever upstream the local branch tracks
        // fully qualified so a local ref named `origin/<ref>` cannot shadow it
        let remote_branch = format!("refs/remotes/origin/{git_ref}");
        run_bootstrap_git(checkout, ["merge", "--ff-only", &remote_branch])
    } else if on_origin(&tag) {
        run_bootstrap_git(checkout, ["checkout", &tag, "--"])
    } else if bootstrap_git_succeeds(checkout, ["show-ref", "--verify", "--quiet", &branch])?
        || bootstrap_git_succeeds(checkout, ["show-ref", "--verify", "--quiet", &tag])?
    {
        // a branch or tag that only survives locally would run stale
        // configuration, so it must not be picked up as a commit
        bail!("{git_ref:?} no longer exists on origin")
    } else {
        run_bootstrap_git(checkout, ["checkout", git_ref, "--"])
    }
}

fn checkout_bootstrap_repository(
    url: &str,
    git_ref: Option<&str>,
    checkout: &Path,
    reuse: bool,
    update: bool,
    dry_run: bool,
) -> Result<bool> {
    if reuse {
        if update {
            if dry_run {
                if let Some(git_ref) = git_ref {
                    miseprintln!(
                        "Would run: git -C {} fetch --force --tags --prune origin",
                        checkout.display_user()
                    );
                    miseprintln!(
                        "Would run: git -C {} checkout {git_ref} -- (or switch {git_ref} if it is a branch on origin)",
                        checkout.display_user()
                    );
                    miseprintln!(
                        "Would run: git -C {} merge --ff-only origin/{git_ref} (if it is a branch)",
                        checkout.display_user()
                    );
                } else {
                    miseprintln!(
                        "Would run: git -C {} pull --ff-only",
                        checkout.display_user()
                    );
                }
            } else {
                if let Some(git_ref) = git_ref {
                    update_bootstrap_ref(checkout, git_ref)?;
                } else {
                    run_bootstrap_git(checkout, ["pull", "--ff-only"])?;
                }
                journal::note(format!(
                    "updated the checkout of {url} in {}",
                    checkout.display_user()
                ));
            }
        }
        return Ok(true);
    }
    if dry_run {
        miseprintln!("Would run: git clone {} {}", url, checkout.display_user());
        if let Some(git_ref) = git_ref {
            miseprintln!(
                "Would run: git -C {} checkout {git_ref} --",
                checkout.display_user()
            );
        }
        return Ok(false);
    }
    if let Some(parent) = checkout
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)?;
    }
    let mut command = Command::new("git");
    command.arg("clone").arg(url).arg(checkout);
    crate::git::sanitize_git_command(&mut command);
    let status = command.status()?;
    if !status.success() {
        bail!("git clone failed with {status}");
    }
    if let Some(git_ref) = git_ref
        && let Err(err) = checkout_bootstrap_ref(checkout, git_ref)
    {
        // a clone left on the default branch would be reused as if it were
        // the requested ref by the next run
        let _ = std::fs::remove_dir_all(checkout);
        return Err(err.wrap_err(format!("could not check out {git_ref:?} from {url}")));
    }
    journal::note(match git_ref {
        Some(git_ref) => format!("cloned {url} at {git_ref} into {}", checkout.display_user()),
        None => format!("cloned {url} into {}", checkout.display_user()),
    });
    Ok(true)
}

fn bootstrap_hooks_enabled() -> bool {
    !(config::Settings::no_hooks()
        || config::Settings::get().no_hooks.unwrap_or(false)
        || config::Settings::get().safe)
}

async fn run_bootstrap_hooks(
    config: &Config,
    hooks: &[hooks::BootstrapHook],
    phase: BootstrapHookPhase,
    dry_run: bool,
) -> Result<()> {
    if !bootstrap_hooks_enabled() {
        debug!("bootstrap: {phase} hooks disabled");
        return Ok(());
    }
    hooks::run_phase(config, hooks, phase, dry_run).await
}

impl BootstrapPackages {
    async fn run(self) -> Result<()> {
        match self.command {
            BootstrapPackagesCommands::Apply(cmd) => cmd.run().await,
            #[cfg(unix)]
            BootstrapPackagesCommands::Brew(cmd) => cmd.run().await,
            BootstrapPackagesCommands::Export(cmd) => cmd.run().await,
            BootstrapPackagesCommands::Import(cmd) => cmd.run().await,
            BootstrapPackagesCommands::Prune(cmd) => cmd.run().await,
            BootstrapPackagesCommands::Status(cmd) => cmd.run().await,
            BootstrapPackagesCommands::Upgrade(cmd) => cmd.run().await,
            BootstrapPackagesCommands::Use(cmd) => cmd.run().await,
            BootstrapPackagesCommands::Where(cmd) => cmd.run().await,
        }
    }
}

impl BootstrapPlugins {
    async fn run(self) -> Result<()> {
        match self.command {
            BootstrapPluginsCommands::Apply(cmd) => cmd.run().await,
            BootstrapPluginsCommands::Status(cmd) => cmd.run().await,
        }
    }
}

impl BootstrapPluginsApply {
    async fn run(self) -> Result<()> {
        OperationScope::wrap("bootstrap plugins apply", self.dry_run, self.run_inner()).await
    }

    async fn run_inner(self) -> Result<()> {
        let config = Config::get().await?;
        apply_bootstrap_plugins(&config, self.dry_run).await
    }
}

impl BootstrapPluginsStatus {
    async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        let installed = crate::toolset::install_state::list_plugins();
        let mut any_missing = false;
        let mut table = MiseTable::new(false, &["Plugin", "URL", "State"]);
        for (name, url) in system::plugins_from_config(&config) {
            let present = installed.get(&name) == Some(&crate::plugins::PluginType::Package);
            any_missing |= !present;
            table.add_row(vec![
                name,
                url,
                if present { "installed" } else { "missing" }.into(),
            ]);
        }
        table.print()?;
        if self.missing && any_missing {
            return Err(crate::request_exit(1));
        }
        Ok(())
    }
}

async fn apply_bootstrap_plugins(config: &Arc<Config>, dry_run: bool) -> Result<()> {
    let plugins = system::plugins_from_config(config);
    if plugins.is_empty() {
        debug!("bootstrap: no [bootstrap.plugins] configured, skipping");
        return Ok(());
    }
    info!("bootstrap: package plugins");
    for (name, url) in plugins {
        install_plugin(
            config,
            &format!("package:{name}"),
            Some(url),
            false,
            dry_run,
        )
        .await?;
    }
    Ok(())
}

impl BootstrapRepos {
    async fn run(self) -> Result<()> {
        match self.command {
            BootstrapReposCommands::Apply(cmd) => cmd.run().await,
            BootstrapReposCommands::Exec(cmd) => cmd.run().await,
            BootstrapReposCommands::Status(cmd) => cmd.run().await,
            BootstrapReposCommands::Update(cmd) => cmd.run().await,
        }
    }
}

impl BootstrapReposApply {
    async fn run(self) -> Result<()> {
        OperationScope::wrap("bootstrap repos apply", self.dry_run, self.run_inner()).await
    }

    async fn run_inner(self) -> Result<()> {
        let config = Config::get().await?;
        install::apply_repos(
            system::repos_from_config(&config),
            self.dry_run,
            self.yes,
            self.skip_dirty,
        )
        .await
    }
}

impl BootstrapReposUpdate {
    async fn run(self) -> Result<()> {
        OperationScope::wrap("bootstrap repos update", self.dry_run, self.run_inner()).await
    }

    async fn run_inner(self) -> Result<()> {
        let config = Config::get().await?;
        let repos = filter_repos(system::repos_from_config(&config), &self.paths)?;
        install::update_repos(repos, self.dry_run, self.yes, self.skip_dirty).await
    }
}

impl BootstrapReposExec {
    async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        let repos = filter_repos(system::repos_from_config(&config), &self.paths)?;
        system::repos::exec(&repos, &self.command, self.dry_run, self.continue_on_error).await
    }
}

fn filter_repos(
    repos: Vec<system::repos::RepoRequest>,
    filters: &[String],
) -> Result<Vec<system::repos::RepoRequest>> {
    if filters.is_empty() {
        return Ok(repos);
    }
    for filter in filters {
        let expanded = crate::file::replace_path(filter);
        if !repos
            .iter()
            .any(|repo| repo.path_raw == *filter || repo.path == expanded)
        {
            eyre::bail!("no configured repo matched path: {filter}");
        }
    }
    Ok(repos
        .into_iter()
        .filter(|repo| {
            filters.iter().any(|filter| {
                repo.path_raw == *filter || repo.path == crate::file::replace_path(filter)
            })
        })
        .collect())
}

impl BootstrapReposStatus {
    async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        let repos = system::repos_from_config(&config);
        let mut any_missing = false;
        let mut rows: Vec<Vec<String>> = vec![];
        let mut json_entries = vec![];
        for s in system::repos::status(&repos).await? {
            if !s.state.is_current() {
                any_missing = true;
            }
            let state = s.state.as_str();
            let reason = match &s.state {
                RepoState::Conflict(reason) => reason.clone(),
                RepoState::Dirty => "local changes".to_string(),
                RepoState::Current | RepoState::Missing | RepoState::Differs => "".to_string(),
            };
            if self.json {
                json_entries.push(json!({
                    "path": s.request.path,
                    "path_raw": s.request.path_raw,
                    "url": s.request.url,
                    "ref": s.request.git_ref,
                    "origin": s.origin,
                    "current_ref": s.current_ref,
                    "current_sha": s.current_sha,
                    "state": state,
                    "reason": reason,
                }));
            } else {
                rows.push(vec![
                    s.request.path_raw,
                    s.request.url,
                    s.request.git_ref.unwrap_or_default(),
                    state.to_string(),
                    reason,
                ]);
            }
        }
        if self.json {
            let mut json_out = serde_json::Map::new();
            json_out.insert("repos".to_string(), json!(json_entries));
            miseprintln!("{}", serde_json::to_string_pretty(&json_out)?);
        } else if rows.is_empty() {
            info!("nothing configured in [bootstrap.repos]");
        } else {
            let mut table = MiseTable::new(false, &["Path", "URL", "Ref", "State", "Reason"]);
            for row in rows {
                table.add_row(row);
            }
            table.print()?;
        }
        if self.missing && any_missing {
            return Err(crate::request_exit(1));
        }
        Ok(())
    }
}

impl BootstrapMacos {
    async fn run(self) -> Result<()> {
        match self.command {
            BootstrapMacosCommands::Defaults(cmd) => cmd.run().await,
            BootstrapMacosCommands::LaunchdAgents(cmd) => cmd.run().await,
        }
    }
}

impl BootstrapLinux {
    async fn run(self) -> Result<()> {
        match self.command {
            BootstrapLinuxCommands::SystemdUnits(cmd) => cmd.run().await,
        }
    }
}

impl BootstrapMacosDefaults {
    async fn run(self) -> Result<()> {
        match self.command {
            BootstrapMacosDefaultsCommands::Apply(cmd) => cmd.run().await,
            BootstrapMacosDefaultsCommands::Status(cmd) => cmd.run().await,
        }
    }
}

impl BootstrapMacosDefaultsCompat {
    async fn run(self) -> Result<()> {
        match self.command {
            BootstrapMacosDefaultsCompatCommands::Apply(cmd) => cmd.run().await,
            BootstrapMacosDefaultsCompatCommands::Status(cmd) => cmd.run().await,
        }
    }
}

impl BootstrapLaunchd {
    async fn run(self) -> Result<()> {
        match self.command {
            BootstrapLaunchdCommands::Apply(cmd) => cmd.run().await,
            BootstrapLaunchdCommands::Status(cmd) => cmd.run().await,
        }
    }
}

impl BootstrapLaunchdCompat {
    async fn run(self) -> Result<()> {
        match self.command {
            BootstrapLaunchdCompatCommands::Apply(cmd) => cmd.run().await,
            BootstrapLaunchdCompatCommands::Status(cmd) => cmd.run().await,
        }
    }
}

impl BootstrapLaunchdApply {
    async fn run(self) -> Result<()> {
        OperationScope::wrap(
            "bootstrap macos launchd-agents apply",
            self.dry_run,
            self.run_inner(),
        )
        .await
    }

    async fn run_inner(self) -> Result<()> {
        let config = Config::get().await?;
        install::apply_launchd(system::launchd_from_config(&config), self.dry_run, self.yes).await
    }
}

impl BootstrapLaunchdStatus {
    async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        let agents = system::launchd_from_config(&config);
        let mut any_missing = false;
        let mut rows: Vec<Vec<String>> = vec![];
        let mut json_out = serde_json::Map::new();
        if !agents.is_empty() {
            if !system::launchd::is_available() {
                let reason = system::launchd::unavailable_reason();
                if self.json {
                    json_out.insert(
                        "launchd".to_string(),
                        json!({ "available": false, "reason": reason }),
                    );
                } else {
                    for req in &agents {
                        rows.push(vec![
                            req.name.clone(),
                            req.label.clone(),
                            "".to_string(),
                            format!("skipped ({reason})"),
                        ]);
                    }
                }
            } else {
                let statuses = system::launchd::status(&agents).await?;
                let mut json_entries = vec![];
                for s in statuses {
                    let state = match &s.state {
                        LaunchdState::Loaded => "loaded",
                        LaunchdState::Unloaded => {
                            any_missing = true;
                            "unloaded"
                        }
                        LaunchdState::Differs => {
                            any_missing = true;
                            "differs"
                        }
                        LaunchdState::Missing => {
                            any_missing = true;
                            "missing"
                        }
                    };
                    if self.json {
                        json_entries.push(json!({
                            "name": s.request.name,
                            "label": s.request.label,
                            "path": s.path,
                            "loaded": s.loaded,
                            "state": state,
                        }));
                    } else {
                        rows.push(vec![
                            s.request.name.clone(),
                            s.request.label.clone(),
                            s.path.display().to_string(),
                            state.to_string(),
                        ]);
                    }
                }
                if self.json {
                    json_out.insert(
                        "launchd".to_string(),
                        json!({ "available": true, "agents": json_entries }),
                    );
                }
            }
        }
        if self.json {
            miseprintln!("{}", serde_json::to_string_pretty(&json_out)?);
        } else if rows.is_empty() {
            info!("nothing configured in [bootstrap.macos.launchd.agents]");
        } else {
            let mut table = MiseTable::new(false, &["Name", "Label", "Path", "State"]);
            for row in rows {
                table.add_row(row);
            }
            table.print()?;
        }
        if self.missing && any_missing {
            return Err(crate::request_exit(1));
        }
        Ok(())
    }
}

impl BootstrapSystemd {
    async fn run(self) -> Result<()> {
        match self.command {
            BootstrapSystemdCommands::Apply(cmd) => cmd.run().await,
            BootstrapSystemdCommands::Status(cmd) => cmd.run().await,
        }
    }
}

impl BootstrapSystemdCompat {
    async fn run(self) -> Result<()> {
        match self.command {
            BootstrapSystemdCompatCommands::Apply(cmd) => cmd.run().await,
            BootstrapSystemdCompatCommands::Status(cmd) => cmd.run().await,
        }
    }
}

impl BootstrapSystemdApply {
    async fn run(self) -> Result<()> {
        OperationScope::wrap(
            "bootstrap linux systemd-units apply",
            self.dry_run,
            self.run_inner(),
        )
        .await
    }

    async fn run_inner(self) -> Result<()> {
        let config = Config::get().await?;
        install::apply_systemd(system::systemd_from_config(&config), self.dry_run, self.yes).await
    }
}

impl BootstrapSystemdStatus {
    async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        let units = system::systemd_from_config(&config);
        let mut any_missing = false;
        let mut rows: Vec<Vec<String>> = vec![];
        let mut json_out = serde_json::Map::new();
        if !units.is_empty() {
            if !system::systemd::is_available() {
                let reason = system::systemd::unavailable_reason();
                if self.json {
                    json_out.insert(
                        "systemd".to_string(),
                        json!({ "available": false, "reason": reason }),
                    );
                } else {
                    for req in &system::systemd::resolve_absent(&units) {
                        rows.push(vec![
                            req.name.clone(),
                            req.unit.clone(),
                            "".to_string(),
                            format!("skipped ({reason})"),
                        ]);
                    }
                }
            } else {
                let statuses = system::systemd::status(&units).await?;
                let mut json_entries = vec![];
                for s in statuses {
                    let desired = s.is_desired();
                    let state = s.label();
                    if !desired {
                        any_missing = true;
                    }
                    if self.json {
                        json_entries.push(json!({
                            "name": s.request.name,
                            "unit": s.request.unit,
                            "path": s.path,
                            "active": s.active,
                            "enabled": s.enabled,
                            "desired": desired,
                            "state": state,
                        }));
                    } else {
                        rows.push(vec![
                            s.request.name.clone(),
                            s.request.unit.clone(),
                            s.path.display().to_string(),
                            state.to_string(),
                        ]);
                    }
                }
                if self.json {
                    json_out.insert(
                        "systemd".to_string(),
                        json!({ "available": true, "units": json_entries }),
                    );
                }
            }
        }
        if self.json {
            miseprintln!("{}", serde_json::to_string_pretty(&json_out)?);
        } else if rows.is_empty() {
            info!("nothing configured in [bootstrap.linux.systemd.units]");
        } else {
            let mut table = MiseTable::new(false, &["Name", "Unit", "Path", "State"]);
            for row in rows {
                table.add_row(row);
            }
            table.print()?;
        }
        if self.missing && any_missing {
            return Err(crate::request_exit(1));
        }
        Ok(())
    }
}

impl BootstrapMacosDefaultsApply {
    async fn run(self) -> Result<()> {
        OperationScope::wrap(
            "bootstrap macos defaults apply",
            self.dry_run,
            self.run_inner(),
        )
        .await
    }

    async fn run_inner(self) -> Result<()> {
        let config = Config::get().await?;
        install::apply_defaults(
            system::defaults_from_config(&config),
            self.dry_run,
            self.yes,
        )
        .await
    }
}

fn unavailable_defaults_json(
    defaults: &[system::defaults::DefaultsRequest],
    reason: &str,
) -> serde_json::Value {
    json!({
        "available": false,
        "reason": reason,
        "entries": defaults.iter().map(|req| {
            json!({
                "domain": req.domain,
                "host": req.host,
                "path": req.path,
                "key": req.key,
                "value": req.value.to_json(),
                "state": "skipped",
            })
        }).collect::<Vec<_>>(),
    })
}

impl BootstrapMacosDefaultsStatus {
    async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        let defaults = system::defaults_from_config(&config);
        let mut any_missing = false;
        let mut rows: Vec<Vec<String>> = vec![];
        let mut json_out = serde_json::Map::new();
        if !defaults.is_empty() {
            if !system::defaults::is_available() {
                let reason = system::defaults::unavailable_reason();
                if self.json {
                    json_out.insert(
                        "macos_defaults".to_string(),
                        unavailable_defaults_json(&defaults, &reason),
                    );
                } else {
                    for req in &defaults {
                        rows.push(vec![
                            req.display_domain(),
                            req.display_key(),
                            req.value.to_string(),
                            "".to_string(),
                            format!("skipped ({reason})"),
                        ]);
                    }
                }
            } else {
                let statuses = system::defaults::status(&defaults).await?;
                let mut json_entries = vec![];
                for s in statuses {
                    let (current, state) = match &s.state {
                        DefaultsState::Set => (s.request.value.to_string(), "set"),
                        DefaultsState::Differs { current } => {
                            any_missing = true;
                            (current.clone(), "differs")
                        }
                        DefaultsState::Unset => {
                            any_missing = true;
                            ("".to_string(), "unset")
                        }
                    };
                    if self.json {
                        json_entries.push(json!({
                            "domain": s.request.domain,
                            "host": s.request.host,
                            "path": s.request.path,
                            "key": s.request.key,
                            "value": s.request.value.to_json(),
                            "current": current,
                            "state": state,
                        }));
                    } else {
                        rows.push(vec![
                            s.request.display_domain(),
                            s.request.display_key(),
                            s.request.value.to_string(),
                            current,
                            state.to_string(),
                        ]);
                    }
                }
                if self.json {
                    json_out.insert(
                        "macos_defaults".to_string(),
                        json!({ "available": true, "entries": json_entries }),
                    );
                }
            }
        }
        if self.json {
            miseprintln!("{}", serde_json::to_string_pretty(&json_out)?);
        } else if rows.is_empty() {
            info!("nothing configured in [bootstrap.macos.defaults]");
        } else {
            let mut table = MiseTable::new(false, &["Domain", "Key", "Value", "Current", "State"]);
            for row in rows {
                table.add_row(row);
            }
            table.print()?;
        }
        if self.missing && any_missing {
            return Err(crate::request_exit(1));
        }
        Ok(())
    }
}

impl BootstrapShell {
    async fn run(self) -> Result<()> {
        match self.command {
            BootstrapShellCommands::Apply(cmd) => cmd.run().await,
            BootstrapShellCommands::Status(cmd) => cmd.run().await,
        }
    }
}

impl BootstrapShellApply {
    async fn run(self) -> Result<()> {
        OperationScope::wrap(
            "bootstrap mise-shell-activate apply",
            self.dry_run,
            self.run_inner(),
        )
        .await
    }

    async fn run_inner(self) -> Result<()> {
        let config = Config::get().await?;
        install::apply_shell_activation(
            &config,
            system::shell_activation_from_config(&config),
            self.dry_run,
            self.yes,
        )
    }
}

impl BootstrapShellStatus {
    async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        let activations = system::shell_activation_from_config(&config);
        let mut any_missing = false;
        let mut rows: Vec<Vec<String>> = vec![];
        let mut json_entries = vec![];
        for request in &activations {
            let state = match system::edits::check(&config, &request.edit) {
                Ok(state) => state,
                Err(err) => FileState::Differs(format!("{err}")),
            };
            any_missing |= !matches!(state, FileState::Applied | FileState::Tracked);
            if self.json {
                let mut entry = json!({
                    "target": request.target.name(),
                    "shell": request.shell.name(),
                    "path": request.edit.path_raw,
                    "mode": request.mode.name(),
                    "state": file_state_json(&state),
                });
                if let FileState::Differs(reason) = &state {
                    entry["reason"] = json!(reason);
                }
                json_entries.push(entry);
            } else {
                rows.push(vec![
                    request.target.name().to_string(),
                    request.shell.name().to_string(),
                    request.edit.path_raw.clone(),
                    request.mode.name().to_string(),
                    file_state_display(&state),
                ]);
            }
        }
        if self.json {
            miseprintln!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "mise_shell_activate": json_entries,
                }))?
            );
        } else if rows.is_empty() {
            info!("nothing configured in [bootstrap.mise_shell_activate]");
        } else {
            let mut table = MiseTable::new(false, &["Target", "Shell", "Path", "Mode", "State"]);
            for row in rows {
                table.add_row(row);
            }
            table.print()?;
        }
        if self.missing && any_missing {
            return Err(crate::request_exit(1));
        }
        Ok(())
    }
}

impl BootstrapUser {
    async fn run(self) -> Result<()> {
        match self.command {
            BootstrapUserCommands::Apply(cmd) => cmd.run().await,
            BootstrapUserCommands::Status(cmd) => cmd.run().await,
        }
    }
}

impl BootstrapUserApply {
    async fn run(self) -> Result<()> {
        OperationScope::wrap("bootstrap user apply", self.dry_run, self.run_inner()).await
    }

    async fn run_inner(self) -> Result<()> {
        let config = Config::get().await?;
        install::apply_login_shell(
            system::login_shell_from_config(&config),
            self.dry_run,
            self.yes,
        )
    }
}

impl BootstrapUserStatus {
    async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        let login_shell = system::login_shell_from_config(&config);
        let mut any_missing = false;
        let mut rows: Vec<Vec<String>> = vec![];
        let mut json_out = serde_json::Map::new();
        if let Some(req) = login_shell {
            if !system::login_shell::is_available() {
                let reason = system::login_shell::unavailable_reason();
                if self.json {
                    json_out.insert(
                        "login_shell".to_string(),
                        json!({
                            "available": false,
                            "reason": reason,
                            "shell": req.shell,
                        }),
                    );
                } else {
                    rows.push(vec![
                        req.shell,
                        "".to_string(),
                        format!("skipped ({reason})"),
                    ]);
                }
            } else {
                let status = system::login_shell::status(&req)?;
                let state = match &status.state {
                    LoginShellState::Set => "set",
                    LoginShellState::Differs { .. } => {
                        any_missing = true;
                        "differs"
                    }
                    LoginShellState::MissingFromShells { .. } => {
                        any_missing = true;
                        "missing from /etc/shells"
                    }
                };
                if self.json {
                    json_out.insert(
                        "login_shell".to_string(),
                        json!({
                            "available": true,
                            "shell": status.request.shell,
                            "user": status.user,
                            "current": status.current,
                            "shell_listed": status.shell_listed,
                            "state": state,
                        }),
                    );
                } else {
                    rows.push(vec![
                        status.request.shell,
                        status.current,
                        state.to_string(),
                    ]);
                }
            }
        }
        if self.json {
            miseprintln!("{}", serde_json::to_string_pretty(&json_out)?);
        } else if rows.is_empty() {
            info!("nothing configured in [bootstrap.user]");
        } else {
            let mut table = MiseTable::new(false, &["Shell", "Current", "State"]);
            for row in rows {
                table.add_row(row);
            }
            table.print()?;
        }
        if self.missing && any_missing {
            return Err(crate::request_exit(1));
        }
        Ok(())
    }
}

fn file_state_display(state: &FileState) -> String {
    match state {
        FileState::Applied => "applied".to_string(),
        FileState::Missing => "missing".to_string(),
        FileState::SourceMissing => "source missing".to_string(),
        FileState::Differs(reason) => format!("differs ({reason})"),
        FileState::Tracked => "tracked".to_string(),
    }
}

fn file_state_json(state: &FileState) -> &'static str {
    match state {
        FileState::Applied => "applied",
        FileState::Missing => "missing",
        FileState::SourceMissing => "source_missing",
        FileState::Differs(_) => "differs",
        FileState::Tracked => "tracked",
    }
}

#[cfg(test)]
mod tests {
    use indexmap::{IndexMap, IndexSet};
    use std::ffi::{OsStr, OsString};
    use std::path::Path;

    use super::{bootstrap_from_child_args, select_remote_inventory, unapply_child_args};
    use crate::cli::{Cli, Commands};
    use crate::system::remote;

    #[test]
    fn unavailable_defaults_json_retains_configured_entries() {
        use crate::system::defaults::{DefaultsRequest, DefaultsValue, HostScope};
        let request = DefaultsRequest {
            dock_apps: false,
            domain: "com.mise.test".into(),
            key: "Settings".into(),
            host: HostScope::Current,
            path: Some(vec!["nested".into(), "enabled".into()]),
            value: DefaultsValue::Bool(false),
        };
        let output = super::unavailable_defaults_json(&[request], "macOS only");
        assert_eq!(
            output,
            serde_json::json!({
                "available": false,
                "reason": "macOS only",
                "entries": [{
                    "domain": "com.mise.test",
                    "key": "Settings",
                    "host": "current",
                    "path": ["nested", "enabled"],
                    "value": false,
                    "state": "skipped",
                }],
            })
        );
    }

    #[test]
    fn remote_adopt_parses_and_conflicts_with_source() {
        let flag = "--adopt";
        let argv = [
            "mise",
            "bootstrap",
            "remote",
            "--host",
            "devbox",
            flag,
            "jdx/dotfiles",
            "--github-relay-read-only",
            "--github-relay-repo",
            "jdx/dotfiles",
        ]
        .map(OsStr::new);
        assert!(Cli::parse_from_argv(&argv).is_ok());
        let conflict = [
            "mise",
            "bootstrap",
            "remote",
            "--host",
            "devbox",
            flag,
            "jdx/dotfiles",
            "--source",
            ".",
        ]
        .map(OsStr::new);
        assert!(Cli::parse_from_argv(&conflict).is_err());
        for archive_flag in ["--copy-link=link", "--copy-links", "--exclude=pattern"] {
            let repository_flag = format!("{flag}=jdx/dotfiles");
            let argv = [
                "mise",
                "bootstrap",
                "remote",
                "--host=devbox",
                &repository_flag,
                archive_flag,
            ]
            .map(OsStr::new);
            assert!(Cli::parse_from_argv(&argv).is_err(), "{archive_flag}");
        }
    }

    #[test]
    fn adopt_parses_locally_and_remotely() {
        for remote in [false, true] {
            for equals in [false, true] {
                let mut args = vec!["mise".to_string(), "bootstrap".to_string()];
                if remote {
                    args.extend(["remote", "--host", "devbox"].map(String::from));
                }
                if equals {
                    args.push("--adopt=jdx/dotfiles".to_string());
                } else {
                    args.extend(["--adopt", "jdx/dotfiles"].map(String::from));
                }
                let argv = args.iter().map(OsStr::new).collect::<Vec<_>>();
                let cli = Cli::parse_from_argv(&argv).unwrap();
                let Some(Commands::Bootstrap(parsed)) = cli.command else {
                    panic!("expected bootstrap");
                };
                let adopt = if remote {
                    let Some(super::Commands::Remote(parsed)) = parsed.command else {
                        panic!("expected remote bootstrap");
                    };
                    parsed.adopt
                } else {
                    parsed.adopt
                };
                assert_eq!(adopt.as_deref(), Some("jdx/dotfiles"));
            }
        }
    }

    #[test]
    fn unapply_reexec_replaces_the_selection_and_keeps_other_arguments() {
        let args = [
            "mise",
            "--log-level",
            "debug",
            "-E",
            "gpg",
            "bootstrap",
            "unapply",
            "ssh",
            "--yes",
        ]
        .map(String::from);
        assert_eq!(
            unapply_child_args("gpg,ssh", &args),
            [
                "--env",
                "gpg,ssh",
                "--log-level",
                "debug",
                "bootstrap",
                "unapply",
                "ssh",
                "--yes"
            ]
            .map(OsString::from)
        );
    }

    #[test]
    fn unapply_reexec_drops_a_directory_this_process_already_entered() {
        // `--cd` took effect before the re-invocation, so the child inherits
        // that directory; forwarding a relative path would resolve it again.
        for directory in [
            vec!["--cd", "sub"],
            vec!["--cd=sub"],
            vec!["-C", "sub"],
            vec!["-Csub"],
        ] {
            let mut args = vec!["mise".to_string()];
            args.extend(directory.iter().map(|arg| arg.to_string()));
            args.extend(["bootstrap", "unapply", "ssh"].map(String::from));
            assert_eq!(
                unapply_child_args("ssh", &args),
                ["--env", "ssh", "bootstrap", "unapply", "ssh"].map(OsString::from)
            );
        }
    }

    #[test]
    fn unapply_reexec_removes_every_selection_spelling() {
        for selection in [
            vec!["--env", "work"],
            vec!["--env=work"],
            vec!["-E", "work"],
            vec!["-Ework"],
            vec!["--profile", "work"],
            vec!["--profile=work"],
            vec!["-P", "work"],
            vec!["-Pwork"],
        ] {
            let mut args = vec!["mise".to_string()];
            args.extend(selection.iter().map(|arg| arg.to_string()));
            args.extend(["bootstrap", "unapply", "ssh"].map(String::from));
            assert_eq!(
                unapply_child_args("work,ssh", &args),
                ["--env", "work,ssh", "bootstrap", "unapply", "ssh"].map(OsString::from)
            );
        }
    }

    #[test]
    fn bootstrap_reexec_removes_the_adoption_flag() {
        let flag = "--adopt";
        for equals in [false, true] {
            let mut args = vec!["mise".to_string(), "bootstrap".to_string()];
            if equals {
                args.push(format!("{flag}=jdx/dotfiles"));
            } else {
                args.extend([flag, "jdx/dotfiles"].map(String::from));
            }
            args.push("--replace-history".to_string());
            args.push("--yes".to_string());
            assert_eq!(
                bootstrap_from_child_args(Path::new("/checkout"), &args),
                ["--cd", "/checkout", "bootstrap", "--yes"].map(OsString::from)
            );
        }
    }

    #[test]
    fn part_values_remember_a_legacy_alias() {
        use super::BootstrapPart;
        let argv = [
            "mise",
            "bootstrap",
            "--skip",
            "launchd,macos-defaults",
            "--skip=shell",
        ]
        .map(OsStr::new);
        let cli = Cli::parse_from_argv(&argv).unwrap();
        let Some(Commands::Bootstrap(parsed)) = cli.command else {
            panic!("bootstrap should be the resolved command");
        };
        let skip = parsed
            .skip
            .iter()
            .map(|arg| (arg.part, arg.alias))
            .collect::<Vec<_>>();
        assert_eq!(
            skip,
            [
                (BootstrapPart::Launchd, Some("launchd")),
                (BootstrapPart::Defaults, None),
                (BootstrapPart::Shell, Some("shell")),
            ]
        );
    }

    #[test]
    fn bootstrap_preserves_dry_run_and_yes_flags() {
        let argv: Vec<&OsStr> = ["mise", "bootstrap", "--dry-run", "--yes", "status"]
            .iter()
            .map(OsStr::new)
            .collect();
        let cli = Cli::parse_from_argv(&argv).unwrap();
        let Some(Commands::Bootstrap(parsed)) = cli.command else {
            panic!("bootstrap should be the resolved command");
        };
        assert!(parsed.dry_run);
        assert!(parsed.yes);
    }

    #[test]
    fn bootstrap_from_reexec_preserves_global_flags() {
        let args = [
            "mise",
            "--no-hooks",
            "-E",
            "work",
            "-C",
            "/old",
            "bootstrap",
            "--adopt=git@example.com:dotfiles.git",
            "--from-dir",
            "/old-checkout",
            "--yes",
            "--only",
            "dotfiles",
        ]
        .map(String::from);

        assert_eq!(
            bootstrap_from_child_args(Path::new("/new-checkout"), &args),
            [
                "--cd",
                "/new-checkout",
                "--no-hooks",
                "-E",
                "work",
                "bootstrap",
                "--yes",
                "--only",
                "dotfiles",
            ]
            .map(OsString::from)
        );
    }

    #[test]
    fn bootstrap_from_reexec_preserves_flag_like_global_values() {
        let args = [
            "mise",
            "-E",
            "--cd",
            "bootstrap",
            "--from=git@example.com:dotfiles.git",
            "--yes",
        ]
        .map(String::from);

        assert_eq!(
            bootstrap_from_child_args(Path::new("/new-checkout"), &args),
            ["--cd", "/new-checkout", "-E", "--cd", "bootstrap", "--yes",].map(OsString::from)
        );
    }

    #[test]
    fn bootstrap_adopt_conflicts_with_project_checkout_options() {
        let flag = "--adopt";
        for args in [
            [
                "mise",
                "bootstrap",
                "--from",
                "project.git",
                flag,
                "global.git",
            ]
            .as_slice(),
            [
                "mise",
                "bootstrap",
                flag,
                "global.git",
                "--from-dir",
                "checkout",
            ]
            .as_slice(),
        ] {
            let argv = args.iter().map(OsStr::new).collect::<Vec<_>>();
            assert!(Cli::parse_from_argv(&argv).is_err(), "{args:?}");
        }
    }

    #[test]
    fn explicit_remote_targets_precede_all_and_tag_expansion() {
        let source = std::env::current_dir().unwrap();
        let mut inventory = IndexMap::new();
        for (name, tags) in [
            ("alpha", &[][..]),
            ("beta", &["selected"][..]),
            ("gamma", &["selected"][..]),
        ] {
            let mut host = remote::ad_hoc_host(name, source.clone(), &[]).unwrap();
            host.tags = tags
                .iter()
                .map(|tag| (*tag).to_string())
                .collect::<IndexSet<_>>();
            inventory.insert(name.to_string(), host);
        }

        let selected = select_remote_inventory(
            &inventory,
            &["gamma".to_string(), "alpha".to_string()],
            true,
            &["selected".to_string()],
        )
        .unwrap();

        assert_eq!(
            selected.keys().map(String::as_str).collect::<Vec<_>>(),
            ["gamma", "alpha", "beta"]
        );
    }
}
