//! Extra verification for mise's own release artifacts. The archive signature
//! remains mandatory too; mirrors only supply bytes, never a new trust root.

use std::path::Path;

use ::packslip::forge::{Expected, ForgePin};
use ::packslip::{Options, Verified};
use eyre::{Result, WrapErr, ensure, eyre};

const PROJECT: &str = "github.com/jdx/mise";
// GitHub repository IDs survive renames and transfers. Do not resolve this
// from the configured mirror or trust an ID supplied by release metadata.
const REPOSITORY_ID: &str = "586920414";

pub(super) fn required(version: &str) -> Result<bool> {
    Ok(crate::cli::version::mise_release_key(version)? >= (2026, 9, 3))
}

fn expected(pin: &ForgePin) -> Expected<'_> {
    // The built-in repository ID, rather than its current owner's name, is
    // the trust anchor. Moving this same repository to an org is supported.
    Expected::new(PROJECT)
        .pinned(Some(pin))
        .accepting_transfer(true)
}

fn verify_manifest(bundle: &str, artifacts: &[&Path]) -> Result<Verified> {
    let root = ::packslip::sigstore::trusted_root(None).map_err(|err| eyre!("{err}"))?;
    let pin = ForgePin::new(PROJECT, REPOSITORY_ID, None);
    let accepted = ::packslip::verify_forge(
        bundle,
        &expected(&pin),
        Options {
            require_log: true,
            trusted_root: &root,
        },
        artifacts,
    )
    .map_err(|err| eyre!("self-update packslip verification failed: {err}"))?;
    check_repository_id(&accepted.check)?;
    Ok(accepted.verified)
}

fn check_repository_id(check: &::packslip::forge::Check) -> Result<()> {
    ensure!(
        check
            .pin
            .as_ref()
            .is_some_and(|pin| pin.repository_id == REPOSITORY_ID),
        "self-update packslip certificate must identify GitHub repository {REPOSITORY_ID}"
    );
    Ok(())
}

fn check_release(
    verified: &Verified,
    version: &str,
    before: Option<jiff::Timestamp>,
) -> Result<()> {
    ensure!(
        verified.version == version,
        "self-update packslip is for version {}, not {version}",
        verified.version
    );
    ensure!(
        verified.attested_by == ::packslip::Attestor::Vendor,
        "self-update requires the vendor's packslip"
    );
    // Keep the workflow pinned while allowing the repository's name to move.
    let workflow = format!("https://{}/.github/workflows/release.yml", verified.project);
    ensure!(
        crate::packslip_pins::signer_of(&verified.scheme.to_string(), &verified.key_id) == workflow,
        "self-update packslip was not signed by the release workflow"
    );
    let logged: jiff::Timestamp = verified
        .logged_at
        .as_deref()
        .ok_or_else(|| eyre!("self-update packslip has no verified transparency-log timestamp"))?
        .parse()
        .wrap_err("invalid packslip log timestamp")?;
    if let Some(before) = before {
        ensure!(
            logged <= before,
            "mise {version} was recorded in the transparency log at {logged}, after the minimum release age cutoff {before}; wait or explicitly select a version"
        );
    }
    Ok(())
}

/// Called on a blocking worker, before extraction or replacement.
pub(super) fn verify(
    bundle: &str,
    version: &str,
    archive: &Path,
    before: Option<jiff::Timestamp>,
) -> Result<()> {
    let verified = verify_manifest(bundle, &[archive])?;
    check_release(&verified, version, before)
}

#[cfg(test)]
mod tests {
    use super::*;
    const BUNDLE: &str =
        include_str!("../../../test/fixtures/self-update/mise-v2026.9.16.sigstore.json");

    #[test]
    fn published_manifest_verifies_by_repository_id_and_release_workflow() {
        let verified = verify_manifest(BUNDLE, &[]).unwrap();
        check_release(
            &verified,
            "2026.9.16",
            Some("2026-09-29T00:00:00Z".parse().unwrap()),
        )
        .unwrap();
        assert!(check_release(&verified, "2026.9.15", None).is_err());
        assert!(
            check_release(
                &verified,
                "2026.9.16",
                Some("2026-09-27T00:00:00Z".parse().unwrap())
            )
            .is_err()
        );
        check_release(&verified, "2026.9.16", None).unwrap();
    }

    #[test]
    fn a_release_workflow_dispatched_from_main_is_supported() {
        let bundle =
            include_str!("../../../test/fixtures/self-update/mise-v2026.9.3.sigstore.json");
        let mut verified = verify_manifest(bundle, &[]).unwrap();
        check_release(&verified, "2026.9.3", None).unwrap();
        verified.key_id = verified.key_id.replace("release.yml", "other.yml");
        assert!(check_release(&verified, "2026.9.3", None).is_err());
    }

    #[test]
    fn changed_archive_and_manifest_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("mise-v2026.9.16-macos-arm64.tar.gz");
        std::fs::write(&archive, b"not the signed release").unwrap();
        assert!(verify(BUNDLE, "2026.9.16", &archive, None).is_err());
        let mut bundle: serde_json::Value = serde_json::from_str(BUNDLE).unwrap();
        bundle["dsseEnvelope"]["payload"] = "e30=".into();
        assert!(verify_manifest(&bundle.to_string(), &[]).is_err());
    }

    #[test]
    fn repository_pin_allows_renames_and_transfers_but_not_replacements() {
        let pin = ForgePin::new(PROJECT, REPOSITORY_ID, None);
        for project in [
            PROJECT,
            "github.com/jdx/renamed",
            "github.com/mise-org/mise",
        ] {
            let mut source = ::packslip::sigstore::source_repository(BUNDLE)
                .unwrap()
                .unwrap();
            source.uri = format!("https://{project}");
            source.owner_uri = Some(format!("https://{}", project.rsplit_once('/').unwrap().0));
            source.owner_id = Some("1234".into());
            let identity =
                format!("https://{project}/.github/workflows/release.yml@refs/tags/v2026.9.16");
            let check = |source: &::packslip::sigstore::SourceRepository| {
                ::packslip::forge::check(
                    &expected(&pin),
                    project,
                    &identity,
                    Some("https://token.actions.githubusercontent.com"),
                    Some(source),
                )
                .map_err(|err| eyre!("{err}"))
                .and_then(|check| check_repository_id(&check))
            };
            check(&source).unwrap();
            source.id = Some("1".into());
            assert!(check(&source).is_err());
            source.id = None;
            assert!(check(&source).is_err());
        }
    }

    #[test]
    fn only_legacy_releases_can_omit_a_manifest() {
        assert!(!required("2026.9.2").unwrap());
        assert!(required("2026.9.3").unwrap());
        assert!(required("2027.1.0").unwrap());
    }
}
