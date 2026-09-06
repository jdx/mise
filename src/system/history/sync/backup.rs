//! Machine recovery refs: this machine's checkpoints pushed as
//! parentless wrapper commits to `refs/mise-history/<machine-id>/<uuid>`
//! on the origin, rebuilt with every policy-excluded and private path
//! removed and the record masked. Journal blobs never travel: journals
//! are data on another machine, never executed. Retention removes only
//! this machine's remote refs for checkpoints it pruned.
//!
//! With `[history.origin] encrypt_backups = true` the wrapper is the
//! encrypted layout (`encrypted`): one age payload per checkpoint for the
//! declared recipients. The scheme in use (plaintext, or which recipients)
//! is recorded in `sync.json`; when it changes, this machine's refs on the
//! origin are replaced in one transaction with eligible checkpoints under
//! the new scheme, so a repository never holds a mix by accident.

use std::collections::{BTreeMap, BTreeSet};

use eyre::{Result, bail};

use super::encrypted::{self, BackupHeader};
use super::network::{MACHINES_PREFIX, PushOutcome, REMOTE_MACHINES_PREFIX, Remote};
use super::privacy::mask_checkpoint;
use super::run::SyncStatus;
use crate::system::history::config::OriginTomlConfig;
use crate::system::history::shadow::{HistoryRepo, Overlay};
use crate::system::history::store::{Entry, OperationStatus};
use crate::system::history::tracked::display_to_tree_path;

/// The recorded scheme of a plaintext connection (and of every connection
/// recorded before schemes existed).
pub(crate) const PLAIN_SCHEME: &str = "plain";

/// The recipients this machine's backups are encrypted for.
pub(crate) struct BackupEncryption {
    pub recipients: Vec<Box<dyn age::Recipient + Send>>,
    /// The same recipients as declared, for the scheme fingerprint.
    pub strings: Vec<String>,
}

impl std::fmt::Debug for BackupEncryption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BackupEncryption")
            .field("recipients", &self.strings)
            .finish()
    }
}

impl BackupEncryption {
    /// `None` for a plaintext connection. An error when encryption is on
    /// but cannot be done (no recipients, or one that does not parse):
    /// the caller skips uploads rather than falling back to plaintext.
    pub(crate) fn resolve(origin: &OriginTomlConfig) -> Result<Option<Self>> {
        if !origin.encrypt_backups {
            return Ok(None);
        }
        if origin.recipients.is_empty() {
            bail!(
                "encrypted backups are on but [history.origin] names no recipients; `mise bootstrap dotfiles origin set {} --encrypt-backups [--recipient …]` adds them (nothing is uploaded until then)",
                origin.url
            );
        }
        let mut recipients = vec![];
        let mut strings = vec![];
        for declared in &origin.recipients {
            match crate::agecrypt::parse_recipient(declared)? {
                Some(recipient) => {
                    recipients.push(recipient);
                    strings.push(declared.trim().to_string());
                }
                None => bail!(
                    "[history.origin] recipient {declared:?} is neither an age public key (age1…) nor an SSH public key (ssh-…)"
                ),
            }
        }
        Ok(Some(Self {
            recipients,
            strings,
        }))
    }
}

/// A fingerprint of how backups are written: `plain`, or `age:` plus a
/// hash of the recipient set (order and repeats do not matter).
pub(crate) fn scheme(encryption: Option<&BackupEncryption>) -> String {
    match encryption {
        None => PLAIN_SCHEME.to_string(),
        Some(encryption) => {
            let mut recipients: Vec<&str> = encryption.strings.iter().map(String::as_str).collect();
            recipients.sort_unstable();
            recipients.dedup();
            format!(
                "age:{}",
                crate::hash::hash_sha256_to_str(&recipients.join("\n"))
            )
        }
    }
}

/// Whether `scheme` differs from the one the refs on the origin were
/// written under (a status without one is plaintext: the only scheme
/// there was).
pub(crate) fn scheme_changes(status: &SyncStatus, scheme: &str) -> bool {
    status.backup_scheme.as_deref().unwrap_or(PLAIN_SCHEME) != scheme
}

