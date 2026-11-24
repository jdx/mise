use crate::file;
use eyre::Result;
use eyre::{bail, eyre};
use pgp::composed::{Deserializable, DetachedSignature, SignedPublicKey, SignedPublicSubKey};
use pgp::packet::{KeyFlags, Signature, SignatureType, SubpacketData};
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
/// subkeys that may sign data, mirroring `gpg --verify` against an imported keyring.
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
    let trusted = trusted_keys(&keys);
    let attempts_left = Cell::new(trusted.len());
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
        if verify_against_keys(sig, &trusted, &open_data, true)? {
            return Ok(());
        }
    }
    // Fallback: try every trusted key, but only for signatures that carried no usable issuer
    // hint. A signature that named an issuer we don't trust is genuinely unverifiable, so skip it
    // rather than re-hashing the (potentially large) content against every key.
    for sig in signatures.iter().filter(|sig| !has_issuer(&sig.signature)) {
        if verify_against_keys(sig, &trusted, &open_data, false)? {
            return Ok(());
        }
    }
    bail!("signature does not match any trusted public key");
}

/// A trusted key or subkey together with the identifiers a signature can name as its issuer.
struct TrustedKey<'a> {
    key: &'a SignedPublicKey,
    /// `None` for the primary key itself.
    subkey: Option<&'a SignedPublicSubKey>,
    fingerprint: Fingerprint,
    key_id: KeyId,
}

/// List every trusted key and subkey that may sign data, with its fingerprint and key id.
/// Encryption, authentication and certify-only keys are left out, as `gpg --verify` leaves them
/// out.
///
/// rPGP serializes and hashes a key each time it is asked for either identifier, so compute them
/// here, once per key, rather than for every signature packet: the download host chooses how many
/// packets there are, and one naming an issuer we don't trust is compared with every key without
/// ever counting as a verification attempt.
fn trusted_keys(keys: &[SignedPublicKey]) -> Vec<TrustedKey<'_>> {
    let mut trusted = Vec::new();
    for key in keys {
        let (fingerprint, key_id) = (key.fingerprint(), key.legacy_key_id());
        if primary_may_sign(key, &fingerprint, &key_id) {
            trusted.push(TrustedKey {
                key,
                subkey: None,
                fingerprint,
                key_id,
            });
        }
        for subkey in signing_subkeys(key) {
            trusted.push(TrustedKey {
                key,
                subkey: Some(subkey),
                fingerprint: subkey.fingerprint(),
                key_id: subkey.legacy_key_id(),
            });
        }
    }
    trusted
}

/// Whether the primary key of `key` may sign data, rather than only certify its user IDs and
/// subkeys.
fn primary_may_sign(key: &SignedPublicKey, fingerprint: &Fingerprint, key_id: &KeyId) -> bool {
    // user IDs also carry certifications by other people's keys, which say nothing about this one
    let own = |sig: &&Signature| {
        sig.issuer_fingerprint().contains(&fingerprint) || sig.issuer_key_id().contains(&key_id)
    };
    // a user ID speaks through its newest self-signature, unless that one revokes it
    let user_ids = key.details.users.iter().filter_map(|user| {
        user.signatures
            .iter()
            .filter(own)
            .max_by_key(|sig| sig.created())
            .filter(|sig| sig.typ() != Some(SignatureType::CertRevocation))
    });
    let direct = key.details.direct_signatures.iter().filter(own);
    primary_flags_allow_signing(direct, user_ids)
}

/// Whether a primary key may sign data, given its own direct-key signatures and the self-signature
/// each user ID speaks through. As for `gpg --verify`, the newest direct-key signature decides
/// when it has key flags, and otherwise the newest user ID self-signature that has them.
fn primary_flags_allow_signing<'a>(
    direct: impl IntoIterator<Item = &'a Signature>,
    user_ids: impl IntoIterator<Item = &'a Signature>,
) -> bool {
    let direct = direct.into_iter().max_by_key(|sig| sig.created());
    match direct.and_then(key_flags) {
        Some(flags) => flags.sign(),
        None => may_sign(user_ids),
    }
}

