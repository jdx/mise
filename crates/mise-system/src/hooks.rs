//! Bootstrap phase hooks for `[bootstrap.hooks]`.
//!
//! Hooks are imperative commands that run at named points during
//! `mise bootstrap`. They are intentionally explicit bootstrap behavior, not
//! part of `mise install` or shell activation.

use std::fmt;
use std::path::PathBuf;

use eyre::{Result, bail};
use serde::Serialize;
use strum::{EnumIter, IntoEnumIterator};

use crate::host::Config;
use mise_settings::Settings;
use mise_util::tera::DryRunRender;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumIter, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum BootstrapHookPhase {
    PrePackages,
    PostPackages,
    PreRepos,
    PostRepos,
    PreDotfiles,
    PostDotfiles,
    PreDefaults,
    PostDefaults,
    PreUser,
    PostUser,
    PreTools,
    PostTools,
    Final,
}

impl BootstrapHookPhase {
    pub fn parse(raw: &str) -> Option<Self> {
        let normalized = raw.replace('_', "-");
        Self::iter().find(|phase| phase.as_str() == normalized)
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::PrePackages => "pre-packages",
            Self::PostPackages => "post-packages",
            Self::PreRepos => "pre-repos",
            Self::PostRepos => "post-repos",
            Self::PreDotfiles => "pre-dotfiles",
            Self::PostDotfiles => "post-dotfiles",
            Self::PreDefaults => "pre-defaults",
            Self::PostDefaults => "post-defaults",
            Self::PreUser => "pre-user",
            Self::PostUser => "post-user",
            Self::PreTools => "pre-tools",
            Self::PostTools => "post-tools",
            Self::Final => "final",
        }
    }
}

impl fmt::Display for BootstrapHookPhase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootstrapHook {
    pub phase: BootstrapHookPhase,
    pub run: String,
    /// What `mise bootstrap` prints instead of the command
    pub description: Option<String>,
    /// A later command of a table with a `description`, which the description already stands for
    pub silent: bool,
    pub config_path: PathBuf,
}

impl BootstrapHook {
    pub(crate) fn from_toml(
        phase_raw: &str,
        value: toml::Value,
        config_path: PathBuf,
    ) -> Result<Vec<Self>> {
        Self::from_toml_for(phase_raw, value, config_path, cfg!(windows))
    }

    /// `windows` picks `run_windows` over `run` when a table sets it.
    fn from_toml_for(
        phase_raw: &str,
        value: toml::Value,
        config_path: PathBuf,
        windows: bool,
    ) -> Result<Vec<Self>> {
        let Some(phase) = BootstrapHookPhase::parse(phase_raw) else {
            let valid = BootstrapHookPhase::iter()
                .map(|phase| phase.as_str())
                .collect::<Vec<_>>();
            bail!(
                "unknown bootstrap hook phase {phase_raw:?}; valid phases are: {}",
                valid.join(", ")
            );
        };
        let mut description = None;
        let runs = match value {
            toml::Value::String(run) => vec![run],
            toml::Value::Array(values) => string_array(values, "expected string commands")?,
            toml::Value::Table(mut table) => {
                let run = table.remove("run");
                let run_windows = table.remove("run_windows");
                description = match table.remove("description") {
                    None => None,
                    Some(toml::Value::String(description)) => {
                        Some(description.trim().to_string()).filter(|d| !d.is_empty())
                    }
                    Some(_) => bail!("expected `description` to be a string"),
                };
                if run.is_none() && run_windows.is_none() {
                    bail!("expected a `run` or `run_windows` command");
                }
                // `run_windows` replaces `run` on Windows; a hook with only
                // `run_windows` does nothing elsewhere.
                let (field, selected) = match (windows, run_windows) {
                    (true, Some(run_windows)) => ("run_windows", Some(run_windows)),
                    _ => ("run", run),
                };
                match selected {
                    Some(toml::Value::String(run)) => vec![run],
                    Some(toml::Value::Array(values)) => string_array(
                        values,
                        &format!("expected `{field}` to contain string commands"),
                    )?,
                    Some(_) => bail!("expected `{field}` to be a string or array of strings"),
                    None => vec![],
                }
            }
            _ => bail!("expected a string, array of strings, or table with `run`"),
        };
        let mut hooks: Vec<Self> = vec![];
        for run in runs {
            let run = run.trim().to_string();
            if run.is_empty() {
                warn!("[bootstrap.hooks.{phase}]: empty command, ignoring entry");
                continue;
            }
            hooks.push(Self {
                phase,
                run,
                // the description names the table, so it is printed once, for its first command
                silent: description.is_some() && !hooks.is_empty(),
                description: description.clone().filter(|_| hooks.is_empty()),
                config_path: config_path.clone(),
            });
        }
        Ok(hooks)
    }
}

fn string_array(values: Vec<toml::Value>, message: &str) -> Result<Vec<String>> {
    let mut out = vec![];
    for value in values {
        match value {
            toml::Value::String(s) => out.push(s),
            _ => bail!("{message}"),
        }
    }
    Ok(out)
}

