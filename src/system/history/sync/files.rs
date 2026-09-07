//! Encryption before Git capture and process-local decryption for live files.
use std::collections::BTreeSet;

use eyre::{Result, WrapErr, bail};
use serde::{Deserialize, Serialize};

use super::{layout, reconcile::Object};
use crate::{agecrypt, system::history::shadow::HistoryRepo};

const MAGIC: &[u8] = b"mise-encrypted-file-v1\n";

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

/// Audit the complete proposed ancestry, not just the latest tree. A newly
/// encrypted path cannot make its previously committed plaintext safe to push.
pub(crate) fn audit_history(
    repo: &HistoryRepo,
    head: &str,
    protected: &BTreeSet<String>,
) -> Result<()> {
    let commits = repo.rev_list(head, usize::MAX)?;
    let mut protected = protected.clone();
    // Policy changes cannot hide older plaintext, including on a merge
    // parent or a platform that this machine never activates.
    for commit in &commits {
        if let Some(manifest) = crate::system::history::manifest::Manifest::read(repo, commit)? {
            protected.extend(manifest.encrypted_paths());
        }
    }
    if protected.is_empty() {
        return Ok(());
    }
    let mut checked = BTreeSet::new();
    for commit in commits {
        for entry in repo.ls_tree(&commit)? {
            if !protected.iter().any(|path| {
                entry.path == *path
                    || entry
                        .path
                        .strip_prefix(path)
                        .is_some_and(|rest| rest.starts_with('/'))
            }) || !checked.insert((entry.path.clone(), entry.mode.clone(), entry.oid.clone()))
            {
                continue;
            }
            let object = (entry.mode, entry.oid);
            let valid =
                envelope(repo, &object, agecrypt::MAX_ENCRYPTED_BYTES)?.is_some_and(|outer| {
                    outer.path == entry.path
                        && matches!(outer.mode.as_str(), "100644" | "100755" | "120000")
                        && outer.ciphertext.0.starts_with(b"age-encryption.org/v1\n")
                });
            if !valid {
                bail!(
                    "cannot publish: encrypted path {} has an unencrypted or invalid version in commit {commit}; encrypting the latest version does not erase earlier plaintext. Review and explicitly rewrite or replace that history before connecting it to origin; mise will not rewrite it automatically",
                    entry.path
                );
            }
        }
    }
    Ok(())
}

/// Resolve encrypted files from the same committed enrollment metadata.
pub(crate) fn encrypted_paths(
    repo: &HistoryRepo,
    commit: Option<&str>,
) -> Result<BTreeSet<String>> {
    let Some(commit) = commit else {
        return Ok(BTreeSet::new());
    };
    let Some(manifest) = crate::system::history::manifest::Manifest::read(repo, commit)? else {
        return Ok(BTreeSet::new());
    };
    let protected = manifest.encrypted_paths();
    Ok(repo
        .ls_tree(commit)?
        .into_iter()
        .filter(|entry| {
            protected.iter().any(|prefix| {
                entry.path == *prefix
                    || entry
                        .path
                        .strip_prefix(prefix)
                        .is_some_and(|rest| rest.starts_with('/'))
            })
        })
        .map(|entry| entry.path)
        .collect())
}

#[derive(Serialize, Deserialize)]
struct Envelope {
    path: String,
    mode: String,
    scheme: String,
    ciphertext: Bytes,
}

#[derive(Serialize, Deserialize)]
struct Plaintext {
    path: String,
    mode: String,
    scheme: String,
    content: Bytes,
}

fn control_file(path: &str) -> bool {
    if path.starts_with(".mise-history/") {
        return true;
    }
    let Some((root, path)) = path.split_once('/') else {
        return false;
    };
    if root.split('@').next() != Some("config") {
        return false;
    }
    (path.starts_with("conf.d/") && path.ends_with(".toml"))
        || (!path.contains('/')
            && path.ends_with(".toml")
            && (path.starts_with("config.") || path.starts_with("mise.")))
}

