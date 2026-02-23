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
    /// Skip early exit check
    #[clap(long, short)]
    force: bool,

    /// Hide warnings such as when a tool is not installed
    #[clap(long, short)]
    quiet: bool,

    /// Shell type to generate script for
    #[clap(long, short)]
    shell: Option<ShellType>,

    /// Reason for calling hook-env (e.g., "precmd", "chpwd")
    #[clap(long, hide = true)]
    reason: Option<HookReason>,

    /// Show "mise: <PLUGIN>@<VERSION>" message when changing directories
    #[clap(long, hide = true)]
    status: bool,
}

impl HookEnv {
    pub async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        let ts = config.get_toolset().await?;
        time!("hook-env");

        // Try to use cached watch_files for early exit check if env_cache is enabled
        // This avoids executing plugins just to get watch_files
        let watch_files = if Settings::get().env_cache {
            if let Ok(Some(cached)) = ts.try_load_env_cache_full(&config) {
                trace!("env_cache: using cached watch_files for early exit check");
                cached
                    .watch_files
                    .iter()
                    .map(|p| WatchFilePattern::from(p.as_path()))
                    .collect()
            } else {
                config.watch_files().await?
            }
        } else {
            config.watch_files().await?
        };

        if !self.force && hook_env::should_exit_early(watch_files.clone(), self.reason) {
            trace!("should_exit_early true");
            return Ok(());
        }
        time!("should_exit_early false");
        let shell = get_shell(self.shell).expect("no shell provided, use `--shell=zsh`");
        miseprint!("{}", hook_env::clear_old_env(&*shell))?;

        // Use env_with_path_and_split which handles caching internally
        let (mut mise_env, user_paths, tool_paths) = ts.env_with_path_and_split(&config).await?;
        mise_env.remove(&*PATH_KEY);

        // Create config_paths from user_paths for display_status and build_session
        let config_paths: IndexSet<PathBuf> = user_paths.iter().cloned().collect();
        self.display_status(&config, ts, &mise_env, &config_paths)
            .await?;

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

        // Combine paths for __MISE_DIFF tracking (all mise-managed paths)
        let all_paths: Vec<PathBuf> = user_paths
            .iter()
            .chain(tool_paths.iter())
            .cloned()
            .collect();
        diff.path.clone_from(&all_paths); // update __MISE_DIFF with the new paths for the next run

        // Get shell aliases from config
        let new_aliases: indexmap::IndexMap<String, String> = config
            .shell_aliases
            .iter()
            .map(|(k, (v, _))| (k.clone(), v.clone()))
            .collect();

        patches.extend(self.build_path_operations(&user_paths, &tool_paths, &__MISE_DIFF.path)?);
        patches.push(self.build_diff_operation(&diff)?);
        patches.push(
            self.build_session_operation(
                &config,
                ts,
                mise_env,
                new_aliases.clone(),
                watch_files,
                &config_paths,
            )
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

        // Build and output alias commands
        let alias_output =
            hook_env::build_alias_commands(&*shell, &PREV_SESSION.aliases, &new_aliases);
        miseprint!("{alias_output}")?;

        hooks::run_all_hooks(&config, ts, &*shell).await;
        watch_files::execute_runs(&config, ts).await;

        Ok(())
    }

