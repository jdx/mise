use crate::backend::platform_target::PlatformTarget;
use crate::cli::args::BackendArg;
use crate::cmd::CmdLineRunner;
use crate::config::Settings;
use crate::http::{HTTP, HTTP_FETCH};
use crate::install_context::InstallContext;
use crate::lockfile::PlatformInfo;
use crate::platform::linux_os_release;
use crate::toolset::{ToolRequest, ToolVersion};
use crate::ui::progress_report::SingleReport;
use crate::{backend::Backend, backend::VersionInfo, config::Config};
use crate::{file, github, gpg, plugins};
use async_trait::async_trait;
use eyre::{Result, bail, eyre};
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
pub(super) struct SwiftPlugin {
    ba: Arc<BackendArg>,
}

impl SwiftPlugin {
    pub(super) fn new() -> Self {
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

    async fn download(
        &self,
        tv: &ToolVersion,
        url: &str,
        pr: &dyn SingleReport,
    ) -> Result<PathBuf> {
        let filename = url.split('/').next_back().unwrap();
        let tarball_path = tv.download_path().join(filename);
        if !tarball_path.exists() {
            pr.set_message(format!("download {filename}"));
            HTTP.download_file(url, &tarball_path, Some(pr)).await?;
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

    async fn verify_gpg(&self, ctx: &InstallContext, url: &str, tarball_path: &Path) -> Result<()> {
        let sig_path = PathBuf::from(format!("{}.sig", tarball_path.to_string_lossy()));
        // Unlike Node (which skips a missing .sig), this path only runs on Linux, where swift.org
        // publishes a detached signature for every release tarball. A missing .sig is therefore
        // unexpected, so surface the download error rather than silently skipping verification.
        HTTP.download_file(format!("{url}.sig"), &sig_path, Some(ctx.pr.as_ref()))
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
            let label = match &Settings::get().swift.platform {
                Some(pinned) => pinned.clone(),
                None => host_distro(target).label(),
            };
            opts.insert(SWIFT_PLATFORM_OPTION.to_string(), label);
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
        let url = url(tv, target, &resolve_platform(tv, target).await?);
        // Which distros are published changes from release to release —
        // `debian12` first appears in 5.10, `ubuntu20.04` is gone by 6.3 — so
        // ask rather than encode a matrix that would go stale. This keeps a
        // lockfile from recording an artifact that isn't there.
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
                let released_at = r.released_at().to_string();
                r.tag_name
                    .strip_prefix("swift-")
                    .and_then(|v| v.strip_suffix("-RELEASE"))
                    .map(|v| (v.to_string(), released_at))
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

    async fn install_version_(
        &self,
        ctx: &InstallContext,
        mut tv: ToolVersion,
    ) -> Result<ToolVersion> {
        let target = PlatformTarget::from_current();
        let url = url(&tv, &target, &resolve_platform(&tv, &target).await?);
        let tarball_path = self.download(&tv, &url, ctx.pr.as_ref()).await?;
        if cfg!(target_os = "linux") && Settings::get().swift.gpg_verify != Some(false) {
            self.verify_gpg(ctx, &url, &tarball_path).await?;
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

fn platform_directory(target: &PlatformTarget, platform: &str) -> String {
    let directory = match target.os_name() {
        "macos" => "xcode".to_string(),
        "windows" => "windows10".to_string(),
        _ => platform.replace(".", ""),
    };
    // x86_64 builds live directly under the platform directory; every other
    // architecture gets its own `<platform>-<arch>` directory — that holds for
    // all the Linux distros, not just Ubuntu, and for Windows on arm64.
    match architecture(target) {
        Some(arch) => format!("{directory}-{arch}"),
        None => directory,
    }
}

/// The distro portion of a Linux artifact name.
///
/// `swift.platform` names a Linux distro build, so it only applies to Linux
/// targets. Letting it through for every target would build URLs like
/// `.../ubi9/swift-6.3.1-RELEASE-ubi9.pkg` for macOS as soon as the setting is
/// configured repo-wide.
async fn resolve_platform(tv: &ToolVersion, target: &PlatformTarget) -> Result<String> {
    match target.os_name() {
        "macos" => Ok("osx".to_string()),
        "windows" => Ok("windows10".to_string()),
        _ => {
            if let Some(pinned) = &Settings::get().swift.platform {
                return Ok(pinned.clone());
            }
            // Every published Linux build links against glibc. `mise lock`
            // already refuses musl targets; without the same check here an
            // Alpine host resolves to a real UBI URL and downloads ~1GB of a
            // toolchain that cannot run.
            if target.libc() == Some("musl") {
                bail!("swift does not publish musl builds");
            }
            let arch = api_arch(target);
            let host = host_distro(target);
            match fetch_linux_builds(&tv.version).await {
                Ok(builds) => match select_build(&builds, &host, arch) {
                    Some((build, fit)) => {
                        // Only warn about the host actually being installed to;
                        // `mise lock` resolves other platforms on assumptions
                        // that say nothing about the machine running the tool.
                        if target.is_current() {
                            warn_about_fit(&tv.version, &host, build, fit);
                        }
                        Ok(build.token.clone())
                    }
                    // The index lists the release and it has nothing for this
                    // architecture — swift.org has only ever built Linux
                    // x86_64 and aarch64. Say so instead of downloading a URL
                    // that cannot exist; the direct install path has no HEAD
                    // check to catch it, so otherwise this surfaces as a 404
                    // against swift.org's error page.
                    None => bail!("swift {} publishes no Linux build for {arch}", tv.version),
                },
                // Offline, or a release the index does not list. Fall back to
                // the family's last known build: it may be stale, but it is a
                // token swift.org has really published, so a tarball already in
                // the download cache is still found. A host label is not —
                // an unrecognized distro labels itself `ubi`, which is nothing.
                Err(err) => {
                    debug!("swift: could not read the release index: {err:#}");
                    Ok(host.family.offline_token().to_string())
                }
            }
        }
    }
}

/// The distro to pick a build for. Only the current host can be probed;
/// cross-platform lock resolution has no way to know what distro another
/// machine runs, so it assumes Ubuntu — the family with the widest published
/// coverage.
fn host_distro(target: &PlatformTarget) -> HostDistro {
    if !target.is_current() {
        return HostDistro {
            family: Family::Ubuntu,
            version: Some(DistroVersion::new(DEFAULT_UBUNTU_VERSION)),
            family_is_fallback: false,
            id: format!("ubuntu {DEFAULT_UBUNTU_VERSION}"),
        };
    }
    let os_release = linux_os_release();
    os_release
        .and_then(HostDistro::from_os_release)
        .unwrap_or_else(|| {
            let id = os_release.map(os_release_id).unwrap_or_default();
            HostDistro::unrecognized(id)
        })
}

/// swift.org's machine-readable index of what each release ships: which
/// distros, for which architectures, and the directory each build lives under.
/// Consulting it keeps mise from hard-coding a platform list that changes every
/// release — 6.2 dropped `ubuntu20.04`, 6.3 added `fedora41` and
/// `amazonlinux2023`, and 6.4 dropped `fedora39` and `amazonlinux2` outright.
const RELEASES_URL: &str = "https://www.swift.org/api/v1/install/releases.json";

#[derive(Debug, serde::Deserialize)]
struct ApiRelease {
    name: String,
    #[serde(default)]
    platforms: Vec<ApiPlatform>,
}

#[derive(Debug, serde::Deserialize)]
struct ApiPlatform {
    name: String,
    platform: String,
    /// Set when the artifact token differs from the display name, which so far
    /// happens only for `Red Hat Universal Base Image 9` → `ubi9`.
    #[serde(default)]
    dir: Option<String>,
    #[serde(default)]
    archs: Vec<String>,
}

/// A distro family swift.org builds for. The set of *versions* it publishes
/// turns over constantly; the families themselves have been stable, so this is
/// the only part of the platform list worth hard-coding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Family {
    Ubuntu,
    Debian,
    Fedora,
    AmazonLinux,
    Ubi,
}

impl Family {
    /// How swift.org spells each family in `releases.json`, and how the token
    /// that ends up in the artifact name begins.
    const DISPLAY_PREFIXES: &'static [(&'static str, Family)] = &[
        ("Ubuntu ", Family::Ubuntu),
        ("Debian ", Family::Debian),
        ("Fedora ", Family::Fedora),
        ("Amazon Linux ", Family::AmazonLinux),
        ("Red Hat Universal Base Image ", Family::Ubi),
    ];

    /// Map an os-release `ID` (or `ID_LIKE` entry) onto a family swift.org
    /// builds for. Derivatives are listed so an Ubuntu remix gets an Ubuntu
    /// build rather than a fabricated `linuxmint22`-style name.
    fn from_distro_id(id: &str) -> Option<Self> {
        match id {
            "ubuntu" => Some(Family::Ubuntu),
            "debian" | "raspbian" => Some(Family::Debian),
            "fedora" => Some(Family::Fedora),
            "amzn" => Some(Family::AmazonLinux),
            "rhel" | "ubi" | "centos" | "rocky" | "almalinux" | "ol" => Some(Family::Ubi),
            _ => None,
        }
    }

    /// The newest build this family had when this was written, used only when
    /// the release index cannot be read. It may be stale — that is the point
    /// of reading the index — but it is a token swift.org has really
    /// published, which a host label is not.
    fn offline_token(self) -> &'static str {
        match self {
            Family::Ubuntu => "ubuntu24.04",
            Family::Debian => "debian12",
            Family::Fedora => "fedora41",
            Family::AmazonLinux => "amazonlinux2023",
            Family::Ubi => "ubi9",
        }
    }

    /// The prefix of the artifact token, used to label a host whose exact
    /// build is only chosen later.
    fn token_prefix(self) -> &'static str {
        match self {
            Family::Ubuntu => "ubuntu",
            Family::Debian => "debian",
            Family::Fedora => "fedora",
            Family::AmazonLinux => "amazonlinux",
            Family::Ubi => "ubi",
        }
    }
}

/// A Linux build swift.org publishes for one release.
#[derive(Debug, Clone, PartialEq, Eq)]
struct LinuxBuild {
    /// The token that appears in the artifact name, e.g. `ubuntu24.04`, `ubi9`.
    token: String,
    family: Family,
    version: DistroVersion,
    archs: Vec<String>,
}

impl LinuxBuild {
    fn from_api(platform: &ApiPlatform) -> Option<Self> {
        if platform.platform != "Linux" {
            return None;
        }
        let (prefix, family) = Family::DISPLAY_PREFIXES
            .iter()
            .find(|(prefix, _)| platform.name.starts_with(prefix))?;
        let version = DistroVersion::new(platform.name.strip_prefix(prefix)?);
        let token = match &platform.dir {
            Some(dir) => dir.clone(),
            None => platform.name.to_lowercase().replace(' ', ""),
        };
        Some(Self {
            token,
            family: *family,
            version,
            archs: platform.archs.clone(),
        })
    }
}

/// A distro version, kept both as swift.org spells it and as components to
/// order by. These are distro versions (`24.04`, `2023`, `9`), not tool
/// versions — swift.org's own numbering, which is plain and ordered. The raw
/// spelling has to survive because `24.04` is not `24.4`; anything unparseable
/// sorts as 0 so a malformed entry can never outrank a real one.
#[derive(Debug, Clone, PartialEq, Eq)]
struct DistroVersion {
    raw: String,
    parts: Vec<u64>,
}

impl DistroVersion {
    fn new(raw: &str) -> Self {
        Self {
            raw: raw.to_string(),
            parts: raw
                .split('.')
                .map(|part| part.parse().unwrap_or(0))
                .collect(),
        }
    }

    /// Red Hat rebuilds are compatible at the major version; `rocky` 9.4 wants
    /// the `ubi9` build, not a nonexistent `ubi9.4`.
    fn major_only(&self) -> Self {
        Self::new(self.raw.split('.').next().unwrap_or(&self.raw))
    }
}

impl Ord for DistroVersion {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.parts.cmp(&other.parts)
    }
}

impl PartialOrd for DistroVersion {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// The host distro, reduced to what picking a build actually needs.
#[derive(Debug, Clone, PartialEq, Eq)]
struct HostDistro {
    family: Family,
    /// `None` when the family was reached through `ID_LIKE`: a derivative's
    /// `VERSION_ID` is its own, not its base's (Linux Mint 22 is Ubuntu 24.04),
    /// so it says nothing about which build to take.
    version: Option<DistroVersion>,
    /// True when `family` is a stand-in because this distro is not one
    /// swift.org builds for, which makes the choice worth reporting.
    family_is_fallback: bool,
    /// How the machine names itself, for messages. The family and version are
    /// normalized for matching; this is not, so a warning can say `omarchy
    /// 4.0.1rc2` rather than the family it was bucketed into.
    id: String,
}

impl HostDistro {
    /// A distro swift.org has never heard of: Arch, Gentoo, openSUSE, NixOS.
    fn unrecognized(id: String) -> Self {
        Self {
            family: FALLBACK_FAMILY,
            version: None,
            family_is_fallback: true,
            id,
        }
    }
}

/// How a machine names itself, e.g. `ubuntu 24.04` or `omarchy 4.0.1rc2`.
fn os_release_id(os_release: &crate::platform::LinuxOsRelease) -> String {
    if os_release.version_id.is_empty() {
        os_release.id.clone()
    } else {
        format!("{} {}", os_release.id, os_release.version_id)
    }
}

impl HostDistro {
    fn from_os_release(os_release: &crate::platform::LinuxOsRelease) -> Option<Self> {
        let mut ids = os_release.ids();
        let own_id = ids.next();
        if let Some(family) = own_id.and_then(Family::from_distro_id) {
            let version = (!os_release.version_id.is_empty()).then(|| {
                let version = DistroVersion::new(&os_release.version_id);
                if family == Family::Ubi {
                    version.major_only()
                } else {
                    version
                }
            });
            return Some(Self {
                family,
                version,
                family_is_fallback: false,
                id: os_release_id(os_release),
            });
        }
        // An unrecognized ID: fall back on the families it claims to be like.
        ids.find_map(Family::from_distro_id).map(|family| Self {
            family,
            version: None,
            family_is_fallback: false,
            id: os_release_id(os_release),
        })
    }

