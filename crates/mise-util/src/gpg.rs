use crate::file;
use eyre::Result;
use eyre::{bail, eyre};
use pgp::composed::{Deserializable, DetachedSignature, SignedPublicKey};
use pgp::packet::Signature;
use pgp::ser::Serialize;
use pgp::types::{Fingerprint, KeyDetails, KeyId, VerifyingKey};
use std::cell::Cell;
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

/// One certificate from an OpenPGP keyring: its primary key fingerprint, as
/// uppercase hex, and its binary encoding.
#[derive(Debug, Clone)]
pub struct KeyringKey {
    pub fingerprint: String,
    pub bytes: Vec<u8>,
}

/// Parse an OpenPGP keyring, ASCII-armored or binary, in the forms vendors
/// publish repository signing keys in.
pub fn parse_keyring(data: &[u8]) -> Result<Vec<KeyringKey>> {
    // armor may follow a comment or a byte order mark; binary keyrings never
    // contain the header
    let armored = data
        .windows(PUBLIC_KEY_ARMOR.len())
        .any(|window| window == PUBLIC_KEY_ARMOR.as_bytes());
    let keys = if armored {
        parse_public_keys(&String::from_utf8_lossy(data))?
    } else {
        SignedPublicKey::from_bytes_many(Cursor::new(data))
            .map_err(|e| eyre!("parsing key: {e}"))?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| eyre!("parsing key: {e}"))?
    };
    keys.into_iter()
        .map(|key| {
            Ok(KeyringKey {
                fingerprint: format!("{:X}", key.fingerprint()),
                bytes: key.to_bytes().map_err(|e| eyre!("encoding key: {e}"))?,
            })
        })
        .collect()
}

const PUBLIC_KEY_ARMOR: &str = "-----BEGIN PGP PUBLIC KEY BLOCK-----";

fn is_armored(data: &[u8]) -> bool {
    data.iter()
        .position(|b| !b.is_ascii_whitespace())
        .is_some_and(|i| data[i..].starts_with(b"-----BEGIN"))
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

    // Every verification attempt re-hashes the whole signed content, and the signature file
    // (which comes from the download host) decides how many attempts there are: it can hold any
    // number of signature packets, each naming any number of issuers. Cap the total at one
    // attempt per trusted key and subkey, which is all a single signature without an issuer hint
    // needs, so the work is bounded by the bundled keyring rather than by the downloaded file.
    let key_count: usize = keys.iter().map(|k| 1 + k.public_subkeys.len()).sum();
    let attempts_left = Cell::new(key_count);
    let open_data = || match attempts_left.get().checked_sub(1) {
        Some(left) => {
            attempts_left.set(left);
            open_data()
        }
        None => bail!("too many signature verification attempts"),
    };

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
    let mut signatures = Vec::new();
    if is_armored(signature) {
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
        let line = line.trim_start_matches('\u{feff}');
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
    fn keyring_parses_armored_and_binary_forms_alike() {
        let armored = parse_keyring(include_bytes!("assets/gpg/swift.asc")).unwrap();
        assert!(!armored.is_empty());
        assert!(armored.iter().all(|key| {
            key.fingerprint.len() == 40
                && key
                    .fingerprint
                    .chars()
                    .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_lowercase())
        }));
        let binary = armored
            .iter()
            .flat_map(|key| key.bytes.clone())
            .collect::<Vec<_>>();
        let reparsed = parse_keyring(&binary).unwrap();
        assert_eq!(
            reparsed.iter().map(|k| &k.fingerprint).collect::<Vec<_>>(),
            armored.iter().map(|k| &k.fingerprint).collect::<Vec<_>>()
        );
    }

    #[test]
    fn keyring_accepts_armor_after_a_comment_or_bom() {
        let armored = include_str!("assets/gpg/swift.asc");
        let expected = parse_keyring(armored.as_bytes()).unwrap().len();
        for prefixed in [
            format!("# Vendor signing key\n\n{armored}"),
            format!("\u{feff}{armored}"),
        ] {
            assert_eq!(parse_keyring(prefixed.as_bytes()).unwrap().len(), expected);
        }
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

    /// Verify `count` copies of a bogus v4 signature carrying `unhashed` subpackets against the
    /// swift keyring and return how often the signed content was read.
    fn count_reads_of_bogus_signatures(unhashed: &[u8], count: usize) -> usize {
        // v4, binary document, RSA, SHA-256, no hashed subpackets
        let mut body = vec![4, 0, 1, 8, 0, 0, 0, unhashed.len() as u8];
        body.extend_from_slice(unhashed);
        // left 16 bits of the digest, then a 1-bit MPI as the "signature"
        body.extend_from_slice(&[0, 0, 0, 1, 1]);
        let mut packet = vec![0xc2, body.len() as u8];
        packet.extend_from_slice(&body);

        let reads = Cell::new(0);
        let open_data = || {
            reads.set(reads.get() + 1);
            Ok(Cursor::new(b"data"))
        };
        let err = verify_detached(
            include_str!("assets/gpg/swift.asc"),
            &packet.repeat(count),
            open_data,
        )
        .unwrap_err();
        assert!(
            err.to_string()
                .contains("too many signature verification attempts"),
            "{err}"
        );
        reads.get()
    }

    fn swift_key_ids() -> Vec<KeyId> {
        parse_public_keys(include_str!("assets/gpg/swift.asc"))
            .unwrap()
            .iter()
            .flat_map(|key| {
                let subkeys = key.public_subkeys.iter().map(|k| k.legacy_key_id());
                std::iter::once(key.legacy_key_id()).chain(subkeys)
            })
            .collect()
    }

    #[test]
    fn bounds_reads_for_signatures_without_issuer() {
        // Issuer-less signatures fall back to every trusted key; many of them must not multiply
        // the number of passes over the signed content.
        assert_eq!(
            count_reads_of_bogus_signatures(&[], 100),
            swift_key_ids().len()
        );
    }

    #[test]
    fn bounds_reads_for_signatures_naming_every_issuer() {
        let key_ids = swift_key_ids();
        // one issuer key id subpacket (type 16) per trusted key, so every key takes the fast path
        let unhashed: Vec<u8> = key_ids
            .iter()
            .flat_map(|id| [&[9, 16], id.as_ref()].concat())
            .collect();
        assert_eq!(
            count_reads_of_bogus_signatures(&unhashed, 100),
            key_ids.len()
        );
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
