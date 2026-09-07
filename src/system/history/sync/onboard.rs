//! Setting a machine up from a history-managed setup repository:
//! `mise bootstrap --from-git <url>` on a repository that carries the
//! `.mise-history/format.toml` marker, locally or on a remote host through
//! the bundle `mise bootstrap remote --from-git` transfers. The branch is
//! fetched into mise's own bare store, never cloned into the configuration
//! directory; the shared configuration, the sources it references, and this
//! machine's tracked streams are written by the same recoverable pull as any
//! other incoming change (a file that differs is held for a decision, never
//! overwritten); the connection is declared machine-locally and recorded
//! for the watcher. An ordinary repository (no marker) is left to the
//! ordinary `--from-git`.

use std::process::{Command, Stdio};

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
}

pub(crate) struct Outcome {
    /// Incoming configuration staged privately for the ordinary bootstrap
    /// preview. The caller owns its lifetime; live configuration is untouched.
    pub preview_config: Option<tempfile::TempDir>,
    /// The repository can be reached without this session's borrowed
    /// GitHub access.
    pub durable_access: bool,
    /// The shared configuration was not written: it waits for a decision
    /// (a differing configuration is here already), so no bootstrap ran.
    pub configuration_held: bool,
}

/// Snapshot only the state needed for planning while its writers are locked.
/// All fetches, reconciliation, and preview status writes then target this
/// private store, so cancellation never needs to roll back live bookkeeping.
fn preview_store(store: &Store) -> Result<(tempfile::TempDir, Store)> {
    let _sync = run::lock(store)?;
    let _capture = store.lock()?;
    let temp = tempfile::tempdir()?;
    hstore::ensure_store_dir_in(temp.path())?;
    std::fs::copy(
        hstore::machine_file_in(store.state_dir()),
        hstore::machine_file_in(temp.path()),
    )?;
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
    // Reopen to rebuild checkpoint and promotion indexes from the copied refs.
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
    for entry in repo.ls_tree(&head)? {
        if super::layout::is_configuration(&entry.path)
            && super::layout::is_safe_branch_path(&entry.path)
        {
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
                &temp.path().join(&entry.path),
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

/// `mise bootstrap --from-git <url>`: `Some` when the repository is
/// history-managed and this machine was set up from it (or would be, on a
/// dry run); `None` leaves the ordinary clone to the caller.
pub(crate) async fn from_git(url: &str, yes: bool, dry_run: bool) -> Result<Option<Outcome>> {
    // Detect marked repositories without creating persistent tracking state
    // for users of the released, ordinary --from-git workflow.
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
        },
    )
    .await?;
    Ok(Some(outcome))
}

/// The branch a fresh machine takes: the repository's default branch (its
/// `HEAD`), else `main`, else `master`, else the first head; `None` when the
/// repository lists none (unreachable, empty).
fn default_branch(remote: &Remote<'_>) -> Result<Option<String>> {
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
            "this machine is connected to {url}; `mise bootstrap dotfiles origin --remove` disconnects it, or `mise bootstrap dotfiles origin set {origin}` moves it"
        );
    }
    Ok(())
}