/// The subkeys of `key` that may sign data, rather than only encrypt or authenticate.
fn signing_subkeys(key: &SignedPublicKey) -> impl Iterator<Item = &SignedPublicSubKey> {
    key.public_subkeys
        .iter()
        .filter(|subkey| may_sign(&subkey.signatures))
}

/// Whether a key may sign data according to its self-signatures. What a key is for is not part of
/// the key but a key flags subpacket of its self-signatures, which `Signature::verify` does not
/// look at. The newest one with key flags decides, as it does for `gpg --verify`. A key without
/// any key flags predates them and is limited only by its algorithm.
fn may_sign<'a>(self_signatures: impl IntoIterator<Item = &'a Signature>) -> bool {
    self_signatures
        .into_iter()
        .filter_map(|sig| Some((sig.created()?, key_flags(sig)?)))
        .max_by_key(|(newest, _)| *newest)
        .is_none_or(|(_, flags)| flags.sign())
}

/// The key flags subpacket of `sig`, if it has one (`Signature::key_flags` does not tell an
/// absent subpacket from an empty one).
fn key_flags(sig: &Signature) -> Option<&KeyFlags> {
    sig.config()?
        .hashed_subpackets()
        .find_map(|subpacket| match &subpacket.data {
            SubpacketData::KeyFlags(flags) => Some(flags),
            _ => None,
        })
}

