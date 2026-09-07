//! The encrypted form of a machine recovery ref. A plaintext backup commit
//! carries `meta.json` and `snapshot/`; an encrypted one carries
//! `backup.toml` (a small plaintext header: format, machine, checkpoint id
//! and time, recipient count) and `payload.age`: the masked record and every
//! backed-up file, serialized together, zstd-compressed, and age-encrypted
//! for the machine's recipients. File names, descriptions, and content are
//! all inside the payload. A mise that predates this layout finds no
//! `meta.json` and skips the ref.
//!
//! Reading one back decrypts the payload once and writes its files as
//! ordinary git objects under a local plaintext wrapper commit (kept alive
//! by `refs/machines-plain/…`), so rollback treats the entry like any other.

use std::collections::BTreeMap;

use eyre::{Result, bail};
use serde::{Deserialize, Serialize};

use crate::agecrypt::{self, DecryptError};
use crate::system::history::shadow::HistoryRepo;
use crate::system::history::store::{Checkpoint, Machine};

/// The plaintext layout is implicitly format 1.
pub(crate) const BACKUP_FORMAT: u32 = 2;
pub(crate) const HEADER_PATH: &str = "backup.toml";
pub(crate) const PAYLOAD_PATH: &str = "payload.age";
const ENCRYPTION: &str = "age";

/// What an encrypted backup says about itself without a key.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct BackupHeader {
    pub format: u32,
    pub encryption: String,
    pub machine: Machine,
    /// The checkpoint uuid.
    pub checkpoint: String,
    /// RFC 3339, UTC.
    pub created_at: String,
    /// How many recipients can read the payload (not who).
    pub recipients: usize,
}

impl BackupHeader {
    pub(crate) fn new(checkpoint: &Checkpoint, recipients: usize) -> Self {
        Self {
            format: BACKUP_FORMAT,
            encryption: ENCRYPTION.to_string(),
            machine: checkpoint.machine.clone(),
            checkpoint: checkpoint.uuid.clone(),
            created_at: checkpoint.created_at.clone(),
            recipients,
        }
    }

    pub(crate) fn to_toml(&self) -> Result<String> {
        Ok(format!(
            "# An encrypted mise machine backup; the files are in payload.age.\n{}",
            toml::to_string(self)?
        ))
    }

    pub(crate) fn parse(bytes: &[u8]) -> Result<Self> {
        let text = std::str::from_utf8(bytes)?;
        let header: Self = toml::from_str(text)?;
        if header.format != BACKUP_FORMAT {
            bail!(
                "backup format {} is newer than this mise understands (up to {BACKUP_FORMAT}); upgrade mise",
                header.format
            );
        }
        if header.encryption != ENCRYPTION {
            bail!(
                "backup encryption {:?} is not one this mise understands",
                header.encryption
            );
        }
        Ok(header)
    }
}

/// Everything inside the payload.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Payload {
    pub format: u32,
    /// The masked record; `tree.snapshot` is meaningless off the machine
    /// and left empty.
    pub checkpoint: Checkpoint,
    #[serde(deserialize_with = "bounded_files")]
    pub files: Vec<PayloadFile>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct PayloadFile {
    /// The snapshot tree path (`home/.zshrc`, `fs/etc/hosts`).
    pub path: String,
    /// The git mode: `100644`, `100755`, `120000` (content is the link
    /// target), or `160000` (content is the recorded commit id).
    pub mode: String,
    pub content: Bytes,
}

/// Raw bytes serialized as a MessagePack `bin`, whatever the serializer's
/// default for `Vec<u8>` is.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Bytes(pub Vec<u8>);

impl Serialize for Bytes {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_bytes(&self.0)
    }
}

impl<'de> Deserialize<'de> for Bytes {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = Bytes;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("bytes")
            }
            fn visit_bytes<E: serde::de::Error>(self, v: &[u8]) -> Result<Bytes, E> {
                Ok(Bytes(v.to_vec()))
            }
            fn visit_byte_buf<E: serde::de::Error>(self, v: Vec<u8>) -> Result<Bytes, E> {
                Ok(Bytes(v))
            }
        }
        deserializer.deserialize_byte_buf(Visitor)
    }
}

const MAX_FILES: usize = 100_000;

