use crate::github;
use crate::lockfile::PlatformInfo;
use eyre::Result;

const RUBYINSTALLER_REPO: &str = "oneclick/rubyinstaller2";

/// Check if a Ruby version string is a standard MRI version (starts with a digit).
/// Non-MRI engines like jruby, truffleruby, etc. have prefixed version strings.
pub fn is_mri_version(version: &str) -> bool {
    version.chars().next().is_some_and(|c| c.is_ascii_digit())
}

/// Build the RubyInstaller2 release tag for a given MRI version.
pub fn rubyinstaller_tag(version: &str) -> String {
    format!("RubyInstaller-{version}-1")
}

/// Build the RubyInstaller2 asset filename for a given MRI version.
pub fn rubyinstaller_asset_name(version: &str) -> String {
    // RubyInstaller2 only provides x64 builds
    format!("rubyinstaller-{version}-1-x64.7z")
}

/// Build the RubyInstaller2 download URL for a given MRI version.
pub fn rubyinstaller_url(version: &str) -> String {
    let tag = rubyinstaller_tag(version);
    let asset = rubyinstaller_asset_name(version);
    format!("https://github.com/{RUBYINSTALLER_REPO}/releases/download/{tag}/{asset}")
}

/// Resolve RubyInstaller2 binary URL and checksum from GitHub releases.
/// Returns `Ok(PlatformInfo::default())` for non-MRI versions since
/// RubyInstaller2 only distributes standard MRI Ruby.
pub async fn resolve_rubyinstaller_lock_info(version: &str) -> Result<PlatformInfo> {
    if !is_mri_version(version) {
        return Ok(PlatformInfo::default());
    }

    let tag = rubyinstaller_tag(version);
    let asset_name = rubyinstaller_asset_name(version);

    if let Ok(release) = github::get_release(RUBYINSTALLER_REPO, &tag).await
        && let Some(asset) = release.assets.iter().find(|a| a.name == asset_name)
    {
        return Ok(PlatformInfo {
            url: Some(asset.browser_download_url.clone()),
            checksum: asset.digest.clone(),
            size: None,
            url_api: None,
            conda_deps: None,
        });
    }

    // Fallback: construct URL without checksum
    Ok(PlatformInfo {
        url: Some(rubyinstaller_url(version)),
        checksum: None,
        size: None,
        url_api: None,
        conda_deps: None,
    })
}
