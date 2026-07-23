use std::path::{Path, PathBuf};
use std::{collections::BTreeMap, sync::Arc};

use crate::Result;
use crate::backend::platform_target::PlatformTarget;
use crate::backend::static_helpers::fetch_checksum_from_file;
use crate::backend::{Backend, VersionInfo, normalize_idiomatic_contents};
use crate::cli::args::BackendArg;
use crate::cmd::CmdLineRunner;
use crate::config::{Config, Settings};
use crate::file::{ExtractOptions, ExtractionFormat};
use crate::http::HTTP;
use crate::install_context::InstallContext;
use crate::lockfile::PlatformInfo;
use crate::toolset::{ToolRequest, ToolVersion, Toolset};
use crate::ui::progress_report::SingleReport;
use crate::{env, file, github, plugins};
use async_trait::async_trait;
use itertools::Itertools;
use tempfile::tempdir_in;
use versions::Versioning;
use xx::regex;

#[derive(Debug)]
pub struct GoPlugin {
    ba: Arc<BackendArg>,
}

impl GoPlugin {
    pub fn new() -> Self {
        Self {
            ba: Arc::new(plugins::core::new_backend_arg("go")),
        }
    }

    /// Check if a Go version string is valid (not "1" and not beta/rc)
    /// - "1" corresponds to the `go1` tag which has no installable download
    /// - beta/rc versions are pre-release and should be excluded by default
    fn is_valid_version(v: &str) -> bool {
        v != "1" && !regex!(r"(beta|rc)[0-9]*$").is_match(v)
    }

    // Represents go binary path
    fn go_bin(&self, tv: &ToolVersion) -> PathBuf {
        tv.install_path().join("bin").join("go")
    }

    // Represents GOPATH environment variable
    fn gopath(&self, tv: &ToolVersion) -> PathBuf {
        tv.install_path().join("packages")
    }

    // Represents GOROOT environment variable
    fn goroot(&self, tv: &ToolVersion) -> PathBuf {
        let old_path = tv.install_path().join("go");
        if old_path.exists() {
            return old_path;
        }
        tv.install_path()
    }

    // Represents GOBIN environment variable
    fn gobin(&self, tv: &ToolVersion) -> PathBuf {
        tv.install_path().join("bin")
    }

    fn install_default_packages(
        &self,
        tv: &ToolVersion,
        pr: &dyn SingleReport,
    ) -> eyre::Result<()> {
        let settings = Settings::get();
        let default_packages_file = file::replace_path(&settings.go.default_packages_file);
        let body = file::read_to_string(default_packages_file).unwrap_or_default();
        let mut packages = body
            .lines()
            .filter_map(Settings::parse_default_package_line)
            .peekable();
        if packages.peek().is_some() {
            Settings::warn_default_package_file_deprecated(
                "go.default_packages_file",
                "go package",
            );
        }
        for package in packages {
            pr.set_message(format!("install default package: {package}"));
            let package = if package.contains('@') {
                package.to_string()
            } else {
                format!("{package}@latest")
            };
            CmdLineRunner::new(self.go_bin(tv))
                .with_pr(pr)
                .arg("install")
                .arg(package)
                .envs(self._exec_env(tv)?)
                .env_values(tv.install_env())
                .execute()?;
        }
        Ok(())
    }

    fn test_go(&self, tv: &ToolVersion, pr: &dyn SingleReport) -> eyre::Result<()> {
        pr.set_message("go version".into());
        CmdLineRunner::new(self.go_bin(tv))
            // run the command in the install path to prevent issues with go.mod version mismatch
            .current_dir(tv.install_path())
            .with_pr(pr)
            .arg("version")
            .env_values(tv.install_env())
            .execute()
    }

