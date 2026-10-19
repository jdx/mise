mod lock;

use crate::args::BackendArg;
use crate::backend::backend_type::BackendType;
use crate::backend::options::BackendOptions;
use crate::backend::platform_target::PlatformTarget;
use crate::backend::{Backend, VersionInfo};
use crate::cache::{CacheManager, CacheManagerBuilder};
use crate::cmd::CmdLineRunner;
use crate::config::{Config, Settings, SettingsExt};
use crate::duration::parse_into_timestamp;
#[cfg(unix)]
use crate::env;
#[cfg(unix)]
use crate::file;
use crate::github::{self, GithubRelease};
use crate::hash::hash_to_str;
use crate::http::HTTP_FETCH;
use crate::install_before::release_age_list_window_for_cutoff;
use crate::install_context::InstallContext;
use crate::plugins::is_python_prerelease;
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
use reqwest::header::{ACCEPT, CONTENT_TYPE, HeaderMap, HeaderValue};
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::{fmt::Debug, sync::Arc};
use versions::Versioning;
use xx::regex;

const UV_EXCLUDE_NEWER_VERSION: &str = "0.2.22";
const UV_WITH_EXECUTABLES_FROM_VERSION: &str = "0.8.5";

/// A portable, mise-owned CPython selector for one PyPI tool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SelectedPython {
    Minor(String),
    Exact(String),
}

impl SelectedPython {
    fn parse(value: &str) -> Result<Self> {
        let parts = value.split('.').collect::<Vec<_>>();
        let numeric = |part: &str| !part.is_empty() && part.bytes().all(|c| c.is_ascii_digit());
        match parts.as_slice() {
            [major, minor] if numeric(major) && numeric(minor) => {
                Ok(Self::Minor(format!("{major}.{minor}")))
            }
            [major, minor, patch] if numeric(major) && numeric(minor) && numeric(patch) => {
                Ok(Self::Exact(format!("{major}.{minor}.{patch}")))
            }
            _ => bail!(
                "python must be a numeric CPython minor (for example 3.12) or exact patch (for example 3.12.12)"
            ),
        }
    }

    pub(crate) fn request_version(&self) -> &str {
        match self {
            Self::Minor(version) | Self::Exact(version) => version,
        }
    }

    pub(crate) fn requires_python(&self) -> String {
        match self {
            Self::Minor(version) => {
                let (major, minor) = version.split_once('.').expect("validated minor");
                let next_minor = minor.parse::<u32>().expect("validated minor") + 1;
                format!(">={major}.{minor},<{major}.{next_minor}")
            }
            Self::Exact(version) => format!("=={version}"),
        }
    }
}

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

    fn string_list(&self, key: &str) -> Result<Vec<String>> {
        let Some(value) = self.values.raw().opts.get(key) else {
            return Ok(Vec::new());
        };
        let values = match value {
            toml::Value::String(value) => {
                if value.trim_start().starts_with('[') {
                    serde_json::from_str(value)
                        .map_err(|_| eyre!("{key} must be a string or array of strings"))?
                } else {
                    vec![value.clone()]
                }
            }
            toml::Value::Array(values) => values
                .iter()
                .map(|value| {
                    value
                        .as_str()
                        .map(str::to_owned)
                        .ok_or_else(|| eyre!("{key} must be a string or array of strings"))
                })
                .collect::<Result<Vec<_>>>()?,
            _ => bail!("{key} must be a string or array of strings"),
        };
        if values.iter().any(|value| value.trim().is_empty()) {
            bail!("{key} cannot contain empty values");
        }
        Ok(values)
    }

    fn with(&self) -> Result<Vec<String>> {
        self.string_list("with")
    }

    fn expose(&self) -> Result<Vec<String>> {
        self.string_list("expose")
    }

    fn exposed_package_names(&self) -> Result<Vec<String>> {
        self.expose()?
            .into_iter()
            .map(|requirement| {
                let Some(name) = Self::requirement_package_name(&requirement) else {
                    bail!("expose must contain named Python package requirements");
                };
                Ok(name.to_string())
            })
            .collect()
    }

    fn requirement_package_name(requirement: &str) -> Option<&str> {
        let name = requirement
            .trim()
            .split(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')))
            .next()
            .unwrap_or_default();
        (!name.is_empty()).then_some(name)
    }

    fn dependency_prereleases(&self) -> Result<Option<&'a str>> {
        let value = match self.values.raw().opts.get("dependency_prereleases") {
            Some(toml::Value::String(value)) => Some(value.as_str()),
            Some(_) => bail!("dependency_prereleases must be a string"),
            None => None,
        };
        if value.is_some_and(|value| {
            !matches!(value, "disallow" | "allow" | "if-necessary" | "explicit")
        }) {
            bail!("dependency_prereleases must be disallow, allow, if-necessary, or explicit");
        }
        Ok(value)
    }

    fn has_uv_only_options(&self) -> Result<bool> {
        Ok(!self.with()?.is_empty()
            || !self.expose()?.is_empty()
            || self.dependency_prereleases()?.is_some())
    }

    fn validate_semantic(&self) -> Result<()> {
        self.with()?;
        self.exposed_package_names()?;
        self.dependency_prereleases()?;
        if self.selected_python()?.is_some() {
            for args in [self.uvx_args(), self.pipx_args()].into_iter().flatten() {
                if uv_args_select_python(&shell_words::split(args)?) {
                    bail!("python cannot be combined with --python or -p in uvx_args or pipx_args");
                }
            }
        }
        if self.has_uv_only_options()? && self.uvx_disabled() {
            bail!("with, expose, and dependency_prereleases cannot be combined with uvx = false");
        }
        Ok(())
    }

    fn uv_install_args(&self) -> Result<Vec<String>> {
        let mut args = Vec::new();
        for requirement in self.with()? {
            args.extend(["--with".to_string(), requirement]);
        }
        for package in self.expose()? {
            args.extend(["--with-executables-from".to_string(), package]);
        }
        if let Some(value) = self.dependency_prereleases()? {
            args.extend(["--prerelease".to_string(), value.to_string()]);
        }
        Ok(args)
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

    pub(crate) fn selected_python(&self) -> Result<Option<SelectedPython>> {
        match self.values.raw().opts.get("python") {
            Some(toml::Value::String(value)) => SelectedPython::parse(value).map(Some),
            Some(_) => bail!("python must be a string"),
            None => Ok(None),
        }
    }

    fn lockfile_options(&self) -> Result<BTreeMap<String, String>> {
        let mut result = BTreeMap::new();
        if let Some(value) = self.extras() {
            result.insert("extras".to_string(), value);
        }
        for key in ["with", "expose"] {
            let values = self.string_list(key)?;
            if !values.is_empty() {
                result.insert(key.to_string(), serde_json::to_string(&values)?);
            }
        }
        for key in install_time_option_keys() {
            if matches!(key.as_str(), "extras" | "with" | "expose") {
                continue;
            }
            if let Some(value) = self.values.raw().get_string(&key) {
                result.insert(key, value);
            }
        }
        self.validate_semantic()?;
        Ok(result)
    }
}

/// Build the dependency request for an explicitly selected interpreter.  This
/// keeps the request anchored to the PyPI tool's source, so its lockfile is
/// consulted instead of whichever Python happens to be active in the shell.
pub fn selected_python_dependency(request: &ToolRequest) -> Result<Option<ToolRequest>> {
    if request.ba().backend_type() != BackendType::Pipx {
        return Ok(None);
    }
    let options = request.options();
    let options = PipxOptions::new(&options);
    options.validate_semantic()?;
    let Some(selector) = options.selected_python()? else {
        return Ok(None);
    };
    let mut dependency = ToolRequest::new(
        Arc::new(BackendArg::from("python")),
        selector.request_version(),
        request.source().clone(),
    )?;
    dependency.set_lockfile_scope(request.lockfile_scope().clone());
    Ok(Some(dependency))
}

pub(crate) fn has_selected_python(tv: &ToolVersion) -> Result<bool> {
    let options = tv.request.options();
    Ok(PipxOptions::new(&options).selected_python()?.is_some())
}

/// A version resolved from an existing legacy `~uv~` directory keeps that
/// directory in `install_path`. A selected interpreter is part of the
/// environment identity, though, so a replacement must never reuse a path
/// whose identity or receipt belongs to another interpreter. Otherwise a
/// failed replacement would clean up the still-working environment.
fn reset_selected_python_install_path(tv: &mut ToolVersion) -> Result<()> {
    if tv.install_path_is_explicit {
        return Ok(());
    }
    let Some(install_path) = tv.install_path.as_ref() else {
        return Ok(());
    };
    let Some((_, expected_identity)) = tv.uv_python.as_ref() else {
        return Ok(());
    };

    let has_target_name = install_path
        .file_name()
        .is_some_and(|name| name == std::ffi::OsStr::new(&tv.tv_pathname()));
    let has_expected_receipt =
        crate::file::read_to_string(install_path.join(".mise-uv/python.json"))
            .ok()
            .and_then(|contents| serde_json::from_str::<(PathBuf, String)>(&contents).ok())
            .is_some_and(|(_, installed_identity)| installed_identity == *expected_identity);
    if !has_target_name {
        tv.install_path = None;
    } else if !has_expected_receipt {
        bail!(
            "cannot safely replace {}: the existing environment has no matching Python receipt",
            tv.style()
        );
    }
    Ok(())
}

#[async_trait]
impl Backend for PIPXBackend {
    fn option_spec(&self) -> Option<crate::backend::OptionSpec> {
        Some(crate::backend::OptionSpec::new(&["tool_pypi_options"]))
    }

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

    fn get_dependencies_for(&self, opts: &ToolVersionOptions) -> eyre::Result<Vec<&str>> {
        if PipxOptions::new(opts).has_uv_only_options()? {
            Ok(vec!["uv", "python"])
        } else {
            self.get_dependencies()
        }
    }

