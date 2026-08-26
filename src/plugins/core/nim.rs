use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::Result;
use crate::backend::platform_target::PlatformTarget;
use crate::backend::static_helpers::fetch_checksum_from_file;
use crate::backend::{Backend, VersionInfo};
use crate::cli::args::BackendArg;
use crate::cmd::CmdLineRunner;
use crate::config::Config;
use crate::config::Settings;
use crate::config::settings::CompilePurpose;
use crate::file::{ExtractOptions, ExtractionFormat};
use crate::http::HTTP;
use crate::install_context::InstallContext;
use crate::lockfile::PlatformInfo;
use crate::toolset::{ToolRequest, ToolVersion};
use crate::ui::progress_report::SingleReport;
use crate::{env, file, github, plugins};
use async_trait::async_trait;
use itertools::Itertools;
use std::collections::BTreeMap;
use tempfile::tempdir_in;
use versions::Versioning;
use xx::regex;

const NIM_REPO: &str = "https://github.com/nim-lang/Nim";
const NIM_DOWNLOAD_BASE: &str = "https://nim-lang.org/download";

/// The Nim core plugin installs the Nim toolchain as a mise-managed tool.
///
/// Nim publishes official prebuilt binaries for Windows (x86/x86_64 zips) and
/// Linux (x86/x86_64 tar.xz) on nim-lang.org. Every other platform (macOS,
/// Linux arm64, Windows arm64, ...) has no prebuilt artifact and builds from
/// source. Source builds are also available on any platform when
/// `nim.compile = true` forces them: `build_all.sh` (bash) on Unix and
/// `build_all.bat` on Windows (Nim's csources bootstrap requires mingw `gcc`
/// on Windows), plus `git` on PATH.
#[derive(Debug)]
pub(super) struct NimPlugin {
    ba: Arc<BackendArg>,
}

impl NimPlugin {
    pub(super) fn new() -> Self {
        Self {
            ba: Arc::new(plugins::core::new_backend_arg("nim")),
        }
    }

    fn nim_bin(&self, tv: &ToolVersion) -> PathBuf {
        tv.install_path().join("bin").join(nim_bin_name())
    }

    fn nimble_bin(&self, tv: &ToolVersion) -> PathBuf {
        tv.install_path().join("bin").join(nimble_bin_name())
    }

    async fn download(&self, tv: &mut ToolVersion, pr: &dyn SingleReport) -> Result<PathBuf> {
        let target = PlatformTarget::from_current();
        let tarball_url = Arc::new(
            prebuilt_url(&tv.version, target.os_name(), target.arch_name())
                .ok_or_else(|| eyre::eyre!("no prebuilt nim binary for this platform"))?,
        );
        let filename = tarball_url.split('/').next_back().unwrap();
        let tarball_path = tv.download_path().join(filename);

        let tarball_url_ = tarball_url.clone();
        let checksum_handle = tokio::spawn(async move {
            let checksum_url = format!("{tarball_url_}.sha256");
            HTTP.get_text(checksum_url).await
        });
        pr.set_message(format!("download {filename}"));
        HTTP.download_file(&*tarball_url, &tarball_path, Some(pr))
            .await?;

        let platform_key = self.get_platform_key();
        let platform_info = tv.lock_platforms.entry(platform_key).or_default();
        platform_info.url = Some(tarball_url.to_string());
        if platform_info.checksum.is_none() {
            let raw = checksum_handle.await.unwrap()?;
            let checksum = raw.split_whitespace().next().unwrap_or(&raw);
            platform_info.checksum = Some(format!("sha256:{checksum}"));
        }
        Ok(tarball_path)
    }

    fn install(&self, tv: &ToolVersion, pr: &dyn SingleReport, tarball_path: &Path) -> Result<()> {
        let tarball = tarball_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy();
        pr.set_message(format!("extract {tarball}"));
        let tmp_extract_path = tempdir_in(tv.install_path().parent().unwrap())?;
        if cfg!(windows) {
            file::unzip(tarball_path, tmp_extract_path.path(), &Default::default())?;
        } else {
            file::untar(
                tarball_path,
                tmp_extract_path.path(),
                ExtractionFormat::TarXz,
                &ExtractOptions {
                    pr: Some(pr),
                    ..Default::default()
                },
            )?;
        }
        file::remove_all(tv.install_path())?;
        // The archive top-level directory is `nim-<version>`; rename it into the
        // install path so `bin/nim` finds `lib/` relative to itself.
        file::rename(
            tmp_extract_path.path().join(format!("nim-{}", tv.version)),
            tv.install_path(),
        )?;
        file::make_executable(self.nim_bin(tv))?;
        file::make_executable(self.nimble_bin(tv))?;
        Ok(())
    }

