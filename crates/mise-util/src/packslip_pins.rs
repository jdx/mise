//! What mise remembers about each packslip project's signer, the way SSH
//! remembers hosts: the identity that signed the first release it
//! accepted, and the things the specification says a consumer never lets
//! get weaker without a person's say-so. The file lives in the state dir;
//! it is this machine's memory, not something to sync.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use eyre::{Report, Result, WrapErr, bail, eyre};
use packslip::forge::{self, Check, Expected, ForgePin, IdentityError};
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
    /// For a GitHub or GitLab project, the forge's repository ID from the
    /// last accepted release, and the name it was signed under. A rename or a
    /// transfer to another owner keeps the ID, so the pin follows the
    /// repository rather than the name. Pins written before mise recorded it
    /// have none, and gain it from the next release accepted. An owner ID
    /// that older pins recorded is still read, and ignored.
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
    /// For a GitHub or GitLab project, the forge identity of the release
    /// list whose sequence is recorded under the same name, so that the
    /// sequence follows the repository through a rename the way a pin does.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    list_forges: BTreeMap<String, ForgePin>,
}

/// Whether two forge identities are the same repository, and in a GitHub
/// monorepo the same tool in it: the same forge, repository ID and subpath.
/// A rename changes none of them.
fn same_repository(a: &ForgePin, b: &ForgePin) -> bool {
    fn host(project: &str) -> &str {
        project.split('/').next().unwrap_or_default()
    }
    a.repository_id == b.repository_id
        && host(&a.project) == host(&b.project)
        && packslip::model::repository_subpath(&a.project)
            == packslip::model::repository_subpath(&b.project)
}

impl Pins {
    /// Every forge identity recorded, a pin's or a release list's, with the
    /// name it is recorded under.
    fn forges(&self) -> impl Iterator<Item = (&str, &ForgePin)> {
        let pins = self
            .pins
            .iter()
            .filter_map(|(key, pin)| Some((key.as_str(), pin.forge.as_ref()?)));
        let lists = self
            .list_forges
            .iter()
            .map(|(key, forge)| (key.as_str(), forge));
        pins.chain(lists)
    }

    /// The names whose pins and release-list sequences speak for `project`:
    /// its own, and any other a rename left them under. Another name is the
    /// same repository by its forge ID: the one a verified certificate shows
    /// (`verified`), or, with none, the IDs recorded under `project`'s own
    /// name or for a release last signed as `project`. A name can only be
    /// added, never taken away, so what is found here is held to in full.
    fn repository_keys(&self, project: &str, verified: Option<&ForgePin>) -> BTreeSet<String> {
        let mut keys = BTreeSet::from([project.to_string()]);
        let ids: Vec<&ForgePin> = match verified {
            Some(verified) => vec![verified],
            None => self
                .forges()
                .filter(|(key, forge)| *key == project || forge.project == project)
                .map(|(_, forge)| forge)
                .collect(),
        };
        for (key, forge) in self.forges() {
            if ids.iter().any(|id| same_repository(id, forge)) {
                keys.insert(key.to_string());
            }
        }
        keys
    }

    /// The names of the pins that speak for `project`, its own first: every
    /// one of [`Self::repository_keys`] that has a pin, and one recorded for
    /// a release last signed as `project`, which is how a config that
    /// followed a rename finds the pin the old name set.
    fn pin_keys(&self, project: &str, verified: Option<&ForgePin>) -> Vec<String> {
        let mut keys = self.repository_keys(project, verified);
        keys.extend(
            self.pins
                .iter()
                .filter(|(_, pin)| pin.forge.as_ref().is_some_and(|f| f.project == project))
                .map(|(key, _)| key.clone()),
        );
        let mut keys: Vec<String> = keys
            .into_iter()
            .filter(|key| self.pins.contains_key(key))
            .collect();
        keys.sort_by_key(|key| key.as_str() != project);
        keys
    }