    fn get_optional_dependencies(&self) -> eyre::Result<Vec<&str>> {
        Ok(vec!["uv"])
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
            PipxRequest::Pypi(_) => {
                let registry_url = self.get_registry_url(config).await?;
                Ok((registry_url != DEFAULT_REGISTRY_URL).then_some(registry_url))
            }
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
                    debug!("Fetching simple index for {}", package);
                    let url = registry_url.replace("{}", &package);
                    Self::fetch_simple_index(&package, url).await?
                }
            }
            PipxRequest::Git(source) => match PipxRequest::github_repo(&source.repo) {
                Some(repo) => {
                    let data = github::list_releases(&repo).await?;
                    Self::versions_from_github_releases(data)
                }
                None => Self::versions_from_git_tags(&source.remote()).await?,
            },
        };
        Ok(versions.into_iter().map(stamp_pep440_prerelease).collect())
    }

    // No `latest_stable_version` override: the trait asks this first and only
    // falls back to the string variant on `None`, which would repeat the
    // registry request for a package with no stable release.
    //
    // The registry answer already dates every release, and the date has to
    // travel with the version: without it, `minimum_release_age` (on by
    // default) can only check the candidate by listing every version, which
    // fetches the same document a second time.
    async fn latest_stable_version_info(
        &self,
        config: &Arc<Config>,
    ) -> eyre::Result<Option<VersionInfo>> {
        self.latest_stable_version_info_before(config, None).await
    }

    async fn latest_stable_version_info_before(
        &self,
        config: &Arc<Config>,
        before: Option<Timestamp>,
    ) -> eyre::Result<Option<VersionInfo>> {
        let package = match self.tool_name().parse()? {
            PipxRequest::Pypi(package) => package,
            PipxRequest::Git(source) => {
                return if PipxRequest::github_repo(&source.repo).is_some() {
                    Ok(None)
                } else {
                    Self::git_head(&source.remote()).await.map(|version| {
                        Some(VersionInfo {
                            version,
                            ..Default::default()
                        })
                    })
                };
            }
        };
        let registry_url = self.get_registry_url(config).await?;
        let mut latest_version_cache = self.latest_version_cache(&registry_url);
        if adopt_latest_within_release_age(&mut latest_version_cache, before) {
            debug!("Using the cached latest release of {package}, within its release-age window");
        }
        timeout::run_with_timeout_async(
            async || {
                latest_version_cache
                    .get_or_try_init_async(async || {
                        let url = registry_url.replace("{}", &package);
                        let versions = if registry_url.contains("/json") {
                            debug!("Fetching JSON for {}", package);
                            let pkg: PypiPackage = HTTP_FETCH.json(url).await?;
                            Self::versions_from_pypi_package(pkg)
                        } else {
                            debug!("Fetching simple index for {}", package);
                            Self::fetch_simple_index(&package, url).await?
                        };
                        Ok(Self::latest_stable(versions))
                    })
                    .await
            },
            Settings::get().fetch_remote_versions_timeout(),
        )
        .await
        .cloned()
    }

    fn is_rolling_channel(&self, version: &str) -> bool {
        version == "latest"
            && matches!(
                self.tool_name().parse::<PipxRequest>(),
                Ok(PipxRequest::Git(source)) if PipxRequest::github_repo(&source.repo).is_none()
            )
    }

    fn latest_installed_channel_version(&self, _channel: &str) -> Option<String> {
        // Installed versions do not retain enough provenance to distinguish a
        // rolling HEAD resolution from an explicitly installed tag or ref.
        None
    }

    async fn resolve_channel_version(
        &self,
        _config: &Arc<Config>,
        version: &str,
    ) -> Result<Option<String>> {
        if !self.is_rolling_channel(version) {
            return Ok(None);
        }
        match self.tool_name().parse()? {
            PipxRequest::Git(source) => Self::git_head(&source.remote()).await.map(Some),
            PipxRequest::Pypi(_) => Ok(None),
        }
    }

    fn requires_concrete_channel_version(&self, version: &str) -> bool {
        self.is_rolling_channel(version)
    }

    fn unresolved_latest_version(&self) -> Option<String> {
        match self.tool_name().parse() {
            Ok(PipxRequest::Git(source)) if PipxRequest::github_repo(&source.repo).is_some() => {
                Some("latest".to_string())
            }
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

    async fn prepare_install_version(
        &self,
        ctx: &InstallContext,
        mut tv: ToolVersion,
    ) -> Result<ToolVersion> {
        let request_options = tv.request.options();
        PipxOptions::new(&request_options).validate_semantic()?;
        // An explicit selector is never a graph-only preference. Bind it
        // before any branch that may fall back to version-only uv or pipx so
        // every installer receives the same exact mise-managed executable.
        if PipxOptions::new(&request_options)
            .selected_python()?
            .is_some()
        {
            self.bind_uv_python(ctx, &mut tv).await?;
            reset_selected_python_install_path(&mut tv)?;
        }
        if let Some(lock) = &tv.uv_lock {
            let lock = if !ctx.locked && lock.load().is_err() {
                lock.refresh()?
            } else {
                lock.clone()
            };
            self.validate_uv_lock(&tv, lock.load()?)?;
            tv.uv_lock = Some(lock);
        } else if self.uv_lock_allowed(&tv) {
            let revision = self.uv_lockfile_version(&ctx.config, &tv)?;
            let graph_required = revision.is_some_and(|v| v >= 2)
                || (!tv.resolved_from_lockfile() && Settings::get().lockfile_creation_enabled());
            if graph_required {
                if !self.uv_lock_options_supported(&tv) {
                    if ctx.locked {
                        self.validate_lock_options(&tv)?;
                    }
                    warn!(
                        "{} dependency graph skipped because uvx_args or pipx_args are configured; installing with version-only resolution",
                        self.ba.short
                    );
                    return Ok(tv);
                }
                if ctx.locked {
                    bail!(
                        "{} has no uv dependency graph; run `mise lock`",
                        self.ba.short
                    );
                }
                if self
                    .spawnable_dependency(&ctx.config, Some(&ctx.ts), "uv")
                    .await
                    .is_none()
                {
                    return Ok(tv);
                }
                if let Some(version) = tv.uv_install_path_version().map(str::to_owned) {
                    tv.version = version;
                }
                match self.resolve_uv_lock(&ctx.config, &tv).await {
                    Ok(lock) => tv.uv_lock = Some(lock),
                    Err(error) => {
                        warn!(
                            "{} dependency graph unavailable; installing with version-only resolution: {error}",
                            self.ba.short
                        );
                        return Ok(tv);
                    }
                }
            }
        }
        // A restored identity carries an empty path placeholder; bind the real
        // interpreter so `uv sync --python` never receives an empty argument.
        if tv.uv_lock.is_some()
            && tv
                .uv_python
                .as_ref()
                .is_none_or(|(python, _)| python.as_os_str().is_empty())
        {
            self.bind_uv_python(ctx, &mut tv).await?;
        }
        reset_selected_python_install_path(&mut tv)?;
        Ok(tv)
    }

    async fn is_install_satisfied(
        &self,
        config: &Arc<Config>,
        tv: &ToolVersion,
        check_symlink: bool,
    ) -> Result<bool> {
        let selected_python = PipxOptions::new(&tv.request.options())
            .selected_python()?
            .is_some();
        if tv.uv_lock.is_some() {
            if tv.uv_python.is_none() {
                return Ok(false);
            }
        } else if self.uv_lock_allowed(tv) {
            let revision = self.uv_lockfile_version(config, tv)?;
            let graph_required = revision.is_some_and(|v| v >= 2)
                || (!tv.resolved_from_lockfile() && Settings::get().lockfile_creation_enabled());
            let locked = config.invocation_locked_for(tv.request.source(), Settings::get().locked)
                || tv.request.tool_config_locked(config, true);
            if graph_required {
                if !selected_python
                    && !locked
                    && self.is_version_installed(config, tv, check_symlink)
                {
                    return Ok(true);
                }
                // Installer arguments which uv cannot lock deliberately use
                // version-only installation. A selected runtime still needs
                // its receipt checked below, but must not be forced to
                // reinstall once that receipt matches.
                let version_only_fallback = !self.uv_lock_options_supported(tv);
                if locked
                    || (!version_only_fallback
                        && self
                            .spawnable_dependency(config, None, "uv")
                            .await
                            .is_some())
                {
                    return Ok(false);
                }
            }
        }
        if selected_python {
            let Some((_, expected_identity)) = &tv.uv_python else {
                return Ok(false);
            };
            let Ok(contents) =
                crate::file::read_to_string(tv.install_path().join(".mise-uv/python.json"))
            else {
                return Ok(false);
            };
            let Ok((_, installed_identity)) = serde_json::from_str::<(PathBuf, String)>(&contents)
            else {
                return Ok(false);
            };
            if &installed_identity != expected_identity {
                return Ok(false);
            }
        }
        Ok(self.is_version_installed(config, tv, check_symlink))
    }

    async fn install_version_(&self, ctx: &InstallContext, tv: ToolVersion) -> Result<ToolVersion> {
        if tv.uv_lock.is_some() {
            self.install_uv_lock(ctx, &tv).await?;
            return Ok(tv);
        }

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
        let uvx_allowed = Settings::get().pypi.uvx != Some(false) && !options.uvx_disabled();
        let uv_program = if uvx_allowed {
            self.spawnable_dependency(&ctx.config, Some(&ctx.ts), "uv")
                .await
        } else {
            None
        };
        if options.has_uv_only_options()? && uv_program.is_none() {
            bail!(
                "{} semantic options (`with`, `expose`, and `dependency_prereleases`) require uv; install uv and ensure uvx is enabled",
                self.ba.short
            );
        }
        let pipx_available = if uv_program.is_none() {
            self.spawnable_dependency(&ctx.config, Some(&ctx.ts), "pipx")
                .await
                .is_some()
        } else {
            false
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
                    "uvx is disabled by the `pypi.uvx` setting"
                };
                format!(
                    "This package is installed with pipx because {reason}, so uv/uvx cannot be \
                     used for it.\n\nInstall pipx first:\n  mise use pipx@latest"
                )
            };
            self.warn_if_dependency_missing(&ctx.config, "pipx", &["pipx"], &instructions)
                .await;

            // Fail with the instructions above rather than letting `pipx install` die with a
            // bare "No such file or directory (os error 2)". The install graph has already
            // processed configured dependencies before this backend starts, so a configured
            // but still unspawnable pipx will not become available later in this invocation.
            //
            // The gate asks `spawnable_dependency`, the same question `spawn_program` asks
            // below, so it cannot pass on evidence the spawn will then reject. On Windows a
            // `pipx.ps1` or a shebang-only `pipx.pyz` satisfies the plain lookup but cannot
            // be launched, and it used to reach `pipx install` and die with
            // "program not found" instead of these instructions.
            if !pipx_available {
                let pipx_configured = match self.dependency_toolset(&ctx.config).await {
                    Ok(ts) => ts.versions.keys().any(|ba| ba.short == "pipx"),
                    Err(_) => false,
                };
                let reason = if pipx_configured {
                    "pipx is configured but its executable was not found or is not runnable"
                        .to_string()
                } else {
                    format!(
                        "pipx is required to install {} but was not found",
                        self.ba()
                    )
                };
                bail!(
                    "{reason}.\n\n{instructions}\n\nIf pipx is already installed, verify it with `mise which pipx` and `pipx --version`."
                );
            }
        }

        let request = self.tool_name().parse::<PipxRequest>()?;

        if let Some(uv_program) = uv_program {
            let package_request = request.uvx_request(&tv.version, &options);
            self.ensure_uv_supports_expose(ctx, &options).await?;
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
            cmd = cmd.args(options.uv_install_args()?);
            let user_args = match options.uvx_args() {
                Some(args) => shell_words::split(args)?,
                None => vec![],
            };
            // uv prefers a Python it downloaded earlier over the one on PATH, which would
            // tie the tool's venv to an interpreter outside the mise data dir. Bind it to
            // mise's Python so a cached data dir restores a working tool.
            if let Some((python, _)) = &tv.uv_python {
                cmd = cmd.arg("--python").arg(python);
            } else if !uv_args_select_python(&user_args)
                && let Some(python) = self.mise_managed_python(&ctx.config, &ctx.ts).await
            {
                cmd = cmd.arg("--python").arg(python);
            }
            cmd = cmd.args(user_args);
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
            if let Some((python, _)) = &tv.uv_python {
                cmd = cmd.arg("--python").arg(python);
            }
            if let Some(args) = options.pipx_args() {
                cmd = cmd.args(shell_words::split(args)?);
            }
            if let Err(err) = cmd.execute() {
                let not_found = err.chain().any(|cause| {
                    cause
                        .downcast_ref::<std::io::Error>()
                        .is_some_and(|io| io.kind() == std::io::ErrorKind::NotFound)
                });
                if not_found {
                    bail!(
                        "pipx was found during dependency validation but could not be launched while installing {}.\n\nVerify the configured executable with `mise which pipx` and `pipx --version`. If pipx is unavailable, reinstall it with `mise use pipx@latest`.",
                        self.ba()
                    );
                }
                return Err(err);
            }
        }

        // A selector also gives version-only installs an immutable runtime
        // identity. Keep the provenance beside the environment so future
        // resolution can verify that a matching directory was built with the
        // same interpreter rather than adopting an old legacy install.
        if let Some(python) = &tv.uv_python {
            let metadata = tv.install_path().join(".mise-uv");
            crate::file::create_dir_all(&metadata)?;
            crate::file::write(metadata.join("python.json"), serde_json::to_string(python)?)?;
        }

        // Legacy environments retain their mutable-minor compatibility link.
        // Selector-owned environments are instead named by a concrete runtime
        // identity and must never be rebound by a later Python patch upgrade.
        if options.selected_python()?.is_none() {
            fix_venv_python_symlink(&tv.install_path())?;
        }

        Ok(tv)
    }

    fn resolve_lockfile_options(
        &self,
        request: &ToolRequest,
        _target: &PlatformTarget,
    ) -> Result<BTreeMap<String, String>> {
        let opts = request.options();
        PipxOptions::new(&opts).lockfile_options()
    }

    /// `pypi.uvx = false` turns uv off for every package, exactly as a per-package
    /// `uvx = false` does, and uv and pipx lay an environment out differently. Both
    /// spellings land on the same `uvx` key so they name the same install.
    ///
    /// `pypi.registry_url` is the index packages are installed from (see
    /// [`Self::get_index_url`]), where the same name and version can be different
    /// code, so another index makes another installation.
    fn install_identity_options(&self, tv: &ToolVersion) -> BTreeMap<String, String> {
        let mut options = super::static_helpers::request_identity_options(self, tv);
        let settings = Settings::get();
        if settings.pypi.uvx == Some(false) {
            options.insert("uvx".to_string(), "false".to_string());
        }
        if let Some(registry) = &settings.pypi.registry_url
            && registry != DEFAULT_REGISTRY_URL
        {
            options.insert("pypi.registry".to_string(), registry.clone());
        }
        options
    }
}

