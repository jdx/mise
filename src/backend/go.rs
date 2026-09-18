use crate::backend::Backend;
use crate::backend::VersionInfo;
use crate::backend::backend_type::BackendType;
use crate::backend::options::BackendOptions;
use crate::backend::platform_target::PlatformTarget;
use crate::cli::args::BackendArg;
use crate::cmd::CmdLineRunner;
use crate::config::Config;
use crate::config::Settings;
use crate::hash::hash_to_str;
use crate::http::HTTP_FETCH;
use crate::install_context::InstallContext;
use crate::timeout;
use crate::toolset::{ToolRequest, ToolVersion, ToolVersionOptions};
use async_trait::async_trait;
use eyre::{Result, WrapErr};
use serde_json::Deserializer;
use std::collections::{BTreeMap, HashMap};
use std::{fmt::Debug, sync::Arc};
use tokio::sync::Semaphore;
use versions::Versioning;
use xx::regex;

#[derive(Debug)]
pub(crate) struct GoBackend {
    ba: Arc<BackendArg>,
}

#[derive(Debug, Clone, Copy)]
struct GoOptions<'a> {
    values: BackendOptions<'a>,
}

impl<'a> GoOptions<'a> {
    fn new(raw: &'a ToolVersionOptions) -> Self {
        Self {
            values: BackendOptions::new(raw),
        }
    }

    fn tags(&self) -> Option<String> {
        self.values.comma_joined("tags")
    }

    fn lockfile_options(&self) -> BTreeMap<String, String> {
        let mut result = BTreeMap::new();
        if let Some(value) = self.tags() {
            result.insert("tags".to_string(), value);
        }
        result
    }
}

#[async_trait]
impl Backend for GoBackend {
    fn get_type(&self) -> BackendType {
        BackendType::Go
    }

    fn ba(&self) -> &Arc<BackendArg> {
        &self.ba
    }

    fn get_dependencies(&self) -> eyre::Result<Vec<&str>> {
        Ok(vec!["go"])
    }

    fn supports_lockfile_url(&self) -> bool {
        false
    }

    fn mark_prereleases_from_version_pattern(&self) -> bool {
        true
    }

    async fn remote_version_cache_context(&self, config: &Arc<Config>) -> Result<Option<String>> {
        let env = self.dependency_env(config).await?;
        Ok(Some(go_routing_cache_context(&env)))
    }

    /// List Go module versions through the configured proxy or Go's module command.
    async fn _list_remote_versions(&self, config: &Arc<Config>) -> eyre::Result<Vec<VersionInfo>> {
        // Check if go is available
        self.warn_if_dependency_missing(
            config,
            "go",
            &["go"],
            "To use go packages with mise, you need to install Go first:\n\
              mise use go@latest\n\n\
            Or install Go via https://go.dev/dl/",
        )
        .await;

        timeout::run_with_timeout_async(
            async || {
                let tool_name = self.tool_name();

                if let Some(versions) = self.fetch_proxy_versions(config, &tool_name).await? {
                    return Ok(versions);
                }

                // Fall back to `go list -m -versions` for GOPROXY=direct or when
                // private-module routing is configured. Package paths are not
                // necessarily module paths, so walk their prefixes
                // just as the proxy resolver does. `go list` runs with
                // GOTOOLCHAIN=local (see go_list_env), so it is safe against
                // untrusted config and does not need a safe-mode gate.
                let mut resolution_error = None;
                for mod_path in module_path_candidates(&tool_name) {
                    match self.fetch_go_module_versions(config, &mod_path).await {
                        Ok(Some(versions)) if !versions.is_empty() => return Ok(versions),
                        Ok(_) => {}
                        Err(err) => {
                            debug!("failed to resolve Go module candidate {mod_path}: {err:#}");
                            resolution_error.get_or_insert(err);
                        }
                    }
                }
                if let Some(err) = resolution_error {
                    warn!("failed to resolve Go module path for {tool_name}: {err:#}");
                }

                Ok(vec![])
            },
            Settings::get().fetch_remote_versions_timeout(),
        )
        .await
    }

    /// Resolve `@latest` the way `go install mod@latest` does, with a single query.
    async fn latest_stable_version_info(
        &self,
        config: &Arc<Config>,
    ) -> eyre::Result<Option<VersionInfo>> {
        if Settings::get().offline() {
            trace!("Skipping latest stable Go module version due to offline mode");
            return Ok(None);
        }

        let env = self.dependency_env(config).await?;
        let tool_name = self.tool_name();
        // Ask for @latest directly instead of listing every tag and fetching
        // metadata for each one; modules with thousands of tags make that
        // listing take minutes. When private-module routing is configured, go
        // resolves it so that its own GOPRIVATE/GONOPROXY patterns apply;
        // otherwise this follows the same GOPROXY chain the listing would.
        // Package paths are not necessarily module paths, so try the path
        // prefixes from deepest to shallowest.
        let proxies = if go_native_resolution_enabled(&env) {
            vec![]
        } else {
            parse_goproxy(env.get("GOPROXY").map(String::as_str))
        };
        timeout::run_with_timeout_async(
            async || {
                if !proxies.is_empty() {
                    match self.proxy_latest_stable(&proxies, &tool_name).await {
                        LatestOutcome::Found(info) => return Ok(Some(info)),
                        LatestOutcome::Unresolved => return Ok(None),
                        // Not on the proxy, which is where a private module
                        // lands: the listing falls through to `go list` here
                        // too, so do the same instead of listing every tag.
                        LatestOutcome::NotFound => {}
                    }
                }
                Ok(match self.go_latest_stable(config, &tool_name).await {
                    LatestOutcome::Found(info) => Some(info),
                    LatestOutcome::Unresolved | LatestOutcome::NotFound => None,
                })
            },
            Settings::get().fetch_remote_versions_timeout(),
        )
        .await
    }

