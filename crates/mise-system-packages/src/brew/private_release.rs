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
    // Parsing the URL drops any query string or fragment from the asset name.
    let url = url::Url::parse(url).ok()?;
    if url.scheme() != "https" || url.host_str() != Some("github.com") {
        return None;
    }
    let mut parts = url.path_segments()?;
    let (owner, repo) = (parts.next()?, parts.next()?);
    if parts.next()? != "releases" || parts.next()? != "download" {
        return None;
    }
    // A tag may hold slashes (`release/v1`), so the asset name is the last
    // segment and everything between `download` and it is the tag.
    let rest: Vec<&str> = parts.collect();
    let (name, tag_parts) = rest.split_last()?;
    if tag_parts.is_empty() || [owner, repo, name].iter().any(|p| p.is_empty()) {
        return None;
    }
    let decode = |s: &str| urlencoding::decode(s).map(|s| s.into_owned()).ok();
    let tag = tag_parts
        .iter()
        .map(|p| if p.is_empty() { None } else { decode(p) })
        .collect::<Option<Vec<_>>>()?
        .join("/");
    Some(ReleaseAsset {
        owner: owner.to_string(),
        repo: repo.to_string(),
        tag,
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
            // The tag is one path segment, so a slash in it is escaped.
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
    let asset_url = url::Url::parse(&format!("{api}/releases/assets/{}", found.id))?;
    let mut headers = HeaderMap::new();
    headers.insert(ACCEPT, HeaderValue::from_static("application/octet-stream"));
    // `download_file_with_headers` sends only what it is given, so layer the
    // GitHub credentials for the API host under the `Accept` header.
    let headers = mise_util::http::with_host_auth(&asset_url, &headers)?;
    // A private archive must not be readable by other local users while it is
    // written, so stage it in an owner-only directory (`tempdir_in` is 0700)
    // and move it into place once it has passed the checksum.
    let parent = dest.parent().unwrap_or_else(|| Path::new("."));
    mise_util::file::create_dir_all(parent)?;
    let stage = tempfile::Builder::new()
        .prefix(".mise-private-")
        .tempdir_in(parent)?;
    let staged = stage
        .path()
        .join(dest.file_name().unwrap_or_else(|| "download".as_ref()));
    HTTP.download_file_with_headers(asset_url, &staged, &headers, pr)
        .await?;
    mise_util::hash::ensure_checksum(&staged, sha256, pr, "sha256")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o600))?;
    }
    std::fs::rename(&staged, dest)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_release_tags_containing_slashes() {
        assert_eq!(
            parse_release_url(
                "https://github.com/acme/tools/releases/download/release/v1.0.0/w-1.tar.gz"
            ),
            Some(ReleaseAsset {
                owner: "acme".into(),
                repo: "tools".into(),
                tag: "release/v1.0.0".into(),
                name: "w-1.tar.gz".into(),
            })
        );
        assert_eq!(
            parse_release_url("https://github.com/acme/tools/releases/download/w-1.tar.gz"),
            None
        );
    }

    #[test]
    fn ignores_a_query_string_and_fragment() {
        let asset = parse_release_url(
            "https://github.com/acme/tools/releases/download/v1/w-1.tar.gz?download=1#x",
        )
        .unwrap();
        assert_eq!(asset.name, "w-1.tar.gz");
        assert_eq!(asset.tag, "v1");
    }

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
            "https://github.com/acme/tools/releases/download/v1//w.tar.gz",
        ] {
            assert!(!is_release_url(url), "{url}");
        }
    }
}
