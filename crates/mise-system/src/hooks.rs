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
    let shell = mise_util::shells::default_inline_shell(&Settings::get())?;
    let Some((program, shell_args)) = shell.split_first() else {
        bail!("default inline shell args must not be empty");
    };
    for hook in phase_hooks {
        let run = if mise_util::tera::contains_template_syntax(&hook.run) {
            let mut context = config.bootstrap_tera_ctx(&hook.config_path).clone();
            if context.get("config_root").is_none() {
                let config_root = crate::host::config_root(&hook.config_path);
                context.insert("config_root", &config_root);
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
                mise_util::tera::render_str(&mut tera, &hook.run, &context)
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
        info!("$ {run}");
        mise_util::cmd::CmdLineRunner::new(program)
            .cmd_body_args(shell_args, &run)
            .optimize_inline(&run, &[], mise_util::shells::implicit_inline_shell())
            .raw(true)
            .execute_async()
            .await?;
    }
    Ok(())
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