    /// Move what a release list left under `from` to `to`, keeping the
    /// higher sequence, so a pin that follows a rename takes it along.
    fn move_list(&mut self, from: &str, to: &str) {
        if let Some(sequence) = self.sequences.remove(from) {
            let to = self.sequences.entry(to.to_string()).or_default();
            *to = (*to).max(sequence);
        }
        if let Some(forge) = self.list_forges.remove(from) {
            // What the list under the new name recorded is the newer.
            let kept = self.list_forges.remove(to).unwrap_or(forge);
            self.list_forges.insert(to.to_string(), kept);
        }
    }
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

/// Whether the signer of a release that passed `check` is `previous`, a
/// signer mise recorded for the project: the same workflow of the same
/// repository, perhaps under a new name. A release can declare
/// `pin_workflow: false` to be held to its repository alone, but mise records
/// no person's acceptance of that, so it holds every release to the workflow
/// it pinned.
pub fn continues_signer(check: &Check, previous: &str) -> bool {
    check.pins_workflow() && check.continues_signer(previous)
}

/// The forge identity pinned for `project`, if any.
pub fn forge_pin(project: &str) -> Result<Option<ForgePin>> {
    forge_pin_at(&pins_file(), project)
}

pub fn forge_pin_at(path: &Path, project: &str) -> Result<Option<ForgePin>> {
    let pins = load(path)?;
    Ok(pins
        .pin_keys(project, None)
        .first()
        .and_then(|key| pins.pins[key].forge.clone()))
}

/// Compare what a release showed with the project's pin, refusing what
/// the specification calls a downgrade. Writes nothing: a release is
/// recorded with [`record`] only once everything else about it has been
/// accepted, so a refused install never leaves a pin behind.
///
/// The pin is the project's own, or one a rename left under another name:
/// the certificate's repository ID finds it, whether the config or the
/// release was the first to use the new name, and it is held to in full.
pub fn check(project: &str, observed: Observed<'_>) -> Result<()> {
    check_at(&pins_file(), project, observed)
}

pub fn check_at(path: &Path, project: &str, observed: Observed<'_>) -> Result<()> {
    let pins = load(path)?;
    let verified = observed.forge.and_then(|check| check.pin.as_ref());
    for key in pins.pin_keys(project, verified) {
        check_against(&pins.pins[&key], &key, project, observed)?;
    }
    Ok(())
}

/// Hold a release to one pin, recorded under `key`, which is `project` or
/// the name a rename left the pin under.
fn check_against(pin: &Pin, key: &str, project: &str, observed: Observed<'_>) -> Result<()> {
    // A pin found under another name holds the release to the repository
    // it recorded, as one found by name does. The pin's repository ID is
    // what shows the release is that repository, so the signer is compared
    // with it: the same workflow path, whatever the repository is called now.
    let by_pin = match (&pin.forge, observed.forge) {
        (Some(pinned), Some(check)) => match &check.pin {
            Some(signed) => Some(
                forge::check(
                    &Expected::new(project).pinned(Some(pinned)),
                    &signed.project,
                    observed.key_id,
                    observed.issuer,
                    check.source.as_ref(),
                )
                .map_err(|err| forge_refusal(project, key, err))?,
            ),
            None => None,
        },
        _ => None,
    };
    let signer = signer_of(observed.scheme, observed.key_id);
    let mut problems = Vec::new();
    // A check made from the pin always holds the workflow; going through
    // `continues_signer` keeps both arms on mise's one rule regardless.
    let same_signer = pin.signer == signer
        || by_pin
            .as_ref()
            .or(observed.forge)
            .is_some_and(|check| continues_signer(check, &pin.signer));
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
            "packslip:{project}: this release {}{}.\n\nIf the vendor announced the change, run `mise packslip forget {key}` and install again; the next release accepted sets the pin.",
            problems.join(", and "),
            pinned_as(key, project)
        );
    }
    Ok(())
}

/// Where a pin found under another name was recorded, for a refusal.
fn pinned_as(key: &str, project: &str) -> String {
    if key == project {
        String::new()
    } else {
        format!(" (mise pinned the repository as packslip:{key}, before it was renamed)")
    }
}

/// A release that is not the repository a pin recorded.
fn forge_refusal(project: &str, key: &str, err: IdentityError) -> Report {
    eyre!(
        "packslip:{project}: this release is not from the repository mise pinned as packslip:{key}: {err}.\n\n\
         If the vendor announced the change, run `mise packslip forget {key}` and install again."
    )
}