fn envelope(repo: &HistoryRepo, object: &Object, limit: u64) -> Result<Option<Envelope>> {
    // A gitlink names a commit in another repository, not a readable blob.
    if object.0 == "160000" {
        return Ok(None);
    }
    if !repo.blob_starts_with(&object.1, MAGIC)? {
        return Ok(None);
    }
    // Only encrypted envelopes have this limit; do not change plaintext sync.
    let bytes = repo.cat_object_bounded(&object.1, limit)?;
    let Some(body) = bytes.strip_prefix(MAGIC) else {
        return Ok(None);
    };
    Ok(Some(
        rmp_serde::from_slice(body).wrap_err("invalid encrypted file envelope")?,
    ))
}

fn validate(path: &str, outer: &Envelope, inner: &Plaintext) -> Result<()> {
    if outer.path != path
        || inner.path != path
        || inner.mode != outer.mode
        || inner.scheme != outer.scheme
        || !layout::is_safe_branch_path(path)
        || !matches!(inner.mode.as_str(), "100644" | "100755" | "120000")
    {
        bail!("encrypted file does not match its path or mode: {path}");
    }
    Ok(())
}

pub(crate) fn decrypt(
    repo: &HistoryRepo,
    path: &str,
    object: &Object,
    interactive: bool,
) -> Result<Object> {
    let outer = envelope(repo, object, agecrypt::MAX_ENCRYPTED_BYTES)?
        .ok_or_else(|| eyre::eyre!("missing encrypted file envelope: {path}"))?;
    if control_file(path) {
        bail!("setup configuration itself cannot be encrypted: {path}");
    }
    if outer.path != path {
        bail!("encrypted file does not match its path: {path}");
    }
    if let Some(decrypted) = repo.decrypted_object(&object.1) {
        return Ok(decrypted);
    }
    let bytes = agecrypt::decrypt_sync(&outer.ciphertext.0, interactive)
        .wrap_err_with(|| format!("cannot unlock {path}; run mise bootstrap dotfiles pull interactively with a matching age identity"))?;
    let inner: Plaintext =
        rmp_serde::from_slice(&bytes).wrap_err("invalid encrypted file payload")?;
    validate(path, &outer, &inner)?;
    let oid = repo.transient_blob_id(&inner.content.0)?;
    let decrypted = (inner.mode, oid);
    repo.remember_decrypted(&object.1, decrypted.clone());
    Ok(decrypted)
}

fn encrypt(
    repo: &HistoryRepo,
    path: &str,
    object: &Object,
    scheme: &str,
    recipients: &[Box<dyn age::Recipient + Send>],
) -> Result<Object> {
    let content = repo.cat_object_bounded(&object.1, agecrypt::MAX_PLAINTEXT_BYTES)?;
    let encoded = encode(path, &object.0, &content, scheme, recipients)?;
    let encrypted = ("100644".into(), repo.hash_blob(&encoded)?);
    repo.remember_decrypted(&encrypted.1, object.clone());
    Ok(encrypted)
}

/// Turn a reconciled process-local object into its committed representation.
/// Reuse an unchanged envelope; a merged plaintext is encrypted before hashing.
pub(super) fn commit_object(
    repo: &HistoryRepo,
    path: &str,
    object: &Object,
    manifest: &crate::system::history::manifest::Manifest,
    parents: &[&str],
    interactive: bool,
) -> Result<Object> {
    if !manifest.encrypted_paths().iter().any(|prefix| {
        path == prefix
            || path
                .strip_prefix(prefix)
                .is_some_and(|rest| rest.starts_with('/'))
    }) {
        return Ok(object.clone());
    }
    let mut strings = manifest.recipients.clone();
    strings.sort();
    strings.dedup();
    let scheme = crate::hash::hash_sha256_to_str(&strings.join("\n"));
    for parent in parents {
        if let Some(raw) = repo.object_at(parent, path)?
            && envelope(repo, &raw, agecrypt::MAX_ENCRYPTED_BYTES)?
                .is_some_and(|outer| outer.scheme == scheme)
            && decrypt(repo, path, &raw, interactive)? == *object
        {
            return Ok(raw);
        }
    }
    let recipients = strings
        .iter()
        .map(|recipient| {
            agecrypt::parse_recipient_mode(recipient, interactive)?
                .ok_or_else(|| eyre::eyre!("invalid age recipient: {recipient}"))
        })
        .collect::<Result<Vec<_>>>()?;
    encrypt(repo, path, object, &scheme, &recipients)
}