    async fn display_status(
        &self,
        config: &Arc<Config>,
        ts: &Toolset,
        cur_env: &EnvMap,
        config_paths: &IndexSet<PathBuf>,
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
            // Use passed config_paths instead of calling config.path_dirs()
            let old_paths = &PREV_SESSION.config_paths;
            let removed_paths = old_paths.difference(config_paths).collect::<IndexSet<_>>();
            let added_paths = config_paths.difference(old_paths).collect::<IndexSet<_>>();
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
        crate::prepare::notify_if_stale(config);
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

        let (pre, post, post_user) = match &*env::__MISE_ORIG_PATH {
            Some(orig_path) if !Settings::get().activate_aggressive => {
                let orig_paths: Vec<PathBuf> = split_paths(orig_path).collect();
                let orig_set: HashSet<_> = orig_paths.iter().collect();

                // Get all mise-managed paths from the previous session
                // to_remove contains ALL paths that mise added (tool installs, config paths, etc.)
                let mise_paths_set: HashSet<_> = to_remove.iter().collect();

                // Find paths in current that are not in original and not mise-managed.
                // Split them into "pre" (before the original PATH entries) and "post_user"
                // (after the original PATH entries) to preserve their intended position.
                // This prevents paths appended after `mise activate` in shell rc from
                // being moved to the front of PATH.
                //
                // Also collect orig paths in their current order to preserve any
                // reordering done after activation (e.g., by ~/.zlogin which runs
                // after ~/.zshrc where mise activate is typically placed).
                let mut pre = Vec::new();
                let mut post_user = Vec::new();
                let mut orig_reordered = Vec::new();
                let mut seen_orig = false;
                let mut seen_in_current: HashSet<&PathBuf> = HashSet::new();
                for path in &current_paths {
                    if orig_set.contains(path) {
                        seen_orig = true;
                        orig_reordered.push(path.clone());
                        seen_in_current.insert(path);
                        continue;
                    }

                    // Skip if it's a mise-managed path from previous session
                    if mise_paths_set.contains(path) {
                        continue;
                    }

                    // Place in pre or post_user based on position relative to original PATH
                    if seen_orig {
                        post_user.push(path.clone());
                    } else {
                        pre.push(path.clone());
                    }
                }

                // Append any orig paths that are no longer in current PATH
                // (to avoid losing paths that may have been temporarily removed)
                for path in &orig_paths {
                    if !seen_in_current.contains(path) {
                        orig_reordered.push(path.clone());
                    }
                }

                (pre, orig_reordered, post_user)
            }
            _ => (vec![], current_paths, vec![]),
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
        let user_additions_set: HashSet<_> = pre.iter().chain(post_user.iter()).collect();
        let user_additions_canonical: HashSet<PathBuf> = pre
            .iter()
            .chain(post_user.iter())
            .filter_map(|p| p.canonicalize().ok())
            .collect();

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
                if let Ok(canonical) = p.canonicalize()
                    && post_canonical.contains(&canonical)
                {
                    return false;
                }

                // Also filter against user additions (pre + post_user) to avoid duplicates
                if user_additions_set.contains(p) {
                    return false;
                }
                if let Ok(canonical) = p.canonicalize()
                    && user_additions_canonical.contains(&canonical)
                {
                    return false;
                }

                true
            })
            .cloned()
            .collect();

        // Filter user_paths against user additions (pre + post_user) to avoid duplicates
        // when users manually add paths after mise activation.
        // IMPORTANT: Do NOT filter against post (__MISE_ORIG_PATH) - this would break
        // the intended behavior where user-configured paths should take precedence
        // even if they already exist in the original PATH.
        let user_paths_filtered: Vec<PathBuf> = user_paths
            .iter()
            .filter(|p| {
                if user_additions_set.contains(p) {
                    return false;
                }
                if let Ok(canonical) = p.canonicalize()
                    && user_additions_canonical.contains(&canonical)
                {
                    return false;
                }
                true
            })
            .cloned()
            .collect();

        // Combine paths in the correct order:
        // pre (user shell prepends) -> user_paths (from config) -> tool_paths -> post (original PATH) -> post_user (user shell appends)
        let new_path = join_paths(
            pre.iter()
                .chain(user_paths_filtered.iter())
                .chain(tool_paths_filtered.iter())
                .chain(post.iter())
                .chain(post_user.iter()),
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
        aliases: indexmap::IndexMap<String, String>,
        watch_files: BTreeSet<WatchFilePattern>,
        config_paths: &IndexSet<PathBuf>,
    ) -> Result<EnvDiffOperation> {
        let loaded_tools = if self.status || Settings::get().status.show_tools {
            ts.list_current_versions()
                .into_iter()
                .map(|(_, tv)| format!("{}@{}", tv.short(), tv.version))
                .collect()
        } else {
            Default::default()
        };
        let session = hook_env::build_session(
            config,
            env,
            aliases,
            loaded_tools,
            watch_files,
            config_paths.clone(),
        )
        .await?;
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
