use std::cmp::max;
use std::env::join_paths;
use std::ops::Deref;
use std::path::PathBuf;

use color_eyre::eyre::Result;
use console::truncate_str;
use itertools::Itertools;

use crate::cli::command::Command;
use crate::config::Config;
use crate::config::MissingRuntimeBehavior::{Prompt, Warn};
use crate::direnv::DirenvDiff;
use crate::env::__RTX_DIFF;
use crate::env_diff::{EnvDiff, EnvDiffOperation};
use crate::output::Output;
use crate::shell::{get_shell, ShellType};
use crate::toolset::{Toolset, ToolsetBuilder};
use crate::{env, hook_env};

/// [internal] called by activate hook to update env vars directory change
#[derive(Debug, clap::Args)]
#[clap(hide = true)]
pub struct HookEnv {
    /// Shell type to generate script for
    #[clap(long, short)]
    shell: Option<ShellType>,

    /// Show "rtx: <PLUGIN>@<VERSION>" message when changing directories
    #[clap(long)]
    status: bool,
}

impl Command for HookEnv {
    fn run(self, mut config: Config, out: &mut Output) -> Result<()> {
        if config.settings.missing_runtime_behavior == Prompt {
            config.settings.missing_runtime_behavior = Warn;
        }
        let ts = ToolsetBuilder::new().with_install_missing().build(&config);

        let shell = get_shell(self.shell).expect("no shell provided, use `--shell=zsh`");
        out.stdout.write(hook_env::clear_old_env(&*shell));
        let env = ts.env();
        let mut diff = EnvDiff::new(&env::PRISTINE_ENV, env);
        let mut patches = diff.to_patches();

        let installs = ts.list_paths(); // load the active runtime paths
        diff.path = installs.clone(); // update __RTX_DIFF with the new paths for the next run

        patches.extend(self.build_path_operations(&installs, &__RTX_DIFF.path)?);
        patches.push(self.build_diff_operation(&diff)?);
        patches.push(self.build_watch_operation(&config)?);

        let output = hook_env::build_env_commands(&*shell, &patches);
        out.stdout.write(output);
        if self.status {
            self.display_status(&ts, out);
        }

        Ok(())
    }
}

impl HookEnv {
    fn display_status(&self, ts: &Toolset, out: &mut Output) {
        let installed_versions = ts
            .list_current_installed_versions()
            .into_iter()
            .rev()
            .map(|v| v.to_string())
            .collect_vec();
        if !installed_versions.is_empty() && !*env::RTX_QUIET {
            let (mut w, _) = term_size::dimensions_stderr().unwrap_or((80, 80));
            w = max(w, 40);
            let status = installed_versions.into_iter().rev().join(" ");
            rtxstatusln!(out, "{}", truncate_str(&status, w - 4, "..."));
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
mod tests {
    use crate::assert_cli;

    #[test]
    fn test_hook_env() {
        assert_cli!("hook-env", "--status", "-s", "fish");
    }
}