pub async fn run_phase(
    config: &Config,
    hooks: &[BootstrapHook],
    phase: BootstrapHookPhase,
    dry_run: bool,
) -> Result<()> {
    let phase_hooks: Vec<_> = hooks.iter().filter(|hook| hook.phase == phase).collect();
    if phase_hooks.is_empty() {
        return Ok(());
    }
    info!("bootstrap: {phase} hooks");
    let shell = mise_util::shells::default_inline_shell(&Settings::get())?;
    let Some((program, shell_args)) = shell.split_first() else {
        bail!("default inline shell args must not be empty");
    };
    for hook in phase_hooks {
        let run = if mise_util::tera::contains_template_syntax(&hook.run) {
            let mut context = config.bootstrap_tera_ctx(&hook.config_path).clone();
            if context.get("config_root").is_none() {
                let config_root = crate::host::config_root(&hook.config_path);
                context.insert("config_root", &config_root.to_string_lossy());
            }
            let rendered = if dry_run {
                match mise_util::tera::render_for_dry_run(
                    hook.config_path.parent(),
                    &hook.run,
                    &context,
                    config.bootstrap_dry_run_unresolved_vars(&hook.config_path),
                ) {
                    DryRunRender::Rendered(run) => Ok(run),
                    // the failure may come from exec() output or a var the
                    // dry run did not compute, so only a real run can tell;
                    // show the command as written with the error
                    DryRunRender::Unrendered(err) => {
                        info!(
                            "[bootstrap.hooks.{phase}] in {}: showing the command unrendered because a dry run does not run exec() or compute the vars that need it; render error: {}",
                            hook.config_path.display(),
                            mise_util::tera::error_chain(&err)
                        );
                        Ok(hook.run.clone())
                    }
                    DryRunRender::Failed(err) => Err(err),
                }
            } else {
                let mut tera = mise_util::tera::get_tera(hook.config_path.parent());
                crate::host::render_config_str(&mut tera, &hook.run, &context, &hook.config_path)
            };
            rendered.map_err(|err| {
                eyre::eyre!(
                    "[bootstrap.hooks.{phase}] in {}: failed to render template: {err}",
                    hook.config_path.display()
                )
            })?
        } else {
            hook.run.clone()
        };
        if dry_run {
            miseprintln!("{} {}", shell.join(" "), shell_words::quote(&run));
            continue;
        }
        if !hook.silent {
            info!(
                "{}",
                hook_header(
                    hook.description.as_deref(),
                    &run,
                    Settings::get().verbose,
                    truncate_width(&Settings::get()),
                )
            );
        }
        mise_util::cmd::CmdLineRunner::new(program)
            .cmd_body_args(shell_args, &run)
            .optimize_inline(&run, &[], mise_util::shells::implicit_inline_shell())
            .raw(true)
            .execute_async()
            .await?;
    }
    Ok(())
}

/// The width to cut a command line to, or `None` when output is not truncated.
fn truncate_width(settings: &Settings) -> Option<usize> {
    let truncate = settings.truncate && !*mise_util::env::AI_AGENT && !settings.ci;
    // room for the `mise ` prefix of the log line, and at least 20 characters of the command
    truncate.then(|| mise_util::env::TERM_WIDTH.saturating_sub(6).max(20))
}

