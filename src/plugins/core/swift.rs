use crate::backend::platform_target::PlatformTarget;
use crate::cli::args::BackendArg;
use crate::cmd::CmdLineRunner;
use crate::config::Settings;
use crate::http::HTTP;
use crate::install_context::InstallContext;
use crate::lockfile::PlatformInfo;
use crate::platform::linux_os_release;
use crate::toolset::{ToolRequest, ToolVersion};
use crate::ui::progress_report::SingleReport;
use crate::{backend::Backend, backend::VersionInfo, config::Config};
use crate::{file, github, gpg, plugins};
use async_trait::async_trait;
use eyre::{Result, bail};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};
use tempfile::tempdir_in;

/// Lockfile option recording which distro build a lock entry describes.
/// Swift's Linux artifacts differ per distro (`ubuntu24.04`, `fedora39`,
/// `ubi9`, `amazonlinux2`, …) and the `<os>-<arch>` platform key can't encode
/// that, so the distro is stored as an option instead. Entries are matched on
/// options exactly, which keeps one distro's checksum from being applied to
/// another distro's tarball.
const SWIFT_PLATFORM_OPTION: &str = "swift_platform";

#[derive(Debug)]
pub struct SwiftPlugin {
    ba: Arc<BackendArg>,
}

impl SwiftPlugin {
    pub fn new() -> Self {
        Self {
            ba: Arc::new(plugins::core::new_backend_arg("swift")),
        }
    }

    fn swift_bin(&self, tv: &ToolVersion) -> PathBuf {
        tv.install_path().join("bin").join(swift_bin_name())
    }

    fn test_swift(&self, ctx: &InstallContext, tv: &ToolVersion) -> Result<()> {
        ctx.pr.set_message("swift --version".into());
        CmdLineRunner::new(self.swift_bin(tv))
            .with_pr(ctx.pr.as_ref())
            .arg("--version")
            .env_values(tv.install_env())
            .execute()
    }

    async fn download(&self, tv: &ToolVersion, pr: &dyn SingleReport) -> Result<PathBuf> {
        let url = url(tv, &PlatformTarget::from_current());
        let filename = url.split('/').next_back().unwrap();
        let tarball_path = tv.download_path().join(filename);
        if !tarball_path.exists() {
            pr.set_message(format!("download {filename}"));
            HTTP.download_file(&url, &tarball_path, Some(pr)).await?;
        }

        Ok(tarball_path)
    }

    fn install(&self, ctx: &InstallContext, tv: &ToolVersion, tarball_path: &Path) -> Result<()> {
        let filename = tarball_path.file_name().unwrap().to_string_lossy();
        let version = &tv.version;
        ctx.pr.set_message(format!("extract {filename}"));
        if cfg!(macos) {
            let tmp = {
                tempdir_in(tv.install_path().parent().unwrap())?
                    .path()
                    .to_path_buf()
            };
            CmdLineRunner::new(pkgutil_path())
                .arg("--expand-full")
                .arg(tarball_path)
                .arg(&tmp)
                .with_pr(ctx.pr.as_ref())
                .env_values(tv.install_env())
                .execute()?;
            file::remove_all(tv.install_path())?;
            file::rename(
                tmp.join(format!("swift-{version}-RELEASE-osx-package.pkg"))
                    .join("Payload"),
                tv.install_path(),
            )?;
        } else if cfg!(windows) {
            todo!("install from exe");
        } else {
            file::untar(
                tarball_path,
                &tv.install_path(),
                file::ExtractionFormat::TarGz,
                &file::ExtractOptions {
                    strip_components: 1,
                    pr: Some(ctx.pr.as_ref()),
                    ..Default::default()
                },
            )?;
        }
        Ok(())
    }

    fn symlink_bins(&self, tv: &ToolVersion) -> Result<()> {
        let usr_bin = tv.install_path().join("usr").join("bin");
        let bin_dir = tv.install_path().join("bin");
        file::create_dir_all(&bin_dir)?;
        for bin in file::ls(&usr_bin)? {
            if !file::is_executable(&bin) {
                continue;
            }
            let file_name = bin.file_name().unwrap().to_string_lossy().to_string();
            if file_name.contains("swift") || file_name.contains("sourcekit") {
                file::make_symlink_or_copy(&bin, &bin_dir.join(file_name))?;
            }
        }
        Ok(())
    }

