//! Installation identity: what makes one installed tool distinct from another.
//!
//! The identity hashes the *request* an installation answers (canonical backend,
//! concrete version, platform, install-affecting options and any inputs the
//! request pins), never the bytes currently on disk. Installed tools can change
//! themselves, so the digest is a name, not an integrity check.
//!
//! The encoding is versioned and canonical: every field is length-prefixed and
//! every map is sorted, so two machines (or two releases of mise) that describe
//! the same request produce the same digest. Changing the encoding requires a
//! new [`IDENTITY_FORMAT`] and a new domain tag; existing digests must keep
//! meaning what they meant.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Version of the canonical encoding below. Recorded in every receipt.
pub(crate) const IDENTITY_FORMAT: u32 = 1;
/// Digest algorithm recorded next to every digest.
pub(crate) const IDENTITY_ALGORITHM: &str = "blake3";
/// Domain separation so an identity digest is never confused with another
/// BLAKE3 use in mise.
const DOMAIN: &[u8] = b"mise-install-identity\0";

/// How completely the identity describes what was installed.
///
/// Recovery guarantees follow this: a [`Mode::Resolved`] install can be
/// restored from its recorded inputs, a [`Mode::Fallback`] install only records
/// the request, so rerunning it may produce different bytes. The mode is part
/// of the digest, so a richer identity never silently reinterprets (or takes
/// over) an installation recorded under a poorer one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Mode {
    /// The request pins an artifact (a lockfile checksum or an explicit
    /// `checksum` option), so the identity names the exact bytes expected.
    Resolved,
    /// Canonical backend, version, platform and known options only. Hidden
    /// inputs (an opaque plugin's installer, a moving upstream tag) are not
    /// distinguished.
    #[default]
    Fallback,
}

impl Mode {
    fn tag(self) -> &'static str {
        match self {
            Mode::Resolved => "resolved",
            Mode::Fallback => "fallback",
        }
    }
}

/// The inputs that determine an installation's identity.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) struct InstallIdentity {
    pub(crate) mode: Mode,
    /// Canonical full backend identifier without `[options]`, for example
    /// `aqua:FiloSottile/age`. Never the registry shorthand: adding a shorthand
    /// later must not move an existing install.
    pub(crate) backend: String,
    /// Concrete version, opaque: only ever compared for equality.
    pub(crate) version: String,
    /// Platform key such as `linux-x64` or `linux-x64-musl`.
    pub(crate) platform: String,
    /// Options that change what is installed (artifact selection, build
    /// flags, installer environment, postinstall). Sorted by key.
    pub(crate) options: BTreeMap<String, String>,
    /// Other resolved inputs: artifact checksums a request pins, dependency
    /// graph identities (aube, uv), a refresh generation. Sorted by key.
    pub(crate) inputs: BTreeMap<String, String>,
}

impl InstallIdentity {
    /// The canonical byte encoding that is hashed.
    pub(crate) fn canonical_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(DOMAIN);
        out.extend_from_slice(&IDENTITY_FORMAT.to_le_bytes());
        field(&mut out, b'm', self.mode.tag());
        field(&mut out, b'b', &self.backend);
        field(&mut out, b'v', &self.version);
        field(&mut out, b'p', &self.platform);
        map(&mut out, b'o', &self.options);
        map(&mut out, b'i', &self.inputs);
        out
    }

    /// The full 256-bit digest.
    pub(crate) fn digest(&self) -> Digest {
        Digest(*blake3::hash(&self.canonical_bytes()).as_bytes())
    }

    /// The identity of the request this one answers when nothing is pinned:
    /// the key an unlocked selection is recorded under. Locked requests that
    /// pin nothing extra map to themselves.
    pub(crate) fn request_key(&self) -> InstallIdentity {
        let mut key = self.clone();
        key.mode = Mode::Fallback;
        key.inputs.retain(|k, _| !is_pin_input(k));
        key
    }
}