    async fn download(&self, tv: &mut ToolVersion, pr: &dyn SingleReport) -> eyre::Result<PathBuf> {
        let settings = Settings::get();
        let tarball_url = Arc::new(
            self.get_tarball_url(tv, &PlatformTarget::from_current())
                .await?
                .ok_or_else(|| eyre::eyre!("Failed to get go tarball URL"))?,
        );
        let filename = tarball_url.split('/').next_back().unwrap();
        let tarball_path = tv.download_path().join(filename);

        let tarball_url_ = tarball_url.clone();
        let checksum_handle = tokio::spawn(async move {
            let checksum_url = format!("{}.sha256", &tarball_url_);
            HTTP.get_text(checksum_url).await
        });
        pr.set_message(format!("download {filename}"));
        HTTP.download_file(&*tarball_url, &tarball_path, Some(pr))
            .await?;

        if !settings.go.skip_checksum {
            let platform_key = self.get_platform_key();
            let platform_info = tv.lock_platforms.entry(platform_key).or_default();
            platform_info.url = Some(tarball_url.to_string());
            if platform_info.checksum.is_none() {
                let checksum = checksum_handle.await.unwrap()?;
                platform_info.checksum = Some(format!("sha256:{checksum}"));
            }
        }
        Ok(tarball_path)
    }

    fn install(
        &self,
        tv: &ToolVersion,
        pr: &dyn SingleReport,
        tarball_path: &Path,
    ) -> eyre::Result<()> {
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
                ExtractionFormat::TarGz,
                &ExtractOptions {
                    pr: Some(pr),
                    ..Default::default()
                },
            )?;
        }
        file::remove_all(tv.install_path())?;
        file::rename(tmp_extract_path.path().join("go"), tv.install_path())?;
        Ok(())
    }

    fn verify(&self, tv: &ToolVersion, pr: &dyn SingleReport) -> eyre::Result<()> {
        self.test_go(tv, pr)?;
        if let Err(err) = self.install_default_packages(tv, pr) {
            warn!("failed to install default go packages: {err:#}");
        }
        let settings = Settings::get();
        if settings.go.set_gopath {
            warn!("setting go.set_gopath is deprecated");
        }
        Ok(())
    }

    fn _exec_env(&self, tv: &ToolVersion) -> eyre::Result<BTreeMap<String, String>> {
        let mut map = BTreeMap::new();
        let mut set = |k: &str, v: PathBuf| {
            map.insert(k.to_string(), v.to_string_lossy().to_string());
        };
        let settings = Settings::get();
        let gobin = settings.go.set_gobin;
        let gobin_env_is_set = env::PRISTINE_ENV.contains_key("GOBIN");
        if gobin == Some(true) || (gobin.is_none() && !gobin_env_is_set) {
            set("GOBIN", self.gobin(tv));
        }
        if settings.go.set_goroot {
            set("GOROOT", self.goroot(tv));
        }
        if settings.go.set_gopath {
            set("GOPATH", self.gopath(tv));
        }
        Ok(map)
    }
}

#[async_trait]
impl Backend for GoPlugin {
    fn ba(&self) -> &Arc<BackendArg> {
        &self.ba
    }

    async fn security_info(&self) -> Vec<crate::backend::SecurityFeature> {
        use crate::backend::SecurityFeature;

        vec![SecurityFeature::Checksum {
            algorithm: Some("sha256".to_string()),
        }]
    }

