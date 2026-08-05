use crate::backend::backend_type::BackendType;
use crate::backend::options::BackendOptions;
use crate::backend::platform_target::PlatformTarget;
use crate::backend::{Backend, VersionInfo};
use crate::cache::{CacheManager, CacheManagerBuilder};
use crate::cli::args::BackendArg;
use crate::cmd::CmdLineRunner;
use crate::config::{Config, Settings};
use crate::duration::parse_into_timestamp;
#[cfg(unix)]
use crate::env;
#[cfg(unix)]
use crate::file;
use crate::github::{self, GithubRelease};
use crate::hash::hash_to_str;
use crate::http::HTTP_FETCH;
use crate::install_context::InstallContext;
use crate::plugins::{PEP440_PRERELEASE_REGEX, VERSION_REGEX};
use crate::semver::semver_is_older_than;
use crate::timeout;
use crate::toolset::{ToolRequest, ToolVersion, ToolVersionOptions, Toolset, ToolsetBuilder};
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::ui::progress_report::SingleReport;
use async_trait::async_trait;
use eyre::{Result, bail, eyre};
use indexmap::IndexMap;
use itertools::Itertools;
use jiff::Timestamp;
use regex::Regex;
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::Path;
#[cfg(unix)]
use std::path::PathBuf;
use std::str::FromStr;
use std::{fmt::Debug, sync::Arc};
use versions::Versioning;
use xx::regex;

const UV_EXCLUDE_NEWER_VERSION: &str = "0.2.22";

#[derive(Debug)]
pub struct PIPXBackend {
    ba: Arc<BackendArg>,
}

#[derive(Debug, Clone, Copy)]
struct PipxOptions<'a> {
    values: BackendOptions<'a>,
}

impl<'a> PipxOptions<'a> {
    fn new(raw: &'a ToolVersionOptions) -> Self {
        Self {
            values: BackendOptions::new(raw),
        }
    }

    fn extras(&self) -> Option<String> {
        self.values.comma_joined("extras")
    }

    fn package_name(&self) -> Option<&'a str> {
        self.values.str("package_name")
    }

    fn pipx_args(&self) -> Option<&'a str> {
        self.values.str("pipx_args")
    }

    fn registry_url(&self) -> Option<&'a str> {
        self.values.str("registry_url")
    }

    fn uvx_args(&self) -> Option<&'a str> {
        self.values.str("uvx_args")
    }

    fn uvx_disabled(&self) -> bool {
        self.values.raw().get_string("uvx").as_deref() == Some("false")
    }

    fn lockfile_options(&self) -> BTreeMap<String, String> {
        let mut result = BTreeMap::new();
        if let Some(value) = self.extras() {
            result.insert("extras".to_string(), value);
        }
        for key in install_time_option_keys() {
            if key == "extras" {
                continue;
            }
            if let Some(value) = self.values.raw().get_string(&key) {
                result.insert(key, value);
            }
        }
        result
    }
}

#[async_trait]
impl Backend for PIPXBackend {
    fn get_type(&self) -> BackendType {
        BackendType::Pipx
    }

    fn ba(&self) -> &Arc<BackendArg> {
        &self.ba
    }

    fn get_dependencies(&self) -> eyre::Result<Vec<&str>> {
        // python is required because pipx.pyz uses `#!/usr/bin/env python3`
        // and pipx_cmd relies on dependency_toolset to put python ahead of
        // any system python on PATH.
        Ok(vec!["pipx", "python"])
    }

    fn get_optional_dependencies(&self) -> eyre::Result<Vec<&str>> {
        Ok(vec!["uv"])
    }

    fn mark_prereleases_from_version_pattern(&self) -> bool {
        true
    }

    fn is_prerelease_version(&self, version: &str) -> bool {
        VERSION_REGEX.is_match(version) || PEP440_PRERELEASE_REGEX.is_match(version)
    }

    /// PyPI versions follow PEP 440, so the shared filter alone (which only
    /// knows about `-rc1`/`-dev` separators) would let `3.12.0a1`-style
    /// versions slip through. See `fuzzy_match_versions_pep440`.
    fn fuzzy_match_filter(
        &self,
        versions: Vec<String>,
        query: &str,
        filter_prereleases: bool,
    ) -> Vec<String> {
        crate::backend::fuzzy_match_versions_pep440(versions, query, filter_prereleases)
    }

    /// Pipx installs packages from PyPI or Git using version specs (e.g., black==24.3.0).
    /// It doesn't support installing from direct URLs, so lockfile URLs are not applicable.
    fn supports_lockfile_url(&self) -> bool {
        false
    }

    async fn remote_version_cache_context(&self, config: &Arc<Config>) -> Result<Option<String>> {
        match self.tool_name().parse()? {
            PipxRequest::Pypi(_) => self.get_registry_url(config).await.map(Some),
            PipxRequest::Git(_) => Ok(None),
        }
    }

    async fn _list_remote_versions(&self, config: &Arc<Config>) -> eyre::Result<Vec<VersionInfo>> {
        let versions: Vec<VersionInfo> = match self.tool_name().parse()? {
            PipxRequest::Pypi(package) => {
                let registry_url = self.get_registry_url(config).await?;
                if registry_url.contains("/json") {
                    debug!("Fetching JSON for {}", package);
                    let url = registry_url.replace("{}", &package);
                    let data: PypiPackage = HTTP_FETCH.json(url).await?;

                    Self::versions_from_pypi_package(data)
                } else {
                    debug!("Fetching HTML for {}", package);
                    let url = registry_url.replace("{}", &package);
                    let html = HTTP_FETCH.get_html(url).await?;

                    // PEP-0503 (HTML format doesn't include timestamps)
                    Self::versions_from_simple_index(&package, &html)
                        .into_iter()
                        .map(|version| VersionInfo {
                            version,
                            ..Default::default()
                        })
                        .sorted_by_cached_key(|v| Versioning::new(&v.version))
                        .collect()
                }
            }
            PipxRequest::Git(url) if url.starts_with("https://github.com/") => {
                let repo = url.strip_prefix("https://github.com/").unwrap();
                let data = github::list_releases(repo).await?;
                Self::versions_from_github_releases(data)
            }
            PipxRequest::Git { .. } => vec![],
        };
        // PyPI versions follow PEP 440. Stamp the separator-less alpha/beta/rc
        // suffixes (`3.12.0a1`, `1.0.0c1`) here rather than in the shared
        // regex so the rule stays scoped to Python — hex commit hashes used
        // by other ecosystems (e.g. Go pseudo-versions) would false-positive.
        Ok(versions
            .into_iter()
            .map(|mut v| {
                if !v.prerelease && PEP440_PRERELEASE_REGEX.is_match(&v.version) {
                    v.prerelease = true;
                }
                v
            })
            .collect())
    }

