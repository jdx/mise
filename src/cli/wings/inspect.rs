//! `mise wings inspect` — inspect Wings OCI registry artifacts.
//!
//! These commands talk directly to the authenticated Wings registry.
//! They are intentionally read-only and use the OCI Distribution API
//! surface Wings serves for installs: manifests, referrers, and blobs.

use eyre::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};

use crate::wings::artifact::{
    MEDIA_TYPE_OCI_IMAGE_INDEX, MEDIA_TYPE_OCI_MANIFEST, WingsReference, ensure_digest,
    registry_headers,
};

const MEDIA_TYPE_SPDX_SBOM: &str = "application/spdx+json";
const MEDIA_TYPE_CYCLONEDX_SBOM: &str = "application/vnd.cyclonedx+json";

/// Inspect Wings OCI artifacts and attached evidence.
///
/// Examples:
///
/// ```sh
/// $ mise wings inspect manifest registry.mise-wings.en.dev/acme/node:20
/// {"schemaVersion":2,"mediaType":"application/vnd.oci.image.manifest.v1+json","layers":[{"mediaType":"application/vnd.mise-wings.artifact.v1","digest":"sha256:..."}]}
/// ```
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment)]
pub struct Inspect {
    #[clap(subcommand)]
    command: Commands,
}

#[derive(Debug, clap::Subcommand)]
enum Commands {
    Manifest(Manifest),
    Referrers(Referrers),
    Sbom(Sbom),
}

impl Inspect {
    pub async fn run(self) -> Result<()> {
        match self.command {
            Commands::Manifest(cmd) => cmd.run().await,
            Commands::Referrers(cmd) => cmd.run().await,
            Commands::Sbom(cmd) => cmd.run().await,
        }
    }
}

/// Print the OCI image manifest for a Wings artifact.
///
/// Examples:
///
/// ```sh
/// $ mise wings inspect manifest registry.mise-wings.en.dev/acme/node:20
/// {"schemaVersion":2,"mediaType":"application/vnd.oci.image.manifest.v1+json","layers":[{"mediaType":"application/vnd.mise-wings.artifact.v1","digest":"sha256:..."}]}
/// ```
///
/// Fetch and verify a specific manifest digest:
///
/// ```sh
/// $ mise wings inspect manifest registry.mise-wings.en.dev/acme/node --digest sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
/// {"schemaVersion":2,"mediaType":"application/vnd.oci.image.manifest.v1+json"}
/// ```
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment)]
struct Manifest {
    /// Wings OCI reference, optionally including @sha256:<digest>.
    reference: String,

    /// Manifest digest to fetch when the reference does not include one.
    #[clap(long)]
    digest: Option<String>,
}

impl Manifest {
    async fn run(self) -> Result<()> {
        let target = InspectTarget::new(&self.reference, self.digest.as_deref(), false)?;
        let token = require_cli_token().await?;
        let manifest_bytes = fetch_manifest(&target, &token).await?;
        miseprintln!("{}", String::from_utf8_lossy(manifest_bytes.as_ref()));
        Ok(())
    }
}

/// Print the OCI referrers index for a Wings artifact.
///
/// Examples:
///
/// ```sh
/// $ mise wings inspect referrers registry.mise-wings.en.dev/acme/node@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
/// {"schemaVersion":2,"mediaType":"application/vnd.oci.image.index.v1+json","manifests":[{"mediaType":"application/vnd.oci.image.manifest.v1+json","artifactType":"application/spdx+json","digest":"sha256:..."}]}
/// ```
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment)]
struct Referrers {
    /// Wings OCI reference, including @sha256:<digest> or paired with --digest.
    reference: String,

    /// Subject manifest digest when the reference does not include one.
    #[clap(long)]
    digest: Option<String>,
}

impl Referrers {
    async fn run(self) -> Result<()> {
        let target = InspectTarget::new(&self.reference, self.digest.as_deref(), true)?;
        let token = require_cli_token().await?;
        let index = fetch_referrers(&target, &token).await?;
        miseprintln!("{}", serde_json::to_string_pretty(&index)?);
        Ok(())
    }
}

/// Print the first SPDX or CycloneDX SBOM attached to a Wings artifact.
///
/// Examples:
///
/// ```sh
/// $ mise wings inspect sbom registry.mise-wings.en.dev/acme/node@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
/// {"spdxVersion":"SPDX-2.3","name":"node-20.11.1-linux-x64","packages":[]}
/// ```
///
/// If no SBOM is attached:
///
/// ```sh
/// $ mise wings inspect sbom registry.mise-wings.en.dev/acme/node@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
/// no SBOM referrer found for sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa; Wings may not have published SBOM referrers for this artifact yet
/// ```
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment)]
struct Sbom {
    /// Wings OCI reference, including @sha256:<digest> or paired with --digest.
    reference: String,

