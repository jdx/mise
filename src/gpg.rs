use crate::Result;
use crate::file;
use eyre::{bail, eyre};
use pgp::composed::{Deserializable, DetachedSignature, SignedPublicKey};
use pgp::packet::Signature;
use pgp::types::{Fingerprint, KeyDetails, KeyId, VerifyingKey};
use std::io::{Cursor, Read};
use std::path::Path;

/// Verify a detached signature over `shasums` using the bundled Node.js release signing keys.
pub fn verify_node(shasums: &[u8], signature: &[u8]) -> Result<()> {
    verify_detached(include_str!("assets/gpg/node.asc"), signature, || {
        Ok(Cursor::new(shasums))
    })
}

/// Verify a detached signature over the Swift release tarball at `tarball_path` using the bundled
/// Swift release signing keys. The tarball is streamed from disk rather than buffered in memory.
pub fn verify_swift(tarball_path: &Path, signature: &[u8]) -> Result<()> {
    verify_detached(include_str!("assets/gpg/swift.asc"), signature, || {
        Ok(std::io::BufReader::new(file::open(tarball_path)?))
    })
}

/// Verify a detached signature entirely in-process (no external `gpg` binary).
///
/// `public_keys_asc` is one or more ASCII-armored public key blocks (a trusted keyring bundled
/// with mise). `open_data` returns a fresh reader over the signed content each time it is called,
/// so the content can be streamed (and, if necessary, re-read for another candidate key) without
/// buffering large files in memory.
///
/// Verification succeeds if any signature validates against any of the trusted keys or their
/// subkeys, mirroring `gpg --verify` against an imported keyring.
fn verify_detached<R, F>(public_keys_asc: &str, signature: &[u8], open_data: F) -> Result<()>
where
    R: Read,
    F: Fn() -> Result<R>,
{
    let keys = parse_public_keys(public_keys_asc)?;
    if keys.is_empty() {
        bail!("no trusted public keys available for verification");
    }
    let signatures = parse_signatures(signature)?;
    if signatures.is_empty() {
        bail!("no signature found to verify");
    }

    // Fast path: only try keys whose id/fingerprint matches the signature's issuer, so the signed
    // content is hashed at most once in the common case.
    for sig in &signatures {
        if verify_against_keys(sig, &keys, &open_data, true)? {
            return Ok(());
        }
    }
    // Fallback: try every trusted key, but only for signatures that carried no usable issuer
    // hint. A signature that named an issuer we don't trust is genuinely unverifiable, so skip it
    // rather than re-hashing the (potentially large) content against every key.
    for sig in signatures.iter().filter(|sig| !has_issuer(&sig.signature)) {
        if verify_against_keys(sig, &keys, &open_data, false)? {
            return Ok(());
        }
    }
    bail!("signature does not match any trusted public key");
}