    async fn latest_stable_version(&self, config: &Arc<Config>) -> eyre::Result<Option<String>> {
        let package = match self.tool_name().parse()? {
            PipxRequest::Pypi(package) => package,
            PipxRequest::Git(_) => return Ok(None),
        };
        let registry_url = self.get_registry_url(config).await?;
        let latest_version_cache = self.latest_version_cache(&registry_url);
        timeout::run_with_timeout_async(
            async || {
                latest_version_cache
                    .get_or_try_init_async(async || {
                        if registry_url.contains("/json") {
                            debug!("Fetching JSON for {}", package);
                            let url = registry_url.replace("{}", &package);
                            let pkg: PypiPackage = HTTP_FETCH.json(url).await?;
                            Ok(Self::latest_stable_from_pypi_package(pkg))
                        } else {
                            debug!("Fetching HTML for {}", package);
                            let url = registry_url.replace("{}", &package);
                            let html = HTTP_FETCH.get_html(url).await?;

                            let version = Self::versions_from_simple_index(&package, &html)
                                .into_iter()
                                .filter(|v| {
                                    !v.contains("dev")
                                        && !v.contains("a")
                                        && !v.contains("b")
                                        && !v.contains("rc")
                                })
                                .sorted_by_cached_key(|v| Versioning::new(v))
                                .next_back();

                            Ok(version)
                        }
                    })
                    .await
            },
            Settings::get().fetch_remote_versions_timeout(),
        )
        .await
        .cloned()
    }

    fn unresolved_latest_version(&self) -> Option<String> {
        match self.tool_name().parse() {
            Ok(PipxRequest::Git(_)) => Some("latest".to_string()),
            _ => None,
        }
    }

    async fn resolve_exact_version(
        &self,
        config: &Arc<Config>,
        version: &str,
    ) -> eyre::Result<Option<String>> {
        // Git-sourced tools resolve versions from repo tags, which cannot be
        // validated from the version string alone.
        if !matches!(
            self.tool_name().parse::<PipxRequest>(),
            Ok(PipxRequest::Pypi(_))
        ) {
            return Ok(None);
        }
        // Surface malformed registry configuration at resolve time like
        // remote discovery would — installation only sees the derived index
        // URL, which skips this validation.
        self.get_registry_url(config).await?;
        // PEP 440 allows non-semver versions (1.2.3.4, 1.2.3rc1, 1.2.3.post1)
        // — those keep using remote discovery. A full semver request is
        // exact; `pipx install pkg==version` / `uv tool install` fail when it
        // does not exist upstream.
        Ok(versions::SemVer::new(version).map(|_| version.to_string()))
    }

    async fn install_version_(&self, ctx: &InstallContext, tv: ToolVersion) -> Result<ToolVersion> {
        let request_options = tv.request.options();
        let options = PipxOptions::new(&request_options);

        // Check if pipx is available (unless uvx is being used)
        //
        // Asks for a *spawnable* uv, because this both picks the branch and supplies the
        // program `uvx_cmd` hands to `CmdLineRunner`. A `uv.ps1` or a shebang-only `uv`
        // satisfies the plain lookup, so mise would commit to the uv branch and then fail
        // at process creation; treating it as absent falls through to pipx, which either
        // works or reports the install instructions below. The branch only changes in the
        // case where the branch it would have taken cannot run.
        let uvx_allowed = Settings::get().pipx.uvx != Some(false) && !options.uvx_disabled();
        let uv_program = if uvx_allowed {
            self.spawnable_dependency(&ctx.config, Some(&ctx.ts), "uv")
                .await
        } else {
            None
        };

        if uv_program.is_none() {
            // Only offer uv as an alternative when this package can actually use it.
            // Packages that set `uvx = false` (or a `pipx.uvx = false` setting) always go
            // through pipx, so pointing at uv there just sends people down a dead end.
            let instructions = if uvx_allowed {
                "To use pipx packages with mise, you need to install pipx first:\n  \
                   mise use pipx@latest\n\n\
                 Alternatively, you can use uv/uvx by installing uv:\n  \
                   mise use uv@latest"
                    .to_string()
            } else {
                let reason = if options.uvx_disabled() {
                    "this package sets `uvx = false`"
                } else {
                    "uvx is disabled by the `pipx.uvx` setting"
                };
                format!(
                    "This package is installed with pipx because {reason}, so uv/uvx cannot be \
                     used for it.\n\nInstall pipx first:\n  mise use pipx@latest"
                )
            };
            self.warn_if_dependency_missing(&ctx.config, "pipx", &["pipx"], &instructions)
                .await;

            // Fail with the instructions above rather than letting `pipx install` die with a
            // bare "No such file or directory (os error 2)". Skipped when a configured tool
            // provides pipx, since mise installs that first — same rule as the warning.
            //
            // The gate asks `spawnable_dependency`, the same question `spawn_program` asks
            // below, so it cannot pass on evidence the spawn will then reject. On Windows a
            // `pipx.ps1` or a shebang-only `pipx.pyz` satisfies the plain lookup but cannot
            // be launched, and it used to reach `pipx install` and die with
            // "program not found" instead of these instructions.
            let pipx_configured = match self.dependency_toolset(&ctx.config).await {
                Ok(ts) => ts.versions.keys().any(|ba| ba.short == "pipx"),
                Err(_) => false,
            };
            if !pipx_configured
                && self
                    .spawnable_dependency(&ctx.config, Some(&ctx.ts), "pipx")
                    .await
                    .is_none()
            {
                bail!(
                    "pipx is required to install {} but was not found.\n\n{instructions}",
                    self.ba()
                );
            }
        }

        let request = self.tool_name().parse::<PipxRequest>()?;

        if let Some(uv_program) = uv_program {
            let package_request = request.uvx_request(&tv.version, &options);
            self.warn_if_uv_may_not_support_exclude_newer(ctx).await;
            ctx.pr
                .set_message(format!("uv tool install {package_request}"));
            let mut cmd = Self::uvx_cmd(
                &uv_program,
                &ctx.config,
                &["tool", "install", &package_request],
                self,
                &tv,
                &ctx.ts,
                ctx.pr.as_ref(),
            )
            .await?;
            cmd = cmd.args(Self::uv_exclude_newer_args(ctx.before_date));
            if let Some(args) = options.uvx_args() {
                cmd = cmd.args(shell_words::split(args)?);
            }
            cmd.execute()?;
        } else {
            // pipx forwards install `--pip-args` into shared-library bootstrap
            // (`pip install --upgrade pip>=23.1`), not just the package install. When mise
            // passes `--uploaded-prior-to`, bootstrap pip from ensurepip may not understand
            // that flag (see pypa/pipx#544). Run upgrade-shared without release-age flags
            // first so shared pip is valid; the subsequent install's shared_libs.create()
            // then no-ops and `--uploaded-prior-to` applies only to the package install.
            if ctx.before_date.is_some() {
                ctx.pr.set_message("pipx upgrade-shared".to_string());
                if let Err(err) = async {
                    Self::pipx_cmd(
                        &ctx.config,
                        &["upgrade-shared"],
                        self,
                        &tv,
                        &ctx.ts,
                        ctx.pr.as_ref(),
                    )
                    .await?
                    .execute()
                }
                .await
                {
                    debug!("failed to upgrade pipx shared libraries before install: {err:#}");
                }
            }

            let package_request = request.pipx_request(&tv.version, &options);
            ctx.pr
                .set_message(format!("pipx install {package_request}"));
            let mut cmd = Self::pipx_cmd(
                &ctx.config,
                &["install", &package_request],
                self,
                &tv,
                &ctx.ts,
                ctx.pr.as_ref(),
            )
            .await?;
            cmd = cmd.args(Self::pip_uploaded_prior_to_args(ctx.before_date));
            if let Some(args) = options.pipx_args() {
                cmd = cmd.args(shell_words::split(args)?);
            }
            cmd.execute()?;
        }

        // Fix venv Python symlink to use minor version path
        // This allows patch upgrades (3.12.1 → 3.12.2) to work without reinstalling
        let pkg_name = self.tool_name();
        fix_venv_python_symlink(&tv.install_path(), &pkg_name)?;

        Ok(tv)
    }

