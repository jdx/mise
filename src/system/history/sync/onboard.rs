//! Setting a machine up from a history-managed setup repository:
//! `mise bootstrap --adopt <url>` on a repository that carries the
//! `.mise-history/format.toml` marker, locally or on a remote host through
//! the bundle `mise bootstrap remote --adopt` transfers. The branch is
//! fetched into mise's own bare store, never cloned into the configuration
//! directory; the shared configuration, the sources it references, and this
//! machine's tracked streams are written by the same recoverable pull as any
//! other incoming change (a file that differs is held for a decision, never
//! overwritten); the connection is declared machine-locally and recorded
//! for the watcher. An ordinary repository (no marker) is left to the
//! ordinary `--adopt`.

use std::process::Stdio;

use eyre::{Result, bail};

use super::SyncMode;
use super::apply::{self, ApplyRequest};
use super::format::{self, RepoState};
use super::network::{Remote, UPSTREAM_REF};
use super::run::{self, SyncRequest};
use crate::file::display_path;
use crate::system::history::checkpoint::Store;
use crate::system::history::config::OriginTomlConfig;
use crate::system::history::store as hstore;
use crate::system::history::tracked::{TrackedSet, global_config_dir};

pub(crate) struct Onboarding {
    /// Where the branch is fetched from: the url, or a transferred bundle.
    pub fetch_from: String,
    /// The repository as this machine keeps reaching it.
    pub origin: String,
    pub branch: String,
    pub yes: bool,
    pub dry_run: bool,
    pub replace_history: bool,
}

pub(crate) struct Outcome {
    /// Incoming configuration staged privately for the ordinary bootstrap
    /// preview. The caller owns its lifetime; live configuration is untouched.
    pub preview_config: Option<tempfile::TempDir>,
    /// The repository can be reached without this session's borrowed
    /// GitHub access.
    pub durable_access: bool,
    /// Any unresolved setup path prevents running bootstrap declarations.
    pub setup_held: bool,
}

#[derive(Debug)]
struct HistoryReplacement {
    local: String,
    remote: String,
}

/// Snapshot only the state needed for planning while its writers are locked.
/// All fetches, reconciliation, and preview status writes then target this
/// private store, so cancellation never needs to roll back live bookkeeping.
fn preview_store(store: &Store) -> Result<(tempfile::TempDir, Store)> {
    let _sync = run::lock(store)?;
    let _capture = store.lock()?;
    let temp = tempfile::tempdir()?;
    hstore::ensure_store_dir_in(temp.path())?;
    let preview = Store::open_in(temp.path())?;
    let source = store
        .repo()
        .ok_or_else(|| eyre::eyre!("preview requires git"))?;
    let destination = preview
        .repo()
        .ok_or_else(|| eyre::eyre!("preview requires git"))?;
    let url = url::Url::from_file_path(source.dir())
        .map_err(|_| eyre::eyre!("cannot address the local history repository"))?;
    let copied = destination.network(["fetch", "--no-tags", url.as_str(), "+refs/*:refs/*"])?;
    if !copied.status.success() {
        bail!("could not snapshot the local history repository for preview");
    }
    run::write_status(temp.path(), &run::read_status(store.state_dir())?)?;
    // Reopen to rebuild the derived history index from the copied commits.
    let preview = Store::open_in(temp.path())?;
    Ok((temp, preview))
}

fn preview_configuration(store: &Store) -> Result<tempfile::TempDir> {
    let temp = tempfile::tempdir()?;
    let repo = store
        .repo()
        .ok_or_else(|| eyre::eyre!("preview requires git"))?;
    let head = repo
        .ref_oid(UPSTREAM_REF)?
        .ok_or_else(|| eyre::eyre!("setup preview has no fetched branch"))?;
    let encrypted = super::files::encrypted_paths(repo, Some(&head))?;
    let tracked = crate::system::history::manifest::Manifest::read(repo, &head)?
        .ok_or_else(|| eyre::eyre!("setup repository has no enrollment metadata"))?
        .tracking()?;
    for entry in repo.ls_tree(&head)? {
        if let Some(relative) = preview_config_path(&tracked, &entry.path)? {
            let Some((mode, oid)) = repo.object_at(&head, &entry.path)? else {
                bail!("configuration disappeared from preview commit");
            };
            let (mode, oid) = if encrypted.contains(&entry.path) {
                super::files::decrypt(repo, &entry.path, &(mode, oid), true)?
            } else {
                (mode, oid)
            };
            crate::system::history::replay::write_path(
                repo,
                &temp.path().join(relative),
                &mode,
                &oid,
            )?;
        }
    }
    // Machine-local inputs are not published, but affect the real bootstrap.
    let local = global_config_dir().join("config.local.toml");
    if local.is_file() {
        std::fs::copy(local, temp.path().join("config.local.toml"))?;
    }
    Ok(temp)
}

