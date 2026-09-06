//! Connecting, disconnecting, and purging the setup repository. Connecting
//! discloses exactly what will happen before anything leaves the machine:
//! the sync mode, what is shared per stream, what is not and why, what is
//! backed up, secret-looking names, committed private content already in
//! the repository's history, and how an existing unmarked repository would
//! be adopted.

use std::collections::BTreeMap;
use std::path::PathBuf;

use eyre::{Result, bail};
use toml_edit::{Item, Value};

use super::SyncMode;
use super::format::{self, RepoState};
use super::layout::{Roots, is_configuration};
use super::network::{MACHINES_PREFIX, PLAIN_PREFIX, PUBLISH_REF, Remote, UPSTREAM_REF};
use super::run::{self, SyncRequest};
use super::{backup, privacy, share};
use crate::file::display_path;
use crate::system::history::checkpoint::Store;
use crate::system::history::store::{self as hstore, Machine};
use crate::system::history::tracked::{EntryKind, TrackedSet};
use crate::ui::prompt;

pub(crate) struct SetOptions {
    pub url: String,
    pub branch: String,
    pub name: Option<String>,
    pub mode: SyncMode,
    pub include_existing: bool,
    pub allow_committed_private: bool,
    pub encrypt_backups: bool,
    /// `--recipient` arguments: keys, or files holding them.
    pub recipients: Vec<String>,
    pub yes: bool,
}

/// What `[history.origin]` says about backups.
pub(crate) struct BackupConfig {
    pub encrypt: bool,
    pub recipients: Vec<String>,
}

/// How this connection will back up, decided before the disclosure.
enum BackupPlan {
    Plain,
    Encrypted {
        recipients: Vec<String>,
        /// No recipient was found anywhere: generate an identity here.
        generate: Option<PathBuf>,
    },
}

/// A recipient as printed: an SSH key is long, so its type and the start
/// of its material stand for it.
fn short_recipient(recipient: &str) -> String {
    let Some((kind, rest)) = recipient.split_once(' ') else {
        return recipient.to_string();
    };
    if rest.chars().count() <= 20 {
        return recipient.to_string();
    }
    format!("{kind} {}…", rest.chars().take(16).collect::<String>())
}

