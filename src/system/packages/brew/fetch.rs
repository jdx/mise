//! Bottle downloads from ghcr.io with sha256 verification.

use std::path::PathBuf;

use eyre::WrapErr;
use reqwest::header::{ACCEPT, AUTHORIZATION, HeaderMap, HeaderValue};
use serde_json::Value;

use super::api::BottleFile;
use crate::http::HTTP;
use crate::result::Result;
use crate::ui::progress_report::SingleReport;

#[derive(Debug, Clone)]
pub struct OciBottleMetadata {
    pub tab: Value,
    pub sbom_supplement: Value,
}

/// Download a bottle to the mise cache (or reuse a verified cached copy).
pub async fn fetch_bottle(
    name: &str,
    pkg_version: &str,
    bottle: &BottleFile,
    pr: Option<&dyn SingleReport>,
) -> Result<PathBuf> {
    let cache_dir = crate::dirs::CACHE.join("system-brew").join("bottles");
    let path = cache_dir.join(format!("{name}-{pkg_version}.tar.gz"));
    if path.exists() && crate::hash::ensure_checksum(&path, &bottle.sha256, None, "sha256").is_ok()
    {
        debug!("bottle cache hit: {}", path.display());
        return Ok(path);
    }
    if let Some(pr) = pr {
        pr.set_message(format!("download {name}-{pkg_version}.tar.gz"));
    }
    // ghcr.io allows anonymous pulls with this static bearer token
    let mut headers = HeaderMap::new();
    headers.insert(AUTHORIZATION, HeaderValue::from_static("Bearer QQ=="));
    HTTP.download_file_with_headers(&bottle.url, &path, &headers, pr)
        .await?;
    if let Some(pr) = pr {
        pr.set_message("checksum".to_string());
    }
    crate::hash::ensure_checksum(&path, &bottle.sha256, pr, "sha256")?;
    Ok(path)
}

/// Fetch digest-bound GHCR metadata. A URL without an OCI blob identity is an
/// archive bottle and must derive facts from its checksum-verified archive.
pub async fn fetch_oci_bottle_metadata(
    name: &str,
    pkg_version: &str,
    tag: &str,
    bottle: &BottleFile,
) -> Result<Option<OciBottleMetadata>> {
    let Some((registry, _)) = bottle.url.split_once("/blobs/") else {
        return Ok(None);
    };
    let url = format!("{registry}/manifests/{pkg_version}");
    let mut headers = HeaderMap::new();
    headers.insert(AUTHORIZATION, HeaderValue::from_static("Bearer QQ=="));
    headers.insert(
        ACCEPT,
        HeaderValue::from_static(
            "application/vnd.oci.image.index.v1+json, application/vnd.oci.image.manifest.v1+json",
        ),
    );
    let response = HTTP
        .get_async_with_headers_allow_error_status(url, &headers)
        .await?
        .error_for_status()?;
    let index: Value = response.json().await?;
    let expected_ref = format!("{pkg_version}.{tag}");
    let descriptor = index
        .get("manifests")
        .and_then(Value::as_array)
        .and_then(|manifests| {
            manifests.iter().find(|manifest| {
                let annotations = manifest.get("annotations");
                annotations
                    .and_then(|value| value.get("sh.brew.bottle.digest"))
                    .and_then(Value::as_str)
                    == Some(bottle.sha256.as_str())
                    || annotations
                        .and_then(|value| value.get("org.opencontainers.image.ref.name"))
                        .and_then(Value::as_str)
                        == Some(expected_ref.as_str())
            })
        })
        .ok_or_else(|| eyre::eyre!("brew:{name}: OCI index has no descriptor for {tag}"))?;
    let annotations = descriptor
        .get("annotations")
        .and_then(Value::as_object)
        .ok_or_else(|| eyre::eyre!("brew:{name}: OCI descriptor has no annotations"))?;
    let tab = annotations
        .get("sh.brew.tab")
        .and_then(Value::as_str)
        .ok_or_else(|| eyre::eyre!("brew:{name}: OCI descriptor has no sh.brew.tab"))?;
    let sbom = annotations
        .get("sh.brew.sbom.supplement")
        .and_then(Value::as_str)
        .ok_or_else(|| eyre::eyre!("brew:{name}: OCI descriptor has no SBOM supplement"))?;
    Ok(Some(OciBottleMetadata {
        tab: serde_json::from_str(tab)
            .wrap_err_with(|| format!("brew:{name}: invalid sh.brew.tab annotation"))?,
        sbom_supplement: serde_json::from_str(sbom)
            .wrap_err_with(|| format!("brew:{name}: invalid SBOM supplement annotation"))?,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn non_oci_bottle_is_explicitly_archive_backed() {
        let bottle = BottleFile {
            cellar: ":any".to_string(),
            url: "https://github.com/example/tap/releases/download/v1/tool.tar.gz".to_string(),
            sha256: "abc123".to_string(),
        };
        assert!(
            fetch_oci_bottle_metadata("tool", "1.0.0", "arm64_sonoma", &bottle)
                .await
                .unwrap()
                .is_none()
        );
    }
}
