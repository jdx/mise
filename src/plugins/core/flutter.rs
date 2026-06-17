use std::{path::PathBuf, sync::Arc};

use async_trait::async_trait;
use eyre::{Result, bail, eyre};
use serde::Deserialize;

use crate::backend::{Backend, VersionInfo};
use crate::cli::args::BackendArg;
use crate::cmd::CmdLineRunner;
use crate::config::{Config, Settings};
use crate::git::{CloneOptions, Git};
use crate::http::HTTP;
use crate::install_context::InstallContext;
use crate::toolset::{ToolRequest, ToolVersion};
use crate::{file, hash, plugins};

/// Default upstream Flutter repository. Users can point at a fork or an
/// internal mirror with the `remote` (or `url`) tool option.
const DEFAULT_REMOTE: &str = "https://github.com/flutter/flutter.git";

#[derive(Debug)]
pub struct FlutterPlugin {
    ba: Arc<BackendArg>,
}

/// The per-OS `releases_{os}.json` document published by Flutter infra.
#[derive(Debug, Deserialize)]
struct Releases {
    base_url: String,
    releases: Vec<Release>,
}

#[derive(Debug, Deserialize)]
struct Release {
    channel: String,
    version: String,
    archive: String,
    #[serde(default)]
    sha256: Option<String>,
    #[serde(default)]
    dart_sdk_arch: Option<String>,
}

impl FlutterPlugin {
    pub fn new() -> Self {
        Self {
            ba: Arc::new(plugins::core::new_backend_arg("flutter")),
        }
    }

    /// The custom remote configured via the `remote` / `url` option, or the
    /// official upstream repository when unset.
    fn remote(&self, tv: &ToolVersion) -> String {
        let opts = tv.request.options();
        opts.get("remote")
            .or_else(|| opts.get("url"))
            .map(|s| s.to_string())
            .unwrap_or_else(|| DEFAULT_REMOTE.to_string())
    }

    /// Whether this request must be satisfied from git rather than a published
    /// release archive: any `ref:`/`branch:`/`tag:`/`rev:` request, or any
    /// request that overrides the remote (a fork / internal mirror).
    fn is_git_request(&self, tv: &ToolVersion) -> bool {
        if matches!(tv.request, ToolRequest::Ref { .. }) {
            return true;
        }
        let opts = tv.request.options();
        opts.contains_key("remote") || opts.contains_key("url")
    }

    fn flutter_bin(&self, tv: &ToolVersion) -> PathBuf {
        tv.install_path().join("bin").join(flutter_bin_name())
    }

    /// `releases_{os}.json` for the current platform.
    fn releases_url() -> String {
        let os = match Settings::get().os() {
            "macos" => "macos",
            "windows" => "windows",
            _ => "linux",
        };
        format!("https://storage.googleapis.com/flutter_infra_release/releases/releases_{os}.json")
    }

    async fn fetch_releases(&self) -> Result<Releases> {
        HTTP.json::<Releases, _>(Self::releases_url()).await
    }