    /// Date one version, for the versions `_list_remote_versions` left undated.
    ///
    /// Listing caps how many versions it dates because a date can cost a VCS
    /// round trip; this pays that cost for the one version a release-age cutoff
    /// is actually deciding on.
    async fn fetch_version_created_at(
        &self,
        config: &Arc<Config>,
        version: &str,
    ) -> eyre::Result<Option<String>> {
        if Settings::get().offline() {
            trace!("Skipping Go module release date for {version} due to offline mode");
            return Ok(None);
        }
        let env = self.dependency_env(config).await?;
        let proxies = if go_native_resolution_enabled(&env) {
            vec![]
        } else {
            parse_goproxy(env.get("GOPROXY").map(String::as_str))
        };
        // Module versions carry the `v` prefix that listing strips off.
        let version = format!("v{}", version.trim_start_matches('v'));
        let tool_name = self.tool_name();
        timeout::run_with_timeout_async(
            async || {
                for mod_path in module_path_candidates(&tool_name) {
                    let metadata = if proxies.is_empty() {
                        self.fetch_go_module_version_metadata(config, &mod_path, &version)
                            .await
                    } else {
                        let endpoint =
                            format!("{}/@v/{version}.info", encode_module_path(&mod_path));
                        match query_proxy_version_metadata(&proxies, &endpoint).await {
                            ProxyVersionInfoResult::Found(info) => Some(info),
                            // A failing proxy is not a missing version, but
                            // either way there is no date to report.
                            ProxyVersionInfoResult::NotFound | ProxyVersionInfoResult::Error => {
                                None
                            }
                        }
                    };
                    if let Some(metadata) = metadata {
                        return Ok(metadata.time);
                    }
                }
                Ok(None)
            },
            Settings::get().fetch_remote_versions_timeout(),
        )
        .await
    }

    async fn resolve_exact_version(
        &self,
        _config: &Arc<Config>,
        version: &str,
    ) -> eyre::Result<Option<String>> {
        // Go module versions are strict semver, so a full semver request is
        // exact. `go install mod@vX.Y.Z` validates the version against the
        // module proxy and fails when it does not exist.
        let version = version.strip_prefix('v').unwrap_or(version);
        Ok(versions::SemVer::new(version).map(|_| version.to_string()))
    }

    async fn install_version_(
        &self,
        ctx: &InstallContext,
        tv: ToolVersion,
    ) -> eyre::Result<ToolVersion> {
        // Check if go is available
        self.warn_if_dependency_missing(
            &ctx.config,
            "go",
            &["go"],
            "To use go packages with mise, you need to install Go first:\n\
              mise use go@latest\n\n\
            Or install Go via https://go.dev/dl/",
        )
        .await;

        let install_version = tv.version.clone();

        let raw_opts = tv.request.options();
        let opts = GoOptions::new(&raw_opts);

        // Hoisted: the closure below runs twice (with and without a `v` prefix), and the
        // program does not change between attempts.
        let go = self.spawn_program(&ctx.config, Some(&ctx.ts), "go").await;

        let install = async |v| {
            let mut cmd = CmdLineRunner::new(&go).arg("install").arg("-mod=readonly");

            if let Some(tags) = opts.tags() {
                cmd = cmd.arg("-tags").arg(tags);
            }

            cmd.arg(format!("{}@{v}", self.tool_name()))
                .with_pr(ctx.pr.as_ref())
                .envs(self.dependency_env(&ctx.config).await?)
                // `go` derives GOROOT from where its own executable lives, so it
                // does not need one. An inherited GOROOT does harm: mise exports
                // one for the Go it manages and `mise activate` carries it into
                // the shell, and on unix `spawn_program` hands back the bare name
                // `go`, so which Go actually runs is up to the child's PATH. The
                // moment that is a different Go, every compile fails with
                // `compile: version "..." does not match go tool version "..."`
                // (#8261, #8877). Dropped before `install_env` so a GOROOT set
                // there deliberately still wins.
                .env_remove("GOROOT")
                .env_values(tv.install_env())
                .env("GOBIN", tv.install_path().join("bin"))
                .execute()
        };

        // try "v" prefix if the version starts with semver
        let use_v = regex!(r"^\d+\.\d+\.\d+").is_match(&install_version);

        if use_v {
            if install(format!("v{}", install_version)).await.is_err() {
                warn!("Failed to install, trying again without added 'v' prefix");
            } else {
                return Ok(tv);
            }
        }

        install(install_version).await?;

        Ok(tv)
    }

    fn resolve_lockfile_options(
        &self,
        request: &ToolRequest,
        _target: &PlatformTarget,
    ) -> Result<BTreeMap<String, String>> {
        let raw_opts = request.options();
        Ok(GoOptions::new(&raw_opts).lockfile_options())
    }
}