    async fn verify_gpg(
        &self,
        ctx: &InstallContext,
        tv: &ToolVersion,
        tarball_path: &Path,
    ) -> Result<()> {
        let sig_path = PathBuf::from(format!("{}.sig", tarball_path.to_string_lossy()));
        // Unlike Node (which skips a missing .sig), this path only runs on Linux, where swift.org
        // publishes a detached signature for every release tarball. A missing .sig is therefore
        // unexpected, so surface the download error rather than silently skipping verification.
        HTTP.download_file(
            format!("{}.sig", url(tv, &PlatformTarget::from_current())),
            &sig_path,
            Some(ctx.pr.as_ref()),
        )
        .await?;
        let signature = file::read(&sig_path)?;
        gpg::verify_swift(tarball_path, &signature)?;
        Ok(())
    }

    fn verify(&self, ctx: &InstallContext, tv: &ToolVersion) -> Result<()> {
        self.test_swift(ctx, tv)
    }
}

#[cfg(macos)]
fn pkgutil_path() -> PathBuf {
    resolve_pkgutil_path(file::which("pkgutil"))
}

#[cfg(not(macos))]
fn pkgutil_path() -> PathBuf {
    PathBuf::from("pkgutil")
}

#[cfg(macos)]
fn resolve_pkgutil_path(which_result: Option<PathBuf>) -> PathBuf {
    if let Some(path) = which_result {
        return path;
    }
    let fallback = PathBuf::from("/usr/sbin/pkgutil");
    if file::is_executable(&fallback) {
        fallback
    } else {
        PathBuf::from("pkgutil")
    }
}

#[cfg(all(test, macos))]
mod tests {
    use super::resolve_pkgutil_path;
    use crate::file;
    use std::path::PathBuf;

    #[test]
    fn resolve_pkgutil_path_prefers_discovered_path() {
        let discovered = PathBuf::from("/tmp/custom/pkgutil");
        assert_eq!(resolve_pkgutil_path(Some(discovered.clone())), discovered);
    }

    #[test]
    fn resolve_pkgutil_path_falls_back_to_system_location() {
        let resolved = resolve_pkgutil_path(None);
        let fallback = PathBuf::from("/usr/sbin/pkgutil");
        if file::is_executable(&fallback) {
            assert_eq!(resolved, fallback);
        } else {
            assert_eq!(resolved, PathBuf::from("pkgutil"));
        }
    }
}

#[async_trait]
impl Backend for SwiftPlugin {
    fn ba(&self) -> &Arc<BackendArg> {
        &self.ba
    }

    /// Swift download URLs are derived from the build host: OS, arch, and—on
    /// Linux—the specific distro (e.g. `ubuntu24.04`, `amazonlinux2`,
    /// `fedora39`, `ubi9`). `mise lock` can only guess the distro of a machine
    /// it isn't running on, so the URL it records for another platform may not
    /// be the one that machine resolves. Opt out of the `--locked` URL
    /// requirement so installs don't hard-fail on a distro the lockfile doesn't
    /// cover; checksums are still verified at install time.
    fn supports_lockfile_url(&self) -> bool {
        false
    }

    /// Record the distro in the lock entry's options so entries from different
    /// distros can't be confused for one another. Without this, a lockfile
    /// written on Ubuntu matches on Fedora — same `linux-x64` key, no options —
    /// and its checksum is checked against the Fedora tarball.
    fn resolve_lockfile_options(
        &self,
        _request: &ToolRequest,
        target: &PlatformTarget,
    ) -> Result<BTreeMap<String, String>> {
        let mut opts = BTreeMap::new();
        if target.os_name() == "linux" {
            opts.insert(SWIFT_PLATFORM_OPTION.to_string(), platform(target));
        }
        Ok(opts)
    }

    /// A lock entry without a `swift_platform` option was written before the
    /// distro was recorded, so which artifact its checksum describes is
    /// unknowable. Such entries still pin the version; their checksum and URL
    /// are ignored and rewritten for this host's distro on install.
    fn lockfile_options_are_host_specific(&self) -> bool {
        true
    }