/// Set the project's pin from an accepted release, or strengthen it: what
/// got stronger is remembered, what stayed the same is left alone. Checks
/// again under the lock, since the file may have changed since [`check`].
/// A pin a rename left under another name moves to `project`, with its
/// release list's sequence, so one repository keeps one pin.
pub fn record(project: &str, observed: Observed<'_>) -> Result<Pin> {
    record_at(&pins_file(), project, observed)
}

pub fn record_at(path: &Path, project: &str, observed: Observed<'_>) -> Result<Pin> {
    let _lock = locked(path)?;
    let mut pins = load(path)?;
    let signer = signer_of(observed.scheme, observed.key_id);
    let forge = observed.forge.and_then(|check| check.pin.clone());
    let keys = pins.pin_keys(project, forge.as_ref());
    let Some(first) = keys.first() else {
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
    for key in &keys {
        check_against(&pins.pins[key], key, project, observed)?;
    }
    let pin = &pins.pins[first];
    let any = |floor: fn(&Pin) -> bool| keys.iter().any(|key| floor(&pins.pins[key]));
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
        provenance: observed.provenance || any(|pin| pin.provenance),
        unlogged: !observed.logged || any(|pin| pin.unlogged),
        forge: forge.or(pin.forge.clone()),
        ..pin.clone()
    };
    let moved: Vec<&String> = keys.iter().filter(|key| key.as_str() != project).collect();
    if !moved.is_empty() || pins.pins.get(project) != Some(&updated) {
        for key in moved {
            pins.pins.remove(key);
            pins.move_list(key, project);
        }
        pins.pins.insert(project.to_string(), updated.clone());
        save(path, &pins)?;
    }
    Ok(updated)
}

/// Refuse a release list whose sequence is below one already accepted for
/// the project, and remember the highest seen. `forge` is the repository a
/// GitHub or GitLab project's list was verified as signed by: a sequence
/// accepted under another name for it counts, and moves to `project`.
pub fn check_sequence(project: &str, sequence: u64, forge: Option<&ForgePin>) -> Result<()> {
    check_sequence_at(&pins_file(), project, sequence, forge)
}

pub fn check_sequence_at(
    path: &Path,
    project: &str,
    sequence: u64,
    forge: Option<&ForgePin>,
) -> Result<()> {
    let _lock = locked(path)?;
    let mut pins = load(path)?;
    let keys = pins.repository_keys(project, forge);
    if let Some((key, last)) = keys
        .iter()
        .filter_map(|key| Some((key, *pins.sequences.get(key)?)))
        .max_by_key(|(_, last)| *last)
        && sequence < last
    {
        bail!(
            "the release list of packslip:{project} has sequence {sequence}, but sequence {last} was already accepted{}; refusing to go back",
            accepted_as(key, project)
        );
    }
    let mut changed = pins.sequences.get(project) != Some(&sequence);
    // Without the repository's ID from the list itself, a sequence found by
    // an ID recorded before still counts, but stays where it is.
    if let Some(forge) = forge {
        for key in keys.iter().filter(|key| key.as_str() != project) {
            changed |= pins.sequences.remove(key).is_some();
            changed |= pins.list_forges.remove(key).is_some();
        }
        changed |= pins.list_forges.get(project) != Some(forge);
        pins.list_forges.insert(project.to_string(), forge.clone());
    }
    if changed {
        pins.sequences.insert(project.to_string(), sequence);
        save(path, &pins)?;
    }
    Ok(())
}

/// Under which name a release list was accepted, for a refusal.
fn accepted_as(key: &str, project: &str) -> String {
    if key == project {
        String::new()
    } else {
        format!(" as packslip:{key}, the same repository before it was renamed")
    }
}

/// An absent supplementary list is allowed only before this machine has
/// accepted one for the repository, under this name or another a rename
/// gave it. Its disappearance must not undo signed withdrawals. `forge`
/// is the repository a release was verified as signed by, once one was:
/// without it, only a repository ID recorded under this name links the
/// names.
pub fn check_missing_list(project: &str, forge: Option<&ForgePin>) -> Result<()> {
    check_missing_list_at(&pins_file(), project, forge)
}