    /// Subject manifest digest when the reference does not include one.
    #[clap(long)]
    digest: Option<String>,
}

impl Sbom {
    async fn run(self) -> Result<()> {
        let target = InspectTarget::new(&self.reference, self.digest.as_deref(), true)?;
        let token = require_cli_token().await?;
        let index = fetch_referrers(&target, &token).await?;
        let Some(descriptor) = index
            .manifests
            .iter()
            .find(|descriptor| descriptor.is_sbom())
        else {
            bail!(
                "no SBOM referrer found for {}; Wings may not have published SBOM referrers for this artifact yet",
                target.subject_digest()
            );
        };

        let blob_descriptor = resolve_sbom_blob(&target.reference, descriptor, &token).await?;
        let blob = fetch_blob(
            &target.reference,
            &blob_descriptor.digest,
            &token,
            &[blob_descriptor.accept()],
        )
        .await?;
        ensure_digest(blob.as_ref(), &blob_descriptor.digest, "SBOM blob")?;
        miseprintln!("{}", String::from_utf8_lossy(blob.as_ref()));
        Ok(())
    }
}

#[derive(Debug)]
struct InspectTarget {
    reference: WingsReference,
    reference_or_digest: String,
    digest: Option<String>,
}

impl InspectTarget {
    fn new(reference: &str, digest: Option<&str>, require_digest: bool) -> Result<Self> {
        let parsed = WingsReference::parse(reference)?;
        let digest = digest
            .map(str::to_owned)
            .or_else(|| digest_from_reference(reference));
        if require_digest {
            ensure!(
                digest.is_some(),
                "missing subject digest; pass a reference with @sha256:<digest> or add --digest sha256:<digest>"
            );
        }
        if let Some(digest) = digest.as_deref() {
            ensure_sha256_digest(digest)?;
        }

        let reference_or_digest = digest
            .clone()
            .or_else(|| tag_from_reference(reference))
            .unwrap_or_else(|| "latest".to_string());

        Ok(Self {
            reference: parsed,
            reference_or_digest,
            digest,
        })
    }

    fn manifest_reference(&self) -> &str {
        self.digest.as_deref().unwrap_or(&self.reference_or_digest)
    }