    async fn resolve_lock_info(
        &self,
        tv: &ToolVersion,
        target: &PlatformTarget,
    ) -> Result<PlatformInfo> {
        // Every published Linux build links against glibc, so there is nothing
        // to lock for a musl target. Fail instead of recording a URL that
        // doesn't exist, so `mise lock` reports it as skipped.
        if target.libc() == Some("musl") {
            bail!("swift does not publish musl builds");
        }
        let url = url(tv, target);
        // Not every distro/arch pair is published — `ubi9` has no aarch64 build,
        // for instance — and which pairs exist changes per release, so ask rather
        // than encode a matrix that would go stale. This keeps a lockfile from
        // recording an artifact that isn't there.
        if let Err(err) = HTTP.head(&url).await {
            bail!("swift does not publish {url}: {err}");
        }
        // swift.org publishes no checksum sidecar (only a detached GPG
        // signature), so a checksum can't be resolved without downloading the
        // whole ~1GB toolchain. Record the URL the entry describes; the checksum
        // is filled in when the tool is installed.
        Ok(PlatformInfo {
            url: Some(url),
            ..Default::default()
        })
    }

    async fn security_info(&self) -> Vec<crate::backend::SecurityFeature> {
        use crate::backend::SecurityFeature;

        let mut features = vec![SecurityFeature::Checksum {
            algorithm: Some("sha256".to_string()),
        }];

        // GPG verification is available on Linux (built-in, no external gpg required)
        if cfg!(target_os = "linux") && Settings::get().swift.gpg_verify != Some(false) {
            features.push(SecurityFeature::Gpg);
        }

        features
    }

    async fn _list_remote_versions(&self, _config: &Arc<Config>) -> Result<Vec<VersionInfo>> {
        let versions = github::list_releases("swiftlang/swift")
            .await?
            .into_iter()
            .filter_map(|r| {
                r.tag_name
                    .strip_prefix("swift-")
                    .and_then(|v| v.strip_suffix("-RELEASE"))
                    .map(|v| (v.to_string(), r.created_at))
            })
            .rev()
            .map(|(version, created_at)| VersionInfo {
                version,
                created_at: Some(created_at),
                ..Default::default()
            })
            .collect();
        Ok(versions)
    }

    async fn _idiomatic_filenames(&self) -> Result<Vec<String>> {
        Ok(vec![".swift-version".into()])
    }

    async fn install_version_(
        &self,
        ctx: &InstallContext,
        mut tv: ToolVersion,
    ) -> Result<ToolVersion> {
        let tarball_path = self.download(&tv, ctx.pr.as_ref()).await?;
        if cfg!(target_os = "linux") && Settings::get().swift.gpg_verify != Some(false) {
            self.verify_gpg(ctx, &tv, &tarball_path).await?;
        }
        self.verify_checksum(ctx, &mut tv, &tarball_path)?;
        self.install(ctx, &tv, &tarball_path)?;
        self.symlink_bins(&tv)?;
        self.verify(ctx, &tv)?;

        Ok(tv)
    }
}

fn swift_bin_name() -> &'static str {
    if cfg!(windows) { "swift.exe" } else { "swift" }
}

fn platform_directory(target: &PlatformTarget) -> String {
    match target.os_name() {
        "macos" => "xcode".into(),
        "windows" => "windows10".into(),
        _ => {
            let platform = platform(target);
            // swift.org files the Linux arm64 builds under a separate
            // `<distro>-aarch64` directory, but only for Ubuntu.
            if platform.starts_with("ubuntu") && target.arch_name() == "arm64" {
                format!("{platform}-aarch64").replace(".", "")
            } else {
                platform.replace(".", "")
            }
        }
    }
}

fn platform(target: &PlatformTarget) -> String {
    match target.os_name() {
        "macos" => "osx".to_string(),
        "windows" => "windows10".to_string(),
        // `swift.platform` names a Linux distro build, so it only applies to
        // Linux targets. Letting it through for every target would build URLs
        // like `.../ubi9/swift-6.3.1-RELEASE-ubi9.pkg` for macOS as soon as the
        // setting is configured repo-wide.
        _ => match &Settings::get().swift.platform {
            Some(platform) => platform.clone(),
            None => linux_platform(target),
        },
    }
}

