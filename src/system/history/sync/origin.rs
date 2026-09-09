//! Connecting and disconnecting the ordinary setup repository. Connecting
//! previews tracked paths and synchronization mode before publication, and
//! validates the repository format without replacing unrelated histories.

use std::collections::BTreeMap;
use std::path::PathBuf;

use eyre::{Result, bail};
use toml_edit::{Item, Value};

use super::SyncMode;
use super::format::{self, RepoState};
use super::layout::{Roots, is_configuration};
use super::network::{Remote, UPSTREAM_REF};
use super::run::{self, SyncRequest};
use super::share;
use crate::file::display_path;
use crate::system::history::checkpoint::Store;
use crate::system::history::tracked::TrackedSet;
use crate::ui::prompt;

pub(crate) struct SetOptions {
    pub url: String,
    /// The branch asked for; `None` takes the repository's own default branch.
    pub branch: Option<String>,
    pub mode: SyncMode,
    pub yes: bool,
}

/// Only for a repository that lists no branches at all.
const DEFAULT_BRANCH: &str = "main";

/// Connects the setup repository.
pub(crate) async fn set(store: &Store, tracked: &TrackedSet, opts: &SetOptions) -> Result<()> {
    super::network::validate_url(&opts.url)?;
    let mut preview_lock = Some(run::lock(store)?);
    let repo = store
        .repo()
        .ok_or_else(|| eyre::eyre!("connecting requires git"))?;
    let previous_upstream = repo.ref_oid(UPSTREAM_REF)?;
    let mut accepted = false;
    let result = set_inner(store, tracked, opts, &mut accepted, &mut preview_lock).await;
    if !accepted {
        // Preview fetches must not change the active connection, even when
        // validation or privacy checks fail before the prompt.
        let mut cleanup_errors = Vec::new();
        let restored = match previous_upstream {
            Some(oid) => repo
                .ref_oid(UPSTREAM_REF)
                .and_then(|current| repo.update_ref(UPSTREAM_REF, &oid, current.as_deref())),
            None => repo.delete_ref(UPSTREAM_REF),
        };
        if let Err(err) = restored {
            cleanup_errors.push(format!("{UPSTREAM_REF}: {err:#}"));
        }
        if !cleanup_errors.is_empty() {
            let message = format!(
                "could not completely restore setup preview refs: {}; retry connecting before syncing",
                cleanup_errors.join("; ")
            );
            return Err(match result {
                Err(err) => err.wrap_err(message),
                Ok(()) => eyre::eyre!(message),
            });
        }
    }
    result
}