/// The `pypi.registry_url` default: PyPI itself.
const DEFAULT_REGISTRY_URL: &str = "https://pypi.org/pypi/{}/json";

/// Returns install-time-only option keys for PIPX backend.
pub(crate) fn install_time_option_keys() -> Vec<String> {
    vec![
        "extras".into(),
        "with".into(),
        "expose".into(),
        "dependency_prereleases".into(),
        "package_name".into(),
        "pipx_args".into(),
        "uvx_args".into(),
        "uvx".into(),
        "python".into(),
    ]
}

impl PIPXBackend {
    /// Resolves a Git remote's default-branch HEAD to a concrete commit SHA.
    async fn git_head(remote: &str) -> Result<String> {
        timeout::run_with_timeout_async(
            async || {
                let output = crate::cmd::cmd_read_async_inherited_env(
                    "git",
                    &["ls-remote", remote, "HEAD"],
                    std::iter::empty::<(&str, &std::ffi::OsStr)>(),
                )
                .await?;
                Self::git_head_from_ls_remote(&output)
                    .ok_or_else(|| eyre!("no HEAD found for {remote}"))
            },
            Settings::get().fetch_remote_versions_timeout(),
        )
        .await
    }

    /// Parses the concrete HEAD commit from `git ls-remote <url> HEAD` output.
    fn git_head_from_ls_remote(output: &str) -> Option<String> {
        output.lines().find_map(|line| {
            let (sha, git_ref) = line.split_once('\t')?;
            (git_ref == "HEAD").then(|| sha.to_string())
        })
    }

    /// Lists tags from a Git remote while preserving Git's source ordering.
    async fn versions_from_git_tags(remote: &str) -> Result<Vec<VersionInfo>> {
        timeout::run_with_timeout_async(
            async || {
                let output = crate::cmd::cmd_read_async_inherited_env(
                    "git",
                    &["ls-remote", "--tags", "--refs", remote],
                    std::iter::empty::<(&str, &std::ffi::OsStr)>(),
                )
                .await?;
                Ok(Self::versions_from_git_ls_remote(&output))
            },
            Settings::get().fetch_remote_versions_timeout(),
        )
        .await
    }

    /// Parses `git ls-remote --tags --refs` output into opaque version candidates.
    fn versions_from_git_ls_remote(output: &str) -> Vec<VersionInfo> {
        output
            .lines()
            .filter_map(|line| line.split_once('\t'))
            .filter_map(|(_, git_ref)| git_ref.strip_prefix("refs/tags/"))
            .filter(|tag| !tag.is_empty())
            .unique()
            .map(|version| VersionInfo {
                version: version.to_string(),
                ..Default::default()
            })
            .collect()
    }

    /// Lists versions from a PEP 503/691 simple index. Asks for the PEP 691 JSON
    /// form so PEP 700's `upload-time` is available for `minimum_release_age`,
    /// and falls back to parsing HTML for indexes that only serve PEP 503.
    async fn fetch_simple_index(package: &str, url: String) -> Result<Vec<VersionInfo>> {
        let mut headers = HeaderMap::new();
        headers.insert(
            ACCEPT,
            HeaderValue::from_static(
                "application/vnd.pypi.simple.v1+json, application/vnd.pypi.simple.v1+html;q=0.2, text/html;q=0.01",
            ),
        );
        let url = reqwest::Url::parse(&url)?;
        let headers = crate::http::with_host_auth(&url, &headers)?;
        let resp = HTTP_FETCH.get_async_with_headers(url, &headers).await?;
        let content_type = resp
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|v| v.split(';').next().unwrap_or(v).trim().to_ascii_lowercase())
            .unwrap_or_default();
        let versions = if content_type.ends_with("+json") || content_type == "application/json" {
            Self::versions_from_simple_json(package, resp.json().await?)
        } else if content_type == "text/html" || content_type.ends_with("+html") {
            // PEP-0503 (HTML format doesn't include timestamps)
            let html = resp.text().await?;
            Self::versions_from_simple_index(package, &html)
                .into_iter()
                .map(|version| VersionInfo {
                    version,
                    ..Default::default()
                })
                .collect()
        } else {
            bail!("Got unexpected content type {content_type:?} from simple index for {package}");
        };
        Ok(versions
            .into_iter()
            .sorted_by_cached_key(|v| Versioning::new(&v.version))
            .collect())
    }

    fn versions_from_simple_json(package: &str, index: SimpleIndex) -> Vec<VersionInfo> {
        let mut versions: IndexMap<String, Option<String>> = IndexMap::new();
        for file in index.files.into_iter().filter(|f| !f.yanked) {
            let Some(version) = Self::version_from_distribution_filename(package, &file.filename)
            else {
                continue;
            };
            let entry = versions.entry(version).or_default();
            // Keep the earliest upload as a parsed instant, since RFC3339 strings
            // with different offsets don't sort chronologically.
            let Some(new) = file.upload_time else {
                continue;
            };
            let is_earlier = entry.as_deref().is_none_or(|cur| {
                parse_into_timestamp(&new).unwrap_or(Timestamp::MAX)
                    < parse_into_timestamp(cur).unwrap_or(Timestamp::MAX)
            });
            if is_earlier {
                *entry = Some(new);
            }
        }
        versions
            .into_iter()
            .map(|(version, created_at)| VersionInfo {
                version,
                created_at,
                ..Default::default()
            })
            .collect()
    }

    fn versions_from_simple_index(package: &str, html: &str) -> Vec<String> {
        let href_re = regex!(r#"(?i)href\s*=\s*["']([^"']+)["']"#);

        href_re
            .captures_iter(html)
            .filter_map(|cap| {
                let href = cap.get(1)?.as_str();
                let filename = Self::distribution_filename_from_url(href)?;

                Self::version_from_distribution_filename(package, &filename)
            })
            .unique()
            .collect()
    }

    fn distribution_filename_from_url(href: &str) -> Option<String> {
        let path = href.split(['?', '#']).next()?;
        let filename = path.rsplit('/').next()?;
        Some(urlencoding::decode(filename).ok()?.into_owned())
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

    /// The newest release that is not a PEP 440 pre-release, with its date.
    fn latest_stable(versions: Vec<VersionInfo>) -> Option<VersionInfo> {
        versions
            .into_iter()
            .filter(|v| !is_python_prerelease(&v.version))
            .sorted_by_cached_key(|v| Versioning::new(&v.version))
            .next_back()
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
                    // Carry GitHub's authoritative flag so the PEP 440 pattern
                    // below cannot reclassify a release explicitly published
                    // as stable. The only current caller feeds
                    // `github::list_releases`, which pre-filters prereleases,
                    // so this is always Some(false) today — but copying the
                    // flag stays correct for unfiltered inputs.
                    prerelease: Some(r.prerelease),
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

    pub(crate) fn from_arg(ba: BackendArg) -> Self {
        Self { ba: Arc::new(ba) }
    }

    fn latest_version_cache(&self, registry_url: &str) -> CacheManager<Option<VersionInfo>> {
        let registry_hash = hash_to_str(&registry_url);
        CacheManagerBuilder::new(
            self.ba
                .cache_path()
                .join(format!("latest_version_info_{registry_hash}.msgpack.z")),
        )
        .with_fresh_duration(Settings::get().fetch_remote_versions_cache())
        .build()
    }

    fn get_index_url() -> eyre::Result<String> {
        let registry_url = Settings::get()
            .pypi
            .registry_url
            .clone()
            .unwrap_or_else(|| DEFAULT_REGISTRY_URL.to_string());

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
            .unwrap_or_else(|| {
                Settings::get()
                    .pypi
                    .registry_url
                    .clone()
                    .unwrap_or_else(|| "https://pypi.org/pypi/{}/json".to_string())
            });

        debug!("Pipx registry URL: {}", registry_url);

        let re = Regex::new(r"^(http|https)://.*\{\}.*$").unwrap();

        if !re.is_match(&registry_url) {
            return Err(eyre!(
                "Registry URL must be a valid URL and contain a {{}} placeholder"
            ));
        }

        Ok(registry_url)
    }

    pub async fn reinstall_all(
        config: &Arc<Config>,
        invocation_locked: bool,
        use_locked_version: bool,
    ) -> Result<()> {
        let ts = Arc::new(ToolsetBuilder::new().build(config).await?);
        let pipx_tools = ts
            .list_installed_versions(config)
            .await?
            .into_iter()
            .filter(|(b, _tv)| b.ba().backend_type() == BackendType::Pipx)
            .collect_vec();
        for (b, tv) in pipx_tools {
            let locked = config.invocation_locked_for(tv.request.source(), invocation_locked)
                || tv.request.tool_config_locked(config, use_locked_version);
            let ctx = InstallContext {
                config: config.clone(),
                ts: ts.clone(),
                pr: MultiProgressReport::get()
                    .add(&format!("reinstalling {}", tv.style()))
                    .into(),
                force: true,
                dry_run: false,
                explicit_yes: false,
                locked,
                before_date: None,
                dependency_context: Default::default(),
            };
            b.install_version(ctx, tv).await?;
        }
        Ok(())
    }

    /// The Python mise installed for this tool's toolset, ignoring any interpreter that is
    /// only on `PATH`.
    async fn mise_managed_python(&self, config: &Arc<Config>, ts: &Toolset) -> Option<PathBuf> {
        if let Some(python) = ts.which_bin_spawnable(config, "python").await {
            return Some(python);
        }
        self.dependency_toolset(config)
            .await
            .ok()?
            .which_bin_spawnable(config, "python")
            .await
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
                "minimum_release_age is set for {} but could not determine uv version required to verify --exclude-newer support. Release-age filtering for transitive dependencies may not work as expected. See https://mise.jdx.dev/dev-tools/backends/pypi.html",
                self.ba.short,
            );
            return;
        };

        if semver_is_older_than(&version, UV_EXCLUDE_NEWER_VERSION).unwrap_or(false) {
            warn!(
                "minimum_release_age is set for {} but uv@{} is older than the documented minimum uv@{} required for --exclude-newer. Older versions may fail while processing the forwarded argument. See https://mise.jdx.dev/dev-tools/backends/pypi.html",
                self.ba.short, version, UV_EXCLUDE_NEWER_VERSION,
            );
        }
    }

    async fn ensure_uv_supports_expose(
        &self,
        ctx: &InstallContext,
        options: &PipxOptions<'_>,
    ) -> Result<()> {
        if options.expose()?.is_empty() {
            return Ok(());
        }
        let Some(version) =
            crate::backend::semver_version_from_toolsets_or_path(self, &ctx.config, &ctx.ts, "uv")
                .await
        else {
            bail!(
                "{} expose requires uv@{} or newer, but the installed uv version could not be determined",
                self.ba.short,
                UV_WITH_EXECUTABLES_FROM_VERSION
            );
        };
        if semver_is_older_than(&version, UV_WITH_EXECUTABLES_FROM_VERSION).unwrap_or(false) {
            bail!(
                "{} expose requires uv@{} or newer, but uv@{} is installed",
                self.ba.short,
                UV_WITH_EXECUTABLES_FROM_VERSION,
                version
            );
        }
        Ok(())
    }
}