fn bounded_files<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<PayloadFile>, D::Error> {
    struct Visitor;
    impl<'de> serde::de::Visitor<'de> for Visitor {
        type Value = Vec<PayloadFile>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a bounded file list")
        }
        fn visit_seq<A: serde::de::SeqAccess<'de>>(
            self,
            mut seq: A,
        ) -> Result<Self::Value, A::Error> {
            if seq.size_hint().is_some_and(|n| n > MAX_FILES) {
                return Err(serde::de::Error::custom("too many backup files"));
            }
            let mut files = Vec::new();
            while let Some(file) = seq.next_element()? {
                if files.len() == MAX_FILES {
                    return Err(serde::de::Error::custom("too many backup files"));
                }
                files.push(file);
            }
            Ok(files)
        }
    }
    deserializer.deserialize_seq(Visitor)
}

fn validate_files(payload: &Payload) -> Result<()> {
    if payload.files.len() > MAX_FILES {
        bail!("too many backup files");
    }
    let mut paths = std::collections::BTreeSet::new();
    for file in &payload.files {
        if !super::layout::is_safe_branch_path(&file.path)
            || !matches!(
                file.mode.as_str(),
                "100644" | "100755" | "120000" | "160000"
            )
            || !paths.insert(file.path.as_str())
        {
            bail!("invalid or duplicate backup file path/mode");
        }
        if file.mode == "160000"
            && !(matches!(file.content.0.len(), 40 | 64)
                && file.content.0.iter().all(u8::is_ascii_hexdigit))
        {
            bail!("invalid gitlink object id");
        }
    }
    for path in &paths {
        for (index, _) in path.match_indices('/') {
            if paths.contains(&path[..index]) {
                bail!("overlapping backup file paths");
            }
        }
    }
    Ok(())
}

pub(crate) fn validate_identity(
    repo: &HistoryRepo,
    commit: &str,
    machine: &str,
    uuid: &str,
    checkpoint: &Checkpoint,
) -> Result<()> {
    let header =
        header_of(repo, commit)?.ok_or_else(|| eyre::eyre!("missing encrypted backup header"))?;
    if header.machine.id != machine
        || header.checkpoint != uuid
        || header.machine != checkpoint.machine
        || header.checkpoint != checkpoint.uuid
        || header.created_at != checkpoint.created_at
    {
        bail!("encrypted backup identity does not match its header and ref");
    }
    Ok(())
}

pub(crate) async fn materialize_checked(
    repo: &HistoryRepo,
    commit: &str,
    machine: &str,
    uuid: &str,
) -> Result<String, ReadError> {
    let payload = read_payload(repo, commit).await?;
    validate_identity(repo, commit, machine, uuid, &payload.checkpoint)?;
    Ok(materialize_payload(repo, &payload)?)
}

/// Why an encrypted backup could not be read.
#[derive(Debug)]
pub(crate) enum ReadError {
    /// The commit has the plaintext layout.
    NotEncrypted,
    /// Not an age payload, or damaged.
    Corrupt(String),
    /// No identity here can decrypt it.
    Decrypt(DecryptError),
    Other(eyre::Report),
}

impl From<eyre::Report> for ReadError {
    fn from(err: eyre::Report) -> Self {
        Self::Other(err)
    }
}

impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotEncrypted => write!(f, "the backup is not encrypted"),
            Self::Corrupt(reason) => write!(f, "the encrypted backup is damaged: {reason}"),
            Self::Decrypt(err) => write!(f, "{err}"),
            Self::Other(err) => write!(f, "{err:#}"),
        }
    }
}

