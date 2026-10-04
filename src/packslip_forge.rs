//! A packslip project on GitHub or GitLab is located by its name and pinned
//! by the forge's repository ID, which the signing certificate records and
//! neither a rename nor a transfer to another owner changes. A renamed or
//! transferred repository's releases, signed under the new name, keep
//! installing under the old one; a new repository that took a deleted one's
//! name is refused. The IDs come from what mise pinned before
//! (`packslip/pins.toml` and `mise.lock`), or, the first time, from what the
//! forge says the name resolves to now.

use std::path::Path;

use eyre::{Report, eyre};
use packslip::forge::{Check, Continuity, Expected, ForgePin, IdentityError, PinSource};
use packslip::{ForgeError, ForgeVerified, Options, Verified, VerifiedList};

use crate::config::{Settings, SettingsExt};
use crate::lockfile::PlatformInfo;
use crate::{github, gitlab, packslip_pins};

/// What a forge project's release must be signed by: the forge identities
/// mise pinned for the project, and, when it pinned none, the repository ID
/// the forge gives for the name.
#[derive(Debug, Clone)]
pub(crate) struct ForgeExpect {
    project: String,
    /// This machine's pin first, then the lockfile's, each with where it
    /// came from. Each one must hold.
    pins: Vec<(PinSource, ForgePin)>,
    resolved: Option<String>,
}

/// What the forge says a project name stands for now.
#[derive(Debug, Clone)]
struct ForgeRepository {
    id: String,
    /// The project as it is called now, `github.com/owner/repo[/tool]` or
    /// `gitlab.com/<path>`.
    project: String,
}

/// The repository ID and name the forge gives `project` now, following a
/// rename's redirect. None offline, for a name that is not a forge project,
/// or when the forge cannot be asked (rate limit, no network): the check then
/// falls back to the name, which is what mise did before it had IDs.
async fn lookup(project: &str) -> Option<ForgeRepository> {
    if Settings::get().offline() {
        return None;
    }
    if let Some((host, owner, repo)) = packslip::model::repository(project) {
        let subpath = packslip::model::repository_subpath(project)
            .map(|sub| format!("/{sub}"))
            .unwrap_or_default();
        return match github::repository_identity(&format!("{owner}/{repo}")).await {
            Ok(found) => Some(ForgeRepository {
                id: found.id,
                project: format!("{host}/{}{subpath}", found.full_name),
            }),
            Err(err) => {
                debug!("packslip:{project}: could not look up its repository ID: {err:#}");
                None
            }
        };
    }
    let path = project.strip_prefix("gitlab.com/")?;
    match gitlab::project_identity(path).await {
        Ok(found) => Some(ForgeRepository {
            id: found.id,
            project: format!("gitlab.com/{}", found.path_with_namespace),
        }),
        Err(err) => {
            debug!("packslip:{project}: could not look up its project ID: {err:#}");
            None
        }
    }
}

/// The project a bundle's statement claims, before anything is verified.
/// Only fit to decide whether a forge lookup is worth making.
fn claimed_project(bundle: &str) -> Option<String> {
    packslip::peek_unverified(bundle)
        .ok()
        .map(|claimed| claimed.project)
}

/// The forge identity a lock entry committed to, if it recorded one.
pub(crate) fn lock_pin(project: &str, info: &PlatformInfo) -> Option<ForgePin> {
    info.signer.as_ref()?;
    let id = info.repository_id.clone()?;
    Some(ForgePin::of(project, id))
}