#[derive(Debug, PartialEq)]
enum PipxRequest {
    /// git+https://github.com/psf/black.git@24.2.0
    /// git+https://github.com/o/monorepo#subdirectory=cli
    /// psf/black@24.2.0
    Git(GitSource),
    /// black@24.2.0
    Pypi(String),
}

/// Whether user-supplied `uv tool install` arguments already pick an interpreter.
fn uv_args_select_python(args: &[String]) -> bool {
    args.iter().any(|a| {
        a == "-p"
            || a == "--python"
            || a.starts_with("--python=")
            || (a.starts_with("-p") && !a.starts_with("--") && a.len() > 2)
    })
}

/// A Git source split into its repository and any pip/uv URL fragment
/// (`#subdirectory=cli`, `#egg=name`). Only `repo` is used to discover
/// versions; the fragment is only applied to install requests.
#[derive(Debug, PartialEq)]
struct GitSource {
    /// Repository URL without `git+`, a trailing `.git`, or the fragment.
    repo: String,
    /// Whether the remote is spelled with `.git`. Kept as written because not
    /// every remote accepts both spellings (e.g. `git+file:///path/to/repo`).
    git_suffix: bool,
    /// Fragment items (`key=value`) in their original order and spelling.
    fragment: Vec<String>,
}

impl GitSource {
    fn new(repo: &str, fragment: Option<&str>) -> Self {
        let stripped = repo.strip_suffix(".git");
        Self {
            repo: stripped.unwrap_or(repo).to_string(),
            git_suffix: stripped.is_some(),
            fragment: fragment
                .into_iter()
                .flat_map(|f| f.split('&'))
                .filter(|item| !item.is_empty())
                .map(str::to_string)
                .collect(),
        }
    }

    fn fragment_key(item: &str) -> &str {
        item.split_once('=').map_or(item, |(key, _)| key)
    }

    fn fragment_value(&self, key: &str) -> Option<&str> {
        self.fragment.iter().find_map(|item| {
            item.split_once('=')
                .filter(|(k, _)| *k == key)
                .map(|(_, value)| value)
        })
    }

    /// The remote as written, for `git ls-remote` and install requests.
    fn remote(&self) -> String {
        if self.git_suffix {
            format!("{}.git", self.repo)
        } else {
            self.repo.clone()
        }
    }

    /// pip and uv expect the ref before the fragment:
    /// `git+<remote>@<ref>#<fragment>`.
    fn url(&self, v: &str, fragment: &[String]) -> String {
        let mut url = format!("git+{}", self.remote());
        if v != "latest" {
            url.push('@');
            url.push_str(v);
        }
        if !fragment.is_empty() {
            url.push('#');
            url.push_str(&fragment.join("&"));
        }
        url
    }

    /// The distribution name used to attach extras. In a monorepo the
    /// subdirectory is a better guess than the repository name.
    fn package_name<'a>(&'a self, opts: &PipxOptions<'a>) -> &'a str {
        let last_segment = |path: &'a str| path.trim_end_matches('/').rsplit('/').next();
        let non_empty = |name: &&str| !name.is_empty();
        opts.package_name()
            .or_else(|| {
                self.fragment_value("egg")
                    .and_then(|egg| egg.split('[').next())
                    .filter(non_empty)
            })
            .or_else(|| {
                self.fragment_value("subdirectory")
                    .and_then(last_segment)
                    .filter(non_empty)
            })
            .unwrap_or_else(|| last_segment(&self.repo).unwrap_or(&self.repo))
    }
}

impl PipxRequest {
    /// Returns the `owner/repo` path for GitHub remotes, regardless of the Git
    /// transport used to reach GitHub.
    fn github_repo(url: &str) -> Option<String> {
        let url = url::Url::parse(url).ok()?;
        if url.host_str() != Some("github.com") {
            return None;
        }
        let repo = url.path().trim_matches('/').trim_end_matches(".git");
        (!repo.is_empty()).then(|| repo.to_string())
    }

    fn extras_from_opts(&self, opts: &PipxOptions<'_>) -> String {
        match opts.extras() {
            Some(extras) => format!("[{extras}]"),
            None => String::new(),
        }
    }

