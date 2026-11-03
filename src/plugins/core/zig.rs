use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};

use crate::args::BackendArg;
use crate::backend::Backend;
use crate::backend::VersionInfo;
use crate::backend::platform_target::PlatformTarget;
use crate::backend::static_helpers::lock_checksum;
use crate::cmd::CmdLineRunner;
use crate::config::{Config, Settings};
use crate::duration::DAILY;
use crate::file::{ExtractOptions, ExtractionFormat};
use crate::http::{HTTP, HTTP_FETCH};
use crate::install_context::InstallContext;
use crate::lockfile::{PlatformInfo, ProvenanceType};
use crate::toolset::ToolVersion;
use crate::ui::progress_report::SingleReport;
use crate::{file, minisign, plugins};
use async_trait::async_trait;
use eyre::Result;
use itertools::Itertools;
use rand::seq::SliceRandom;
use versions::Versioning;
use xx::regex;

#[derive(Debug)]
pub(super) struct ZigPlugin {
    ba: Arc<BackendArg>,
}

const ZIG_MINISIGN_KEY: &str = "RWSGOq2NVecA2UPNdBUZykf1CCb147pkmdtYxgb3Ti+JO/wCYvhbAb/U";
const REQUEST_SUFFIX: &str = "?source=mise-en-place";
const MIRRORS_FILENAME: &str = "community-mirrors.txt";

impl ZigPlugin {
    pub(super) fn new() -> Self {
        Self {
            ba: Arc::new(plugins::core::new_backend_arg("zig")),
        }
    }

    fn zig_bin(&self, tv: &ToolVersion) -> PathBuf {
        if cfg!(windows) {
            tv.install_path().join("zig.exe")
        } else {
            tv.install_path().join("bin").join("zig")
        }
    }

    /// Nightly (`-dev.`) builds are not top-level download-index keys; they are
    /// published under /builds/. Both get_tarball_url and resolve_lock_info need
    /// this URL, so construct it in one place to keep the pattern in sync. (#10251)
    fn nightly_tarball_url(arch: &str, os: &str, version: &str) -> String {
        let ext = if os == "windows" { "zip" } else { "tar.xz" };
        format!("https://ziglang.org/builds/zig-{arch}-{os}-{version}.{ext}")
    }

    /// The release URL for a numbered version, for when the download index
    /// cannot be read. Zig named archives `zig-{os}-{arch}-...` up to 0.14.0
    /// and `zig-{arch}-{os}-...` from 0.14.1 on. (#15349)
    fn release_tarball_url(arch: &str, os: &str, version: &str) -> String {
        let name = Self::archive_name(arch, os, version);
        format!("https://ziglang.org/download/{version}/{name}")
    }

    /// The file name of `version`'s archive for this arch and os.
    fn archive_name(arch: &str, os: &str, version: &str) -> String {
        let ext = if os == "windows" { "zip" } else { "tar.xz" };
        if Self::arch_first(version) {
            format!("zig-{arch}-{os}-{version}.{ext}")
        } else {
            format!("zig-{os}-{arch}-{version}.{ext}")
        }
    }

    /// Whether `version`'s archives put the arch before the os (0.14.1 on).
    fn arch_first(version: &str) -> bool {
        let parts: Vec<u64> = version.split('.').filter_map(|p| p.parse().ok()).collect();
        parts.as_slice() >= [0, 14, 1].as_slice()
    }

    /// Whether a locked `url` can be downloaded for `version` without the
    /// index. The signature only proves the archive matches its own file name,
    /// so that name must be this version's archive for this platform, with no
    /// query to change it. Older mise wrote `zig-{os}-...` names for 0.14.1 and
    /// later, and `.tar.xz` on Windows, when the index was unreadable; those
    /// files do not exist, so the index resolves them.
    fn usable_locked_url(url: &str, version: &str, arch: &str, os: &str) -> bool {
        !url.contains('?')
            && url
                .rsplit('/')
                .next()
                .is_some_and(|filename| filename == Self::archive_name(arch, os, version))
    }