    fn resolve_lockfile_options(
        &self,
        request: &ToolRequest,
        _target: &PlatformTarget,
    ) -> Result<BTreeMap<String, String>> {
        let opts = request.options();
        Ok(PipxOptions::new(&opts).lockfile_options())
    }
}

/// Returns install-time-only option keys for PIPX backend.
pub fn install_time_option_keys() -> Vec<String> {
    vec![
        "extras".into(),
        "package_name".into(),
        "pipx_args".into(),
        "uvx_args".into(),
        "uvx".into(),
    ]
}

impl PIPXBackend {
    fn versions_from_simple_index(package: &str, html: &str) -> Vec<String> {
        let href_re = regex!(r#"(?i)href\s*=\s*["']([^"']+)["']"#);

        href_re
            .captures_iter(html)
            .filter_map(|cap| {
                let href = cap.get(1)?.as_str();
                let path = href.split(['?', '#']).next()?;
                let filename = path.rsplit('/').next()?;
                let filename = urlencoding::decode(filename).ok()?;

                Self::version_from_distribution_filename(package, &filename)
            })
            .unique()
            .collect()
    }

    fn version_from_distribution_filename(package: &str, filename: &str) -> Option<String> {
        let normalized_package = Self::normalize_package_name(package);

        if let Some(stem) = filename.strip_suffix(".whl") {
            let fields = stem.split('-').collect_vec();
            if !(fields.len() == 5 || fields.len() == 6)
                || Self::normalize_package_name(fields[0]) != normalized_package
            {
                return None;
            }
            return Some(fields[1].to_string());
        }

        let stem = filename.strip_suffix(".tar.gz")?;
        stem.match_indices('-').find_map(|(index, _)| {
            (Self::normalize_package_name(&stem[..index]) == normalized_package)
                .then(|| stem[index + 1..].to_string())
        })
    }

    fn normalize_package_name(package: &str) -> String {
        let mut normalized = String::with_capacity(package.len());
        let mut separator = false;
        for c in package.chars() {
            if matches!(c, '-' | '_' | '.') {
                if !separator {
                    normalized.push('-');
                    separator = true;
                }
            } else {
                normalized.extend(c.to_lowercase());
                separator = false;
            }
        }
        normalized
    }

    fn versions_from_pypi_package(data: PypiPackage) -> Vec<VersionInfo> {
        // Releases with only yanked files are ignored so fuzzy/latest
        // resolution mirrors pip's default yanked-file behavior.
        data.releases
            .into_iter()
            .filter(|(_, files)| files.iter().any(|f| !f.yanked))
            .sorted_by_cached_key(|(v, _)| Versioning::new(v))
            .map(|(version, files)| {
                // Prefer the RFC3339 `upload_time_iso_8601` over the
                // timezone-naive `upload_time`: the latter has no offset, so
                // `parse_into_timestamp` parses it as a `civil::Date` and
                // substitutes end-of-day UTC, dropping the real time-of-day
                // and inflating `minimum_release_age` for same-day releases by
                // up to ~24h. Custom indexes without the ISO field fall back to
                // `upload_time` as before.
                let created_at = files
                    .iter()
                    .filter(|f| !f.yanked)
                    .filter_map(|f| {
                        f.upload_time_iso_8601
                            .clone()
                            .or_else(|| f.upload_time.clone())
                    })
                    // Pick the earliest upload as a parsed instant, not by
                    // lexicographic string order: RFC3339 strings with
                    // different offsets (`...00:00-05:00` vs `...04:00Z`) do
                    // not sort chronologically, so a naive `.min()` on the raw
                    // strings can select a later instant and over-gate.
                    .min_by_key(|s| parse_into_timestamp(s).unwrap_or(Timestamp::MAX));

                VersionInfo {
                    version,
                    created_at,
                    ..Default::default()
                }
            })
            .collect()
    }

    fn latest_stable_from_pypi_package(data: PypiPackage) -> Option<String> {
        Self::versions_from_pypi_package(data)
            .into_iter()
            .rev()
            .find(|v| !PEP440_PRERELEASE_REGEX.is_match(&v.version))
            .map(|v| v.version)
    }

    fn versions_from_github_releases(releases: Vec<GithubRelease>) -> Vec<VersionInfo> {
        releases
            .into_iter()
            .rev()
            .map(|r| {
                let created_at = Some(r.released_at().to_string());
                VersionInfo {
                    version: r.tag_name,
                    created_at,
                    ..Default::default()
                }
            })
            .collect()
    }

    fn uv_exclude_newer_args(before_date: Option<Timestamp>) -> Vec<OsString> {
        match before_date {
            Some(before_date) => vec!["--exclude-newer".into(), before_date.to_string().into()],
            None => vec![],
        }
    }

    fn pip_uploaded_prior_to_args(before_date: Option<Timestamp>) -> Vec<OsString> {
        match before_date {
            Some(before_date) => {
                vec![format!("--pip-args=--uploaded-prior-to={before_date}").into()]
            }
            None => vec![],
        }
    }

    pub fn from_arg(ba: BackendArg) -> Self {
        Self { ba: Arc::new(ba) }
    }

    fn latest_version_cache(&self, registry_url: &str) -> CacheManager<Option<String>> {
        let registry_hash = hash_to_str(&registry_url);
        CacheManagerBuilder::new(
            self.ba
                .cache_path
                .join(format!("latest_version_{registry_hash}.msgpack.z")),
        )
        .with_fresh_duration(Settings::get().fetch_remote_versions_cache())
        .build()
    }

    fn get_index_url() -> eyre::Result<String> {
        let registry_url = Settings::get().pipx.registry_url.clone();

        // Remove {} placeholders and trailing slashes
        let mut url = registry_url
            .replace("{}", "")
            .trim_end_matches('/')
            .to_string();

        // Handle different URL formats and convert to simple format
        if url.contains("pypi.org") {
            // For pypi.org, convert any format to simple format
            if url.contains("/pypi/") {
                // Replace /pypi/*/json or /pypi/*/simple with /simple
                let re = Regex::new(r"/pypi/[^/]*/(?:json|simple)$").unwrap();
                url = re.replace(&url, "/simple").to_string();
            } else if !url.ends_with("/simple") {
                // If it's pypi.org but doesn't already end with /simple, make it /simple
                let base_url = url.split("/simple").next().unwrap_or(&url);
                url = format!("{}/simple", base_url.trim_end_matches('/'));
            }
        } else {
            // For custom registries, ensure they end with /simple
            if url.ends_with("/json") {
                // Replace /json with /simple
                url = url.replace("/json", "/simple");
            } else if !url.ends_with("/simple") {
                // If it doesn't end with /simple, append it
                url = format!("{url}/simple");
            }
        }

        debug!("Converted registry URL to index URL: {}", url);
        Ok(url)
    }

    async fn get_registry_url(&self, config: &Arc<Config>) -> eyre::Result<String> {
        let raw_options = config.get_tool_opts_with_overrides(&self.ba).await?;
        let options = PipxOptions::new(&raw_options);
        let registry_url = options
            .registry_url()
            .map(str::to_owned)
            .unwrap_or_else(|| Settings::get().pipx.registry_url.clone());

        debug!("Pipx registry URL: {}", registry_url);

        let re = Regex::new(r"^(http|https)://.*\{\}.*$").unwrap();

        if !re.is_match(&registry_url) {
            return Err(eyre!(
                "Registry URL must be a valid URL and contain a {{}} placeholder"
            ));
        }

        Ok(registry_url)
    }

    pub async fn reinstall_all(config: &Arc<Config>) -> Result<()> {
        let ts = Arc::new(ToolsetBuilder::new().build(config).await?);
        let pipx_tools = ts
            .list_installed_versions(config)
            .await?
            .into_iter()
            .filter(|(b, _tv)| b.ba().backend_type() == BackendType::Pipx)
            .collect_vec();
        for (b, tv) in pipx_tools {
            let ctx = InstallContext {
                config: config.clone(),
                ts: ts.clone(),
                pr: MultiProgressReport::get().add(&format!("reinstalling {}", tv.style())),
                force: true,
                dry_run: false,
                locked: false,
                before_date: None,
            };
            b.install_version(ctx, tv).await?;
        }
        Ok(())
    }

    async fn uvx_cmd<'a>(
        uv_program: &Path,
        config: &Arc<Config>,
        args: &[&str],
        b: &dyn Backend,
        tv: &ToolVersion,
        ts: &Toolset,
        pr: &'a dyn SingleReport,
    ) -> Result<CmdLineRunner<'a>> {
        let mut cmd = CmdLineRunner::new(uv_program);
        for arg in args {
            cmd = cmd.arg(arg);
        }
        cmd.with_pr(pr)
            .envs(ts.env_with_path_without_tools(config).await?)
            .env_values(tv.install_env())
            .env("UV_TOOL_DIR", tv.install_path())
            .env("UV_TOOL_BIN_DIR", tv.install_path().join("bin"))
            .env("UV_INDEX", Self::get_index_url()?)
            .prepend_path(ts.list_paths(config).await)?
            .prepend_path(vec![tv.install_path().join("bin")])?
            .prepend_path(b.dependency_toolset(config).await?.list_paths(config).await)
    }

    async fn pipx_cmd<'a>(
        config: &Arc<Config>,
        args: &[&str],
        b: &dyn Backend,
        tv: &ToolVersion,
        ts: &Toolset,
        pr: &'a dyn SingleReport,
    ) -> Result<CmdLineRunner<'a>> {
        // Resolved rather than a bare "pipx": on Windows std only appends `.exe`, so a
        // pipx that exists only as `pipx.cmd` — how scoop and `pip install pipx` leave it —
        // cleared mise's dependency check and then died at the spawn (discussion #5333).
        // Same question the `bail!` gate in `install_version_` asks, so the two agree.
        let mut cmd = CmdLineRunner::new(b.spawn_program(config, Some(ts), "pipx").await);
        for arg in args {
            cmd = cmd.arg(arg);
        }
        cmd.with_pr(pr)
            .envs(ts.env_with_path_without_tools(config).await?)
            .env_values(tv.install_env())
            // pipx 1.12+ auto-picks uv on PATH; this path passes pip-only --pip-args.
            .env("PIPX_DEFAULT_BACKEND", "pip")
            .env("PIP_INDEX_URL", Self::get_index_url()?)
            .env_remove("PIPX_SHARED_LIBS")
            .env("PIPX_HOME", tv.install_path())
            .env("PIPX_BIN_DIR", tv.install_path().join("bin"))
            .prepend_path(ts.list_paths(config).await)?
            .prepend_path(vec![tv.install_path().join("bin")])?
            .prepend_path(b.dependency_toolset(config).await?.list_paths(config).await)
    }

    async fn warn_if_uv_may_not_support_exclude_newer(&self, ctx: &InstallContext) {
        if ctx.before_date.is_none() {
            return;
        }

        let Some(version) =
            crate::backend::semver_version_from_toolsets_or_path(self, &ctx.config, &ctx.ts, "uv")
                .await
        else {
            warn!(
                "minimum_release_age is set for pipx:{} but could not determine uv version required to verify --exclude-newer support. Release-age filtering for transitive dependencies may not work as expected. See https://mise.jdx.dev/dev-tools/backends/pipx.html",
                self.tool_name(),
            );
            return;
        };

        if semver_is_older_than(&version, UV_EXCLUDE_NEWER_VERSION).unwrap_or(false) {
            warn!(
                "minimum_release_age is set for pipx:{} but uv@{} is older than the documented minimum uv@{} required for --exclude-newer. Older versions may fail while processing the forwarded argument. See https://mise.jdx.dev/dev-tools/backends/pipx.html",
                self.tool_name(),
                version,
                UV_EXCLUDE_NEWER_VERSION,
            );
        }
    }
}