/// Record who signed an accepted release in its lock entry: the signer
/// (`scheme:signer`), and the forge identity it was accepted with. Without
/// one, because explicit signer options skip the forge check or the
/// certificate records no IDs, an entry that already named this signer keeps
/// the IDs it recorded, so a commitment is never dropped silently; an entry
/// for another signer loses them with the signer they came with.
pub(crate) fn lock_record(info: &mut PlatformInfo, signer: String, check: Option<&Check>) {
    let same_signer = info.signer.as_deref() == Some(signer.as_str());
    info.signer = Some(signer);
    match check.and_then(|check| check.pin.as_ref()) {
        Some(pin) => {
            // mise no longer records the owner's ID: the repository ID alone
            // is the identity. One an older mise recorded for the same
            // repository stays, so an unchanged entry is not rewritten.
            if info.repository_id.as_deref() != Some(&pin.repository_id) {
                info.repository_owner_id = None;
            }
            info.repository_id = Some(pin.repository_id.clone());
        }
        None if same_signer => {}
        None => {
            info.repository_id = None;
            info.repository_owner_id = None;
        }
    }
}

/// Whether a signer a lock entry recorded (`scheme:signer`) is the one that
/// signed this release: the same string, or the same workflow of the same
/// repository under another name when the forge check passed.
pub(crate) fn lock_signer_continues(locked: &str, signer: &str, check: Option<&Check>) -> bool {
    if locked == signer {
        return true;
    }
    let (Some((locked_scheme, locked_signer)), Some((scheme, _))) =
        (locked.split_once(':'), signer.split_once(':'))
    else {
        return false;
    };
    locked_scheme == scheme
        && check.is_some_and(|check| packslip_pins::continues_signer(check, locked_signer))
}

/// Whether a newly resolved lock entry for `project` is signed by the signer
/// the old one committed to: the same scheme, and the same signer given the
/// forge pin each entry recorded ([`packslip::forge::same_workflow`]). A
/// rename or a transfer keeps the signer; a changed repository ID does not;
/// without IDs on both sides the signer must be the same string. An entry
/// that committed to no signer takes any. `project` is none when the
/// backend names no packslip project, and then no IDs are compared.
pub(crate) fn lock_entry_continues(
    project: Option<&str>,
    old: &PlatformInfo,
    new: &PlatformInfo,
) -> bool {
    let (Some(previous), Some(current)) = (&old.signer, &new.signer) else {
        return old.signer.is_none();
    };
    let ((previous_scheme, previous), (scheme, current)) =
        (split_signer(previous), split_signer(current));
    let pin = |info: &PlatformInfo| project.and_then(|project| lock_pin(project, info));
    previous_scheme == scheme
        && packslip::forge::same_workflow(previous, pin(old).as_ref(), current, pin(new).as_ref())
}

/// A signer as a lock entry records it, `scheme:identity`, split in two.
fn split_signer(signer: &str) -> (&str, &str) {
    signer.split_once(':').unwrap_or(("", signer))
}

