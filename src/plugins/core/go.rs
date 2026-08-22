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
pub(super) struct GoPlugin {
    ba: Arc<BackendArg>,
}

impl GoPlugin {
    pub(super) fn new() -> Self {
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
            let checksum_url = format!("{}.sha256", tarball_url_);
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
    async fn _parse_idiomatic_file(&self, path: &Path) -> eyre::Result<Vec<String>> {
        let v = match path.file_name() {
            Some(name) if name == "go.mod" => parse_gomod(
                &file::read_to_string(path)?,
                Settings::get().idiomatic_version_file_ignore_minimum_versions,
            ),
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
            settings.go.download_mirror, tv.version, platform, arch, ext
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
            let checksum_url = format!("{}.sha256", url);
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

/// A `go` directive version: the minimum language version, `major.minor` with an
/// optional patch (e.g. `1.22` or `1.22.5`). A bare `1` is rejected so it can't be
/// mistaken for a version prefix that resolves to the newest Go release.
fn is_go_directive_version(v: &str) -> bool {
    regex!(r"^[0-9]+\.[0-9]+(\.[0-9]+)?$").is_match(v)
}

/// A `toolchain` version: a fully-qualified Go release `major.minor.patch` (e.g.
/// `1.22.5`). Go toolchain names always carry the patch (`go1.22.5`, never `go1.22`),
/// and pre-releases are excluded from resolution, so anything else falls back to the
/// deprecated `go` directive rather than being used as an exact pin.
fn is_go_toolchain_version(v: &str) -> bool {
    regex!(r"^[0-9]+\.[0-9]+\.[0-9]+$").is_match(v)
}

/// Parse a `go.mod` file into a Go version request for idiomatic version resolution.
///
/// `toolchain goX.Y.Z` is the *exact* toolchain the module builds and tests with (what
/// `go version` reports inside the repo), so it is a real version declaration and is
/// what mise reads.
///
/// `go X.Y` declares only the *minimum* Go version the module is compatible with. That
/// is a consumer compatibility floor, the same kind of declaration as `package.json`'s
/// `engines` field, which mise ignores — it says nothing about which version the project
/// is developed with, so it should never have selected a version to install. It is
/// deprecated and still resolves (as a prefix, to the latest matching patch) until
/// 2026.11.0, or until `ignore_minimums` opts a project into the final behavior early.
///
/// Returns an empty string when no usable version is found (malformed, pre-release, or
/// missing directive) so the caller skips the file rather than erroring or pinning a
/// wrong version.
fn parse_gomod(body: &str, ignore_minimums: bool) -> String {
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

    // A fully-qualified `toolchain goX.Y.Z` pin is the only non-deprecated source. A
    // malformed/partial/pre-release toolchain (e.g. `toolchain default`,
    // `toolchain go1.22`, `toolchain go1.22rc1`) is not a real toolchain name, so it
    // falls through to the `go` directive rather than discarding the file.
    if let Some(toolchain) = directive_value("toolchain")
        .and_then(|v| v.strip_prefix("go").map(|s| s.to_string()))
        .filter(|v| is_go_toolchain_version(v))
    {
        return toolchain;
    }

    if ignore_minimums {
        return String::new();
    }

    match directive_value("go").filter(|v| is_go_directive_version(v)) {
        Some(minimum) => {
            deprecated_at!(
                "2026.8.10",
                "2026.11.0",
                "idiomatic.go.mod.go-directive",
                "the `go` directive in go.mod is only a minimum compatible version, not the version this project is built with, so mise will stop reading it. Add a `toolchain goX.Y.Z` line to go.mod, or set the version in .go-version or mise.toml."
            );
            minimum
        }
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use indoc::indoc;
    use pretty_assertions::assert_eq;

    #[test]
    fn test_parse_gomod() {
        // a fully-qualified `toolchain` pin is used
        assert_eq!(
            parse_gomod(
                indoc! {r#"
                module example.com/m
                go 1.22
                toolchain go1.22.5
            "#},
                false
            ),
            "1.22.5"
        );
        // a `toolchain` pin with no `go` directive is still usable
        assert_eq!(parse_gomod("toolchain go1.22.0\n", false), "1.22.0");
        // inline `//` comments and extra whitespace are ignored
        assert_eq!(
            parse_gomod("toolchain   go1.20.3   // set by go mod tidy\n", false),
            "1.20.3"
        );
        // no version directive -> empty (file skipped)
        assert_eq!(parse_gomod("module example.com/m\n", false), "");
    }

    /// The `go` directive is a minimum, not a version to install. It is deprecated and
    /// still resolves until 2026.11.0; every case here goes away with it.
    #[test]
    fn test_parse_gomod_deprecated_go_directive() {
        // bare `go` directive -> minor version (mise resolves to the latest patch)
        assert_eq!(
            parse_gomod(
                indoc! {r#"
                module example.com/mymodule
                go 1.14
                require (
                    example.com/othermodule v1.2.3
                )
            "#},
                false
            ),
            "1.14"
        );
        // `toolchain` (exact pin) takes precedence over `go` (minimum)
        assert_eq!(
            parse_gomod(
                indoc! {r#"
                module example.com/m
                go 1.22
                toolchain go1.22.5
            "#},
                false
            ),
            "1.22.5"
        );
        // `toolchain default` is ignored -> fall back to the `go` directive
        assert_eq!(
            parse_gomod(
                indoc! {r#"
                go 1.22
                toolchain default
            "#},
                false
            ),
            "1.22"
        );
        // full patch version in the `go` directive is used as-is (resolves exactly)
        assert_eq!(parse_gomod("go 1.21.4\n", false), "1.21.4");
        // inline `//` comments and extra whitespace are ignored
        assert_eq!(
            parse_gomod("go   1.20   // set by go mod tidy\n", false),
            "1.20"
        );
        // pre-releases are not resolvable -> skip the file
        assert_eq!(parse_gomod("go 1.22rc1\n", false), "");
        // an invalid pre-release toolchain falls back to a valid `go` line
        assert_eq!(parse_gomod("go 1.21\ntoolchain go1.21rc1\n", false), "1.21");
        // a partial (not fully-qualified) toolchain is not a real toolchain name;
        // fall back to the `go` directive
        assert_eq!(parse_gomod("go 1.22\ntoolchain go1.22\n", false), "1.22");
        // a bare major-only directive is rejected (would resolve to the newest Go)
        assert_eq!(parse_gomod("go 1\n", false), "");
    }

    /// `idiomatic_version_file_ignore_minimum_versions` opts a project into the final
    /// behavior early: the `go` directive is not read, and `toolchain` is unaffected.
    #[test]
    fn test_parse_gomod_ignore_minimum_versions() {
        assert_eq!(parse_gomod("go 1.21\n", true), "");
        assert_eq!(parse_gomod("go 1.21.4\n", true), "");
        assert_eq!(parse_gomod("go 1.21\ntoolchain default\n", true), "");
        assert_eq!(parse_gomod("go 1.21\ntoolchain go1.21.4\n", true), "1.21.4");
        assert_eq!(parse_gomod("toolchain go1.21.4\n", true), "1.21.4");
    }
}