/// The encrypted payload of a filtered snapshot: every file under
/// `snapshot` (none when there is no snapshot) plus the masked record.
pub(crate) fn build(
    repo: &HistoryRepo,
    snapshot: Option<&str>,
    checkpoint: &Checkpoint,
    recipients: &[Box<dyn age::Recipient + Send>],
) -> Result<Vec<u8>> {
    let mut files = vec![];
    let mut remaining = agecrypt::MAX_PLAINTEXT_BYTES;
    if let Some(snapshot) = snapshot {
        for entry in repo.ls_tree(snapshot)? {
            if files.len() == MAX_FILES {
                bail!("too many backup files");
            }
            let content = if entry.mode == "160000" {
                entry.oid.clone().into_bytes()
            } else {
                repo.cat_object_bounded(&entry.oid, remaining)?
            };
            remaining = remaining
                .checked_sub(content.len() as u64)
                .ok_or_else(|| eyre::eyre!("backup exceeds size limit"))?;
            files.push(PayloadFile {
                path: entry.path,
                mode: entry.mode,
                content: Bytes(content),
            });
        }
    }
    let mut checkpoint = checkpoint.clone();
    checkpoint.tree.snapshot = None;
    let payload = Payload {
        format: BACKUP_FORMAT,
        checkpoint,
        files,
    };
    validate_files(&payload)?;
    let serialized = rmp_serde::to_vec_named(&payload)?;
    agecrypt::encrypt_bytes(&serialized, recipients)
}

/// The wrapper commit for an encrypted backup: `backup.toml` and
/// `payload.age`, parentless, with a message that names only the
/// checkpoint (the description is inside the payload).
pub(crate) fn write_commit(
    repo: &HistoryRepo,
    header: &BackupHeader,
    ciphertext: &[u8],
) -> Result<String> {
    let header_oid = repo.hash_blob(header.to_toml()?.as_bytes())?;
    let payload_oid = repo.hash_blob(ciphertext)?;
    let listing = format!(
        "100644 blob {header_oid}\t{HEADER_PATH}\n100644 blob {payload_oid}\t{PAYLOAD_PATH}\n"
    );
    let tree = repo.mktree(&listing)?;
    repo.commit_tree(
        &tree,
        vec![],
        &format!("mise encrypted backup {}", header.checkpoint),
    )
}

/// The header when `commit` has the encrypted layout; `None` for a
/// plaintext backup.
pub(crate) fn header_of(repo: &HistoryRepo, commit: &str) -> Result<Option<BackupHeader>> {
    let Some((_, oid)) = repo.object_at(commit, HEADER_PATH)? else {
        return Ok(None);
    };
    let bytes = repo.cat_object_bounded(&oid, 64 * 1024)?;
    Ok(Some(BackupHeader::parse(&bytes)?))
}

/// Decrypts the payload of `commit` with this machine's identities.
pub(crate) async fn read_payload(repo: &HistoryRepo, commit: &str) -> Result<Payload, ReadError> {
    let Some((_, oid)) = repo.object_at(commit, PAYLOAD_PATH)? else {
        return Err(ReadError::NotEncrypted);
    };
    let ciphertext = repo.cat_object_bounded(&oid, agecrypt::MAX_ENCRYPTED_BYTES)?;
    let serialized = agecrypt::decrypt_bytes(&ciphertext)
        .await
        .map_err(|err| match err {
            DecryptError::Corrupt(reason) => ReadError::Corrupt(reason),
            other => ReadError::Decrypt(other),
        })?;
    let payload: Payload = rmp_serde::from_slice(&serialized)
        .map_err(|err| ReadError::Corrupt(format!("unreadable payload: {err}")))?;
    if payload.format != BACKUP_FORMAT {
        return Err(ReadError::Corrupt(format!(
            "payload format {} is newer than this mise understands",
            payload.format
        )));
    }
    validate_files(&payload)?;
    Ok(payload)
}

/// Writes a decrypted payload as a local plaintext wrapper commit
/// (`meta.json` + `snapshot/`), indistinguishable from a plaintext backup.
pub(crate) fn materialize_payload(repo: &HistoryRepo, payload: &Payload) -> Result<String> {
    validate_files(payload)?;
    let mut entries = vec![];
    for file in &payload.files {
        let oid = if file.mode == "160000" {
            String::from_utf8(file.content.0.clone())?
        } else {
            repo.hash_blob(&file.content.0)?
        };
        entries.push((file.mode.clone(), oid, file.path.clone()));
    }
    let tree = repo.write_tree(&entries)?;
    let mut checkpoint = payload.checkpoint.clone();
    checkpoint.tree.snapshot = Some(tree.clone());
    repo.write_checkpoint_commit(Some(&tree), &checkpoint, &BTreeMap::new())
}