    fn verify(&self, tv: &ToolVersion, pr: &dyn SingleReport) -> Result<()> {
        pr.set_message("nim --version".into());
        CmdLineRunner::new(self.nim_bin(tv))
            .current_dir(tv.install_path())
            .with_pr(pr)
            .arg("--version")
            .env_values(tv.install_env())
            .execute()?;
        pr.set_message("nimble --version".into());
        CmdLineRunner::new(self.nimble_bin(tv))
            .current_dir(tv.install_path())
            .with_pr(pr)
            .arg("--version")
            .env_values(tv.install_env())
            .execute()?;
        Ok(())
    }

    /// Build Nim from source when no prebuilt binary exists for the platform,
    /// or when `nim.compile = true` forces a source build.
    ///
    /// Runs `build_all.sh` (bash) on Unix and `build_all.bat` on Windows.
    /// Requires `git` on PATH plus a C compiler: `gcc`/`clang` on Unix, mingw
    /// `gcc` on Windows (Nim's csources bootstrap does not support MSVC).
    /// Nim's GitHub source tarball excludes the `nimble` submodule, so we
    /// clone recursively.
    async fn install_from_source(&self, tv: &ToolVersion, ctx: &InstallContext) -> Result<()> {
        let pr = ctx.pr.as_ref();
        let tmp_extract_path = tempdir_in(tv.install_path().parent().unwrap())?;
        let src_dir = tmp_extract_path.path().join(format!("Nim-{}", tv.version));
        let branch = format!("v{}", tv.version);
        pr.set_message("clone nim source (recursive)".into());
        CmdLineRunner::new("git")
            .arg("clone")
            .arg("--depth")
            .arg("1")
            .arg("--branch")
            .arg(&branch)
            .arg("--recursive")
            .arg(NIM_REPO)
            .arg(src_dir.to_str().unwrap())
            .with_pr(pr)
            .execute()?;
        // build_all.sh needs bash; on Windows the repo ships build_all.bat,
        // which std runs through cmd.exe. Nim's csources bootstrap uses gcc
        // (mingw) on Windows, so MSVC alone is not enough.
        pr.set_message("build nim (build_all)".into());
        let build_cmd = if cfg!(windows) {
            CmdLineRunner::new(src_dir.join("build_all.bat"))
        } else {
            CmdLineRunner::new("bash").arg(src_dir.join("build_all.sh"))
        };
        build_cmd
            .current_dir(&src_dir)
            .with_pr(pr)
            .env_values(tv.install_env())
            .execute()?;
        file::remove_all(tv.install_path())?;
        file::rename(&src_dir, tv.install_path())?;
        file::make_executable(self.nim_bin(tv))?;
        file::make_executable(self.nimble_bin(tv))?;
        self.verify(tv, pr)?;
        Ok(())
    }
}

/// Build the prebuilt-binary URL for a platform, or `None` when Nim does not
/// ship a prebuilt artifact for it (macOS, and non-x86 Linux).
///
/// Nim publishes official binaries for Windows and Linux x86/x86_64. The two
/// use different naming conventions: `nim-<v>-linux_<arch>.tar.xz` vs
/// `nim-<v>_<arch>.zip` (no OS segment on Windows).
fn prebuilt_url(version: &str, os: &str, arch: &str) -> Option<String> {
    let arch = match arch {
        "x64" => "x64",
        "x32" | "i686" | "i386" => "x32",
        _ => return None,
    };
    match os {
        "linux" => Some(format!(
            "{NIM_DOWNLOAD_BASE}/nim-{version}-linux_{arch}.tar.xz"
        )),
        "windows" => Some(format!("{NIM_DOWNLOAD_BASE}/nim-{version}_{arch}.zip")),
        _ => None,
    }
}

fn nim_bin_name() -> &'static str {
    if cfg!(windows) { "nim.exe" } else { "nim" }
}

fn nimble_bin_name() -> &'static str {
    if cfg!(windows) {
        "nimble.exe"
    } else {
        "nimble"
    }
}

/// Lockfile entry for a source build: lock the repo and record the method so
/// locked installs reproduce it.
fn source_lock_info() -> PlatformInfo {
    PlatformInfo {
        url: Some(NIM_REPO.to_string()),
        install: Some("source".to_string()),
        ..Default::default()
    }
}

