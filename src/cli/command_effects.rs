//! What each mise command does to the world.
//!
//! mise's usage spec is derived from clap, and clap has no way to express this,
//! so the classification lives here and is applied in [`crate::cli::usage`].
//! Keeping it in one table is deliberate: a safety classification is much easier
//! to review as a single list than as annotations scattered over sixty files.
//!
//! The three values are defined by the usage spec:
//!
//! - `read` — only inspects state; running it twice is the same as running it
//!   once, and not running it changes nothing.
//! - `write` — creates or modifies state, but removes nothing the user cannot
//!   recreate.
//! - `destructive` — removes something the user installed or configured, where
//!   getting it back means redoing work. Deserves a confirmation prompt.
//!
//! **Installing a declared tool version is not an effect.** Almost anything
//! that resolves a toolset — `env`, `activate`, `exec`, `hook-env` — may
//! install a version the config already asks for. That is mise doing its job:
//! it is idempotent, mise recreates it on demand, and it changes nothing the
//! user authored. Counting it would make nearly every command `write` and
//! leave the field with no signal at all. What counts is a change to something
//! the user manages — their config, their files, their installed toolset as a
//! deliberate act. `install` and `use` are `write` because installing is the
//! point, not a precondition.
//!
//! The same reasoning covers the cache: `cache clear` is `write` rather than
//! `destructive` because mise refills it without the user doing anything.
//!
//! **An unlisted command means "unknown", not "safe".** Consumers treat the
//! absence of a value as "ask", so leaving a command out is the conservative
//! choice and mislabeling one `read` is the dangerous one. Commands that run
//! user-supplied code have no fixed effect at all and are listed in
//! [`UNCLASSIFIED`] with the reason.

use std::collections::HashMap;

use usage::SpecCommandEffect::{self, Destructive, Read, Write};