    /// A stable name for this host, recorded in the lockfile so one distro's
    /// checksum is never applied to another's tarball. It is a label for the
    /// machine, not necessarily the token of the build it resolves to — which
    /// build that is depends on the Swift release.
    fn label(&self) -> String {
        let prefix = self.family.token_prefix();
        match &self.version {
            Some(version) => format!("{prefix}{}", version.raw),
            None => prefix.to_string(),
        }
    }
}

/// Distros swift.org has no build for at all (Arch, Alpine, openSUSE, …) get
/// the Red Hat UBI build: it targets the oldest glibc of any current Swift
/// Linux artifact, so it is the one most likely to run somewhere unforeseen,
/// and it is what the Arch `swift-bin` package ships.
const FALLBACK_FAMILY: Family = Family::Ubi;

/// How well the chosen build matches the host, so a compromise can be
/// explained rather than silently downloaded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fit {
    /// swift.org ships a build for exactly this distro version.
    Exact,
    /// An older build than the host, which is fine: a binary linked against an
    /// older glibc runs on a newer one.
    OlderThanHost,
    /// The right family, but nothing said which version to take — a
    /// derivative's `VERSION_ID` is its own, not its base's. The family's
    /// oldest build is a guess, not a match.
    UnknownVersion,
    /// Everything published is newer than the host. Nothing here can be
    /// expected to run, but it is still the closest thing available.
    NewerThanHost,
    /// swift.org publishes nothing for this distro family at all.
    OtherFamily,
}

/// Pick the build to download for `host` out of what a release actually ships.
///
/// An exact version match wins: that is the combination swift.org tests, and
/// the artifacts differ in more than glibc (libcurl, libxml2 and ICU are all
/// linked per-distro). Failing that, take the newest published version *older*
/// than the host, since a binary built against an older glibc runs on a newer
/// one but not the reverse. With nothing older — or with no host version at
/// all — take the family's oldest build, the most conservative choice
/// available.
fn select_build<'a>(
    builds: &'a [LinuxBuild],
    host: &HostDistro,
    arch: &str,
) -> Option<(&'a LinuxBuild, Fit)> {
    let candidates = |family: Family| {
        let mut matching = builds
            .iter()
            .filter(|build| build.family == family && build.archs.iter().any(|a| a == arch))
            .collect::<Vec<_>>();
        matching.sort_by(|a, b| a.version.cmp(&b.version));
        matching
    };

    let pick = |family: Family, version: Option<&DistroVersion>| -> Option<(&'a LinuxBuild, Fit)> {
        let matching = candidates(family);
        let Some(version) = version else {
            // No usable host version: the oldest build is the safest guess, but
            // it is still a guess and must not be reported as a match.
            return matching
                .first()
                .copied()
                .map(|build| (build, Fit::UnknownVersion));
        };
        if let Some(exact) = matching.iter().find(|build| &build.version == version) {
            return Some((exact, Fit::Exact));
        }
        if let Some(older) = matching.iter().rev().find(|build| &build.version < version) {
            return Some((older, Fit::OlderThanHost));
        }
        matching
            .first()
            .copied()
            .map(|build| (build, Fit::NewerThanHost))
    };

    // The host version only means something within the host's own family:
    // Fedora 41 is not "newer than" UBI 10, the two just count differently. So
    // every fallback drops it and takes the conservative oldest.
    let own_family = (!host.family_is_fallback)
        .then(|| pick(host.family, host.version.as_ref()))
        .flatten();
    own_family
        .or_else(|| pick(FALLBACK_FAMILY, None).map(|(build, _)| (build, Fit::OtherFamily)))
        .or_else(|| {
            // Neither the host's family nor UBI publishes this architecture.
            // Take whatever does rather than build a URL that cannot exist.
            let mut any = builds
                .iter()
                .filter(|build| build.archs.iter().any(|a| a == arch))
                .collect::<Vec<_>>();
            any.sort_by(|a, b| a.version.cmp(&b.version));
            any.first().copied().map(|build| (build, Fit::OtherFamily))
        })
}