/// Records `scheme`; when it changed, forgets what was uploaded so every
/// eligible checkpoint is uploaded again. Returns whether it changed.
#[cfg(test)]
pub(crate) fn reconcile_scheme(status: &mut SyncStatus, scheme: &str) -> bool {
    if !scheme_changes(status, scheme) {
        status
            .backup_scheme
            .get_or_insert_with(|| scheme.to_string());
        return false;
    }
    status.uploaded.clear();
    status.backup_scheme = Some(scheme.to_string());
    true
}

/// Whether a checkpoint may leave the machine at all: it has content and
/// at least one entry with `backup = true`.
pub(crate) fn eligible(entry: &Entry) -> bool {
    entry.checkpoint.tree.available
        && entry.checkpoint.status() != Some(OperationStatus::Pending)
        && entry
            .checkpoint
            .tree
            .coverage
            .entries
            .iter()
            .any(|coverage| coverage.backup && coverage.mode != "private")
}

/// The paths a backup must not carry.
fn excluded_paths(entry: &Entry) -> BTreeSet<String> {
    entry
        .checkpoint
        .tree
        .coverage
        .entries
        .iter()
        .filter(|coverage| !coverage.backup || coverage.mode == "private")
        .map(|coverage| coverage.path.clone())
        .collect()
}

/// A wrapper commit holding only what may be backed up: the plaintext
/// layout, or the encrypted one for `encryption`.
pub(crate) fn wrapper_commit(
    repo: &HistoryRepo,
    entry: &Entry,
    encryption: Option<&BackupEncryption>,
    encrypted_paths: &BTreeSet<String>,
) -> Result<String> {
    let mut excluded = excluded_paths(entry);
    if encryption.is_none() {
        excluded.extend(encrypted_paths.iter().cloned());
        excluded.extend(
            entry
                .checkpoint
                .tree
                .coverage
                .entries
                .iter()
                .filter(|c| c.encrypt)
                .map(|c| c.path.clone()),
        );
    }
    let snapshot = match &entry.checkpoint.tree.snapshot {
        Some(snapshot) if excluded.is_empty() => Some(snapshot.clone()),
        Some(snapshot) => {
            let overlays: Vec<Overlay> = excluded
                .iter()
                .map(|path| Overlay {
                    path: display_to_tree_path(path),
                    object: None,
                })
                .collect();
            Some(repo.compose(snapshot, &overlays)?)
        }
        None => None,
    };
    let mut masked = mask_checkpoint(&entry.checkpoint, &excluded);
    masked.tree.snapshot = snapshot.clone();
    if encryption.is_none()
        && (!encrypted_paths.is_empty()
            || entry
                .checkpoint
                .tree
                .coverage
                .entries
                .iter()
                .any(|coverage| coverage.encrypt))
    {
        // Descriptions, labels, and operation notes can contain text derived
        // from protected files. Plaintext backups must not disclose it.
        masked.description = "checkpoint (encrypted files omitted)".into();
        masked.summary = masked.description.clone();
        masked.labels.clear();
        masked.task = None;
        masked.operation = None;
    }
    match encryption {
        // a commit of its own: the local checkpoint ref keeps naming the
        // full wrapper, private and unbacked files included
        None => repo.write_checkpoint_commit(snapshot.as_deref(), &masked, &BTreeMap::new()),
        Some(encryption) => {
            let ciphertext =
                encrypted::build(repo, snapshot.as_deref(), &masked, &encryption.recipients)?;
            encrypted::write_commit(
                repo,
                &BackupHeader::new(&masked, encryption.recipients.len()),
                &ciphertext,
            )
        }
    }
}

pub(crate) fn remote_ref(machine_id: &str, uuid: &str) -> String {
    format!("{REMOTE_MACHINES_PREFIX}{machine_id}/{uuid}")
}

/// The mirrored remote ref of one of this machine's checkpoints, as the
/// sync's fetch left it: present means the origin holds the backup, whatever
/// the local record remembers.
fn mirrored(repo: &HistoryRepo, machine_id: &str, uuid: &str) -> Result<Option<String>> {
    repo.ref_oid(&format!("{MACHINES_PREFIX}{machine_id}/{uuid}"))
}