    /// Map mise's arch name onto Flutter's `dart_sdk_arch` values.
    fn dart_sdk_arch() -> &'static str {
        match Settings::get().arch() {
            "arm64" | "aarch64" => "arm64",
            _ => "x64",
        }
    }

    /// Find the release matching `version` for the current platform/arch.
    fn find_release<'a>(&self, releases: &'a Releases, version: &str) -> Option<&'a Release> {
        let arch = Self::dart_sdk_arch();
        releases
            .releases
            .iter()
            .filter(|r| r.version == version)
            // macOS publishes both x64 and arm64 archives with an explicit
            // `dart_sdk_arch`. Linux omits the field on its (x64-only) archives
            // and publishes no arm64 build, so treat a missing field as x64
            // rather than "any arch" — otherwise an arm64 Linux host would be
            // handed an x64 archive it can't run instead of a clean error.
            .find(|r| match &r.dart_sdk_arch {
                Some(a) => a == arch,
                None => arch == "x64",
            })
    }

    async fn install_release(&self, ctx: &InstallContext, tv: &ToolVersion) -> Result<()> {
        let releases = self.fetch_releases().await?;
        let release = self.find_release(&releases, &tv.version).ok_or_else(|| {
            eyre!(
                "no Flutter release '{}' found for this platform; for a custom build use \
                 'flutter@ref:<commit>' or set the 'remote' option",
                tv.version
            )
        })?;

        let url = format!("{}/{}", releases.base_url, release.archive);
        let filename = release
            .archive
            .rsplit('/')
            .next()
            .unwrap_or(&release.archive);
        let tarball_path = tv.download_path().join(filename);

        ctx.pr.set_message(format!("download {filename}"));
        HTTP.download_file(&url, &tarball_path, Some(ctx.pr.as_ref()))
            .await?;

        // Every official stable archive ships a sha256 in the releases JSON, so
        // a missing checksum means something is wrong upstream — fail closed
        // rather than install an unverified archive.
        let expected = release
            .sha256
            .as_deref()
            .ok_or_else(|| eyre!("missing sha256 for Flutter archive '{}'", release.archive))?;
        ctx.pr.set_message(format!("verify {filename}"));
        hash::ensure_checksum(&tarball_path, expected, Some(ctx.pr.as_ref()), "sha256")?;

        ctx.pr.set_message(format!("extract {filename}"));
        // Flutter archives contain a single top-level `flutter/` directory:
        // `.tar.xz` on Linux, `.zip` on macOS/Windows.
        let format = file::ExtractionFormat::from_file_name(filename);
        if format.is_tar_archive() {
            file::untar(
                &tarball_path,
                &tv.download_path(),
                format,
                &Default::default(),
            )?;
        } else {
            file::unzip(&tarball_path, &tv.download_path(), &Default::default())?;
        }
        let extracted = tv.download_path().join("flutter");
        if !extracted.is_dir() {
            bail!(
                "unexpected Flutter archive layout: missing top-level 'flutter/' in {}",
                file::display_path(&tarball_path)
            );
        }
        file::remove_all(tv.install_path())?;
        file::rename(&extracted, tv.install_path())?;
        Ok(())
    }

    async fn install_git(&self, ctx: &InstallContext, tv: &ToolVersion) -> Result<()> {
        let remote = self.remote(tv);
        let gitref = match &tv.request {
            ToolRequest::Ref { ref_, .. } => ref_.clone(),
            // A remote override on a plain version request: treat the version
            // as a git ref (branch/tag) against the custom remote.
            _ => tv.version.clone(),
        };

        ctx.pr.set_message(format!("clone {remote} @ {gitref}"));
        let git = Git::new(tv.install_path());
        file::remove_all(tv.install_path())?;
        let clone_opts = CloneOptions::default().pr(ctx.pr.as_ref()).branch(&gitref);
        git.clone(&remote, clone_opts)?;
        Ok(())
    }

    /// `flutter --version` validates the install and, for a freshly cloned
    /// commit, triggers the first-run download of the matching Dart SDK +
    /// engine artifacts into the tree.
    fn verify(&self, ctx: &InstallContext, tv: &ToolVersion) -> Result<()> {
        ctx.pr.set_message("flutter --version".into());
        CmdLineRunner::new(self.flutter_bin(tv))
            .with_pr(ctx.pr.as_ref())
            .arg("--version")
            .envs(tv.install_env())
            .execute()
    }
}

#[async_trait]
impl Backend for FlutterPlugin {
    fn ba(&self) -> &Arc<BackendArg> {
        &self.ba
    }

    async fn _list_remote_versions(&self, _config: &Arc<Config>) -> Result<Vec<VersionInfo>> {
        let releases = self.fetch_releases().await?;
        // Default to the stable channel to match prior behavior; beta/dev/master
        // are reachable explicitly via version or `ref:`.
        let mut seen = std::collections::HashSet::new();
        let versions = releases
            .releases
            .into_iter()
            .filter(|r| r.channel == "stable")
            .filter(|r| seen.insert(r.version.clone()))
            .map(|r| VersionInfo {
                version: r.version,
                ..Default::default()
            })
            .collect();
        Ok(versions)
    }

    async fn _idiomatic_filenames(&self) -> Result<Vec<String>> {
        Ok(vec![".flutter-version".into()])
    }

    async fn install_version_(&self, ctx: &InstallContext, tv: ToolVersion) -> Result<ToolVersion> {
        if self.is_git_request(&tv) {
            self.install_git(ctx, &tv).await?;
        } else {
            self.install_release(ctx, &tv).await?;
        }
        self.verify(ctx, &tv)?;
        Ok(tv)
    }
}

fn flutter_bin_name() -> &'static str {
    if cfg!(windows) {
        "flutter.bat"
    } else {
        "flutter"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dart_sdk_arch_is_known() {
        assert!(matches!(FlutterPlugin::dart_sdk_arch(), "arm64" | "x64"));
    }
}