/// Returns install-time-only option keys for Go backend.
pub(crate) fn install_time_option_keys() -> Vec<String> {
    vec!["tags".into()]
}

const DEFAULT_GOPROXY: &str = "https://proxy.golang.org,direct";
const GO_PROXY_VERSION_INFO_CONCURRENCY: usize = 20;
const GO_LIST_VERSION_INFO_BATCH_SIZE: usize = 50;
/// How many of the newest versions get a release date from the module proxy.
///
/// A date costs one `.info` request, which these run 20 at a time, so this is
/// generous: it only exists to keep a module with thousands of tags from
/// spending a request per tag on dates nobody reads.
const GO_PROXY_VERSION_METADATA_LIMIT: usize = 100;
/// How many of the newest versions get a release date from `go list`.
///
/// Far lower, because this route reaches the module over VCS and each version
/// costs its own round trip — around half a second in practice, so dating a
/// module with hundreds of tags took minutes and usually hit the fetch timeout
/// instead of listing at all. Release dates decide which versions a
/// [`minimum_release_age`](https://mise.jdx.dev/configuration/settings.html#minimum_release_age)
/// cutoff hides, and a version without one counts as old enough to install, so
/// this has to cover the releases published inside that window — ten is ample
/// for the 24h default and still bounds the listing at a few seconds.
const GO_LIST_VERSION_METADATA_LIMIT: usize = 10;

impl GoBackend {
    pub(crate) fn from_arg(ba: BackendArg) -> Self {
        Self { ba: Arc::new(ba) }
    }

    /// Query `$GOPROXY` to find versions, matching `go install`'s resolution algorithm.
    /// Returns `None` if no proxy is configured or Go should handle private-module routing.
    async fn fetch_proxy_versions(
        &self,
        config: &Arc<Config>,
        tool_name: &str,
    ) -> eyre::Result<Option<Vec<VersionInfo>>> {
        let env = self.dependency_env(config).await?;
        if go_native_resolution_enabled(&env) {
            return Ok(None);
        }

        // Read GOPROXY from the effective mise environment. In particular, this
        // includes values supplied through [env], which are not necessarily in
        // mise's own process environment.
        let proxies = parse_goproxy(env.get("GOPROXY").map(String::as_str));
        if proxies.is_empty() {
            return Ok(None);
        }

        let candidates = module_path_candidates(tool_name);

        let mut join_set = tokio::task::JoinSet::new();
        for (idx, path) in candidates.iter().enumerate() {
            let encoded = encode_module_path(path);
            let proxies = proxies.clone();
            join_set.spawn(async move {
                let result = query_proxy_list(&proxies, &encoded).await;
                (idx, result)
            });
        }

        let mut list_results: Vec<(usize, ProxyListResult)> = Vec::new();
        while let Some(result) = join_set.join_next().await {
            match result {
                Ok(r) => list_results.push(r),
                Err(e) => warn!("proxy query task panicked: {e}"),
            }
        }
        list_results.sort_by_key(|(idx, _)| *idx);

        for (idx, result) in &list_results {
            let path = &candidates[*idx];
            match result {
                ProxyListResult::Versions(versions) if !versions.is_empty() => {
                    let mut versions: Vec<String> = versions
                        .iter()
                        .filter(|v| Versioning::new(v.trim_start_matches('v')).is_some())
                        .cloned()
                        .collect();
                    if versions.is_empty() {
                        let encoded = encode_module_path(path);
                        match query_proxy_latest(&proxies, &encoded).await {
                            ProxyVersionInfoResult::Found(info) => {
                                return Ok(Some(vec![version_info_from_metadata(info)]));
                            }
                            ProxyVersionInfoResult::NotFound => continue,
                            ProxyVersionInfoResult::Error => return Ok(None),
                        }
                    }
                    // `@v/list` is unordered, so sort before fetching metadata:
                    // only the newest versions get a `.info` request. Ordering
                    // these by semver is sound where it generally is not —
                    // https://go.dev/ref/mod#versions requires module versions
                    // to be semver, and the filter above has already dropped
                    // anything `Versioning` cannot parse, so nothing here falls
                    // back to arbitrary ordering.
                    versions.sort_by_cached_key(|v| Versioning::new(v.trim_start_matches('v')));
                    return Ok(Some(
                        fetch_proxy_version_infos(&proxies, path, &versions).await,
                    ));
                }
                ProxyListResult::Versions(_) => {
                    // Check if @latest resolves (module using pseudo-versions)
                    let encoded = encode_module_path(path);
                    match query_proxy_latest(&proxies, &encoded).await {
                        ProxyVersionInfoResult::Found(info) => {
                            return Ok(Some(vec![version_info_from_metadata(info)]));
                        }
                        ProxyVersionInfoResult::NotFound => continue,
                        ProxyVersionInfoResult::Error => return Ok(None),
                    }
                }
                ProxyListResult::NotFound => continue,
                ProxyListResult::Error => return Ok(None),
            }
        }

        Ok(None)
    }