/// Strip the portable root, and stage only explicitly enrolled active streams.
fn preview_config_path(
    tracked: &TrackedSet,
    branch_path: &str,
) -> Result<Option<std::path::PathBuf>> {
    let roots = super::layout::Roots::current();
    let located = roots.locate(branch_path);
    let Some(local) = located.path() else {
        return Ok(None);
    };
    let Ok(relative) = local.strip_prefix(&roots.config_dir) else {
        return Ok(None);
    };
    let Some(enrolled) = tracked.entry_for(local) else {
        return Ok(None);
    };
    Ok(
        (enrolled.tree_path(local)? == branch_path && tracked.would_capture(local)?)
            .then(|| relative.to_path_buf()),
    )
}

/// `mise bootstrap --adopt <url>`: `Some` when the repository is
/// history-managed and this machine was set up from it (or would be, on a
/// dry run); `None` leaves the ordinary clone to the caller.
pub(crate) async fn from_git(
    url: &str,
    yes: bool,
    dry_run: bool,
    replace_history: bool,
) -> Result<Option<Outcome>> {
    // Detect marked repositories without creating persistent tracking state
    // for users of the released, ordinary --adopt workflow.
    let probe_dir = tempfile::tempdir()?;
    let store = Store::open_in(probe_dir.path())?;
    if let Some(reason) = store.unavailable() {
        bail!("cannot determine whether {url} is a setup repository: {reason}");
    }
    let repo = store
        .repo()
        .ok_or_else(|| eyre::eyre!("probing a setup repository requires git"))?;
    let Some(branch) = default_branch(&Remote::new(repo, url))? else {
        return Ok(None);
    };
    if !matches!(probe(&store, url, &branch)?, RepoState::Marked(_)) {
        return Ok(None);
    }
    let store = Store::open()?;
    if let Some(reason) = store.unavailable() {
        bail!("cannot onboard this setup repository: history is unavailable: {reason}");
    }
    refuse_other_connection(&store, url, &branch)?;
    let outcome = run(
        &store,
        &Onboarding {
            fetch_from: url.to_string(),
            origin: url.to_string(),
            branch,
            yes,
            dry_run,
            replace_history,
        },
    )
    .await?;
    Ok(Some(outcome))
}

/// The branch a fresh machine takes: the repository's default branch (its
/// `HEAD`), else `main`, else `master`, else the first head; `None` when the
/// repository lists none (unreachable, empty).
pub(super) fn default_branch(remote: &Remote<'_>) -> Result<Option<String>> {
    if let Ok(Some(head)) = remote.symbolic_head() {
        return Ok(Some(head));
    }
    let Ok(refs) = remote.ls_remote() else {
        return Ok(None);
    };
    for candidate in ["refs/heads/main", "refs/heads/master"] {
        if refs.iter().any(|(_, name)| name == candidate) {
            return Ok(Some(
                candidate.trim_start_matches("refs/heads/").to_string(),
            ));
        }
    }
    Ok(refs
        .iter()
        .find_map(|(_, name)| name.strip_prefix("refs/heads/").map(str::to_string)))
}

/// A machine connected to another repository is not silently moved.
fn refuse_other_connection(store: &Store, origin: &str, branch: &str) -> Result<()> {
    let status = run::read_status(store.state_dir())?;
    if let Some(url) = &status.origin_url
        && !status.disconnected
        && (url != origin || status.origin_branch.as_deref() != Some(branch))
    {
        bail!(
            "this machine is connected to {url}; `mise dot origin --remove` disconnects it, or `mise dot origin set {origin}` moves it"
        );
    }
    Ok(())
}