/// Connects the setup repository.
pub(crate) async fn set(store: &Store, tracked: &TrackedSet, opts: &SetOptions) -> Result<()> {
    super::network::validate_url(&opts.url)?;
    let mut preview_lock = Some(run::lock(store)?);
    let repo = store
        .repo()
        .ok_or_else(|| eyre::eyre!("connecting requires git"))?;
    let previous_upstream = repo.ref_oid(UPSTREAM_REF)?;
    let previous_machines = repo.list_refs(MACHINES_PREFIX)?;
    let mut accepted = false;
    let result = set_inner(store, tracked, opts, &mut accepted, &mut preview_lock).await;
    if !accepted {
        // Preview fetches must not change the active connection, even when
        // validation or privacy checks fail before the prompt.
        let mut cleanup_errors = Vec::new();
        match repo.list_refs(MACHINES_PREFIX) {
            Ok(refs) => {
                for (name, _) in refs {
                    if !previous_machines.iter().any(|(old, _)| old == &name)
                        && let Err(err) = repo.delete_ref(&name)
                    {
                        cleanup_errors.push(format!("{name}: {err:#}"));
                    }
                }
            }
            Err(err) => cleanup_errors.push(format!("listing fetched machine refs: {err:#}")),
        }
        for (name, oid) in previous_machines {
            let restored = repo
                .ref_oid(&name)
                .and_then(|current| repo.update_ref(&name, &oid, current.as_deref()));
            if let Err(err) = restored {
                cleanup_errors.push(format!("{name}: {err:#}"));
            }
        }
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
    if !opts.recipients.is_empty() && !opts.encrypt_backups {
        bail!("--recipient needs --encrypt-backups");
    }
    let repo = store
        .repo()
        .ok_or_else(|| eyre::eyre!("connecting a setup repository requires git"))?;
    let state_dir = store.state_dir();
    let mut machine = store.machine().clone();
    if let Some(name) = &opts.name {
        machine = Machine {
            id: machine.id.clone(),
            name: name.clone(),
        };
    }
    // what a previous repository left behind, before the fetch adds to it
    let previous_machine_refs = repo.list_refs(MACHINES_PREFIX)?;
    // nothing is pruned before the confirmation: declining leaves the
    // connected repository's recovery refs as they were
    let remote = Remote::new(repo, &opts.url);
    if !remote.fetch(&opts.branch)? && repo.ref_oid(UPSTREAM_REF)?.is_some() {
        repo.delete_ref(UPSTREAM_REF)?;
    }
    let upstream = repo.ref_oid(UPSTREAM_REF)?;
    let repo_state = format::detect(repo, upstream.as_deref())?;
    repo_state.check()?;

    run::capture_now(store, tracked);
    let shared = share::current(repo, store, tracked)?;
    let walk = tracked.walk()?;
    // the connection as it was: another repository or branch starts from a
    // clean slate, and a scheme change on the same one replaces its refs
    let status_before = run::read_status(state_dir)?;
    let connected_before = status_before.origin_url.is_some();
    let same_origin = status_before.origin_url.as_deref() == Some(opts.url.as_str())
        && status_before.origin_branch.as_deref() == Some(opts.branch.as_str());
    let backups = if opts.encrypt_backups {
        let mut recipients: Vec<String> = vec![];
        for arg in &opts.recipients {
            recipients.extend(crate::agecrypt::resolve_recipient_arg(arg).await?);
        }
        if recipients.is_empty() {
            recipients = crate::agecrypt::default_recipient_strings().await?;
        }
        let mut seen = std::collections::BTreeSet::new();
        recipients.retain(|recipient| seen.insert(recipient.clone()));
        let generate = recipients
            .is_empty()
            .then(crate::agecrypt::default_identity_file);
        BackupPlan::Encrypted {
            recipients,
            generate,
        }
    } else {
        BackupPlan::Plain
    };

    // committed private content in the branch's history
    if let Some(head) = &upstream {
        let unshared: Vec<String> = unshared_destinations(&shared, tracked);
        let found = privacy::committed_private(repo, head, &unshared, 2000)?;
        if !found.is_empty() {
            miseprintln!("{} already holds private content in its history:", opts.url);
            for item in found.iter().take(20) {
                miseprintln!(
                    "  {} (commit {})",
                    item.path,
                    crate::cli::dotfiles::history::short(&item.commit)
                );
            }
            if found.len() > 20 {
                miseprintln!("  … {} more", found.len() - 20);
            }
            if !opts.allow_committed_private {
                bail!(
                    "remove it upstream first (rewriting history is your decision, never mise's), or pass --allow-committed-private to continue knowingly"
                );
            }
            miseprintln!("continuing with --allow-committed-private");
        }
    }

    // disclosure
    miseprintln!("Setup repository: {} (branch {})", opts.url, opts.branch);
    miseprintln!("Machine: {} ({})", machine.name, machine.id);
    miseprintln!("Sync mode {}", opts.mode.disclosure());
    match &repo_state {
        RepoState::Empty => miseprintln!(
            "The repository is empty: the first publication creates `{}` with the mise marker.",
            opts.branch
        ),
        RepoState::Marked(_) => {
            miseprintln!("The repository is a mise setup repository; continuing.")
        }
        RepoState::Unmarked => {
            miseprintln!(
                "The repository already has content without the mise marker. Adopting it means the first publication adds the marker in the same commit as the files below; unrelated files and their history stay exactly as they are. Until you confirm, ordinary `--from`/`--from-git` use of it keeps its old behaviour and nothing is published."
            );
        }
    }
    let mut streams: BTreeMap<String, usize> = BTreeMap::new();
    for branch_path in shared.files.keys() {
        let stream = if is_configuration(branch_path) {
            "configuration".to_string()
        } else if let Some(rest) = branch_path.strip_prefix("tracked/") {
            format!("tracked ({})", rest.split('/').next().unwrap_or("home"))
        } else {
            "sources".to_string()
        };
        *streams.entry(stream).or_default() += 1;
    }
    miseprintln!("Published from this machine:");
    if streams.is_empty() {
        miseprintln!("  (nothing shareable yet)");
    }
    for (stream, count) in &streams {
        miseprintln!("  {stream}: {count} file(s)");
    }
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
                && !privacy::is_private_branch_path(branch_path)
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
    if !shared.unshared.is_empty() {
        miseprintln!("Not shared:");
        for unshared in shared.unshared.iter().take(30) {
            miseprintln!("  {}: {}", display_path(&unshared.local), unshared.reason);
        }
        if shared.unshared.len() > 30 {
            miseprintln!("  … {} more", shared.unshared.len() - 30);
        }
    }
    if !shared.overrides.is_empty() {
        miseprintln!("Privacy overrides (private by default, shared by a per-file declaration):");
        for path in &shared.overrides {
            miseprintln!("  {}", display_path(path));
        }
    }
    // Show actual paths in both outgoing channels: not sharing a file does
    // not imply that its recovery history stays local.
    let shared_paths: BTreeMap<_, _> = shared
        .files
        .values()
        .map(|file| (&file.local, file))
        .collect();
    miseprintln!(
        "File destinations (current captured files; historical backups follow the policies below):"
    );
    for group in ["Local only", "Shared setup", "Remote backup"] {
        miseprintln!("  {group}:");
        let mut count = 0;
        for (path, (_, policy)) in &walk.files {
            let shared_file = shared_paths.get(path);
            let backup =
                policy.backup && (!policy.encrypt || !matches!(backups, BackupPlan::Plain));
            let included = match group {
                "Local only" => shared_file.is_none() && !backup,
                "Shared setup" => shared_file.is_some(),
                _ => backup,
            };
            if !included {
                continue;
            }
            count += 1;
            let protection = match group {
                "Local only" => "local history only",
                "Shared setup" if shared_file.is_some_and(|file| file.encrypt) => {
                    "encrypted contents; path visible"
                }
                "Remote backup" if !matches!(backups, BackupPlan::Plain) => {
                    "encrypted contents and paths"
                }
                _ => "plaintext",
            };
            miseprintln!("    {} ({protection})", display_path(path));
        }
        if count == 0 {
            miseprintln!("    (none)");
        }
    }
    miseprintln!(
        "A file can be both shared and backed up. `track <path> --private` disables both for its contents; its declaration may still be shared. Previously published content is not erased."
    );
    let backed_up = walk
        .files
        .iter()
        .filter(|(_, (_, policy))| {
            policy.backup && (!policy.encrypt || !matches!(backups, BackupPlan::Plain))
        })
        .count();
    match &backups {
        BackupPlan::Plain => miseprintln!(
            "Machine backups: {backed_up} of {} captured file(s) are backed up in plain form under refs/mise-history/{}/ — anyone who can read this repository can read every file in these snapshots. Setup configuration stays plaintext; selected dotfiles can be encrypted. Use a private repository.",
            walk.files.len(),
            machine.id
        ),
        BackupPlan::Encrypted {
            recipients,
            generate,
        } => {
            match generate {
                None => {
                    miseprintln!(
                        "Machine backups: {backed_up} of {} captured file(s) are backed up encrypted (age) under refs/mise-history/{}/ for {} recipient(s):",
                        walk.files.len(),
                        machine.id,
                        recipients.len()
                    );
                    for recipient in recipients {
                        miseprintln!("  {}", short_recipient(recipient));
                    }
                }
                Some(path) => miseprintln!(
                    "Machine backups: {backed_up} of {} captured file(s) are backed up encrypted (age) under refs/mise-history/{}/. No age identity or SSH public key was found here: an age identity will be generated at {} (mode 0600) and used as the only recipient. Keep it: without it these backups cannot be restored; copy it to every machine that should read them.",
                    walk.files.len(),
                    machine.id,
                    display_path(path)
                ),
            }
            miseprintln!(
                "File names, descriptions, and content are readable only with one of these identities; the machine name, checkpoint time, and count stay visible. Setup configuration stays plaintext; selected dotfiles can be encrypted. Use a private repository."
            );
            if generate.is_none() {
                let restorable = crate::agecrypt::restorability(recipients).await;
                if restorable.is_none() {
                    miseprintln!(
                        "Hardware identity configured but unverified; restore interactively to unlock it."
                    );
                }
                if recipients
                    .iter()
                    .all(|r| r.parse::<age::plugin::Recipient>().is_ok())
                {
                    miseprintln!(
                        "Hardware-only recipients: losing these devices can make backups unrecoverable. Add an independent recovery recipient."
                    );
                }
                if restorable == Some(false) {
                    miseprintln!(
                        "Note: none of this machine's identities is among the recipients, so this machine could not restore its own backups."
                    );
                }
            }
        }
    }
    if same_origin && !status_before.uploaded.is_empty() {
        let previous_plain = status_before
            .backup_scheme
            .as_deref()
            .is_none_or(|scheme| scheme == backup::PLAIN_SCHEME);
        let changes = match &backups {
            BackupPlan::Plain => backup::scheme_changes(&status_before, backup::PLAIN_SCHEME),
            BackupPlan::Encrypted {
                generate: Some(_), ..
            } => true,
            BackupPlan::Encrypted { recipients, .. } => {
                let planned = backup::BackupEncryption {
                    recipients: vec![],
                    strings: recipients.clone(),
                };
                backup::scheme_changes(&status_before, &backup::scheme(Some(&planned)))
            }
        };
        if changes {
            miseprintln!(
                "This machine's {} existing recovery ref(s) on the repository are replaced: the {} ones are deleted and eligible checkpoints are uploaded again {}.",
                status_before.uploaded.len(),
                if previous_plain {
                    "plaintext"
                } else {
                    "previously encrypted"
                },
                if matches!(backups, BackupPlan::Plain) {
                    "IN PLAIN FORM"
                } else {
                    "encrypted"
                }
            );
        }
    }
    miseprintln!(
        "Existing checkpoints: {}",
        if opts.include_existing {
            "included (--include-existing)"
        } else {
            "not uploaded; only checkpoints from now on (pass --include-existing to upload them too)"
        }
    );
    let secrets = privacy::secret_names(walk.files.keys().map(PathBuf::as_path));
    if !secrets.is_empty() {
        miseprintln!(
            "Files whose names look like secrets (each is captured; make sure the policy is what you want):"
        );
        for path in secrets.iter().take(20) {
            let policy = walk.files.get(path).map(|(_, policy)| *policy);
            let state = match policy {
                Some(policy) if !policy.share && !policy.backup => "never leaves this machine",
                Some(policy) if !policy.share => "backed up, not shared",
                _ => "SHARED",
            };
            miseprintln!(
                "  {} ({state}): mise bootstrap dotfiles track {} --no-share --no-backup",
                display_path(path),
                display_path(path)
            );
        }
    }
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

    // record: machine name, configuration, status
    if opts.name.is_some() {
        hstore::write_json(&hstore::machine_file_in(state_dir), &machine)?;
    }
    // the mode is recorded only when it differs from what the settings
    // say, so `mise settings set history.sync …` keeps working afterwards
    let mode =
        (opts.mode.as_str() != crate::config::Settings::get().history.sync).then_some(opts.mode);
    let backup_config = match backups {
        BackupPlan::Plain => BackupConfig {
            encrypt: false,
            recipients: vec![],
        },
        BackupPlan::Encrypted {
            mut recipients,
            generate,
        } => {
            if let Some(path) = generate {
                let public = crate::agecrypt::generate_identity_file(&path)?;
                miseprintln!(
                    "Generated an age identity at {}; its public key is {public}. Copy the file to every machine that should restore these backups (mise never uploads it).",
                    display_path(&path)
                );
                recipients = vec![public];
            }
            BackupConfig {
                encrypt: true,
                recipients,
            }
        }
    };
    write_config(&opts.url, &opts.branch, mode, Some(&backup_config))?;
    // another repository or branch starts from a clean slate: the previous
    // one's per-path state, pending changes, and conflicts would read its
    // absence of a file as a deletion
    if connected_before && !same_origin {
        reset_sync_state(repo, &previous_machine_refs)?;
        status = run::SyncStatus::default();
        info!(
            "history: a different setup repository; the previous one's sync state is discarded (local checkpoints are kept)"
        );
    }
    status.origin_url = Some(opts.url.clone());
    status.origin_branch = Some(opts.branch.clone());
    status.disconnected = false;
    status.adopted = repo_state == RepoState::Unmarked || status.adopted;
    // from now on: the checkpoint holding the current state (taken above)
    // is included, not only what changes later
    status.upload_since = if opts.include_existing {
        None
    } else if same_origin {
        status.upload_since.clone()
    } else {
        let newest = store
            .list()?
            .into_iter()
            .last()
            .map(|entry| entry.checkpoint.created_at);
        Some(newest.unwrap_or_else(hstore::now_rfc3339))
    };
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

pub(crate) fn report(outcome: &run::SyncOutcome) {
    match &outcome.published {
        Some(commit) => info!(
            "history: published {}",
            crate::cli::dotfiles::history::short(commit)
        ),
        None => info!("history: nothing new to publish"),
    }
    if outcome.uploaded > 0 {
        info!("history: uploaded {} checkpoint(s)", outcome.uploaded);
    }
    if outcome.pruned_remote > 0 {
        info!(
            "history: removed {} pruned checkpoint(s) from the origin",
            outcome.pruned_remote
        );
    }
    if outcome.replaced_remote > 0 {
        info!(
            "history: replaced {} recovery ref(s) on the origin (the backup scheme changed)",
            outcome.replaced_remote
        );
    }
    if let Some(error) = &outcome.backup_error {
        warn!("history: machine backups skipped: {error}");
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

/// Setup-branch paths of local entries with `share = false`, so their
/// committed copies upstream count as private content.
fn unshared_destinations(shared: &share::ShareReport, tracked: &TrackedSet) -> Vec<String> {
    let roots = Roots::current();
    let mut out = vec![];
    for entry in &tracked.entries {
        if entry.kind == EntryKind::Track
            && !entry.policy.share
            && let Some(path) = roots.branch_path(entry.kind, &entry.path, entry.variant.as_deref())
        {
            out.push(path);
        }
    }
    // the walked files, under the entry that owns each (a derived symlink
    // target too, which no declared entry covers): a source or a
    // configuration file lives elsewhere in the branch than a tracked file
    out.extend(
        shared
            .unshared
            .iter()
            .filter_map(|unshared| unshared.branch_path.clone()),
    );
    out.sort();
    out.dedup();
    out
}

/// Where the connection is declared: `config.local.toml` next to the
/// global configuration. Machine-local, so it is never published (each
/// machine names the repository the way it reaches it) and a fresh
/// machine's own declaration never conflicts with the configuration it
/// pulls.
fn origin_file() -> Result<PathBuf> {
    crate::cli::dotfiles::track::declaration_file(true)
}

/// Writes `[history.origin]`. `backups` `None` leaves any `encrypt_backups`
/// and `recipients` keys as they are (a re-run must not undo a choice).
pub(super) fn write_config(
    url: &str,
    branch: &str,
    mode: Option<SyncMode>,
    backups: Option<&BackupConfig>,
) -> Result<()> {
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
    match backups {
        Some(BackupConfig {
            encrypt: true,
            recipients,
        }) => {
            origin.insert("encrypt_backups", Item::Value(Value::from(true)));
            let mut array = toml_edit::Array::new();
            for recipient in recipients {
                array.push(recipient.as_str());
            }
            origin.insert("recipients", Item::Value(Value::Array(array)));
        }
        Some(BackupConfig { encrypt: false, .. }) => {
            origin.remove("encrypt_backups");
            origin.remove("recipients");
        }
        None => {}
    }
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

/// Forgets the previous repository: its per-path state, its publication
/// ref, and the machine refs fetched from it.
fn reset_sync_state(
    repo: &crate::system::history::shadow::HistoryRepo,
    machine_refs: &[(String, String)],
) -> Result<()> {
    for name in [super::state::STATE_REF, PUBLISH_REF] {
        if repo.ref_oid(name)?.is_some() {
            repo.delete_ref(name)?;
        }
    }
    for (name, _) in machine_refs {
        if repo.ref_oid(name)?.is_some() {
            repo.delete_ref(name)?;
        }
    }
    // and the decrypted copies of the previous repository's refs
    for (name, _) in repo.list_refs(PLAIN_PREFIX)? {
        repo.delete_ref(&name)?;
    }
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

/// Deletes this machine's recovery refs from the origin, then disconnects.
pub(crate) fn purge(store: &Store, yes: bool) -> Result<()> {
    let _sync_lock = run::lock(store)?;
    let state_dir = store.state_dir();
    let mut status = run::read_status(state_dir)?;
    let origin = run::origin()?;
    let repo = store
        .repo()
        .ok_or_else(|| eyre::eyre!("purging requires git"))?;
    let machine = store.machine();
    let remote = Remote::new(repo, &origin.url);
    let refs = backup::remote_refs(&remote, &machine.id)?;
    miseprintln!(
        "This deletes {} recovery ref(s) of {} ({}) from {}. Objects may persist until the host runs gc; setup commits are never deleted; forks, clones, and host backups may keep content. This is not erasure.",
        refs.len(),
        machine.name,
        machine.id,
        origin.url
    );
    if !confirmed(yes, "Purge this machine's recovery refs?")? {
        bail!("not purged");
    }
    remote.delete(&refs)?;
    status.uploaded.clear();
    run::write_status(state_dir, &status)?;
    info!("history: deleted {} ref(s) from {}", refs.len(), origin.url);
    remove_locked(state_dir, &mut status)
}