    /// Environment for `go list` metadata queries.
    ///
    /// Adds `GOTOOLCHAIN=local` on top of the normal dependency env so that an
    /// untrusted project `go.mod` cannot make `go` download and re-exec a
    /// different toolchain (Go >= 1.21). This is what lets `go list` run under
    /// safe mode: version listing then only performs module-metadata queries
    /// (including VCS access for GOPROXY=direct) without any toolchain
    /// download-and-execute step.
    async fn go_list_env(&self, config: &Arc<Config>) -> eyre::Result<BTreeMap<String, String>> {
        let mut env = self.dependency_env(config).await?;
        env.insert("GOTOOLCHAIN".to_string(), "local".to_string());
        Ok(env)
    }

    async fn fetch_go_module_versions(
        &self,
        config: &Arc<Config>,
        mod_path: &str,
    ) -> eyre::Result<Option<Vec<VersionInfo>>> {
        let go = self.spawn_program(config, None, "go").await;
        let raw = match crate::cmd::cmd_read_async(
            &go,
            &[
                "list",
                "-mod=readonly",
                "-m",
                "-versions",
                "-json",
                mod_path,
            ],
            self.go_list_env(config).await?,
        )
        .await
        {
            Ok(raw) => raw,
            Err(err) => {
                // `_list_remote_versions` calls this once per module-path candidate, so a
                // valid setup routinely produces misses. `go` writes its own complaint to
                // stderr for each one; capturing it into the error keeps the default output
                // clean and leaves it readable under `--verbose`.
                debug!("go list -versions failed for {mod_path}: {err:#}");
                return Ok(None);
            }
        };

        let mod_info = match serde_json::from_str::<GoModInfo>(&raw) {
            Ok(info) => info,
            Err(_) => return Ok(None),
        };

        if mod_info.versions.is_empty() {
            return self.fetch_go_module_latest_info(config, mod_path).await;
        }

        let versions = self
            .fetch_go_module_version_infos(config, mod_path, &mod_info.versions)
            .await;

        Ok(Some(versions))
    }

    /// Attach release dates to the newest versions of a module.
    ///
    /// Dates are best-effort: a failed query leaves those versions undated
    /// rather than failing the listing.
    async fn fetch_go_module_version_infos(
        &self,
        config: &Arc<Config>,
        mod_path: &str,
        versions: &[String],
    ) -> Vec<VersionInfo> {
        let bare = |version: &String| VersionInfo {
            version: version.trim_start_matches('v').to_string(),
            ..Default::default()
        };
        let env = match self.go_list_env(config).await {
            Ok(env) => env,
            Err(_) => return versions.iter().map(bare).collect(),
        };

        // Resolved once for every batch below. Metadata is best-effort — a failed batch
        // leaves those versions with defaults rather than failing the listing — so without
        // this the whole enrichment step would silently no-op on a Windows go that is not
        // spawnable by bare name.
        let go = self.spawn_program(config, None, "go").await;

        // `go list -m -versions` orders versions ascending, so the newest ones
        // are at the end; everything before them stays undated.
        let (_, newest) = split_for_metadata(versions, GO_LIST_VERSION_METADATA_LIMIT);
        let mut metadata_by_version = HashMap::with_capacity(newest.len());
        for chunk in newest.chunks(GO_LIST_VERSION_INFO_BATCH_SIZE) {
            let mut args = vec![
                "list".to_string(),
                "-mod=readonly".to_string(),
                "-m".to_string(),
                "-json".to_string(),
            ];
            for version in chunk {
                args.push(format!("{mod_path}@{version}"));
            }
            let args: Vec<&str> = args.iter().map(String::as_str).collect();
            let raw = match crate::cmd::cmd_read_async(&go, &args, &env).await {
                Ok(raw) => raw,
                Err(err) => {
                    debug!("go list metadata batch failed for {mod_path}: {err:#}");
                    continue;
                }
            };
            let Ok(infos) = Deserializer::from_str(&raw)
                .into_iter::<GoModuleVersionMetadata>()
                .collect::<Result<Vec<_>, _>>()
            else {
                continue;
            };
            for info in infos {
                metadata_by_version.insert(info.version.clone(), info);
            }
        }

        versions
            .iter()
            .map(|version| match metadata_by_version.remove(version) {
                Some(info) => version_info_from_metadata(info),
                None => bare(version),
            })
            .collect()
    }

    /// Read one module version's metadata through `go list`.
    async fn fetch_go_module_version_metadata(
        &self,
        config: &Arc<Config>,
        mod_path: &str,
        version: &str,
    ) -> Option<GoModuleVersionMetadata> {
        let env = self.go_list_env(config).await.ok()?;
        let go = self.spawn_program(config, None, "go").await;
        let target = format!("{mod_path}@{version}");
        let raw = match crate::cmd::cmd_read_async(
            &go,
            &["list", "-mod=readonly", "-m", "-json", target.as_str()],
            env,
        )
        .await
        {
            Ok(raw) => raw,
            Err(err) => {
                // Candidates that are not the module root miss routinely.
                debug!("go list metadata failed for {target}: {err:#}");
                return None;
            }
        };
        serde_json::from_str::<GoModuleVersionMetadata>(&raw).ok()
    }

    /// Resolve `@latest` for the module-path candidates through the module proxies.
    async fn proxy_latest_stable(&self, proxies: &[GoProxy], tool_name: &str) -> LatestOutcome {
        for mod_path in module_path_candidates(tool_name) {
            match query_proxy_latest(proxies, &encode_module_path(&mod_path)).await {
                ProxyVersionInfoResult::Found(info) => {
                    return classify_latest(&mod_path, version_info_from_metadata(info));
                }
                ProxyVersionInfoResult::NotFound => {}
                // A failing proxy is not a missing module: leave the report to
                // the version-list path.
                ProxyVersionInfoResult::Error => return LatestOutcome::Unresolved,
            }
        }
        LatestOutcome::NotFound
    }

