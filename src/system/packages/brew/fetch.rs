//! Bottle downloads from ghcr.io with sha256 verification.

use std::path::PathBuf;

use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};

use super::api::BottleFile;
use crate::http::HTTP_FETCH;
use crate::result::Result;

/// Download a bottle to the mise cache (or reuse a verified cached copy).
pub async fn fetch_bottle(name: &str, pkg_version: &str, bottle: &BottleFile) -> Result<PathBuf> {
    let cache_dir = crate::dirs::CACHE.join("system-brew").join("bottles");
    let path = cache_dir.join(format!("{name}-{pkg_version}.tar.gz"));
    if path.exists() && crate::hash::ensure_checksum(&path, &bottle.sha256, None, "sha256").is_ok()
    {
        debug!("bottle cache hit: {}", path.display());
        return Ok(path);
    }
    // ghcr.io allows anonymous pulls with this static bearer token
    let mut headers = HeaderMap::new();
    headers.insert(AUTHORIZATION, HeaderValue::from_static("Bearer QQ=="));
    HTTP_FETCH
        .download_file_with_headers(&bottle.url, &path, &headers, None)
        .await?;
    crate::hash::ensure_checksum(&path, &bottle.sha256, None, "sha256")?;
    Ok(path)
}