    fn uvx_request(&self, v: &str, opts: &PipxOptions<'_>) -> String {
        let extras = self.extras_from_opts(opts);

        match self {
            PipxRequest::Git(source) => {
                let git_url = source.url(v, &source.fragment);
                if extras.is_empty() {
                    git_url
                } else {
                    let package = source.package_name(opts);
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
            PipxRequest::Git(source) => {
                if extras.is_empty() {
                    return source.url(v, &source.fragment);
                }
                // pipx ignored extras on PEP 508 URL requirements before 0.15.6.0.
                // Its VCS `egg` form works across the supported pipx version range.
                // Merge it into the existing fragment so the URL keeps one `#`.
                let egg = format!("egg={}{extras}", source.package_name(opts));
                let mut fragment = source.fragment.clone();
                match fragment
                    .iter_mut()
                    .find(|item| GitSource::fragment_key(item) == "egg")
                {
                    Some(item) => *item = egg,
                    None => fragment.push(egg),
                }
                source.url(v, &fragment)
            }
            PipxRequest::Pypi(package) if v == "latest" => format!("{package}{extras}"),
            PipxRequest::Pypi(package) => format!("{package}{extras}=={v}"),
        }
    }
}

/// PEP 691 project detail, with PEP 700's `upload-time`.
#[derive(serde::Deserialize)]
struct SimpleIndex {
    #[serde(default)]
    files: Vec<SimpleIndexFile>,
}

#[derive(serde::Deserialize)]
struct SimpleIndexFile {
    filename: String,
    #[serde(rename = "upload-time")]
    upload_time: Option<String>,
    #[serde(default, deserialize_with = "deserialize_pypi_yanked")]
    yanked: bool,
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
        // pip/uv VCS URLs may carry a fragment such as `#subdirectory=cli`; the
        // `.git` suffix is optional (`git+https://github.com/o/r#subdirectory=cli`).
        let (source, fragment) = match s.split_once('#') {
            Some((source, fragment)) => (source, Some(fragment)),
            None => (s, None),
        };
        if let Some(url) = source.strip_prefix("git+")
            && (url.contains("://") || url.ends_with(".git"))
        {
            Ok(PipxRequest::Git(GitSource::new(url, fragment)))
        } else if source.contains('/') {
            // Shorthand has always been requested with a `.git` suffix.
            let repo = source.strip_suffix(".git").unwrap_or(source);
            Ok(PipxRequest::Git(GitSource::new(
                &format!("https://github.com/{repo}.git"),
                fragment,
            )))
        } else {
            Ok(PipxRequest::Pypi(s.to_string()))
        }
    }
}

/// Check if a path is within mise's Python installs directory
#[cfg(unix)]
fn is_mise_managed_python(installs_dir: &Path, path: &Path) -> bool {
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
fn ensure_minor_version_symlink(installs_dir: &Path, full_version_path: &Path) -> Result<()> {
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
fn fix_venv_python_symlink(install_path: &Path) -> Result<()> {
    fix_venv_python_symlink_in(&env::MISE_INSTALLS_DIR, install_path)
}

/// [`fix_venv_python_symlink`] for the installs root `installs_dir`.
#[cfg(unix)]
fn fix_venv_python_symlink_in(installs_dir: &Path, install_path: &Path) -> Result<()> {
    // uv and pipx name the venv after the Python distribution, which the tool
    // name doesn't reliably give (`o/repo#subdirectory=cli` builds `cli-dist`).
    // Each install path holds one tool, so check every venv under it:
    // {dist}/ for uvx, venvs/{dist}/ for pipx.
    let mut venv_dirs = vec![];
    for base in [install_path.to_path_buf(), install_path.join("venvs")] {
        for name in file::dir_subdirs(&base)? {
            venv_dirs.push(base.join(name));
        }
    }

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

            // Under the identity layout an interpreter that was found through its own
            // directory (a locked request, or a version link that names another variant) is
            // recorded by its hashed directory. Name it through the version link instead,
            // which is what the minor version alias below is built on.
            let target =
                crate::backend::static_helpers::tool_link_path(installs_dir, &target, "python")
                    .unwrap_or(target);

            if !is_mise_managed_python(installs_dir, &target) {
                continue; // Leave non-mise Python alone (homebrew, uv, etc.)
            }

            if let Some(minor_path) = path_with_minor_version(&target)
                && target.exists()
            {
                // Create the minor version symlink (e.g., python/3.12 -> python/3.12.1)
                // if it doesn't exist yet. This is normally done by runtime_symlinks::rebuild,
                // but that runs after postinstall hooks, so we need to create it now
                // to ensure the venv symlink works immediately for postinstall hooks.
                ensure_minor_version_symlink(installs_dir, &target)?;

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
fn fix_venv_python_symlink(_install_path: &Path) -> Result<()> {
    Ok(())
}

/// Load a cached latest release that is past `fetch_remote_versions_cache` but
/// within the release-age window of the cutoff `before`, when it is dated, and
/// say whether it did.
///
/// It is the newest stable release in the registry's list, not a tag a
/// maintainer can move. It changes when a newer version is uploaded, which the
/// cutoff hides for the window, and resolution then takes the newest release
/// past the cutoff, which is this one again when it qualifies. Or when it is
/// yanked, which a version list cached for the same window shows as late.
/// Without a date, a new upload would pass the cutoff at once.
fn adopt_latest_within_release_age(
    cache: &mut CacheManager<Option<VersionInfo>>,
    before: Option<Timestamp>,
) -> bool {
    if cache.has_fresh_value() {
        return false;
    }
    let Some(window) = before.and_then(release_age_list_window_for_cutoff) else {
        return false;
    };
    cache.adopt_file_if_fresh_for(Some(window), |_, latest| {
        latest
            .as_ref()
            .and_then(|latest| latest.created_at.as_deref())
            .is_some_and(|created_at| parse_into_timestamp(created_at).is_ok())
    })
}

/// PyPI versions follow PEP 440. Stamp the separator-less alpha/beta/rc
/// suffixes (`3.12.0a1`, `1.0.0c1`) here rather than in the shared regex so
/// the rule stays scoped to Python — hex commit hashes used by other
/// ecosystems (e.g. Go pseudo-versions) would false-positive. This replaces
/// the generic `mark_prerelease`, so the shared channel tags are checked here
/// too, against the public version only (`1.1+gpu.dev0` is stable). Only
/// fills in unknowns: an authoritative flag from a GitHub release (either
/// value) wins over pattern detection.
fn stamp_pep440_prerelease(mut version: VersionInfo) -> VersionInfo {
    if version.prerelease.is_none() && is_python_prerelease(&version.version) {
        version.prerelease = Some(true);
    }
    version
}

#[cfg(test)]
mod tests {
    use super::{
        GitSource, PIPXBackend, PipxOptions, PipxRequest, PypiPackage, PypiRelease, SelectedPython,
        UV_EXCLUDE_NEWER_VERSION, uv_args_select_python,
    };
    use crate::args::BackendArg;
    use crate::backend::{Backend, VersionInfo};
    use crate::config::Config;
    #[cfg(unix)]
    use crate::file;
    use crate::github::GithubRelease;
    use crate::install_context::InstallContext;
    use crate::toolset::tool_request::LockfileScope;
    use crate::toolset::{
        ToolRequest, ToolSource, ToolVersion, ToolVersionOptions, Toolset, parse_tool_options,
    };
    use crate::ui::progress_report::QuietReport;
    use async_trait::async_trait;
    use std::path::PathBuf;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use tempfile::tempdir;

    #[test]
    fn selected_python_accepts_numeric_minor_and_exact_patch() {
        assert_eq!(
            SelectedPython::parse("3.12").unwrap().requires_python(),
            ">=3.12,<3.13"
        );
        assert_eq!(
            SelectedPython::parse("3.12.12").unwrap().requires_python(),
            "==3.12.12"
        );
    }

    #[test]
    fn selected_python_rejects_nonportable_selectors() {
        for selector in ["latest", "3.12.*", "cpython@3.12", "/usr/bin/python3"] {
            assert!(SelectedPython::parse(selector).is_err(), "{selector}");
        }
    }

    #[test]
    fn selected_python_dependency_keeps_its_lockfile_scope() {
        let owner = ToolSource::MiseToml("/project/mise.toml".into());
        let mut request = ToolRequest::new_with_options(
            Arc::new(BackendArg::from("pypi:demo")),
            "latest",
            parse_tool_options(r#"python = "3.12""#),
            ToolSource::Argument,
        )
        .unwrap();
        request.set_lockfile_scope(LockfileScope::Source(owner.clone()));

        let dependency = super::selected_python_dependency(&request)
            .unwrap()
            .unwrap();
        assert_eq!(dependency.lockfile_scope(), &LockfileScope::Source(owner));
    }

    #[test]
    fn selected_python_rejects_installer_python_arguments() {
        for (key, value) in [
            ("uvx_args", "--python 3.13"),
            ("uvx_args", "-p3.13"),
            ("pipx_args", "-p=python3"),
            ("pipx_args", "-p python3"),
        ] {
            let options = parse_tool_options(&format!("python = \"3.12\", {key} = \"{value}\""));
            assert!(
                PipxOptions::new(&options).validate_semantic().is_err(),
                "{key}"
            );
        }
    }

    fn selected_python_tool_version(identity: &str) -> ToolVersion {
        selected_python_tool_version_for("pypi:demo", identity)
    }

    fn selected_python_tool_version_for(backend: &str, identity: &str) -> ToolVersion {
        let request = ToolRequest::new_with_options(
            Arc::new(BackendArg::from(backend)),
            "1.0.0",
            parse_tool_options(r#"python = "3.13""#),
            ToolSource::Unknown,
        )
        .unwrap();
        let mut tv = ToolVersion::new(request, "1.0.0".into());
        tv.uv_python = Some((
            PathBuf::from("/data/installs/python/3.13.0/bin/python"),
            identity.into(),
        ));
        tv
    }

    fn write_python_receipt(path: &std::path::Path, identity: &str) {
        std::fs::create_dir_all(path.join(".mise-uv")).unwrap();
        std::fs::write(
            path.join(".mise-uv/python.json"),
            serde_json::to_string(&(PathBuf::from("/data/installs/python/bin/python"), identity))
                .unwrap(),
        )
        .unwrap();
    }

    #[derive(Debug)]
    struct FailingInstallBackend {
        ba: Arc<BackendArg>,
        attempts: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl Backend for FailingInstallBackend {
        fn ba(&self) -> &Arc<BackendArg> {
            &self.ba
        }

        async fn _list_remote_versions(
            &self,
            _config: &Arc<Config>,
        ) -> eyre::Result<Vec<VersionInfo>> {
            Ok(vec![])
        }

        async fn install_version_(
            &self,
            _ctx: &InstallContext,
            _tv: ToolVersion,
        ) -> eyre::Result<ToolVersion> {
            self.attempts.fetch_add(1, Ordering::SeqCst);
            eyre::bail!("controlled installer failure")
        }
    }

    fn lifecycle_context(config: Arc<Config>) -> InstallContext {
        InstallContext {
            config,
            ts: Arc::new(Toolset::default()),
            pr: Arc::new(QuietReport::new()),
            force: false,
            dry_run: false,
            explicit_yes: true,
            locked: false,
            before_date: None,
            dependency_context: Default::default(),
        }
    }

    #[test]
    fn selected_python_same_identity_keeps_discovered_environment() {
        let temp = tempdir().unwrap();
        let mut tv = selected_python_tool_version("core:python:3.13.0:same");
        let old = temp.path().join(tv.tv_pathname());
        write_python_receipt(&old, "core:python:3.13.0:same");
        tv.install_path = Some(old.clone());

        super::reset_selected_python_install_path(&mut tv).unwrap();

        assert_eq!(tv.install_path.as_deref(), Some(old.as_path()));
    }

    #[test]
    fn selected_python_missing_receipt_fails_without_removing_environment() {
        let temp = tempdir().unwrap();
        let mut tv = selected_python_tool_version("core:python:3.13.0:new");
        let old = temp.path().join(tv.tv_pathname());
        std::fs::create_dir_all(&old).unwrap();
        tv.install_path = Some(old.clone());

        assert!(super::reset_selected_python_install_path(&mut tv).is_err());

        assert_eq!(tv.install_path.as_deref(), Some(old.as_path()));
        assert!(old.is_dir());
    }

    #[tokio::test]
    async fn selected_python_replacement_keeps_old_environment_when_install_lifecycle_fails() {
        let config = Config::get().await.unwrap();
        let temp = tempdir().unwrap();
        // The controlled backend has no declared dependencies, so this reaches the
        // generic installation lifecycle rather than attempting a real Python install.
        let mut tv = selected_python_tool_version_for("node", "core:python:3.13.0:new");
        let old = temp.path().join("1.0.0~uv~old");
        write_python_receipt(&old, "core:python:3.12.0:old");
        tv.install_path = Some(old.clone());

        super::reset_selected_python_install_path(&mut tv).unwrap();
        assert!(tv.install_path.is_none());

        let replacement = temp.path().join(tv.tv_pathname());
        assert_ne!(replacement, old);
        tv.install_path = Some(replacement.clone());
        tv.install_path_is_explicit = true;
        let attempts = Arc::new(AtomicUsize::new(0));
        let backend = FailingInstallBackend {
            ba: tv.ba().clone().into(),
            attempts: attempts.clone(),
        };

        let error = backend
            .install_version(lifecycle_context(config), tv)
            .await
            .unwrap_err();

        assert!(error.to_string().contains("controlled installer failure"));
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
        assert!(
            replacement.exists(),
            "the controlled installer created only the new target"
        );
        assert!(old.join(".mise-uv/python.json").is_file());
    }

    #[test]
    fn selected_python_stale_same_destination_rejects_before_install_cleanup() {
        let temp = tempdir().unwrap();
        let mut tv = selected_python_tool_version("core:python:3.13.0:new");
        let old = temp.path().join(tv.tv_pathname());
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join("working-tool"), "preserve me").unwrap();
        tv.install_path = Some(old.clone());

        let error = super::reset_selected_python_install_path(&mut tv).unwrap_err();

        assert!(error.to_string().contains("cannot safely replace"));
        assert_eq!(tv.install_path.as_deref(), Some(old.as_path()));
        assert_eq!(
            std::fs::read_to_string(old.join("working-tool")).unwrap(),
            "preserve me"
        );
    }

    #[test]
    fn uv_args_detect_every_python_flag_spelling() {
        for args in [
            vec!["--python".into(), "3.12".into()],
            vec!["--python=3.12".into()],
            vec!["-p".into(), "3.12".into()],
            vec!["-p3.12".into()],
            vec!["-p=3.12".into()],
        ] {
            assert!(uv_args_select_python(&args), "{args:?}");
        }
        for args in [
            vec!["--resolution".into(), "lowest".into()],
            vec!["--prerelease".into(), "allow".into()],
            vec!["--pip-args".into(), "--pre".into()],
            vec!["--python-preference".into(), "system".into()],
        ] {
            assert!(!uv_args_select_python(&args), "{args:?}");
        }
    }
    use indexmap::IndexMap;
    use pretty_assertions::assert_eq;
    use std::ffi::OsString;

    #[test]
    fn parses_versions_from_simple_json_with_upload_times() {
        let index: super::SimpleIndex = serde_json::from_str(
            r#"{"meta":{"api-version":"1.1"},"name":"demo-pkg","files":[
              {"filename":"demo_pkg-1.0.0-py3-none-any.whl","upload-time":"2024-01-01T23:00:00-05:00","yanked":false},
              {"filename":"demo-pkg-1.0.0.tar.gz","upload-time":"2024-01-02T01:00:00+00:00"},
              {"filename":"demo-pkg-2.0.0.tar.gz","upload-time":"2024-02-01T00:00:00Z","yanked":"broken"},
              {"filename":"demo-pkg-3.0.0.tar.gz"}]}"#,
        )
        .unwrap();
        let versions = PIPXBackend::versions_from_simple_json("demo-pkg", index);
        let got = versions
            .iter()
            .map(|v| (v.version.as_str(), v.created_at.as_deref()))
            .collect::<Vec<_>>();
        assert_eq!(
            got,
            vec![
                // the sdist is earlier as an instant although it sorts later as a string
                ("1.0.0", Some("2024-01-02T01:00:00+00:00")),
                ("3.0.0", None)
            ]
        );
    }

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
    fn test_latest_stable_uses_python_prerelease_rule() {
        let versions = [
            "1.0",
            "1.1+gpu.dev0",
            "1.2.dev0",
            "1.3b1",
            "1.4-rc1",
            "1.5a",
        ]
        .map(String::from)
        .to_vec();
        assert_eq!(
            PIPXBackend::latest_stable(
                versions
                    .into_iter()
                    .map(|version| VersionInfo {
                        version,
                        ..Default::default()
                    })
                    .collect()
            )
            .map(|v| v.version)
            .as_deref(),
            Some("1.1+gpu.dev0")
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

    #[test]
    fn parses_versions_from_git_ls_remote() {
        let output = "\
aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\trefs/tags/v0.8.0\n\
bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\trefs/tags/v0.8.1\n\
cccccccccccccccccccccccccccccccccccccccc\trefs/heads/main\n";

        assert_eq!(
            PIPXBackend::versions_from_git_ls_remote(output)
                .into_iter()
                .map(|version| version.version)
                .collect::<Vec<_>>(),
            vec!["v0.8.0", "v0.8.1"]
        );
    }

    #[test]
    fn parses_git_head_from_ls_remote() {
        let output = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\tHEAD\n";

        assert_eq!(
            PIPXBackend::git_head_from_ls_remote(output).as_deref(),
            Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
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
            backend.list_remote_versions(&config).await.unwrap(),
            vec!["1.0.0", "2.0.0"]
        );
        assert_eq!(
            backend
                .latest_stable_version_info(&config)
                .await
                .unwrap()
                .map(|v| v.version)
                .as_deref(),
            Some("2.0.0")
        );
        index.assert_async().await;
    }

    #[tokio::test]
    async fn simple_index_json_preserves_upload_times() {
        use crate::backend::Backend;

        let mut server = mockito::Server::new_async().await;
        let index = server
            .mock("GET", "/simple/json-tool/")
            .match_header(
                "accept",
                mockito::Matcher::Regex("application/vnd.pypi.simple.v1\\+json".into()),
            )
            .with_status(200)
            .with_header("content-type", "application/vnd.pypi.simple.v1+json")
            .with_body(
                r#"{"meta":{"api-version":"1.1"},"name":"json-tool","files":[
                  {"filename":"json_tool-1.0.0-py3-none-any.whl","upload-time":"2024-01-01T00:00:00Z"},
                  {"filename":"json_tool-2.0.0-py3-none-any.whl","upload-time":"2024-02-01T00:00:00Z"}]}"#,
            )
            .expect(1)
            .create_async()
            .await;
        let config = crate::config::Config::get().await.unwrap();
        let backend = PIPXBackend::from_arg(
            format!(
                "pipx:json-tool[registry_url='{}/simple/{{}}/']",
                server.url()
            )
            .into(),
        );

        let versions = backend._list_remote_versions(&config).await.unwrap();
        assert_eq!(
            versions
                .iter()
                .map(|v| (v.version.as_str(), v.created_at.as_deref()))
                .collect::<Vec<_>>(),
            vec![
                ("1.0.0", Some("2024-01-01T00:00:00Z")),
                ("2.0.0", Some("2024-02-01T00:00:00Z"))
            ]
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
                .latest_stable_version_info(&config)
                .await
                .unwrap()
                .map(|v| v.version)
                .as_deref(),
            Some("1.0.0")
        );
        registry.assert_async().await;
    }

    /// Mocks a JSON registry serving `1.0.0` (January) and `2.0.0` (March)
    /// that expects `hits` requests.
    async fn dated_registry(
        server: &mut mockito::Server,
        package: &str,
        hits: usize,
    ) -> mockito::Mock {
        server
            .mock("GET", format!("/pypi/{package}/json").as_str())
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                serde_json::json!({
                    "releases": {
                        "1.0.0": [{
                            "upload_time_iso_8601": "2026-01-01T00:00:00Z",
                            "yanked": false
                        }],
                        "2.0.0": [{
                            "upload_time_iso_8601": "2026-03-01T00:00:00Z",
                            "yanked": false
                        }]
                    }
                })
                .to_string(),
            )
            .expect(hits)
            .create_async()
            .await
    }

    /// `minimum_release_age` is on by default, so resolving `latest` has to
    /// date its candidate. The registry document the shortcut reads already
    /// does, so a candidate older than the cutoff costs that one request, not
    /// a second fetch of the same document to list versions.
    #[tokio::test]
    async fn latest_older_than_cutoff_reads_registry_once() {
        let _settings = crate::test::SettingsGuard::lock();
        let mut server = mockito::Server::new_async().await;
        let registry = dated_registry(&mut server, "dated-old", 1).await;
        let config = crate::config::Config::get().await.unwrap();
        let backend = PIPXBackend::from_arg(
            format!(
                "pipx:dated-old[registry_url='{}/pypi/{{}}/json']",
                server.url()
            )
            .into(),
        );
        let _ = std::fs::remove_dir_all(backend.ba().cache_path());
        let before = crate::duration::parse_into_timestamp("2026-06-01").unwrap();

        assert_eq!(
            backend
                .latest_version(&config, Some("latest".into()), Some(before))
                .await
                .unwrap()
                .as_deref(),
            Some("2.0.0")
        );
        registry.assert_async().await;
    }

    /// A candidate newer than the cutoff still falls back to the full list,
    /// which is what picks the older release.
    #[tokio::test]
    async fn latest_newer_than_cutoff_falls_back_to_listing() {
        let _settings = crate::test::SettingsGuard::lock();
        let mut server = mockito::Server::new_async().await;
        let registry = dated_registry(&mut server, "dated-new", 2).await;
        let config = crate::config::Config::get().await.unwrap();
        let backend = PIPXBackend::from_arg(
            format!(
                "pipx:dated-new[registry_url='{}/pypi/{{}}/json']",
                server.url()
            )
            .into(),
        );
        let _ = std::fs::remove_dir_all(backend.ba().cache_path());
        let before = crate::duration::parse_into_timestamp("2026-02-01").unwrap();

        assert_eq!(
            backend
                .latest_version(&config, Some("latest".into()), Some(before))
                .await
                .unwrap()
                .as_deref(),
            Some("1.0.0")
        );
        registry.assert_async().await;
    }

    /// Set the modified time of `backend`'s cached latest releases to `secs` ago.
    fn backdate_cached_latest(backend: &PIPXBackend, secs: u64) {
        let then = filetime::FileTime::from_system_time(
            std::time::SystemTime::now() - std::time::Duration::from_secs(secs),
        );
        for entry in std::fs::read_dir(backend.ba().cache_path()).unwrap() {
            let path = entry.unwrap().path();
            if path.to_string_lossy().contains("latest_version_info_") {
                filetime::set_file_mtime(&path, then).unwrap();
            }
        }
    }

    /// Past `fetch_remote_versions_cache`, resolving `latest` under a cutoff
    /// keeps the cached, dated latest release for the cutoff's release-age
    /// window instead of fetching the registry document again, unless the user
    /// set `fetch_remote_versions_cache`.
    #[tokio::test]
    async fn latest_within_release_age_window_reads_registry_once() {
        use crate::config::settings::SettingsPartial;
        use crate::config::{Settings, SettingsExt};
        use confique::Layer;

        let _settings = crate::test::SettingsGuard::lock();
        Settings::reset(None);
        let mut server = mockito::Server::new_async().await;
        let registry = dated_registry(&mut server, "dated-window", 2).await;
        let config = crate::config::Config::get().await.unwrap();
        let tool = format!(
            "pipx:dated-window[registry_url='{}/pypi/{{}}/json']",
            server.url()
        );
        let backend = PIPXBackend::from_arg(tool.as_str().into());
        let _ = std::fs::remove_dir_all(backend.ba().cache_path());
        let before = crate::duration::parse_into_timestamp("2026-06-01").unwrap();
        let latest = async |backend: &PIPXBackend| {
            backend
                .latest_version(&config, Some("latest".into()), Some(before))
                .await
                .unwrap()
        };

        assert_eq!(latest(&backend).await.as_deref(), Some("2.0.0"));
        backdate_cached_latest(&backend, 2 * 60 * 60);
        let backend = PIPXBackend::from_arg(tool.as_str().into());
        assert_eq!(latest(&backend).await.as_deref(), Some("2.0.0"));

        let mut partial = SettingsPartial::empty();
        partial.fetch_remote_versions_cache = Some("1h".to_string());
        Settings::reset(Some(partial));
        backdate_cached_latest(&backend, 2 * 60 * 60);
        let backend = PIPXBackend::from_arg(tool.as_str().into());
        assert_eq!(latest(&backend).await.as_deref(), Some("2.0.0"));
        Settings::reset(None);
        registry.assert_async().await;
    }

    /// Only a dated latest release is kept for the window, and only under a
    /// cutoff.
    #[test]
    fn adopt_latest_within_release_age_needs_a_date_and_a_cutoff() {
        use crate::cache::{CacheManager, CacheManagerBuilder};
        use crate::config::{Settings, SettingsExt};

        let _settings = crate::test::SettingsGuard::lock();
        Settings::reset(None);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("latest.msgpack.z");
        let cache = || -> CacheManager<Option<VersionInfo>> {
            CacheManagerBuilder::new(path.clone())
                .with_fresh_duration(Some(std::time::Duration::from_secs(60 * 60)))
                .build()
        };
        let write = |created_at: Option<&str>| {
            cache()
                .write(&Some(VersionInfo {
                    version: "1.0.0".to_string(),
                    created_at: created_at.map(str::to_string),
                    ..Default::default()
                }))
                .unwrap();
            let then = filetime::FileTime::from_system_time(
                std::time::SystemTime::now() - std::time::Duration::from_secs(2 * 60 * 60),
            );
            for entry in std::fs::read_dir(dir.path()).unwrap() {
                filetime::set_file_mtime(entry.unwrap().path(), then).unwrap();
            }
        };
        let day_ago = Some(jiff::Timestamp::now() - jiff::SignedDuration::from_hours(24));
        let six_hours_ago = Some(jiff::Timestamp::now() - jiff::SignedDuration::from_hours(6));
        let ninety_minutes_ago = Some(jiff::Timestamp::now() - jiff::SignedDuration::from_mins(90));

        write(Some("2026-01-01T00:00:00Z"));
        assert!(super::adopt_latest_within_release_age(
            &mut cache(),
            day_ago
        ));
        assert!(super::adopt_latest_within_release_age(
            &mut cache(),
            six_hours_ago
        ));
        // A 90 minute cutoff keeps it for 90 minutes, not two hours.
        assert!(!super::adopt_latest_within_release_age(
            &mut cache(),
            ninety_minutes_ago
        ));
        assert!(!super::adopt_latest_within_release_age(&mut cache(), None));

        write(None);
        assert!(!super::adopt_latest_within_release_age(
            &mut cache(),
            day_ago
        ));
        write(Some("not a date"));
        assert!(!super::adopt_latest_within_release_age(
            &mut cache(),
            day_ago
        ));
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
            first_backend.list_remote_versions(&config).await.unwrap(),
            vec!["1.0.0"]
        );
        assert_eq!(
            second_backend.list_remote_versions(&config).await.unwrap(),
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
    fn non_github_latest_is_a_rolling_channel() {
        use crate::backend::Backend;

        let gitlab =
            PIPXBackend::from_arg("pipx:git+https://gitlab.example.com/sre/mytool.git".into());
        let github = PIPXBackend::from_arg("pipx:git+https://github.com/psf/black.git".into());
        let github_ssh =
            PIPXBackend::from_arg("pipx:git+ssh://git@github.com/psf/black.git".into());

        assert!(gitlab.is_rolling_channel("latest"));
        assert!(gitlab.requires_concrete_channel_version("latest"));
        assert!(!gitlab.is_rolling_channel("v0.8.1"));
        assert!(!github.is_rolling_channel("latest"));
        assert!(!github_ssh.is_rolling_channel("latest"));
        assert_eq!(
            PipxRequest::github_repo("ssh://git@github.com/psf/black"),
            Some("psf/black".to_string())
        );
    }

    #[tokio::test]
    async fn semantic_uv_options_make_uv_the_required_backend_dependency() {
        let _config = crate::config::Config::get().await.unwrap();
        let backend = PIPXBackend::from_arg("pipx:black[with=['click']]".into());
        assert_eq!(
            backend
                .get_all_dependencies(false)
                .unwrap()
                .into_iter()
                .map(|ba| ba.short)
                .collect::<Vec<_>>(),
            vec!["uv", "python"]
        );
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
                .unwrap()
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
                .unwrap()
                .get("extras"),
            Some(&"postgres,s3".to_string())
        );
    }

    #[test]
    fn test_semantic_uv_options_accept_arrays_and_build_args() {
        let mut opts = ToolVersionOptions::default();
        opts.opts.insert(
            "with".to_string(),
            toml::Value::Array(vec![
                toml::Value::String("pip".to_string()),
                toml::Value::String("plugin>=1,<2".to_string()),
            ]),
        );
        opts.opts.insert(
            "expose".to_string(),
            toml::Value::Array(vec![toml::Value::String("plugin>=1".to_string())]),
        );
        opts.opts.insert(
            "dependency_prereleases".to_string(),
            toml::Value::String("allow".to_string()),
        );
        let opts = PipxOptions::new(&opts);

        assert_eq!(
            opts.uv_install_args().unwrap(),
            [
                "--with",
                "pip",
                "--with",
                "plugin>=1,<2",
                "--with-executables-from",
                "plugin>=1",
                "--prerelease",
                "allow",
            ]
        );
        let locked = opts.lockfile_options().unwrap();
        assert_eq!(locked.get("with").unwrap(), r#"["pip","plugin>=1,<2"]"#);
        assert_eq!(locked.get("expose").unwrap(), r#"["plugin>=1"]"#);
        assert_eq!(locked.get("dependency_prereleases").unwrap(), "allow");
    }

    #[test]
    fn test_semantic_uv_options_reject_invalid_values() {
        let mut opts = ToolVersionOptions::default();
        opts.opts.insert(
            "dependency_prereleases".to_string(),
            toml::Value::String("sometimes".to_string()),
        );
        assert!(PipxOptions::new(&opts).validate_semantic().is_err());

        opts.opts.insert(
            "dependency_prereleases".to_string(),
            toml::Value::String("allow".to_string()),
        );
        opts.opts.insert(
            "with".to_string(),
            toml::Value::Array(vec![toml::Value::Integer(1)]),
        );
        assert!(PipxOptions::new(&opts).validate_semantic().is_err());
    }

    #[test]
    fn empty_semantic_uv_options_allow_pipx() {
        let mut opts = ToolVersionOptions::default();
        opts.opts
            .insert("with".to_string(), toml::Value::Array(vec![]));
        opts.opts
            .insert("expose".to_string(), toml::Value::Array(vec![]));
        opts.opts
            .insert("uvx".to_string(), toml::Value::Boolean(false));

        let opts = PipxOptions::new(&opts);
        assert!(!opts.has_uv_only_options().unwrap());
        opts.validate_semantic().unwrap();
    }

    #[test]
    fn test_install_identity_options_normalize_the_uvx_switch() {
        use crate::backend::static_helpers::test_identity_options;
        use crate::config::settings::SettingsPartial;
        use crate::config::{Settings, SettingsExt};
        use confique::Layer;

        let _settings = crate::test::SettingsGuard::lock();
        let backend = PIPXBackend::from_arg("pipx:black".into());
        let identity = |uvx_setting: Option<bool>, options: &[(&str, &str)]| {
            let mut partial = SettingsPartial::empty();
            partial.pypi.uvx = uvx_setting;
            Settings::reset(Some(partial));
            test_identity_options(&backend, "24.3.0", options)
        };

        let default = identity(None, &[]);
        assert!(default.is_empty(), "{default:?}");
        assert_eq!(identity(Some(true), &[]), default);

        // The setting and the per-package option turn uv off the same way.
        let pipx_only = identity(None, &[("uvx", "false")]);
        assert_ne!(pipx_only, default);
        assert_eq!(identity(Some(false), &[]), pipx_only);
        assert_eq!(identity(Some(false), &[("uvx", "false")]), pipx_only);

        // A package option cannot turn uv back on while the setting has it off.
        assert_eq!(identity(Some(false), &[("uvx", "true")]), pipx_only);
        Settings::reset(None);
    }

    #[test]
    fn test_install_identity_options_include_the_index() {
        use crate::backend::static_helpers::test_identity_options;
        use crate::config::settings::SettingsPartial;
        use crate::config::{Settings, SettingsExt};
        use confique::Layer;

        let _settings = crate::test::SettingsGuard::lock();
        let backend = PIPXBackend::from_arg("pipx:black".into());
        let identity = |registry: Option<&str>| {
            let mut partial = SettingsPartial::empty();
            partial.pypi.registry_url = registry.map(str::to_string);
            Settings::reset(Some(partial));
            test_identity_options(&backend, "24.3.0", &[])
        };

        let default = identity(None);
        assert!(default.is_empty(), "{default:?}");
        // Packages are installed from this index: another one is other code.
        let internal = identity(Some("https://pypi.internal.example/simple/{}/"));
        assert_ne!(internal, default);
        assert_ne!(
            internal,
            identity(Some("https://pypi.other.example/simple/{}/"))
        );
        Settings::reset(None);
    }

    /// A venv whose `bin/python3` points at `target`, under `tool_install/<venv>`.
    #[cfg(unix)]
    fn venv_pointing_at(tool_install: &std::path::Path, venv: &str, target: &std::path::Path) {
        let bin = tool_install.join(venv).join("bin");
        file::create_dir_all(&bin).unwrap();
        file::make_symlink(target, &bin.join("python3")).unwrap();
        file::make_symlink(std::path::Path::new("python3"), &bin.join("python")).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn test_venv_python_follows_the_minor_alias_in_both_layouts() {
        use super::fix_venv_python_symlink_in;
        use crate::backend::static_helpers::{test_identity_install, test_link_install};

        let tmp = tempfile::tempdir().unwrap();
        let installs = tmp.path().join("installs");
        let tool_install = tmp.path().join("tool");

        // Identity layout: the interpreter was found through its hashed directory.
        let hashed = test_identity_install(&installs, "core:python", "3.12.1", "python-aaaaaaaa");
        file::write(hashed.join("bin/python3"), "").unwrap();
        test_link_install(&installs, "python", "3.12.1", "python-aaaaaaaa");
        venv_pointing_at(&tool_install, "hashed", &hashed.join("bin/python3"));

        // Identity layout again, the interpreter found through the version link.
        venv_pointing_at(
            &tool_install,
            "linked",
            &installs.join("python/3.12.1/bin/python3"),
        );

        // Legacy layout: a real version directory.
        let legacy = installs.join("python/3.11.4");
        file::create_dir_all(legacy.join("bin")).unwrap();
        file::write(legacy.join("bin/python3"), "").unwrap();
        venv_pointing_at(&tool_install, "legacy", &legacy.join("bin/python3"));

        // Not mise's python.
        let system = tmp.path().join("usr/bin/python3");
        file::create_dir_all(system.parent().unwrap()).unwrap();
        file::write(&system, "").unwrap();
        venv_pointing_at(&tool_install, "system", &system);

        fix_venv_python_symlink_in(&installs, &tool_install).unwrap();

        let target = |venv: &str| {
            file::resolve_symlink(&tool_install.join(venv).join("bin/python3"))
                .unwrap()
                .unwrap()
        };
        assert_eq!(target("hashed"), installs.join("python/3.12/bin/python3"));
        assert_eq!(target("linked"), installs.join("python/3.12/bin/python3"));
        assert_eq!(target("legacy"), installs.join("python/3.11/bin/python3"));
        assert_eq!(target("system"), system);

        // The alias those venvs now go through is made on the spot, as before, and
        // follows the version link on to the installation.
        assert_eq!(
            file::resolve_symlink(&installs.join("python/3.12")).unwrap(),
            Some(std::path::PathBuf::from("./3.12.1"))
        );
        assert!(installs.join("python/3.12/bin/python3").exists());
        assert!(installs.join("python/3.11/bin/python3").exists());
    }

    #[test]
    fn test_git_requests_without_fragments_are_unchanged() {
        let raw = ToolVersionOptions::default();
        let opts = PipxOptions::new(&raw);
        for (tool, version, expected) in [
            (
                "git+https://github.com/psf/black.git",
                "latest",
                "git+https://github.com/psf/black.git",
            ),
            (
                "git+https://github.com/psf/black.git",
                "24.3.0",
                "git+https://github.com/psf/black.git@24.3.0",
            ),
            (
                "psf/black",
                "24.3.0",
                "git+https://github.com/psf/black.git@24.3.0",
            ),
            (
                "git+ssh://git@github.com/psf/black.git",
                "main",
                "git+ssh://git@github.com/psf/black.git@main",
            ),
            (
                "git+https://gitlab.example.com/sre/mytool.git",
                "v0.8.1",
                "git+https://gitlab.example.com/sre/mytool.git@v0.8.1",
            ),
        ] {
            let request: PipxRequest = tool.parse().unwrap();
            assert_eq!(request.uvx_request(version, &opts), expected, "{tool}");
            assert_eq!(request.pipx_request(version, &opts), expected, "{tool}");
        }
    }

    #[test]
    fn test_git_sources_parse_optional_suffix_and_fragment() {
        let ltui = "https://github.com/runpantheon/ltui";
        for (tool, repo, git_suffix, fragment) in [
            (
                "git+https://github.com/runpantheon/ltui",
                ltui,
                false,
                vec![],
            ),
            (
                "git+https://github.com/runpantheon/ltui.git",
                ltui,
                true,
                vec![],
            ),
            (
                "git+https://github.com/runpantheon/ltui#subdirectory=ltui",
                ltui,
                false,
                vec!["subdirectory=ltui"],
            ),
            (
                "git+https://github.com/runpantheon/ltui.git#subdirectory=ltui",
                ltui,
                true,
                vec!["subdirectory=ltui"],
            ),
            (
                "runpantheon/ltui#subdirectory=ltui",
                ltui,
                true,
                vec!["subdirectory=ltui"],
            ),
            ("runpantheon/ltui.git", ltui, true, vec![]),
            (
                "git+ssh://git@github.com/psf/black.git",
                "ssh://git@github.com/psf/black",
                true,
                vec![],
            ),
            (
                "git+file:///tmp/project",
                "file:///tmp/project",
                false,
                vec![],
            ),
            (
                "git+https://github.com/o/r#subdirectory=pkg&foo=bar",
                "https://github.com/o/r",
                false,
                vec!["subdirectory=pkg", "foo=bar"],
            ),
        ] {
            assert_eq!(
                tool.parse::<PipxRequest>().unwrap(),
                PipxRequest::Git(GitSource {
                    repo: repo.to_string(),
                    git_suffix,
                    fragment: fragment.into_iter().map(str::to_string).collect(),
                }),
                "{tool}"
            );
        }
        assert_eq!(
            "black".parse::<PipxRequest>().unwrap(),
            PipxRequest::Pypi("black".to_string())
        );
    }

    #[test]
    fn test_git_fragment_requests_put_ref_before_fragment() {
        let raw = ToolVersionOptions::default();
        let opts = PipxOptions::new(&raw);
        let sha = "f05b7ddaa9725baddf7e0833502795b66f9d7dd7";
        for (tool, version, expected) in [
            (
                "git+https://github.com/runpantheon/ltui#subdirectory=ltui",
                "main",
                "git+https://github.com/runpantheon/ltui@main#subdirectory=ltui".to_string(),
            ),
            (
                "runpantheon/ltui#subdirectory=ltui",
                sha,
                format!("git+https://github.com/runpantheon/ltui.git@{sha}#subdirectory=ltui"),
            ),
            (
                "git+https://github.com/runpantheon/ltui#subdirectory=ltui",
                "latest",
                "git+https://github.com/runpantheon/ltui#subdirectory=ltui".to_string(),
            ),
            (
                "git+https://github.com/o/r#subdirectory=pkg&foo=bar",
                "v1",
                "git+https://github.com/o/r@v1#subdirectory=pkg&foo=bar".to_string(),
            ),
            (
                "git+file:///tmp/project#subdirectory=pkg",
                "v1",
                "git+file:///tmp/project@v1#subdirectory=pkg".to_string(),
            ),
        ] {
            let request: PipxRequest = tool.parse().unwrap();
            assert_eq!(request.uvx_request(version, &opts), expected, "{tool}");
            assert_eq!(request.pipx_request(version, &opts), expected, "{tool}");
        }
    }

    #[test]
    fn test_git_fragment_extras_keep_one_fragment() {
        let mut raw = ToolVersionOptions::default();
        raw.opts.insert(
            "extras".to_string(),
            toml::Value::Array(vec![toml::Value::String("cli".to_string())]),
        );
        raw.opts.insert(
            "package_name".to_string(),
            toml::Value::String("mypkg".to_string()),
        );
        let opts = PipxOptions::new(&raw);

        let request: PipxRequest = "git+https://github.com/o/r#subdirectory=pkg"
            .parse()
            .unwrap();
        assert_eq!(
            request.uvx_request("v1", &opts),
            "mypkg[cli] @ git+https://github.com/o/r@v1#subdirectory=pkg"
        );
        assert_eq!(
            request.pipx_request("v1", &opts),
            "git+https://github.com/o/r@v1#subdirectory=pkg&egg=mypkg[cli]"
        );

        let request: PipxRequest = "git+https://github.com/o/r#egg=old&subdirectory=pkg"
            .parse()
            .unwrap();
        assert_eq!(
            request.pipx_request("v1", &opts),
            "git+https://github.com/o/r@v1#egg=mypkg[cli]&subdirectory=pkg"
        );
    }

    #[test]
    fn test_git_package_name_precedence() {
        let raw = ToolVersionOptions::default();
        let opts = PipxOptions::new(&raw);
        let mut named = ToolVersionOptions::default();
        named.opts.insert(
            "package_name".to_string(),
            toml::Value::String("explicit".to_string()),
        );
        let named = PipxOptions::new(&named);
        let source = |tool: &str| match tool.parse::<PipxRequest>().unwrap() {
            PipxRequest::Git(source) => source,
            PipxRequest::Pypi(_) => panic!("{tool} should be a git source"),
        };

        let egg = source("git+https://github.com/o/repo#egg=fromegg[x]&subdirectory=tools/sub");
        assert_eq!(egg.package_name(&named), "explicit");
        assert_eq!(egg.package_name(&opts), "fromegg");
        assert_eq!(
            source("git+https://github.com/o/repo#subdirectory=tools/ltui/").package_name(&opts),
            "ltui"
        );
        assert_eq!(
            source("git+https://github.com/o/repo#egg=&subdirectory=sub").package_name(&opts),
            "sub"
        );
        assert_eq!(
            source("git+https://github.com/o/repo.git").package_name(&opts),
            "repo"
        );
    }

    #[test]
    fn test_git_fragment_does_not_reach_version_discovery() {
        use crate::backend::Backend;

        let source = |tool: &str| match tool.parse::<PipxRequest>().unwrap() {
            PipxRequest::Git(source) => source,
            PipxRequest::Pypi(_) => panic!("{tool} should be a git source"),
        };
        let with_fragment = source("git+https://github.com/runpantheon/ltui#subdirectory=ltui");
        assert_eq!(
            PipxRequest::github_repo(&with_fragment.repo),
            PipxRequest::github_repo(&source("runpantheon/ltui").repo)
        );
        assert_eq!(
            PipxRequest::github_repo(&with_fragment.repo).as_deref(),
            Some("runpantheon/ltui")
        );
        assert_eq!(
            source("git+https://gitlab.example.com/sre/mytool.git#subdirectory=x").remote(),
            "https://gitlab.example.com/sre/mytool.git"
        );
        assert_eq!(
            source("git+file:///tmp/project#subdirectory=x").remote(),
            "file:///tmp/project"
        );

        for (bare, fragment) in [
            (
                "pipx:git+https://gitlab.example.com/sre/mytool.git",
                "pipx:git+https://gitlab.example.com/sre/mytool.git#subdirectory=x",
            ),
            (
                "pipx:git+https://github.com/psf/black.git",
                "pipx:git+https://github.com/psf/black#subdirectory=x",
            ),
        ] {
            let bare = PIPXBackend::from_arg(bare.into());
            let fragment = PIPXBackend::from_arg(fragment.into());
            assert_eq!(
                bare.is_rolling_channel("latest"),
                fragment.is_rolling_channel("latest")
            );
            assert_eq!(
                bare.unresolved_latest_version(),
                fragment.unresolved_latest_version()
            );
        }
    }

    #[test]
    fn test_git_extras_use_frontend_compatible_requests() {
        let mut inferred_opts = ToolVersionOptions::default();
        inferred_opts.opts.insert(
            "extras".to_string(),
            toml::Value::Array(vec![toml::Value::String("jupyter".to_string())]),
        );
        let inferred_opts = PipxOptions::new(&inferred_opts);
        let inferred_request =
            PipxRequest::Git(GitSource::new("https://github.com/psf/black.git", None));
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
        let request = PipxRequest::Git(GitSource::new(
            "https://github.com/psf/black-repository.git",
            None,
        ));

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
            named_opts.lockfile_options().unwrap().get("package_name"),
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
    fn test_latest_stable_of_pypi_package_skips_yanked_and_prerelease() {
        let version = latest_stable_of_pypi_package(pypi_package(vec![
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
            (
                "2024.5.1.123456.dev0",
                vec![pypi_release(Some("2024-05-01T00:00:00Z"), false)],
            ),
            (
                "2.0-rc1",
                vec![pypi_release(Some("2024-06-01T00:00:00Z"), false)],
            ),
        ]));

        assert_eq!(version.as_deref(), Some("1.1.0"));
    }

    #[test]
    fn test_pep440_local_label_does_not_mark_prerelease() {
        let version = latest_stable_of_pypi_package(pypi_package(vec![
            (
                "1.0.0",
                vec![pypi_release(Some("2024-01-01T00:00:00Z"), false)],
            ),
            (
                "1.1+build1dev0",
                vec![pypi_release(Some("2024-02-01T00:00:00Z"), false)],
            ),
        ]));
        assert_eq!(version.as_deref(), Some("1.1+build1dev0"));

        let stamped = super::stamp_pep440_prerelease(crate::backend::VersionInfo {
            version: "1.1+build1dev0".into(),
            ..Default::default()
        });
        assert_eq!(stamped.prerelease, None);

        // `.dev` inside the local label matches the shared channel-tag regex,
        // which must not apply to the local label either.
        let stamped = super::stamp_pep440_prerelease(crate::backend::VersionInfo {
            version: "1.1+gpu.dev0".into(),
            ..Default::default()
        });
        assert_eq!(stamped.prerelease, None);

        // The shared channel tags still apply to the public version.
        let stamped = super::stamp_pep440_prerelease(crate::backend::VersionInfo {
            version: "1.1-rc1".into(),
            ..Default::default()
        });
        assert_eq!(stamped.prerelease, Some(true));
    }

    #[test]
    fn test_pipx_stamps_prereleases_itself() {
        // The generic `mark_prerelease` would flag `1.1+gpu.dev0` from the
        // local label; pipx stamps with the PEP 440-aware rule instead.
        assert!(
            !PIPXBackend::from_arg("pipx:black".into()).mark_prereleases_from_version_pattern()
        );
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
    fn test_github_release_stable_flag_survives_pep440_looking_tag() {
        // A GitHub release explicitly published as a full release keeps its
        // authoritative Some(false) even when the tag looks like a PEP 440
        // prerelease, so the pattern stamp (None-gated) cannot reclassify it.
        let versions = PIPXBackend::versions_from_github_releases(vec![github_release(
            "1.0.0a1",
            "2024-01-01T00:00:00Z",
        )]);
        assert_eq!(versions[0].prerelease, Some(false));

        let stamped: Vec<_> = versions
            .into_iter()
            .map(super::stamp_pep440_prerelease)
            .collect();
        assert_eq!(stamped[0].prerelease, Some(false));
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

    fn latest_stable_of_pypi_package(data: PypiPackage) -> Option<String> {
        PIPXBackend::latest_stable(PIPXBackend::versions_from_pypi_package(data)).map(|v| v.version)
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
