//! On-disk records: what the catalog and a receipt say about an installation.
//!
//! * An [`IdentityRecord`] lives in the catalog (`installs/.mise/`). It binds a
//!   full identity digest to the directory assigned to it, so the path stays
//!   stable even after the payload is pruned, and keeps the provenance needed
//!   to recover the install.
//! * A [`Receipt`] lives inside the installation directory and describes that
//!   one installation. Discovery reads receipts; a lost catalog is rebuilt
//!   from them.
//! * A [`Selection`] remembers which installation satisfies an unlocked
//!   request, so routine use is sticky and only an explicit refresh moves it.
//!
//! None of these authenticate the installed payload. A receipt restored next to
//! a cached tool is not evidence that the tool is genuine.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::identity::{Digest, IDENTITY_ALGORITHM, IDENTITY_FORMAT, InstallIdentity};

/// File name of the receipt inside an installation directory.
pub(crate) const RECEIPT_FILE: &str = ".mise-install.toml";

/// What was acquired for an installation. Provenance describes the inputs the
/// install was made from, not the tree as it is now.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Provenance {
    /// Digests of acquired artifacts keyed by algorithm-qualified name, such as
    /// `sha256`. An unlocked download's digest identifies what was acquired; it
    /// does not authenticate it.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) artifacts: BTreeMap<String, String>,
    /// Digests that lockfiles asked this installation to satisfy. An installation
    /// that a lockfile adopted is never replaced in place by a refresh.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) pinned_by: Vec<String>,
    /// How many times an explicit refresh has had to move the unlocked
    /// selection to a new directory.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub(crate) generation: u32,
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

/// The catalog entry for one identity.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct IdentityRecord {
    pub(crate) format: u32,
    pub(crate) algorithm: String,
    /// The full digest, base32.
    pub(crate) digest: String,
    /// The assigned directory name under the installs root. Once allocated it
    /// is stable: never renamed, even to resolve a later collision.
    pub(crate) dir: String,
    pub(crate) identity: InstallIdentity,
    #[serde(default)]
    pub(crate) provenance: Provenance,
}

impl IdentityRecord {
    pub(crate) fn new(identity: InstallIdentity, dir: String) -> Self {
        Self {
            format: IDENTITY_FORMAT,
            algorithm: IDENTITY_ALGORITHM.to_string(),
            digest: identity.digest().to_base32(),
            dir,
            identity,
            provenance: Provenance::default(),
        }
    }

    pub(crate) fn digest(&self) -> Option<Digest> {
        Digest::from_base32(&self.digest)
    }

    /// A record is trustworthy only if its digest is what its inputs hash to.
    /// A record written by a future encoding version is left alone, not
    /// reinterpreted.
    pub(crate) fn is_consistent(&self) -> bool {
        self.format == IDENTITY_FORMAT
            && self.algorithm == IDENTITY_ALGORITHM
            && self.digest().is_some_and(|d| d == self.identity.digest())
            && is_plain_dir_name(&self.dir)
    }
}

/// The description of one installation, kept inside its directory.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Receipt {
    #[serde(flatten)]
    pub(crate) record: IdentityRecord,
    /// The spelling the tool was requested with when it was installed, for
    /// display. Identity never depends on it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) requested_as: Option<String>,
    /// The mise that wrote the receipt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) mise_version: Option<String>,
}

/// Which installation satisfies an unlocked request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Selection {
    pub(crate) request: InstallIdentity,
    /// Full digest of the selected installation.
    pub(crate) selected: String,
    /// The installs root that holds it, when that is not the root holding this
    /// selection (a read-only shared or system root).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) root: Option<String>,
}

/// A directory name that is a single plain component: it can neither escape
/// the installs root nor name something other than one directory.
pub(crate) fn is_plain_dir_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.starts_with('.')
        && !name.contains(['/', '\\', ':', '\0'])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity() -> InstallIdentity {
        InstallIdentity {
            backend: "aqua:FiloSottile/age".into(),
            version: "1.2.1".into(),
            platform: "linux-x64".into(),
            ..Default::default()
        }
    }

    #[test]
    fn plain_dir_names() {
        assert!(is_plain_dir_name("age-p4n6w2ra"));
        for bad in ["", ".", "..", ".mise", "a/b", "a\\b", "../x", "c:", "a\0b"] {
            assert!(!is_plain_dir_name(bad), "{bad:?}");
        }
    }

    #[test]
    fn receipt_round_trips_through_toml() {
        let mut record = IdentityRecord::new(identity(), "age-p4n6w2ra".into());
        record
            .provenance
            .artifacts
            .insert("sha256".into(), "abc".into());
        record.provenance.pinned_by.push("abc".into());
        let receipt = Receipt {
            record,
            requested_as: Some("age".into()),
            mise_version: Some("2026.10.0".into()),
        };
        let text = toml::to_string_pretty(&receipt).unwrap();
        let back: Receipt = toml::from_str(&text).unwrap();
        assert_eq!(back, receipt);
        assert!(back.record.is_consistent());
    }

    #[test]
    fn tampered_records_are_inconsistent() {
        let mut record = IdentityRecord::new(identity(), "age-p4n6w2ra".into());
        assert!(record.is_consistent());
        record.identity.version = "9.9.9".into();
        assert!(!record.is_consistent());
        let mut record = IdentityRecord::new(identity(), "../escape".into());
        assert!(!record.is_consistent());
        record.dir = "ok-dir".into();
        assert!(record.is_consistent());
        record.format = 99;
        assert!(!record.is_consistent());
    }
}