async fn set_inner(
    store: &Store,
    tracked: &TrackedSet,
    opts: &SetOptions,
    accepted: &mut bool,
    preview_lock: &mut Option<fslock::LockFile>,
) -> Result<()> {
    if opts.url.trim().is_empty() {
        bail!("a repository url is required");
    }
    let repo = store
        .repo()
        .ok_or_else(|| eyre::eyre!("connecting a setup repository requires git"))?;
    let state_dir = store.state_dir();
    // the connection as it was: another repository or branch starts from a
    // clean slate, and a scheme change on the same one replaces its refs
    let status_before = run::read_status(state_dir)?;
    // This is the disposable preview repository. The connected repository's
    // remote-tracking ref remains untouched unless the connection is confirmed.
    let remote = Remote::new(repo, &opts.url);
    // re-running the same connection keeps the branch it already has, so an
    // unattended `origin set <url> --yes` stays idempotent when the
    // repository's default branch is not the one this machine follows
    let connected = (!status_before.disconnected
        && status_before.origin_url.as_deref() == Some(opts.url.as_str()))
    .then_some(status_before.origin_branch.as_deref())
    .flatten();
    let branch = resolve_branch(&remote, opts.branch.as_deref(), connected)?;
    if !remote.fetch(&branch)? {
        refuse_missing_branch(&remote, &branch, &opts.url)?;
        if repo.ref_oid(UPSTREAM_REF)?.is_some() {
            repo.delete_ref(UPSTREAM_REF)?;
        }
    }
    let upstream = repo.ref_oid(UPSTREAM_REF)?;
    let repo_state = format::detect(repo, upstream.as_deref())?;
    repo_state.check()?;

    run::capture_now(store, tracked);
    let shared = share::current(repo, tracked)?;
    let connected_before = status_before.origin_url.is_some();
    let same_origin = status_before.origin_url.as_deref() == Some(opts.url.as_str())
        && status_before.origin_branch.as_deref() == Some(branch.as_str());
    miseprintln!("Every committed tracked-file version is eligible for origin synchronization.");
    // disclosure
    miseprintln!("Setup repository: {} (branch {})", opts.url, branch);
    miseprintln!("Sync mode {}", opts.mode.disclosure());
    crate::system::history::notify::warn_if_release_signing_unavailable();
    match &repo_state {
        RepoState::Empty => miseprintln!(
            "The repository is empty: the first publication creates `{branch}` with the mise marker."
        ),
        RepoState::Marked(_) => {
            miseprintln!("The repository is a mise setup repository; continuing.")
        }
        RepoState::Unmarked => {
            miseprintln!(
                "The repository already has content without mise enrollment metadata. Connecting does not import its files or replace unrelated history. Synchronization requires compatible Git ancestry; use an empty origin or reconcile the histories explicitly. Ordinary non-tracking `--from`/`--adopt` workflows remain available."
            );
        }
    }
    let mut streams: BTreeMap<String, usize> = BTreeMap::new();
    for branch_path in shared.files.keys() {
        let stream = if is_configuration(branch_path) {
            "configuration".to_string()
        } else {
            format!(
                "tracked ({})",
                branch_path.split('/').next().unwrap_or("home")
            )
        };
        *streams.entry(stream).or_default() += 1;
    }
    miseprintln!("Tracked files in the current commit:");
    if streams.is_empty() {
        miseprintln!("  (no tracked files yet)");
    }
    for (stream, count) in &streams {
        miseprintln!("  {stream}: {count} file(s)");
    }
    setup_requirements(tracked).await?;
    if let Some(upstream) = &upstream {
        let upstream_files = super::reconcile::upstream(repo, Some(upstream))?;
        let mut present = 0;
        let mut differing = vec![];
        let mut incoming = 0;
        let roots = Roots::current();
        for (branch_path, file) in &shared.files {
            match upstream_files.files.get(branch_path) {
                Some((_, oid)) if *oid == file.oid => present += 1,
                Some(_) => differing.push(branch_path.clone()),
                None => {}
            }
        }
        for branch_path in upstream_files.files.keys() {
            if !shared.files.contains_key(branch_path)
                && roots.locate(branch_path).path().is_some()
                && run::eligible(&roots, tracked, branch_path)
            {
                incoming += 1;
            }
        }
        miseprintln!(
            "Against the repository: {present} identical, {} differing (decided with `mise bootstrap dotfiles pull --take-remote|--keep-local`), {incoming} incoming to apply.",
            differing.len()
        );
        for path in differing.iter().take(10) {
            miseprintln!("  differs: {path}");
        }
    }
    miseprintln!(
        "All committed versions of explicitly tracked files are eligible for synchronization. Untracking does not erase earlier versions."
    );
    let config_dir = crate::system::history::tracked::global_config_dir();
    if config_dir.join(".git").exists() {
        miseprintln!(
            "{} is a git checkout: enabling history does not convert or migrate it; applied files appear there as ordinary working-tree changes.",
            display_path(&config_dir)
        );
    }
    let mut status = run::read_status(state_dir)?;
    if !confirmed(opts.yes, "Connect this setup repository?")? {
        bail!("not connected");
    }
    *accepted = true;

    // the mode is recorded only when it differs from what the settings
    // say, so `mise settings set history.sync …` keeps working afterwards
    let mode =
        (opts.mode.as_str() != crate::config::Settings::get().history.sync).then_some(opts.mode);
    write_config(&opts.url, &branch, mode)?;
    // another repository or branch starts from a clean slate: the previous
    // one's per-path state, pending changes, and conflicts would read its
    // absence of a file as a deletion
    if connected_before && !same_origin {
        reset_sync_state(repo)?;
        status = run::SyncStatus::default();
        info!(
            "history: a different setup repository; the previous one's sync state is discarded (local checkpoints are kept)"
        );
    }
    status.origin_url = Some(opts.url.clone());
    status.origin_branch = Some(branch.clone());
    status.disconnected = false;
    status.adopted = repo_state == RepoState::Unmarked || status.adopted;
    run::write_status(state_dir, &status)?;
    info!(
        "history: connected {} ({}); [history.origin] written to {}",
        opts.url,
        opts.mode.as_str(),
        display_path(origin_file()?)
    );

    // the first synchronization
    drop(preview_lock.take());
    crate::config::Config::reset().await?;
    let tracked = TrackedSet::effective().await?;
    let store = Store::open_in(state_dir)?;
    // the mode just chosen decides, not the settings loaded before it was
    // written: fetch-only connects without publishing anything
    let outcome = run::sync(&store, &tracked, &SyncRequest::new(!opts.mode.publishes()))?;
    report(&outcome);
    Ok(())
}

