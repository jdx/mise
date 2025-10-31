use crate::config::{Config, Settings};
use crate::direnv::DirenvDiff;
use crate::env::{__MISE_DIFF, PATH_KEY, TERM_WIDTH};
use crate::env::{join_paths, split_paths};
use crate::env_diff::{EnvDiff, EnvDiffOperation, EnvMap};
use crate::file::display_rel_path;
use crate::hook_env::{PREV_SESSION, WatchFilePattern};
use crate::shell::{ShellType, get_shell};
use crate::toolset::Toolset;
use crate::{env, hook_env, hooks, watch_files};
use console::truncate_str;
use eyre::Result;
use indexmap::IndexSet;
use itertools::Itertools;
use std::collections::{BTreeSet, HashSet};
use std::ops::Deref;
use std::path::PathBuf;
use std::{borrow::Cow, sync::Arc};

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
#[clap(rename_all = "lowercase")]
pub enum HookReason {
    Precmd,
    Chpwd,
}

/// [internal] called by activate hook to update env vars directory change
#[derive(Debug, clap::Args)]
#[clap(hide = true)]
pub struct HookEnv {
    /// Shell type to generate script for
    #[clap(long, short)]
    shell: Option<ShellType>,

    /// Skip early exit check
    #[clap(long, short)]
    force: bool,

    /// Show "mise: <PLUGIN>@<VERSION>" message when changing directories
    #[clap(long, hide = true)]
    status: bool,

    /// Hide warnings such as when a tool is not installed
    #[clap(long, short)]
    quiet: bool,

    /// Reason for calling hook-env (e.g., "precmd", "chpwd")
    #[clap(long, hide = true)]
    reason: Option<HookReason>,
}

impl HookEnv {
    pub async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        let watch_files = config.watch_files().await?;
        time!("hook-env");
        if !self.force && hook_env::should_exit_early(watch_files.clone(), self.reason) {
            trace!("should_exit_early true");
            return Ok(());
        }
        time!("should_exit_early false");
        let ts = config.get_toolset().await?;
        let shell = get_shell(self.shell).expect("no shell provided, use `--shell=zsh`");
        miseprint!("{}", hook_env::clear_old_env(&*shell))?;
        let (mut mise_env, env_results) = ts.final_env(&config).await?;
        mise_env.remove(&*PATH_KEY);
        self.display_status(&config, ts, &mise_env).await?;
        let mut diff = EnvDiff::new(&env::PRISTINE_ENV, mise_env.clone());
        let mut patches = diff.to_patches();

        // For fish shell, filter out PATH operations from diff patches because
        // fish's PATH handling conflicts with setting PATH multiple times
        if shell.to_string() == "fish" {
            patches.retain(|p| match p {
                EnvDiffOperation::Add(k, _)
                | EnvDiffOperation::Change(k, _)
                | EnvDiffOperation::Remove(k) => k != &*PATH_KEY,
            });
        }

        let (user_paths, tool_paths) = ts.list_final_paths_split(&config, env_results).await?;
        // Combine paths for __MISE_DIFF tracking (all mise-managed paths)
        let all_paths: Vec<PathBuf> = user_paths
            .iter()
            .chain(tool_paths.iter())
            .cloned()
            .collect();
        diff.path.clone_from(&all_paths); // update __MISE_DIFF with the new paths for the next run

        patches.extend(self.build_path_operations(&user_paths, &tool_paths, &__MISE_DIFF.path)?);
        patches.push(self.build_diff_operation(&diff)?);
        patches.push(
            self.build_session_operation(&config, ts, mise_env, watch_files)
                .await?,
        );

        // Clear the precmd run flag after running once from precmd
        if self.reason == Some(HookReason::Precmd) && !*env::__MISE_ZSH_PRECMD_RUN {
            patches.push(EnvDiffOperation::Add(
                "__MISE_ZSH_PRECMD_RUN".into(),
                "1".into(),
            ));
        }

        let output = hook_env::build_env_commands(&*shell, &patches);
        miseprint!("{output}")?;

        hooks::run_all_hooks(&config, ts, &*shell).await;
        watch_files::execute_runs(&config, ts).await;