/// Fetches the branch into the store and says what the repository is. A
/// format this mise does not understand is an error; an ordinary
/// repository leaves nothing behind.
fn probe(store: &Store, fetch_from: &str, branch: &str) -> Result<RepoState> {
    let repo = store
        .repo()
        .ok_or_else(|| eyre::eyre!("probing a setup repository requires git"))?;
    Remote::new(repo, fetch_from).fetch_tip(branch)?;
    let upstream = repo.ref_oid(UPSTREAM_REF)?;
    let state = format::detect(repo, upstream.as_deref())?;
    state.check()?;
    if !matches!(state, RepoState::Marked(_)) && upstream.is_some() {
        repo.delete_ref(UPSTREAM_REF)?;
    }
    Ok(state)
}

/// How to clear the paths an adoption left undecided.
///
/// A blanket choice only decides conflicts. Other holds — invalid incoming
/// TOML, a directory where the repository has a file, staged git changes —
/// each need their own fix, so `--take-remote-all` is offered only when every
/// undecided path is a conflict it would actually resolve.
fn undecided_advice(undecided: usize, conflicts: usize) -> String {
    if undecided == 0 {
        return String::new();
    }
    let blanket = if conflicts == undecided {
        ", `mise dot pull --take-remote-all` takes the repository's version of every one"
    } else {
        ""
    };
    format!(
        "; {undecided} path(s) need a decision (`mise dot status` lists them and why, `mise dot pull --take-remote <path>` or `mise dot pull --keep-local <path>` decides one{blanket})"
    )
}