/// Uploads every eligible checkpoint the origin does not hold yet. Returns
/// how many were pushed. The origin's refs decide, not only the local record
/// (a lost `sync.json` must not re-push, and a backup already there is never
/// replaced): a checkpoint mirrored by the fetch is recorded as uploaded, and
/// one the origin refuses (a differing backup under the same name) is left as
/// it is on the origin and reported, while the others go on.
pub(crate) fn upload(
    remote: &Remote<'_>,
    repo: &HistoryRepo,
    entries: &[Entry],
    machine_id: &str,
    uploaded: &mut BTreeSet<String>,
    encryption: Option<&BackupEncryption>,
    encrypted_paths: &BTreeSet<String>,
) -> Result<usize> {
    let mut count = 0;
    for entry in entries {
        let uuid = &entry.checkpoint.uuid;
        if uploaded.contains(uuid) || !eligible(entry) {
            continue;
        }
        if mirrored(repo, machine_id, uuid)?.is_some() {
            uploaded.insert(uuid.clone());
            continue;
        }
        let commit = wrapper_commit(repo, entry, encryption, encrypted_paths)?;
        // Scheme changes use `replace`'s complete transaction. Ordinary
        // uploads must not overwrite a backup published since our fetch.
        let refspec = format!("{commit}:{}", remote_ref(machine_id, uuid));
        match remote.push(&[refspec], None)? {
            PushOutcome::Done => {
                uploaded.insert(uuid.clone());
                count += 1;
            }
            PushOutcome::Rejected(reason) => {
                warn!(
                    "history sync: the origin already holds a different backup of checkpoint {uuid}; keeping the origin's ({reason})"
                );
            }
        }
    }
    Ok(count)
}

/// Prepare every replacement before touching the remote. Status is committed
/// only after the atomic push, so a crash can safely repeat the transaction.
pub(crate) fn replace(
    remote: &Remote<'_>,
    repo: &HistoryRepo,
    entries: &[Entry],
    machine_id: &str,
    status: &mut SyncStatus,
    encryption: Option<&BackupEncryption>,
    encrypted_paths: &BTreeSet<String>,
) -> Result<(usize, usize)> {
    let old_refs = remote_refs(remote, machine_id)?;
    let old: BTreeSet<&str> = old_refs.iter().map(String::as_str).collect();
    let mut refs = BTreeSet::new();
    let mut uploaded = BTreeSet::new();
    let mut refspecs = Vec::new();
    for entry in entries.iter().filter(|entry| eligible(entry)) {
        let uuid = &entry.checkpoint.uuid;
        let name = remote_ref(machine_id, uuid);
        let newly_eligible = status
            .upload_since
            .as_deref()
            .is_none_or(|since| entry.checkpoint.created_at.as_str() >= since);
        if !newly_eligible && !status.uploaded.contains(uuid) && !old.contains(name.as_str()) {
            continue;
        }
        let commit = wrapper_commit(repo, entry, encryption, encrypted_paths)?;
        refspecs.push(format!("+{commit}:{name}"));
        refs.insert(name);
        uploaded.insert(uuid.clone());
    }
    refspecs.extend(
        old_refs
            .iter()
            .filter(|name| !refs.contains(*name))
            .map(|name| format!(":{name}")),
    );
    remote.push_atomic(&refspecs)?;
    status.backup_scheme = Some(scheme(encryption));
    status.uploaded = uploaded;
    Ok((status.uploaded.len(), old_refs.len()))
}

/// Removes this machine's remote refs for checkpoints no longer retained
/// locally. Returns how many were deleted.
pub(crate) fn prune_remote(
    remote: &Remote<'_>,
    entries: &[Entry],
    machine_id: &str,
    uploaded: &mut BTreeSet<String>,
) -> Result<usize> {
    let retained: BTreeSet<&str> = entries
        .iter()
        .map(|entry| entry.checkpoint.uuid.as_str())
        .collect();
    let prefix = format!("{REMOTE_MACHINES_PREFIX}{machine_id}/");
    let stale: Vec<String> = remote
        .ls_remote()?
        .into_iter()
        .filter_map(|(_, name)| {
            let uuid = name.strip_prefix(&prefix)?;
            (!retained.contains(uuid)).then(|| name.clone())
        })
        .collect();
    remote.delete(&stale)?;
    for name in &stale {
        if let Some(uuid) = name.strip_prefix(&prefix) {
            uploaded.remove(uuid);
        }
    }
    Ok(stale.len())
}