/// Encrypt bytes before they enter Git. Callers may store only the returned
/// envelope, never the input or the decrypted payload in repository objects.
pub(crate) fn encode(
    path: &str,
    mode: &str,
    content: &[u8],
    scheme: &str,
    recipients: &[Box<dyn age::Recipient + Send>],
) -> Result<Vec<u8>> {
    if control_file(path) {
        bail!("encrypt an external dotfile source instead of configuration: {path}");
    }
    if !matches!(mode, "100644" | "100755" | "120000") {
        bail!("unsupported encrypted file mode: {path}");
    }
    let inner = Plaintext {
        path: path.into(),
        mode: mode.into(),
        scheme: scheme.into(),
        content: Bytes(content.to_vec()),
    };
    let bytes = rmp_serde::to_vec_named(&inner)?;
    let outer = Envelope {
        path: path.into(),
        mode: mode.into(),
        scheme: scheme.into(),
        ciphertext: Bytes(agecrypt::encrypt_bytes(&bytes, recipients)?),
    };
    let mut encoded = MAGIC.to_vec();
    encoded.extend(rmp_serde::to_vec_named(&outer)?);
    if encoded.len() as u64 > agecrypt::MAX_ENCRYPTED_BYTES {
        bail!("encrypted file exceeds the size limit: {path}");
    }
    Ok(encoded)
}