    /// The (arch, os) pair the download index and archive names use.
    fn index_platform(target: &PlatformTarget) -> (&str, &'static str) {
        let arch = match target.arch_name() {
            "x64" => "x86_64",
            "arm64" => "aarch64",
            "arm" => "armv7a",
            "riscv64" => "riscv64",
            other => other,
        };
        let os = match target.os_name() {
            "macos" => "macos",
            "linux" => "linux",
            "freebsd" => "freebsd",
            "windows" => "windows",
            _ => "linux",
        };
        (arch, os)
    }

    fn test_zig(&self, ctx: &InstallContext, tv: &ToolVersion) -> Result<()> {
        ctx.pr.set_message("zig version".into());
        CmdLineRunner::new(self.zig_bin(tv))
            .with_pr(ctx.pr.as_ref())
            .arg("version")
            .env_values(tv.install_env())
            .execute()
    }

    async fn download(&self, tv: &ToolVersion, pr: &dyn SingleReport) -> Result<PathBuf> {
        let settings = Settings::get();
        // A URL the lockfile pins needs no download index, which can be
        // unavailable or unparseable. Minisign still checks the archive. (#15349)
        let target = PlatformTarget::from_current();
        let (arch, os) = Self::index_platform(&target);
        let locked_url = tv
            .lock_platforms
            .get(&self.get_platform_key())
            .and_then(|pi| pi.url.as_deref())
            .filter(|url| Self::usable_locked_url(url, &tv.version, arch, os))
            .map(str::to_string);
        let url = match locked_url {
            Some(url) => url,
            None => self.get_tarball_url(tv, &target).await?.ok_or_else(|| {
                eyre::eyre!("Failed to resolve zig tarball URL for {}", tv.version)
            })?,
        };

        let filename = url.split('/').next_back().unwrap();
        let tarball_path = tv.download_path().join(filename);

        let mut downloaded = false;
        let mut used_url = url.clone();
        // The ziglang.org website kindly asks for trying mirrors for automated downloads,
        // read more on https://ziglang.org/download/community-mirrors/
        let community_mirrors = if url.starts_with("https://ziglang.org") {
            self.get_community_mirrors().await
        } else {
            None
        };

        // The same checksum names the archive whichever host serves it.
        let pinned = lock_checksum(tv, &self.get_platform_key(), &url);
        if settings.zig.use_community_mirrors
            && let Some(mirrors) = community_mirrors.as_ref()
        {
            for i in 0..mirrors.len() {
                let disp_i = i + 1;
                let disp_len = mirrors.len();
                pr.set_message(format!("mirror {disp_i}/{disp_len} {filename}"));

                let mirror_url = &mirrors[i];
                used_url = format!("{mirror_url}/{filename}");

                if HTTP
                    .download_file_stored(
                        format!("{used_url}{REQUEST_SUFFIX}"),
                        &tarball_path,
                        pinned.as_deref(),
                        Some(pr),
                    )
                    .await
                    .is_ok()
                {
                    downloaded = true;
                    break;
                }
            }
        }

        if !downloaded {
            // Try the usual ziglang.org or machengine.org download
            pr.set_message(format!("download {filename}"));
            used_url = url.clone();
            HTTP.download_file_stored(&url, &tarball_path, pinned.as_deref(), Some(pr))
                .await?;
            // If this was ziglang.org and error is not 404 and community_mirrors is None,
            // the user might want to place the mirror list in cache dir by hand
        }

        pr.set_message(format!("minisign {filename}"));
        let tarball_data = file::read(&tarball_path)?;
        // The archive may have come from the download store without contacting
        // `used_url`, so the signature is looked for there first, then on the
        // other hosts. The key is fixed, so any host's signature is checked the
        // same way.
        let mut sig_bases = vec![used_url.clone()];
        if !sig_bases.contains(&url) {
            sig_bases.push(url.clone());
        }
        for mirror in community_mirrors.iter().flatten() {
            let base = format!("{mirror}/{filename}");
            if !sig_bases.contains(&base) {
                sig_bases.push(base);
            }
        }
        // A host only counts once its signature verifies this file, so a stale or
        // broken `.minisig` on one mirror falls through to the next host.
        let mut last_err = None;
        for base in sig_bases {
            let sig = match HTTP
                .get_text(format!("{base}.minisig{REQUEST_SUFFIX}"))
                .await
            {
                Ok(text) => text,
                Err(err) => {
                    last_err = Some(err);
                    continue;
                }
            };
            match Self::verify_signature(&tarball_data, &sig, filename, &base) {
                Ok(()) => return Ok(tarball_path),
                Err(err) => last_err = Some(err),
            }
        }
        Err(last_err.expect("at least one signature host was tried"))
    }

