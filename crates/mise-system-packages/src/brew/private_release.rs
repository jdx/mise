//! Downloads for tap formulae that name a download strategy class of their own.
//!
//! mise cannot run a tap's Ruby download strategy. The common reason for one is
//! a private GitHub repository whose release assets need a token, so mise
//! fetches the asset the way Homebrew's own private-release strategy does:
//! through the API, with the GitHub credentials mise is configured with
//! (`github.credential_command`, `MISE_GITHUB_TOKEN`, ...).

use std::path::Path;

use eyre::{Result, WrapErr, bail};
use mise_ui::progress_report::SingleReport;
use mise_util::http::{HTTP, HTTP_FETCH};
use reqwest::header::{ACCEPT, HeaderMap, HeaderValue};
use serde::Deserialize;

/// `https://github.com/{owner}/{repo}/releases/download/{tag}/{asset}`
#[derive(Debug, PartialEq, Eq)]
struct ReleaseAsset {
    owner: String,
    repo: String,
    tag: String,
    name: String,
}

fn parse_release_url(url: &str) -> Option<ReleaseAsset> {
    let rest = url.strip_prefix("https://github.com/")?;
    let mut parts = rest.split('/');
    let (owner, repo) = (parts.next()?, parts.next()?);
    if parts.next()? != "releases" || parts.next()? != "download" {
        return None;
    }
    let (tag, name) = (parts.next()?, parts.next()?);
    if parts.next().is_some() || [owner, repo, tag, name].iter().any(|p| p.is_empty()) {
        return None;
    }
    let decode = |s: &str| urlencoding::decode(s).map(|s| s.into_owned()).ok();
    Some(ReleaseAsset {
        owner: owner.to_string(),
        repo: repo.to_string(),
        tag: decode(tag)?,
        name: decode(name)?,
    })
}

pub(super) fn is_release_url(url: &str) -> bool {
    parse_release_url(url).is_some()
}

#[derive(Deserialize)]
struct Release {
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    id: u64,
    name: String,
}

/// Download the release asset `url` names through the GitHub API and check it
/// against `sha256`; a file that fails the check is not kept.
pub(super) async fn download(
    url: &str,
    dest: &Path,
    sha256: &str,
    pr: Option<&dyn SingleReport>,
) -> Result<()> {
    let Some(asset) = parse_release_url(url) else {
        bail!("{url} is not a GitHub release asset URL");
    };
    let api = format!(
        "https://api.github.com/repos/{}/{}",
        asset.owner, asset.repo
    );
    let release: Release = HTTP_FETCH
        .json(format!(
            "{api}/releases/tags/{}",
            urlencoding::encode(&asset.tag)
        ))
        .await
        .wrap_err_with(|| {
            format!(
                "failed to read release {} of {}/{}; a private repository needs GitHub credentials \
                 (github.credential_command or MISE_GITHUB_TOKEN)",
                asset.tag, asset.owner, asset.repo
            )
        })?;
    let Some(found) = release.assets.iter().find(|a| a.name == asset.name) else {
        bail!(
            "release {} of {}/{} has no asset named {}",
            asset.tag,
            asset.owner,
            asset.repo,
            asset.name
        );
    };
    let mut headers = HeaderMap::new();
    headers.insert(ACCEPT, HeaderValue::from_static("application/octet-stream"));
    HTTP.download_file_with_headers(
        format!("{api}/releases/assets/{}", found.id),
        dest,
        &headers,
        pr,
    )
    .await?;
    if let Err(err) = mise_util::hash::ensure_checksum(dest, sha256, pr, "sha256") {
        let _ = mise_util::file::remove_file(dest);
        return Err(err);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_release_asset_urls() {
        assert_eq!(
            parse_release_url("https://github.com/acme/tools/releases/download/v1%2B2/w-1.tar.gz"),
            Some(ReleaseAsset {
                owner: "acme".into(),
                repo: "tools".into(),
                tag: "v1+2".into(),
                name: "w-1.tar.gz".into(),
            })
        );
        for url in [
            "https://github.com/acme/tools/archive/refs/tags/v1.tar.gz",
            "https://example.com/acme/tools/releases/download/v1/w.tar.gz",
            "https://github.com/acme/tools/releases/download/v1",
            "https://github.com/acme/tools/releases/download/v1/a/b",
        ] {
            assert!(!is_release_url(url), "{url}");
        }
    }
}