/// The setup branch: the one asked for, else the branch this machine already
/// follows on the same repository, else the repository's own default branch.
/// Assuming `main` reads a repository that does not have it as empty, and
/// publishes an unrelated root branch beside its real history.
fn resolve_branch(
    remote: &Remote<'_>,
    requested: Option<&str>,
    connected: Option<&str>,
) -> Result<String> {
    if let Some(branch) = requested {
        let branch = branch.trim();
        if branch.is_empty() {
            bail!("a branch name is required");
        }
        return Ok(branch.to_string());
    }
    if let Some(branch) = connected {
        return Ok(branch.to_string());
    }
    // an unreachable repository is reported by the fetch that follows
    Ok(super::onboard::default_branch(remote)?.unwrap_or_else(|| DEFAULT_BRANCH.to_string()))
}

/// A missing branch is only an empty repository when the repository has no
/// branches at all; otherwise connecting would publish an unrelated root
/// branch beside the history that is already there.
pub(super) fn refuse_missing_branch(remote: &Remote<'_>, branch: &str, url: &str) -> Result<()> {
    // a listing that fails must not read as an empty repository: that is the
    // path this check exists to prevent
    let refs = remote.ls_remote()?;
    let heads: Vec<&str> = refs
        .iter()
        .filter_map(|(_, name)| name.strip_prefix("refs/heads/"))
        .collect();
    if heads.is_empty() || heads.contains(&branch) {
        return Ok(());
    }
    let mut listed: Vec<&str> = heads.iter().copied().take(10).collect();
    listed.sort_unstable();
    let more = heads.len().saturating_sub(listed.len());
    let suffix = if more > 0 {
        format!(" (and {more} more)")
    } else {
        String::new()
    };
    bail!(
        "{url} has no branch `{branch}`; it has: {}{suffix}. Connect with `--branch <name>`, or push `{branch}` there first.",
        listed.join(", ")
    );
}

pub(crate) fn report(outcome: &run::SyncOutcome) {
    match &outcome.published {
        Some(commit) => info!(
            "history: published {}",
            crate::cli::dotfiles::history::short(commit)
        ),
        None => info!("history: nothing new to publish"),
    }
    if outcome.pending > 0 {
        info!(
            "history: {} incoming change(s) pending; `mise bootstrap dotfiles pull` applies them",
            outcome.pending
        );
    }
    if outcome.conflicts > 0 {
        warn!(
            "history: {} conflict(s) need a decision; `mise bootstrap dotfiles status` lists them",
            outcome.conflicts
        );
    }
}

/// `--yes`, `MISE_YES`, or an interactive confirmation; unattended without
/// either is a refusal.
pub(crate) fn confirmed(yes: bool, question: &str) -> Result<bool> {
    if yes || crate::config::Settings::get().yes {
        return Ok(true);
    }
    if !console::user_attended_stderr() {
        return Ok(false);
    }
    Ok(prompt::confirm(question)?.is_yes())
}

/// Where the connection is declared: `config.local.toml` next to the
/// global configuration. Machine-local, so it is never published (each
/// machine names the repository the way it reaches it) and a fresh
/// machine's own declaration never conflicts with the configuration it
/// pulls.
fn origin_file() -> Result<PathBuf> {
    crate::cli::dotfiles::track::declaration_file(true)
}

/// Writes the ordinary repository connection.
pub(super) fn write_config(url: &str, branch: &str, mode: Option<SyncMode>) -> Result<()> {
    let global = origin_file()?;
    if let Some(parent) = global.parent() {
        crate::file::create_dir_all(parent)?;
    }
    let mut doc = crate::cli::dotfiles::track::read_document(&global)?;
    let history = doc
        .entry("history")
        .or_insert(Item::Table(toml_edit::Table::new()));
    let Some(history) = history.as_table_mut() else {
        bail!("[history] in {} is not a table", display_path(&global));
    };
    history.set_implicit(true);
    let origin = history
        .entry("origin")
        .or_insert(Item::Table(toml_edit::Table::new()));
    let Some(origin) = origin.as_table_mut() else {
        bail!(
            "[history.origin] in {} is not a table",
            display_path(&global)
        );
    };
    origin.set_implicit(false);
    origin.insert("url", Item::Value(Value::from(url)));
    origin.insert("branch", Item::Value(Value::from(branch)));
    let Some(mode) = mode else {
        crate::file::write(&global, doc.to_string())?;
        return Ok(());
    };
    let settings = doc
        .entry("settings")
        .or_insert(Item::Table(toml_edit::Table::new()));
    let Some(settings) = settings.as_table_mut() else {
        bail!("[settings] in {} is not a table", display_path(&global));
    };
    settings.set_implicit(true);
    let history_settings = settings
        .entry("history")
        .or_insert(Item::Table(toml_edit::Table::new()));
    let Some(history_settings) = history_settings.as_table_mut() else {
        bail!(
            "[settings.history] in {} is not a table",
            display_path(&global)
        );
    };
    history_settings.set_implicit(false);
    history_settings.insert("sync", Item::Value(Value::from(mode.as_str())));
    crate::file::write(&global, doc.to_string())?;
    Ok(())
}

