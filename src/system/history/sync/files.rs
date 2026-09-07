//! Encryption at the publication boundary. Reconciliation only sees local
//! plaintext objects; no plaintext comparison hashes enter the shared tree.
use std::collections::{BTreeMap, BTreeSet};

use eyre::{Result, WrapErr, bail};
use serde::{Deserialize, Serialize};

use super::{encrypted::Bytes, layout, reconcile::Object, share::ShareReport};
use crate::{agecrypt, system::history::shadow::HistoryRepo};

const MAGIC: &[u8] = b"mise-encrypted-file-v1\n";
const CACHE: &str = "refs/mise-decrypted-files/";
const INDEX: &str = ".mise-history/encrypted-files.json";

#[derive(Serialize, Deserialize)]
struct EncryptionIndex {
    version: u32,
    paths: BTreeSet<String>,
}

pub(crate) fn encrypted_paths(
    repo: &HistoryRepo,
    commit: Option<&str>,
) -> Result<BTreeSet<String>> {
    let Some(commit) = commit else {
        return Ok(BTreeSet::new());
    };
    let Some((mode, oid)) = repo.object_at(commit, INDEX)? else {
        return Ok(BTreeSet::new());
    };
    if mode != "100644" {
        bail!("invalid encrypted-file index mode");
    }
    let index: EncryptionIndex =
        serde_json::from_slice(&repo.cat_object_bounded(&oid, 16 * 1024 * 1024)?)?;
    if index.version != 1
        || index
            .paths
            .iter()
            .any(|path| !layout::is_safe_branch_path(path) || control_file(path))
    {
        bail!("invalid encrypted-file index");
    }
    for path in &index.paths {
        if repo.object_at(commit, path)?.is_none() {
            bail!("encrypted file index names a missing file: {path}");
        }
    }
    Ok(index.paths)
}