/// Decrypts `commit` and materializes it; the local plaintext wrapper.
#[cfg(test)]
pub(crate) async fn materialize(repo: &HistoryRepo, commit: &str) -> Result<String, ReadError> {
    let payload = read_payload(repo, commit).await?;
    Ok(materialize_payload(repo, &payload)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::system::history::checkpoint::test_checkpoint;

    fn recipients_for(key: &age::x25519::Identity) -> Vec<Box<dyn age::Recipient + Send>> {
        vec![Box::new(key.to_public())]
    }

    fn sample_snapshot(repo: &HistoryRepo) -> String {
        let zshrc = repo.hash_blob(b"export EDITOR=vim\n").unwrap();
        let script = repo.hash_blob(b"#!/bin/sh\necho hi\n").unwrap();
        let link = repo.hash_blob(b".zshrc").unwrap();
        repo.write_tree(&[
            ("100644".into(), zshrc, "home/.zshrc".into()),
            ("100755".into(), script, "home/bin/x".into()),
            ("120000".into(), link, "home/link".into()),
            (
                "160000".into(),
                "0123456789abcdef0123456789abcdef01234567".into(),
                "home/repo".into(),
            ),
        ])
        .unwrap()
    }

    fn listing(repo: &HistoryRepo, tree: &str) -> Vec<(String, String, Option<u64>, String)> {
        repo.ls_tree(tree)
            .unwrap()
            .into_iter()
            .map(|entry| (entry.mode, entry.oid, entry.size, entry.path))
            .collect()
    }

    #[test]
    fn header_round_trips_and_refuses_newer_formats() {
        let checkpoint = test_checkpoint("u1", None);
        let header = BackupHeader::new(&checkpoint, 2);
        let text = header.to_toml().unwrap();
        assert!(text.contains("encryption = \"age\""), "{text}");
        assert_eq!(BackupHeader::parse(text.as_bytes()).unwrap(), header);
        let newer = text.replace("format = 2", "format = 3");
        let err = BackupHeader::parse(newer.as_bytes())
            .unwrap_err()
            .to_string();
        assert!(err.contains("upgrade mise"), "{err}");
    }

    #[tokio::test]
    async fn payload_round_trips_every_mode_through_git() {
        if crate::git::plumbing_binary().is_none() {
            return;
        }
        use age::secrecy::ExposeSecret;
        let tmp = tempfile::tempdir().unwrap();
        let repo = HistoryRepo::open_or_init_in(tmp.path())
            .unwrap()
            .expect("git available");
        let snapshot = sample_snapshot(&repo);
        let mut checkpoint = test_checkpoint("u1", Some(&snapshot));
        checkpoint.description = "secret description".into();
        let key = age::x25519::Identity::generate();

        let ciphertext = build(&repo, Some(&snapshot), &checkpoint, &recipients_for(&key)).unwrap();
        let commit = write_commit(&repo, &BackupHeader::new(&checkpoint, 1), &ciphertext).unwrap();

        // the wrapper holds exactly the two files, and nothing readable
        let names: Vec<String> = repo
            .ls_tree(&commit)
            .unwrap()
            .into_iter()
            .map(|entry| entry.path)
            .collect();
        assert_eq!(names, vec![HEADER_PATH, PAYLOAD_PATH]);
        let payload_oid = repo.object_at(&commit, PAYLOAD_PATH).unwrap().unwrap().1;
        let raw = repo.cat_object(&payload_oid).unwrap();
        assert!(!raw.windows(6).any(|w| w == b"EDITOR"));
        assert!(!raw.windows(6).any(|w| w == b"secret"));
        assert!(
            repo.read_meta(&commit).is_err(),
            "no meta.json in the clear"
        );
        let header = header_of(&repo, &commit).unwrap().unwrap();
        assert_eq!(header.checkpoint, "u1");
        assert_eq!(header.recipients, 1);

        // without a key: refused, not silently empty
        let mut environment = crate::test::EnvVarGuard::new();
        environment.remove("MISE_AGE_KEY");
        let no_key = materialize(&repo, &commit).await;
        assert!(
            matches!(no_key, Err(ReadError::Decrypt(DecryptError::NoIdentities))),
            "{no_key:?}"
        );

        environment.set("MISE_AGE_KEY", key.to_string().expose_secret());
        let plain = materialize(&repo, &commit).await;
        environment.remove("MISE_AGE_KEY");
        let plain = plain.unwrap();
        let restored = repo.read_meta(&plain).unwrap();
        assert_eq!(restored.description, "secret description");
        let restored_snapshot = repo.object_at(&plain, "snapshot").unwrap().unwrap().1;
        assert_eq!(
            listing(&repo, &restored_snapshot),
            listing(&repo, &snapshot)
        );
        assert_eq!(
            restored_snapshot, snapshot,
            "identical content, identical tree"
        );
    }

    #[tokio::test]
    async fn a_tampered_payload_is_reported_as_damaged() {
        if crate::git::plumbing_binary().is_none() {
            return;
        }
        use age::secrecy::ExposeSecret;
        let tmp = tempfile::tempdir().unwrap();
        let repo = HistoryRepo::open_or_init_in(tmp.path())
            .unwrap()
            .expect("git available");
        let checkpoint = test_checkpoint("u2", None);
        let key = age::x25519::Identity::generate();
        let header = BackupHeader::new(&checkpoint, 1);
        let commit = write_commit(&repo, &header, b"this is not an age file").unwrap();
        let mut environment = crate::test::EnvVarGuard::new();
        environment.set("MISE_AGE_KEY", key.to_string().expose_secret());
        let result = read_payload(&repo, &commit).await;
        environment.remove("MISE_AGE_KEY");
        assert!(matches!(result, Err(ReadError::Corrupt(_))), "{result:?}");

        // a plaintext wrapper is not encrypted
        let plain = repo
            .write_checkpoint_commit(None, &checkpoint, &BTreeMap::new())
            .unwrap();
        assert!(header_of(&repo, &plain).unwrap().is_none());
        assert!(matches!(
            read_payload(&repo, &plain).await,
            Err(ReadError::NotEncrypted)
        ));
    }
}

#[cfg(test)]
mod validation_tests {
    use super::*;
    use crate::system::history::checkpoint::test_checkpoint;

    #[test]
    fn refuses_mismatched_header_and_ref_before_materializing() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = HistoryRepo::open_or_init_in(tmp.path()).unwrap().unwrap();
        let checkpoint = test_checkpoint("uuid", None);
        let header = BackupHeader::new(&checkpoint, 1);
        let commit = write_commit(&repo, &header, b"irrelevant").unwrap();
        assert!(
            validate_identity(&repo, &commit, &checkpoint.machine.id, "uuid", &checkpoint).is_ok()
        );
        assert!(validate_identity(&repo, &commit, "other", "uuid", &checkpoint).is_err());
        assert!(
            validate_identity(&repo, &commit, &checkpoint.machine.id, "other", &checkpoint)
                .is_err()
        );
        let mut changed = checkpoint.clone();
        changed.created_at = "2020-01-01T00:00:00Z".into();
        assert!(
            validate_identity(&repo, &commit, &checkpoint.machine.id, "uuid", &changed).is_err()
        );
    }

    #[test]
    fn gitlinks_accept_both_git_hash_formats() {
        for (oid, valid) in [
            ("a".repeat(40), true),
            ("b".repeat(64), true),
            ("a".repeat(41), false),
            ("z".repeat(40), false),
        ] {
            let payload = Payload {
                format: BACKUP_FORMAT,
                checkpoint: test_checkpoint("uuid", None),
                files: vec![PayloadFile {
                    path: "home/repo".into(),
                    mode: "160000".into(),
                    content: Bytes(oid.into_bytes()),
                }],
            };
            assert_eq!(validate_files(&payload).is_ok(), valid);
        }
    }

    #[test]
    fn rejects_unsafe_and_overlapping_paths_before_git_writes() {
        let mut payload = Payload {
            format: BACKUP_FORMAT,
            checkpoint: test_checkpoint("uuid", None),
            files: vec![],
        };
        for path in ["../outside", "home/.git/config", "home//file"] {
            payload.files = vec![PayloadFile {
                path: path.into(),
                mode: "100644".into(),
                content: Bytes(vec![]),
            }];
            assert!(validate_files(&payload).is_err());
        }
        payload.files = ["home/file", "home/file/child"]
            .into_iter()
            .map(|path| PayloadFile {
                path: path.into(),
                mode: "100644".into(),
                content: Bytes(vec![]),
            })
            .collect();
        assert!(validate_files(&payload).is_err());
    }
}