/// The line printed before a hook runs: its description, else the command cut to its first line
/// and to `width`, as tasks print theirs. `verbose` prints the whole command.
fn hook_header(
    description: Option<&str>,
    run: &str,
    verbose: bool,
    width: Option<usize>,
) -> String {
    if verbose {
        return format!("$ {run}");
    }
    if let Some(description) = description {
        return description.to_string();
    }
    let first_line = run.lines().next().unwrap_or_default();
    let line = format!("$ {first_line}");
    match width {
        Some(width) => console::truncate_str(&line, width, "…").to_string(),
        None => line,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The schema is generated from `mise_config::bootstrap::BOOTSTRAP_HOOK_PHASES`; the code
    // reads `BootstrapHookPhase`. A phase added to one without the other fails here.
    #[test]
    fn the_declared_hook_phases_are_the_phases_the_code_reads() {
        let read: Vec<&str> = BootstrapHookPhase::iter()
            .map(BootstrapHookPhase::as_str)
            .collect();
        assert_eq!(read, mise_config::bootstrap::BOOTSTRAP_HOOK_PHASES);
    }

    #[test]
    fn parses_known_phases_with_hyphen_or_underscore() {
        assert_eq!(
            BootstrapHookPhase::parse("pre-packages"),
            Some(BootstrapHookPhase::PrePackages)
        );
        assert_eq!(
            BootstrapHookPhase::parse("post_tools"),
            Some(BootstrapHookPhase::PostTools)
        );
        assert_eq!(BootstrapHookPhase::parse("nope"), None);
    }

    #[test]
    fn unknown_phase_error_lists_valid_phases() {
        let err = BootstrapHook::from_toml(
            "pre-things",
            toml::Value::String("echo nope".into()),
            "mise.toml".into(),
        )
        .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("pre-things"));
        assert!(msg.contains("pre-packages"));
        assert!(msg.contains("final"));
    }

    #[test]
    fn parses_hook_values() {
        let hooks = BootstrapHook::from_toml(
            "pre-packages",
            toml::Value::String("echo preparing".into()),
            "mise.toml".into(),
        )
        .unwrap();
        assert_eq!(
            hooks,
            vec![BootstrapHook {
                phase: BootstrapHookPhase::PrePackages,
                run: "echo preparing".into(),
                description: None,
                silent: false,
                config_path: "mise.toml".into(),
            }]
        );

        let mut table = toml::map::Map::new();
        table.insert(
            "run".into(),
            toml::Value::Array(vec![
                toml::Value::String("echo one".into()),
                toml::Value::String("echo two".into()),
            ]),
        );
        let hooks =
            BootstrapHook::from_toml("final", toml::Value::Table(table), "mise.toml".into())
                .unwrap();
        assert_eq!(hooks.len(), 2);
        assert_eq!(hooks[1].run, "echo two");
    }

    #[test]
    fn run_windows_replaces_run_on_windows_only() {
        let table = || {
            let mut table = toml::map::Map::new();
            table.insert("run".into(), toml::Value::String("echo unix".into()));
            table.insert(
                "run_windows".into(),
                toml::Value::String("echo windows".into()),
            );
            toml::Value::Table(table)
        };
        let unix = BootstrapHook::from_toml_for("final", table(), "mise.toml".into(), false);
        assert_eq!(unix.unwrap()[0].run, "echo unix");
        let windows = BootstrapHook::from_toml_for("final", table(), "mise.toml".into(), true);
        assert_eq!(windows.unwrap()[0].run, "echo windows");

        let only_windows = || {
            let mut table = toml::map::Map::new();
            table.insert(
                "run_windows".into(),
                toml::Value::String("echo windows".into()),
            );
            toml::Value::Table(table)
        };
        let unix = BootstrapHook::from_toml_for("final", only_windows(), "mise.toml".into(), false);
        assert!(unix.unwrap().is_empty());
        let windows =
            BootstrapHook::from_toml_for("final", only_windows(), "mise.toml".into(), true);
        assert_eq!(windows.unwrap().len(), 1);
    }

    #[test]
    fn type_errors_name_the_selected_field() {
        let table = || {
            let mut table = toml::map::Map::new();
            table.insert("run".into(), toml::Value::String("echo unix".into()));
            table.insert("run_windows".into(), toml::Value::Integer(1));
            toml::Value::Table(table)
        };
        let err =
            BootstrapHook::from_toml_for("final", table(), "mise.toml".into(), true).unwrap_err();
        assert!(err.to_string().contains("`run_windows`"), "{err}");
        assert!(BootstrapHook::from_toml_for("final", table(), "mise.toml".into(), false).is_ok());
    }

    #[test]
    fn a_description_names_the_first_command_only() {
        let parse = |run: toml::Value| {
            let mut table = toml::map::Map::new();
            table.insert("run".into(), run);
            table.insert("description".into(), " Tidy up ".into());
            BootstrapHook::from_toml("final", toml::Value::Table(table), "mise.toml".into())
                .unwrap()
        };
        let hooks = parse("echo one".into());
        assert_eq!(hooks[0].description.as_deref(), Some("Tidy up"));
        assert!(!hooks[0].silent);

        let hooks = parse(toml::Value::Array(vec![
            "echo one".into(),
            "echo two".into(),
        ]));
        assert_eq!(hooks[0].description.as_deref(), Some("Tidy up"));
        assert!(!hooks[0].silent);
        assert_eq!(hooks[1].description, None);
        assert!(hooks[1].silent);
    }

    #[test]
    fn a_description_must_be_a_string() {
        let mut table = toml::map::Map::new();
        table.insert("run".into(), "echo one".into());
        table.insert("description".into(), toml::Value::Integer(1));
        let err = BootstrapHook::from_toml("final", toml::Value::Table(table), "mise.toml".into())
            .unwrap_err();
        assert!(err.to_string().contains("`description`"), "{err}");
    }

    #[test]
    fn the_header_is_the_description_or_one_truncated_line() {
        let script = "copy=/usr/local/lib/pam/pam_reattach.so\nif [ -e \"$copy\" ]; then\n  rm \"$copy\"\nfi";
        assert_eq!(
            hook_header(Some("Remove the copy"), script, false, Some(80)),
            "Remove the copy"
        );
        assert_eq!(
            hook_header(None, script, false, Some(80)),
            "$ copy=/usr/local/lib/pam/pam_reattach.so"
        );
        assert_eq!(hook_header(None, script, false, Some(12)), "$ copy=/usr…");
        assert_eq!(hook_header(None, "echo hi", false, None), "$ echo hi");
        // verbose prints the whole script, description or not
        assert_eq!(
            hook_header(Some("Remove the copy"), script, true, Some(80)),
            format!("$ {script}")
        );
    }
}