fn write_index(
    repo: &HistoryRepo,
    upstream: Option<&str>,
    paths: BTreeSet<String>,
    out: &mut BTreeMap<String, Option<Object>>,
) -> Result<()> {
    let object = if paths.is_empty() {
        None
    } else {
        Some((
            "100644".into(),
            repo.hash_blob(&serde_json::to_vec(&EncryptionIndex { version: 1, paths })?)?,
        ))
    };
    let previous = upstream
        .map(|head| repo.object_at(head, INDEX))
        .transpose()?
        .flatten();
    if previous != object {
        out.insert(INDEX.into(), object);
    }
    Ok(())
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
    if path.starts_with("sources/") || path.starts_with("tracked/") {
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
    let cache_ref = format!("{CACHE}{}", object.1);
    let bytes = match repo.ref_oid(&cache_ref)? {
        Some(oid) => repo.cat_object_bounded(&oid, agecrypt::MAX_PLAINTEXT_BYTES)?,
        None => agecrypt::decrypt_sync(&outer.ciphertext.0, interactive)
            .wrap_err_with(|| format!("cannot unlock {path}; run mise bootstrap dotfiles pull interactively with a matching age identity"))?,
    };
    let inner: Plaintext =
        rmp_serde::from_slice(&bytes).wrap_err("invalid encrypted file payload")?;
    validate(path, &outer, &inner)?;
    let oid = repo.hash_blob(&inner.content.0)?;
    let cached = repo.hash_blob(&bytes)?;
    if repo.ref_oid(&cache_ref)?.is_none() {
        repo.update_ref(&cache_ref, &cached, None)?;
    }
    Ok((inner.mode, oid))
}

fn encrypt(
    repo: &HistoryRepo,
    path: &str,
    object: &Object,
    scheme: &str,
    recipients: &[Box<dyn age::Recipient + Send>],
) -> Result<Object> {
    if control_file(path) {
        bail!("encrypt an external dotfile source instead of configuration: {path}");
    }
    if !matches!(object.0.as_str(), "100644" | "100755" | "120000") {
        bail!("unsupported encrypted file mode: {path}");
    }
    let inner = Plaintext {
        path: path.into(),
        mode: object.0.clone(),
        scheme: scheme.into(),
        content: Bytes(repo.cat_object_bounded(&object.1, agecrypt::MAX_PLAINTEXT_BYTES)?),
    };
    let bytes = rmp_serde::to_vec_named(&inner)?;
    let outer = Envelope {
        path: path.into(),
        mode: object.0.clone(),
        scheme: scheme.into(),
        ciphertext: Bytes(agecrypt::encrypt_bytes(&bytes, recipients)?),
    };
    let mut encoded = MAGIC.to_vec();
    encoded.extend(rmp_serde::to_vec_named(&outer)?);
    if encoded.len() as u64 > agecrypt::MAX_ENCRYPTED_BYTES {
        bail!("encrypted file exceeds the size limit: {path}");
    }
    let oid = repo.hash_blob(&encoded)?;
    // A publisher already has the plaintext, including on hardware-only
    // setups. Keep it locally so the next sync needs no hardware operation.
    repo.update_ref(&format!("{CACHE}{oid}"), &repo.hash_blob(&bytes)?, None)?;
    Ok(("100644".into(), oid))
}

/// Turn reconciled plaintext changes into a publishable tree delta, including
/// recipient rotation for unchanged files. Ciphertext is never a merge base.
pub(crate) fn publication(
    repo: &HistoryRepo,
    upstream: Option<&str>,
    shared: &ShareReport,
    changes: &BTreeMap<String, Option<Object>>,
    interactive: bool,
) -> Result<BTreeMap<String, Option<Object>>> {
    publication_with(
        repo,
        upstream,
        shared,
        changes,
        interactive,
        crate::system::history::config::file_recipients,
    )
}

fn publication_with(
    repo: &HistoryRepo,
    upstream: Option<&str>,
    shared: &ShareReport,
    changes: &BTreeMap<String, Option<Object>>,
    interactive: bool,
    configured_recipients: impl FnOnce() -> Result<Vec<String>>,
) -> Result<BTreeMap<String, Option<Object>>> {
    let mut out = changes.clone();
    let mut existing = BTreeMap::new();
    let mut protected = BTreeSet::new();
    if let Some(commit) = upstream {
        for path in encrypted_paths(repo, Some(commit))? {
            let object = repo
                .object_at(commit, &path)?
                .ok_or_else(|| eyre::eyre!("encrypted file index names a missing file: {path}"))?;
            let envelope = envelope(repo, &object, agecrypt::MAX_ENCRYPTED_BYTES)?
                .ok_or_else(|| eyre::eyre!("missing encrypted file envelope: {path}"))?;
            protected.insert(path.clone());
            existing.insert(path, (object, envelope));
        }
    }
    for (path, file) in &shared.files {
        if file.encrypt {
            protected.insert(path.clone());
        } else if file.encrypt_explicit {
            protected.remove(path);
            if !changes.contains_key(path)
                && let Some((object, _)) = existing.get(path)
            {
                out.insert(
                    path.clone(),
                    Some(decrypt(repo, path, object, interactive)?),
                );
            }
        }
    }
    if protected.is_empty() {
        if !existing.is_empty() {
            write_index(repo, upstream, BTreeSet::new(), &mut out)?;
        }
        return Ok(out);
    }
    let strings = configured_recipients()?;
    if strings.is_empty() {
        bail!("encrypted dotfiles require [history.encryption].recipients; nothing was published");
    }
    let mut normalized = strings.clone();
    normalized.sort();
    normalized.dedup();
    let scheme = crate::hash::hash_sha256_to_str(&normalized.join("\n"));
    if (existing.is_empty()
        || existing
            .values()
            .any(|(_, envelope)| envelope.scheme != scheme))
        && strings
            .iter()
            .all(|s| s.parse::<age::plugin::Recipient>().is_ok())
    {
        warn!(
            "hardware-only file recipients: device loss can make these files unrecoverable; add an independent recovery recipient"
        );
    }
    // Reusing cached ciphertext requires no recipient plugin or hardware.
    // Resolve recipients only if a file actually needs fresh encryption.
    let mut recipients = None;
    for path in &protected {
        let previous = existing.get(path);
        let current = if let Some(change) = changes.get(path) {
            change.clone()
        } else if let Some((object, _)) = previous {
            Some(decrypt(repo, path, object, interactive)?)
        } else {
            shared
                .files
                .get(path)
                .map(|file| (file.mode.clone(), file.oid.clone()))
        };
        let Some(current) = current else {
            continue;
        };
        if let Some((object, envelope)) = previous
            && envelope.scheme == scheme
            && decrypt(repo, path, object, interactive)? == current
        {
            out.remove(path);
        } else {
            let recipients = match &recipients {
                Some(recipients) => recipients,
                None => recipients.insert(
                    strings
                        .iter()
                        .map(|s| {
                            agecrypt::parse_recipient_mode(s, interactive)?
                                .ok_or_else(|| eyre::eyre!("invalid encrypted-file recipient"))
                        })
                        .collect::<Result<Vec<_>>>()?,
                ),
            };
            out.insert(
                path.clone(),
                Some(encrypt(repo, path, &current, &scheme, recipients)?),
            );
        }
    }
    let mut indexed: BTreeSet<_> = existing
        .keys()
        .filter(|path| protected.contains(*path))
        .cloned()
        .collect();
    for (path, object) in &out {
        if protected.contains(path) && object.is_some() {
            indexed.insert(path.clone());
        } else {
            indexed.remove(path);
        }
    }
    write_index(repo, upstream, indexed, &mut out)?;
    if out
        .iter()
        .any(|(p, o)| !p.starts_with(".mise-history/") && o.is_some())
    {
        out.insert(
            layout::MARKER_PATH.into(),
            Some(("100644".into(), repo.hash_blob(b"format = 2\n")?)),
        );
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    #[test]
    fn unchanged_ciphertext_does_not_initialize_recipients() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = HistoryRepo::open_or_init_in(tmp.path()).unwrap().unwrap();
        let key = age::x25519::Identity::generate();
        let recipients: Vec<Box<dyn age::Recipient + Send>> = vec![Box::new(key.to_public())];
        // Parsing this recipient would fail. Cached content with the same
        // scheme must not parse recipients at all, as with a hardware plugin.
        let configured = "unavailable-plugin-recipient";
        let scheme = crate::hash::hash_sha256_to_str(configured);
        let path = "tracked/home/secret";
        let object = ("100644".into(), repo.hash_blob(b"secret").unwrap());
        let encrypted = encrypt(&repo, path, &object, &scheme, &recipients).unwrap();
        let mut delta = BTreeMap::from([(path.to_string(), Some(encrypted))]);
        write_index(&repo, None, BTreeSet::from([path.to_string()]), &mut delta).unwrap();
        let tree = repo
            .compose(
                &repo.empty_object("tree").unwrap(),
                &delta
                    .into_iter()
                    .map(|(path, object)| crate::system::history::shadow::Overlay { path, object })
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        let plain = ("100644".into(), repo.hash_blob(b"new plaintext").unwrap());
        let changes = BTreeMap::from([("tracked/home/plain".into(), Some(plain.clone()))]);
        let result = publication_with(
            &repo,
            Some(&tree),
            &ShareReport::default(),
            &changes,
            false,
            || Ok(vec![configured.into()]),
        )
        .unwrap();
        assert_eq!(result.get("tracked/home/plain"), Some(&Some(plain)));
        assert!(!result.contains_key(path));
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
            let object = (mode.into(), repo.hash_blob(b"private contents").unwrap());
            let encrypted =
                encrypt(&repo, "tracked/home/secret", &object, "scheme", &recipients).unwrap();
            assert_eq!(encrypted.0, "100644");
            repo.delete_ref(&format!("{CACHE}{}", encrypted.1)).unwrap();
            let wire = repo.cat_object(&encrypted.1).unwrap();
            assert!(!wire.windows(16).any(|w| w == b"private contents"));
            assert_eq!(
                decrypt(&repo, "tracked/home/secret", &encrypted, false).unwrap(),
                object
            );
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
        assert!(control_file("config.toml"));
        assert!(control_file("conf.d/tools.toml"));
        assert!(!control_file("templates/app.toml"));
        assert!(!control_file("tracked/home/config.toml"));
    }
}