/// Commands whose effect is fixed, keyed by their full path under `mise`.
pub const EFFECTS: &[(&str, SpecCommandEffect)] = &[
    ("activate", Read),
    ("backends", Read),
    ("backends ls", Read),
    ("bin-paths", Read),
    ("bootstrap", Destructive),
    ("bootstrap __apply-account-plan", Destructive),
    ("bootstrap __apply-system-plan", Destructive),
    ("bootstrap __inspect-system-files", Read),
    ("bootstrap accounts", Read),
    ("bootstrap accounts apply", Destructive),
    ("bootstrap accounts status", Read),
    ("bootstrap dotfiles", Read),
    ("bootstrap files", Read),
    ("bootstrap files apply", Destructive),
    ("bootstrap files status", Read),
    // Hidden compatibility spellings of the nested macos/linux subcommands.
    ("bootstrap launchd", Read),
    ("bootstrap launchd apply", Write),
    ("bootstrap launchd status", Read),
    ("bootstrap macos-defaults", Read),
    ("bootstrap macos-defaults apply", Write),
    ("bootstrap macos-defaults status", Read),
    ("bootstrap systemd", Read),
    ("bootstrap systemd apply", Write),
    ("bootstrap systemd status", Read),
    ("bootstrap dotfiles add", Write),
    ("bootstrap dotfiles apply", Write),
    ("bootstrap dotfiles edit", Write),
    ("bootstrap dotfiles status", Read),
    ("bootstrap dotfiles unapply", Destructive),
    ("bootstrap linux", Read),
    ("bootstrap linux systemd-units", Read),
    ("bootstrap linux systemd-units apply", Write),
    ("bootstrap linux systemd-units status", Read),
    ("bootstrap macos", Read),
    ("bootstrap macos defaults", Read),
    ("bootstrap macos defaults apply", Write),
    ("bootstrap macos defaults status", Read),
    ("bootstrap macos launchd-agents", Read),
    ("bootstrap macos launchd-agents apply", Write),
    ("bootstrap macos launchd-agents status", Read),
    ("bootstrap mise-shell-activate", Read),
    ("bootstrap mise-shell-activate apply", Write),
    ("bootstrap mise-shell-activate status", Read),
    ("bootstrap packages", Read),
    ("bootstrap packages apply", Write),
    ("bootstrap packages import", Write),
    // Uninstalls system packages that are no longer declared.
    ("bootstrap packages prune", Destructive),
    ("bootstrap packages status", Read),
    ("bootstrap packages upgrade", Write),
    ("bootstrap packages use", Write),
    ("bootstrap plan", Read),
    ("bootstrap plugins", Read),
    ("bootstrap plugins apply", Write),
    ("bootstrap plugins status", Read),
    ("bootstrap repos", Read),
    ("bootstrap repos apply", Write),
    ("bootstrap repos status", Read),
    ("bootstrap repos update", Write),
    ("bootstrap secrets", Read),
    ("bootstrap secrets status", Read),
    ("bootstrap status", Read),
    // Changes the current user's login shell.
    ("bootstrap user", Read),
    ("bootstrap user apply", Write),
    ("bootstrap user status", Read),
    ("cache", Read),
    // The cache is regenerated automatically, so clearing it costs the user
    // nothing but time — `write` rather than `destructive`.
    ("cache clear", Write),
    ("cache path", Read),
    ("cache prune", Write),
    ("cache task", Read),
    ("completion", Read),
    ("config", Read),
    ("config get", Read),
    ("config ls", Read),
    ("config set", Write),
    ("current", Read),
    ("deactivate", Read),
    // Bare `mise deps` defaults to `deps install` and runs install steps.
    ("deps", Write),
    ("deps add", Write),
    ("deps install", Write),
    ("deps remove", Destructive),
    ("direnv", Read),
    ("direnv activate", Read),
    // Writing the file *is* the command — `File::create` under MISE_TMP_DIR.
    // The temp location does not make it a read: unlike `env`, where an
    // install is incidental to printing, producing a file is the whole job.
    ("direnv envrc", Write),
    ("doctor", Read),
    ("doctor path", Read),
    ("dotfiles", Read),
    ("dotfiles add", Write),
    ("dotfiles apply", Write),
    ("dotfiles edit", Write),
    ("dotfiles status", Read),
    ("dotfiles unapply", Destructive),
    ("edit", Write),
    ("env", Read),
    ("fmt", Write),
    ("generate", Read),
    ("generate bootstrap", Write),
    ("generate config", Write),
    ("generate devcontainer", Write),
    ("generate git-pre-commit", Write),
    ("generate github-action", Write),
    ("generate task-docs", Write),
    ("generate task-stubs", Write),
    ("generate tool-stub", Write),
    // Removes the mise CLI and every tool, plugin and cache it owns.
    ("github", Read),
    ("github token", Read),
    // Writes the global config after setting the version.
    ("global", Write),
    ("hook-env", Read),
    // Unlike `env` or `activate`, this installs a tool that no config asked
    // for: with not_found_auto_install set it resolves an unknown command name
    // against the registry and installs it. That is a new tool on disk from a
    // typo, not a declared version being realized.
    ("hook-not-found", Write),
    // Removes the mise CLI and every tool, plugin and cache it owns.
    ("implode", Destructive),
    ("install", Write),
    ("install-into", Write),
    ("latest", Read),
    ("link", Write),
    ("lock", Write),
    ("local", Write),
    ("ls", Read),
    ("ls-remote", Read),
    ("oci", Read),
    ("oci build", Write),
    ("oci push", Write),
    ("outdated", Read),
    ("patrons", Read),
    ("plugins", Read),
    ("plugins install", Write),
    ("plugins link", Write),
    ("plugins ls", Read),
    ("plugins ls-remote", Read),
    ("plugins uninstall", Destructive),
    ("plugins update", Write),
    ("prune", Destructive),
    ("registry", Read),
    ("reshim", Write),
    ("search", Read),
    ("self-update", Write),
    ("set", Write),
    // Bare `mise settings` lists, but `mise settings foo bar` and
    // `mise settings foo=bar` both route to `settings set` and write config.
    //
    // usage 4 can raise an effect from an argument being *present*, which
    // covers the two-argument form — but not `foo=bar`, where the trigger is
    // the value of `[SETTING]` rather than whether `[VALUE]` was given. Since
    // one of the two writing forms cannot be expressed, the command keeps
    // `write` as its floor: labelling it `read` would report `read` for
    // `mise settings foo=bar`, which is the dangerous direction.
    ("settings", Write),
    ("settings add", Write),
    ("settings get", Read),
    ("settings ls", Read),
    ("settings set", Write),
    ("settings unset", Write),
    // Emits shell code; the session change is ephemeral, and installing the
    // requested version is the same incidental install as `env` or `activate`.
    ("shell", Read),
    ("shell-alias", Read),
    ("shell-alias get", Read),
    ("shell-alias ls", Read),
    ("shell-alias set", Write),
    ("shell-alias unset", Write),
    ("sponsors", Read),
    ("sync", Read),
    ("sync node", Write),
    ("sync python", Write),
    ("sync ruby", Write),
    ("tasks", Read),
    ("tasks add", Write),
    ("tasks deps", Read),
    ("tasks edit", Write),
    ("tasks graph", Read),
    ("tasks info", Read),
    ("tasks ls", Read),
    ("tasks validate", Read),
    ("token", Read),
    ("token forgejo", Read),
    ("token github", Read),
    ("token gitlab", Read),
    ("tool", Read),
    ("tool-alias", Read),
    ("tool-alias get", Read),
    ("tool-alias ls", Read),
    ("tool-alias set", Write),
    ("tool-alias unset", Write),
    ("trust", Write),
    ("uninstall", Destructive),
    ("unset", Write),
    ("untrust", Write),
    ("unuse", Destructive),
    ("upgrade", Write),
    ("usage", Read),
    ("use", Write),
    ("version", Read),
    ("where", Read),
    ("which", Read),
];