/// Say so when the chosen build is not one swift.org tests on this distro.
/// Without this the mismatch surfaces as `swift --version` exiting 127 after a
/// ~1GB download, which is a hard failure to read.
fn warn_about_fit(version: &str, host: &HostDistro, build: &LinuxBuild, fit: Fit) {
    match fit {
        Fit::Exact | Fit::OlderThanHost => {}
        Fit::UnknownVersion => warn!(
            "swift {version}: {} does not say which {} release it is based on; using {}",
            host.id,
            build.family.token_prefix(),
            build.token
        ),
        Fit::NewerThanHost => warn!(
            "swift {version} publishes no build for {} or anything older; using {} and it may not run here",
            host.id, build.token
        ),
        Fit::OtherFamily => warn!(
            "swift {version} publishes no build for {}; using {}",
            host.id, build.token
        ),
    }
}

/// The architecture as `releases.json` spells it.
fn api_arch(target: &PlatformTarget) -> &str {
    match target.arch_name() {
        "x64" => "x86_64",
        "arm64" => "aarch64",
        other => other,
    }
}

async fn fetch_linux_builds(version: &str) -> Result<Vec<LinuxBuild>> {
    let releases: Vec<ApiRelease> = HTTP_FETCH.json_cached(RELEASES_URL).await?;
    let release = releases
        .iter()
        .find(|release| release.name == version)
        .ok_or_else(|| eyre!("swift.org publishes no release named {version}"))?;
    Ok(release
        .platforms
        .iter()
        .filter_map(LinuxBuild::from_api)
        .collect())
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

/// The Ubuntu release assumed for a target that cannot be probed. Which
/// versions are actually published is read from the release index, so this only
/// has to name a plausible one.
const DEFAULT_UBUNTU_VERSION: &str = "24.04";

fn url(tv: &ToolVersion, target: &PlatformTarget, platform: &str) -> String {
    format!(
        "https://download.swift.org/swift-{version}-release/{platform_directory}/swift-{version}-RELEASE/swift-{version}-RELEASE-{platform}{architecture}.{extension}",
        version = tv.version,
        platform_directory = platform_directory(target, platform),
        extension = extension(target),
        architecture = match architecture(target) {
            Some(arch) => format!("-{arch}"),
            None => "".into(),
        }
    )
}

#[cfg(test)]
mod platform_selection_tests {
    use super::*;
    use crate::platform::LinuxOsRelease;

    /// A trimmed copy of the shape `releases.json` actually returns, covering
    /// the two things the parser has to get right: the `dir` override that
    /// `Red Hat Universal Base Image 9` needs, and a family (Fedora) whose
    /// published version moves between releases.
    const RELEASES_FIXTURE: &str = r#"[
      {
        "name": "6.3.3",
        "platforms": [
          {"name": "Ubuntu 22.04", "platform": "Linux", "archs": ["x86_64", "aarch64"]},
          {"name": "Ubuntu 24.04", "platform": "Linux", "archs": ["x86_64", "aarch64"]},
          {"name": "Debian 12", "platform": "Linux", "archs": ["x86_64", "aarch64"]},
          {"name": "Fedora 39", "platform": "Linux", "archs": ["x86_64", "aarch64"]},
          {"name": "Fedora 41", "platform": "Linux", "archs": ["x86_64", "aarch64"]},
          {"name": "Amazon Linux 2", "platform": "Linux", "archs": ["x86_64", "aarch64"]},
          {"name": "Red Hat Universal Base Image 9", "platform": "Linux", "dir": "ubi9",
           "archs": ["x86_64", "aarch64"]},
          {"name": "Windows 10", "platform": "Windows", "archs": ["x86_64", "arm64"]},
          {"name": "Static SDK", "platform": "static-sdk", "archs": ["x86_64", "arm64"]}
        ]
      },
      {
        "name": "6.4.0",
        "platforms": [
          {"name": "Ubuntu 24.04", "platform": "Linux", "archs": ["x86_64", "aarch64"]},
          {"name": "Fedora 41", "platform": "Linux", "archs": ["x86_64"]},
          {"name": "Amazon Linux 2023", "platform": "Linux", "archs": ["x86_64", "aarch64"]},
          {"name": "Red Hat Universal Base Image 9", "platform": "Linux", "dir": "ubi9",
           "archs": ["x86_64", "aarch64"]},
          {"name": "Red Hat Universal Base Image 10", "platform": "Linux", "dir": "ubi10",
           "archs": ["x86_64", "aarch64"]}
        ]
      }
    ]"#;

    fn builds(version: &str) -> Vec<LinuxBuild> {
        let releases: Vec<ApiRelease> =
            serde_json::from_str(RELEASES_FIXTURE).expect("valid fixture");
        releases
            .iter()
            .find(|release| release.name == version)
            .expect("fixture has the release")
            .platforms
            .iter()
            .filter_map(LinuxBuild::from_api)
            .collect()
    }

    fn host(os_release: &str) -> HostDistro {
        let release = LinuxOsRelease::parse(os_release).expect("valid os-release");
        HostDistro::from_os_release(&release)
            .unwrap_or_else(|| HostDistro::unrecognized(os_release_id(&release)))
    }

    fn chosen(version: &str, os_release: &str, arch: &str) -> String {
        select(version, os_release, arch).0
    }

    fn select(version: &str, os_release: &str, arch: &str) -> (String, Fit) {
        let builds = builds(version);
        let (build, fit) = select_build(&builds, &host(os_release), arch).expect("a build");
        (build.token.clone(), fit)
    }

    /// Only Linux entries are candidates, and `dir` wins over the display name
    /// where swift.org sets it.
    #[test]
    fn parses_linux_builds_and_honors_the_dir_override() {
        let tokens: Vec<_> = builds("6.3.3").iter().map(|b| b.token.clone()).collect();
        assert_eq!(
            tokens,
            vec![
                "ubuntu22.04",
                "ubuntu24.04",
                "debian12",
                "fedora39",
                "fedora41",
                "amazonlinux2",
                "ubi9",
            ]
        );
    }

    /// The version a distro calls itself is kept verbatim: `24.04` is not
    /// `24.4`.
    #[test]
    fn distro_versions_keep_their_spelling_and_still_order() {
        assert_eq!(DistroVersion::new("24.04").raw, "24.04");
        assert!(DistroVersion::new("24.04") > DistroVersion::new("22.04"));
        assert!(DistroVersion::new("2023") > DistroVersion::new("2"));
        assert!(DistroVersion::new("9") < DistroVersion::new("10"));
    }

    /// The exact distro build is preferred when the release ships it.
    #[test]
    fn exact_distro_match_wins() {
        assert_eq!(
            chosen("6.3.3", "ID=ubuntu\nVERSION_ID=\"24.04\"\n", "x86_64"),
            "ubuntu24.04"
        );
        assert_eq!(
            chosen("6.3.3", "ID=fedora\nVERSION_ID=39\n", "x86_64"),
            "fedora39"
        );
    }

    /// Without an exact match, take the newest build *older* than the host: a
    /// binary linked against an older glibc runs on a newer one, not the
    /// reverse.
    #[test]
    fn unlisted_version_takes_the_newest_older_build() {
        assert_eq!(
            chosen("6.3.3", "ID=fedora\nVERSION_ID=40\n", "x86_64"),
            "fedora39"
        );
        assert_eq!(
            chosen("6.3.3", "ID=ubuntu\nVERSION_ID=\"25.10\"\n", "x86_64"),
            "ubuntu24.04"
        );
    }

    /// A host older than anything still published has no safe choice; the
    /// family's oldest build is the least-bad one.
    #[test]
    fn host_older_than_every_build_takes_the_oldest() {
        assert_eq!(
            chosen("6.3.3", "ID=ubuntu\nVERSION_ID=\"20.04\"\n", "x86_64"),
            "ubuntu22.04"
        );
    }

    /// The regression from #13289: an unknown `ID` used to be pasted straight
    /// into the URL as `omarchy4.0.1rc2`. Arch has no swift.org build and no
    /// usable `ID_LIKE` family, so it falls back to UBI.
    #[test]
    fn unknown_distro_falls_back_instead_of_fabricating_a_name() {
        assert_eq!(
            chosen(
                "6.3.3",
                "ID=omarchy\nID_LIKE=arch\nVERSION_ID=4.0.1rc2\n",
                "aarch64"
            ),
            "ubi9"
        );
        assert_eq!(chosen("6.3.3", "ID=arch\n", "x86_64"), "ubi9");
    }

    /// A derivative is served by its base family, but its own `VERSION_ID`
    /// says nothing about which base build to take (Linux Mint 22 is Ubuntu
    /// 24.04), so the conservative oldest is used.
    #[test]
    fn derivatives_use_their_base_family() {
        assert_eq!(
            chosen(
                "6.3.3",
                "ID=linuxmint\nID_LIKE=\"ubuntu debian\"\nVERSION_ID=22\n",
                "x86_64"
            ),
            "ubuntu22.04"
        );
    }

    /// Red Hat rebuilds are compatible at the major version, so 9.4 resolves
    /// to `ubi9` rather than a nonexistent `ubi9.4`.
    #[test]
    fn rhel_rebuilds_match_on_the_major_version() {
        assert_eq!(
            chosen("6.3.3", "ID=rocky\nVERSION_ID=\"9.4\"\n", "x86_64"),
            "ubi9"
        );
        assert_eq!(host("ID=rocky\nVERSION_ID=\"9.4\"\n").label(), "ubi9");
    }

    /// A family that publishes nothing for this architecture is skipped rather
    /// than producing a URL that cannot exist. 6.4.0 ships Fedora for x86_64
    /// only.
    #[test]
    fn a_family_without_the_architecture_falls_through() {
        assert_eq!(
            chosen("6.4.0", "ID=fedora\nVERSION_ID=41\n", "x86_64"),
            "fedora41"
        );
        assert_eq!(
            chosen("6.4.0", "ID=fedora\nVERSION_ID=41\n", "aarch64"),
            "ubi9"
        );
    }

    /// The cases that made mise 404 on Swift 6.4.0: the hard-coded map pinned
    /// Fedora to 39 and Amazon Linux 2 to a build 6.4.0 no longer ships.
    #[test]
    fn selection_follows_the_release_rather_than_a_fixed_map() {
        assert_eq!(
            chosen("6.3.3", "ID=amzn\nVERSION_ID=2\n", "x86_64"),
            "amazonlinux2"
        );
        // 6.4.0 dropped both fedora39 and amazonlinux2 entirely.
        assert_eq!(
            chosen("6.4.0", "ID=fedora\nVERSION_ID=39\n", "x86_64"),
            "fedora41"
        );
    }

    /// A host older than anything the release ships gets the closest build,
    /// flagged — an Amazon Linux 2023 binary will not run on Amazon Linux 2,
    /// and saying so beats a bare exit 127 after a 1GB download.
    #[test]
    fn a_host_older_than_every_build_is_flagged() {
        assert_eq!(
            select("6.4.0", "ID=amzn\nVERSION_ID=2\n", "x86_64"),
            ("amazonlinux2023".to_string(), Fit::NewerThanHost)
        );
        assert_eq!(
            select("6.3.3", "ID=ubuntu\nVERSION_ID=\"20.04\"\n", "x86_64"),
            ("ubuntu22.04".to_string(), Fit::NewerThanHost)
        );
    }

    /// Landing on another family is a bigger compromise than landing on
    /// another version of the right one, and is reported separately.
    #[test]
    fn fit_distinguishes_the_kind_of_compromise() {
        assert_eq!(
            select("6.3.3", "ID=ubuntu\nVERSION_ID=\"24.04\"\n", "x86_64").1,
            Fit::Exact
        );
        assert_eq!(
            select("6.3.3", "ID=fedora\nVERSION_ID=40\n", "x86_64").1,
            Fit::OlderThanHost
        );
        assert_eq!(select("6.3.3", "ID=arch\n", "x86_64").1, Fit::OtherFamily);
        assert_eq!(
            select("6.4.0", "ID=fedora\nVERSION_ID=41\n", "aarch64").1,
            Fit::OtherFamily
        );
    }

    /// swift.org has only ever built Linux x86_64 and aarch64, so nothing is
    /// selected for any other architecture. `resolve_platform` turns that into
    /// an error naming the architecture, rather than a 404 against swift.org's
    /// error page.
    #[test]
    fn an_unsupported_architecture_selects_nothing() {
        for arch in ["riscv64", "loongarch64", "x86"] {
            assert!(
                select_build(
                    &builds("6.3.3"),
                    &host("ID=ubuntu\nVERSION_ID=\"24.04\"\n"),
                    arch
                )
                .is_none(),
                "{arch} should select nothing"
            );
        }
    }

    /// A derivative is served by its base family, but nothing said which base
    /// version: Linux Mint 22 is built on Ubuntu 24.04, and the oldest Ubuntu
    /// build is a guess. Reporting that as an exact match would suppress the
    /// warning the guess deserves.
    #[test]
    fn a_guessed_base_version_is_not_reported_as_exact() {
        assert_eq!(
            select(
                "6.3.3",
                "ID=linuxmint\nID_LIKE=\"ubuntu debian\"\nVERSION_ID=22\n",
                "x86_64"
            ),
            ("ubuntu22.04".to_string(), Fit::UnknownVersion)
        );
    }

    /// The offline fallback has to name a build swift.org really published.
    /// A host label does not: an unrecognized distro labels itself `ubi`,
    /// which is not an artifact token at all.
    #[test]
    fn offline_tokens_are_real_published_builds() {
        let published: Vec<String> = ["6.3.3", "6.4.0"]
            .iter()
            .flat_map(|version| builds(version))
            .map(|build| build.token)
            .collect();
        for family in [
            Family::Ubuntu,
            Family::Debian,
            Family::Fedora,
            Family::AmazonLinux,
            Family::Ubi,
        ] {
            let token = family.offline_token();
            assert!(
                published.contains(&token.to_string()),
                "{token} is not a published build"
            );
            assert_ne!(
                token,
                family.token_prefix(),
                "{token} is a family name, not a build"
            );
        }
    }

    /// The lockfile label names the machine, not the artifact, so it stays put
    /// as swift.org's published set moves underneath it.
    #[test]
    fn host_labels_describe_the_machine() {
        assert_eq!(
            host("ID=ubuntu\nVERSION_ID=\"24.04\"\n").label(),
            "ubuntu24.04"
        );
        assert_eq!(host("ID=fedora\nVERSION_ID=40\n").label(), "fedora40");
        assert_eq!(host("ID=omarchy\nID_LIKE=arch\n").label(), "ubi");
    }
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
        let lock = crate::test::lock_ignoring_poison(&TEST_SETTINGS_LOCK);
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
            url(&tv, &target("linux-arm64"), "ubuntu24.04"),
            "https://download.swift.org/swift-6.3.1-release/ubuntu2404-aarch64/swift-6.3.1-RELEASE/swift-6.3.1-RELEASE-ubuntu24.04-aarch64.tar.gz"
        );
    }

    /// swift.org files every non-x86_64 build under its own
    /// `<platform>-<arch>` directory. Treating that as an Ubuntu-only
    /// convention 404s every other distro's arm64 build (#13291).
    #[test]
    fn arm64_urls_use_the_arch_directory_on_every_distro() {
        for (pinned, directory, filename) in [
            ("ubuntu24.04", "ubuntu2404-aarch64", "ubuntu24.04-aarch64"),
            ("ubi9", "ubi9-aarch64", "ubi9-aarch64"),
            ("fedora39", "fedora39-aarch64", "fedora39-aarch64"),
            (
                "amazonlinux2",
                "amazonlinux2-aarch64",
                "amazonlinux2-aarch64",
            ),
        ] {
            let _guard = pin_platform(Some(pinned));
            let backend = SwiftPlugin::new();
            let tv = tool_version(&backend, "6.3.3");

            assert_eq!(
                url(&tv, &target("linux-arm64"), pinned),
                format!(
                    "https://download.swift.org/swift-6.3.3-release/{directory}/swift-6.3.3-RELEASE/swift-6.3.3-RELEASE-{filename}.tar.gz"
                )
            );
        }
    }

    /// x86_64 builds sit directly under the platform directory — no arch
    /// suffix on either the directory or the file.
    #[test]
    fn x64_urls_have_no_arch_suffix() {
        let _guard = pin_platform(Some("ubi9"));
        let backend = SwiftPlugin::new();
        let tv = tool_version(&backend, "6.3.3");

        assert_eq!(
            url(&tv, &target("linux-x64"), "ubi9"),
            "https://download.swift.org/swift-6.3.3-release/ubi9/swift-6.3.3-RELEASE/swift-6.3.3-RELEASE-ubi9.tar.gz"
        );
    }

    /// Windows follows the same layout: the arm64 build is under
    /// `windows10-arm64`, while x64 stays in `windows10`.
    #[test]
    fn windows_arm64_uses_the_arch_directory() {
        let _guard = pin_platform(None);
        let backend = SwiftPlugin::new();
        let tv = tool_version(&backend, "6.3.3");

        assert_eq!(
            url(&tv, &target("windows-arm64"), "windows10"),
            "https://download.swift.org/swift-6.3.3-release/windows10-arm64/swift-6.3.3-RELEASE/swift-6.3.3-RELEASE-windows10-arm64.exe"
        );
    }

    /// `swift.platform` names a Linux distro build. A repo-wide pin must not
    /// leak into the macOS and Windows URLs.
    #[tokio::test]
    async fn pinned_distro_is_ignored_off_linux() {
        let _guard = pin_platform(Some("ubi9"));
        let backend = SwiftPlugin::new();
        let tv = tool_version(&backend, "6.3.1");

        assert_eq!(
            resolve_platform(&tv, &target("macos-arm64")).await.unwrap(),
            "osx"
        );
        assert_eq!(
            resolve_platform(&tv, &target("windows-x64")).await.unwrap(),
            "windows10"
        );
    }

    #[test]
    fn pinned_distro_does_not_apply_off_linux() {
        let _guard = pin_platform(Some("ubi9"));
        let backend = SwiftPlugin::new();
        let tv = tool_version(&backend, "6.3.1");

        assert_eq!(
            url(&tv, &target("macos-arm64"), "osx"),
            "https://download.swift.org/swift-6.3.1-release/xcode/swift-6.3.1-RELEASE/swift-6.3.1-RELEASE-osx.pkg"
        );
        assert_eq!(
            url(&tv, &target("windows-x64"), "windows10"),
            "https://download.swift.org/swift-6.3.1-release/windows10/swift-6.3.1-RELEASE/swift-6.3.1-RELEASE-windows10.exe"
        );
    }

    /// Every published Linux build links against glibc. Resolution has to
    /// refuse musl before it picks anything, or an Alpine host resolves to a
    /// real UBI URL and downloads ~1GB that cannot run.
    #[tokio::test]
    async fn musl_targets_are_refused_before_any_build_is_chosen() {
        let _guard = pin_platform(None);
        let backend = SwiftPlugin::new();
        let tv = tool_version(&backend, "6.3.1");

        assert!(
            resolve_platform(&tv, &target("linux-x64-musl"))
                .await
                .is_err()
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
        assert_eq!(
            url(&tv, &foreign, "ubuntu24.04"),
            "https://download.swift.org/swift-6.3.1-release/ubuntu2404-riscv64/swift-6.3.1-RELEASE/swift-6.3.1-RELEASE-ubuntu24.04-riscv64.tar.gz"
        );
    }

    /// The directory suffix follows the filename suffix for every architecture
    /// that has one, not just aarch64. swift.org has only ever published
    /// x86_64 and aarch64 Linux builds, so these URLs 404 either way — the
    /// assertions exist so the two halves cannot drift apart unnoticed.
    #[test]
    fn every_suffixed_architecture_gets_a_matching_directory() {
        let _guard = pin_platform(Some("ubuntu24.04"));
        let backend = SwiftPlugin::new();
        let tv = tool_version(&backend, "6.3.1");

        for (platform, arch) in [
            ("linux-x86", "x86"),
            ("linux-riscv64", "riscv64"),
            ("linux-loongarch64", "loongarch64"),
        ] {
            assert_eq!(
                url(&tv, &target(platform), "ubuntu24.04"),
                format!(
                    "https://download.swift.org/swift-6.3.1-release/ubuntu2404-{arch}/swift-6.3.1-RELEASE/swift-6.3.1-RELEASE-ubuntu24.04-{arch}.tar.gz"
                )
            );
        }
    }
}
