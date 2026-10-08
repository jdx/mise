//! Bootstrap phase hooks for `[bootstrap.hooks]`.
//!
//! Hooks are imperative commands that run at named points during
//! `mise bootstrap`. They are intentionally explicit bootstrap behavior, not
//! part of `mise install` or shell activation.

use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use eyre::{Result, bail};
use serde::Serialize;
use serde_json::Value as JsonValue;
use strum::{EnumIter, IntoEnumIterator};
use tera::{Kwargs, State, TeraResult, Value};

use crate::config::{Config, Settings, SettingsExt};
use crate::tera::TeraEngine;

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
    pub config_path: PathBuf,
}

impl BootstrapHook {
    pub(crate) fn from_toml(
        phase_raw: &str,
        value: toml::Value,
        config_path: PathBuf,
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
        let runs = match value {
            toml::Value::String(run) => vec![run],
            toml::Value::Array(values) => string_array(values, "expected string commands")?,
            toml::Value::Table(mut table) => match table.remove("run") {
                Some(toml::Value::String(run)) => vec![run],
                Some(toml::Value::Array(values)) => {
                    string_array(values, "expected `run` to contain string commands")?
                }
                Some(_) => bail!("expected `run` to be a string or array of strings"),
                None => bail!("expected a `run` command"),
            },
            _ => bail!("expected a string, array of strings, or table with `run`"),
        };
        let hooks = runs
            .into_iter()
            .filter_map(|run| {
                let run = run.trim().to_string();
                if run.is_empty() {
                    warn!("[bootstrap.hooks.{phase}]: empty command, ignoring entry");
                    None
                } else {
                    Some(Self {
                        phase,
                        run,
                        config_path: config_path.clone(),
                    })
                }
            })
            .collect();
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
    let shell = Settings::get().default_inline_shell()?;
    let Some((program, shell_args)) = shell.split_first() else {
        bail!("default inline shell args must not be empty");
    };
    for hook in phase_hooks {
        let run = if crate::tera::contains_template_syntax(&hook.run) {
            let reached_exec = Arc::new(AtomicBool::new(false));
            let mut tera = if dry_run {
                get_tera_for_dry_run_hook(hook.config_path.parent(), &reached_exec)
            } else {
                crate::tera::get_tera(hook.config_path.parent())
            };
            let mut context = config.bootstrap_tera_ctx(&hook.config_path).clone();
            if context.get("config_root").is_none() {
                let config_root =
                    crate::config::config_file::config_root::config_root(&hook.config_path);
                context.insert("config_root", &config_root);
            }
            match crate::tera::render_str(&mut tera, &hook.run, &context) {
                Ok(run) => run,
                // a dry run must not execute anything, so a command that
                // needs exec() output is shown as written instead
                Err(_) if reached_exec.load(Ordering::Relaxed) => {
                    info!(
                        "[bootstrap.hooks.{phase}] in {}: exec() does not run during a dry run; showing the command unrendered",
                        hook.config_path.display()
                    );
                    hook.run.clone()
                }
                Err(err) => bail!(
                    "[bootstrap.hooks.{phase}] in {}: failed to render template: {err}",
                    hook.config_path.display()
                ),
            }
        } else {
            hook.run.clone()
        };
        if dry_run {
            miseprintln!("{} {}", shell.join(" "), shell_words::quote(&run));
            continue;
        }
        info!("$ {run}");
        crate::cmd::CmdLineRunner::new(program)
            .cmd_body_args(shell_args, &run)
            .optimize_inline(&run, &[], Settings::get().implicit_inline_shell())
            .raw(true)
            .execute_async()
            .await?;
    }
    Ok(())
}

/// The dry-run renderer, with `exec()` recording that the template reached it
/// so the caller can tell that failure apart from a broken template.
fn get_tera_for_dry_run_hook(dir: Option<&Path>, reached_exec: &Arc<AtomicBool>) -> TeraEngine {
    const MESSAGE: &str = "exec() is disabled during dry run";
    let mut tera = crate::tera::get_tera_for_dry_run(dir);
    let reached = reached_exec.clone();
    match &mut tera {
        TeraEngine::V2(tera) => {
            tera.register_function("exec", move |_: Kwargs, _: &State| -> TeraResult<Value> {
                reached.store(true, Ordering::Relaxed);
                Err(tera::Error::message(MESSAGE))
            })
        }
        TeraEngine::V1(tera) => tera.register_function(
            "exec",
            move |_: &HashMap<String, JsonValue>| -> tera1::Result<JsonValue> {
                reached.store(true, Ordering::Relaxed);
                Err(tera1::Error::msg(MESSAGE))
            },
        ),
    }
    tera
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dry_run_renderer_reports_exec_without_running_it() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("ran");
        let context = crate::tera::BASE_CONTEXT.clone();
        let render = |input: &str| {
            let reached_exec = Arc::new(AtomicBool::new(false));
            let mut tera = get_tera_for_dry_run_hook(Some(dir.path()), &reached_exec);
            let rendered = crate::tera::render_str(&mut tera, input, &context);
            (rendered.is_ok(), reached_exec.load(Ordering::Relaxed))
        };

        let exec = format!("echo {{{{ exec(command='touch {}') }}}}", marker.display());
        assert_eq!(render(&exec), (false, true));
        assert!(!marker.exists());
        assert_eq!(render("echo {{ 1 + 1 }}"), (true, false));
        assert_eq!(render("echo {{ nope() }}"), (false, false));
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
}