fn verify_against_keys<R, F>(
    sig: &DetachedSignature,
    trusted: &[TrustedKey<'_>],
    open_data: &F,
    require_issuer_match: bool,
) -> Result<bool>
where
    R: Read,
    F: Fn() -> Result<R>,
{
    // Likewise read the issuers out of the signature's subpackets once, not once per key.
    let issuer_fingerprints = sig.signature.issuer_fingerprint();
    let issuer_key_ids = sig.signature.issuer_key_id();
    for candidate in trusted {
        if require_issuer_match
            && !issuer_fingerprints.contains(&&candidate.fingerprint)
            && !issuer_key_ids.contains(&&candidate.key_id)
        {
            continue;
        }
        let verified = match candidate.subkey {
            Some(subkey) => try_verify(sig, subkey, open_data)?,
            None => try_verify(sig, candidate.key, open_data)?,
        };
        if verified {
            return Ok(true);
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

/// The largest detached signature file mise parses. A release signature is a few packets of
/// well under 1 KiB each (node's `SHASUMS256.txt.sig` for v20.11.0 is 566 bytes). The file comes
/// from the download host, a parsed packet can take 70 times its size in memory (rPGP reserves
/// room for 32 subpackets per area), and armor parsing time grows with the square of the size,
/// so the size is checked before any parsing.
pub const MAX_SIGNATURE_BYTES: u64 = 256 * 1024;

/// Parse detached signatures, accepting both ASCII-armored and binary encodings.
fn parse_signatures(signature: &[u8]) -> Result<Vec<DetachedSignature>> {
    if signature.len() as u64 > MAX_SIGNATURE_BYTES {
        bail!(
            "signature file is {} bytes, larger than the {MAX_SIGNATURE_BYTES} bytes a release signature can need",
            signature.len()
        );
    }
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
    fn rejects_an_oversized_signature_file_before_parsing() {
        // valid packets: a parser that ran would stop at the attempt limit instead
        let count = MAX_SIGNATURE_BYTES as usize / NODE_SIG.len() + 1;
        let err = verify_node(NODE_SHASUMS, &NODE_SIG.repeat(count)).unwrap_err();
        assert!(err.to_string().contains("larger than"), "{err}");
    }

    #[test]
    fn rejects_unparseable_signature() {
        let sig = "-----BEGIN PGP SIGNATURE-----\n\ngarbage\n-----END PGP SIGNATURE-----\n";
        assert!(verify_node(b"data", sig.as_bytes()).is_err());
    }

    /// A bogus v4 signature packet carrying `unhashed` subpackets.
    fn bogus_signature(unhashed: &[u8]) -> Vec<u8> {
        // v4, binary document, RSA, SHA-256, no hashed subpackets
        let mut body = vec![4, 0, 1, 8, 0, 0, 0, unhashed.len() as u8];
        body.extend_from_slice(unhashed);
        // left 16 bits of the digest, then a 1-bit MPI as the "signature"
        body.extend_from_slice(&[0, 0, 0, 1, 1]);
        let mut packet = vec![0xc2, body.len() as u8];
        packet.extend_from_slice(&body);
        packet
    }

    /// Verify `count` copies of a bogus v4 signature carrying `unhashed` subpackets against the
    /// swift keyring and return how often the signed content was read.
    fn count_reads_of_bogus_signatures(unhashed: &[u8], count: usize) -> usize {
        let packet = bogus_signature(unhashed);

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

    /// The key ids of the swift keys and subkeys, all of which may sign.
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

    /// A bogus v4 self-signature of type `typ` made at `created`, with a key flags subpacket of
    /// `flags` unless that is `None`.
    fn self_signature(typ: u8, created: u32, flags: Option<u8>) -> Signature {
        let mut hashed = vec![5, 2];
        hashed.extend_from_slice(&created.to_be_bytes());
        if let Some(flags) = flags {
            hashed.extend_from_slice(&[2, 27, flags]);
        }
        let mut body = vec![4, typ, 1, 8, 0, hashed.len() as u8];
        body.extend_from_slice(&hashed);
        body.extend_from_slice(&[0, 0, 0, 0, 0, 1, 1]);
        let mut packet = vec![0xc2, body.len() as u8];
        packet.extend_from_slice(&body);
        parse_signatures(&packet).unwrap().remove(0).signature
    }

    #[test]
    fn a_direct_key_signature_decides_what_the_primary_key_may_do() {
        const DIRECT: u8 = 0x1f;
        const POSITIVE: u8 = 0x13;
        const CERTIFY: u8 = 0x01;
        const SIGN: u8 = 0x02;
        let may_sign = |direct: &[Signature], user_ids: &[Signature]| {
            primary_flags_allow_signing(direct, user_ids)
        };
        let certify_only = self_signature(DIRECT, 100, Some(CERTIFY));
        let signer = self_signature(DIRECT, 100, Some(CERTIFY | SIGN));
        let no_flags = self_signature(DIRECT, 300, None);
        let older_uid = |flags| self_signature(POSITIVE, 50, Some(flags));
        let newer_uid = |flags| self_signature(POSITIVE, 200, Some(flags));

        // the direct-key signature's flags win over any user ID, however new
        assert!(!may_sign(&[certify_only.clone()], &[newer_uid(SIGN)]));
        assert!(may_sign(&[signer.clone()], &[newer_uid(CERTIFY)]));
        // only the newest direct-key signature speaks, and one without flags leaves it to the
        // user IDs
        assert!(may_sign(
            &[certify_only.clone(), no_flags.clone()],
            &[newer_uid(SIGN)]
        ));
        assert!(!may_sign(&[no_flags.clone()], &[newer_uid(CERTIFY)]));
        // among user IDs the newest one with key flags decides
        assert!(may_sign(&[], &[older_uid(CERTIFY), newer_uid(SIGN)]));
        assert!(!may_sign(&[], &[older_uid(SIGN), newer_uid(CERTIFY)]));
        // and with no key flags anywhere only the algorithm limits the key
        assert!(may_sign(&[no_flags], &[]));
        assert!(!may_sign(&[certify_only], &[]));
    }

    #[test]
    fn issuers_are_matched_against_the_identifiers_listed_once() {
        // Matching a signature's issuer must compare it with the identifiers `trusted_keys`
        // computed, never compute them again: a signature naming an issuer that is not trusted
        // uses up no verification attempt, so the download host chooses how often that matching
        // runs. Give a trusted key a stand-in key id that rPGP would never compute for it: only a
        // comparison with the listed id can match a signature naming the stand-in, and only a
        // recomputed id can match one naming the key's real id.
        let keys = parse_public_keys(include_str!("assets/gpg/node.asc")).unwrap();
        let mut trusted = trusted_keys(&keys);
        let real = trusted[0].key_id;
        let stand_in = KeyId::from([0xaa; 8]);
        assert_ne!(real, stand_in);
        trusted[0].key_id = stand_in;
        let reads_for = |issuer: KeyId| {
            let unhashed = [&[9, 16], issuer.as_ref()].concat();
            let sig = &parse_signatures(&bogus_signature(&unhashed)).unwrap()[0];
            let reads = Cell::new(0);
            let open_data = || {
                reads.set(reads.get() + 1);
                Ok(Cursor::new(b"data"))
            };
            assert!(!verify_against_keys(sig, &trusted, &open_data, true).unwrap());
            reads.get()
        };
        assert_eq!(reads_for(stand_in), 1, "the listed key id was not used");
        assert_eq!(reads_for(real), 0, "the key id was computed again");
    }

    #[test]
    fn signatures_naming_an_unknown_issuer_are_skipped_without_reading() {
        // An issuer key id subpacket (type 16) for a key that is in no keyring. Such signatures
        // never use up a verification attempt, so none of them may read the signed content.
        let packet = bogus_signature(&[9, 16, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa]);
        let reads = Cell::new(0);
        let open_data = || {
            reads.set(reads.get() + 1);
            Ok(Cursor::new(b"data"))
        };
        let err = verify_detached(
            include_str!("assets/gpg/node.asc"),
            &packet.repeat(2000),
            open_data,
        )
        .unwrap_err();
        assert!(
            err.to_string()
                .contains("signature does not match any trusted public key"),
            "{err}"
        );
        assert_eq!(reads.get(), 0);
    }

    #[test]
    fn lists_every_trusted_key_and_subkey_with_its_identifiers() {
        let keys = parse_public_keys(include_str!("assets/gpg/swift.asc")).unwrap();
        let trusted = trusted_keys(&keys);
        assert_eq!(
            trusted.iter().map(|t| t.key_id).collect::<Vec<_>>(),
            swift_key_ids()
        );
        for t in &trusted {
            let (fingerprint, key_id) = match t.subkey {
                Some(subkey) => (subkey.fingerprint(), subkey.legacy_key_id()),
                None => (t.key.fingerprint(), t.key.legacy_key_id()),
            };
            assert_eq!((&t.fingerprint, &t.key_id), (&fingerprint, &key_id));
        }
    }

    // A certificate made for these tests: a primary key that may only certify, a signing subkey
    // and an authentication subkey, with a signature by each of the three keys (the two that may
    // not sign made theirs before their key flags were changed). `gpg --verify` accepts only the
    // one by the signing subkey and reports "No public key" for the other two.
    fn verify_with_key_flags_keyring(signature: &str) -> Result<()> {
        verify_detached(
            include_str!("gpg_testdata/key_flags_keyring.asc"),
            signature.as_bytes(),
            || Ok(Cursor::new(b"signed content\n")),
        )
    }

    #[test]
    fn accepts_signature_by_signing_subkey() {
        verify_with_key_flags_keyring(include_str!(
            "gpg_testdata/key_flags_signed_by_signing_subkey.asc"
        ))
        .expect("signature by a signing subkey must verify");
    }

    #[test]
    fn rejects_signature_by_key_that_may_not_sign() {
        for signature in [
            include_str!("gpg_testdata/key_flags_signed_by_primary.asc"),
            include_str!("gpg_testdata/key_flags_signed_by_authentication_subkey.asc"),
        ] {
            let err = verify_with_key_flags_keyring(signature).unwrap_err();
            assert!(err.to_string().contains("does not match"), "{err}");
        }
    }

    #[test]
    fn every_bundled_certificate_keeps_a_signing_key() {
        // the keyrings are refreshed from upstream; no releaser may end up without a usable key
        for keyring in [
            include_str!("assets/gpg/node.asc"),
            include_str!("assets/gpg/swift.asc"),
        ] {
            let keys = parse_public_keys(keyring).unwrap();
            let trusted = trusted_keys(&keys);
            for key in &keys {
                assert!(
                    trusted.iter().any(|t| std::ptr::eq(t.key, key)),
                    "{:X} has no key that may sign",
                    key.fingerprint()
                );
            }
        }
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