/// The distro portion of a Linux artifact name. Only the current host's distro
/// can be detected; cross-platform lock resolution has no way to know what
/// distro another machine runs, so it falls back to Ubuntu — the only distro
/// swift.org publishes Linux arm64 builds for, and the most broadly useful
/// default for x64.
fn linux_platform(target: &PlatformTarget) -> String {
    if !target.is_current() {
        return format!("ubuntu{}", DEFAULT_UBUNTU_VERSION);
    }
    let Some(os_release) = linux_os_release() else {
        return "ubi9".to_string();
    };
    if os_release.id == "amzn" {
        format!("amazonlinux{}", os_release.version_id)
    } else if os_release.id == "ubi" {
        "ubi9".to_string() // only 9 is available
    } else if os_release.id == "fedora" {
        "fedora39".to_string() // only 39 is available
    } else if os_release.id == "ubuntu" {
        format!("ubuntu{}", ubuntu_swift_version(&os_release.version_id))
    } else {
        format!("{}{}", os_release.id, os_release.version_id)
    }
}

fn extension(target: &PlatformTarget) -> &'static str {
    match target.os_name() {
        "macos" => "pkg",
        "windows" => "exe",
        _ => "tar.gz",
    }
}

fn architecture(target: &PlatformTarget) -> Option<&str> {
    let arch = target.arch_name();
    match target.os_name() {
        "linux" => match arch {
            "x64" => None,
            "arm64" => Some("aarch64"),
            _ => Some(arch),
        },
        "windows" if arch == "arm64" => Some("arm64"),
        _ => None,
    }
}

/// The newest Ubuntu release swift.org publishes builds for.
const DEFAULT_UBUNTU_VERSION: &str = "24.04";

/// Swift only provides Ubuntu binaries for specific versions.
/// Map unsupported Ubuntu versions to the latest supported one.
fn ubuntu_swift_version(version_id: &str) -> &str {
    match version_id {
        "20.04" | "22.04" | "24.04" => version_id,
        _ => DEFAULT_UBUNTU_VERSION,
    }
}

fn url(tv: &ToolVersion, target: &PlatformTarget) -> String {
    format!(
        "https://download.swift.org/swift-{version}-release/{platform_directory}/swift-{version}-RELEASE/swift-{version}-RELEASE-{platform}{architecture}.{extension}",
        version = tv.version,
        platform = platform(target),
        platform_directory = platform_directory(target),
        extension = extension(target),
        architecture = match architecture(target) {
            Some(arch) => format!("-{arch}"),
            None => "".into(),
        }
    )
}

#[cfg(test)]
mod lockfile_tests {
    use super::*;
    use crate::config::settings::SettingsPartial;
    use crate::platform::Platform;
    use crate::toolset::ToolSource;
    use confique::Layer;