/// Commands that only exist in some builds, so they must only be classified
/// where they exist — otherwise the stale-entry test below fails there.
///
/// `bootstrap packages brew` is `#[cfg(unix)]`; `render-help` is
/// `#[cfg(debug_assertions)]`.
pub const PLATFORM_EFFECTS: &[(&str, SpecCommandEffect)] = &[
    #[cfg(unix)]
    ("bootstrap packages brew", Read),
    #[cfg(unix)]
    ("bootstrap packages brew tap", Write),
    #[cfg(unix)]
    ("bootstrap packages brew untap", Write),
    #[cfg(debug_assertions)]
    ("render-help", Write),
];

/// Commands with no fixed effect, and why.
///
/// These run code the user supplies — a task, a tool, a shell — so their effect
/// is whatever that code does. Labeling them would be a lie in whichever
/// direction it was labeled, and `read` in particular would be dangerous.
// Only the coverage test reads this; it exists so the reason a command is
// left unclassified lives next to the decision rather than in a commit message.
#[cfg(test)]
pub const UNCLASSIFIED: &[(&str, &str)] = &[
    ("asdf", "proxies whatever asdf command a plugin invoked"),
    (
        "bootstrap repos exec",
        "runs an arbitrary command in each repo",
    ),
    ("direnv exec", "runs an arbitrary command"),
    ("en", "starts an interactive shell"),
    ("exec", "runs an arbitrary command"),
    ("mcp", "serves tools that run tasks on request"),
    ("oci run", "runs an arbitrary command in a container"),
    ("run", "runs project tasks"),
    ("tasks run", "runs project tasks"),
    ("test-tool", "installs and executes a tool"),
    ("tool-stub", "executes the tool a stub points at"),
    ("watch", "runs project tasks on change"),
];

/// Annotate every command in the spec that has a declared effect.
pub fn apply(spec: &mut usage::Spec) {
    let effects: HashMap<&str, SpecCommandEffect> =
        EFFECTS.iter().chain(PLATFORM_EFFECTS).copied().collect();
    annotate(&mut spec.cmd, &mut vec![], &effects);
}

