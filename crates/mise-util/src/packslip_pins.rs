//! What mise remembers about each packslip project's signer, the way SSH
//! remembers hosts: the identity that signed the first release it
//! accepted, and the things the specification says a consumer never lets
//! get weaker without a person's say-so. The file lives in the state dir;
//! it is this machine's memory, not something to sync.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use eyre::{Result, WrapErr, bail};
use packslip::forge::{Check, ForgePin};
use serde::{Deserialize, Serialize};

use crate::{dirs, file};

/// Where the pins live.
pub fn pins_file() -> PathBuf {
    dirs::STATE.join("packslip").join("pins.toml")
}

/// The signer a project's releases are accepted from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pin {
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
    /// For a GitHub or GitLab project, the forge's repository and owner IDs
    /// from the last accepted release, and the name it was signed under. A
    /// rename keeps the IDs, so the pin follows the repository rather than
    /// the name. Pins written before mise recorded it have none, and gain
    /// it from the next release accepted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forge: Option<ForgePin>,
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
pub struct Observed<'a> {
    pub scheme: &'a str,
    pub key_id: &'a str,
    pub issuer: Option<&'a str>,
    pub attested_by: &'a str,
    pub provenance: bool,
    pub logged: bool,
    /// The forge check the release passed, for a GitHub or GitLab project
    /// verified by repository ID. With it, a signer is the pinned one when it
    /// is the same workflow of the same repository under a new name.
    pub forge: Option<&'a Check>,
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
pub fn signer_of(scheme: &str, key_id: &str) -> String {
    if scheme == "sigstore-oidc"
        && key_id.starts_with("https://")
        && let Some((path, _ref)) = key_id.rsplit_once('@')
    {
        return path.to_string();
    }
    key_id.to_string()
}

/// Whether two signers, as a pin or a lockfile records them (`scheme:` and
/// all, or without it), are the same workflow inside their repository. Only
/// meaningful once the forge's repository ID has shown the repository is the
/// same one, whatever it was called when each was recorded.
pub fn same_workflow(a: &str, b: &str) -> bool {
    fn path(signer: &str) -> Option<(&str, &str)> {
        let signer = signer.strip_prefix("sigstore-oidc:").unwrap_or(signer);
        let rest = signer.strip_prefix("https://")?;
        let rest = rest.rsplit_once('@').map_or(rest, |(path, _)| path);
        if let Some(path) = rest.strip_prefix("github.com/") {
            let mut parts = path.splitn(3, '/');
            let (_owner, _repo, file) = (parts.next()?, parts.next()?, parts.next()?);
            return Some(("github.com", file));
        }
        let path = rest.strip_prefix("gitlab.com/")?;
        let (_project, file) = path.split_once("//")?;
        Some(("gitlab.com", file))
    }
    matches!((path(a), path(b)), (Some(a), Some(b)) if a == b)
}

/// The pin for `project`: its own, or one recorded under an older name for
/// a repository that was accepted as `project` after a rename, so that
/// following the rename in a config keeps what was pinned.
fn find<'a>(pins: &'a Pins, project: &str) -> Option<&'a Pin> {
    pins.pins.get(project).or_else(|| {
        pins.pins
            .values()
            .find(|pin| pin.forge.as_ref().is_some_and(|f| f.project == project))
    })
}

/// The forge identity pinned for `project`, if any.
pub fn forge_pin(project: &str) -> Result<Option<ForgePin>> {
    forge_pin_at(&pins_file(), project)
}

pub fn forge_pin_at(path: &Path, project: &str) -> Result<Option<ForgePin>> {
    Ok(find(&load(path)?, project).and_then(|pin| pin.forge.clone()))
}

/// Compare what a release showed with the project's pin, refusing what
/// the specification calls a downgrade. Writes nothing: a release is
/// recorded with [`record`] only once everything else about it has been
/// accepted, so a refused install never leaves a pin behind.
pub fn check(project: &str, observed: Observed<'_>) -> Result<()> {
    check_at(&pins_file(), project, observed)
}

pub fn check_at(path: &Path, project: &str, observed: Observed<'_>) -> Result<()> {
    let pins = load(path)?;
    match find(&pins, project) {
        Some(pin) => check_against(pin, project, observed),
        None => Ok(()),
    }
}