/// Sets this machine up from a repository already found to be
/// history-managed: says what will happen, shows the plan, confirms, records
/// the connection, and pulls (the configuration first, then what it
/// declares, like any pull).
pub(crate) async fn run(store: &Store, onboarding: &Onboarding) -> Result<Outcome> {
    let state_dir = store.state_dir();
    let mode = SyncMode::current()?;
    let config_dir = global_config_dir();
    refuse_other_connection(store, &onboarding.origin, &onboarding.branch)?;
    let repository_format = match store.repo() {
        Some(repo) => match format::detect(repo, repo.ref_oid(UPSTREAM_REF)?.as_deref())? {
            RepoState::Marked(version) => version,
            _ => format::FORMAT,
        },
        None => format::FORMAT,
    };
    miseprintln!(
        "{} is a mise setup repository (format {}); branch {}.",
        onboarding.origin,
        repository_format,
        onboarding.branch
    );
    miseprintln!("Sync mode {}", mode.disclosure());
    miseprintln!(
        "Only explicitly enrolled files are restored to their native paths. Existing files that differ pause the whole setup until you resolve them. Recreating tools, services, and templates requires their configuration and sources to be explicitly tracked too."
    );
    if config_dir.join(".git").exists() {
        miseprintln!(
            "{} is a git checkout: it is not converted or committed to; files written there appear as ordinary working-tree changes.",
            display_path(&config_dir)
        );
    }

    // Preview in an isolated store, never in the live sync state.
    let (_preview_dir, planning_store) = preview_store(store)?;
    let tracked = TrackedSet::effective().await?;
    let preview_replacement = if onboarding.replace_history {
        probe(&planning_store, &onboarding.fetch_from, &onboarding.branch)?;
        detach_local_history(&planning_store)?
    } else {
        None
    };
    if let Some(replacement) = &preview_replacement {
        miseprintln!(
            "Would replace local history {} with origin {}.",
            short_oid(&replacement.local),
            short_oid(&replacement.remote)
        );
    }
    let mut request = SyncRequest::new(true);
    request.origin = Some(OriginTomlConfig::plain(
        onboarding.fetch_from.clone(),
        onboarding.branch.clone(),
    ));
    request.capture = false;
    request.dry_run = true;
    // Pending decisions belong only to this preview store.
    run::sync(&planning_store, &tracked, &request)?;
    let mut preview = ApplyRequest::automatic();
    preview.automatic = false;
    preview.dry_run = true;
    preview.plan_only = true;
    apply::apply(&planning_store, &tracked, &preview).await?;
    if onboarding.dry_run {
        let preview_config = Some(preview_configuration(&planning_store)?);
        miseprintln!("Dry run: nothing was changed.");
        return Ok(Outcome {
            preview_config,
            durable_access: true,
            setup_held: false,
        });
    }
    if !super::origin::confirmed(onboarding.yes, "Set this machine up from the repository?")? {
        bail!("not set up");
    }

    // Recompute after confirmation against current local files and remote
    // refs; never apply a stale preview or restore over a watcher's new state.
    refuse_other_connection(store, &onboarding.origin, &onboarding.branch)?;
    let applied = if onboarding.replace_history {
        let operation =
            crate::system::history::scope::OperationScope::begin_replacement_apply().await?;
        let _sync = run::lock(store)?;
        let previous_status = run::read_status(state_dir)?;
        let repo = store
            .repo()
            .ok_or_else(|| eyre::eyre!("setup requires git"))?;
        let previous_upstream = repo.ref_oid(UPSTREAM_REF)?;
        let previous_sync_state = super::state::load(repo)?;
        seed_confirmed_fetch_locked(store, &planning_store)?;
        let replacement = detach_local_history(store)?;
        if let Some(replacement) = &replacement {
            miseprintln!(
                "Replacing local history {} with origin {}.",
                short_oid(&replacement.local),
                short_oid(&replacement.remote)
            );
        }
        request.capture = false;
        request.dry_run = false;
        let result = async {
            run::sync_locked(store, &tracked, &request)?;
            let applied = apply::apply_locked_with_scope(
                store,
                &tracked,
                &ApplyRequest::automatic(),
                Some(operation),
            )
            .await?;
            if applied.held > 0 {
                bail!(
                    "cannot replace local history while {} path(s) need a decision; move the conflicting files aside and retry",
                    applied.held
                );
            }
            Ok(applied)
        }
        .await;
        match result {
            Ok(applied) => applied,
            Err(error) => {
                restore_after_failed_replacement(
                    store,
                    replacement.as_ref(),
                    previous_upstream.as_deref(),
                    &previous_status,
                    &previous_sync_state,
                )?;
                return Err(error);
            }
        }
    } else {
        seed_confirmed_fetch(store, &planning_store)?;
        // A fresh adoption has no local ancestry yet. Its existing live files
        // are checked by apply's complete preflight; capturing them here would
        // manufacture an unrelated root before that comparison can happen.
        let repo = store
            .repo()
            .ok_or_else(|| eyre::eyre!("setup requires git"))?;
        request.capture = repo
            .ref_oid(crate::system::history::shadow::HistoryRepo::HISTORY_REF)?
            .is_some();
        request.dry_run = false;
        run::sync(store, &tracked, &request)?;
        apply::apply(store, &tracked, &ApplyRequest::automatic()).await?
    };

    // the connection: declared machine-locally, recorded for the watcher
    run::update_status(state_dir, run::STATUS_LOCK_WAIT, |status| {
        status.origin_url = Some(onboarding.origin.clone());
        status.origin_branch = Some(onboarding.branch.clone());
        status.disconnected = false;
    })?;
    super::origin::write_config(&onboarding.origin, &onboarding.branch, None)?;
    crate::system::history::notify::warn_if_release_signing_unavailable();

    // a conflict (a file that exists here and differs) is not pending: it
    // waits for a decision, like a path held with its group
    let conflicts = run::read_status(store.state_dir())?.conflicts.len();
    let undecided = conflicts.max(applied.held);
    let setup_held = {
        let status = run::read_status(state_dir)?;
        undecided > 0
            || !status.pending_applications.is_empty()
            || status.application_failure.is_some()
            || status.validation_error.is_some()
    };
    miseprintln!(
        "Wrote {} file(s) from {}{}.",
        applied.written,
        onboarding.origin,
        undecided_advice(undecided, conflicts)
    );
    let durable_access = durable_access(&onboarding.origin, &onboarding.branch).await;
    if !durable_access {
        warn!(
            "setup complete, but ongoing synchronization needs credentials on this host: the borrowed GitHub access ends with this session. Run `mise x gh -- gh auth login` and `mise x gh -- gh auth setup-git` here, or connect an SSH url with `mise dot origin set <url>`"
        );
    }
    Ok(Outcome {
        preview_config: None,
        durable_access,
        setup_held,
    })
}

/// Reuse the fully fetched preview after consent, without touching local HEAD
/// or importing preview decisions. The following normal remote fetch still
/// discovers newer commits and recomputes against current local state.
fn seed_confirmed_fetch(store: &Store, preview: &Store) -> Result<()> {
    let _sync = run::lock(store)?;
    seed_confirmed_fetch_locked(store, preview)
}