/// Decide whether to build from source, mirroring the node/python/ruby logic.
///
/// In locked mode the lockfile records the install method; otherwise the
/// `nim.compile` setting forces source when `Some(true)`.
fn should_compile_from_source(
    locked: bool,
    lock_platforms: &BTreeMap<String, PlatformInfo>,
    platform_key: &str,
    nim_compile: Option<bool>,
) -> bool {
    if locked {
        lock_platforms
            .get(platform_key)
            .is_some_and(|pi| pi.install.as_deref() == Some("source"))
    } else {
        nim_compile == Some(true)
    }
}

/// Reject non-semver and pre-release tags (e.g. `2.2.0-rc1`, `devel`).
fn is_valid_version(v: &str) -> bool {
    regex!(r"^[0-9]+\.[0-9]+\.[0-9]+$").is_match(v)
}

#[async_trait]
impl Backend for NimPlugin {
    fn ba(&self) -> &Arc<BackendArg> {
        &self.ba
    }

    async fn security_info(&self) -> Vec<crate::backend::SecurityFeature> {
        use crate::backend::SecurityFeature;

        vec![SecurityFeature::Checksum {
            algorithm: Some("sha256".to_string()),
        }]
    }

    async fn _list_remote_versions(&self, _config: &Arc<Config>) -> Result<Vec<VersionInfo>> {
        // Nim tags are `vX.Y.Z`. When MISE_LIST_ALL_VERSIONS is set, fetch tags
        // with dates (slower); otherwise use the fast `git ls-remote` path.
        let versions: Vec<VersionInfo> = if *env::MISE_LIST_ALL_VERSIONS {
            github::list_tags_with_dates("nim-lang/Nim")
                .await?
                .into_iter()
                .filter_map(|t| t.name.strip_prefix('v').map(|v| (v.to_string(), t.date)))
                .filter(|(v, _)| is_valid_version(v))
                .unique_by(|(v, _)| v.clone())
                .sorted_by_cached_key(|(v, _)| (Versioning::new(v), v.to_string()))
                .map(|(version, created_at)| VersionInfo {
                    version,
                    created_at,
                    ..Default::default()
                })
                .collect()
        } else {
            let repo = NIM_REPO.to_string();
            plugins::core::run_fetch_task_with_timeout_async(async move || {
                let output = crate::cmd::cmd_read_async_inherited_env(
                    "git",
                    &["ls-remote", "--tags", "--refs", &repo, "v*"],
                    std::iter::empty::<(&str, &std::ffi::OsStr)>(),
                )
                .await?;
                let versions: Vec<VersionInfo> = output
                    .lines()
                    .filter_map(|line| line.split("/v").last())
                    .filter(|s| !s.is_empty())
                    .filter(|s| is_valid_version(s))
                    .map(|s| s.to_string())
                    .unique()
                    .sorted_by_cached_key(|v| (Versioning::new(v), v.to_string()))
                    .map(|version| VersionInfo {
                        version,
                        ..Default::default()
                    })
                    .collect();
                Ok(versions)
            })
            .await?
        };
        Ok(versions)
    }

    async fn get_tarball_url(
        &self,
        tv: &ToolVersion,
        target: &PlatformTarget,
    ) -> Result<Option<String>> {
        Ok(prebuilt_url(
            &tv.version,
            target.os_name(),
            target.arch_name(),
        ))
    }

    async fn resolve_lock_info(
        &self,
        tv: &ToolVersion,
        target: &PlatformTarget,
    ) -> Result<PlatformInfo> {
        // Mirror node: when the compile setting forces a source build, record
        // the source method in the lockfile even on platforms that also have a
        // prebuilt artifact, so locked installs reproduce the build method.
        if Settings::get().nim_compile(CompilePurpose::Inspect) == Some(true) {
            return Ok(source_lock_info());
        }
        match prebuilt_url(&tv.version, target.os_name(), target.arch_name()) {
            Some(url) => {
                let checksum = fetch_checksum_from_file(&format!("{url}.sha256"), "sha256").await;
                Ok(PlatformInfo {
                    url: Some(url),
                    checksum,
                    ..Default::default()
                })
            }
            None => Ok(source_lock_info()),
        }
    }

