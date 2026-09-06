//! Encryption at the publication boundary. Reconciliation only sees local
//! plaintext objects; no plaintext comparison hashes enter the shared tree.
use std::collections::{BTreeMap, BTreeSet};

use eyre::{Result, WrapErr, bail};
use serde::{Deserialize, Serialize};

use super::{encrypted::Bytes, layout, reconcile::Object, share::ShareReport};
use crate::{agecrypt, system::history::shadow::HistoryRepo};

const MAGIC: &[u8] = b"mise-encrypted-file-v1\n";
const CACHE: &str = "refs/mise-decrypted-files/";

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

fn envelope(repo: &HistoryRepo, object: &Object) -> Result<Option<Envelope>> {
    // Bound remote data before decoding its envelope.
    let bytes = repo.cat_object_bounded(&object.1, agecrypt::MAX_ENCRYPTED_BYTES)?;
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
    let Some(outer) = envelope(repo, object)? else {
        return Ok(object.clone());
    };
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
    let mut out = changes.clone();
    let mut existing = BTreeMap::new();
    let mut protected = BTreeSet::new();
    if let Some(commit) = upstream {
        for item in repo.ls_tree(commit)? {
            if control_file(&item.path) || item.path.starts_with(".mise-history/") {
                continue;
            }
            let object = (item.mode, item.oid);
            if let Some(envelope) = envelope(repo, &object)? {
                protected.insert(item.path.clone());
                existing.insert(item.path, (object, envelope));
            }
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
        return Ok(out);
    }
    let strings = crate::system::history::config::file_recipients()?;
    if strings.is_empty() {
        bail!("encrypted dotfiles require [history.encryption].recipients; nothing was published");
    }
    if existing.is_empty()
        && strings
            .iter()
            .all(|s| s.parse::<age::plugin::Recipient>().is_ok())
    {
        warn!(
            "hardware-only file recipients: device loss can make these files unrecoverable; add an independent recovery recipient"
        );
    }
    let mut normalized = strings.clone();
    normalized.sort();
    normalized.dedup();
    let scheme = crate::hash::hash_sha256_to_str(&normalized.join("\n"));
    let recipients: Vec<_> = strings
        .iter()
        .map(|s| {
            agecrypt::parse_recipient(s)?
                .ok_or_else(|| eyre::eyre!("invalid encrypted-file recipient"))
        })
        .collect::<Result<_>>()?;
    for path in protected {
        let previous = existing.get(&path);
        let current = if let Some(change) = changes.get(&path) {
            change.clone()
        } else if let Some((object, _)) = previous {
            Some(decrypt(repo, &path, object, interactive)?)
        } else {
            shared
                .files
                .get(&path)
                .map(|file| (file.mode.clone(), file.oid.clone()))
        };
        let Some(current) = current else {
            continue;
        };
        if let Some((object, envelope)) = previous
            && envelope.scheme == scheme
            && decrypt(repo, &path, object, interactive)? == current
        {
            out.remove(&path);
        } else {
            out.insert(
                path.clone(),
                Some(encrypt(repo, &path, &current, &scheme, &recipients)?),
            );
        }
    }
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
    use super::*;

    #[test]
    fn envelope_preserves_modes_and_binds_path_and_scheme() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = HistoryRepo::open_or_init_in(tmp.path()).unwrap().unwrap();
        let key = age::x25519::Identity::generate();
        let recipients: Vec<Box<dyn age::Recipient + Send>> = vec![Box::new(key.to_public())];
        for mode in ["100644", "100755", "120000"] {
            let object = (mode.into(), repo.hash_blob(b"private contents").unwrap());
            let encrypted =
                encrypt(&repo, "tracked/home/secret", &object, "scheme", &recipients).unwrap();
            assert_eq!(encrypted.0, "100644");
            let wire = repo.cat_object(&encrypted.1).unwrap();
            assert!(!wire.windows(16).any(|w| w == b"private contents"));
            assert_eq!(
                decrypt(&repo, "tracked/home/secret", &encrypted, false).unwrap(),
                object
            );
            assert!(decrypt(&repo, "tracked/home/other", &encrypted, false).is_err());
            let mut outer = envelope(&repo, &encrypted).unwrap().unwrap();
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