/// Inputs that pin one artifact. They distinguish installs that coexist but do
/// not distinguish *requests*, so they are dropped from the request key.
pub(crate) fn is_pin_input(key: &str) -> bool {
    key.starts_with("artifact.")
}

fn field(out: &mut Vec<u8>, tag: u8, value: &str) {
    out.push(tag);
    out.extend_from_slice(&(value.len() as u64).to_le_bytes());
    out.extend_from_slice(value.as_bytes());
}

fn map(out: &mut Vec<u8>, tag: u8, entries: &BTreeMap<String, String>) {
    out.push(tag);
    out.extend_from_slice(&(entries.len() as u64).to_le_bytes());
    for (k, v) in entries {
        field(out, b'k', k);
        field(out, b'x', v);
    }
}

/// A 256-bit identity digest.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct Digest([u8; 32]);

/// Length of the full digest in base32 characters (256 bits, unpadded).
pub(crate) const DIGEST_CHARS: usize = 52;
/// Characters of the digest used by a first-choice directory name (40 bits).
pub(crate) const SHORT_CHARS: usize = 8;

impl Digest {
    /// BLAKE3 of arbitrary bytes, for callers that need a stable short name.
    pub(crate) fn of(bytes: &[u8]) -> Self {
        Digest(*blake3::hash(bytes).as_bytes())
    }

    /// Lowercase RFC 4648 base32 (`a-z2-7`), unpadded: 52 characters.
    pub(crate) fn to_base32(self) -> String {
        base32_encode(&self.0)
    }

    pub(crate) fn from_base32(s: &str) -> Option<Self> {
        base32_decode(s).map(Digest)
    }
}

impl std::fmt::Display for Digest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.to_base32())
    }
}

const ALPHABET: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz234567";