    /// Check `sig` against the archive and that it is the signature for `filename`.
    fn verify_signature(data: &[u8], sig: &str, filename: &str, base: &str) -> Result<()> {
        minisign::verify(ZIG_MINISIGN_KEY, data, sig)?;
        // Since this passed the verify step, the format is guaranteed to be correct
        let trusted_comment = sig.split('\n').nth(2).unwrap_or_default();
        // Verify that this is the desired version using trusted comment to prevent downgrade attacks
        if !trusted_comment.contains(&format!("file:{filename}")) {
            return Err(eyre::eyre!(
                "Expected {}, but signature {}.minisig had:\n{}",
                filename,
                base,
                trusted_comment
            ));
        }
        Ok(())
    }

    fn install(&self, ctx: &InstallContext, tv: &ToolVersion, tarball_path: &Path) -> Result<()> {
        let filename = tarball_path.file_name().unwrap().to_string_lossy();
        ctx.pr.set_message(format!("extract {filename}"));
        file::remove_all(tv.install_path())?;
        file::extract_archive(
            tarball_path,
            &tv.install_path(),
            ExtractionFormat::from_file_name(&filename),
            &ExtractOptions {
                strip_components: 1,
                pr: Some(ctx.pr.as_ref()),
                ..Default::default()
            },
        )?;

        if cfg!(unix) {
            file::create_dir_all(tv.install_path().join("bin"))?;
            file::make_symlink(Path::new("../zig"), &tv.install_path().join("bin/zig"))?;
        }

        Ok(())
    }

    fn verify(&self, ctx: &InstallContext, tv: &ToolVersion) -> Result<()> {
        self.test_zig(ctx, tv)
    }

    async fn get_tarball_url_from_json(
        &self,
        json_url: &str,
        version: &str,
        arch: &str,
        os: &str,
    ) -> Result<String> {
        let version_json: serde_json::Value = HTTP_FETCH.json(json_url).await?;
        let zig_tarball_url = version_json
            .pointer(&format!("/{version}/{arch}-{os}/tarball"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| eyre::eyre!("Failed to get zig tarball url from {:?}", json_url))?;
        Ok(zig_tarball_url.to_string())
    }

    /// Get full download info (tarball URL, shasum, size) from JSON index
    /// Uses cached request since same index is fetched for all platforms
    async fn get_download_info_from_json(
        &self,
        json_url: &str,
        version: &str,
        arch: &str,
        os: &str,
    ) -> Result<(String, Option<String>, Option<u64>)> {
        let version_json: serde_json::Value = HTTP_FETCH.json_cached(json_url).await?;
        let platform_info = version_json
            .pointer(&format!("/{version}/{arch}-{os}"))
            .ok_or_else(|| eyre::eyre!("Failed to get zig platform info from {:?}", json_url))?;

        let tarball_url = platform_info
            .get("tarball")
            .and_then(|v| v.as_str())
            .ok_or_else(|| eyre::eyre!("Failed to get zig tarball url from {:?}", json_url))?
            .to_string();

        let shasum = platform_info
            .get("shasum")
            .and_then(|v| v.as_str())
            .map(|s| format!("sha256:{s}"));

        let size = platform_info
            .get("size")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<u64>().ok());

        Ok((tarball_url, shasum, size))
    }