pub fn check_missing_list_at(path: &Path, project: &str, forge: Option<&ForgePin>) -> Result<()> {
    let _lock = locked(path)?;
    let pins = load(path)?;
    if let Some(key) = pins
        .repository_keys(project, forge)
        .into_iter()
        .find(|key| pins.sequences.contains_key(key))
    {
        bail!(
            "the signed release list of packslip:{project} disappeared, but one was accepted{}; restore the list or run `mise packslip forget {key}`",
            accepted_as(&key, project)
        );
    }
    Ok(())
}

/// Every pin, by project.
pub fn list() -> Result<BTreeMap<String, Pin>> {
    list_at(&pins_file())
}

pub fn list_at(path: &Path) -> Result<BTreeMap<String, Pin>> {
    Ok(load(path)?.pins)
}

/// Drop a project's pin and sequence, so the next release accepted sets
/// them again. Returns whether there was one.
pub fn forget(project: &str) -> Result<bool> {
    forget_at(&pins_file(), project)
}

pub fn forget_at(path: &Path, project: &str) -> Result<bool> {
    let _lock = locked(path)?;
    let mut pins = load(path)?;
    // What a rename left under another name goes too, or the next install
    // would find it again by the repository's ID.
    let mut keys = pins.repository_keys(project, None);
    keys.extend(pins.pin_keys(project, None));
    let mut had = false;
    for key in &keys {
        had |= pins.pins.remove(key).is_some();
        had |= pins.sequences.remove(key).is_some();
        pins.list_forges.remove(key);
    }
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
        check_missing_list_at(&path, "github.com/o/r", None).unwrap();
        check_sequence_at(&path, "github.com/o/r", 0, None).unwrap();
        assert!(check_missing_list_at(&path, "github.com/o/r", None).is_err());
        check_missing_list_at(&path, "github.com/o/other", None).unwrap();
        assert!(forget_at(&path, "github.com/o/r").unwrap());
        check_missing_list_at(&path, "github.com/o/r", None).unwrap();
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
        check_sequence_at(&path, "t.example.com", 3, None).unwrap();
        check_sequence_at(&path, "t.example.com", 5, None).unwrap();
        let err = check_sequence_at(&path, "t.example.com", 4, None).unwrap_err();
        assert!(err.to_string().contains("refusing to go back"), "{err}");
        check_sequence_at(&path, "t.example.com", 5, None).unwrap();
        check_sequence_at(&path, "other.example.com", 1, None).unwrap();
        assert!(forget_at(&path, "t.example.com").unwrap());
        check_sequence_at(&path, "t.example.com", 1, None).unwrap();
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
                forge: Some(ForgePin::of("github.com/new/r", "42")),
            },
        );
        save(&path, &pins).unwrap();
        let text = file::read_to_string(&path).unwrap();
        assert!(text.contains("[pins.\"github.com/old/r\".forge]"), "{text}");
        assert!(text.contains("repository_id = \"42\""), "{text}");
        let pin = ForgePin::of("github.com/new/r", "42");
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

    fn repo(project: &str, id: &str) -> ForgePin {
        ForgePin::of(project, id)
    }

    #[test]
    fn a_release_list_sequence_follows_the_repository_through_a_rename() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pins.toml");
        let (old, new) = ("github.com/o/old", "github.com/o/new");
        check_sequence_at(&path, old, 5, Some(&repo(old, "42"))).unwrap();
        // The config follows the rename: an older list signed under the new
        // name is still older.
        let err = check_sequence_at(&path, new, 4, Some(&repo(new, "42"))).unwrap_err();
        assert!(
            err.to_string()
                .contains("sequence 5 was already accepted as packslip:github.com/o/old"),
            "{err}"
        );
        check_sequence_at(&path, new, 6, Some(&repo(new, "42"))).unwrap();
        let pins = load(&path).unwrap();
        assert_eq!(
            pins.sequences,
            BTreeMap::from([(new.to_string(), 6)]),
            "one sequence per repository"
        );
        assert_eq!(pins.list_forges[new], repo(new, "42"));
        // And back again: a config still naming the old one is held to it.
        let err = check_sequence_at(&path, old, 5, Some(&repo(new, "42"))).unwrap_err();
        assert!(err.to_string().contains("refusing to go back"), "{err}");
        // Another repository, or another tool of a monorepo, has its own.
        check_sequence_at(
            &path,
            "github.com/o/other",
            1,
            Some(&repo("github.com/o/other", "43")),
        )
        .unwrap();
        check_sequence_at(
            &path,
            "github.com/o/new/tool",
            1,
            Some(&repo("github.com/o/new/tool", "42")),
        )
        .unwrap();
        check_sequence_at(
            &path,
            "gitlab.com/o/new",
            1,
            Some(&repo("gitlab.com/o/new", "42")),
        )
        .unwrap();
        // Without the list's own forge identity, a sequence found through an
        // ID recorded under the name still counts, and stays where it is.
        let err = check_sequence_at(&path, new, 1, None).unwrap_err();
        assert!(err.to_string().contains("refusing to go back"), "{err}");
        // Forgetting the name it is recorded under, which a refusal gives,
        // lets an older list in.
        assert!(forget_at(&path, new).unwrap());
        check_sequence_at(&path, new, 1, Some(&repo(new, "42"))).unwrap();
    }

    #[test]
    fn an_owner_id_an_older_pin_recorded_is_read_and_ignored() {
        // mise 2026.9 recorded the repository's owner ID too. Such a pin still
        // reads, and a release of the same repository under another owner,
        // which is what a transfer looks like, is the same repository.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pins.toml");
        file::write(
            &path,
            format!(
                r#"[pins."github.com/o/r"]
scheme = "sigstore-oidc"
signer = "{WORKFLOW}"
attested_by = "vendor"
pinned_at = "2026-09-01T00:00:00Z"

[pins."github.com/o/r".forge]
project = "github.com/o/r"
repository_id = "42"
owner_id = "7"

[list_forges."github.com/o/r"]
project = "github.com/o/r"
repository_id = "42"
owner_id = "7"

[sequences]
"github.com/o/r" = 1
"#
            ),
        )
        .unwrap();
        assert_eq!(
            forge_pin_at(&path, "github.com/o/r").unwrap(),
            Some(repo("github.com/o/r", "42"))
        );
        let moved = repo("github.com/acme/r", "42");
        check_sequence_at(&path, "github.com/acme/r", 2, Some(&moved)).unwrap();
        let pins = load(&path).unwrap();
        assert_eq!(
            pins.list_forges.keys().collect::<Vec<_>>(),
            ["github.com/acme/r"]
        );
        assert_eq!(pins.sequences["github.com/acme/r"], 2);
        assert!(
            !file::read_to_string(&path).unwrap().contains("owner_id"),
            "a pin rewritten for another reason drops the owner ID"
        );
    }

    #[test]
    fn a_release_list_accepted_under_another_name_cannot_disappear() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pins.toml");
        let (old, new) = ("github.com/o/old", "github.com/o/new");
        check_sequence_at(&path, old, 0, Some(&repo(old, "42"))).unwrap();
        // Nothing under the new name says it is the old repository until a
        // release shows its ID...
        check_missing_list_at(&path, new, None).unwrap();
        let err = check_missing_list_at(&path, new, Some(&repo(new, "42"))).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("one was accepted as packslip:github.com/o/old"),
            "{msg}"
        );
        assert!(
            msg.contains("mise packslip forget github.com/o/old"),
            "{msg}"
        );
        check_missing_list_at(&path, new, Some(&repo(new, "43"))).unwrap();
        assert!(check_missing_list_at(&path, old, None).is_err());
        // ...or a pin records a release signed under it.
        let mut pins = load(&path).unwrap();
        pins.pins.insert(
            old.into(),
            Pin {
                scheme: "sigstore-oidc".into(),
                signer: WORKFLOW.into(),
                issuer: None,
                attested_by: "vendor".into(),
                provenance: false,
                unlogged: false,
                pinned_at: "2026-09-01T00:00:00Z".into(),
                forge: Some(repo(new, "42")),
            },
        );
        pins.list_forges.clear();
        save(&path, &pins).unwrap();
        assert!(check_missing_list_at(&path, new, None).is_err());
    }
}
