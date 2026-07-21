//! Bottle downloads from ghcr.io with sha256 verification.

use std::path::PathBuf;

use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};

use super::api::BottleFile;
use crate::http::HTTP_FETCH;
use crate::result::Result;
use crate::ui::progress_report::SingleReport;

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
    HTTP_FETCH
        .download_file_with_headers(&bottle.url, &path, &headers, pr)
        .await?;
    if let Some(pr) = pr {
        pr.set_message("checksum".to_string());
    }
    crate::hash::ensure_checksum(&path, &bottle.sha256, pr, "sha256")?;
    Ok(path)
}
