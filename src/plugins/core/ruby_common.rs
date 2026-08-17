use crate::github;
use crate::lockfile::PlatformInfo;
use eyre::Result;

const RUBYINSTALLER_REPO: &str = "oneclick/rubyinstaller2";

/// Build revision assumed when the release list cannot be consulted. mise used
/// this unconditionally before, so falling back to it keeps offline and
/// API-failure behavior unchanged.
const FALLBACK_BUILD_REVISION: u32 = 1;

/// Check if a Ruby version string is a standard MRI version (starts with a digit).
/// Non-MRI engines like jruby, truffleruby, etc. have prefixed version strings.
pub fn is_mri_version(version: &str) -> bool {
    version.chars().next().is_some_and(|c| c.is_ascii_digit())
}

/// The tag prefix shared by a version's RubyInstaller2 releases, which are
/// tagged `RubyInstaller-<version>-<build revision>`.
fn rubyinstaller_tag_prefix(version: &str) -> String {
    format!("RubyInstaller-{version}")
}

/// Build the RubyInstaller2 release tag for a version and build revision.
pub fn rubyinstaller_tag(version: &str, revision: u32) -> String {
    format!("{}-{revision}", rubyinstaller_tag_prefix(version))
}

/// Build the RubyInstaller2 asset filename for a version and build revision.
pub fn rubyinstaller_asset_name(version: &str, revision: u32) -> String {
    // RubyInstaller2 publishes arm and x86 archives too, but mise only installs x64.
    format!("rubyinstaller-{version}-{revision}-x64.7z")
}

/// Build the RubyInstaller2 download URL for a version and build revision.
pub fn rubyinstaller_url(version: &str, revision: u32) -> String {
    let tag = rubyinstaller_tag(version, revision);
    let asset = rubyinstaller_asset_name(version, revision);
    format!("https://github.com/{RUBYINSTALLER_REPO}/releases/download/{tag}/{asset}")
}

/// A resolved RubyInstaller2 download.
#[derive(Debug, Clone)]
pub struct RubyInstallerArtifact {
    pub url: String,
    pub checksum: Option<String>,
    /// e.g. `rubyinstaller-3.4.4-2-x64.7z`. Only the Windows installer downloads
    /// the archive; elsewhere this type is built solely to record lockfile info.
    #[cfg_attr(not(windows), allow(dead_code))]
    pub filename: String,
}

/// Resolve the archive to download for an MRI version.
///
/// RubyInstaller2 republishes a corrected build of the same Ruby version as
/// `-2`, `-3`, … and leaves the superseded `-1` release in place. Pinning `-1`
/// therefore always installs the build that was corrected, so pick the highest
/// build revision instead. See discussion #5227.
pub async fn resolve_rubyinstaller_artifact(version: &str) -> RubyInstallerArtifact {
    if let Some(artifact) = resolve_from_releases(version).await {
        return artifact;
    }
    RubyInstallerArtifact {
        url: rubyinstaller_url(version, FALLBACK_BUILD_REVISION),
        checksum: None,
        filename: rubyinstaller_asset_name(version, FALLBACK_BUILD_REVISION),
    }
}

async fn resolve_from_releases(version: &str) -> Option<RubyInstallerArtifact> {
    let prefix = rubyinstaller_tag_prefix(version);
    // Passing the prefixed tag lets the shared build-revision picker work
    // unchanged, and reuses the release-list cache that `_list_remote_versions`
    // already fills, so this costs no extra request.
    let (release, _) =
        github::get_release_with_build_revision_status(RUBYINSTALLER_REPO, &prefix, true)
            .await
            .ok()?;
    let revision: u32 = release
        .tag_name
        .strip_prefix(&format!("{prefix}-"))?
        .parse()
        .ok()?;
    let filename = rubyinstaller_asset_name(version, revision);
    let asset = release.assets.iter().find(|a| a.name == filename)?;
    Some(RubyInstallerArtifact {
        url: asset.browser_download_url.clone(),
        checksum: asset.digest.clone(),
        filename,
    })
}

/// Resolve RubyInstaller2 binary URL and checksum from GitHub releases.
/// Returns `Ok(PlatformInfo::default())` for non-MRI versions since
/// RubyInstaller2 only distributes standard MRI Ruby.
#[cfg_attr(windows, allow(dead_code))]
pub async fn resolve_rubyinstaller_lock_info(version: &str) -> Result<PlatformInfo> {
    if !is_mri_version(version) {
        return Ok(PlatformInfo::default());
    }

    // Resolve through the same path the installer uses so a lockfile records the
    // archive that would actually be downloaded.
    let artifact = resolve_rubyinstaller_artifact(version).await;
    Ok(PlatformInfo {
        url: Some(artifact.url),
        checksum: artifact.checksum,
        size: None,
        url_api: None,
        conda_deps: None,
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn tag_and_asset_names_use_the_resolved_revision() {
        assert_eq!(rubyinstaller_tag("3.4.4", 2), "RubyInstaller-3.4.4-2");
        assert_eq!(
            rubyinstaller_asset_name("3.4.4", 2),
            "rubyinstaller-3.4.4-2-x64.7z"
        );
        assert_eq!(
            rubyinstaller_url("3.4.4", 2),
            "https://github.com/oneclick/rubyinstaller2/releases/download/RubyInstaller-3.4.4-2/rubyinstaller-3.4.4-2-x64.7z"
        );
    }

    #[test]
    fn fallback_keeps_the_previous_revision_one_urls() {
        assert_eq!(
            rubyinstaller_url("3.4.4", FALLBACK_BUILD_REVISION),
            "https://github.com/oneclick/rubyinstaller2/releases/download/RubyInstaller-3.4.4-1/rubyinstaller-3.4.4-1-x64.7z"
        );
    }

    #[test]
    fn tag_prefix_does_not_match_a_longer_patch_version() {
        // `3.4.4` must not pick up `3.4.10` releases when tags are compared by prefix.
        let prefix = format!("{}-", rubyinstaller_tag_prefix("3.4.4"));
        assert!(rubyinstaller_tag("3.4.4", 2).starts_with(&prefix));
        assert!(!rubyinstaller_tag("3.4.10", 1).starts_with(&prefix));
    }

    #[test]
    fn is_mri_version_rejects_named_engines() {
        assert!(is_mri_version("3.4.4"));
        assert!(!is_mri_version("jruby-9.4.0.0"));
        assert!(!is_mri_version("truffleruby-24.1.1"));
    }
}