/// Fetches the branch into the store and says what the repository is. A
/// format this mise does not understand is an error; an ordinary
/// repository leaves nothing behind.
pub(crate) fn probe(store: &Store, fetch_from: &str, branch: &str) -> Result<RepoState> {
    let repo = store
        .repo()
        .ok_or_else(|| eyre::eyre!("probing a setup repository requires git"))?;
    Remote::new(repo, fetch_from).fetch(branch)?;
    let upstream = repo.ref_oid(UPSTREAM_REF)?;
    let state = format::detect(repo, upstream.as_deref())?;
    state.check()?;
    if !matches!(state, RepoState::Marked(_)) && upstream.is_some() {
        repo.delete_ref(UPSTREAM_REF)?;
    }
    Ok(state)
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
        "The shared configuration goes to {}, sources to where that configuration puts them, and this machine's tracked files to their places under {}. A file that already exists and differs is held for a decision, never overwritten.",
        display_path(&config_dir),
        display_path(*crate::dirs::HOME)
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
    // backups follow the other machines: where they encrypt, this machine
    // does too, for the identities found here; a declaration already in
    // place (a re-run) keeps whatever it says
    let declared = crate::system::history::config::origin()?
        .is_some_and(|(_, origin)| origin.url == onboarding.origin);
    let backups = if declared {
        None
    } else if let Some(repo) = planning_store.repo()
        && super::machines::any_encrypted(repo, &store.machine().id)?
    {
        let recipients = crate::agecrypt::default_recipient_strings().await?;
        if recipients.is_empty() {
            miseprintln!(
                "Other machines encrypt their backups, but no age identity or SSH public key was found here: nothing is backed up from this machine until `mise bootstrap dotfiles origin set {} --encrypt-backups` adds recipients (plaintext is never used as a fallback).",
                onboarding.origin
            );
        } else {
            miseprintln!(
                "Other machines encrypt their backups; this machine's are encrypted too, for the {} recipient(s) found here.",
                recipients.len()
            );
        }
        Some(super::origin::BackupConfig {
            encrypt: true,
            recipients,
        })
    } else {
        Some(super::origin::BackupConfig {
            encrypt: false,
            recipients: vec![],
        })
    };
    if onboarding.dry_run {
        let preview_config = Some(preview_configuration(&planning_store)?);
        miseprintln!("Dry run: nothing was changed.");
        return Ok(Outcome {
            preview_config,
            durable_access: true,
            configuration_held: false,
        });
    }
    if !super::origin::confirmed(onboarding.yes, "Set this machine up from the repository?")? {
        bail!("not set up");
    }

    // Recompute after confirmation against current local files and remote
    // refs; never apply a stale preview or restore over a watcher's new state.
    refuse_other_connection(store, &onboarding.origin, &onboarding.branch)?;
    request.capture = true;
    request.dry_run = false;
    run::sync(store, &tracked, &request)?;

    // the connection: declared machine-locally, recorded for the watcher
    let newest = store
        .list()?
        .into_iter()
        .last()
        .map(|entry| entry.checkpoint.created_at);
    run::update_status(state_dir, run::STATUS_LOCK_WAIT, |status| {
        status.origin_url = Some(onboarding.origin.clone());
        status.origin_branch = Some(onboarding.branch.clone());
        status.disconnected = false;
        if status.upload_since.is_none() {
            status.upload_since = Some(newest.unwrap_or_else(hstore::now_rfc3339));
        }
    })?;
    super::origin::write_config(
        &onboarding.origin,
        &onboarding.branch,
        None,
        backups.as_ref(),
    )?;

    let applied = apply::apply(store, &tracked, &ApplyRequest::automatic()).await?;
    // a conflict (a file that exists here and differs) is not pending: it
    // waits for a decision, like a path held with its group
    let undecided = run::read_status(store.state_dir())?
        .conflicts
        .len()
        .max(applied.held);
    // configuration among them: nothing to bootstrap from yet
    let configuration_held = {
        let status = run::read_status(state_dir)?;
        status
            .pending_applications
            .iter()
            .any(|pending| pending.configuration)
            || status
                .conflicts
                .iter()
                .any(|conflict| super::layout::is_configuration(&conflict.branch_path))
    };
    miseprintln!(
        "Wrote {} file(s) from {}{}.",
        applied.written,
        onboarding.origin,
        if undecided > 0 {
            format!(
                "; {undecided} path(s) need a decision (`mise bootstrap dotfiles status` lists them, `mise bootstrap dotfiles pull --take-remote|--keep-local <path>` decides)"
            )
        } else {
            String::new()
        }
    );
    let durable_access = durable_access(&onboarding.origin, &onboarding.branch);
    if !durable_access {
        warn!(
            "setup complete, but ongoing synchronization needs credentials on this host: the borrowed GitHub access ends with this session. Run `mise x gh -- gh auth login` and `mise x gh -- gh auth setup-git` here, or connect an SSH url with `mise bootstrap dotfiles origin set <url>`"
        );
    }
    Ok(Outcome {
        preview_config: None,
        durable_access,
        configuration_held,
    })
}

/// Whether this host reaches the repository on its own: with the
/// session's GitHub relay (its `url.<relay>.insteadOf` rewrites travel as
/// `GIT_CONFIG_KEY_n`/`GIT_CONFIG_VALUE_n`) taken out of the environment.
/// Without a relay, whatever works now keeps working.
fn durable_access(url: &str, branch: &str) -> bool {
    if std::env::var_os("MISE_GITHUB_RELAY_SOCKET").is_none() {
        return true;
    }
    let Some(git) = crate::file::which_spawnable("git") else {
        return true;
    };
    let mut command = Command::new(git);
    command
        .args(["ls-remote", "--exit-code", "--heads", url, branch])
        .env("GIT_TERMINAL_PROMPT", "0")
        .env_remove("MISE_GITHUB_RELAY_SOCKET")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    for (key, _) in std::env::vars_os() {
        let name = key.to_string_lossy();
        if name == "GIT_CONFIG_COUNT"
            || name.starts_with("GIT_CONFIG_KEY_")
            || name.starts_with("GIT_CONFIG_VALUE_")
        {
            command.env_remove(&key);
        }
    }
    command.status().is_ok_and(|status| status.success())
}