    async fn get_community_mirrors(&self) -> Option<Vec<String>> {
        let cache_path = self.ba.cache_path().join(MIRRORS_FILENAME);
        let recent_cache =
            file::modified_duration(&cache_path).is_ok_and(|updated_at| updated_at < DAILY);
        if !recent_cache {
            HTTP.download_file(
                &format!("https://ziglang.org/download/{MIRRORS_FILENAME}"),
                &cache_path,
                None,
            )
            .await
            .unwrap_or_else(|_| {
                // We can still use an older mirror list
                warn!("{}: Could not download {}", self.ba, MIRRORS_FILENAME);
            });
        }

        let mirror_list = String::from_utf8(file::read(cache_path).ok()?).ok()?;
        let mut mirrors: Vec<String> = mirror_list
            .split('\n')
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect();
        let mut rng = rand::rng();
        mirrors.shuffle(&mut rng);
        Some(mirrors)
    }
}

#[async_trait]
impl Backend for ZigPlugin {
    fn ba(&self) -> &Arc<BackendArg> {
        &self.ba
    }

    async fn security_info(&self) -> Vec<crate::backend::SecurityFeature> {
        use crate::backend::SecurityFeature;

        vec![
            SecurityFeature::Checksum {
                algorithm: Some("sha256".to_string()),
            },
            SecurityFeature::Minisign {
                public_key: Some(ZIG_MINISIGN_KEY.to_string()),
            },
        ]
    }

    async fn _list_remote_versions(&self, _config: &Arc<Config>) -> Result<Vec<VersionInfo>> {
        let indexes = [
            "https://ziglang.org/download/index.json",
            // "https://machengine.org/zig/index.json", // need to handle mach's CalVer
        ];
        let mut versions: Vec<(String, Option<String>)> = Vec::new();

        for index in indexes {
            let index_json: serde_json::Value = HTTP_FETCH.json(index).await?;
            let index_obj = index_json
                .as_object()
                .ok_or_else(|| eyre::eyre!("Failed to get zig version from {:?}", index))?;

            for (version, data) in index_obj {
                let date = data.get("date").and_then(|d| d.as_str()).map(String::from);
                versions.push((version.clone(), date));
            }
        }

        let versions = versions
            .into_iter()
            .unique_by(|(v, _)| v.clone())
            .sorted_by_cached_key(|(s, _)| (Versioning::new(s), s.to_string()))
            .map(|(version, date)| VersionInfo {
                version,
                created_at: date,
                ..Default::default()
            })
            .collect();

        Ok(versions)
    }

    fn is_rolling_channel(&self, version: &str) -> bool {
        version == "master"
    }

    fn latest_installed_channel_version(&self, channel: &str) -> Option<String> {
        if !self.is_rolling_channel(channel) {
            return None;
        }
        // "master" builds are dev versions (e.g. 0.17.0-dev.NNN). Reuse the latest
        // installed nightly so hook-env / exec stay network-free, but never fall
        // back to a stable release that has nothing to do with the channel. (#10251)
        //
        // list_installed_versions() is already ordered ascending by install_state
        // (the canonical version sort), so take the last "-dev." entry instead of
        // re-sorting here. list_installed_versions_matching() is unusable: it drops
        // prereleases (the "-dev." nightlies) unless the tool opts into them.
        // rfind() walks from the newest end (the list is double-ended), giving the
        // latest nightly without re-sorting.
        self.list_installed_versions()
            .into_iter()
            .rfind(|v| v.contains("-dev."))
    }

    async fn resolve_channel_version(
        &self,
        _config: &Arc<Config>,
        version: &str,
    ) -> Result<Option<String>> {
        if !self.is_rolling_channel(version) {
            return Ok(None);
        }
        // The download index lists "master" as a moving channel whose `version`
        // field is the concrete nightly it currently points at (e.g.
        // "0.17.0-dev.836+e357134f0"). Resolve to that concrete version so the
        // install lands in a versioned dir and `mise upgrade`/`outdated` can track
        // new nightlies instead of pinning "master" forever. (#10251)
        // Treat a failed fetch as "cannot resolve right now" (Ok(None)) rather than
        // a hard error, so a transient network blip falls through to normal
        // resolution instead of breaking hook-env / exec. (#10251)
        let index_json: serde_json::Value = match HTTP_FETCH
            .json("https://ziglang.org/download/index.json")
            .await
        {
            Ok(v) => v,
            Err(err) => {
                debug!("zig: failed to fetch download index resolving {version}: {err:#}");
                return Ok(None);
            }
        };
        Ok(index_json
            .pointer(&format!("/{version}/version"))
            .and_then(|v| v.as_str())
            .map(String::from))
    }

