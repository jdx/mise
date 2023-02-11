use std::cmp::max;
use std::env::join_paths;
use std::ops::Deref;
use std::path::PathBuf;

use color_eyre::eyre::Result;
use itertools::Itertools;

use crate::cli::command::Command;
use crate::config::Config;
use crate::config::MissingRuntimeBehavior::{Prompt, Warn};
use crate::direnv::DirenvDiff;
use crate::env::__RTX_DIFF;
use crate::env_diff::{EnvDiff, EnvDiffOperation, EnvDiffPatches};
use crate::output::Output;
use crate::shell::{get_shell, ShellType};
use crate::{env, hook_env};

/// [internal] called by activate hook to update env vars directory change
#[derive(Debug, clap::Args)]
#[clap(hide = true)]
pub struct HookEnv {
    /// Shell type to generate script for
    #[clap(long, short)]
    shell: Option<ShellType>,
}

impl Command for HookEnv {
    fn run(self, mut config: Config, out: &mut Output) -> Result<()> {
        if config.settings.missing_runtime_behavior == Prompt {
            config.settings.missing_runtime_behavior = Warn;
        }
        config.ensure_installed()?;

        self.clear_old_env(out);
        let env = config.env()?;
        let mut diff = EnvDiff::new(&env::PRISTINE_ENV, env);
        let mut patches = diff.to_patches();

        let installs = config.list_paths()?; // load the active runtime paths
        diff.path = installs.clone(); // update __RTX_DIFF with the new paths for the next run

        patches.extend(self.build_path_operations(&installs, &__RTX_DIFF.path)?);
        patches.push(self.build_diff_operation(&diff)?);
        patches.push(self.build_watch_operation(&config)?);

        let output = self.build_env_commands(&patches);
        out.stdout.write(output);
        self.display_status(&config, out);

        Ok(())
    }
}

impl HookEnv {
    fn build_env_commands(&self, patches: &EnvDiffPatches) -> String {
        let shell = get_shell(self.shell);
        let mut output = String::new();

        for patch in patches.iter() {
            match patch {
                EnvDiffOperation::Add(k, v) | EnvDiffOperation::Change(k, v) => {
                    output.push_str(&shell.set_env(k, v));
                }
                EnvDiffOperation::Remove(k) => {
                    output.push_str(&shell.unset_env(k));
                }
            }
        }

        output
    }

    fn clear_old_env(&self, out: &mut Output) {
        let mut patches = env::__RTX_DIFF.reverse().to_patches();
        if let Some(path) = env::PRISTINE_ENV.deref().get("PATH") {
            patches.push(EnvDiffOperation::Change("PATH".into(), path.to_string()));
        }
        let output = self.build_env_commands(&patches);
        out.stdout.write(output);
    }

    fn display_status(&self, config: &Config, out: &mut Output) {
        let installed_versions = config
            .ts
            .list_current_installed_versions()
            .into_iter()
            .map(|v| v.to_string())
            .collect_vec();
        if !installed_versions.is_empty() && !*env::RTX_QUIET {
            let (w, _) = term_size::dimensions_stderr().unwrap_or_default();
            let w = max(w, 80);
            let status = installed_versions.into_iter().rev().join(" ");
            if status.len() > w - 5 {
                rtxstatusln!(out, "{}...", &status[..w - 9])
            } else {
                rtxstatusln!(out, "{}", status)
            };
        }
    }

    /// modifies the PATH and optionally DIRENV_DIFF env var if it exists
    fn build_path_operations(
        &self,
        installs: &Vec<PathBuf>,
        to_remove: &Vec<PathBuf>,
    ) -> Result<Vec<EnvDiffOperation>> {
        let new_path = join_paths([installs.clone(), env::PATH.clone()].concat())?
            .to_string_lossy()
            .to_string();
        let mut ops = vec![EnvDiffOperation::Add("PATH".into(), new_path)];

        if let Some(input) = env::DIRENV_DIFF.deref() {
            match self.update_direnv_diff(input, installs, to_remove) {
                Ok(Some(op)) => {
                    ops.push(op);
                }
                Err(err) => warn!("failed to update DIRENV_DIFF: {}", err),
                _ => {}
            }
        }

        Ok(ops)
    }

    /// inserts install path to DIRENV_DIFF both for old and new
    /// this makes direnv think that these paths were added before it ran
    /// that way direnv will not remove the path when it runs the next time
    fn update_direnv_diff(
        &self,
        input: &str,
        installs: &Vec<PathBuf>,
        to_remove: &Vec<PathBuf>,
    ) -> Result<Option<EnvDiffOperation>> {
        let mut diff = DirenvDiff::parse(input)?;
        if diff.new_path().is_empty() {
            return Ok(None);
        }
        for path in to_remove {
            diff.remove_path_from_old_and_new(path)?;
        }
        for install in installs {
            diff.add_path_to_old_and_new(install)?;
        }

        Ok(Some(EnvDiffOperation::Change(
            "DIRENV_DIFF".into(),
            diff.dump()?,
        )))
    }

    fn build_diff_operation(&self, diff: &EnvDiff) -> Result<EnvDiffOperation> {
        Ok(EnvDiffOperation::Add(
            "__RTX_DIFF".into(),
            diff.serialize()?,
        ))
    }

    fn build_watch_operation(&self, config: &Config) -> Result<EnvDiffOperation> {
        Ok(EnvDiffOperation::Add(
            "__RTX_WATCH".into(),
            hook_env::serialize_watches(&hook_env::build_watches(config)?)?,
        ))
    }
}

#[cfg(test)]
mod test {
    use crate::assert_cli;

    #[test]
    fn test_hook_env() {
        assert_cli!("hook-env", "-s", "fish");
    }
}