/// Offer enrollment explicitly; connecting origin must not enroll prerequisites.
async fn setup_requirements(tracked: &TrackedSet) -> Result<()> {
    use crate::system::files::FileMode;
    let config = crate::config::Config::get().await?;
    let mut required = std::collections::BTreeSet::new();
    let shared_config = crate::config::global_shared_config_path();
    if shared_config.is_file() {
        required.insert(shared_config);
    }
    for request in crate::system::files::composed_files_from_config(&config)? {
        if request.enabled
            && crate::system::files::declaration_is_global(&config, &request)
            && matches!(
                request.mode,
                FileMode::Symlink | FileMode::SymlinkEach | FileMode::Copy | FileMode::Template
            )
        {
            required.insert(request.source);
        }
    }
    let roots = Roots::current();
    for path in required {
        if !tracked.would_capture(&path)? {
            let display = display_path(&path);
            if roots.branch_path(&path, None).is_some() {
                miseprintln!(
                    "Not enrolled for recreating this setup: {display}. To include it explicitly: mise bootstrap dotfiles track {}",
                    shell_escape::escape(display.as_str().into())
                );
            } else {
                miseprintln!(
                    "Not portable for setup sharing: {display}. Place this prerequisite under your home or mise config directory before enrolling it."
                );
            }
        }
    }
    Ok(())
}

/// Forget operational state when switching to a different origin.
fn reset_sync_state(repo: &crate::system::history::shadow::HistoryRepo) -> Result<()> {
    super::state::clear(repo)?;
    Ok(())
}

/// Disconnects: the declaration is removed; local refs, state, and
/// checkpoints stay.
pub(crate) fn remove() -> Result<()> {
    let state_dir: &std::path::Path = &crate::dirs::STATE;
    let _sync_lock = run::lock_wait(state_dir, run::STATUS_LOCK_WAIT)?;
    let mut status = run::read_status(state_dir)?;
    remove_locked(state_dir, &mut status)
}

