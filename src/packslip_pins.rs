//! What mise remembers about each packslip project's signer, the way SSH
//! remembers hosts: the identity that signed the first release it
//! accepted, and the things the specification says a consumer never lets
//! get weaker without a person's say-so. The file lives in the state dir;
//! it is this machine's memory, not something to sync.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use eyre::{Result, WrapErr, bail};
use serde::{Deserialize, Serialize};

use crate::{dirs, file};

/// Where the pins live.
pub(crate) fn pins_file() -> PathBuf {
    dirs::STATE.join("packslip").join("pins.toml")
}

/// The signer a project's releases are accepted from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Pin {
    /// `sigstore-oidc` or `sigstore-key`.
    pub scheme: String,
    /// A workflow path without its ref, or a key id: see [`signer_of`].
    pub signer: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issuer: Option<String>,
    /// `vendor` or `repackager`.
    pub attested_by: String,
    /// Whether every artifact of an accepted release linked provenance.
    #[serde(default)]
    pub provenance: bool,
    /// Whether a bundle with no transparency log entry was ever accepted.
    #[serde(default)]
    pub unlogged: bool,
    /// RFC 3339, when the pin was set.
    pub pinned_at: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Pins {
    #[serde(default)]
    pins: BTreeMap<String, Pin>,
    /// The highest release-list sequence accepted per project, so a
    /// mirror cannot show an older list than this machine has seen.
    #[serde(default)]
    sequences: BTreeMap<String, u64>,
}

/// Hold the file lock for a read-modify-write of the pins file, so two
/// installs running at once cannot drop each other's pin or sequence.
/// A lock beside the pins file itself, not under the cache directory:
/// every process that shares the state directory must share the lock,
/// whatever its cache directory is.
fn locked(path: &Path) -> Result<fslock::LockFile> {
    if let Some(parent) = path.parent() {
        file::create_dir_all(parent)?;
    }
    let lock_path = path.with_extension("lock");
    let mut lock = fslock::LockFile::open(&lock_path)?;
    if !lock.try_lock()? {
        debug!("waiting for lock on {}", lock_path.display());
        lock.lock()?;
    }
    Ok(lock)
}

fn load(path: &Path) -> Result<Pins> {
    if !path.is_file() {
        return Ok(Pins::default());
    }
    let text = file::read_to_string(path)?;
    toml::from_str(&text).wrap_err_with(|| {
        format!(
            "{} is not valid; it records which signers mise accepts packslips from. Fix or remove it",
            path.display()
        )
    })
}

fn save(path: &Path, pins: &Pins) -> Result<()> {
    if let Some(parent) = path.parent() {
        file::create_dir_all(parent)?;
    }
    file::write_atomic(path, toml::to_string_pretty(pins)?)
}

/// What a verified packslip showed about who signed it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Observed<'a> {
    pub scheme: &'a str,
    pub key_id: &'a str,
    pub issuer: Option<&'a str>,
    pub attested_by: &'a str,
    pub provenance: bool,
    pub logged: bool,
}

/// The signer a pin records for a key id. For a workflow identity that is
/// the path without its ref, as the specification says: a new tag of the
/// same workflow is the same signer. A key is its id.
///
/// Only a workflow identity has a ref to drop, and it is a URL whose ref
/// comes after the last `@`. Every other keyless identity — an email, a
/// SPIFFE URI — can carry `@` as part of itself, and cutting there would
/// file `alice@example.com` and `alice@example.invalid` under one signer,
/// so the second would pass as a signer already accepted.
pub(crate) fn signer_of(scheme: &str, key_id: &str) -> String {
    if scheme == "sigstore-oidc"
        && key_id.starts_with("https://")
        && let Some((path, _ref)) = key_id.rsplit_once('@')
    {
        return path.to_string();
    }
    key_id.to_string()
}

/// Compare what a release showed with the project's pin, refusing what
/// the specification calls a downgrade. Writes nothing: a release is
/// recorded with [`record`] only once everything else about it has been
/// accepted, so a refused install never leaves a pin behind.
pub(crate) fn check(project: &str, observed: Observed<'_>) -> Result<()> {
    check_at(&pins_file(), project, observed)
}

pub(crate) fn check_at(path: &Path, project: &str, observed: Observed<'_>) -> Result<()> {
    let pins = load(path)?;
    match pins.pins.get(project) {
        Some(pin) => check_against(pin, project, observed),
        None => Ok(()),
    }
}