    /// Resolve `@latest` for the module-path candidates through `go list`.
    async fn go_latest_stable(&self, config: &Arc<Config>, tool_name: &str) -> LatestOutcome {
        for mod_path in module_path_candidates(tool_name) {
            match self.fetch_go_module_latest_info(config, &mod_path).await {
                Ok(Some(mut infos)) => {
                    if let Some(info) = infos.pop() {
                        return classify_latest(&mod_path, info);
                    }
                }
                Ok(None) => {}
                Err(err) => {
                    debug!("failed to resolve latest Go module candidate {mod_path}: {err:#}");
                }
            }
        }
        LatestOutcome::NotFound
    }

    /// Resolve a module's `@latest` version using Go's native module routing.
    async fn fetch_go_module_latest_info(
        &self,
        config: &Arc<Config>,
        mod_path: &str,
    ) -> eyre::Result<Option<Vec<VersionInfo>>> {
        let env = self.go_list_env(config).await?;
        let go = self.spawn_program(config, None, "go").await;
        let latest = format!("{mod_path}@latest");
        let raw = crate::cmd::cmd_read_async(
            &go,
            &["list", "-mod=readonly", "-m", "-json", latest.as_str()],
            env,
        )
        .await
        .wrap_err_with(|| format!("failed to resolve latest Go module version for {mod_path}"))?;
        let info = serde_json::from_str::<GoModuleVersionMetadata>(&raw).wrap_err_with(|| {
            format!("failed to parse latest Go module metadata for {mod_path}")
        })?;
        Ok(Some(vec![version_info_from_metadata(info)]))
    }
}

/// Result of asking one route for a module's `@latest` version.
enum LatestOutcome {
    /// A stable release to install.
    Found(VersionInfo),
    /// The module was reached but `@latest` is not a stable release, or the
    /// route itself failed. Either way the version-list path decides.
    Unresolved,
    /// No module of that path here; another route may still have it.
    NotFound,
}

/// Accept a resolved `@latest` only when it is a stable release.
fn classify_latest(mod_path: &str, mut info: VersionInfo) -> LatestOutcome {
    if !is_stable_go_version(&info.version) {
        debug!("Go @latest resolved to a non-release version for {mod_path}");
        // This candidate is the module root; fall back to the normal
        // version-list path instead of trying a different module.
        return LatestOutcome::Unresolved;
    }
    // Checked here rather than relying on `VersionInfo::prerelease`, which
    // module metadata does not set.
    info.prerelease = Some(false);
    LatestOutcome::Found(info)
}

/// Split a version list ordered oldest-first into the versions that stay
/// undated and the newest `limit` worth a metadata query.
fn split_for_metadata(versions: &[String], limit: usize) -> (&[String], &[String]) {
    versions.split_at(versions.len().saturating_sub(limit))
}

fn module_path_candidates(tool_name: &str) -> Vec<String> {
    let parts: Vec<&str> = tool_name.split('/').collect();
    (1..=parts.len())
        .rev()
        .map(|i| parts[..i].join("/"))
        .collect()
}

enum ProxyListResult {
    Versions(Vec<String>),
    NotFound,
    Error,
}

enum ProxyVersionInfoResult {
    Found(GoModuleVersionMetadata),
    NotFound,
    Error,
}

#[derive(Clone, Debug, PartialEq)]
enum FallThrough {
    OnNotFound,
    OnAnyError,
}

#[derive(Clone)]
struct GoProxy {
    url: String,
    fall_through: FallThrough,
}

async fn query_proxy_list(proxies: &[GoProxy], encoded_path: &str) -> ProxyListResult {
    for proxy in proxies {
        let url = format!("{}/{}/@v/list", proxy.url, encoded_path);
        match HTTP_FETCH.get_text(&url).await {
            Ok(body) => {
                let versions: Vec<String> = body
                    .lines()
                    .filter(|l| !l.is_empty())
                    .map(|s| s.to_string())
                    .collect();
                return ProxyListResult::Versions(versions);
            }
            Err(e) => {
                let is_not_found_or_gone = e
                    .downcast_ref::<reqwest::Error>()
                    .and_then(|e| e.status())
                    .is_some_and(|s| {
                        s == reqwest::StatusCode::NOT_FOUND || s == reqwest::StatusCode::GONE
                    });

                if is_not_found_or_gone || proxy.fall_through == FallThrough::OnAnyError {
                    continue;
                }
                return ProxyListResult::Error;
            }
        }
    }

    ProxyListResult::NotFound
}

async fn query_proxy_latest(proxies: &[GoProxy], encoded_path: &str) -> ProxyVersionInfoResult {
    query_proxy_version_metadata(proxies, &format!("{encoded_path}/@latest")).await
}

