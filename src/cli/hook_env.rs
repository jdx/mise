use crate::config::{Config, Settings};
use crate::direnv::DirenvDiff;
use crate::env::{__MISE_DIFF, PATH_KEY, TERM_WIDTH};
use crate::env::{join_paths, split_paths};
use crate::env_diff::{EnvDiff, EnvDiffOperation, EnvMap};
use crate::file::{canonicalize_cached, display_path, display_rel_path};
use crate::hook_env::{PREV_SESSION, WatchFilePattern};
use crate::shell::{ShellType, get_shell};
use crate::toolset::{ResolveOptions, Toolset, ToolsetBuilder};
use crate::ui::style;
use crate::{env, hook_env, hooks, watch_files};
use console::truncate_str;
use eyre::Result;
use indexmap::IndexSet;
use itertools::Itertools;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::ops::Deref;
use std::path::{Path, PathBuf};
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

    /// Show "mise: <TOOL>@<VERSION>" message when changing directories
    #[clap(long, hide = true)]
    status: bool,
}

impl HookEnv {
    pub async fn run(self) -> Result<()> {
        let shell = get_shell(self.shell).expect("no shell provided, use `--shell=zsh`");
        let config = match Config::get().await {
            Ok(config) => config,
            Err(err) => {
                let Some(config_path) = hook_env::untrusted_config_error_path(&err) else {
                    return Err(err);
                };
                if hook_env::should_show_untrusted_config_warning(&config_path) {
                    if let Err(mark_err) =
                        hook_env::mark_untrusted_config_warning_seen(&*shell, &config_path)
                    {
                        trace!("failed to mark untrusted config warning seen: {mark_err}");
                    }
                    // Entering a directory is not an explicit action, so show a
                    // single-line warning instead of the full error chain.
                    // Explicit commands still raise the full UntrustedConfig error.
                    // Written directly to stderr because the untrusted config's own
                    // [settings] (e.g. quiet, log_level) are applied before the trust
                    // check and must not be able to silence this notice.
                    safe_eprintln!(
                        "{} {} {} is not trusted, run `mise trust` to enable it",
                        style::eyellow("mise"),
                        style::eyellow("WARN"),
                        display_path(&config_path)
                    );
                }
                return Err(crate::request_exit(1));
            }
        };
        // Shell activation must stay fast and non-networked; missing tools are
        // handled by the normal install paths instead of hook-env.
        let ts = ToolsetBuilder::new()
            .with_resolve_options(ResolveOptions {
                offline: true,
                ..Default::default()
            })
            .build(&config)
            .await?;
        time!("hook-env");

        // Try to use cached watch_files for early exit check if env_cache is enabled
        // This avoids executing plugins just to get watch_files
        let watch_files = if Settings::get().env_cache {
            if let Ok(Some(cached)) = ts.try_load_env_cache_full(&config).await {
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

        // For the slow-path check, include watch_files from the previous session to detect
        // changes to files from tools=true plugins (not yet available via config.watch_files()).
        // We use a separate variable to ensure deleted watch_files don't persist indefinitely.
        let slow_path_watch_files: BTreeSet<WatchFilePattern> = watch_files
            .iter()
            .cloned()
            .chain(PREV_SESSION.watch_files.iter().map(|p| p.as_path().into()))
            .collect();

        if !self.force && hook_env::should_exit_early(slow_path_watch_files, self.reason) {
            trace!("should_exit_early true");
            return Ok(());
        }
        time!("should_exit_early false");
        miseprint!("{}", hook_env::clear_old_env(&*shell))?;

        // Use env_with_path_and_split which handles caching internally
        let (mut mise_env, user_paths, tool_paths, env_watch_files) =
            ts.env_with_path_and_split(&config).await?;
        mise_env.remove(&*PATH_KEY);

        // Create config_paths from user_paths for display_status and build_session
        let config_paths: IndexSet<PathBuf> = user_paths.iter().cloned().collect();
        self.display_status(&config, &ts, &mise_env, &config_paths)
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

        // Include env watch_files in the session for the next prompt's fast-path check.
        // On cache miss, env_watch_files contains only plugin-returned watch_files.
        // On cache hit, it contains the full CachedEnv.watch_files set (config files,
        // env_files, env_scripts, mise.lock files, and plugin watch_files). The BTreeSet
        // deduplicates any overlap with the config-level watch_files above.
        let watch_files: BTreeSet<WatchFilePattern> = watch_files
            .into_iter()
            .chain(env_watch_files.iter().map(|p| p.as_path().into()))
            .collect();

        patches.extend(self.build_path_operations(&user_paths, &tool_paths, &__MISE_DIFF.path)?);
        patches.push(self.build_diff_operation(&diff)?);
        patches.push(
            self.build_session_operation(
                &config,
                &ts,
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
        hook_env::clear_untrusted_config_warning(&mut patches);

        let output = hook_env::build_env_commands(&*shell, &patches);
        miseprint!("{output}")?;

        // Build and output alias commands
        let alias_output =
            hook_env::build_alias_commands(&*shell, &PREV_SESSION.aliases, &new_aliases);
        miseprint!("{alias_output}")?;

        hooks::run_all_hooks(&config, &ts, &*shell).await;
        hooks::run_enter_hooks_for_newly_loaded_configs(&config, &ts, &*shell).await;
        watch_files::execute_runs(&config, &ts).await;

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
                info!("{}", format_status(&env_diff));
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
        crate::deps::notify_if_stale(config);
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
                let mise_paths_set: HashSet<_> = to_remove.iter().map(PathBuf::as_path).collect();
                let mise_install_dirs = crate::path_env::mise_install_dirs();
                partition_path_entries(&current_paths, &orig_paths, |path| {
                    mise_paths_set.contains(path)
                        || crate::path_env::is_mise_install_path(path, &mise_install_dirs)
                })
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
            post.iter().filter_map(|p| canonicalize_cached(p)).collect();
        let user_additions_set: HashSet<_> = pre.iter().chain(post_user.iter()).collect();
        let user_additions_canonical: HashSet<PathBuf> = pre
            .iter()
            .chain(post_user.iter())
            .filter_map(|p| canonicalize_cached(p))
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
                if let Some(canonical) = canonicalize_cached(p)
                    && post_canonical.contains(&canonical)
                {
                    return false;
                }

                // Also filter against user additions (pre + post_user) to avoid duplicates
                if user_additions_set.contains(p) {
                    return false;
                }
                if let Some(canonical) = canonicalize_cached(p)
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
                if let Some(canonical) = canonicalize_cached(p)
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
        // This PATH goes to the user's own shell rather than to a computed child env, so
        // `PathEnv::join`'s check never sees it — and it is the copy a tool started directly
        // from an activated shell inherits.
        crate::path_env::warn_if_cmd_ignores_path_str(&new_path);
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
        let mut diff = DirenvDiff::parse(input)
            .inspect_err(|err| debug!("Failed to parse diff, error: '{:?}'", err))?;
        if diff.new_path().is_empty() {
            return Ok(None);
        }
        for path in to_remove {
            diff.remove_path_from_old_and_new(path).inspect_err(|err| {
                debug!(
                    "Failed to remove path from diff: '{:?}' path: '{}'",
                    err,
                    path.display()
                )
            })?;
        }
        for install in installs {
            diff.add_path_to_old_and_new(install).inspect_err(|err| {
                debug!(
                    "Failed to add path to diff: '{:?}' path: '{}'",
                    err,
                    install.display()
                )
            })?;
        }

        Ok(Some(EnvDiffOperation::Change(
            "DIRENV_DIFF".into(),
            diff.dump()
                .inspect_err(|err| debug!("Failed to dump diff: '{:?}'", err))?,
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

/// Partition PATH entries into user prepends, captured originals, and user appends.
///
/// The rightmost available occurrence of each captured original establishes the
/// start of the original PATH block. Entries before that boundary are user
/// prepends; original-valued entries at or after it remain in the original block;
/// and other entries after it are user appends. This makes the assembled result a
/// fixed point while preserving duplicate entries the user added or retained.
/// Missing captured occurrences are not restored. Mise-managed entries are only
/// retained when they belong to the original block.
fn partition_path_entries(
    current_paths: &[PathBuf],
    orig_paths: &[PathBuf],
    is_mise_managed: impl Fn(&Path) -> bool,
) -> (Vec<PathBuf>, Vec<PathBuf>, Vec<PathBuf>) {
    let mut remaining: HashMap<&Path, usize> = HashMap::new();
    for path in orig_paths {
        *remaining.entry(path.as_path()).or_default() += 1;
    }

    // Prefer later occurrences as the captured originals. A duplicate prepended
    // after activation therefore remains a user prepend, while the later copy
    // anchors the original block. Original entries may be reordered, so this is
    // deliberately occurrence-counted rather than an ordered subsequence match.
    let mut first_original = None;
    for (idx, path) in current_paths.iter().enumerate().rev() {
        if let Some(count) = remaining.get_mut(path.as_path())
            && *count > 0
        {
            *count -= 1;
            first_original = Some(idx);
        }
    }

    let original_values: HashSet<&Path> = orig_paths.iter().map(PathBuf::as_path).collect();
    let mut pre = Vec::new();
    let mut post = Vec::new();
    let mut post_user = Vec::new();
    for (idx, path) in current_paths.iter().enumerate() {
        if first_original.is_some_and(|first| idx >= first)
            && original_values.contains(path.as_path())
        {
            post.push(path.clone());
        } else if is_mise_managed(path) {
            continue;
        } else if first_original.is_some_and(|first| idx > first) {
            post_user.push(path.clone());
        } else {
            pre.push(path.clone());
        }
    }

    (pre, post, post_user)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(values: &[&str]) -> Vec<PathBuf> {
        values.iter().map(PathBuf::from).collect()
    }

    fn partition(
        current: &[&str],
        original: &[&str],
    ) -> (Vec<PathBuf>, Vec<PathBuf>, Vec<PathBuf>) {
        partition_path_entries(&paths(current), &paths(original), |_| false)
    }

    fn assemble_partition(partition: &(Vec<PathBuf>, Vec<PathBuf>, Vec<PathBuf>)) -> Vec<PathBuf> {
        partition
            .0
            .iter()
            .chain(&partition.1)
            .chain(&partition.2)
            .cloned()
            .collect()
    }

    fn sequences(alphabet: &[&str], max_len: usize) -> Vec<Vec<PathBuf>> {
        fn extend(
            sequences: &mut Vec<Vec<PathBuf>>,
            current: &mut Vec<PathBuf>,
            alphabet: &[&str],
            remaining: usize,
        ) {
            sequences.push(current.clone());
            if remaining == 0 {
                return;
            }
            for value in alphabet {
                current.push(PathBuf::from(value));
                extend(sequences, current, alphabet, remaining - 1);
                current.pop();
            }
        }

        let mut sequences = Vec::new();
        extend(&mut sequences, &mut Vec::new(), alphabet, max_len);
        sequences
    }

    #[test]
    fn path_partition_preserves_prepended_original_duplicate() {
        let current = paths(&["B", "original", "A", "managed", "original", "system"]);
        let captured = paths(&["original", "system"]);
        let (pre, original, post_user) =
            partition_path_entries(&current, &captured, |path| path == Path::new("managed"));

        assert_eq!(pre, paths(&["B", "original", "A"]));
        assert_eq!(original, paths(&["original", "system"]));
        assert!(post_user.is_empty());
    }

    #[test]
    fn path_partition_keeps_duplicate_free_order() {
        let (pre, original, post_user) =
            partition(&["B", "A", "original", "system"], &["original", "system"]);

        assert_eq!(pre, paths(&["B", "A"]));
        assert_eq!(original, paths(&["original", "system"]));
        assert!(post_user.is_empty());
    }

    #[test]
    fn path_partition_preserves_original_duplicates() {
        let (pre, original, post_user) = partition(
            &["B", "original", "original", "system"],
            &["original", "original", "system"],
        );

        assert_eq!(pre, paths(&["B"]));
        assert_eq!(original, paths(&["original", "original", "system"]));
        assert!(post_user.is_empty());
    }

    #[test]
    fn path_partition_preserves_appended_original_duplicate() {
        let (pre, original, post_user) = partition(
            &["original", "system", "original", "appended"],
            &["original", "system"],
        );

        assert_eq!(pre, paths(&["original"]));
        assert_eq!(original, paths(&["system", "original"]));
        assert_eq!(post_user, paths(&["appended"]));
    }

    #[test]
    fn path_partition_preserves_reordered_original_entries() {
        let (pre, original, post_user) = partition(
            &["system", "original", "other"],
            &["original", "other", "system"],
        );

        assert!(pre.is_empty());
        assert_eq!(original, paths(&["system", "original", "other"]));
        assert!(post_user.is_empty());
    }

    #[test]
    fn path_partition_does_not_restore_missing_original_entries() {
        let (pre, original, post_user) = partition(&["B", "original"], &["original", "missing"]);

        assert_eq!(pre, paths(&["B"]));
        assert_eq!(original, paths(&["original"]));
        assert!(post_user.is_empty());
    }

    #[test]
    fn path_partition_is_a_fixed_point_and_never_restores_removed_entries() {
        let alphabet = ["A", "B", "C"];
        let current_paths = sequences(&alphabet, 5);
        let original_paths = sequences(&alphabet, 3);

        for managed_mask in 0_u8..(1 << alphabet.len()) {
            for current in &current_paths {
                for original in &original_paths {
                    let is_managed = |path: &Path| {
                        alphabet.iter().enumerate().any(|(idx, value)| {
                            managed_mask & (1 << idx) != 0 && path == Path::new(value)
                        })
                    };
                    let first = partition_path_entries(current, original, is_managed);
                    let assembled = assemble_partition(&first);
                    let second = partition_path_entries(&assembled, original, is_managed);

                    assert_eq!(
                        second, first,
                        "partition was not stable for current={current:?}, original={original:?}, managed_mask={managed_mask:#05b}"
                    );
                    for value in alphabet {
                        let before = current
                            .iter()
                            .filter(|path| *path == Path::new(value))
                            .count();
                        let after = assembled
                            .iter()
                            .filter(|path| *path == Path::new(value))
                            .count();
                        assert!(
                            after <= before,
                            "partition restored {value:?} for current={current:?}, original={original:?}, managed_mask={managed_mask:#05b}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn path_partition_handles_reported_oscillation_case() {
        let current = paths(&["B", "C", "D", "A", "A", "D"]);
        let original = paths(&["A", "B", "C", "D"]);
        let first = partition_path_entries(&current, &original, |_| false);
        let assembled = assemble_partition(&first);
        let second = partition_path_entries(&assembled, &original, |_| false);

        assert_eq!(assembled, current);
        assert_eq!(second, first);
    }

    #[test]
    fn path_partition_ignores_mise_managed_entries() {
        let current = paths(&["B", "managed", "original", "system"]);
        let captured = paths(&["original", "system"]);
        let (pre, original, post_user) =
            partition_path_entries(&current, &captured, |path| path == Path::new("managed"));

        assert_eq!(pre, paths(&["B"]));
        assert_eq!(original, paths(&["original", "system"]));
        assert!(post_user.is_empty());
    }

    #[test]
    fn path_partition_preserves_mise_managed_original_entries() {
        let current = paths(&["managed", "B", "managed", "system"]);
        let captured = paths(&["managed", "system"]);
        let (pre, original, post_user) =
            partition_path_entries(&current, &captured, |path| path == Path::new("managed"));

        assert_eq!(pre, paths(&["B"]));
        assert_eq!(original, paths(&["managed", "system"]));
        assert!(post_user.is_empty());
    }

    #[test]
    fn path_partition_handles_large_sparse_paths() {
        let original = (0..2_000)
            .map(|idx| PathBuf::from(format!("original-{idx}")))
            .collect_vec();
        let mut current = paths(&["prepend-a", "prepend-b"]);
        current.extend(original.iter().cloned());
        current.push(PathBuf::from("append"));

        let (pre, post, post_user) = partition_path_entries(&current, &original, |_| false);

        assert_eq!(pre, paths(&["prepend-a", "prepend-b"]));
        assert_eq!(post, original);
        assert_eq!(post_user, paths(&["append"]));
    }
}
