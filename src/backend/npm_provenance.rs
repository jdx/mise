//! Verifies the Sigstore provenance npm publishes for an `npm:` tool's own
//! package, inside the embedded aube install.
//!
//! aube's `trustPolicy` checks the shape of a release's attestation metadata
//! but never the signed bundle. aube hands each registry package to the
//! [`InstallProvenanceVerifier`] before it downloads the tarball, with the
//! integrity it pins for it; aube then checks the tarball against that
//! integrity itself, so an attestation whose subject digest equals it covers
//! the bytes that get installed.
//!
//! Only the tool's own package is checked. A package that publishes no
//! provenance, and a private registry that serves none, install as they do
//! with verification off.

use std::sync::Arc;

use aube::embed::{InstallProvenanceFuture, InstallProvenanceSubject, InstallProvenanceVerifier};
use base64::Engine as _;
use miette::miette;

use crate::backend::npm_registry;
use crate::github::sigstore::{Attestation, verify_github_attestation_sources_for_sha512};
use crate::http::HTTP;

pub(crate) struct NpmProvenanceVerifier {
    package: String,
}

impl NpmProvenanceVerifier {
    pub(crate) fn new(package: &str) -> Arc<Self> {
        Arc::new(Self {
            package: package.to_string(),
        })
    }
}

impl InstallProvenanceVerifier for NpmProvenanceVerifier {
    fn verify(&self, packages: Vec<InstallProvenanceSubject>) -> InstallProvenanceFuture<'_> {
        Box::pin(async move {
            for subject in packages.iter().filter(|s| s.name == self.package) {
                verify_subject(subject)
                    .await
                    .map_err(|e| miette!("{e:#}"))?;
            }
            Ok(())
        })
    }
}

#[derive(serde::Deserialize)]
struct AttestationsResponse {
    attestations: Vec<Attestation>,
}

async fn verify_subject(subject: &InstallProvenanceSubject) -> eyre::Result<()> {
    let (name, version) = (&subject.name, &subject.version);
    let registry = npm_registry::package_registry(name);
    if !npm_registry::is_public_registry(&registry) {
        debug!("npm:{name}@{version}: {registry} is not the public registry, skipping provenance");
        return Ok(());
    }

    let url = attestations_url(&registry, name, version);
    let response = match HTTP.json::<AttestationsResponse, _>(&url).await {
        Ok(response) => response,
        Err(err) if is_not_found(&err) => {
            debug!("npm:{name}@{version} publishes no attestations");
            return Ok(());
        }
        Err(err) => {
            return Err(err.wrap_err(format!(
                "failed to fetch npm attestations for {name}@{version}"
            )));
        }
    };
    if response.attestations.is_empty() {
        debug!("npm:{name}@{version} publishes no attestations");
        return Ok(());
    }

    // Only a package that publishes provenance needs a digest to check it
    // against; an older release pinned by sha1 and one with no attestations
    // install as they do with this off.
    let integrity = subject.integrity.as_deref().ok_or_else(|| {
        eyre::eyre!("npm:{name}@{version} has no pinned integrity to verify provenance against")
    })?;
    let sha512 = sha512_from_integrity(integrity).ok_or_else(|| {
        eyre::eyre!(
            "npm:{name}@{version} publishes provenance but is pinned by {integrity}, and verifying it needs a sha512 integrity"
        )
    })?;

    let sources =
        verify_github_attestation_sources_for_sha512(&sha512, &response.attestations, None)
            .await
            .map_err(|e| eyre::eyre!("npm provenance for {name}@{version} did not verify: {e}"))?;
    debug!(
        "npm:{name}@{version} provenance verified, built from {}",
        sources.join(", ")
    );
    Ok(())
}

fn is_not_found(err: &eyre::Report) -> bool {
    err.downcast_ref::<reqwest::Error>()
        .and_then(|e| e.status())
        .is_some_and(|s| s == reqwest::StatusCode::NOT_FOUND)
}

/// The raw digest behind a `sha512-<base64>` SRI string.
fn sha512_from_integrity(integrity: &str) -> Option<Vec<u8>> {
    let digest = base64::engine::general_purpose::STANDARD
        .decode(integrity.strip_prefix("sha512-")?)
        .ok()?;
    (digest.len() == 64).then_some(digest)
}

/// npm's attestations endpoint; a scoped name keeps its `/` as `%2f`.
fn attestations_url(registry: &str, name: &str, version: &str) -> String {
    format!(
        "{}/-/npm/v1/attestations/{}@{version}",
        registry.trim_end_matches('/'),
        name.replace('/', "%2f")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha512_comes_only_from_a_sha512_sri_string() {
        let integrity = "sha512-Mm1E3/CmDDCz3nDhFKTuYdB47EdRFRQMOE/EAbiG1MJW77/w1b3P7Qx7JSrVJs8PfwOLOVcKQCHErIwCTyPbag==";
        assert_eq!(sha512_from_integrity(integrity).unwrap().len(), 64);
        assert_eq!(sha512_from_integrity("sha1-abc="), None);
        assert_eq!(sha512_from_integrity("sha512-abc="), None);
        assert_eq!(sha512_from_integrity("sha512-not base64"), None);
    }

    #[test]
    fn attestations_url_encodes_a_scoped_name() {
        assert_eq!(
            attestations_url("https://registry.npmjs.org/", "@sigstore/bundle", "3.1.0"),
            "https://registry.npmjs.org/-/npm/v1/attestations/@sigstore%2fbundle@3.1.0"
        );
        assert_eq!(
            attestations_url("https://registry.npmjs.org", "wrangler", "4.156.0"),
            "https://registry.npmjs.org/-/npm/v1/attestations/wrangler@4.156.0"
        );
    }
}
