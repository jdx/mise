use crate::config::{Config, Settings};
use crate::direnv::DirenvDiff;
use crate::env::{__MISE_DIFF, PATH_KEY, TERM_WIDTH};
use crate::env::{join_paths, split_paths};
use crate::env_diff::{EnvDiff, EnvDiffOperation, EnvMap, env_value_matches};
use crate::file::{self, canonicalize_cached, display_path, display_rel_path};
use crate::hook_env::{PREV_SESSION, WatchFilePattern};
use crate::shell::{EXAMPLE_SHELL, ShellType, require_shell};
use crate::toolset::{ResolveOptions, Toolset, ToolsetBuilder};
use crate::ui::style;
use crate::{env, hook_env, hooks, watch_files};
use console::truncate_str;
use eyre::Result;
use indexmap::IndexSet;
use itertools::Itertools;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::ops::Deref;
use std::path::PathBuf;
use std::{borrow::Cow, sync::Arc};

pub(crate) use crate::hook_env::HookReason;

/// [internal] Update the environment at each prompt
///
/// The shell hook that `mise activate` installs runs this command.
#[derive(Debug, usage_rs::Args)]
#[usage(hide = true)]
pub(crate) struct HookEnv {
    /// Skip early exit check
    #[usage(long, short)]
    force: bool,

    /// Hide warnings such as when a tool is not installed
    #[usage(long, short)]
    quiet: bool,

    /// Shell type to generate script for
    #[usage(long, short, conflicts = "json", value_enum)]
    shell: Option<ShellType>,

    /// Print the env and alias changes as JSON instead of shell script, for
    /// programs that manage their own environment such as editor plugins
    #[usage(long, short = 'J', conflicts = "shell")]
    json: bool,

    /// Reason for calling hook-env (e.g., "precmd", "chpwd")
    #[usage(long, hide = true, value_enum)]
    reason: Option<HookReason>,

    /// Show "mise: <TOOL>@<VERSION>" message when changing directories
    #[usage(long, hide = true)]
    status: bool,
    #[usage(long, hide = true)]
    shell_pid: Option<u32>,
}