    async fn install_version_(
        &self,
        ctx: &InstallContext,
        mut tv: ToolVersion,
    ) -> Result<ToolVersion> {
        let target = PlatformTarget::from_current();
        let platform_key = self.get_platform_key();
        // Honor the lockfile whenever the version was resolved from it (not
        // only in strict --locked mode): the lock's install = "source" marker
        // must reproduce the recorded build method.
        let locked = ctx.locked || tv.resolved_from_lockfile();
        let nim_compile = Settings::get().nim_compile(if locked {
            CompilePurpose::Inspect
        } else {
            CompilePurpose::Install
        });
        let force_source =
            should_compile_from_source(locked, &tv.lock_platforms, &platform_key, nim_compile);
        let has_prebuilt =
            prebuilt_url(&tv.version, target.os_name(), target.arch_name()).is_some();

        if has_prebuilt && !force_source {
            let tarball_path = self.download(&mut tv, ctx.pr.as_ref()).await?;
            ctx.pr.next_operation();
            self.verify_checksum(ctx, &mut tv, &tarball_path)?;
            ctx.pr.next_operation();
            self.install(&tv, ctx.pr.as_ref(), &tarball_path)?;
            self.verify(&tv, ctx.pr.as_ref())?;
        } else if force_source || nim_compile != Some(false) {
            self.install_from_source(&tv, ctx).await?;
        } else {
            eyre::bail!(
                "no prebuilt nim binary for this platform; set nim.compile = true to build from source"
            );
        }
        Ok(tv)
    }

    async fn list_bin_paths(
        &self,
        _config: &Arc<Config>,
        tv: &ToolVersion,
    ) -> Result<Vec<PathBuf>> {
        if let ToolRequest::System { .. } = tv.request {
            return Ok(vec![]);
        }
        Ok(vec![tv.install_path().join("bin")])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn test_prebuilt_url() {
        // Linux prebuilt artifacts are published for x86_64 and i686.
        assert_eq!(
            prebuilt_url("2.2.0", "linux", "x64"),
            Some("https://nim-lang.org/download/nim-2.2.0-linux_x64.tar.xz".into())
        );
        assert_eq!(
            prebuilt_url("2.2.0", "linux", "i686"),
            Some("https://nim-lang.org/download/nim-2.2.0-linux_x32.tar.xz".into())
        );
        // Windows prebuilts are zips without an OS segment in the filename.
        assert_eq!(
            prebuilt_url("2.2.0", "windows", "x64"),
            Some("https://nim-lang.org/download/nim-2.2.0_x64.zip".into())
        );
        assert_eq!(
            prebuilt_url("2.2.0", "windows", "x32"),
            Some("https://nim-lang.org/download/nim-2.2.0_x32.zip".into())
        );
        // No prebuilt binaries for macOS or non-x86 Linux/Windows.
        assert_eq!(prebuilt_url("2.2.0", "macos", "x64"), None);
        assert_eq!(prebuilt_url("2.2.0", "linux", "arm64"), None);
        assert_eq!(prebuilt_url("2.2.0", "windows", "arm64"), None);
    }

    #[test]
    fn test_is_valid_version() {
        assert!(is_valid_version("2.2.0"));
        assert!(is_valid_version("1.6.0"));
        // Tags and pre-releases are excluded.
        assert!(!is_valid_version("v2.2.0"));
        assert!(!is_valid_version("2.2.0-rc1"));
        assert!(!is_valid_version("devel"));
        assert!(!is_valid_version("2.2"));
    }

    #[test]
    fn test_locked_install_uses_source_marker() {
        // A lockfile recording install = "source" must be honored even when
        // the current nim.compile setting says false.
        let lock_platforms = BTreeMap::from([(
            "linux-x64".to_string(),
            PlatformInfo {
                install: Some("source".to_string()),
                ..Default::default()
            },
        )]);
        assert!(should_compile_from_source(
            true,
            &lock_platforms,
            "linux-x64",
            Some(false)
        ));
    }

    #[test]
    fn test_locked_install_ignores_compile_setting_for_binary_lock() {
        // A prebuilt lock entry wins over the current compile setting.
        let lock_platforms = BTreeMap::from([(
            "linux-x64".to_string(),
            PlatformInfo {
                url: Some("https://nim-lang.org/download/nim-2.2.0-linux_x64.tar.xz".into()),
                ..Default::default()
            },
        )]);
        assert!(!should_compile_from_source(
            true,
            &lock_platforms,
            "linux-x64",
            Some(true)
        ));
    }

    #[test]
    fn test_unlocked_install_uses_compile_setting() {
        assert!(should_compile_from_source(
            false,
            &BTreeMap::new(),
            "linux-x64",
            Some(true)
        ));
        assert!(!should_compile_from_source(
            false,
            &BTreeMap::new(),
            "linux-x64",
            None
        ));
        assert!(!should_compile_from_source(
            false,
            &BTreeMap::new(),
            "linux-x64",
            Some(false)
        ));
    }
}