    async fn _list_remote_versions(&self, _config: &Arc<Config>) -> eyre::Result<Vec<VersionInfo>> {
        // Extract repo name (e.g., "golang/go") from the configured URL
        // The go.repo setting is like "https://github.com/golang/go"
        let settings = Settings::get();
        let repo = settings
            .go
            .repo
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .trim_start_matches("github.com/")
            .trim_end_matches(".git")
            .trim_end_matches('/');

        // Go uses tags, not releases. When MISE_LIST_ALL_VERSIONS is set,
        // we fetch tags with dates (slower). Otherwise, use fast method without dates.
        let versions: Vec<VersionInfo> = if *env::MISE_LIST_ALL_VERSIONS {
            // Slow path: fetch tags with commit dates for versions host
            github::list_tags_with_dates(repo)
                .await?
                .into_iter()
                .filter_map(|t| t.name.strip_prefix("go").map(|v| (v.to_string(), t.date)))
                .filter(|(v, _)| Self::is_valid_version(v))
                .unique_by(|(v, _)| v.clone())
                .sorted_by_cached_key(|(v, _)| (Versioning::new(v), v.to_string()))
                .map(|(version, created_at)| VersionInfo {
                    version,
                    created_at,
                    ..Default::default()
                })
                .collect()
        } else {
            // Fast path: use git ls-remote to get all go tags efficiently
            // We can't use github::list_tags here because golang/go has 500+ tags
            // and the "go1.x" version tags aren't on the first page of API results
            let go_repo = Settings::get().go.repo.clone();
            plugins::core::run_fetch_task_with_timeout_async(async move || {
                let output = crate::cmd::cmd_read_async_inherited_env(
                    "git",
                    &["ls-remote", "--tags", "--refs", &go_repo, "go*"],
                    std::iter::empty::<(&str, &std::ffi::OsStr)>(),
                )
                .await?;
                let versions: Vec<VersionInfo> = output
                    .lines()
                    .filter_map(|line| line.split("/go").last())
                    .filter(|s| !s.is_empty())
                    .filter(|s| Self::is_valid_version(s))
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
    async fn _idiomatic_filenames(&self) -> eyre::Result<Vec<String>> {
        Ok(vec![".go-version".into(), "go.mod".into()])
    }

    async fn _parse_idiomatic_file(&self, path: &Path) -> eyre::Result<Vec<String>> {
        let v = match path.file_name() {
            Some(name) if name == "go.mod" => parse_gomod(&file::read_to_string(path)?),
            _ => {
                // .go-version
                let body = normalize_idiomatic_contents(&file::read_to_string(path)?);
                body.trim().trim_start_matches('v').to_string()
            }
        };
        if v.is_empty() {
            return Ok(vec![]);
        }
        Ok(vec![v])
    }

    async fn install_version_(
        &self,
        ctx: &InstallContext,
        mut tv: ToolVersion,
    ) -> Result<ToolVersion> {
        let tarball_path = self.download(&mut tv, ctx.pr.as_ref()).await?;
        ctx.pr.next_operation();
        self.verify_checksum(ctx, &mut tv, &tarball_path)?;
        ctx.pr.next_operation();
        self.install(&tv, ctx.pr.as_ref(), &tarball_path)?;
        self.verify(&tv, ctx.pr.as_ref())?;

        Ok(tv)
    }

    async fn uninstall_version_impl(
        &self,
        _config: &Arc<Config>,
        _pr: &dyn SingleReport,
        tv: &ToolVersion,
    ) -> eyre::Result<()> {
        let gopath = self.gopath(tv);
        if gopath.exists() {
            cmd!("chmod", "-R", "u+wx", gopath).run()?;
        }
        Ok(())
    }

    async fn list_bin_paths(
        &self,
        _config: &Arc<Config>,
        tv: &ToolVersion,
    ) -> eyre::Result<Vec<PathBuf>> {
        if let ToolRequest::System { .. } = tv.request {
            return Ok(vec![]);
        }
        // goroot/bin must always be included, irrespective of MISE_GO_SET_GOROOT
        Ok(vec![self.gobin(tv)])
    }

    async fn exec_env(
        &self,
        _config: &Arc<Config>,
        _ts: &Toolset,
        tv: &ToolVersion,
    ) -> eyre::Result<BTreeMap<String, String>> {
        self._exec_env(tv)
    }

    async fn get_tarball_url(
        &self,
        tv: &ToolVersion,
        target: &PlatformTarget,
    ) -> Result<Option<String>> {
        let settings = Settings::get();
        let platform = match target.os_name() {
            "macos" => "darwin",
            "linux" => "linux",
            "windows" => "windows",
            _ => "linux",
        };
        let arch = match target.arch_name() {
            "x64" => "amd64",
            "arm64" => "arm64",
            "arm" => "armv6l",
            "riscv64" => "riscv64",
            other => other,
        };
        let ext = if target.os_name() == "windows" {
            "zip"
        } else {
            "tar.gz"
        };
        Ok(Some(format!(
            "{}/go{}.{}-{}.{}",
            &settings.go.download_mirror, tv.version, platform, arch, ext
        )))
    }

    async fn resolve_lock_info(
        &self,
        tv: &ToolVersion,
        target: &PlatformTarget,
    ) -> Result<PlatformInfo> {
        let settings = Settings::get();

        // Build tarball URL
        let url = self
            .get_tarball_url(tv, target)
            .await?
            .ok_or_else(|| eyre::eyre!("Failed to get go tarball URL"))?;

        // Go provides .sha256 files alongside each tarball
        let checksum = if !settings.go.skip_checksum {
            let checksum_url = format!("{}.sha256", &url);
            fetch_checksum_from_file(&checksum_url, "sha256").await
        } else {
            None
        };

        Ok(PlatformInfo {
            url: Some(url),
            checksum,
            size: None,
            url_api: None,
            conda_deps: None,
            ..Default::default()
        })
    }
}

/// Returns true if `v` is a plain numeric-dotted version like `1.22` or `1.22.5`.
/// Pre-releases (`1.22rc1`), `default`, and other non-numeric forms are rejected so
/// they never mis-resolve.
fn is_go_version(v: &str) -> bool {
    regex!(r"^[0-9]+(\.[0-9]+)*$").is_match(v)
}

/// Parse a `go.mod` file into a Go version request for idiomatic version resolution.
///
/// go.mod carries two relevant directives with different semantics:
/// - `toolchain goX.Y.Z` is the *exact* toolchain the module builds/tests with (what
///   `go version` reports in the repo), so it takes precedence and resolves exactly.
/// - `go X.Y` is the *minimum* required Go version. mise materializes it as a prefix,
///   resolving to the latest matching patch (e.g. `go 1.22` -> latest `1.22.x`), which
///   is consistent with how every other idiomatic version file (`.go-version`, etc.) is
///   resolved and picks up patch/security updates.
///
/// Returns an empty string when no usable version is found (malformed, pre-release, or
/// missing directive) so the caller skips the file rather than erroring or pinning a
/// wrong version.
fn parse_gomod(body: &str) -> String {
    // Value of the first `<keyword> <value>` directive, ignoring `//` line comments.
    let directive_value = |keyword: &str| -> Option<String> {
        body.lines().find_map(|line| {
            let line = line.split("//").next().unwrap_or("");
            let mut parts = line.split_whitespace();
            if parts.next() == Some(keyword) {
                parts.next().map(|s| s.to_string())
            } else {
                None
            }
        })
    };

    // Prefer a concrete `toolchain goX.Y.Z` pin; otherwise fall back to the `go` minimum.
    // An invalid toolchain value (e.g. `toolchain default`, a pre-release) falls through
    // to a usable `go` directive rather than discarding the file.
    directive_value("toolchain")
        .and_then(|v| v.strip_prefix("go").map(|s| s.to_string()))
        .filter(|v| is_go_version(v))
        .or_else(|| directive_value("go").filter(|v| is_go_version(v)))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use indoc::indoc;
    use pretty_assertions::assert_eq;

    #[test]
    fn test_parse_gomod() {
        // bare `go` directive -> minor version (mise resolves to the latest patch)
        assert_eq!(
            parse_gomod(indoc! {r#"
                module example.com/mymodule
                go 1.14
                require (
                    example.com/othermodule v1.2.3
                )
            "#}),
            "1.14"
        );
        // `toolchain` (exact pin) takes precedence over `go` (minimum)
        assert_eq!(
            parse_gomod(indoc! {r#"
                module example.com/m
                go 1.22
                toolchain go1.22.5
            "#}),
            "1.22.5"
        );
        // `toolchain default` is ignored -> fall back to the `go` directive
        assert_eq!(
            parse_gomod(indoc! {r#"
                go 1.22
                toolchain default
            "#}),
            "1.22"
        );
        // full patch version in the `go` directive is used as-is (resolves exactly)
        assert_eq!(parse_gomod("go 1.21.4\n"), "1.21.4");
        // inline `//` comments and extra whitespace are ignored
        assert_eq!(parse_gomod("go   1.20   // set by go mod tidy\n"), "1.20");
        // pre-releases are not resolvable -> skip the file
        assert_eq!(parse_gomod("go 1.22rc1\n"), "");
        // an invalid pre-release toolchain falls back to a valid `go` line
        assert_eq!(parse_gomod("go 1.21\ntoolchain go1.21rc1\n"), "1.21");
        // no version directive -> empty (file skipped)
        assert_eq!(parse_gomod("module example.com/m\n"), "");
    }
}