fn seed_confirmed_fetch_locked(store: &Store, preview: &Store) -> Result<()> {
    let source = preview
        .repo()
        .ok_or_else(|| eyre::eyre!("preview requires git"))?;
    let destination = store
        .repo()
        .ok_or_else(|| eyre::eyre!("setup requires git"))?;
    let url = url::Url::from_file_path(source.dir())
        .map_err(|_| eyre::eyre!("cannot address the preview repository"))?;
    let refspec = format!("+{UPSTREAM_REF}:{UPSTREAM_REF}");
    let copied = destination.network(["fetch", "--no-tags", "--", url.as_str(), &refspec])?;
    if !copied.status.success() {
        bail!("could not reuse the fetched setup preview");
    }
    Ok(())
}

fn detach_local_history(store: &Store) -> Result<Option<HistoryReplacement>> {
    use crate::system::history::shadow::HistoryRepo;

    let repo = store
        .repo()
        .ok_or_else(|| eyre::eyre!("setup requires git"))?;
    let Some(local) = repo.ref_oid(HistoryRepo::HISTORY_REF)? else {
        return Ok(None);
    };
    let remote = repo
        .ref_oid(UPSTREAM_REF)?
        .ok_or_else(|| eyre::eyre!("setup repository has no fetched branch"))?;
    repo.delete_history_head(&local)?;
    Ok(Some(HistoryReplacement { local, remote }))
}

fn restore_after_failed_replacement(
    store: &Store,
    replacement: Option<&HistoryReplacement>,
    previous_upstream: Option<&str>,
    previous_status: &run::SyncStatus,
    previous_sync_state: &super::state::SyncState,
) -> Result<()> {
    use crate::system::history::shadow::HistoryRepo;

    let repo = store
        .repo()
        .ok_or_else(|| eyre::eyre!("setup requires git"))?;
    if let Some(replacement) = replacement {
        let current = repo.ref_oid(HistoryRepo::HISTORY_REF)?;
        repo.update_history_head(&replacement.local, current.as_deref())?;
    }
    let current_upstream = repo.ref_oid(UPSTREAM_REF)?;
    match previous_upstream {
        Some(previous) if current_upstream.as_deref() != Some(previous) => {
            repo.update_ref(UPSTREAM_REF, previous, current_upstream.as_deref())?;
        }
        None if current_upstream.is_some() => repo.delete_ref(UPSTREAM_REF)?,
        _ => {}
    }
    run::write_status(store.state_dir(), previous_status)?;
    super::state::save(repo, previous_sync_state, "history replacement rolled back")
}

fn short_oid(oid: &str) -> &str {
    &oid[..oid.len().min(12)]
}

/// Whether this host reaches the repository on its own: with the
/// session's GitHub relay (its `url.<relay>.insteadOf` rewrites travel as
/// `GIT_CONFIG_KEY_n`/`GIT_CONFIG_VALUE_n`) taken out of the environment.
/// Without a relay, whatever works now keeps working.
async fn durable_access(url: &str, branch: &str) -> bool {
    if std::env::var_os("MISE_GITHUB_RELAY_SOCKET").is_none() {
        return true;
    }
    let Some(git) = crate::file::which_spawnable("git") else {
        return true;
    };
    if super::network::validate_url(url).is_err() {
        return false;
    }
    let mut command = tokio::process::Command::new(git);
    command
        .args(["ls-remote", "--exit-code", "--heads", "--", url, branch])
        .env("GIT_TERMINAL_PROMPT", "0")
        .env_remove("MISE_GITHUB_RELAY_SOCKET")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    for (key, _) in std::env::vars_os() {
        let name = key.to_string_lossy();
        if name == "GIT_CONFIG_COUNT"
            || name.starts_with("GIT_CONFIG_KEY_")
            || name.starts_with("GIT_CONFIG_VALUE_")
        {
            command.env_remove(&key);
        }
    }
    matches!(
        tokio::time::timeout(std::time::Duration::from_secs(20), command.status()).await,
        Ok(Ok(status)) if status.success()
    )
}

#[cfg(test)]
mod preview_tests {
    use super::*;
    use crate::system::history::tracked::TrackedEntry;

