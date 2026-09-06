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
    /// The repository can be reached without this session's borrowed
    /// GitHub access.
    pub durable_access: bool,
}

/// `mise bootstrap --from-git <url>`: `Some` when the repository is
/// history-managed and this machine was set up from it (or would be, on a
/// dry run); `None` leaves the ordinary clone to the caller.
pub(crate) async fn from_git(url: &str, yes: bool, dry_run: bool) -> Result<Option<Outcome>> {
    let store = Store::open()?;
    if let Some(reason) = store.unavailable() {
        debug!("history: {url} is not probed for a setup repository: {reason}");
        return Ok(None);
    }
    let repo = store
        .repo()
        .ok_or_else(|| eyre::eyre!("probing a setup repository requires git"))?;
    let Some(branch) = default_branch(&Remote::new(repo, url))? else {
        return Ok(None);
    };
    refuse_other_connection(&store, url, &branch)?;
    if !matches!(probe(&store, url, &branch)?, RepoState::Marked(_)) {
        return Ok(None);
    }
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

/// The branch a fresh machine takes: `main`, else `master`, else the first
/// head; `None` when the repository lists none (unreachable, empty).
fn default_branch(remote: &Remote<'_>) -> Result<Option<String>> {
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
    let status = run::read_status(store.state_dir());
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
    miseprintln!(
        "{} is a mise setup repository (format {}); branch {}.",
        onboarding.origin,
        format::FORMAT,
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

    // what is pending, then the plan
    let tracked = TrackedSet::effective().await?;
    let mut request = SyncRequest::new(true);
    request.origin = Some(OriginTomlConfig {
        url: onboarding.fetch_from.clone(),
        branch: onboarding.branch.clone(),
        encrypt_backups: false,
    });
    request.capture = !onboarding.dry_run;
    let synced = run::sync(store, &tracked, &request)?;
    let mut preview = ApplyRequest::automatic();
    preview.automatic = false;
    preview.dry_run = true;
    preview.plan_only = true;
    apply::apply(store, &tracked, &preview).await?;
    if onboarding.dry_run {
        miseprintln!("Dry run: nothing was changed.");
        return Ok(Outcome {
            durable_access: true,
        });
    }
    if !super::origin::confirmed(onboarding.yes, "Set this machine up from the repository?")? {
        bail!("not set up");
    }

    // the connection: declared machine-locally, recorded for the watcher
    let mut status = run::read_status(state_dir);
    status.origin_url = Some(onboarding.origin.clone());
    status.origin_branch = Some(onboarding.branch.clone());
    status.disconnected = false;
    if status.upload_since.is_none() {
        let newest = store
            .list()?
            .into_iter()
            .last()
            .map(|entry| entry.checkpoint.created_at);
        status.upload_since = Some(newest.unwrap_or_else(hstore::now_rfc3339));
    }
    run::write_status(state_dir, &status)?;
    super::origin::write_config(&onboarding.origin, &onboarding.branch, None)?;

    let applied = apply::apply(store, &tracked, &ApplyRequest::automatic()).await?;
    // a conflict (a file that exists here and differs) is not pending: it
    // waits for a decision, like a path held with its group
    let undecided = applied.held + synced.conflicts;
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
    Ok(Outcome { durable_access })
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