impl ForgeExpect {
    /// The expectation for `project`'s bundle or release list, from this
    /// machine's pin and the lock entries given. With neither, the forge is
    /// asked what the name resolves to, but only when the not yet verified
    /// `bundle` claims another name: under the requested name there is
    /// nothing a first lookup could add, since a new repository that took the
    /// name resolves to itself, and an API request per install would spend a
    /// rate limit that many users share.
    pub(crate) async fn new<'a>(
        project: &str,
        lock: impl IntoIterator<Item = &'a PlatformInfo>,
        bundle: &str,
    ) -> eyre::Result<Self> {
        let mut pins = Vec::new();
        if let Some(pin) = packslip_pins::forge_pin(project)? {
            pins.push((PinSource::Local, pin));
        }
        for pin in lock.into_iter().filter_map(|info| lock_pin(project, info)) {
            let pin = (PinSource::Lockfile, pin);
            if !pins.contains(&pin) {
                pins.push(pin);
            }
        }
        let resolved = if pins.is_empty() && claimed_project(bundle).as_deref() != Some(project) {
            lookup(project).await.map(|found| found.id)
        } else {
            None
        };
        Ok(Self {
            project: project.to_string(),
            pins,
            resolved,
        })
    }

    fn expected(&self) -> Expected<'_> {
        Expected::new(&self.project)
            .pinned_by(&self.pins)
            .resolved(self.resolved.as_deref())
    }

    /// Verify a bundle under the forge's policy and check who signed it.
    pub(crate) fn verify(
        &self,
        bundle: &str,
        options: Options<'_>,
        artifacts: &[&Path],
    ) -> eyre::Result<ForgeVerified<Verified>> {
        packslip::verify_forge(bundle, &self.expected(), options, artifacts)
            .map_err(|err| self.error(err))
    }

    /// Verify a release list under the forge's policy and check who signed it.
    pub(crate) fn verify_list(
        &self,
        bundle: &str,
        options: Options<'_>,
    ) -> eyre::Result<ForgeVerified<VerifiedList>> {
        packslip::verify_forge_release_list(bundle, &self.expected(), options)
            .map_err(|err| self.error(err))
    }

    fn error(&self, err: ForgeError) -> Report {
        match err {
            ForgeError::Identity(err) => self.identity_error(err),
            err => eyre!("{err}"),
        }
    }

    /// Which records pin another repository than `actual`, the ID the
    /// release's certificate records: this machine's pin, mise.lock, or
    /// both. packslip reports the first pin that failed, but a recovery has
    /// to clear every one, or the next install is refused by the next.
    fn disagreeing(&self, actual: &str, cited: Option<PinSource>) -> Disagreeing {
        let from = |want: PinSource| {
            cited == Some(want)
                || self
                    .pins
                    .iter()
                    .any(|(source, pin)| *source == want && pin.repository_id != actual)
        };
        Disagreeing {
            local: from(PinSource::Local),
            lockfile: from(PinSource::Lockfile),
        }
    }

    fn identity_error(&self, err: IdentityError) -> Report {
        let requested = &self.project;
        match err {
            IdentityError::DifferentRepository {
                project,
                expected,
                actual,
                evidence,
            } => {
                let kind = id_kind(requested);
                let Some(cited) = evidence.pin_source() else {
                    return eyre!(
                        "packslip:{requested}: this release was signed by {project} as {kind} {actual}, but {requested} is {kind} {expected} now. \
                         The release comes from a different repository than the one the name belongs to, so mise refuses it."
                    );
                };
                let pinned = match cited {
                    PinSource::Lockfile => "mise.lock pins",
                    _ => "mise pinned",
                };
                let steps = self.disagreeing(&actual, Some(cited)).steps(requested);
                eyre!(
                    "packslip:{requested}: this release was signed by {project} as {kind} {actual}, but {pinned} {kind} {expected} for it. \
                     The name now belongs to a different repository, as it would if the original was deleted and someone else created one under its name, so mise refuses it.\n\n\
                     If the vendor re-created the repository itself, {steps}."
                )
            }
            err => eyre!("{err}"),
        }
    }
}

/// The pins a refused release disagrees with, by where they came from.
#[derive(Debug, Clone, Copy)]
struct Disagreeing {
    local: bool,
    lockfile: bool,
}

impl Disagreeing {
    /// Every step that clears them, for the project as `requested` names it.
    fn steps(self, requested: &str) -> String {
        let forget = format!("run `mise packslip forget {requested}`");
        let unlock = "remove the tool's entries from mise.lock";
        match (self.local, self.lockfile) {
            (true, true) => format!("{forget}, {unlock}, and install again"),
            (true, false) => format!("{forget} and install again"),
            (false, _) => format!("{unlock} and install again"),
        }
    }
}

/// What `id` is called on the project's forge.
fn id_kind(project: &str) -> &'static str {
    if project.starts_with("gitlab.com/") {
        "GitLab project ID"
    } else {
        "GitHub repository ID"
    }
}

