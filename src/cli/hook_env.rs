use crate::config::{Config, Settings};
use crate::direnv::DirenvDiff;
use crate::env::{join_paths, split_paths, PATH_ENV_SEP};
use crate::env::{PATH_KEY, TERM_WIDTH, __MISE_DIFF};
use crate::env_diff::{EnvDiff, EnvDiffOperation};
use crate::file::display_rel_path;
use crate::hook_env::{WatchFilePattern, CUR_SESSION};
use crate::shell::{get_shell, ShellType};
use crate::toolset::Toolset;
use crate::{env, hook_env, hooks, watch_files};
use console::truncate_str;
use eyre::Result;
use indexmap::IndexSet;
use itertools::Itertools;
use std::ops::Deref;
use std::path::PathBuf;

/// [internal] called by activate hook to update env vars directory change
#[derive(Debug, clap::Args)]
#[clap(hide = true)]
pub struct HookEnv {
    /// Shell type to generate script for
    #[clap(long, short)]
    shell: Option<ShellType>,

    /// Show "mise: <PLUGIN>@<VERSION>" message when changing directories
    #[clap(long, hide = true)]
    status: bool,

    /// Hide warnings such as when a tool is not installed
    #[clap(long, short)]
    quiet: bool,
}

impl HookEnv {
    pub fn run(self) -> Result<()> {
        let config = Config::try_get()?;
        let watch_files = config.watch_files()?;
        time!("hook-env");
        if hook_env::should_exit_early(watch_files.clone()) {
            trace!("should_exit_early true");
            return Ok(());
        }
        time!("should_exit_early false");
        let ts = config.get_toolset()?;
        let shell = get_shell(self.shell).expect("no shell provided, use `--shell=zsh`");
        miseprint!("{}", hook_env::clear_old_env(&*shell))?;
        let mut env = ts.env(&config)?;
        let env_path = env.remove(&*PATH_KEY);
        let mut diff = EnvDiff::new(&env::PRISTINE_ENV, env);
        let mut patches = diff.to_patches();

        let mut paths = config.path_dirs()?.clone();
        if let Some(p) = env_path {
            paths.extend(split_paths(&p).collect_vec());
        }
        paths.extend(ts.list_paths()); // load the active runtime paths
        diff.path.clone_from(&paths); // update __MISE_DIFF with the new paths for the next run

        let settings = Settings::try_get()?;
        patches.extend(self.build_path_operations(&settings, &paths, &__MISE_DIFF.path)?);
        patches.push(self.build_diff_operation(&diff)?);
        patches.push(self.build_session_operation(watch_files.clone())?);

        let output = hook_env::build_env_commands(&*shell, &patches);
        miseprint!("{output}")?;
        self.display_status(&config, ts)?;

        hooks::run_all_hooks(ts, &*shell);
        watch_files::execute_runs(ts);

        Ok(())
    }

    fn display_status(&self, config: &Config, ts: &Toolset) -> Result<()> {
        let settings = Settings::get();
        if self.status || settings.status.show_tools {
            let prev = &CUR_SESSION.loaded_tools;
            let cur = ts
                .list_current_installed_versions()
                .into_iter()
                .rev()
                .map(|(_, tv)| format!("{}@{}", tv.short(), tv.version))
                .collect::<IndexSet<_>>();
            let removed = prev.difference(&cur).collect::<IndexSet<_>>();
            let new = cur.difference(prev).collect::<IndexSet<_>>();
            if !new.is_empty() {
                let status = new.into_iter().map(|t| format!("+{t}")).rev().join(" ");
                info!("{}", truncate_str(&status, TERM_WIDTH.max(60) - 5, "…"));
            }
            if !removed.is_empty() {
                let status = removed.into_iter().map(|t| format!("-{t}")).rev().join(" ");
                info!("{}", truncate_str(&status, TERM_WIDTH.max(60) - 5, "…"));
            }
        }
        if self.status || settings.status.show_env {
            let env_diff = EnvDiff::new(&env::PRISTINE_ENV, config.env()?.clone()).to_patches();
            if !env_diff.is_empty() {
                let env_diff = env_diff.into_iter().map(patch_to_status).join(" ");
                info!("{}", truncate_str(&env_diff, TERM_WIDTH.max(60) - 5, "…"));
            }
            let new_paths: IndexSet<PathBuf> = config
                .path_dirs()
                .map(|p| p.iter().cloned().collect())
                .unwrap_or_default();
            let old_paths = &CUR_SESSION.config_paths;
            let removed_paths = old_paths.difference(&new_paths).collect::<IndexSet<_>>();
            let added_paths = new_paths.difference(old_paths).collect::<IndexSet<_>>();
            if !added_paths.is_empty() {
                let status = added_paths
                    .iter()
                    .map(|p| format!("+{}", display_rel_path(p)))
                    .join(" ");
                info!("{}", truncate_str(&status, TERM_WIDTH.max(60) - 5, "…"));
            }
            if !removed_paths.is_empty() {
                let status = removed_paths
                    .iter()
                    .map(|p| format!("-{}", display_rel_path(p)))
                    .join(" ");
                info!("{}", truncate_str(&status, TERM_WIDTH.max(60) - 5, "…"));
            }
        }
        ts.notify_if_versions_missing();
        Ok(())
    }

    /// modifies the PATH and optionally DIRENV_DIFF env var if it exists
    fn build_path_operations(
        &self,
        settings: &Settings,
        installs: &Vec<PathBuf>,
        to_remove: &Vec<PathBuf>,
    ) -> Result<Vec<EnvDiffOperation>> {
        let full = join_paths(&*env::PATH)?.to_string_lossy().to_string();
        let (pre, post) = match &*env::__MISE_ORIG_PATH {
            Some(orig_path) => match full.split_once(&format!("{PATH_ENV_SEP}{orig_path}")) {
                Some((pre, post)) if !settings.activate_aggressive => (
                    split_paths(pre).collect_vec(),
                    split_paths(&format!("{orig_path}{post}")).collect_vec(),
                ),
                _ => (vec![], split_paths(&full).collect_vec()),
            },
            None => (vec![], split_paths(&full).collect_vec()),
        };

        let new_path = join_paths(pre.iter().chain(installs.iter()).chain(post.iter()))?
            .to_string_lossy()
            .into_owned();
        let mut ops = vec![EnvDiffOperation::Add(PATH_KEY.to_string(), new_path)];

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
            "__MISE_DIFF".into(),
            diff.serialize()?,
        ))
    }

    fn build_session_operation(
        &self,
        watch_files: impl IntoIterator<Item = WatchFilePattern>,
    ) -> Result<EnvDiffOperation> {
        let session = hook_env::build_session(watch_files)?;
        Ok(EnvDiffOperation::Add(
            "__MISE_SESSION".into(),
            hook_env::serialize(&session)?,
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