    async fn list_bin_paths(
        &self,
        _config: &Arc<Config>,
        tv: &ToolVersion,
    ) -> Result<Vec<PathBuf>> {
        if cfg!(windows) {
            Ok(vec![tv.install_path()])
        } else {
            Ok(vec![tv.install_path().join("bin")])
        }
    }

    async fn install_version_(
        &self,
        ctx: &InstallContext,
        mut tv: ToolVersion,
    ) -> Result<ToolVersion> {
        // download() unconditionally verifies minisign (not behind a settings check).
        // If minisign verification fails, download() returns Err and we never reach
        // the provenance recording below. This is safe to record unconditionally.
        let tarball_path = self.download(&tv, ctx.pr.as_ref()).await?;

        // Enforce lockfile provenance expectation
        let platform_key = PlatformTarget::from_current().to_key();
        let locked_provenance = tv
            .lock_platforms
            .get_mut(&platform_key)
            .and_then(|pi| pi.provenance.take());

        if let Some(ref expected) = locked_provenance
            && !expected.is_minisign()
        {
            return Err(eyre::eyre!(
                "Lockfile requires {expected} provenance for {tv} but minisign was used. \
                     This may indicate a downgrade attack."
            ));
        }

        // Record minisign provenance — only reached if download() (and its
        // minisign::verify call) succeeded
        let pi = tv.lock_platforms.entry(platform_key.clone()).or_default();
        pi.provenance = Some(ProvenanceType::Minisign);

        ctx.pr.next_operation();
        self.verify_checksum(ctx, &mut tv, &tarball_path)?;
        ctx.pr.next_operation();
        self.install(ctx, &tv, &tarball_path)?;
        self.verify(ctx, &tv)?;
        Ok(tv)
    }

    async fn get_tarball_url(
        &self,
        tv: &ToolVersion,
        target: &PlatformTarget,
    ) -> Result<Option<String>> {
        let indexes = HashMap::from([
            ("zig", "https://ziglang.org/download/index.json"),
            ("mach", "https://machengine.org/zig/index.json"),
        ]);

        let (arch, os) = Self::index_platform(target);

        let (json_url, version) = if regex!(r"^mach-|-mach$").is_match(&tv.version) {
            (indexes["mach"], tv.version.as_str())
        } else {
            (indexes["zig"], tv.version.as_str())
        };

        match self
            .get_tarball_url_from_json(json_url, version, arch, os)
            .await
        {
            Ok(url) => Ok(Some(url)),
            Err(err) if regex!(r"^\d+\.\d+\.\d+$").is_match(&tv.version) => {
                // Fallback: construct URL directly for numbered versions
                warn!("zig: {err:#}; building the {version} URL without the download index");
                Ok(Some(Self::release_tarball_url(arch, os, &tv.version)))
            }
            Err(_) if tv.version.contains("-dev.") => {
                // Resolved from a rolling channel (e.g. "master"): nightly builds
                // are not top-level index keys; they live under /builds/. (#10251)
                Ok(Some(Self::nightly_tarball_url(arch, os, &tv.version)))
            }
            Err(_) => Ok(None),
        }
    }