/// Say, once, that a project was installed under a name it no longer has,
/// after a rename or a transfer to another owner. A release older than a
/// rename is signed under the old name while the
/// config already names the new one, so the forge is asked which is current
/// before anyone is told to change their config.
pub(crate) async fn warn_if_renamed(check: &Check) {
    let Continuity::Renamed { requested, signed } = &check.continuity else {
        return;
    };
    match lookup(requested).await {
        Some(current) if current.project.eq_ignore_ascii_case(signed) => {
            warn_once!(
                "packslip:{requested} was renamed to {signed}; mise followed it by its repository ID. Change the tool to packslip:{signed} in your config"
            );
        }
        _ => debug!(
            "packslip:{requested}: this release was signed under {signed}, the same repository by its ID"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// jdx/hk's v2.3.0 packslip as its release workflow published it. Its
    /// certificate records repository ID 922514152.
    const HK: &str = include_str!("../test/fixtures/packslip-forge/hk-v2.3.0.sigstore.json");
    const HK_SIGNER: &str = "sigstore-oidc:https://github.com/jdx/hk/.github/workflows/release.yml";
    const JDX_ID: &str = "216188";

    fn verify(expect: &ForgeExpect) -> eyre::Result<ForgeVerified<Verified>> {
        let root = packslip::sigstore::trusted_root(None).unwrap();
        let options = Options {
            require_log: true,
            trusted_root: &root,
        };
        expect.verify(HK, options, &[])
    }

    fn expect(
        project: &str,
        pins: Vec<(PinSource, ForgePin)>,
        resolved: Option<&str>,
    ) -> ForgeExpect {
        ForgeExpect {
            project: project.into(),
            pins,
            resolved: resolved.map(str::to_string),
        }
    }

    fn hk_pin(project: &str) -> ForgePin {
        ForgePin::of(project, "922514152")
    }

    fn local(pin: ForgePin) -> (PinSource, ForgePin) {
        (PinSource::Local, pin)
    }

    fn locked(pin: ForgePin) -> (PinSource, ForgePin) {
        (PinSource::Lockfile, pin)
    }

    #[test]
    fn the_same_repository_under_its_own_name_is_accepted() {
        for expect in [
            expect("github.com/jdx/hk", vec![], None),
            expect(
                "github.com/jdx/hk",
                vec![local(hk_pin("github.com/jdx/hk"))],
                None,
            ),
            expect(
                "github.com/jdx/hk",
                vec![locked(hk_pin("github.com/jdx/hk"))],
                None,
            ),
        ] {
            let ok = verify(&expect).unwrap();
            assert_eq!(ok.check.continuity, Continuity::Same);
            assert_eq!(ok.check.pin, Some(hk_pin("github.com/jdx/hk")));
            let mut info = PlatformInfo::default();
            lock_record(&mut info, HK_SIGNER.into(), Some(&ok.check));
            assert_eq!(info.signer.as_deref(), Some(HK_SIGNER));
            assert_eq!(info.repository_id.as_deref(), Some("922514152"));
            assert_eq!(info.repository_owner_id, None, "no owner ID is recorded");
            assert_eq!(
                lock_pin("github.com/jdx/hk", &info),
                Some(hk_pin("github.com/jdx/hk"))
            );
        }
    }

    #[test]
    fn a_renamed_project_keeps_installing_by_repository_id() {
        // Were jdx/hk renamed to jdx/hook, a config that says the new name,
        // with the pin or the forge's answer for it, still takes this
        // release signed under the old one.
        for expect in [
            expect(
                "github.com/jdx/hook",
                vec![local(hk_pin("github.com/jdx/hook"))],
                None,
            ),
            expect("github.com/jdx/hook", vec![], Some("922514152")),
        ] {
            let ok = verify(&expect).unwrap();
            assert_eq!(
                ok.check.continuity,
                Continuity::Renamed {
                    requested: "github.com/jdx/hook".into(),
                    signed: "github.com/jdx/hk".into(),
                }
            );
            // The signer mise pinned under the new name continues.
            let pinned = "sigstore-oidc:https://github.com/jdx/hook/.github/workflows/release.yml";
            assert!(lock_signer_continues(pinned, HK_SIGNER, Some(&ok.check)));
            assert!(!lock_signer_continues(pinned, HK_SIGNER, None));
            assert!(!lock_signer_continues(
                "sigstore-oidc:https://github.com/jdx/hook/.github/workflows/other.yml",
                HK_SIGNER,
                Some(&ok.check)
            ));
            assert!(!lock_signer_continues(
                "sigstore-key:https://github.com/jdx/hook/.github/workflows/release.yml",
                HK_SIGNER,
                Some(&ok.check)
            ));
        }
        // With nothing to show the names are one repository, another name is
        // refused, as it was before mise knew the IDs.
        let err = verify(&expect("github.com/jdx/hook", vec![], None)).unwrap_err();
        assert!(err.to_string().contains("nothing shows"), "{err}");
    }

    #[test]
    fn a_recreated_name_is_refused() {
        let forget = "run `mise packslip forget github.com/jdx/hk` and install again";
        let unlock = "remove the tool's entries from mise.lock and install again";
        let both = "run `mise packslip forget github.com/jdx/hk`, remove the tool's entries from mise.lock, and install again";
        let other = || ForgePin::of("github.com/jdx/hk", "1");
        let refusal = |pins| {
            verify(&expect("github.com/jdx/hk", pins, None))
                .unwrap_err()
                .to_string()
        };

        let msg = refusal(vec![local(other())]);
        assert!(msg.contains("belongs to a different repository"), "{msg}");
        assert!(
            msg.contains("signed by github.com/jdx/hk as GitHub repository ID 922514152, but mise pinned GitHub repository ID 1 for it"),
            "{msg}"
        );
        assert!(msg.contains(forget), "{msg}");

        // Every pin must hold, and the refusal says which one did not.
        let msg = refusal(vec![local(hk_pin("github.com/jdx/hk")), locked(other())]);
        assert!(
            msg.contains("mise.lock pins GitHub repository ID 1 for it"),
            "{msg}"
        );
        assert!(msg.contains(unlock), "{msg}");
        assert!(!msg.contains("mise packslip forget"), "{msg}");

        // packslip stops at the first pin that fails, but the advice clears
        // every record that pins another repository, whichever comes first.
        let two = || ForgePin::of("github.com/jdx/hk", "2");
        for pins in [
            vec![local(other()), locked(two())],
            vec![local(two()), locked(other())],
            vec![local(other()), locked(other())],
        ] {
            let msg = refusal(pins);
            assert!(msg.contains(both), "{msg}");
        }

        let err = verify(&expect("github.com/jdx/hook", vec![], Some("555"))).unwrap_err();
        assert!(
            err.to_string()
                .contains("comes from a different repository than the one the name belongs to"),
            "{err}"
        );
    }

    #[test]
    fn a_transfer_to_another_owner_keeps_installing_by_repository_id() {
        // Were jdx/hk transferred to acme/hk, a config naming acme's
        // repository by its new name would see a release from before the
        // transfer as signed by jdx: the same repository, by its ID.
        let pinned = expect(
            "github.com/acme/hk",
            vec![locked(hk_pin("github.com/acme/hk"))],
            None,
        );
        for expect in [
            expect("github.com/acme/hk", vec![], Some("922514152")),
            pinned,
        ] {
            let ok = verify(&expect).unwrap();
            assert_eq!(
                ok.check.continuity,
                Continuity::Renamed {
                    requested: "github.com/acme/hk".into(),
                    signed: "github.com/jdx/hk".into(),
                }
            );
            let pinned = "sigstore-oidc:https://github.com/acme/hk/.github/workflows/release.yml";
            assert!(lock_signer_continues(pinned, HK_SIGNER, Some(&ok.check)));
        }
    }

    #[test]
    fn lock_entries_continue_by_their_forge_ids() {
        let entry = |repo: &str, id: Option<&str>| PlatformInfo {
            signer: Some(format!(
                "sigstore-oidc:https://gitlab.com/{repo}//.gitlab-ci.yml"
            )),
            repository_id: id.map(str::to_string),
            ..Default::default()
        };
        let project = Some("gitlab.com/g/tool");
        let old = entry("g/tool", Some("42"));
        assert!(lock_entry_continues(project, &old, &old));
        // Renamed, or moved to another group: the same repository.
        for moved in ["g/tool2", "acme/tool"] {
            assert!(lock_entry_continues(
                project,
                &old,
                &entry(moved, Some("42"))
            ));
        }
        assert!(!lock_entry_continues(
            project,
            &old,
            &entry("g/tool", Some("43"))
        ));
        // Without IDs on both sides, the signer itself must be the same.
        let legacy = entry("g/tool", None);
        assert!(lock_entry_continues(project, &legacy, &old));
        assert!(!lock_entry_continues(
            project,
            &legacy,
            &entry("g/tool2", Some("42"))
        ));
        assert!(!lock_entry_continues(
            None,
            &old,
            &entry("g/tool2", Some("42"))
        ));
        // Another scheme is another signer, whatever the identity.
        let key = PlatformInfo {
            signer: Some("sigstore-key:https://gitlab.com/g/tool//.gitlab-ci.yml".into()),
            ..old.clone()
        };
        assert!(!lock_entry_continues(project, &old, &key));
        // An entry that committed to no signer takes any.
        assert!(lock_entry_continues(
            project,
            &PlatformInfo::default(),
            &old
        ));
        assert!(!lock_entry_continues(
            project,
            &old,
            &PlatformInfo::default()
        ));
    }

    #[test]
    fn a_lock_entry_without_forge_ids_pins_nothing() {
        let info = PlatformInfo {
            signer: Some(HK_SIGNER.into()),
            ..Default::default()
        };
        assert_eq!(lock_pin("github.com/jdx/hk", &info), None);
        let unsigned = PlatformInfo {
            repository_id: Some("922514152".into()),
            ..Default::default()
        };
        assert_eq!(lock_pin("github.com/jdx/hk", &unsigned), None);
        let mut cleared = PlatformInfo {
            signer: Some(HK_SIGNER.into()),
            repository_id: Some("1".into()),
            repository_owner_id: Some("2".into()),
            ..Default::default()
        };
        // Explicit signer options check no forge identity: the same signer
        // keeps what its entry recorded...
        lock_record(&mut cleared, HK_SIGNER.into(), None);
        assert_eq!(cleared.repository_id.as_deref(), Some("1"));
        assert_eq!(cleared.repository_owner_id.as_deref(), Some("2"));
        // An owner ID an older mise recorded stays while the repository ID
        // does, and goes with it.
        let check = verify(&expect("github.com/jdx/hk", vec![], None))
            .unwrap()
            .check;
        let mut entry = PlatformInfo {
            repository_id: Some("922514152".into()),
            repository_owner_id: Some("216188".into()),
            ..Default::default()
        };
        lock_record(&mut entry, HK_SIGNER.into(), Some(&check));
        assert_eq!(entry.repository_owner_id.as_deref(), Some("216188"));
        let mut moved = PlatformInfo {
            repository_id: Some("1".into()),
            repository_owner_id: Some("2".into()),
            ..Default::default()
        };
        lock_record(&mut moved, HK_SIGNER.into(), Some(&check));
        assert_eq!(moved.repository_id.as_deref(), Some("922514152"));
        assert_eq!(
            moved.repository_owner_id, None,
            "another repository's owner is not kept"
        );
        // ...and another signer does not inherit it.
        let other = "sigstore-key:5A0A".to_string();
        lock_record(&mut cleared, other.clone(), None);
        assert_eq!(cleared.signer, Some(other));
        assert_eq!(cleared.repository_id, None);
        assert_eq!(cleared.repository_owner_id, None);
    }

    #[test]
    fn a_machine_pin_follows_the_rename_and_records_the_forge_ids() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pins.toml");
        let observed = |check: Option<&'static Check>| packslip_pins::Observed {
            scheme: "sigstore-oidc",
            key_id: "https://github.com/jdx/hk/.github/workflows/release.yml@refs/tags/v2.3.0",
            issuer: Some("https://token.actions.githubusercontent.com"),
            attested_by: "vendor",
            provenance: false,
            logged: true,
            forge: check,
        };
        // A pin set under the new name, before mise recorded forge IDs.
        let hook = "https://github.com/jdx/hook/.github/workflows/release.yml@refs/tags/v3.0.0";
        packslip_pins::record_at(
            &path,
            "github.com/jdx/hook",
            packslip_pins::Observed {
                key_id: hook,
                ..observed(None)
            },
        )
        .unwrap();
        // A release signed under the old name is another signer by name...
        let err =
            packslip_pins::check_at(&path, "github.com/jdx/hook", observed(None)).unwrap_err();
        assert!(err.to_string().contains("mise packslip forget"), "{err}");
        // ...and the same workflow of the same repository by its ID.
        let check: &'static Check = Box::leak(Box::new(
            verify(&expect("github.com/jdx/hook", vec![], Some("922514152")))
                .unwrap()
                .check,
        ));
        packslip_pins::check_at(&path, "github.com/jdx/hook", observed(Some(check))).unwrap();
        let pin =
            packslip_pins::record_at(&path, "github.com/jdx/hook", observed(Some(check))).unwrap();
        assert_eq!(pin.forge, Some(hk_pin("github.com/jdx/hk")));
        assert_eq!(
            pin.signer,
            "https://github.com/jdx/hk/.github/workflows/release.yml"
        );
        assert_eq!(
            packslip_pins::forge_pin_at(&path, "github.com/jdx/hook").unwrap(),
            Some(hk_pin("github.com/jdx/hk"))
        );
    }

    #[test]
    fn the_claimed_project_is_read_before_verification() {
        assert_eq!(claimed_project(HK).as_deref(), Some("github.com/jdx/hk"));
        assert_eq!(claimed_project("not a bundle"), None);
    }

    /// What the hk release showed, with `check` the forge check it passed.
    fn hk_observed(check: Option<&Check>, provenance: bool) -> packslip_pins::Observed<'_> {
        packslip_pins::Observed {
            scheme: "sigstore-oidc",
            key_id: "https://github.com/jdx/hk/.github/workflows/release.yml@refs/tags/v2.3.0",
            issuer: Some("https://token.actions.githubusercontent.com"),
            attested_by: "vendor",
            provenance,
            logged: true,
            forge: check,
        }
    }

    /// A pins file with one pin for jdx/hk's repository under `key`, as a
    /// machine that installed it before a rename to jdx/hk wrote it, and a
    /// release-list sequence under the same name.
    fn pins_under(
        key: &str,
        workflow: &str,
        owner_id: &str,
    ) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pins.toml");
        crate::file::write(
            &path,
            format!(
                r#"[pins."{key}"]
scheme = "sigstore-oidc"
signer = "https://{key}/.github/workflows/{workflow}"
issuer = "https://token.actions.githubusercontent.com"
attested_by = "vendor"
provenance = true
unlogged = false
pinned_at = "2026-09-01T00:00:00Z"

[pins."{key}".forge]
project = "{key}"
repository_id = "922514152"
owner_id = "{owner_id}"

[sequences]
"{key}" = 7
"#
            ),
        )
        .unwrap();
        (dir, path)
    }

    #[test]
    fn a_config_that_follows_a_rename_first_keeps_the_pin() {
        // The config (or another machine's) says jdx/hk while this machine
        // pinned the repository as jdx/old-hk, before any release signed
        // under the new name was accepted. The release's repository ID finds
        // the pin: it is not a first install.
        let (_dir, path) = pins_under("github.com/jdx/old-hk", "release.yml", JDX_ID);
        let check = verify(&expect("github.com/jdx/hk", vec![], None))
            .unwrap()
            .check;
        assert_eq!(check.continuity, Continuity::Same);
        let err =
            packslip_pins::check_at(&path, "github.com/jdx/hk", hk_observed(Some(&check), false))
                .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("drops the build provenance"), "{msg}");
        assert!(
            msg.contains("mise pinned the repository as packslip:github.com/jdx/old-hk"),
            "{msg}"
        );
        assert!(
            msg.contains("mise packslip forget github.com/jdx/old-hk"),
            "{msg}"
        );
        assert!(
            packslip_pins::record_at(&path, "github.com/jdx/hk", hk_observed(Some(&check), false))
                .is_err()
        );

        // What the pin requires, it accepts, and the pin moves to the new
        // name with everything it had: one pin for the repository.
        let pin =
            packslip_pins::record_at(&path, "github.com/jdx/hk", hk_observed(Some(&check), true))
                .unwrap();
        let pins = packslip_pins::list_at(&path).unwrap();
        assert_eq!(
            pins.keys().collect::<Vec<_>>(),
            ["github.com/jdx/hk"],
            "no duplicate pin"
        );
        assert_eq!(pins["github.com/jdx/hk"], pin);
        assert_eq!(pin.pinned_at, "2026-09-01T00:00:00Z");
        assert!(pin.provenance);
        assert_eq!(pin.forge, Some(hk_pin("github.com/jdx/hk")));
        // Its release-list sequence came along.
        let err =
            packslip_pins::check_sequence_at(&path, "github.com/jdx/hk", 6, None).unwrap_err();
        assert!(
            err.to_string().contains("sequence 7 was already accepted"),
            "{err}"
        );
        assert!(packslip_pins::check_missing_list_at(&path, "github.com/jdx/hk", None).is_err());
        // And the name the pin had is still held to it.
        assert!(
            packslip_pins::check_at(
                &path,
                "github.com/jdx/old-hk",
                hk_observed(Some(&check), false),
            )
            .is_err()
        );
    }

    #[test]
    fn a_pin_found_by_repository_id_still_refuses_another_signer() {
        let (_dir, path) = pins_under("github.com/jdx/old-hk", "other.yml", JDX_ID);
        let check = verify(&expect("github.com/jdx/hk", vec![], None))
            .unwrap()
            .check;
        let err =
            packslip_pins::record_at(&path, "github.com/jdx/hk", hk_observed(Some(&check), true))
                .unwrap_err();
        assert!(
            err.to_string().contains("signed what mise accepted before"),
            "{err}"
        );
        let pins = packslip_pins::list_at(&path).unwrap();
        assert_eq!(
            pins.keys().collect::<Vec<_>>(),
            ["github.com/jdx/old-hk"],
            "a refusal moves nothing"
        );
    }

    #[test]
    fn a_pin_found_by_repository_id_follows_a_transfer() {
        // The pin recorded the repository as acme/hk under owner 999, as
        // mise 2026.9 wrote it; the release is signed by it as jdx/hk.
        let (_dir, path) = pins_under("github.com/acme/hk", "release.yml", "999");
        let check = verify(&expect("github.com/jdx/hk", vec![], None))
            .unwrap()
            .check;
        packslip_pins::check_at(&path, "github.com/jdx/hk", hk_observed(Some(&check), true))
            .unwrap();
        let pin =
            packslip_pins::record_at(&path, "github.com/jdx/hk", hk_observed(Some(&check), true))
                .unwrap();
        assert_eq!(pin.forge, Some(hk_pin("github.com/jdx/hk")));
        let pins = packslip_pins::list_at(&path).unwrap();
        assert_eq!(pins.keys().collect::<Vec<_>>(), ["github.com/jdx/hk"]);
    }

    #[test]
    fn another_tool_of_the_same_repository_keeps_its_own_pin() {
        let (_dir, path) = pins_under("github.com/jdx/hk/other", "other.yml", JDX_ID);
        let check = verify(&expect("github.com/jdx/hk", vec![], None))
            .unwrap()
            .check;
        packslip_pins::record_at(&path, "github.com/jdx/hk", hk_observed(Some(&check), false))
            .unwrap();
        let pins = packslip_pins::list_at(&path).unwrap();
        assert_eq!(
            pins.keys().collect::<Vec<_>>(),
            ["github.com/jdx/hk", "github.com/jdx/hk/other"]
        );
    }
}