async fn query_proxy_version_metadata(
    proxies: &[GoProxy],
    endpoint: &str,
) -> ProxyVersionInfoResult {
    for proxy in proxies {
        let url = format!("{}/{}", proxy.url, endpoint);
        match HTTP_FETCH.get_text(&url).await {
            Ok(body) => match serde_json::from_str::<GoModuleVersionMetadata>(&body) {
                Ok(info) => return ProxyVersionInfoResult::Found(info),
                Err(_) => return ProxyVersionInfoResult::Error,
            },
            Err(e) => {
                let is_not_found_or_gone = e
                    .downcast_ref::<reqwest::Error>()
                    .and_then(|e| e.status())
                    .is_some_and(|s| {
                        s == reqwest::StatusCode::NOT_FOUND || s == reqwest::StatusCode::GONE
                    });

                if is_not_found_or_gone || proxy.fall_through == FallThrough::OnAnyError {
                    continue;
                }
                return ProxyVersionInfoResult::Error;
            }
        }
    }
    ProxyVersionInfoResult::NotFound
}

/// Parse the effective GOPROXY setting into ordered proxy endpoints.
fn parse_goproxy(goproxy: Option<&str>) -> Vec<GoProxy> {
    // Treat unset or empty GOPROXY as the default, matching `go env GOPROXY`.
    let goproxy = goproxy.filter(|s| !s.is_empty()).unwrap_or(DEFAULT_GOPROXY);
    parse_goproxy_value(goproxy)
}

/// Return whether Go should handle module routing instead of mise's proxy client.
///
/// If private-module routing is configured, delegating all discovery to Go is
/// deliberately conservative: Go applies its own patterns and independently
/// honors GOPROXY, GONOPROXY, GOPRIVATE, GONOSUMDB, and GOSUMDB.
fn go_native_resolution_enabled(env: &BTreeMap<String, String>) -> bool {
    ["GOPRIVATE", "GONOPROXY"]
        .iter()
        .any(|key| env.get(*key).is_some_and(|value| !value.is_empty()))
}

/// Digest the effective Go module-routing settings for version-cache isolation.
fn go_routing_cache_context(env: &BTreeMap<String, String>) -> String {
    let goproxy = env
        .get("GOPROXY")
        .filter(|value| !value.is_empty())
        .map(String::as_str)
        .unwrap_or(DEFAULT_GOPROXY);
    let goprivate = env.get("GOPRIVATE").map(String::as_str).unwrap_or_default();
    // Go uses GOPRIVATE as GONOPROXY's default when GONOPROXY is unset or empty.
    let gonoproxy = env
        .get("GONOPROXY")
        .filter(|value| !value.is_empty())
        .map(String::as_str)
        .unwrap_or(goprivate);
    hash_to_str(&(goproxy, goprivate, gonoproxy))
}

/// Parse GOPROXY value per https://go.dev/ref/mod#goproxy-protocol:
/// - Comma after a URL: fall through to next entry only on 404/410.
/// - Pipe after a URL: fall through to next entry on any error.
fn parse_goproxy_value(goproxy: &str) -> Vec<GoProxy> {
    let mut proxies = Vec::new();
    let mut rest = goproxy;
    while !rest.is_empty() {
        let (entry, separator) = match rest.find([',', '|']) {
            Some(pos) => {
                let sep = rest.as_bytes()[pos];
                let entry = &rest[..pos];
                rest = &rest[pos + 1..];
                (entry, Some(sep))
            }
            None => {
                let entry = rest;
                rest = "";
                (entry, None)
            }
        };
        let entry = entry.trim();
        match entry {
            "" | "direct" => continue,
            "off" => break,
            url => {
                proxies.push(GoProxy {
                    url: url.trim_end_matches('/').to_string(),
                    fall_through: if separator == Some(b'|') {
                        FallThrough::OnAnyError
                    } else {
                        FallThrough::OnNotFound
                    },
                });
            }
        }
    }
    proxies
}