    static TEST_SETTINGS_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    struct SettingsResetGuard {
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    impl Drop for SettingsResetGuard {
        fn drop(&mut self) {
            Settings::reset(None);
        }
    }

    /// Pin `swift.platform` so the assertions don't depend on the distro the
    /// tests happen to run on.
    fn pin_platform(platform: Option<&str>) -> SettingsResetGuard {
        let lock = TEST_SETTINGS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let guard = SettingsResetGuard { _lock: lock };
        let mut settings = SettingsPartial::empty();
        settings.swift.platform = platform.map(str::to_string);
        Settings::reset(Some(settings));
        guard
    }

    fn target(platform: &str) -> PlatformTarget {
        PlatformTarget::new(Platform::parse(platform).expect("valid platform"))
    }

    fn tool_version(backend: &SwiftPlugin, version: &str) -> ToolVersion {
        let request = ToolRequest::new(backend.ba().clone(), version, ToolSource::Unknown)
            .expect("valid swift request");
        ToolVersion::new(request, version.to_string())
    }

    fn options(
        backend: &SwiftPlugin,
        tv: &ToolVersion,
        platform: &str,
    ) -> BTreeMap<String, String> {
        backend
            .resolve_lockfile_options(&tv.request, &target(platform))
            .expect("swift lockfile options")
    }

    #[test]
    fn lockfile_options_record_the_pinned_distro() {
        let _guard = pin_platform(Some("ubi9"));
        let backend = SwiftPlugin::new();
        let tv = tool_version(&backend, "6.3.1");

        assert_eq!(
            options(&backend, &tv, "linux-x64"),
            BTreeMap::from([("swift_platform".to_string(), "ubi9".to_string())])
        );
    }

    /// The distro pin is what keeps a lock entry written for one distro from
    /// being matched — and checksum-verified — against another distro's tarball.
    #[test]
    fn lockfile_options_differ_between_distros() {
        let ubuntu = {
            let _guard = pin_platform(Some("ubuntu24.04"));
            let backend = SwiftPlugin::new();
            let tv = tool_version(&backend, "6.3.1");
            options(&backend, &tv, "linux-x64")
        };
        let fedora = {
            let _guard = pin_platform(Some("fedora39"));
            let backend = SwiftPlugin::new();
            let tv = tool_version(&backend, "6.3.1");
            options(&backend, &tv, "linux-x64")
        };

        assert_ne!(ubuntu, fedora);
    }

    /// macOS and Windows artifacts are not distro-specific, so they keep
    /// option-free entries.
    #[test]
    fn lockfile_options_are_empty_off_linux() {
        let _guard = pin_platform(None);
        let backend = SwiftPlugin::new();
        let tv = tool_version(&backend, "6.3.1");

        assert!(options(&backend, &tv, "macos-arm64").is_empty());
        assert!(options(&backend, &tv, "windows-x64").is_empty());
    }

    /// Locking another platform must build that platform's URL, not the host's.
    #[test]
    fn url_is_built_for_the_target_platform() {
        let _guard = pin_platform(Some("ubuntu24.04"));
        let backend = SwiftPlugin::new();
        let tv = tool_version(&backend, "6.3.1");

        assert_eq!(
            url(&tv, &target("linux-arm64")),
            "https://download.swift.org/swift-6.3.1-release/ubuntu2404-aarch64/swift-6.3.1-RELEASE/swift-6.3.1-RELEASE-ubuntu24.04-aarch64.tar.gz"
        );
    }

    /// `swift.platform` names a Linux distro build. A repo-wide pin must not
    /// leak into the macOS and Windows URLs.
    #[test]
    fn pinned_distro_does_not_apply_off_linux() {
        let _guard = pin_platform(Some("ubi9"));
        let backend = SwiftPlugin::new();
        let tv = tool_version(&backend, "6.3.1");

        assert_eq!(
            url(&tv, &target("macos-arm64")),
            "https://download.swift.org/swift-6.3.1-release/xcode/swift-6.3.1-RELEASE/swift-6.3.1-RELEASE-osx.pkg"
        );
        assert_eq!(
            url(&tv, &target("windows-x64")),
            "https://download.swift.org/swift-6.3.1-release/windows10/swift-6.3.1-RELEASE/swift-6.3.1-RELEASE-windows10.exe"
        );
    }

    #[tokio::test]
    async fn musl_targets_have_nothing_to_lock() {
        let _guard = pin_platform(None);
        let backend = SwiftPlugin::new();
        let tv = tool_version(&backend, "6.3.1");

        assert!(
            backend
                .resolve_lock_info(&tv, &target("linux-x64-musl"))
                .await
                .is_err()
        );
    }

    /// A target that isn't this machine can't be probed for its distro, so
    /// resolution falls back to Ubuntu rather than labeling it with the host's.
    #[test]
    fn foreign_linux_targets_fall_back_to_ubuntu() {
        let _guard = pin_platform(None);
        let backend = SwiftPlugin::new();
        let tv = tool_version(&backend, "6.3.1");
        // riscv64 is never the platform the test suite runs on
        let foreign = target("linux-riscv64");
        assert!(!foreign.is_current());

        assert_eq!(
            options(&backend, &tv, "linux-riscv64"),
            BTreeMap::from([(
                "swift_platform".to_string(),
                format!("ubuntu{DEFAULT_UBUNTU_VERSION}")
            )])
        );
        assert!(url(&tv, &foreign).ends_with("-ubuntu24.04-riscv64.tar.gz"));
    }
}