enum PipxRequest {
    /// git+https://github.com/psf/black.git@24.2.0
    /// psf/black@24.2.0
    Git(String),
    /// black@24.2.0
    Pypi(String),
}

impl PipxRequest {
    fn extras_from_opts(&self, opts: &PipxOptions<'_>) -> String {
        match opts.extras() {
            Some(extras) => format!("[{extras}]"),
            None => String::new(),
        }
    }

    fn git_url(url: &str, v: &str) -> String {
        if v == "latest" {
            format!("git+{url}.git")
        } else {
            format!("git+{url}.git@{v}")
        }
    }

    fn git_package_name<'a>(url: &'a str, opts: &PipxOptions<'a>) -> &'a str {
        opts.package_name()
            .unwrap_or_else(|| url.rsplit('/').next().unwrap_or(url))
    }

    fn uvx_request(&self, v: &str, opts: &PipxOptions<'_>) -> String {
        let extras = self.extras_from_opts(opts);

        match self {
            PipxRequest::Git(url) => {
                let git_url = Self::git_url(url, v);
                if extras.is_empty() {
                    git_url
                } else {
                    let package = Self::git_package_name(url, opts);
                    format!("{package}{extras} @ {git_url}")
                }
            }
            PipxRequest::Pypi(package) if v == "latest" => format!("{package}{extras}"),
            PipxRequest::Pypi(package) => format!("{package}{extras}=={v}"),
        }
    }