fn check_against(pin: &Pin, project: &str, observed: Observed<'_>) -> Result<()> {
    let signer = signer_of(observed.scheme, observed.key_id);
    let mut problems = Vec::new();
    if pin.scheme != observed.scheme || pin.signer != signer {
        problems.push(format!(
            "is signed by {signer} ({}), but {} ({}) signed what mise accepted before",
            observed.scheme, pin.signer, pin.scheme
        ));
    }
    if pin.attested_by == "vendor" && observed.attested_by == "repackager" {
        problems.push(
            "is attested by a repackager, but the vendor's own packslip was accepted before".into(),
        );
    }
    if pin.provenance && !observed.provenance {
        problems.push("drops the build provenance every artifact linked before".into());
    }
    if !problems.is_empty() {
        bail!(
            "packslip:{project}: this release {}.\n\nIf the vendor announced the change, run `mise packslip forget {project}` and install again; the next release accepted sets the pin.",
            problems.join(", and ")
        );
    }
    Ok(())
}

/// Set the project's pin from an accepted release, or strengthen it: what
/// got stronger is remembered, what stayed the same is left alone. Checks
/// again under the lock, since the file may have changed since [`check`].
pub(crate) fn record(project: &str, observed: Observed<'_>) -> Result<Pin> {
    record_at(&pins_file(), project, observed)
}

pub(crate) fn record_at(path: &Path, project: &str, observed: Observed<'_>) -> Result<Pin> {
    let _lock = locked(path)?;
    let mut pins = load(path)?;
    let signer = signer_of(observed.scheme, observed.key_id);
    let Some(pin) = pins.pins.get(project).cloned() else {
        let pin = Pin {
            scheme: observed.scheme.to_string(),
            signer,
            issuer: observed.issuer.map(str::to_string),
            attested_by: observed.attested_by.to_string(),
            provenance: observed.provenance,
            unlogged: !observed.logged,
            pinned_at: jiff::Timestamp::now().to_string(),
        };
        pins.pins.insert(project.to_string(), pin.clone());
        save(path, &pins)?;
        return Ok(pin);
    };
    check_against(&pin, project, observed)?;
    let updated = Pin {
        issuer: observed.issuer.map(str::to_string).or(pin.issuer.clone()),
        attested_by: observed.attested_by.to_string(),
        provenance: pin.provenance || observed.provenance,
        unlogged: pin.unlogged || !observed.logged,
        ..pin.clone()
    };
    if updated != pin {
        pins.pins.insert(project.to_string(), updated.clone());
        save(path, &pins)?;
    }
    Ok(updated)
}

/// Refuse a release list whose sequence is below one already accepted for
/// the project, and remember the highest seen.
pub(crate) fn check_sequence(project: &str, sequence: u64) -> Result<()> {
    check_sequence_at(&pins_file(), project, sequence)
}

pub(crate) fn check_sequence_at(path: &Path, project: &str, sequence: u64) -> Result<()> {
    let _lock = locked(path)?;
    let mut pins = load(path)?;
    if let Some(last) = pins.sequences.get(project).copied()
        && sequence < last
    {
        bail!(
            "the release list of packslip:{project} has sequence {sequence}, but sequence {last} was already accepted; refusing to go back"
        );
    }
    if pins.sequences.get(project) != Some(&sequence) {
        pins.sequences.insert(project.to_string(), sequence);
        save(path, &pins)?;
    }
    Ok(())
}

/// Every pin, by project.
pub(crate) fn list() -> Result<BTreeMap<String, Pin>> {
    Ok(load(&pins_file())?.pins)
}

/// Drop a project's pin and sequence, so the next release accepted sets
/// them again. Returns whether there was one.
pub(crate) fn forget(project: &str) -> Result<bool> {
    forget_at(&pins_file(), project)
}