        Ok(())
    }

    async fn display_status(
        &self,
        config: &Arc<Config>,
        ts: &Toolset,
        cur_env: &EnvMap,
    ) -> Result<()> {
        if self.status || Settings::get().status.show_tools {
            let prev = &PREV_SESSION.loaded_tools;
            let cur = ts
                .list_current_installed_versions(config)
                .into_iter()
                .rev()
                .map(|(_, tv)| format!("{}@{}", tv.short(), tv.version))
                .collect::<IndexSet<_>>();
            let removed = prev.difference(&cur).collect::<IndexSet<_>>();
            let new = cur.difference(prev).collect::<IndexSet<_>>();
            if !new.is_empty() {
                let status = new.into_iter().map(|t| format!("+{t}")).rev().join(" ");
                info!("{}", format_status(&status));
            }
            if !removed.is_empty() {
                let status = removed.into_iter().map(|t| format!("-{t}")).rev().join(" ");
                info!("{}", format_status(&status));
            }
        }
        if self.status || Settings::get().status.show_env {
            let mut env_diff = EnvDiff::new(&PREV_SESSION.env, cur_env.clone()).to_patches();
            // TODO: this logic should be in EnvDiff
            let removed_keys = PREV_SESSION
                .env
                .keys()
                .collect::<IndexSet<_>>()
                .difference(&cur_env.keys().collect::<IndexSet<_>>())
                .map(|k| EnvDiffOperation::Remove(k.to_string()))
                .collect_vec();
            env_diff.extend(removed_keys);
            if !env_diff.is_empty() {
                let env_diff = env_diff.into_iter().map(patch_to_status).join(" ");
                info!("{}", truncate_str(&env_diff, TERM_WIDTH.max(60) - 5, "…"));
            }
            let new_paths: IndexSet<PathBuf> = config
                .path_dirs()
                .await
                .map(|p| p.iter().cloned().collect())
                .unwrap_or_default();
            let old_paths = &PREV_SESSION.config_paths;
            let removed_paths = old_paths.difference(&new_paths).collect::<IndexSet<_>>();
            let added_paths = new_paths.difference(old_paths).collect::<IndexSet<_>>();
            if !added_paths.is_empty() {
                let status = added_paths
                    .iter()
                    .map(|p| format!("+{}", display_rel_path(p)))
                    .join(" ");
                info!("{}", format_status(&status));
            }
            if !removed_paths.is_empty() {
                let status = removed_paths
                    .iter()
                    .map(|p| format!("-{}", display_rel_path(p)))
                    .join(" ");
                info!("{}", format_status(&status));
            }
        }
        ts.notify_if_versions_missing(config).await;
        Ok(())
    }

    /// modifies the PATH and optionally DIRENV_DIFF env var if it exists
    /// user_paths are paths from env._.path config that are prepended (filtered only against user manual additions)
    /// tool_paths are paths from tool installations that should be filtered if already in original PATH
    fn build_path_operations(
        &self,
        user_paths: &[PathBuf],
        tool_paths: &[PathBuf],
        to_remove: &[PathBuf],
    ) -> Result<Vec<EnvDiffOperation>> {
        let full = join_paths(&*env::PATH)?.to_string_lossy().to_string();
        let current_paths: Vec<PathBuf> = split_paths(&full).collect();

        let (pre, post) = match &*env::__MISE_ORIG_PATH {
            Some(orig_path) if !Settings::get().activate_aggressive => {
                let orig_paths: Vec<PathBuf> = split_paths(orig_path).collect();
                let orig_set: HashSet<_> = orig_paths.iter().collect();

                // Get all mise-managed paths from the previous session
                // to_remove contains ALL paths that mise added (tool installs, config paths, etc.)
                let mise_paths_set: HashSet<_> = to_remove.iter().collect();

                // Find paths in current that are not in original and not mise-managed
                // These are genuine user additions after mise activation.
                let mut pre = Vec::new();
                for path in &current_paths {
                    // Skip if in original PATH
                    if orig_set.contains(path) {
                        continue;
                    }

                    // Skip if it's a mise-managed path from previous session
                    if mise_paths_set.contains(path) {
                        continue;
                    }

                    // This is a genuine user addition
                    pre.push(path.clone());
                }

                // Use the original PATH directly as "post" to ensure it's preserved exactly
                (pre, orig_paths)
            }
            _ => (vec![], current_paths),
        };

        // Filter out tool paths that are already in the original PATH (post) or
        // in the pre paths (user additions). This prevents mise from claiming ownership
        // of paths that were in the user's original PATH before mise activation, and also
        // prevents duplicates when paths from previous mise activations are in the current
        // PATH. When a tool is deactivated, these paths will remain accessible since they're
        // preserved in the `post` section or `pre` section.
        // This fixes the issue where system tools (e.g., rustup) become unavailable
        // after leaving a mise project that uses the same tool.
        //
        // IMPORTANT: Only filter tool_paths against __MISE_ORIG_PATH (post).
        // User-configured paths are filtered separately (only against user manual additions)
        // to preserve user's intended ordering while avoiding duplicates.
        //
        // Use canonicalized paths for comparison to handle symlinks, relative paths,
        // and other path variants that refer to the same filesystem location.
        let post_canonical: HashSet<PathBuf> =
            post.iter().filter_map(|p| p.canonicalize().ok()).collect();
        let pre_set: HashSet<_> = pre.iter().collect();
        let pre_canonical: HashSet<PathBuf> =
            pre.iter().filter_map(|p| p.canonicalize().ok()).collect();

        let tool_paths_filtered: Vec<PathBuf> = tool_paths
            .iter()
            .filter(|p| {
                // Check both the original path and its canonical form
                // This handles cases where the path doesn't exist yet (can't canonicalize)
                // or where the canonical form differs from the string representation

                // Filter against post (original PATH)
                if post.contains(p) {
                    return false;
                }
                if let Ok(canonical) = p.canonicalize() {
                    if post_canonical.contains(&canonical) {
                        return false;
                    }
                }

                // Also filter against pre (user additions) to avoid duplicates
                if pre_set.contains(p) {
                    return false;
                }
                if let Ok(canonical) = p.canonicalize() {
                    if pre_canonical.contains(&canonical) {
                        return false;
                    }
                }

                true
            })
            .cloned()
            .collect();

        // Filter user_paths against pre (user manual additions) to avoid duplicates
        // when users manually add paths after mise activation.
        // IMPORTANT: Do NOT filter against post (__MISE_ORIG_PATH) - this would break
        // the intended behavior where user-configured paths should take precedence
        // even if they already exist in the original PATH.
        let pre_set: HashSet<_> = pre.iter().collect();
        let pre_canonical: HashSet<PathBuf> =
            pre.iter().filter_map(|p| p.canonicalize().ok()).collect();
        let user_paths_filtered: Vec<PathBuf> = user_paths
            .iter()
            .filter(|p| {
                // Filter against pre only (user manual additions after mise activation)
                if pre_set.contains(p) {
                    return false;
                }
                if let Ok(canonical) = p.canonicalize() {
                    if pre_canonical.contains(&canonical) {
                        return false;
                    }
                }
                true
            })
            .cloned()
            .collect();

        // Combine paths in the correct order:
        // pre (user shell additions) -> user_paths (from config, filtered against pre) -> tool_paths (filtered) -> post (original PATH)
        let new_path = join_paths(
            pre.iter()
                .chain(user_paths_filtered.iter())
                .chain(tool_paths_filtered.iter())
                .chain(post.iter()),
        )?
        .to_string_lossy()
        .into_owned();
        let mut ops = vec![EnvDiffOperation::Add(PATH_KEY.to_string(), new_path)];

        // For DIRENV_DIFF, we need to include both filtered user_paths and filtered tool_paths
        let all_installs: Vec<PathBuf> = user_paths_filtered
            .iter()
            .chain(tool_paths_filtered.iter())
            .cloned()
            .collect();
        if let Some(input) = env::DIRENV_DIFF.deref() {
            match self.update_direnv_diff(input, &all_installs, to_remove) {
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
        installs: &[PathBuf],
        to_remove: &[PathBuf],
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

    async fn build_session_operation(
        &self,
        config: &Arc<Config>,
        ts: &Toolset,
        env: EnvMap,
        watch_files: BTreeSet<WatchFilePattern>,
    ) -> Result<EnvDiffOperation> {
        let loaded_tools = if self.status || Settings::get().status.show_tools {
            ts.list_current_versions()
                .into_iter()
                .map(|(_, tv)| format!("{}@{}", tv.short(), tv.version))
                .collect()
        } else {
            Default::default()
        };
        let session = hook_env::build_session(config, env, loaded_tools, watch_files).await?;
        Ok(EnvDiffOperation::Add(
            "__MISE_SESSION".into(),
            hook_env::serialize(&session)?,
        ))
    }
}

fn patch_to_status(patch: EnvDiffOperation) -> String {
    match patch {
        EnvDiffOperation::Add(k, _) => format!("+{k}"),
        EnvDiffOperation::Change(k, _) => format!("~{k}"),
        EnvDiffOperation::Remove(k) => format!("-{k}"),
    }
}

fn format_status(status: &str) -> Cow<'_, str> {
    if Settings::get().status.truncate {
        truncate_str(status, TERM_WIDTH.max(60) - 5, "…")
    } else {
        status.into()
    }
}
