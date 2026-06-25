//! Bootstrap phase hooks for `[bootstrap.hooks]`.
//!
//! Hooks are imperative commands that run at named points during
//! `mise bootstrap`. They are intentionally explicit bootstrap behavior, not
//! part of `mise install` or shell activation.

use std::fmt;

use eyre::{Result, bail};
use serde::Serialize;
use strum::{EnumIter, IntoEnumIterator};

use crate::config::Settings;

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

    pub fn as_str(self) -> &'static str {
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
}

impl BootstrapHook {
    pub fn from_toml(phase_raw: &str, value: toml::Value) -> Result<Vec<Self>> {
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
                    Some(Self { phase, run })
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
        if dry_run {
            miseprintln!("{} {}", shell.join(" "), shell_words::quote(&hook.run));
            continue;
        }
        info!("$ {}", hook.run);
        crate::cmd::CmdLineRunner::new(program)
            .cmd_body_args(shell_args, &hook.run)
            .raw(true)
            .execute_async()
            .await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let err = BootstrapHook::from_toml("pre-things", toml::Value::String("echo nope".into()))
            .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("pre-things"));
        assert!(msg.contains("pre-packages"));
        assert!(msg.contains("final"));
    }

    #[test]
    fn parses_hook_values() {
        let hooks =
            BootstrapHook::from_toml("pre-packages", toml::Value::String("echo preparing".into()))
                .unwrap();
        assert_eq!(
            hooks,
            vec![BootstrapHook {
                phase: BootstrapHookPhase::PrePackages,
                run: "echo preparing".into(),
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
        let hooks = BootstrapHook::from_toml("final", toml::Value::Table(table)).unwrap();
        assert_eq!(hooks.len(), 2);
        assert_eq!(hooks[1].run, "echo two");
    }
}