    #[test]
    fn blanket_resolution_is_offered_only_when_every_undecided_path_is_a_conflict() {
        // Every undecided path is a conflict: the blanket command clears them all.
        let all = undecided_advice(3, 3);
        assert!(all.contains("3 path(s) need a decision"));
        assert!(all.contains("--take-remote-all"));

        // Some paths are held for reasons a resolution cannot fix (invalid
        // incoming TOML, a directory in the way, staged git changes). Offering
        // the blanket command would promise a fix it does not deliver.
        let mixed = undecided_advice(3, 1);
        assert!(mixed.contains("3 path(s) need a decision"));
        assert!(!mixed.contains("--take-remote-all"));
        // the per-path commands still apply, and each is separately runnable
        assert!(mixed.contains("`mise dot pull --take-remote <path>`"));
        assert!(mixed.contains("`mise dot pull --keep-local <path>`"));

        // No conflicts at all, only other holds.
        assert!(!undecided_advice(2, 0).contains("--take-remote-all"));

        // Nothing undecided: no advice to give.
        assert_eq!(undecided_advice(0, 0), "");
    }

    #[test]
    fn confirmed_fetch_reuses_objects_without_replacing_local_head_or_status() -> Result<()> {
        use crate::system::history::shadow::HistoryRepo;
        let temp = tempfile::tempdir()?;
        let local = Store::open_in(&temp.path().join("local"))?;
        let preview = Store::open_in(&temp.path().join("preview"))?;
        let source = preview.repo().unwrap();
        let destination = local.repo().unwrap();
        let tree = source.empty_object("tree")?;
        let first = source.commit_tree(&tree, vec![], "first")?;
        let second = source.commit_tree(&tree, vec![&first], "second")?;
        source.update_ref(UPSTREAM_REF, &second, None)?;
        let local_tree = destination.empty_object("tree")?;
        let head = destination.commit_tree(&local_tree, vec![], "local")?;
        destination.update_ref(HistoryRepo::HISTORY_REF, &head, None)?;
        seed_confirmed_fetch(&local, &preview)?;
        assert_eq!(destination.ref_oid(HistoryRepo::HISTORY_REF)?, Some(head));
        assert_eq!(destination.rev_list(UPSTREAM_REF, 10)?, vec![second, first]);
        assert!(run::read_status(local.state_dir())?.origin_url.is_none());
        assert!(!destination.dir().join("shallow").exists());
        Ok(())
    }

    #[test]
    fn preview_does_not_decrypt_encrypted_files_outside_configuration() -> Result<()> {
        use crate::system::history::manifest::{Enrollment, Manifest};
        let temp = tempfile::tempdir()?;
        let store = Store::open_in(temp.path())?;
        let repo = store.repo().unwrap();
        // Deliberately not decryptable: preview must never read its contents.
        let blob = repo.hash_blob(b"not an encrypted envelope")?;
        let tree = repo.write_tree(&[("100644".into(), blob, "home/preview-secret".into())])?;
        let manifest = Manifest {
            enrollment: vec![Enrollment {
                path: "home/preview-secret".into(),
                autosave: true,
                encrypt: true,
                variants: vec![],
            }],
            ..Default::default()
        };
        let tree = manifest.write(repo, &tree)?;
        let head = repo.commit_tree(&tree, vec![], "preview fixture")?;
        repo.update_ref(UPSTREAM_REF, &head, None)?;
        let preview = preview_configuration(&store)?;
        assert!(!preview.path().join("preview-secret").exists());
        Ok(())
    }

    #[test]
    fn preview_strips_portable_root_and_requires_active_enrollment() {
        let mut tracked = TrackedSet::default();
        let policy =
            crate::system::files::FilePolicy::for_mode(crate::system::files::FileMode::Track);
        tracked.entries.push(TrackedEntry::new(
            global_config_dir().join("config.toml"),
            "track",
            policy,
        ));
        assert_eq!(
            preview_config_path(&tracked, "config/config.toml").unwrap(),
            Some("config.toml".into())
        );
        assert_eq!(
            preview_config_path(&tracked, "config/other.toml").unwrap(),
            None
        );
        assert_eq!(
            preview_config_path(&tracked, "config@other/config.toml").unwrap(),
            None
        );
        tracked.entries[0].variant = Some("selected".into());
        assert_eq!(
            preview_config_path(&tracked, "config/config.toml").unwrap(),
            None
        );
        assert_eq!(
            preview_config_path(&tracked, "config@selected/config.toml").unwrap(),
            Some("config.toml".into())
        );
    }
}