    fn pipx_request(&self, v: &str, opts: &PipxOptions<'_>) -> String {
        let extras = self.extras_from_opts(opts);

        match self {
            PipxRequest::Git(url) => {
                let git_url = Self::git_url(url, v);
                if extras.is_empty() {
                    git_url
                } else {
                    // pipx ignored extras on PEP 508 URL requirements before 0.15.6.0.
                    // Its VCS `egg` form works across the supported pipx version range.
                    let package = Self::git_package_name(url, opts);
                    format!("{git_url}#egg={package}{extras}")
                }
            }
            PipxRequest::Pypi(package) if v == "latest" => format!("{package}{extras}"),
            PipxRequest::Pypi(package) => format!("{package}{extras}=={v}"),
        }
    }
}

#[derive(serde::Deserialize)]
struct PypiPackage {
    releases: IndexMap<String, Vec<PypiRelease>>,
}

#[derive(serde::Deserialize)]
struct PypiRelease {
    upload_time: Option<String>,
    upload_time_iso_8601: Option<String>,
    #[serde(default, deserialize_with = "deserialize_pypi_yanked")]
    yanked: bool,
}

fn deserialize_pypi_yanked<'de, D>(deserializer: D) -> std::result::Result<bool, D::Error>
where
    D: serde::Deserializer<'de>,
{
    match Option::<Value>::deserialize(deserializer)? {
        None | Some(Value::Null) => Ok(false),
        Some(Value::Bool(yanked)) => Ok(yanked),
        Some(Value::String(_)) => Ok(true),
        Some(value) => Err(serde::de::Error::custom(format!(
            "expected bool or string for yanked, got {value}"
        ))),
    }
}

impl FromStr for PipxRequest {
    type Err = eyre::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Some(cap) = regex!(r"(git\+)(.*)(\.git)").captures(s) {
            Ok(PipxRequest::Git(cap.get(2).unwrap().as_str().to_string()))
        } else if s.contains('/') {
            Ok(PipxRequest::Git(format!("https://github.com/{s}")))
        } else {
            Ok(PipxRequest::Pypi(s.to_string()))
        }
    }
}

/// Check if a path is within mise's Python installs directory
#[cfg(unix)]
fn is_mise_managed_python(path: &Path) -> bool {
    let installs_dir = &*env::MISE_INSTALLS_DIR;
    path.starts_with(installs_dir.join("python"))
}

/// Convert a Python path with full version to use minor version
/// e.g., .../python/3.12.1/bin/python → .../python/3.12/bin/python
#[cfg(unix)]
fn path_with_minor_version(path: &Path) -> Option<PathBuf> {
    let path_str = path.to_str()?;

    // Match pattern: /python/X.Y.Z/ and replace with /python/X.Y/
    let re = regex!(r"/python/(\d+)\.(\d+)\.\d+/");
    if re.is_match(path_str) {
        let result = re.replace(path_str, "/python/$1.$2/");
        Some(PathBuf::from(result.to_string()))
    } else {
        None
    }
}

/// Ensure the minor version symlink exists for a Python installation path.
/// For example, if the path is `.../python/3.12.1/bin/python3`, this ensures
/// that `.../python/3.12` exists as a symlink to `./3.12.1`.
///
/// This is normally done by `runtime_symlinks::rebuild()`, but that runs after
/// postinstall hooks. We need to create it early so that venv symlinks work
/// immediately for postinstall hooks.
#[cfg(unix)]
fn ensure_minor_version_symlink(full_version_path: &Path) -> Result<()> {
    // Extract version components from path like .../python/3.12.1/bin/python3
    // Use same regex pattern as path_with_minor_version for consistency
    let re = regex!(r"/python/(\d+)\.(\d+)\.(\d+)/");
    let path_str = match full_version_path.to_str() {
        Some(s) => s,
        None => return Ok(()),
    };

    let caps = match re.captures(path_str) {
        Some(c) => c,
        None => return Ok(()),
    };

    let minor_version = format!("{}.{}", &caps[1], &caps[2]); // e.g., "3.12"
    let full_version = format!("{}.{}.{}", &caps[1], &caps[2], &caps[3]); // e.g., "3.12.1"

    let installs_dir = &*env::MISE_INSTALLS_DIR;
    let python_installs = installs_dir.join("python");
    let minor_version_dir = python_installs.join(&minor_version);
    let full_version_dir = python_installs.join(&full_version);

    // Only create if the minor version symlink doesn't exist but the full version does
    if !minor_version_dir.exists() && full_version_dir.exists() {
        trace!(
            "Creating early minor version symlink: {:?} -> ./{:?}",
            minor_version_dir, full_version
        );
        // Use relative symlink with "./" prefix like runtime_symlinks does
        // This allows is_runtime_symlink() to identify it for cleanup/updates
        file::make_symlink(&PathBuf::from(".").join(&full_version), &minor_version_dir)?;
    }

    Ok(())
}

/// Fix the venv Python symlinks to use mise's minor version path
/// This allows patch upgrades (3.12.1 → 3.12.2) to work without reinstalling
///
/// The venv structure typically has:
/// - python -> python3 (relative symlink)
/// - python3 -> /path/to/mise/installs/python/3.12.1/bin/python3 (absolute symlink)
///
/// We need to fix the absolute symlink to use minor version path (3.12 instead of 3.12.1)
#[cfg(unix)]
fn fix_venv_python_symlink(install_path: &Path, pkg_name: &str) -> Result<()> {
    // For Git-based packages like "psf/black", the venv directory is just "black"
    // Extract the actual package name (last component after any '/')
    let actual_pkg_name = pkg_name.rsplit('/').next().unwrap_or(pkg_name);

    // Check both possible venv locations: {pkg}/ for uvx, venvs/{pkg}/ for pipx
    let venv_dirs = [
        install_path.join(actual_pkg_name),
        install_path.join("venvs").join(actual_pkg_name),
    ];

    trace!(
        "fix_venv_python_symlink: checking venv dirs: {:?}",
        venv_dirs
    );

    for venv_dir in &venv_dirs {
        let bin_dir = venv_dir.join("bin");
        if !bin_dir.exists() {
            continue;
        }

        // Check python, python3, and python3.X symlinks for the one with absolute mise path
        for name in &["python", "python3"] {
            let symlink_path = bin_dir.join(name);
            if !symlink_path.is_symlink() {
                continue;
            }

            let target = match file::resolve_symlink(&symlink_path)? {
                Some(t) => t,
                None => continue,
            };

            // Skip relative symlinks (like python -> python3)
            if !target.is_absolute() {
                continue;
            }

            if !is_mise_managed_python(&target) {
                continue; // Leave non-mise Python alone (homebrew, uv, etc.)
            }

            if let Some(minor_path) = path_with_minor_version(&target)
                && target.exists()
            {
                // Create the minor version symlink (e.g., python/3.12 -> python/3.12.1)
                // if it doesn't exist yet. This is normally done by runtime_symlinks::rebuild,
                // but that runs after postinstall hooks, so we need to create it now
                // to ensure the venv symlink works immediately for postinstall hooks.
                ensure_minor_version_symlink(&target)?;

                trace!(
                    "Updating venv Python symlink {:?} to use minor version: {:?}",
                    symlink_path, minor_path
                );
                file::make_symlink(&minor_path, &symlink_path)?;
            }
        }
    }
    Ok(())
}