/// Encode a module path per https://go.dev/ref/mod#goproxy-protocol
fn encode_module_path(path: &str) -> String {
    let mut encoded = String::with_capacity(path.len());
    for c in path.chars() {
        if c.is_ascii_uppercase() {
            encoded.push('!');
            encoded.push(c.to_ascii_lowercase());
        } else {
            encoded.push(c);
        }
    }
    encoded
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct GoModInfo {
    #[serde(default)]
    versions: Vec<String>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
struct GoModuleVersionMetadata {
    version: String,
    #[serde(default)]
    time: Option<String>,
}

fn version_info_from_metadata(info: GoModuleVersionMetadata) -> VersionInfo {
    VersionInfo {
        version: info.version.trim_start_matches('v').to_string(),
        created_at: info.time,
        ..Default::default()
    }
}

/// Return whether a Go module version is a stable release rather than a pre-release or pseudo-version.
fn is_stable_go_version(version: &str) -> bool {
    versions::SemVer::new(version.trim_start_matches('v'))
        .is_some_and(|version| version.pre_rel.is_none())
}

async fn fetch_proxy_version_infos(
    proxies: &[GoProxy],
    path: &str,
    versions: &[String],
) -> Vec<VersionInfo> {
    let encoded = Arc::new(encode_module_path(path));
    let proxies = Arc::new(proxies.to_vec());
    let sem = Arc::new(Semaphore::new(GO_PROXY_VERSION_INFO_CONCURRENCY));
    let mut join_set = tokio::task::JoinSet::new();

    let (_, newest) = split_for_metadata(versions, GO_PROXY_VERSION_METADATA_LIMIT);
    for version in newest {
        let proxies = proxies.clone();
        let encoded = encoded.clone();
        let sem = sem.clone();
        let version = version.clone();
        join_set.spawn(async move {
            let _permit = sem.acquire_owned().await.expect("semaphore closed");
            let endpoint = format!("{encoded}/@v/{version}.info");
            let info = query_proxy_version_metadata(proxies.as_slice(), &endpoint).await;
            (version, info)
        });
    }

    let mut times = BTreeMap::new();
    while let Some(result) = join_set.join_next().await {
        match result {
            Ok((version, ProxyVersionInfoResult::Found(info))) => {
                times.insert(version, info.time);
            }
            Ok((version, ProxyVersionInfoResult::NotFound | ProxyVersionInfoResult::Error)) => {
                times.insert(version, None);
            }
            Err(e) => warn!("proxy version info task panicked: {e}"),
        }
    }

    versions
        .iter()
        .map(|version| VersionInfo {
            version: version.trim_start_matches('v').to_string(),
            created_at: times.get(version).cloned().flatten(),
            ..Default::default()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::toolset::ToolVersionOptions;

    #[tokio::test]
    async fn exact_semver_versions_resolve_without_remote_discovery() {
        let config = Config::get().await.unwrap();
        let backend = GoBackend::from_arg("go:github.com/example/tool".into());

        assert_eq!(
            backend
                .resolve_exact_version(&config, "1.2.3")
                .await
                .unwrap()
                .as_deref(),
            Some("1.2.3")
        );
        // "v" prefixes are normalized away, matching how versions are listed.
        assert_eq!(
            backend
                .resolve_exact_version(&config, "v1.2.3")
                .await
                .unwrap()
                .as_deref(),
            Some("1.2.3")
        );
    }

    #[tokio::test]
    async fn fuzzy_versions_require_remote_discovery() {
        let config = Config::get().await.unwrap();
        let backend = GoBackend::from_arg("go:github.com/example/tool".into());

        for version in ["latest", "1", "1.2", "^1.2.3", "main"] {
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

    #[test]
    fn go_options_reads_tags() {
        let mut opts = ToolVersionOptions::default();
        opts.opts.insert(
            "tags".to_string(),
            toml::Value::String("sqlite,fts5".to_string()),
        );

        assert_eq!(GoOptions::new(&opts).tags().as_deref(), Some("sqlite,fts5"));
        assert_eq!(
            GoOptions::new(&opts).lockfile_options(),
            BTreeMap::from([("tags".to_string(), "sqlite,fts5".to_string())])
        );
    }

    #[test]
    fn go_options_accepts_array_tags() {
        let mut opts = ToolVersionOptions::default();
        opts.opts.insert(
            "tags".to_string(),
            toml::Value::Array(vec![
                toml::Value::String("sqlite".to_string()),
                toml::Value::Integer(1),
                toml::Value::String("fts5".to_string()),
            ]),
        );

        assert_eq!(GoOptions::new(&opts).tags().as_deref(), Some("sqlite,fts5"));
        assert_eq!(
            GoOptions::new(&opts).lockfile_options(),
            BTreeMap::from([("tags".to_string(), "sqlite,fts5".to_string())])
        );
    }

    #[test]
    fn go_options_no_tags() {
        let opts = ToolVersionOptions::default();

        assert_eq!(GoOptions::new(&opts).tags(), None);
        assert_eq!(GoOptions::new(&opts).lockfile_options(), BTreeMap::new());
    }

    #[test]
    fn parse_go_mod_info_without_versions() {
        let raw = r#"{"Path":"github.com/go-kratos/kratos/cmd/kratos/v2"}"#;
        let info: GoModInfo = serde_json::from_str(raw).unwrap();
        assert!(info.versions.is_empty());
    }

    #[test]
    fn parse_go_mod_info_with_versions() {
        let raw = r#"{"Path":"example.com/mod","Versions":["v1.0.0","v1.1.0"]}"#;
        let info: GoModInfo = serde_json::from_str(raw).unwrap();
        assert_eq!(info.versions, vec!["v1.0.0", "v1.1.0"]);
    }

    #[test]
    fn parse_go_module_version_metadata() {
        let raw = r#"{"Version":"v1.2.3","Time":"2026-04-08T12:56:30Z"}"#;
        let info: GoModuleVersionMetadata = serde_json::from_str(raw).unwrap();
        assert_eq!(info.version, "v1.2.3");
        assert_eq!(info.time, Some("2026-04-08T12:56:30Z".to_string()));
    }

    #[test]
    fn short_version_lists_are_fully_dated() {
        let versions: Vec<String> = (0..3).map(|i| format!("v1.0.{i}")).collect();
        let (undated, newest) = split_for_metadata(&versions, 10);

        assert!(undated.is_empty());
        assert_eq!(newest, versions.as_slice());
    }

    #[test]
    fn long_version_lists_only_date_the_newest() {
        let versions: Vec<String> = (0..15).map(|i| format!("v1.0.{i}")).collect();
        let (undated, newest) = split_for_metadata(&versions, 10);

        assert_eq!(undated.len(), 5);
        assert_eq!(newest.len(), 10);
        assert_eq!(undated.first().unwrap(), "v1.0.0");
        assert_eq!(newest.last().unwrap(), versions.last().unwrap());
    }

    #[test]
    fn module_candidates_are_deepest_first() {
        assert_eq!(
            module_path_candidates("github.com/example/tool/cmd/tool"),
            vec![
                "github.com/example/tool/cmd/tool",
                "github.com/example/tool/cmd",
                "github.com/example/tool",
                "github.com/example",
                "github.com",
            ]
        );
    }

    #[test]
    fn encode_module_path_lowercase() {
        assert_eq!(
            encode_module_path("github.com/foo/bar"),
            "github.com/foo/bar"
        );
    }

    #[test]
    fn encode_module_path_uppercase() {
        assert_eq!(
            encode_module_path("github.com/GoogleCloudPlatform/scion"),
            "github.com/!google!cloud!platform/scion"
        );
    }

    #[test]
    fn parse_goproxy_default() {
        let proxies = parse_goproxy_value("https://proxy.golang.org,direct");
        assert_eq!(proxies.len(), 1);
        assert_eq!(proxies[0].url, "https://proxy.golang.org");
        assert_eq!(proxies[0].fall_through, FallThrough::OnNotFound);
    }

    #[test]
    fn parse_goproxy_pipe_separated() {
        let proxies =
            parse_goproxy_value("https://corp-proxy.example.com|https://proxy.golang.org|direct");
        assert_eq!(proxies.len(), 2);
        assert_eq!(proxies[0].url, "https://corp-proxy.example.com");
        assert_eq!(proxies[0].fall_through, FallThrough::OnAnyError);
        assert_eq!(proxies[1].url, "https://proxy.golang.org");
        assert_eq!(proxies[1].fall_through, FallThrough::OnAnyError);
    }

    #[test]
    fn parse_goproxy_mixed_separators() {
        let proxies =
            parse_goproxy_value("https://corp-proxy.example.com|https://proxy.golang.org,direct");
        assert_eq!(proxies.len(), 2);
        assert_eq!(proxies[0].url, "https://corp-proxy.example.com");
        assert_eq!(proxies[0].fall_through, FallThrough::OnAnyError);
        assert_eq!(proxies[1].url, "https://proxy.golang.org");
        assert_eq!(proxies[1].fall_through, FallThrough::OnNotFound);
    }

    #[test]
    fn parse_goproxy_direct_only() {
        let proxies = parse_goproxy_value("direct");
        assert!(proxies.is_empty());
    }

    #[test]
    fn parse_goproxy_off() {
        let proxies = parse_goproxy_value("off");
        assert!(proxies.is_empty());
    }

    #[test]
    fn parse_goproxy_off_stops_parsing() {
        let proxies =
            parse_goproxy_value("https://corp-proxy.example.com,off,https://proxy.golang.org");
        assert_eq!(proxies.len(), 1);
        assert_eq!(proxies[0].url, "https://corp-proxy.example.com");
    }

    /// Empty GOPROXY uses Go's default public proxy configuration.
    #[test]
    fn parse_goproxy_empty_uses_default() {
        let proxies = parse_goproxy(Some(""));
        assert_eq!(proxies.len(), 1);
        assert_eq!(proxies[0].url, "https://proxy.golang.org");
    }

    /// Any private-module routing setting delegates discovery to Go.
    #[test]
    fn private_module_settings_enable_go_native_resolution() {
        assert!(go_native_resolution_enabled(&BTreeMap::from([
            ("GOPRIVATE".to_string(), "github.com/example/*".to_string()),
            ("GONOPROXY".to_string(), String::new()),
        ])));
        assert!(go_native_resolution_enabled(&BTreeMap::from([(
            "GONOPROXY".to_string(),
            "github.com/example/*".to_string(),
        )])));
        assert!(!go_native_resolution_enabled(&BTreeMap::from([
            ("GOPRIVATE".to_string(), String::new()),
            ("GONOPROXY".to_string(), String::new()),
        ])));
    }

    /// Cache contexts track routing changes without exposing their values.
    #[test]
    fn routing_cache_context_is_private_and_tracks_effective_values() {
        let default = go_routing_cache_context(&BTreeMap::new());
        let explicit_default = go_routing_cache_context(&BTreeMap::from([
            ("GOPROXY".to_string(), DEFAULT_GOPROXY.to_string()),
            ("GOPRIVATE".to_string(), String::new()),
            ("GONOPROXY".to_string(), String::new()),
        ]));
        assert_eq!(default, explicit_default);

        for (key, value) in [
            (
                "GOPROXY",
                "https://user:secret@corp-proxy.example.com,direct",
            ),
            ("GOPRIVATE", "private.example.com/*"),
            ("GONOPROXY", "direct.example.com/*"),
        ] {
            let context =
                go_routing_cache_context(&BTreeMap::from([(key.to_string(), value.to_string())]));
            assert_ne!(default, context, "{key} must partition the cache");
            assert!(!context.contains(value));
            assert!(!context.contains("secret"));
        }
    }

    /// Stable Go versions exclude prereleases and pseudo-versions.
    #[test]
    fn stable_go_versions_exclude_prereleases_and_pseudo_versions() {
        assert!(is_stable_go_version("v1.2.3"));
        assert!(is_stable_go_version("1.2.3+incompatible"));
        assert!(!is_stable_go_version("v1.2.3-rc.1"));
        assert!(!is_stable_go_version("v0.0.0-20260903092947-0123456789ab"));
        assert!(!is_stable_go_version("not-a-version"));
    }
}