/// The caller holds the sync lock across both configuration and state writes.
fn remove_locked(state_dir: &std::path::Path, status: &mut run::SyncStatus) -> Result<()> {
    let mut removed = vec![];
    // the machine-local file, and the shared one for a declaration written
    // there by hand or by an earlier mise
    for file in [origin_file()?, crate::config::global_shared_config_path()] {
        if !file.exists() {
            continue;
        }
        let mut doc = crate::cli::dotfiles::track::read_document(&file)?;
        let mut changed = false;
        if let Some(history) = doc.get_mut("history").and_then(Item::as_table_mut) {
            changed = history.remove("origin").is_some();
        }
        if changed {
            crate::file::write(&file, doc.to_string())?;
            removed.push(display_path(&file));
        }
    }
    // the recorded connection no longer stands in for a declaration
    let mut disconnected = false;
    if status.origin_url.is_some() && !status.disconnected {
        status.disconnected = true;
        disconnected = true;
    }
    run::write_status(state_dir, status)?;
    if disconnected && removed.is_empty() {
        removed.push("the recorded connection".to_string());
    }
    if removed.is_empty() {
        info!("history: no setup repository was connected");
    } else {
        info!(
            "history: disconnected; [history.origin] removed from {} (local checkpoints and fetched refs are kept)",
            removed.join(" and ")
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::system::history::shadow::HistoryRepo;

    /// A bare repository whose only branch is `name`, advertised through
    /// `HEAD` the way a hosted repository advertises its default branch.
    fn origin_with_branch(dir: &std::path::Path, name: &str) -> (HistoryRepo, String) {
        let repo = HistoryRepo::open_or_init_in(dir).unwrap().unwrap();
        let tree = repo.empty_object("tree").unwrap();
        let commit = repo.commit_tree(&tree, vec![], "setup").unwrap();
        repo.update_ref(&format!("refs/heads/{name}"), &commit, None)
            .unwrap();
        std::fs::write(repo.dir().join("HEAD"), format!("ref: refs/heads/{name}\n")).unwrap();
        let url = url::Url::from_file_path(repo.dir()).unwrap().to_string();
        (repo, url)
    }

    fn local_repo(dir: &std::path::Path) -> HistoryRepo {
        HistoryRepo::open_or_init_in(dir).unwrap().unwrap()
    }

    #[test]
    fn an_unrequested_branch_follows_the_repository_default() {
        if crate::git::plumbing_binary().is_none() {
            return;
        }
        let temp = tempfile::tempdir().unwrap();
        for name in ["master", "main", "trunk"] {
            let (_origin, url) = origin_with_branch(&temp.path().join(name), name);
            let local = local_repo(&temp.path().join(format!("{name}-local")));
            let remote = Remote::new(&local, &url);
            assert_eq!(resolve_branch(&remote, None, None).unwrap(), name);
        }
    }

    #[test]
    fn a_requested_branch_is_taken_as_given() {
        if crate::git::plumbing_binary().is_none() {
            return;
        }
        let temp = tempfile::tempdir().unwrap();
        let (_origin, url) = origin_with_branch(&temp.path().join("origin"), "master");
        let local = local_repo(&temp.path().join("local"));
        let remote = Remote::new(&local, &url);
        assert_eq!(
            resolve_branch(&remote, Some("release"), None).unwrap(),
            "release"
        );
        assert_eq!(
            resolve_branch(&remote, Some(" release "), None).unwrap(),
            "release"
        );
        assert!(resolve_branch(&remote, Some("  "), None).is_err());
        // an explicit branch still wins over the one already followed
        assert_eq!(
            resolve_branch(&remote, Some("release"), Some("main")).unwrap(),
            "release"
        );
    }

    #[test]
    fn a_live_connection_keeps_the_branch_it_follows() {
        if crate::git::plumbing_binary().is_none() {
            return;
        }
        let temp = tempfile::tempdir().unwrap();
        let (_origin, url) = origin_with_branch(&temp.path().join("origin"), "master");
        let local = local_repo(&temp.path().join("local"));
        let remote = Remote::new(&local, &url);
        // re-running the connection does not move it to the default branch
        assert_eq!(resolve_branch(&remote, None, Some("main")).unwrap(), "main");
        // a connection that was removed detects again
        assert_eq!(resolve_branch(&remote, None, None).unwrap(), "master");
    }

    #[test]
    fn a_repository_without_branches_takes_the_default_name() {
        if crate::git::plumbing_binary().is_none() {
            return;
        }
        let temp = tempfile::tempdir().unwrap();
        let empty = local_repo(&temp.path().join("empty"));
        let url = url::Url::from_file_path(empty.dir()).unwrap().to_string();
        let local = local_repo(&temp.path().join("local"));
        let remote = Remote::new(&local, &url);
        assert_eq!(resolve_branch(&remote, None, None).unwrap(), DEFAULT_BRANCH);
    }

    #[test]
    fn a_missing_branch_is_not_reported_as_an_empty_repository() {
        if crate::git::plumbing_binary().is_none() {
            return;
        }
        let temp = tempfile::tempdir().unwrap();
        let (_origin, url) = origin_with_branch(&temp.path().join("origin"), "master");
        let local = local_repo(&temp.path().join("local"));
        let remote = Remote::new(&local, &url);
        assert!(!remote.fetch("main").unwrap());
        let error = refuse_missing_branch(&remote, "main", &url)
            .unwrap_err()
            .to_string();
        assert!(error.contains("no branch `main`"), "{error}");
        assert!(error.contains("master"), "{error}");
        // the repository's own default branch connects without complaint
        assert!(remote.fetch("master").unwrap());
        assert!(refuse_missing_branch(&remote, "master", &url).is_ok());
    }

    #[test]
    fn an_empty_repository_still_reads_as_empty() {
        if crate::git::plumbing_binary().is_none() {
            return;
        }
        let temp = tempfile::tempdir().unwrap();
        let empty = local_repo(&temp.path().join("empty"));
        let url = url::Url::from_file_path(empty.dir()).unwrap().to_string();
        let local = local_repo(&temp.path().join("local"));
        let remote = Remote::new(&local, &url);
        assert!(!remote.fetch(DEFAULT_BRANCH).unwrap());
        assert!(refuse_missing_branch(&remote, DEFAULT_BRANCH, &url).is_ok());
    }
}