fn verify_against_keys<R, F>(
    sig: &DetachedSignature,
    keys: &[SignedPublicKey],
    open_data: &F,
    require_issuer_match: bool,
) -> Result<bool>
where
    R: Read,
    F: Fn() -> Result<R>,
{
    for key in keys {
        if (!require_issuer_match
            || issuer_matches(&sig.signature, &key.fingerprint(), &key.legacy_key_id()))
            && try_verify(sig, key, open_data)?
        {
            return Ok(true);
        }
        for subkey in &key.public_subkeys {
            if (!require_issuer_match
                || issuer_matches(
                    &sig.signature,
                    &subkey.fingerprint(),
                    &subkey.legacy_key_id(),
                ))
                && try_verify(sig, subkey, open_data)?
            {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn try_verify<R, F, K>(sig: &DetachedSignature, key: &K, open_data: &F) -> Result<bool>
where
    R: Read,
    F: Fn() -> Result<R>,
    K: VerifyingKey,
{
    let reader = open_data()?;
    Ok(sig.signature.verify(key, reader).is_ok())
}

fn issuer_matches(sig: &Signature, fingerprint: &Fingerprint, key_id: &KeyId) -> bool {
    sig.issuer_fingerprint()
        .into_iter()
        .any(|f| f == fingerprint)
        || sig.issuer_key_id().into_iter().any(|k| k == key_id)
}

fn has_issuer(sig: &Signature) -> bool {
    !sig.issuer_fingerprint().is_empty() || !sig.issuer_key_id().is_empty()
}

/// Parse one or more concatenated ASCII-armored public key blocks.
///
/// rPGP's `from_armor_*` helpers only consume a single armored block, but the bundled keyrings
/// concatenate many blocks, so split them apart and parse each independently.
fn parse_public_keys(asc: &str) -> Result<Vec<SignedPublicKey>> {
    let mut keys = Vec::new();
    for block in split_armor_blocks(asc, "PGP PUBLIC KEY BLOCK") {
        let (iter, _headers) =
            SignedPublicKey::from_string_many(&block).map_err(|e| eyre!("parsing key: {e}"))?;
        for key in iter {
            keys.push(key.map_err(|e| eyre!("parsing key: {e}"))?);
        }
    }
    Ok(keys)
}

/// Parse detached signatures, accepting both ASCII-armored and binary encodings.
fn parse_signatures(signature: &[u8]) -> Result<Vec<DetachedSignature>> {
    let is_armored = signature
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .is_some_and(|i| signature[i..].starts_with(b"-----BEGIN"));
    let mut signatures = Vec::new();
    if is_armored {
        let (iter, _headers) = DetachedSignature::from_armor_many(signature)
            .map_err(|e| eyre!("parsing signature: {e}"))?;
        for sig in iter {
            signatures.push(sig.map_err(|e| eyre!("parsing signature: {e}"))?);
        }
    } else {
        for sig in DetachedSignature::from_bytes_many(signature)
            .map_err(|e| eyre!("parsing signature: {e}"))?
        {
            signatures.push(sig.map_err(|e| eyre!("parsing signature: {e}"))?);
        }
    }
    Ok(signatures)
}

/// Split concatenated ASCII armor blocks (`-----BEGIN <label>-----` … `-----END <label>-----`)
/// into individual block strings.
fn split_armor_blocks(input: &str, label: &str) -> Vec<String> {
    let begin = format!("-----BEGIN {label}-----");
    let end = format!("-----END {label}-----");
    let mut blocks = Vec::new();
    let mut current: Option<String> = None;
    for line in input.lines() {
        let trimmed = line.trim();
        if trimmed == begin {
            current = Some(String::new());
        }
        if let Some(buf) = current.as_mut() {
            buf.push_str(line);
            buf.push('\n');
            if trimmed == end {
                blocks.push(current.take().unwrap());
            }
        }
    }
    blocks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_bundled_node_keys() {
        let keys = parse_public_keys(include_str!("assets/gpg/node.asc")).unwrap();
        // node.asc concatenates many key blocks; ensure every block parses, not just the first.
        assert!(
            keys.len() > 10,
            "expected many node keys, got {}",
            keys.len()
        );
    }

    #[test]
    fn parses_all_bundled_swift_keys() {
        let keys = parse_public_keys(include_str!("assets/gpg/swift.asc")).unwrap();
        assert!(!keys.is_empty(), "expected at least one swift key");
    }

    #[test]
    fn rejects_when_no_signature() {
        let err = verify_node(b"data", b"").unwrap_err();
        assert!(err.to_string().contains("no signature"), "{err}");
    }

    #[test]
    fn rejects_unparseable_signature() {
        let sig = "-----BEGIN PGP SIGNATURE-----\n\ngarbage\n-----END PGP SIGNATURE-----\n";
        assert!(verify_node(b"data", sig.as_bytes()).is_err());
    }

    // A real, checked-in detached signature over the genuine Node.js v20.11.0 `SHASUMS256.txt`
    // (a binary `.sig` bundle with multiple signature packets). This exercises the full happy
    // path against the actual bundled keyring so a regression in the verify/reader plumbing
    // surfaces here rather than as a spurious install failure.
    const NODE_SHASUMS: &[u8] = include_bytes!("gpg_testdata/node_v20.11.0_SHASUMS256.txt");
    const NODE_SIG: &[u8] = include_bytes!("gpg_testdata/node_v20.11.0_SHASUMS256.txt.sig");

    #[test]
    fn verifies_real_node_signature() {
        verify_node(NODE_SHASUMS, NODE_SIG).expect("genuine node signature must verify");
    }

    #[test]
    fn rejects_tampered_content() {
        let mut tampered = NODE_SHASUMS.to_vec();
        tampered.push(b'x');
        assert!(
            verify_node(&tampered, NODE_SIG).is_err(),
            "tampered content must not verify"
        );
    }

    #[test]
    fn rejects_signature_from_untrusted_keyring() {
        // The genuine node signature must not verify against the unrelated swift keyring.
        assert!(verify_swift_bytes(NODE_SHASUMS, NODE_SIG).is_err());
    }

    fn verify_swift_bytes(data: &[u8], signature: &[u8]) -> Result<()> {
        verify_detached(include_str!("assets/gpg/swift.asc"), signature, || {
            Ok(Cursor::new(data))
        })
    }
}