fn annotate(
    cmd: &mut usage::SpecCommand,
    path: &mut Vec<String>,
    effects: &HashMap<&str, SpecCommandEffect>,
) {
    for (name, sub) in cmd.subcommands.iter_mut() {
        path.push(name.clone());
        if let Some(effect) = effects.get(path.join(" ").as_str()) {
            sub.effect = Some(*effect);
        }
        annotate(sub, path, effects);
        path.pop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;
    use std::collections::HashSet;

    /// Every command in the tree, hidden ones included: a hidden command is
    /// still runnable, and `bootstrap launchd`/`systemd`/`macos-defaults` are
    /// hidden compatibility spellings of commands that change system state.
    fn all_commands() -> Vec<String> {
        let command = crate::cli::expand_deferred_subcommands(
            crate::cli::Cli::command().disable_help_subcommand(true),
        );
        let spec: usage::Spec = command.into();
        let mut out = vec![];
        collect(&spec.cmd, &mut vec![], &mut out);
        out
    }

    fn collect(cmd: &usage::SpecCommand, path: &mut Vec<String>, out: &mut Vec<String>) {
        for (name, sub) in &cmd.subcommands {
            path.push(name.clone());
            out.push(path.join(" "));
            collect(sub, path, out);
            path.pop();
        }
    }

    /// The tables are only worth having if they reach the spec. Everything
    /// else here checks the tables against the CLI; this checks that `apply`
    /// actually transfers them.
    #[test]
    fn apply_annotates_the_spec() {
        let command = crate::cli::expand_deferred_subcommands(
            crate::cli::Cli::command().disable_help_subcommand(true),
        );
        let mut spec: usage::Spec = command.into();
        apply(&mut spec);

        let cmd = |name: &str| {
            spec.cmd
                .subcommands
                .get(name)
                .unwrap_or_else(|| panic!("no `mise {name}`"))
        };
        assert_eq!(cmd("uninstall").effect, Some(Destructive));
        assert_eq!(cmd("ls").effect, Some(Read));
        assert_eq!(cmd("use").effect, Some(Write));
        // Nested commands are reached too.
        assert_eq!(
            cmd("plugins").subcommands["uninstall"].effect,
            Some(Destructive)
        );
        // Anything in UNCLASSIFIED must be left unset, not defaulted.
        assert_eq!(cmd("run").effect, None);
        assert_eq!(cmd("exec").effect, None);
    }

    /// Adding a command without deciding what it does to the world is the
    /// failure mode this whole table exists to prevent, so make it a test
    /// failure rather than a silently missing annotation.
    #[test]
    fn every_visible_command_is_classified() {
        let classified: HashSet<&str> = EFFECTS
            .iter()
            .chain(PLATFORM_EFFECTS)
            .map(|(name, _)| *name)
            .chain(UNCLASSIFIED.iter().map(|(name, _)| *name))
            .collect();

        let missing: Vec<String> = all_commands()
            .into_iter()
            .filter(|cmd| !classified.contains(cmd.as_str()))
            .collect();

        assert!(
            missing.is_empty(),
            "these commands have no entry in EFFECTS or UNCLASSIFIED \
             (src/cli/command_effects.rs) — decide whether each is read, write, \
             destructive, or genuinely unclassifiable:\n  {}",
            missing.join("\n  ")
        );
    }

    /// Catches entries left behind by a renamed or removed command.
    #[test]
    fn no_classification_refers_to_a_missing_command() {
        let present: HashSet<String> = all_commands().into_iter().collect();
        let stale: Vec<&str> = EFFECTS
            .iter()
            .chain(PLATFORM_EFFECTS)
            .map(|(name, _)| *name)
            .chain(UNCLASSIFIED.iter().map(|(name, _)| *name))
            .filter(|name| !present.contains(*name))
            .collect();

        assert!(
            stale.is_empty(),
            "these entries in src/cli/command_effects.rs no longer match a \
             command:\n  {}",
            stale.join("\n  ")
        );
    }

    #[test]
    fn classifications_are_not_duplicated() {
        let mut seen = HashSet::new();
        for (name, _) in EFFECTS
            .iter()
            .chain(PLATFORM_EFFECTS)
            .map(|(n, e)| (*n, Some(*e)))
            .chain(
                UNCLASSIFIED
                    .iter()
                    .map(|(n, _)| (*n, None::<SpecCommandEffect>)),
            )
        {
            assert!(seen.insert(name), "{name} is classified twice");
        }
    }
}