impl HookEnv {
    pub(crate) async fn run(self) -> Result<()> {
        // --json output is shell-agnostic, so no shell is needed for it
        let shell = match self.json {
            true => None,
            false => Some(require_shell(
                self.shell,
                &format!("Name the shell: `mise hook-env --shell {EXAMPLE_SHELL}`."),
            )?),
        };
        let shell = shell.as_deref();
        // The env the --json consumer will apply the changes to, taken before
        // mise does any work that could modify its own process env
        let inherited_env: EnvMap = match self.json {
            true => env::vars_safe().collect(),
            false => EnvMap::new(),
        };
        // Entering a directory is not an explicit action, so an untrusted config is
        // skipped (with a warning) rather than blocking the configs that are trusted.
        crate::config::skip_untrusted_configs();
        let config = Config::get().await?;
        let untrusted_configs = crate::config::skipped_untrusted_configs();
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
        super::tool_update::update_in_background(&config, &ts);

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

        if self.force {
            // Activation forces its first run, which skips should_exit_early, but that
            // run is still the shell's entry into the current directory.
            hook_env::schedule_dir_change_hooks();
        } else if hook_env::should_exit_early(slow_path_watch_files, self.reason) {
            trace!("should_exit_early true");
            if self.json {
                HookEnvJson::print_unchanged()?;
            }
            return Ok(());
        }
        time!("should_exit_early false");
        let old_patches = hook_env::clear_old_env_patches(shell);
        let cleared_keys: BTreeSet<&str> = old_patches
            .iter()
            .filter_map(|patch| match patch {
                EnvDiffOperation::Remove(key) => Some(key.as_str()),
                _ => None,
            })
            .collect();
        // Printed only after the new environment is known to fit, together with
        // it, so a refused update leaves the shell as it was rather than half
        // undone. These undo commands are not size-checked themselves: they put
        // back values the shell held before mise changed them, and a PATH no
        // longer than the current one, so they fit wherever the shell's
        // environment already does. Refusing them over a variable the user set
        // would instead keep mise's old variables in the shell indefinitely.
        let old_output = shell
            .map(|shell| hook_env::build_env_commands(shell, &old_patches))
            .unwrap_or_default();

        // Use env_with_path_and_split which handles caching internally
        let (mut mise_env, env_remove, user_paths, tool_paths, env_watch_files) =
            ts.env_with_path_and_split(&config).await?;
        // The daemon lifecycle follows a shell's pid and emits shell script, so
        // --json leaves daemons alone.
        let daemon_commands = match shell {
            Some(shell) => crate::daemons::hook_env::emit(
                &config,
                &ts,
                &mise_env,
                self.shell_pid,
                shell,
                self.force,
            )
            .await
            .unwrap_or_else(|err| {
                warn!("daemon auto lifecycle: {err:#}");
                String::new()
            }),
            None => String::new(),
        };
        mise_env.remove(&*PATH_KEY);

        // Create config_paths from user_paths for display_status and build_session
        let config_paths: IndexSet<PathBuf> = user_paths.iter().cloned().collect();
        self.display_status(&config, &ts, &mise_env, &config_paths)
            .await?;

        let mut diff = EnvDiff::new(&env::PRISTINE_ENV, mise_env.clone());
        for key in env_remove {
            if let Some(value) = env::PRISTINE_ENV.get(&key) {
                diff.old.insert(key, value.clone());
            }
        }
        let mut diff_patches = diff.to_patches();

        // For fish shell, filter out PATH operations from diff patches because
        // fish's PATH handling conflicts with setting PATH multiple times
        if hook_env::is_fish(shell) {
            hook_env::retain_non_path_patches(&mut diff_patches);
        }
        diff_patches.retain(|patch| match patch {
            EnvDiffOperation::Remove(key) => !cleared_keys.contains(key.as_str()),
            _ => true,
        });
        let mut patches = diff_patches;

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

        let retain_shims = Settings::get().activate_shims
            && (Settings::get().not_found_auto_install || ts.has_lazy_declarations());
        patches.extend(self.build_path_operations(
            &user_paths,
            &tool_paths,
            &__MISE_DIFF.path,
            retain_shims,
        )?);
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
        if untrusted_configs.is_empty() {
            hook_env::clear_untrusted_config_warning(&mut patches);
        }
        let warn_untrusted = !untrusted_configs.is_empty()
            && hook_env::should_show_untrusted_config_warning(&untrusted_configs);
        if warn_untrusted {
            patches.push(hook_env::untrusted_config_warning_seen_patch(
                &untrusted_configs,
            ));
        }

        // A shell snapshots PATH in `mise activate`. A JSON consumer has no
        // activation step, so its first run takes the snapshot, which later runs
        // use to keep directories that were on PATH before mise.
        if shell.is_none() && env::__MISE_ORIG_PATH.is_none() {
            patches.push(EnvDiffOperation::Add(
                "__MISE_ORIG_PATH".into(),
                join_paths(&*env::PATH)?.to_string_lossy().to_string(),
            ));
        }

        hook_env::ensure_env_fits(&patches)?;
        match shell {
            Some(shell) => {
                let output = hook_env::build_env_commands(shell, &patches);
                miseprint!("{old_output}{output}")?;
            }
            None => HookEnvJson {
                env: without_noops(
                    merge_patch(old_patches.iter().chain(&patches)),
                    &inherited_env,
                ),
                aliases: merge_patch(&hook_env::alias_patches(
                    &PREV_SESSION.aliases,
                    &new_aliases,
                )),
                untrusted_configs: untrusted_configs.clone(),
            }
            .print()?,
        }
        if warn_untrusted {
            // Written directly to stderr because the untrusted config's own
            // [settings] (e.g. quiet, log_level) are applied before the trust
            // check and must not be able to silence this notice.
            for config_path in &untrusted_configs {
                safe_eprintln!(
                    "{} {} {} is not trusted, run `mise trust` to enable it",
                    style::eyellow("mise"),
                    style::eyellow("WARN"),
                    display_path(config_path)
                );
            }
        }
        if let Some(shell) = shell {
            miseprint!("{daemon_commands}")?;

            // Build and output alias commands
            let alias_output =
                hook_env::build_alias_commands(shell, &PREV_SESSION.aliases, &new_aliases);
            miseprint!("{alias_output}")?;

            miseprint!(
                "{}",
                crate::packslip::completions::activate(&config, &ts, &shell.to_string())
            )?;
        }

        hooks::run_all_hooks(&config, &ts, shell).await;
        hooks::run_enter_hooks_for_newly_loaded_configs(&config, &ts, shell).await;
        watch_files::execute_runs(&config, &ts).await;
        hooks::take_output_error()?;

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
            let mut env_diff = cur_env
                .iter()
                .filter_map(|(k, v)| match PREV_SESSION.env.get(k) {
                    Some(prev) if env_value_matches(PREV_SESSION.v, prev, v) => None,
                    Some(_) => Some(EnvDiffOperation::Change(k.clone(), String::new())),
                    None => Some(EnvDiffOperation::Add(k.clone(), String::new())),
                })
                .collect_vec();
            env_diff.extend(
                PREV_SESSION
                    .env
                    .keys()
                    .filter(|k| !cur_env.contains_key(*k))
                    .map(|k| EnvDiffOperation::Remove(k.clone())),
            );
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
        crate::deps::notify_if_stale(config, cur_env);
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
        retain_shims: bool,
    ) -> Result<Vec<EnvDiffOperation>> {
        let full = join_paths(&*env::PATH)?.to_string_lossy().to_string();
        let current_paths: Vec<PathBuf> = split_paths(&full).collect();

        let (pre, post, post_user) = match &*env::__MISE_ORIG_PATH {
            Some(orig_path) if !Settings::get().activate_aggressive => {
                let orig_path = crate::windows_posix::orig_path_for_windows(orig_path);
                let orig_paths: Vec<PathBuf> = split_paths(orig_path.as_ref()).collect();
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
                let mise_install_dirs = crate::path_env::mise_install_dirs();
                for path in &current_paths {
                    // Shim farms are a mise-managed boundary. Reinsert them below
                    // the selected tool paths instead of preserving their temporary
                    // activation-prelude position as a user-owned prefix.
                    if file::is_mise_shims_dir(path) {
                        continue;
                    }
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

                    // Paths under mise's installs dir are mise-managed even if
                    // the previous session diff did not claim them. Preserving a
                    // stale install path as a user prefix can shadow the active
                    // version selected by the current toolset.
                    if crate::path_env::is_mise_install_path(path, &mise_install_dirs) {
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
                    if !file::is_mise_shims_dir(path) && !seen_in_current.contains(path) {
                        orig_reordered.push(path.clone());
                    }
                }

                (pre, orig_reordered, post_user)
            }
            _ => (
                vec![],
                current_paths
                    .into_iter()
                    .filter(|path| !file::is_mise_shims_dir(path))
                    .collect(),
                vec![],
            ),
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

        let shim_dirs = if retain_shims {
            crate::shims::shim_farm_dirs()
        } else {
            vec![]
        };

        // Combine paths in the correct order:
        // pre (user shell prepends) -> user_paths (from config) -> tool_paths ->
        // shim fallback boundary -> post (original PATH) -> post_user (user shell appends)
        let new_path = join_paths(
            pre.iter()
                .chain(user_paths_filtered.iter())
                .chain(tool_paths_filtered.iter())
                .chain(shim_dirs.iter())
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

/// What `hook-env --json` prints: the env and alias changes as data, for programs
/// that can't eval shell script.
///
/// `env` and `aliases` are JSON merge patches (RFC 7396): a name with a string is
/// set to it and a name with `null` is removed. Changes are relative to the
/// environment mise was run with. `env` includes mise's own `__MISE_DIFF` and
/// `__MISE_SESSION`, which the consumer must pass back on the next run so mise
/// can undo what this run applied.
#[derive(Debug, Default, serde::Serialize)]
pub(crate) struct HookEnvJson {
    env: MergePatch,
    aliases: MergePatch,
    /// Config files skipped because they are not trusted
    untrusted_configs: Vec<PathBuf>,
}

/// Name -> new value, or `None` to remove it
type MergePatch = BTreeMap<String, Option<String>>;

impl HookEnvJson {
    /// Printed when hook-env exits early, so consumers always get JSON
    pub(crate) fn print_unchanged() -> Result<()> {
        Self::default().print()
    }

    fn print(&self) -> Result<()> {
        miseprintln!("{}", serde_json::to_string_pretty(self)?);
        Ok(())
    }
}

/// Collapses ordered patches into their net effect: the last patch to a key wins.
fn merge_patch<'a>(patches: impl IntoIterator<Item = &'a EnvDiffOperation>) -> MergePatch {
    let mut merged = MergePatch::new();
    for patch in patches {
        match patch {
            EnvDiffOperation::Add(k, v) | EnvDiffOperation::Change(k, v) => {
                merged.insert(k.clone(), Some(v.clone()));
            }
            EnvDiffOperation::Remove(k) => {
                merged.insert(k.clone(), None);
            }
        }
    }
    merged
}

/// Drops entries that would leave `current` as it is. A full run undoes the
/// previous session and reapplies the whole env, so most of its patches set a
/// var to the value it already has.
fn without_noops(mut patch: MergePatch, current: &EnvMap) -> MergePatch {
    patch.retain(|k, v| current.get(k) != v.as_ref());
    patch
}

fn patch_to_status(patch: EnvDiffOperation) -> String {
    match patch {
        EnvDiffOperation::Add(k, _) => format!("+{k}"),
        EnvDiffOperation::Change(k, _) => format!("~{k}"),
        EnvDiffOperation::Remove(k) => format!("-{k}"),
    }
}

fn format_status(status: &str) -> Cow<'_, str> {
    if Settings::get().status.truncate && crate::env::should_truncate() {
        truncate_str(status, TERM_WIDTH.max(60) - 5, "…")
    } else {
        status.into()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    /// The fast path reads `hook-env`'s short flags before the CLI parses them,
    /// so it has to know every short option that takes a value: either it
    /// declines the fast path, or the fast path skips over its value. A new one
    /// fails this test until it is added to the list it belongs in.
    #[test]
    fn fast_path_knows_every_short_option_that_takes_a_value() {
        let spec = super::super::usage::spec();
        let expected: BTreeSet<char> = spec
            .cmd
            .flags
            .iter()
            .filter(|flag| flag.global)
            .chain(&spec.cmd.subcommands["hook-env"].flags)
            .filter(|flag| flag.arg.is_some())
            .flat_map(|flag| flag.short.iter().chain(&flag.hidden_short_aliases))
            .copied()
            .collect();
        let actual: BTreeSet<char> = crate::hook_env::SHORT_OPTIONS_WITH_VALUE
            .iter()
            .copied()
            .chain(
                crate::hook_env::FAST_PATH_DECLINING_OPTIONS
                    .iter()
                    .map(|(_, short)| *short),
            )
            .collect();
        assert_eq!(actual, expected);
    }
}