    fn subject_digest(&self) -> &str {
        self.digest
            .as_deref()
            .expect("subject digest is required by this command")
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReferrersIndex {
    schema_version: u8,
    media_type: String,
    manifests: Vec<ReferrerDescriptor>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReferrerDescriptor {
    media_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    artifact_type: Option<String>,
    digest: String,
    size: u64,
}

impl ReferrerDescriptor {
    fn is_sbom(&self) -> bool {
        matches!(
            self.artifact_type.as_deref().unwrap_or(&self.media_type),
            MEDIA_TYPE_SPDX_SBOM | MEDIA_TYPE_CYCLONEDX_SBOM
        )
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EvidenceManifest {
    #[serde(default)]
    artifact_type: Option<String>,
    #[serde(default)]
    layers: Vec<EvidenceDescriptor>,
    #[serde(default)]
    blobs: Vec<EvidenceDescriptor>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EvidenceDescriptor {
    media_type: String,
    digest: String,
}

impl EvidenceDescriptor {
    fn is_sbom(&self) -> bool {
        matches!(
            self.media_type.as_str(),
            MEDIA_TYPE_SPDX_SBOM | MEDIA_TYPE_CYCLONEDX_SBOM
        )
    }

    fn accept(&self) -> &str {
        &self.media_type
    }
}

async fn require_cli_token() -> Result<String> {
    crate::wings::auth::session_token_for_cli()
        .await?
        .ok_or_else(|| eyre::eyre!("not signed in to mise-wings; run `mise wings login`"))
}

async fn fetch_manifest(target: &InspectTarget, token: &str) -> Result<Vec<u8>> {
    fetch_manifest_reference(
        target,
        target.manifest_reference(),
        token,
        target.digest.as_deref(),
    )
    .await
}

async fn fetch_manifest_reference(
    target: &InspectTarget,
    manifest_reference: &str,
    token: &str,
    expected_digest: Option<&str>,
) -> Result<Vec<u8>> {
    let url = format!(
        "https://{}/v2/{}/manifests/{}",
        target.reference.registry, target.reference.repository, manifest_reference
    );
    let headers = registry_headers(token, &[MEDIA_TYPE_OCI_MANIFEST])?;
    let bytes = crate::http::HTTP
        .get_bytes_with_headers(&url, &headers)
        .await
        .wrap_err_with(|| format!("fetching wings OCI manifest {manifest_reference}"))?;
    if let Some(digest) = expected_digest {
        ensure_digest(bytes.as_ref(), digest, "manifest")?;
    }
    Ok(bytes.as_ref().to_vec())
}

async fn fetch_referrers(target: &InspectTarget, token: &str) -> Result<ReferrersIndex> {
    let url = format!(
        "https://{}/v2/{}/referrers/{}",
        target.reference.registry,
        target.reference.repository,
        target.subject_digest()
    );
    let headers = registry_headers(token, &[MEDIA_TYPE_OCI_IMAGE_INDEX])?;
    let bytes = crate::http::HTTP
        .get_bytes_with_headers(&url, &headers)
        .await
        .wrap_err_with(|| format!("fetching wings OCI referrers {}", target.subject_digest()))?;
    serde_json::from_slice(bytes.as_ref()).wrap_err("decoding wings OCI referrers index")
}

async fn fetch_blob(
    reference: &WingsReference,
    digest: &str,
    token: &str,
    accept: &[&str],
) -> Result<Vec<u8>> {
    ensure_sha256_digest(digest)?;
    let url = format!(
        "https://{}/v2/{}/blobs/{}",
        reference.registry, reference.repository, digest
    );
    let headers = registry_headers(token, accept)?;
    let bytes = crate::http::HTTP
        .get_bytes_with_headers(&url, &headers)
        .await
        .wrap_err_with(|| format!("fetching wings OCI blob {digest}"))?;
    Ok(bytes.as_ref().to_vec())
}

async fn resolve_sbom_blob(
    reference: &WingsReference,
    descriptor: &ReferrerDescriptor,
    token: &str,
) -> Result<EvidenceDescriptor> {
    if descriptor.media_type == MEDIA_TYPE_SPDX_SBOM
        || descriptor.media_type == MEDIA_TYPE_CYCLONEDX_SBOM
    {
        return Ok(EvidenceDescriptor {
            media_type: descriptor.media_type.clone(),
            digest: descriptor.digest.clone(),
        });
    }

    let target = InspectTarget {
        reference: reference.clone(),
        reference_or_digest: descriptor.digest.clone(),
        digest: Some(descriptor.digest.clone()),
    };
    let manifest_bytes =
        fetch_manifest_reference(&target, &descriptor.digest, token, Some(&descriptor.digest))
            .await?;
    let manifest: EvidenceManifest = serde_json::from_slice(&manifest_bytes)
        .wrap_err("decoding wings SBOM referrer manifest")?;
    ensure!(
        manifest.artifact_type.as_deref().is_some_and(|media_type| {
            media_type == MEDIA_TYPE_SPDX_SBOM || media_type == MEDIA_TYPE_CYCLONEDX_SBOM
        }),
        "referrer {} is not an SBOM artifact",
        descriptor.digest
    );

    manifest
        .blobs
        .iter()
        .chain(manifest.layers.iter())
        .find(|blob| blob.is_sbom())
        .cloned()
        .ok_or_else(|| {
            eyre::eyre!(
                "SBOM referrer {} does not contain an SPDX or CycloneDX blob",
                descriptor.digest
            )
        })
}

fn digest_from_reference(reference: &str) -> Option<String> {
    reference
        .rsplit_once('@')
        .map(|(_, digest)| digest.to_string())
}

fn tag_from_reference(reference: &str) -> Option<String> {
    let without_digest = reference
        .split_once('@')
        .map_or(reference, |(name, _)| name);
    match without_digest.rsplit_once(':') {
        Some((_, tag)) if !tag.contains('/') => Some(tag.to_string()),
        _ => None,
    }
}

fn ensure_sha256_digest(digest: &str) -> Result<()> {
    let Some(hex) = digest.strip_prefix("sha256:") else {
        bail!("expected sha256 digest, got {digest}");
    };
    ensure!(
        hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit()),
        "expected sha256 digest, got {digest}"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_digest_from_reference() {
        assert_eq!(
            digest_from_reference("registry.example.com/acme/node@sha256:aaaaaaaa"),
            Some("sha256:aaaaaaaa".into())
        );
    }

    #[test]
    fn extracts_tag_from_reference() {
        assert_eq!(
            tag_from_reference("registry.example.com/acme/node:20"),
            Some("20".into())
        );
        assert_eq!(tag_from_reference("registry.example.com/acme/node"), None);
    }

    #[test]
    fn validates_sha256_digests() {
        let digest = format!("sha256:{}", "a".repeat(64));
        ensure_sha256_digest(&digest).unwrap();
        assert!(ensure_sha256_digest("sha256:abc").is_err());
        assert!(ensure_sha256_digest("sha512:abc").is_err());
    }
}