/// No-op on non-Unix platforms
#[cfg(not(unix))]
fn fix_venv_python_symlink(_install_path: &Path, _pkg_name: &str) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        PIPXBackend, PipxOptions, PipxRequest, PypiPackage, PypiRelease, UV_EXCLUDE_NEWER_VERSION,
    };
    use crate::github::GithubRelease;
    use crate::toolset::ToolVersionOptions;
    use indexmap::IndexMap;
    use pretty_assertions::assert_eq;
    use std::ffi::OsString;

    #[test]
    fn parses_versions_from_simple_index_artifacts() {
        let html = r#"
            <a href="https://files.example/demo_pkg-1.0.0.tar.gz#sha256=abc">sdist</a>
            <a href="demo_pkg-2.0.0-py3-none-any.whl#sha256=def">wheel</a>
            <a href="/demo_pkg-2.0.0-cp313-cp313-manylinux_x86_64.whl">duplicate wheel</a>
            <a href="/demo_pkg-2.1.0-1-py3-none-any.whl">wheel with build tag</a>
            <a href="/demo_pkg-3.0.0rc1-py3-none-any.whl">prerelease wheel</a>
            <a href="/other-9.9.9-py3-none-any.whl">other package</a>
        "#;

        assert_eq!(
            PIPXBackend::versions_from_simple_index("demo-pkg", html),
            vec!["1.0.0", "2.0.0", "2.1.0", "3.0.0rc1"]
        );
    }

    #[test]
    fn parses_normalized_and_encoded_simple_index_filenames() {
        let html = r#"
            <a href="demo.pkg-1.0%2Bcpu.tar.gz?download=1">sdist</a>
            <a HREF='DEMO_PKG-2.0%2Bcpu-py3-none-any.whl'>wheel</a>
        "#;

        assert_eq!(
            PIPXBackend::versions_from_simple_index("Demo_Pkg", html),
            vec!["1.0+cpu", "2.0+cpu"]
        );
    }

    #[tokio::test]
    async fn simple_index_resolves_wheel_only_packages() {
        use crate::backend::Backend;

        let mut server = mockito::Server::new_async().await;
        let index = server
            .mock("GET", "/simple/wheel-only/")
            .with_status(200)
            .with_header("content-type", "text/html")
            .with_body(
                r#"
                    <a href="wheel_only-1.0.0-py3-none-any.whl">1.0.0</a>
                    <a href="wheel_only-2.0.0-py3-none-any.whl">2.0.0</a>
                    <a href="wheel_only-2.0.0-cp313-cp313-manylinux_x86_64.whl">2.0.0</a>
                "#,
            )
            .expect(2)
            .create_async()
            .await;
        let config = crate::config::Config::get().await.unwrap();
        let backend = PIPXBackend::from_arg(
            format!(
                "pipx:wheel-only[registry_url='{}/simple/{{}}/']",
                server.url()
            )
            .into(),
        );

        assert_eq!(
            backend
                .list_remote_versions_with_selection_options(&config, &backend.ba().opts(), false,)
                .await
                .unwrap(),
            vec!["1.0.0", "2.0.0"]
        );
        assert_eq!(
            backend
                .latest_stable_version(&config)
                .await
                .unwrap()
                .as_deref(),
            Some("2.0.0")
        );
        index.assert_async().await;
    }

    #[tokio::test]
    async fn exact_semver_versions_resolve_without_remote_discovery() {
        use crate::backend::Backend;
        let config = crate::config::Config::get().await.unwrap();
        let backend = PIPXBackend::from_arg("pipx:black".into());

        assert_eq!(
            backend
                .resolve_exact_version(&config, "24.3.0")
                .await
                .unwrap()
                .as_deref(),
            Some("24.3.0")
        );
    }

    #[tokio::test]
    async fn per_tool_registry_url_resolves_latest_version() {
        use crate::backend::Backend;

        let mut server = mockito::Server::new_async().await;
        let registry = server
            .mock("GET", "/pypi/private-tool/json")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                serde_json::json!({
                    "releases": {
                        "1.0.0": [{
                            "upload_time": "2026-01-01T00:00:00",
                            "upload_time_iso_8601": "2026-01-01T00:00:00Z",
                            "yanked": false
                        }]
                    }
                })
                .to_string(),
            )
            .create_async()
            .await;
        let config = crate::config::Config::get().await.unwrap();
        let backend = PIPXBackend::from_arg(
            format!(
                "pipx:private-tool[registry_url='{}/pypi/{{}}/json']",
                server.url()
            )
            .into(),
        );

        assert_eq!(
            backend
                .latest_stable_version(&config)
                .await
                .unwrap()
                .as_deref(),
            Some("1.0.0")
        );
        registry.assert_async().await;
    }

    #[tokio::test]
    async fn per_tool_registries_isolate_remote_version_cache() {
        use crate::backend::Backend;

        let mut first_server = mockito::Server::new_async().await;
        let first_registry = first_server
            .mock("GET", "/pypi/private-tool/json")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                serde_json::json!({
                    "releases": {
                        "1.0.0": [{
                            "upload_time": "2026-01-01T00:00:00",
                            "upload_time_iso_8601": "2026-01-01T00:00:00Z",
                            "yanked": false
                        }]
                    }
                })
                .to_string(),
            )
            .create_async()
            .await;
        let mut second_server = mockito::Server::new_async().await;
        let second_registry = second_server
            .mock("GET", "/pypi/private-tool/json")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                serde_json::json!({
                    "releases": {
                        "2.0.0": [{
                            "upload_time": "2026-02-01T00:00:00",
                            "upload_time_iso_8601": "2026-02-01T00:00:00Z",
                            "yanked": false
                        }]
                    }
                })
                .to_string(),
            )
            .create_async()
            .await;
        let config = crate::config::Config::get().await.unwrap();
        let first_backend = PIPXBackend::from_arg(
            format!(
                "pipx:private-tool[registry_url='{}/pypi/{{}}/json']",
                first_server.url()
            )
            .into(),
        );
        let second_backend = PIPXBackend::from_arg(
            format!(
                "pipx:private-tool[registry_url='{}/pypi/{{}}/json']",
                second_server.url()
            )
            .into(),
        );

        assert_eq!(
            first_backend
                .list_remote_versions_with_selection_options(
                    &config,
                    &first_backend.ba().opts(),
                    false,
                )
                .await
                .unwrap(),
            vec!["1.0.0"]
        );
        assert_eq!(
            second_backend
                .list_remote_versions_with_selection_options(
                    &config,
                    &second_backend.ba().opts(),
                    false,
                )
                .await
                .unwrap(),
            vec!["2.0.0"]
        );
        first_registry.assert_async().await;
        second_registry.assert_async().await;
    }

    #[tokio::test]
    async fn non_semver_versions_require_remote_discovery() {
        use crate::backend::Backend;
        let config = crate::config::Config::get().await.unwrap();
        let backend = PIPXBackend::from_arg("pipx:black".into());

        // PEP 440 versions that are not semver must keep resolving against
        // the remote version list.
        for version in ["latest", "24", "24.3", "1.2.3.4", "1.2.3rc1", "1.2.3.post1"] {
            assert_eq!(
                backend
                    .resolve_exact_version(&config, version)
                    .await
                    .unwrap(),
                None,
                "{version} should use remote discovery"
            );
        }
    }

    #[tokio::test]
    async fn git_tools_keep_remote_discovery() {
        use crate::backend::Backend;
        let config = crate::config::Config::get().await.unwrap();

        for tool in [
            "pipx:psf/black",
            "pipx:git+https://github.com/psf/black.git",
        ] {
            let backend = PIPXBackend::from_arg(tool.into());
            assert_eq!(
                backend
                    .resolve_exact_version(&config, "24.3.0")
                    .await
                    .unwrap(),
                None,
                "{tool} should use remote discovery"
            );
        }
    }

    #[test]
    fn test_extras_accepts_string_or_array() {
        let mut string_opts = ToolVersionOptions::default();
        string_opts.opts.insert(
            "extras".to_string(),
            toml::Value::String("postgres,s3".to_string()),
        );
        assert_eq!(
            PipxOptions::new(&string_opts).extras().as_deref(),
            Some("postgres,s3")
        );
        assert_eq!(
            PipxOptions::new(&string_opts)
                .lockfile_options()
                .get("extras"),
            Some(&"postgres,s3".to_string())
        );

        let mut array_opts = ToolVersionOptions::default();
        array_opts.opts.insert(
            "extras".to_string(),
            toml::Value::Array(vec![
                toml::Value::String("postgres".to_string()),
                toml::Value::Integer(1),
                toml::Value::String("s3".to_string()),
            ]),
        );
        assert_eq!(
            PipxOptions::new(&array_opts).extras().as_deref(),
            Some("postgres,s3")
        );
        assert_eq!(
            PipxRequest::Pypi("harlequin".to_string())
                .pipx_request("latest", &PipxOptions::new(&array_opts)),
            "harlequin[postgres,s3]"
        );
        assert_eq!(
            PipxOptions::new(&array_opts)
                .lockfile_options()
                .get("extras"),
            Some(&"postgres,s3".to_string())
        );
    }

    #[test]
    fn test_git_extras_use_frontend_compatible_requests() {
        let mut inferred_opts = ToolVersionOptions::default();
        inferred_opts.opts.insert(
            "extras".to_string(),
            toml::Value::Array(vec![toml::Value::String("jupyter".to_string())]),
        );
        let inferred_opts = PipxOptions::new(&inferred_opts);
        let inferred_request = PipxRequest::Git("https://github.com/psf/black".to_string());
        assert_eq!(
            inferred_request.uvx_request("latest", &inferred_opts),
            "black[jupyter] @ git+https://github.com/psf/black.git"
        );
        assert_eq!(
            inferred_request.pipx_request("latest", &inferred_opts),
            "git+https://github.com/psf/black.git#egg=black[jupyter]"
        );

        let mut named_opts = ToolVersionOptions::default();
        named_opts.opts.insert(
            "extras".to_string(),
            toml::Value::Array(vec![toml::Value::String("jupyter".to_string())]),
        );
        named_opts.opts.insert(
            "package_name".to_string(),
            toml::Value::String("black".to_string()),
        );
        let named_opts = PipxOptions::new(&named_opts);
        let request = PipxRequest::Git("https://github.com/psf/black-repository".to_string());

        assert_eq!(
            request.uvx_request("latest", &named_opts),
            "black[jupyter] @ git+https://github.com/psf/black-repository.git"
        );
        assert_eq!(
            request.uvx_request("24.3.0", &named_opts),
            "black[jupyter] @ git+https://github.com/psf/black-repository.git@24.3.0"
        );
        assert_eq!(
            request.pipx_request("latest", &named_opts),
            "git+https://github.com/psf/black-repository.git#egg=black[jupyter]"
        );
        assert_eq!(
            request.pipx_request("24.3.0", &named_opts),
            "git+https://github.com/psf/black-repository.git@24.3.0#egg=black[jupyter]"
        );
        assert_eq!(
            named_opts.lockfile_options().get("package_name"),
            Some(&"black".to_string())
        );
    }

    #[test]
    fn test_versions_from_pypi_package_skips_yanked_releases() {
        let versions = PIPXBackend::versions_from_pypi_package(pypi_package(vec![
            (
                "1.0.0",
                vec![pypi_release(Some("2024-01-01T00:00:00Z"), false)],
            ),
            (
                "1.1.0",
                vec![pypi_release(Some("2024-02-01T00:00:00Z"), true)],
            ),
            (
                "1.2.0",
                vec![
                    pypi_release(Some("2024-03-01T00:00:00Z"), true),
                    pypi_release(Some("2024-03-01T00:01:00Z"), false),
                ],
            ),
        ]));

        assert_eq!(
            versions
                .iter()
                .map(|v| (v.version.as_str(), v.created_at.as_deref()))
                .collect::<Vec<_>>(),
            vec![
                ("1.0.0", Some("2024-01-01T00:00:00Z")),
                ("1.2.0", Some("2024-03-01T00:01:00Z")),
            ]
        );
    }

    #[test]
    fn test_versions_from_pypi_package_preserves_time_of_day_from_iso_field() {
        // PyPI's JSON carries both a naive `upload_time` (no offset) and an
        // RFC3339 `upload_time_iso_8601`. Preferring the ISO field keeps the
        // real upload instant; the naive field would parse as a `civil::Date`
        // and collapse to end-of-day UTC, inflating `minimum_release_age`.
        fn release(iso: Option<&str>, naive: Option<&str>) -> PypiRelease {
            PypiRelease {
                upload_time: naive.map(str::to_string),
                upload_time_iso_8601: iso.map(str::to_string),
                yanked: false,
            }
        }

        let versions = PIPXBackend::versions_from_pypi_package(pypi_package(vec![
            // Both fields present: the precise midday instant wins.
            (
                "1.0.0",
                vec![release(
                    Some("2024-01-02T10:05:14.723989Z"),
                    Some("2024-01-02T10:05:14"),
                )],
            ),
            // Index without the ISO field: naive fallback still yields a
            // timestamp (here end-of-day UTC), so release-age gating degrades
            // gracefully rather than disappearing.
            ("1.1.0", vec![release(None, Some("2024-01-03"))]),
        ]));

        let parsed: Vec<_> = versions
            .iter()
            .map(|v| v.created_at_timestamp().map(|t| t.to_string()))
            .collect();
        // ISO field: real upload instant, not 2024-01-02T23:59:59Z.
        assert_eq!(parsed[0].as_deref(), Some("2024-01-02T10:05:14.723989Z"));
        // Naive-only fallback: parses as a date, end-of-day UTC.
        assert_eq!(parsed[1].as_deref(), Some("2024-01-03T23:59:59Z"));
    }

    #[test]
    fn test_versions_from_pypi_package_picks_earliest_instant_not_lexical_min() {
        // A custom index may return RFC3339 timestamps with differing offsets.
        // These two are the same instant, but lexicographic order and
        // chronological UTC order diverge for different-offset strings, so the
        // earliest upload must be selected by parsed instant.
        //
        //   "2024-01-02T00:00:00-05:00"  ==  2024-01-02T05:00:00Z
        //   "2024-01-02T04:30:00Z"            2024-01-02T04:30:00Z  (earlier)
        //
        // Lexical min is the first ("-05:00" string sorts before "Z"), but the
        // second is 30 min earlier in UTC and must win.
        let release = |iso: &str| PypiRelease {
            upload_time: None,
            upload_time_iso_8601: Some(iso.to_string()),
            yanked: false,
        };
        let versions = PIPXBackend::versions_from_pypi_package(pypi_package(vec![(
            "1.0.0",
            vec![
                release("2024-01-02T00:00:00-05:00"),
                release("2024-01-02T04:30:00Z"),
            ],
        )]));
        assert_eq!(
            versions[0].created_at.as_deref(),
            Some("2024-01-02T04:30:00Z"),
        );
    }

    #[test]
    fn test_latest_stable_from_pypi_package_skips_yanked_and_prerelease() {
        let version = PIPXBackend::latest_stable_from_pypi_package(pypi_package(vec![
            (
                "1.0.0",
                vec![pypi_release(Some("2024-01-01T00:00:00Z"), false)],
            ),
            (
                "1.1.0",
                vec![pypi_release(Some("2024-02-01T00:00:00Z"), false)],
            ),
            (
                "1.2.0",
                vec![pypi_release(Some("2024-03-01T00:00:00Z"), true)],
            ),
            (
                "2.0.0a1",
                vec![pypi_release(Some("2024-04-01T00:00:00Z"), false)],
            ),
        ]));

        assert_eq!(version.as_deref(), Some("1.1.0"));
    }

    #[test]
    fn test_pypi_release_deserializes_string_yanked_reason() {
        let release: PypiRelease = serde_json::from_value(serde_json::json!({
            "upload_time": "2024-01-01T00:00:00Z",
            "yanked": "broken release"
        }))
        .unwrap();

        assert!(release.yanked);
    }

    #[test]
    fn test_versions_from_pypi_package_skips_empty_releases() {
        let versions = PIPXBackend::versions_from_pypi_package(pypi_package(vec![
            ("1.0.0", vec![]),
            (
                "1.1.0",
                vec![pypi_release(Some("2024-02-01T00:00:00Z"), false)],
            ),
        ]));

        assert_eq!(
            versions
                .iter()
                .map(|v| v.version.as_str())
                .collect::<Vec<_>>(),
            vec!["1.1.0"]
        );
    }

    #[test]
    fn test_versions_from_empty_github_releases_stays_empty() {
        let versions = PIPXBackend::versions_from_github_releases(vec![]);

        assert!(versions.is_empty());
    }

    #[test]
    fn test_versions_from_github_releases_preserves_tags() {
        let versions = PIPXBackend::versions_from_github_releases(vec![
            github_release("2.0.0", "2024-02-01T00:00:00Z"),
            github_release("1.0.0", "2024-01-01T00:00:00Z"),
        ]);

        assert_eq!(
            versions
                .iter()
                .map(|v| (v.version.as_str(), v.created_at.as_deref()))
                .collect::<Vec<_>>(),
            vec![
                ("1.0.0", Some("2024-01-01T00:00:00Z")),
                ("2.0.0", Some("2024-02-01T00:00:00Z")),
            ]
        );
    }

    #[test]
    fn test_uv_exclude_newer_args_with_cutoff() {
        let before_date = "2024-01-02T03:04:05Z".parse().unwrap();
        let args = PIPXBackend::uv_exclude_newer_args(Some(before_date));

        assert_eq!(
            args,
            vec![
                OsString::from("--exclude-newer"),
                OsString::from("2024-01-02T03:04:05Z"),
            ]
        );
    }

    #[test]
    fn test_uv_exclude_newer_args_without_cutoff() {
        assert_eq!(
            PIPXBackend::uv_exclude_newer_args(None),
            Vec::<OsString>::new()
        );
    }

    #[test]
    fn test_uv_exclude_newer_version_requirement() {
        assert_eq!(UV_EXCLUDE_NEWER_VERSION, "0.2.22");
        assert_eq!(
            crate::semver::semver_is_at_least("0.2.22", UV_EXCLUDE_NEWER_VERSION),
            Some(true)
        );
        assert_eq!(
            crate::semver::semver_is_at_least("0.2.21", UV_EXCLUDE_NEWER_VERSION),
            Some(false)
        );
    }

    #[test]
    fn test_pip_uploaded_prior_to_args_with_cutoff() {
        let before_date = "2024-01-02T03:04:05Z".parse().unwrap();
        let args = PIPXBackend::pip_uploaded_prior_to_args(Some(before_date));

        // Combined into a single `--pip-args=VALUE` argv element so pipx's
        // argparse doesn't treat the leading `--` of the value as a new flag
        // (see discussion #9976).
        assert_eq!(
            args,
            vec![OsString::from(
                "--pip-args=--uploaded-prior-to=2024-01-02T03:04:05Z"
            )]
        );
    }

    #[test]
    fn test_pip_uploaded_prior_to_args_without_cutoff() {
        assert_eq!(
            PIPXBackend::pip_uploaded_prior_to_args(None),
            Vec::<OsString>::new()
        );
    }

    fn github_release(tag_name: &str, created_at: &str) -> GithubRelease {
        GithubRelease {
            tag_name: tag_name.to_string(),
            draft: false,
            prerelease: false,
            created_at: created_at.to_string(),
            published_at: None,
            assets: vec![],
        }
    }

    fn pypi_package(releases: Vec<(&str, Vec<PypiRelease>)>) -> PypiPackage {
        PypiPackage {
            releases: releases
                .into_iter()
                .map(|(version, files)| (version.to_string(), files))
                .collect::<IndexMap<_, _>>(),
        }
    }

    fn pypi_release(upload_time: Option<&str>, yanked: bool) -> PypiRelease {
        PypiRelease {
            upload_time: upload_time.map(str::to_string),
            upload_time_iso_8601: upload_time.map(str::to_string),
            yanked,
        }
    }
}