/// Every one of this machine's remote refs, for `origin --purge` and for
/// replacing them after a scheme change.
pub(crate) fn remote_refs(remote: &Remote<'_>, machine_id: &str) -> Result<Vec<String>> {
    let prefix = format!("{REMOTE_MACHINES_PREFIX}{machine_id}/");
    Ok(remote
        .ls_remote()?
        .into_iter()
        .filter(|(_, name)| name.starts_with(&prefix))
        .map(|(_, name)| name)
        .collect())
}

#[cfg(test)]
mod upload_tests {
    use std::process::Command;

    use super::*;
    use crate::system::history::checkpoint::test_checkpoint;
    use crate::system::history::store::CoverageEntry;

    fn repo(tmp: &std::path::Path) -> HistoryRepo {
        HistoryRepo::open_or_init_in(&tmp.join("state"))
            .unwrap()
            .expect("git is required for these tests")
    }

    fn origin(tmp: &std::path::Path) -> String {
        let path = tmp.join("origin.git");
        let status = Command::new("git")
            .args(["init", "-q", "--bare", "-b", "main"])
            .arg(&path)
            .status()
            .unwrap();
        assert!(status.success());
        format!("file://{}", path.display())
    }

    fn origin_refs(url: &str) -> Vec<String> {
        let output = Command::new("git")
            .args(["ls-remote", "--quiet", url])
            .output()
            .unwrap();
        String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .filter_map(|line| line.split('\t').nth(1).map(str::to_string))
            .collect()
    }

    /// An eligible checkpoint: content, and one entry with `backup = true`.
    fn entry(repo: &HistoryRepo, uuid: &str) -> Entry {
        let blob = repo.hash_blob(b"alias ll='ls -l'\n").unwrap();
        let tree = repo
            .mktree(&format!("100644 blob {blob}\t.zshrc\n"))
            .unwrap();
        let mut checkpoint = test_checkpoint(uuid, Some(&tree));
        checkpoint.tree.coverage.entries.push(CoverageEntry {
            path: "~/.zshrc".into(),
            mode: "track".into(),
            variant: None,
            source: None,
            autosave: true,
            share: true,
            backup: true,
            encrypt: false,
            state: "live".into(),
            promotion: None,
            private: None,
            declared_in: None,
        });
        let commit = repo
            .write_checkpoint_commit(Some(&tree), &checkpoint, &BTreeMap::new())
            .unwrap();
        Entry {
            id: 1,
            commit,
            checkpoint,
        }
    }