fn check_against(pin: &Pin, project: &str, observed: Observed<'_>) -> Result<()> {
    let signer = signer_of(observed.scheme, observed.key_id);
    let mut problems = Vec::new();
    let same_signer = pin.signer == signer
        || observed
            .forge
            .is_some_and(|check| check.continues_signer(&pin.signer));
    if pin.scheme != observed.scheme || !same_signer {
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
pub fn record(project: &str, observed: Observed<'_>) -> Result<Pin> {
    record_at(&pins_file(), project, observed)
}

pub fn record_at(path: &Path, project: &str, observed: Observed<'_>) -> Result<Pin> {
    let _lock = locked(path)?;
    let mut pins = load(path)?;
    let signer = signer_of(observed.scheme, observed.key_id);
    let forge = observed.forge.and_then(|check| check.pin.clone());
    let Some(pin) = find(&pins, project).cloned() else {
        let pin = Pin {
            scheme: observed.scheme.to_string(),
            signer,
            issuer: observed.issuer.map(str::to_string),
            attested_by: observed.attested_by.to_string(),
            provenance: observed.provenance,
            unlogged: !observed.logged,
            pinned_at: jiff::Timestamp::now().to_string(),
            forge,
        };
        pins.pins.insert(project.to_string(), pin.clone());
        save(path, &pins)?;
        return Ok(pin);
    };
    check_against(&pin, project, observed)?;
    let updated = Pin {
        // A forge check that passed says this is the pinned workflow, perhaps
        // under the repository's new name: remember it as it is called now.
        signer: if forge.is_some() {
            signer
        } else {
            pin.signer.clone()
        },
        issuer: observed.issuer.map(str::to_string).or(pin.issuer.clone()),
        attested_by: observed.attested_by.to_string(),
        provenance: pin.provenance || observed.provenance,
        unlogged: pin.unlogged || !observed.logged,
        forge: forge.or(pin.forge.clone()),
        ..pin.clone()
    };
    if pins.pins.get(project) != Some(&updated) {
        pins.pins.insert(project.to_string(), updated.clone());
        save(path, &pins)?;
    }
    Ok(updated)
}

/// Refuse a release list whose sequence is below one already accepted for
/// the project, and remember the highest seen.
pub fn check_sequence(project: &str, sequence: u64) -> Result<()> {
    check_sequence_at(&pins_file(), project, sequence)
}

pub fn check_sequence_at(path: &Path, project: &str, sequence: u64) -> Result<()> {
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

/// An absent supplementary list is allowed only before this machine has
/// accepted one. Its disappearance must not undo signed withdrawals.
pub fn check_missing_list(project: &str) -> Result<()> {
    check_missing_list_at(&pins_file(), project)
}

fn check_missing_list_at(path: &Path, project: &str) -> Result<()> {
    let _lock = locked(path)?;
    if load(path)?.sequences.contains_key(project) {
        bail!(
            "the signed release list of packslip:{project} disappeared; restore the list or explicitly forget this project's packslip pin"
        );
    }
    Ok(())
}

/// Every pin, by project.
pub fn list() -> Result<BTreeMap<String, Pin>> {
    Ok(load(&pins_file())?.pins)
}

/// Drop a project's pin and sequence, so the next release accepted sets
/// them again. Returns whether there was one.
pub fn forget(project: &str) -> Result<bool> {
    forget_at(&pins_file(), project)
}

pub fn forget_at(path: &Path, project: &str) -> Result<bool> {
    let _lock = locked(path)?;
    let mut pins = load(path)?;
    let before = pins.pins.len();
    // A pin found for the project under an older name goes too, or `find`
    // would keep answering with it.
    pins.pins.retain(|name, pin| {
        name != project && pin.forge.as_ref().is_none_or(|f| f.project != project)
    });
    let had = (pins.pins.len() != before) | pins.sequences.remove(project).is_some();
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
            forge: None,
        }
    }

    #[test]
    fn an_accepted_list_cannot_disappear_until_forgotten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pins.toml");
        check_missing_list_at(&path, "github.com/o/r").unwrap();
        check_sequence_at(&path, "github.com/o/r", 0).unwrap();
        assert!(check_missing_list_at(&path, "github.com/o/r").is_err());
        check_missing_list_at(&path, "github.com/o/other").unwrap();
        assert!(forget_at(&path, "github.com/o/r").unwrap());
        check_missing_list_at(&path, "github.com/o/r").unwrap();
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

    #[test]
    fn a_pin_written_before_forge_ids_still_reads_and_gains_none_by_name() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pins.toml");
        file::write(
            &path,
            format!(
                r#"[pins."github.com/o/r"]
scheme = "sigstore-oidc"
signer = "{WORKFLOW}"
issuer = "https://token.actions.githubusercontent.com"
attested_by = "vendor"
provenance = false
unlogged = false
pinned_at = "2026-09-01T00:00:00Z"

[sequences]
"github.com/o/r" = 3
"#
            ),
        )
        .unwrap();
        assert_eq!(forge_pin_at(&path, "github.com/o/r").unwrap(), None);
        let v2 = oidc("v2");
        check_at(&path, "github.com/o/r", observed("sigstore-oidc", &v2)).unwrap();
        let pin = record_at(&path, "github.com/o/r", observed("sigstore-oidc", &v2)).unwrap();
        assert_eq!(pin.forge, None, "nothing to record without a forge check");
        assert_eq!(pin.pinned_at, "2026-09-01T00:00:00Z");
        assert!(
            !file::read_to_string(&path).unwrap().contains("forge"),
            "an unchanged pin is not rewritten"
        );
        assert_eq!(load(&path).unwrap().sequences["github.com/o/r"], 3);
    }

    #[test]
    fn a_forge_pin_round_trips_and_is_found_under_its_new_name() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pins.toml");
        let mut pins = Pins::default();
        pins.pins.insert(
            "github.com/old/r".into(),
            Pin {
                scheme: "sigstore-oidc".into(),
                signer: "https://github.com/new/r/.github/workflows/release.yml".into(),
                issuer: None,
                attested_by: "vendor".into(),
                provenance: false,
                unlogged: false,
                pinned_at: "2026-09-01T00:00:00Z".into(),
                forge: Some(ForgePin::new("github.com/new/r", "42", Some("7".into()))),
            },
        );
        save(&path, &pins).unwrap();
        let text = file::read_to_string(&path).unwrap();
        assert!(text.contains("[pins.\"github.com/old/r\".forge]"), "{text}");
        assert!(text.contains("repository_id = \"42\""), "{text}");
        let pin = ForgePin::new("github.com/new/r", "42", Some("7".into()));
        assert_eq!(
            forge_pin_at(&path, "github.com/old/r").unwrap().as_ref(),
            Some(&pin)
        );
        // A config that follows the rename keeps what was pinned.
        assert_eq!(
            forge_pin_at(&path, "github.com/new/r").unwrap().as_ref(),
            Some(&pin)
        );
        assert_eq!(forge_pin_at(&path, "github.com/other/r").unwrap(), None);
        // Forgetting the new name forgets the pin found under it.
        assert!(forget_at(&path, "github.com/new/r").unwrap());
        assert_eq!(forge_pin_at(&path, "github.com/old/r").unwrap(), None);
    }

    #[test]
    fn workflows_are_compared_within_their_repository() {
        let release =
            |repo: &str| format!("https://github.com/{repo}/.github/workflows/release.yml");
        assert!(same_workflow(&release("old/r"), &release("new/r")));
        assert!(same_workflow(
            &format!("sigstore-oidc:{}", release("old/r")),
            &format!("{}@refs/tags/v2", release("new/r"))
        ));
        assert!(!same_workflow(
            &release("o/r"),
            "https://github.com/o/r/.github/workflows/other.yml"
        ));
        assert!(same_workflow(
            "https://gitlab.com/g/old//.gitlab-ci.yml",
            "https://gitlab.com/g/sub/new//.gitlab-ci.yml@refs/tags/v1"
        ));
        assert!(!same_workflow(
            "https://gitlab.com/g/r//.github/workflows/release.yml",
            &release("g/r")
        ));
        assert!(!same_workflow("sigstore-key:5A0A", "sigstore-key:5A0A"));
        assert!(!same_workflow("alice@example.com", "alice@example.com"));
    }
}