#[cfg(test)]
mod tests {
    #[test]
    fn encrypted_tip_does_not_hide_plaintext_in_ancestry_or_merge_parents() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = HistoryRepo::open_or_init_in(tmp.path()).unwrap().unwrap();
        let key = age::x25519::Identity::generate();
        let recipients: Vec<Box<dyn age::Recipient + Send>> = vec![Box::new(key.to_public())];
        let path = "home/secret";
        let plain = repo.hash_blob(b"private").unwrap();
        let cipher = repo
            .hash_blob(&encode(path, "100644", b"private", "test", &recipients).unwrap())
            .unwrap();
        let empty = repo.empty_object("tree").unwrap();
        let make_tree = |oid: String| {
            repo.compose(
                &empty,
                &[crate::system::history::shadow::Overlay {
                    path: path.into(),
                    object: Some(("100644".into(), oid)),
                }],
            )
            .unwrap()
        };
        let plain_tree = make_tree(plain);
        let encrypted_tree = make_tree(cipher);
        let exposed = repo.commit_tree(&plain_tree, vec![], "plaintext").unwrap();
        let encrypted = repo
            .commit_tree(&encrypted_tree, vec![&exposed], "encrypted now")
            .unwrap();
        let protected = BTreeSet::from([path.into()]);
        assert!(audit_history(&repo, &encrypted, &protected).is_err());
        let clean = repo
            .commit_tree(&encrypted_tree, vec![], "encrypted from first commit")
            .unwrap();
        audit_history(&repo, &clean, &protected).unwrap();
        let merge = repo
            .commit_tree(&encrypted_tree, vec![&clean, &exposed], "merge")
            .unwrap();
        assert!(audit_history(&repo, &merge, &protected).is_err());
    }

    #[test]
    fn plaintext_magic_is_not_an_encryption_declaration() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = HistoryRepo::open_or_init_in(tmp.path()).unwrap().unwrap();
        let oid = repo.hash_blob(MAGIC).unwrap();
        let tree = repo
            .write_tree(&[("100644".into(), oid.clone(), "tracked/home/literal".into())])
            .unwrap();
        let upstream = super::super::reconcile::upstream(&repo, Some(&tree)).unwrap();
        assert_eq!(upstream.files["tracked/home/literal"].1, oid);
        assert!(encrypted_paths(&repo, Some(&tree)).unwrap().is_empty());
    }
    use super::*;

    #[test]
    fn plaintext_bypasses_envelope_limit_but_ciphertext_does_not() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = HistoryRepo::open_or_init_in(tmp.path()).unwrap().unwrap();
        let larger_than_pipe = vec![b'x'; 128 * 1024];
        for bytes in [
            b"".as_slice(),
            b"ordinary plaintext longer than the envelope limit".as_slice(),
            larger_than_pipe.as_slice(),
        ] {
            let object = ("100644".into(), repo.hash_blob(bytes).unwrap());
            assert!(envelope(&repo, &object, 4).unwrap().is_none());
        }
        let object = ("100644".into(), repo.hash_blob(MAGIC).unwrap());
        assert!(envelope(&repo, &object, 4).is_err());
        // The referenced submodule commit need not exist in the setup repo.
        let gitlink = ("160000".into(), "a".repeat(40));
        assert!(envelope(&repo, &gitlink, 4).unwrap().is_none());
    }

    #[test]
    fn envelope_preserves_modes_and_binds_path_and_scheme() {
        use age::secrecy::ExposeSecret;
        let tmp = tempfile::tempdir().unwrap();
        let repo = HistoryRepo::open_or_init_in(tmp.path()).unwrap().unwrap();
        let key = age::x25519::Identity::generate();
        let mut environment = crate::test::EnvVarGuard::new();
        environment.set("MISE_AGE_KEY", key.to_string().expose_secret());
        let recipients: Vec<Box<dyn age::Recipient + Send>> = vec![Box::new(key.to_public())];
        for mode in ["100644", "100755", "120000"] {
            let object = (
                mode.into(),
                repo.transient_blob_id(b"private contents").unwrap(),
            );
            let encrypted =
                encrypt(&repo, "tracked/home/secret", &object, "scheme", &recipients).unwrap();
            assert_eq!(encrypted.0, "100644");
            let wire = repo.cat_object(&encrypted.1).unwrap();
            assert!(!wire.windows(16).any(|w| w == b"private contents"));
            assert_eq!(
                decrypt(&repo, "tracked/home/secret", &encrypted, false).unwrap(),
                object
            );
            assert!(repo.object_type(&object.1).unwrap().is_none());
            assert!(
                repo.list_refs("refs/mise-decrypted-files/")
                    .unwrap()
                    .is_empty()
            );
            assert_eq!(repo.cat_object(&object.1).unwrap(), b"private contents");
            assert!(decrypt(&repo, "tracked/home/other", &encrypted, false).is_err());
            let mut outer = envelope(&repo, &encrypted, agecrypt::MAX_ENCRYPTED_BYTES)
                .unwrap()
                .unwrap();
            outer.scheme = "forged".into();
            let inner = Plaintext {
                path: outer.path.clone(),
                mode: mode.into(),
                scheme: "scheme".into(),
                content: Bytes(vec![]),
            };
            assert!(validate("tracked/home/secret", &outer, &inner).is_err());
        }
    }

    #[test]
    fn refuses_encryption_of_control_configuration() {
        assert!(control_file("config/config.toml"));
        assert!(control_file("config/conf.d/tools.toml"));
        assert!(control_file("config@macos/config.toml"));
        assert!(!control_file("templates/app.toml"));
        assert!(!control_file("home/config.toml"));
    }
}
