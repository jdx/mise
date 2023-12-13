use std::cmp::max;
use std::env::{join_paths, split_paths};
use std::ops::Deref;
use std::path::PathBuf;

use color_eyre::eyre::Result;
use console::truncate_str;
use itertools::Itertools;
use terminal_size::{terminal_size, Width};

use crate::config::Config;

use crate::config::Settings;
use crate::direnv::DirenvDiff;
use crate::env::__RTX_DIFF;
use crate::env_diff::{EnvDiff, EnvDiffOperation};

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

impl HookEnv {
    pub fn run(self, config: Config) -> Result<()> {
        let ts = ToolsetBuilder::new().build(&config)?;
        let shell = get_shell(self.shell).expect("no shell provided, use `--shell=zsh`");
        rtxprint!("{}", hook_env::clear_old_env(&*shell));
        let mut env = ts.env(&config);
        let env_path = env.remove("PATH");
        let mut diff = EnvDiff::new(&env::PRISTINE_ENV, env);
        let mut patches = diff.to_patches();

        let mut paths = config.path_dirs.clone();
        if let Some(p) = env_path {
            paths.extend(split_paths(&p).collect_vec());
        }
        paths.extend(ts.list_paths(&config)); // load the active runtime paths
        diff.path = paths.clone(); // update __RTX_DIFF with the new paths for the next run

        patches.extend(self.build_path_operations(&config.settings, &paths, &__RTX_DIFF.path)?);
        patches.push(self.build_diff_operation(&diff)?);
        patches.push(self.build_watch_operation(&config)?);

        let output = hook_env::build_env_commands(&*shell, &patches);
        rtxprint!("{output}");
        if self.status {
            self.display_status(&config, &ts);
        }

        Ok(())
    }

    fn display_status(&self, config: &Config, ts: &Toolset) {
        let installed_versions = ts
            .list_current_installed_versions(config)
            .into_iter()
            .rev()
            .map(|(_, v)| v.to_string())
            .collect_vec();
        if !installed_versions.is_empty() {
            let w = match terminal_size() {
                Some((Width(w), _)) => w,
                None => 80,
            } as usize;
            let w = max(w, 40);
            let status = installed_versions.into_iter().rev().join(" ");
            rtxstatusln!("{}", truncate_str(&status, w - 4, "..."));
        }
        let env_diff = EnvDiff::new(&env::PRISTINE_ENV, config.env.clone()).to_patches();
        if !env_diff.is_empty() {
            let env_diff = env_diff.into_iter().map(patch_to_status).join(" ");
            rtxstatusln!("{env_diff}");
        }
    }

    /// modifies the PATH and optionally DIRENV_DIFF env var if it exists
    fn build_path_operations(
        &self,
        settings: &Settings,
        installs: &Vec<PathBuf>,
        to_remove: &Vec<PathBuf>,
    ) -> Result<Vec<EnvDiffOperation>> {
        let full = join_paths(&*env::PATH)?.to_string_lossy().to_string();
        let (pre, post) = match &*env::__RTX_ORIG_PATH {
            Some(orig_path) => match full.split_once(&format!(":{orig_path}")) {
                Some((pre, post)) if settings.experimental => {
                    (pre.to_string(), (orig_path.to_string() + post))
                }
                _ => (String::new(), full),
            },
            None => (String::new(), full),
        };
        let install_path = join_paths(installs)?.to_string_lossy().to_string();
        let new_path = vec![pre, install_path, post]
            .into_iter()
            .filter(|p| !p.is_empty())
            .join(":");
        let mut ops = vec![EnvDiffOperation::Add("PATH".into(), new_path)];

        if let Some(input) = env::DIRENV_DIFF.deref() {
            match self.update_direnv_diff(input, installs, to_remove) {
                Ok(Some(op)) => {
                    ops.push(op);
                }
                Err(err) => warn!("failed to update DIRENV_DIFF: {:#}", err),
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
        let watch_files: Vec<_> = config
            .config_files
            .values()
            .flat_map(|p| p.watch_files())
            .collect();
        let watches = hook_env::build_watches(&watch_files)?;
        Ok(EnvDiffOperation::Add(
            "__RTX_WATCH".into(),
            hook_env::serialize_watches(&watches)?,
        ))
    }
}

fn patch_to_status(patch: EnvDiffOperation) -> String {
    match patch {
        EnvDiffOperation::Add(k, _) => format!("+{}", k),
        EnvDiffOperation::Change(k, _) => format!("~{}", k),
        EnvDiffOperation::Remove(k) => format!("-{}", k),
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