fn base32_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity((bytes.len() * 8).div_ceil(5));
    let mut buffer: u32 = 0;
    let mut bits = 0;
    for &byte in bytes {
        buffer = (buffer << 8) | byte as u32;
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(ALPHABET[((buffer >> bits) & 0x1f) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(ALPHABET[((buffer << (5 - bits)) & 0x1f) as usize] as char);
    }
    out
}

fn base32_decode(s: &str) -> Option<[u8; 32]> {
    if s.len() != DIGEST_CHARS {
        return None;
    }
    let mut out = [0u8; 32];
    let mut buffer: u32 = 0;
    let mut bits = 0;
    let mut written = 0;
    for c in s.bytes() {
        let value = ALPHABET.iter().position(|&a| a == c)? as u32;
        buffer = (buffer << 5) | value;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            if written == 32 {
                return None;
            }
            out[written] = ((buffer >> bits) & 0xff) as u8;
            written += 1;
        }
    }
    // 256 bits leave four padding bits that must be zero for a canonical form.
    (written == 32 && (buffer & ((1 << bits) - 1)) == 0).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity() -> InstallIdentity {
        InstallIdentity {
            mode: Mode::Fallback,
            backend: "aqua:FiloSottile/age".into(),
            version: "1.2.1".into(),
            platform: "linux-x64".into(),
            options: BTreeMap::new(),
            inputs: BTreeMap::new(),
        }
    }

    #[test]
    fn base32_matches_rfc4648_vectors() {
        // RFC 4648 section 10 vectors, lowercased and unpadded.
        assert_eq!(base32_encode(b""), "");
        assert_eq!(base32_encode(b"f"), "my");
        assert_eq!(base32_encode(b"fo"), "mzxq");
        assert_eq!(base32_encode(b"foo"), "mzxw6");
        assert_eq!(base32_encode(b"foob"), "mzxw6yq");
        assert_eq!(base32_encode(b"fooba"), "mzxw6ytb");
        assert_eq!(base32_encode(b"foobar"), "mzxw6ytboi");
    }

    #[test]
    fn digest_is_52_lowercase_base32_chars_and_round_trips() {
        let digest = identity().digest();
        let text = digest.to_base32();
        assert_eq!(text.len(), DIGEST_CHARS);
        assert!(
            text.bytes().all(|c| matches!(c, b'a'..=b'z' | b'2'..=b'7')),
            "{text}"
        );
        assert_eq!(Digest::from_base32(&text), Some(digest));
        // Non-canonical trailing bits and wrong lengths are rejected.
        assert_eq!(Digest::from_base32(&text[..51]), None);
        assert_eq!(Digest::from_base32(&format!("{text}a")), None);
        assert_eq!(Digest::from_base32(&text.to_uppercase()), None);
    }

    /// A change here means every installed directory name changes. Bump
    /// IDENTITY_FORMAT instead of editing this vector.
    #[test]
    fn digest_encoding_is_stable() {
        assert_eq!(
            identity().digest().to_base32(),
            "hlencrstldyjkqnpak2zyr453avqxfjf6so4vopt7au5nsxvzska",
            "canonical encoding changed; bump IDENTITY_FORMAT instead"
        );
    }

    #[test]
    fn every_input_changes_the_digest() {
        let base = identity().digest();
        let mut seen = vec![base];
        let mut check = |id: InstallIdentity| {
            let d = id.digest();
            assert!(!seen.contains(&d), "{id:?} collided");
            seen.push(d);
        };
        check(InstallIdentity {
            mode: Mode::Resolved,
            ..identity()
        });
        check(InstallIdentity {
            backend: "github:FiloSottile/age".into(),
            ..identity()
        });
        check(InstallIdentity {
            version: "1.2.2".into(),
            ..identity()
        });
        check(InstallIdentity {
            platform: "linux-x64-musl".into(),
            ..identity()
        });
        check(InstallIdentity {
            options: [("matching".to_string(), "age".to_string())].into(),
            ..identity()
        });
        check(InstallIdentity {
            inputs: [("graph".to_string(), "x".to_string())].into(),
            ..identity()
        });
    }

    #[test]
    fn fields_are_length_prefixed_not_concatenated() {
        // Moving a character across a field boundary must not collide.
        let a = InstallIdentity {
            backend: "ab".into(),
            version: "c".into(),
            ..identity()
        };
        let b = InstallIdentity {
            backend: "a".into(),
            version: "bc".into(),
            ..identity()
        };
        assert_ne!(a.digest(), b.digest());
        let a = InstallIdentity {
            options: [("k".to_string(), "v".to_string())].into(),
            ..identity()
        };
        let b = InstallIdentity {
            inputs: [("k".to_string(), "v".to_string())].into(),
            ..identity()
        };
        assert_ne!(a.digest(), b.digest());
    }

    #[test]
    fn map_insertion_order_does_not_matter() {
        let a = InstallIdentity {
            options: [
                ("a".to_string(), "1".to_string()),
                ("b".to_string(), "2".to_string()),
            ]
            .into(),
            ..identity()
        };
        let mut options = BTreeMap::new();
        options.insert("b".to_string(), "2".to_string());
        options.insert("a".to_string(), "1".to_string());
        let b = InstallIdentity {
            options,
            ..identity()
        };
        assert_eq!(a.digest(), b.digest());
    }

    #[test]
    fn request_key_drops_pins_and_mode() {
        let pinned = InstallIdentity {
            mode: Mode::Resolved,
            inputs: [
                ("artifact.sha256".to_string(), "abc".to_string()),
                ("graph".to_string(), "g".to_string()),
            ]
            .into(),
            ..identity()
        };
        let key = pinned.request_key();
        assert_eq!(key.mode, Mode::Fallback);
        assert_eq!(key.inputs.len(), 1);
        assert!(key.inputs.contains_key("graph"));
        // Pinning one artifact never changes the unlocked request key.
        let other = InstallIdentity {
            inputs: [
                ("artifact.sha256".to_string(), "def".to_string()),
                ("graph".to_string(), "g".to_string()),
            ]
            .into(),
            ..pinned.clone()
        };
        assert_eq!(key.digest(), other.request_key().digest());
        assert_ne!(pinned.digest(), other.digest());
    }
}