    #[test]
    fn a_backup_the_fetch_mirrored_is_recorded_not_pushed() {
        if crate::git::plumbing_binary().is_none() {
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let repo = repo(tmp.path());
        let url = origin(tmp.path());
        let remote = Remote::new(&repo, &url);
        let entry = entry(&repo, "u1");
        repo.update_ref(&format!("{MACHINES_PREFIX}m/u1"), &entry.commit, None)
            .unwrap();
        let mut uploaded = BTreeSet::new();
        assert_eq!(
            upload(
                &remote,
                &repo,
                &[entry],
                "m",
                &mut uploaded,
                None,
                &BTreeSet::new()
            )
            .unwrap(),
            0
        );
        assert_eq!(uploaded, BTreeSet::from(["u1".to_string()]));
        assert!(origin_refs(&url).is_empty());
    }

    #[test]
    fn a_forgotten_upload_is_found_on_the_origin_instead_of_pushed_again() {
        if crate::git::plumbing_binary().is_none() {
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let repo = repo(tmp.path());
        let url = origin(tmp.path());
        let remote = Remote::new(&repo, &url);
        let entry = entry(&repo, "u1");
        let mut uploaded = BTreeSet::new();
        assert_eq!(
            upload(
                &remote,
                &repo,
                std::slice::from_ref(&entry),
                "m",
                &mut uploaded,
                None,
                &BTreeSet::new()
            )
            .unwrap(),
            1
        );
        assert_eq!(origin_refs(&url), vec![remote_ref("m", "u1")]);
        // the local record is lost; the next sync's fetch mirrors the ref
        uploaded.clear();
        remote.fetch("main").unwrap();
        assert_eq!(
            upload(
                &remote,
                &repo,
                &[entry],
                "m",
                &mut uploaded,
                None,
                &BTreeSet::new()
            )
            .unwrap(),
            0
        );
        assert_eq!(uploaded, BTreeSet::from(["u1".to_string()]));
        assert_eq!(origin_refs(&url), vec![remote_ref("m", "u1")]);
    }

    #[test]
    fn a_differing_backup_on_the_origin_is_kept() {
        if crate::git::plumbing_binary().is_none() {
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let repo = repo(tmp.path());
        let url = origin(tmp.path());
        let remote = Remote::new(&repo, &url);
        // another build of the same checkpoint is on the origin already
        let other = entry(&repo, "u1");
        let refspec = format!("{}:{}", other.commit, remote_ref("m", "u1"));
        assert_eq!(remote.push(&[refspec], None).unwrap(), PushOutcome::Done);
        let mut entry = entry(&repo, "u1");
        entry.checkpoint.description = "rebuilt".into();
        let mut uploaded = BTreeSet::new();
        assert_eq!(
            upload(
                &remote,
                &repo,
                std::slice::from_ref(&entry),
                "m",
                &mut uploaded,
                None,
                &BTreeSet::new()
            )
            .unwrap(),
            0
        );
        assert!(uploaded.is_empty());
        let output = Command::new("git")
            .args(["ls-remote", "--quiet", &url, &remote_ref("m", "u1")])
            .output()
            .unwrap();
        assert!(String::from_utf8_lossy(&output.stdout).starts_with(&other.commit));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encryption(recipients: &[&str]) -> BackupEncryption {
        BackupEncryption {
            recipients: vec![],
            strings: recipients.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn origin(encrypt: bool, recipients: &[&str]) -> OriginTomlConfig {
        OriginTomlConfig {
            url: "file:///origin.git".into(),
            branch: "main".into(),
            encrypt_backups: encrypt,
            recipients: recipients.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn scheme_ignores_order_and_repeats() {
        assert_eq!(scheme(None), PLAIN_SCHEME);
        let a = scheme(Some(&encryption(&["age1a", "age1b"])));
        let b = scheme(Some(&encryption(&["age1b", "age1a", "age1a"])));
        assert_eq!(a, b);
        assert!(a.starts_with("age:"), "{a}");
        assert_ne!(a, scheme(Some(&encryption(&["age1a"]))));
    }

    #[test]
    fn reconcile_scheme_forgets_uploads_only_on_a_change() {
        let mut status = SyncStatus::default();
        status.uploaded.insert("u1".into());
        // an old status without a scheme is plaintext: no change, no re-upload
        assert!(!reconcile_scheme(&mut status, PLAIN_SCHEME));
        assert_eq!(status.uploaded.len(), 1);
        assert_eq!(status.backup_scheme.as_deref(), Some(PLAIN_SCHEME));

        let age = scheme(Some(&encryption(&["age1a"])));
        assert!(reconcile_scheme(&mut status, &age));
        assert!(status.uploaded.is_empty());
        assert_eq!(status.backup_scheme.as_deref(), Some(age.as_str()));

        status.uploaded.insert("u1".into());
        assert!(!reconcile_scheme(&mut status, &age));
        assert_eq!(status.uploaded.len(), 1);

        assert!(reconcile_scheme(&mut status, PLAIN_SCHEME));
        assert!(status.uploaded.is_empty());
    }

    #[test]
    fn resolve_refuses_encryption_without_usable_recipients() {
        assert!(
            BackupEncryption::resolve(&origin(false, &[]))
                .unwrap()
                .is_none()
        );
        let err = BackupEncryption::resolve(&origin(true, &[]))
            .unwrap_err()
            .to_string();
        assert!(err.contains("names no recipients"), "{err}");
        assert!(!err.contains("experimental"), "{err}");
        let err = BackupEncryption::resolve(&origin(true, &["not-a-key"]))
            .unwrap_err()
            .to_string();
        assert!(err.contains("neither an age public key"), "{err}");
        let resolved = BackupEncryption::resolve(&origin(
            true,
            &["age1ql3z7hjy54pw3hyww5ayyfg7zqgvc7w3j2elw8zmrj2kg5sfn9aqmcac8p"],
        ))
        .unwrap()
        .unwrap();
        assert_eq!(resolved.recipients.len(), 1);
    }
}