    async fn resolve_lock_info(
        &self,
        tv: &ToolVersion,
        target: &PlatformTarget,
    ) -> Result<PlatformInfo> {
        let indexes = HashMap::from([
            ("zig", "https://ziglang.org/download/index.json"),
            ("mach", "https://machengine.org/zig/index.json"),
        ]);

        let (arch, os) = Self::index_platform(target);

        let (json_url, version) = if regex!(r"^mach-|-mach$").is_match(&tv.version) {
            (indexes["mach"], tv.version.as_str())
        } else {
            (indexes["zig"], tv.version.as_str())
        };

        // Try to get full info from JSON (includes checksum and size)
        // Don't pre-set provenance at lock time — minisign verification hasn't run yet.
        // Provenance is recorded in install_version_ after download() confirms minisign
        // verification succeeded.
        match self
            .get_download_info_from_json(json_url, version, arch, os)
            .await
        {
            Ok((url, checksum, size)) => Ok(PlatformInfo {
                url: Some(url),
                checksum,
                size,
                ..Default::default()
            }),
            Err(err) if regex!(r"^\d+\.\d+\.\d+$").is_match(&tv.version) => {
                // Fallback: construct URL directly for numbered versions (no checksum available)
                // Don't pre-set provenance here — record it only after download() confirms
                // minisign verification succeeded during install
                warn!("zig: {err:#}; locking the {version} URL without a checksum");
                Ok(PlatformInfo {
                    url: Some(Self::release_tarball_url(arch, os, &tv.version)),
                    ..Default::default()
                })
            }
            Err(_) if tv.version.contains("-dev.") => {
                // Resolved from a rolling channel (e.g. "master"): nightly lives
                // under /builds/, no checksum (minisign verifies at install). (#10251)
                Ok(PlatformInfo {
                    url: Some(Self::nightly_tarball_url(arch, os, &tv.version)),
                    ..Default::default()
                })
            }
            Err(_) => Ok(PlatformInfo::default()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ZigPlugin;

    #[test]
    fn test_release_tarball_url_follows_the_naming_change() {
        assert_eq!(
            ZigPlugin::release_tarball_url("x86_64", "linux", "0.17.0"),
            "https://ziglang.org/download/0.17.0/zig-x86_64-linux-0.17.0.tar.xz"
        );
        assert_eq!(
            ZigPlugin::release_tarball_url("x86_64", "linux", "0.14.1"),
            "https://ziglang.org/download/0.14.1/zig-x86_64-linux-0.14.1.tar.xz"
        );
        assert_eq!(
            ZigPlugin::release_tarball_url("aarch64", "macos", "0.14.0"),
            "https://ziglang.org/download/0.14.0/zig-macos-aarch64-0.14.0.tar.xz"
        );
        assert_eq!(
            ZigPlugin::release_tarball_url("x86_64", "windows", "0.17.0"),
            "https://ziglang.org/download/0.17.0/zig-x86_64-windows-0.17.0.zip"
        );
        assert_eq!(
            ZigPlugin::release_tarball_url("x86_64", "windows", "0.13.0"),
            "https://ziglang.org/download/0.13.0/zig-windows-x86_64-0.13.0.zip"
        );
    }

    #[test]
    fn test_locked_url_must_name_the_version_and_platform() {
        let url = ZigPlugin::release_tarball_url("x86_64", "linux", "0.15.1");
        let usable = ZigPlugin::usable_locked_url;
        assert!(usable(&url, "0.15.1", "x86_64", "linux"));
        assert!(!usable(&url, "0.14.1", "x86_64", "linux"));
        assert!(!usable(&url, "5.1", "x86_64", "linux"));
        assert!(!usable(&url, "0.15.1", "aarch64", "linux"));
        assert!(!usable(&url, "0.15.1", "x86_64", "macos"));
        assert!(!usable(&format!("{url}?x=1"), "0.15.1", "x86_64", "linux"));
        let windows = ZigPlugin::release_tarball_url("x86_64", "windows", "0.13.0");
        assert!(usable(&windows, "0.13.0", "x86_64", "windows"));
        let nightly = ZigPlugin::nightly_tarball_url("aarch64", "macos", "0.16.0-dev.1+abc");
        assert!(usable(&nightly, "0.16.0-dev.1+abc", "aarch64", "macos"));
        // What the old fallback wrote for releases that use the new order.
        let old_fallback = "https://ziglang.org/download/0.17.0/zig-linux-x86_64-0.17.0.tar.xz";
        assert!(!usable(old_fallback, "0.17.0", "x86_64", "linux"));
        let old_windows = "https://ziglang.org/download/0.13.0/zig-windows-x86_64-0.13.0.tar.xz";
        assert!(!usable(old_windows, "0.13.0", "x86_64", "windows"));
    }
}