pub(crate) fn forget_at(path: &Path, project: &str) -> Result<bool> {
    let _lock = locked(path)?;
    let mut pins = load(path)?;
    let had = pins.pins.remove(project).is_some() | pins.sequences.remove(project).is_some();
    if had {
        save(path, &pins)?;
    }
    Ok(had)
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORKFLOW: &str = "https://github.com/o/r/.github/workflows/release.yml";

    fn oidc(tag: &str) -> String {
        format!("{WORKFLOW}@refs/tags/{tag}")
    }

    fn observed<'a>(scheme: &'a str, key_id: &'a str) -> Observed<'a> {
        Observed {
            scheme,
            key_id,
            issuer: Some("https://token.actions.githubusercontent.com"),
            attested_by: "vendor",
            provenance: false,
            logged: true,
        }
    }

    #[test]
    fn a_new_tag_of_the_same_workflow_is_the_same_signer() {
        assert_eq!(signer_of("sigstore-oidc", &oidc("v1")), WORKFLOW);
        assert_eq!(signer_of("sigstore-key", "5A0A"), "5A0A");
        // Only a workflow identity has a ref to drop. An identity that is
        // itself an email or a URI keeps every character of itself, or two
        // signers sharing a local part would share one pin.
        for identity in [
            "alice@example.com",
            "alice@example.invalid",
            "spiffe://example.com/ns/ci/sa/build@v2",
        ] {
            assert_eq!(signer_of("sigstore-oidc", identity), identity);
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pins.toml");
        let v1 = oidc("v1");
        check_at(&path, "github.com/o/r", observed("sigstore-oidc", &v1)).unwrap();
        assert!(!path.exists(), "a check writes nothing");
        let pin = record_at(&path, "github.com/o/r", observed("sigstore-oidc", &v1)).unwrap();
        assert_eq!(pin.signer, WORKFLOW);
        let v2 = oidc("v2");
        let again = record_at(&path, "github.com/o/r", observed("sigstore-oidc", &v2)).unwrap();
        assert_eq!(again, pin, "nothing changed, nothing rewritten");
        assert!(
            file::read_to_string(&path)
                .unwrap()
                .contains("[pins.\"github.com/o/r\"]")
        );
    }

    #[test]
    fn downgrades_are_refused_and_upgrades_remembered() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pins.toml");
        let project = "github.com/o/r";
        let v1 = oidc("v1");
        let strong = Observed {
            provenance: true,
            ..observed("sigstore-oidc", &v1)
        };
        // The first release sets the pin with no provenance; provenance
        // arriving later is remembered as the new floor.
        record_at(&path, project, observed("sigstore-oidc", &v1)).unwrap();
        let pin = record_at(&path, project, strong).unwrap();
        assert!(pin.provenance);

        let other = "https://github.com/o/r/.github/workflows/other.yml@refs/tags/v3";
        let err = record_at(
            &path,
            project,
            Observed {
                provenance: true,
                ..observed("sigstore-oidc", other)
            },
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("signed what mise accepted before"),
            "{err}"
        );
        assert!(err.to_string().contains("mise packslip forget"), "{err}");

        let keyed = Observed {
            provenance: true,
            ..observed("sigstore-key", "5A0A")
        };
        assert!(
            record_at(&path, project, keyed).is_err(),
            "a scheme change is a signer change"
        );

        let dropped = observed("sigstore-oidc", &v1);
        let err = record_at(&path, project, dropped).unwrap_err();
        assert!(
            err.to_string().contains("drops the build provenance"),
            "{err}"
        );

        let repackaged = Observed {
            attested_by: "repackager",
            provenance: true,
            ..observed("sigstore-oidc", &v1)
        };
        let err = record_at(&path, project, repackaged).unwrap_err();
        assert!(err.to_string().contains("repackager"), "{err}");

        assert_eq!(
            load(&path).unwrap().pins[project].signer,
            WORKFLOW,
            "a refused release leaves no mark"
        );

        // Forgetting lets a new signer in, once.
        assert!(forget_at(&path, project).unwrap());
        assert!(!forget_at(&path, project).unwrap());
        record_at(&path, project, keyed).unwrap();
        assert_eq!(load(&path).unwrap().pins[project].scheme, "sigstore-key");
    }

    #[test]
    fn a_repackager_pin_yields_to_the_vendor() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pins.toml");
        let v1 = oidc("v1");
        let repackaged = Observed {
            attested_by: "repackager",
            ..observed("sigstore-oidc", &v1)
        };
        record_at(&path, "p.example.com", repackaged).unwrap();
        let pin = record_at(&path, "p.example.com", observed("sigstore-oidc", &v1)).unwrap();
        assert_eq!(pin.attested_by, "vendor");
    }

    #[test]
    fn sequences_only_go_up() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pins.toml");
        check_sequence_at(&path, "t.example.com", 3).unwrap();
        check_sequence_at(&path, "t.example.com", 5).unwrap();
        let err = check_sequence_at(&path, "t.example.com", 4).unwrap_err();
        assert!(err.to_string().contains("refusing to go back"), "{err}");
        check_sequence_at(&path, "t.example.com", 5).unwrap();
        check_sequence_at(&path, "other.example.com", 1).unwrap();
        assert!(forget_at(&path, "t.example.com").unwrap());
        check_sequence_at(&path, "t.example.com", 1).unwrap();
    }

    #[test]
    fn a_malformed_file_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pins.toml");
        file::write(&path, "not = [toml").unwrap();
        let v1 = oidc("v1");
        let err = record_at(&path, "github.com/o/r", observed("sigstore-oidc", &v1)).unwrap_err();
        assert!(err.to_string().contains("is not valid"), "{err}");
    }
}
