use crate::args::BackendArg;
use crate::backend::Backend;
use crate::backend::VersionInfo;
use crate::backend::backend_type::BackendType;
use crate::backend::options::BackendOptions;
use crate::backend::platform_target::PlatformTarget;
use crate::cmd::CmdLineRunner;
use crate::config::Settings;
use crate::config::{Config, SettingsExt};
use crate::hash::hash_to_str;
use crate::http::HTTP_FETCH;
use crate::install_context::InstallContext;
use crate::timeout;
use crate::toolset::{ToolRequest, ToolVersion, ToolVersionOptions};
use async_trait::async_trait;
use eyre::{Result, WrapErr, eyre};
use serde_json::Deserializer;
use std::collections::{BTreeMap, HashMap};
use std::ffi::OsStr;
use std::{fmt::Debug, sync::Arc};
use tokio::sync::Semaphore;
use versions::Versioning;

#[derive(Debug)]
pub(crate) struct GoBackend {
    ba: Arc<BackendArg>,
    /// The module paths that could provide the tool's package, deepest first,
    /// or why the tool name is not a Go package path. Checked once here because
    /// the name comes from config that loads without trust, and every route
    /// puts it into a `go` command line or a module proxy URL.
    module_candidates: std::result::Result<Vec<String>, String>,
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
    fn option_spec(&self) -> Option<crate::backend::OptionSpec> {
        Some(crate::backend::OptionSpec::new(&["tool_go_options"]))
    }

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
        Ok(go_routing_cache_context(&env))
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
                let candidates = self.module_candidates()?;

                if let Some(versions) = self.fetch_proxy_versions(config, candidates).await? {
                    return Ok(versions);
                }

                // Fall back to `go list -m -versions` for GOPROXY=direct or when
                // private-module routing is configured. Package paths are not
                // necessarily module paths, so walk their prefixes
                // just as the proxy resolver does. `go list` runs with
                // GOTOOLCHAIN=local (see go_list_env) and gets the module path
                // after `--`, and the path is a checked package path (see
                // check_go_package_path), so it is safe against untrusted
                // config and does not need a safe-mode gate.
                let mut resolution_error = None;
                for mod_path in candidates {
                    match self.fetch_go_module_versions(config, mod_path).await {
                        // An empty list is an answer for this candidate, such as
                        // one past the version limit: a parent module's versions
                        // are not this tool's.
                        Ok(Some(versions)) => return Ok(versions),
                        Ok(None) => {}
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

        let candidates = self.module_candidates()?;
        let env = self.dependency_env(config).await?;
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
                    match self.proxy_latest_stable(&proxies, candidates).await {
                        LatestOutcome::Found(info) => return Ok(Some(info)),
                        LatestOutcome::Unresolved => return Ok(None),
                        // Not on the proxy, which is where a private module
                        // lands: the listing falls through to `go list` here
                        // too, so do the same instead of listing every tag.
                        LatestOutcome::NotFound => {}
                    }
                }
                Ok(match self.go_latest_stable(config, candidates).await {
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
        let candidates = self.module_candidates()?;
        // One full pass per route, not one per candidate: a package path's first
        // candidates are not the module root, and spending the timeout on a
        // `go list` miss for one of those would strand the request before the
        // proxy is ever asked about the path that listing already resolved.
        // Listing and `@latest` are laid out the same way.
        timeout::run_with_timeout_async(
            async || {
                // A source that could not be reached is not a version without a
                // release date, and the caller cannot tell them apart from
                // `Ok(None)`: it would take the version as checked and let it
                // past the cutoff without a word. Keep the first real failure so
                // this can report one.
                let mut failure: Option<eyre::Report> = None;
                for mod_path in candidates {
                    if proxies.is_empty() {
                        break;
                    }
                    let endpoint = format!("{}/@v/{version}.info", encode_module_path(mod_path));
                    match query_proxy_version_metadata(&proxies, &endpoint).await {
                        ProxyVersionInfoResult::Found(info) => return Ok(info.time),
                        // The module proxy does not serve this path; another
                        // candidate or `go list` still might.
                        ProxyVersionInfoResult::NotFound => {}
                        ProxyVersionInfoResult::Error => {
                            failure.get_or_insert_with(|| {
                                eyre!("module proxy could not be reached for {mod_path}@{version}")
                            });
                        }
                    }
                }
                // A private module under the default `proxy,direct` GOPROXY is
                // not on the proxy, and reporting no date for it would hand the
                // cutoff an undated version to treat as eligible — the exact gap
                // this exists to close.
                for mod_path in candidates {
                    match self
                        .fetch_go_module_version_metadata(config, mod_path, &version)
                        .await
                    {
                        Ok(metadata) => return Ok(metadata.time),
                        Err(err) => {
                            failure.get_or_insert(err);
                        }
                    }
                }
                // Only versions the listing already produced reach this, so the
                // module does resolve; nothing answering for it means something
                // was broken rather than undated.
                match failure {
                    Some(err) => Err(err),
                    None => Ok(None),
                }
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

        self.module_candidates()?;
        let install_version = tv.version.clone();

        let raw_opts = tv.request.options();
        let opts = GoOptions::new(&raw_opts);

        // The program does not change if an explicitly requested version needs a retry.
        let go = self.spawn_program(&ctx.config, Some(&ctx.ts), "go").await;

        // One `go install` attempt. Returns the captured stderr alongside the
        // result so the caller can tell a network blip from a bad version.
        let install_once = async |v: &str| {
            // An explicit -mod keeps a project's GOFLAGS=-mod=vendor out of this
            // install (#7052). `mod` rather than `readonly` also installs a tool whose
            // published go.mod is missing a requirement, which readonly rejects.
            let mut cmd = CmdLineRunner::new(&go).arg("install").arg("-mod=mod");

            if let Some(tags) = opts.tags() {
                cmd = cmd.arg("-tags").arg(tags);
            }

            let stderr = Arc::new(std::sync::Mutex::new(String::new()));
            let captured = stderr.clone();
            let env = match self.dependency_env(&ctx.config).await {
                Ok(env) => env,
                Err(e) => return (Err(e), String::new()),
            };
            // Raw mode hands `go` mise's own stderr, which would leave nothing to
            // classify and send every network failure down the non-transient path.
            let result = cmd
                .never_raw()
                // The name is a checked package path, so it never starts with
                // a dash; `--` keeps it an operand regardless.
                .arg("--")
                .arg(format!("{}@{v}", self.tool_name()))
                .with_pr(ctx.pr.as_ref())
                .with_stderr_observer(move |line| {
                    if let Ok(mut buf) = captured.lock() {
                        buf.push_str(line);
                        buf.push('\n');
                    }
                })
                .envs(env)
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
                .execute();
            let stderr = stderr.lock().map(|s| s.clone()).unwrap_or_default();
            (result, stderr)
        };

        // `go` downloads modules itself, so `http_retries` does not reach it.
        // Retry the same version on a transient transport failure with the same
        // setting and backoff; a failure that is not transient is returned as is,
        // along with whether it says that the version does not exist.
        let install = async |v: String| {
            let mut backoff =
                mise_util::http::default_backoff_strategy(Settings::get().http_retries);
            let mut attempt = 1;
            loop {
                let (result, stderr) = install_once(&v).await;
                let Err(err) = result else {
                    return Ok(());
                };
                if !is_transient_go_network_failure(&stderr) {
                    let missing = is_missing_go_version(&stderr, &self.tool_name(), &v);
                    return Err((err, missing));
                }
                let Some(delay) = backoff.next() else {
                    return Err((err, false));
                };
                warn!(
                    "go install {}@{v} attempt {attempt} failed (transient): {err:#}; retrying in {delay:?}",
                    self.tool_name()
                );
                tokio::time::sleep(delay).await;
                attempt += 1;
            }
        };

        let (version, fallback) = go_install_versions(&install_version, &tv.request.version());
        if let Err((first_error, missing_version)) = install(version).await {
            // The unprefixed version can only fix a version lookup failure. After
            // a network failure it would hide the cause behind `unknown revision`,
            // and after any other failure it is a different request: without the
            // `v`, Go looks for a tag, branch or commit of that name. Retrying
            // after Go rejected a download that does not match the checksum
            // database would install whatever such a ref holds instead.
            if let Some(fallback) = fallback.filter(|_| missing_version) {
                warn!("Failed to install, trying again without added 'v' prefix");
                if let Err((retry_error, _)) = install(fallback).await {
                    return Err(go_install_retry_error(first_error, retry_error));
                }
            } else {
                return Err(first_error);
            }
        }

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

fn go_install_versions(version: &str, requested_version: &str) -> (String, Option<String>) {
    if !version.starts_with('v') && versions::SemVer::new(version).is_some() {
        let fallback = (requested_version == version).then(|| version.to_string());
        (format!("v{version}"), fallback)
    } else {
        (version.to_string(), None)
    }
}

/// Whether `go install` stderr shows a transport failure worth retrying with
/// the same version, such as the proxy or checksum database dropping a stream.
/// A missing module or version is deterministic, and so is a DNS failure, which
/// mise's HTTP retries also skip.
fn is_transient_go_network_failure(stderr: &str) -> bool {
    const MARKERS: &[&str] = &[
        "stream error",
        "internal_error",
        "connection reset",
        "connection refused",
        "i/o timeout",
        "tls handshake timeout",
        "timeout awaiting response headers",
        "unexpected eof",
        ": eof",
        "500 internal server error",
        "502 bad gateway",
        "503 service unavailable",
        "504 gateway timeout",
        "429 too many requests",
    ];
    let stderr = stderr.to_lowercase();
    // A compiler diagnostic such as `syntax error: unexpected EOF` is not a
    // network failure.
    !stderr.contains("no such host")
        && !stderr.contains("syntax error")
        && MARKERS.iter().any(|m| stderr.contains(m))
}

/// Whether `go install package@version` stderr says that `version` of the
/// package's own module does not exist, which is the one failure the
/// unprefixed version can fix.
///
/// The rule: the output has a `module@version: ` token, at the start of the
/// output or after whitespace, where the module is the package path or a
/// leading part of it, followed by one of
///
/// - `invalid version: unknown revision rev`, from version control, where the
///   revision is the version itself or, for a module in a subdirectory of its
///   repository, `dir/version` with `dir` the trailing path elements of the
///   module path without its `/vN` major version suffix;
/// - `reading url: status`, from a module proxy, where the URL ends in
///   `/module/@v/version.info` with the module path in the proxy's encoding and
///   the status is 404 or 410; only the code is matched, since not every proxy
///   sends a reason phrase.
///
/// The token has to name the module and the version: the same words about a
/// dependency, another module whose path merely ends the same way, or another
/// version say nothing about the version that was asked for. A token after a
/// `requires` line is a dependency of a module Go found, even when its path
/// leads the package path, such as a parent module at the same version. A failed
/// verification is never a missing version, whatever else the output quotes:
/// Go found the version and refused what it downloaded.
fn is_missing_go_version(stderr: &str, package: &str, version: &str) -> bool {
    const VERIFICATION_MARKERS: &[&str] = &["verifying ", "checksum mismatch", "security error"];
    let stderr = stderr.to_lowercase();
    if VERIFICATION_MARKERS.iter().any(|m| stderr.contains(m)) {
        return false;
    }
    let version = version.to_lowercase();
    package
        .match_indices('/')
        .map(|(i, _)| &package[..i])
        .chain([package])
        .any(|module| {
            let token = format!("{}@{version}: ", module.to_lowercase());
            stderr.match_indices(&token).any(|(i, _)| {
                let before = &stderr[..i];
                let at_boundary = before.chars().next_back().is_none_or(char::is_whitespace);
                let required = before.contains(" requires\n");
                at_boundary
                    && !required
                    && says_version_is_missing(&stderr[i + token.len()..], module, &version)
            })
        })
}

/// Whether `rest`, the text after a `module@version: ` token, is one of the two
/// messages [`is_missing_go_version`] describes. `rest` and `version` are in
/// lower case, `module` is as written.
fn says_version_is_missing(rest: &str, module: &str, version: &str) -> bool {
    let next_word = |s: &str| {
        s.split(char::is_whitespace)
            .next()
            .unwrap_or_default()
            .to_string()
    };
    if let Some(rest) = rest.strip_prefix("invalid version: unknown revision ") {
        let revision = next_word(rest);
        if revision == version {
            return true;
        }
        let module = module.to_lowercase();
        let module = strip_major_version_suffix(&module);
        return revision
            .strip_suffix(version)
            .and_then(|dir| dir.strip_suffix('/'))
            .is_some_and(|dir| !dir.is_empty() && module.ends_with(&format!("/{dir}")));
    }
    if let Some(rest) = rest.strip_prefix("reading ") {
        let url = next_word(rest);
        let info = format!(
            "/{}/@v/{version}.info:",
            encode_module_path(module).to_lowercase()
        );
        let status = rest[url.len()..].trim_start();
        return url.ends_with(&info)
            && ["404", "410"].iter().any(|code| {
                status
                    .strip_prefix(code)
                    .is_some_and(|after| after.chars().next().is_none_or(char::is_whitespace))
            });
    }
    false
}

/// The module path without a `/vN` major version suffix, which names no
/// directory of the repository. Go writes one only for major version 2 and up,
/// without a leading zero, so a last element such as `v1` or `v0` is a
/// directory and stays.
fn strip_major_version_suffix(module: &str) -> &str {
    match module.rsplit_once('/') {
        Some((base, last))
            if last.strip_prefix('v').is_some_and(|n| {
                !n.is_empty()
                    && !n.starts_with('0')
                    && n != "1"
                    && n.bytes().all(|b| b.is_ascii_digit())
            }) =>
        {
            base
        }
        _ => module,
    }
}

fn go_install_retry_error(first_error: eyre::Report, retry_error: eyre::Report) -> eyre::Report {
    eyre!("{first_error:#}\nRetry without the 'v' prefix also failed: {retry_error:#}")
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
/// Most bytes of a module proxy's `.info` or `@latest` answer that are read.
///
/// A real one is a few hundred bytes, but the proxy decides what it sends, and
/// the HTTP client's own limit is sized for one large document: these are
/// fetched 20 at a time, and the date out of each is kept until the whole
/// listing is dated.
const GO_PROXY_VERSION_INFO_MAX_BYTES: usize = 64 * 1024;
/// Most versions accepted in one module's version list.
///
/// The HTTP client caps the size of a proxy's `@v/list` response, not the
/// number of lines in it, and each line becomes an owned string, a parsed sort
/// key and a `VersionInfo`: hundreds of bytes for a line that can be two.
/// The most heavily tagged modules have a few thousand versions, so a longer
/// list is refused rather than truncated: `@v/list` is unordered, and cutting
/// it short could drop the newest versions.
const GO_MODULE_VERSIONS_LIMIT: usize = 100_000;

impl GoBackend {
    pub(crate) fn from_arg(ba: BackendArg) -> Self {
        let tool_name = ba.tool_name();
        let module_candidates = check_go_package_path(&tool_name)
            .map(|()| module_path_candidates(&tool_name))
            .map_err(|reason| format!("{tool_name:?} is not a Go package path: {reason}"));
        Self {
            ba: Arc::new(ba),
            module_candidates,
        }
    }

    /// The module paths that could provide the tool's package, deepest first.
    /// Fails for a tool name that is not a Go package path.
    fn module_candidates(&self) -> Result<&[String]> {
        match &self.module_candidates {
            Ok(candidates) => Ok(candidates),
            Err(err) => Err(eyre!("{err}")),
        }
    }

    /// Query `$GOPROXY` to find versions, matching `go install`'s resolution algorithm.
    /// Returns `None` if no proxy is configured or Go should handle private-module routing,
    /// and an empty list when a proxy lists more versions than mise accepts.
    async fn fetch_proxy_versions(
        &self,
        config: &Arc<Config>,
        candidates: &[String],
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

        let mut join_set = tokio::task::JoinSet::new();
        for (idx, path) in candidates.iter().enumerate() {
            let encoded = encode_module_path(path);
            let proxies = proxies.clone();
            join_set.spawn(mise_util::untrusted_hosts::inherit_trusted_scope(
                async move {
                    let result = query_proxy_list(&proxies, &encoded).await;
                    (idx, result)
                },
            ));
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
                ProxyListResult::TooMany => return Ok(Some(vec![])),
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
    ///
    /// `GOWORK=off` and [`go_list`] running in a directory of mise's own keep
    /// a project `go.work` or `go.mod` out of the query altogether: as the main
    /// module, its `exclude` lines would hide versions from a listing that is
    /// cached for every directory.
    async fn go_list_env(&self, config: &Arc<Config>) -> eyre::Result<BTreeMap<String, String>> {
        let mut env = self.dependency_env(config).await?;
        env.insert("GOTOOLCHAIN".to_string(), "local".to_string());
        env.insert("GOWORK".to_string(), "off".to_string());
        Ok(env)
    }

    async fn fetch_go_module_versions(
        &self,
        config: &Arc<Config>,
        mod_path: &str,
    ) -> eyre::Result<Option<Vec<VersionInfo>>> {
        let go = self.spawn_program(config, None, "go").await;
        let raw = match go_list(
            &go,
            &[
                "list",
                "-mod=readonly",
                "-m",
                "-versions",
                "-json",
                "--",
                mod_path,
            ],
            &self.go_list_env(config).await?,
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
        // `go` relays whatever the proxy or the repository listed. Too long a
        // list is still the module's answer, and lists nothing.
        if mod_info.versions.len() > GO_MODULE_VERSIONS_LIMIT {
            warn!(
                "ignoring Go module version list for {mod_path}: more than {GO_MODULE_VERSIONS_LIMIT} versions"
            );
            return Ok(Some(vec![]));
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
                "--".to_string(),
            ];
            for version in chunk {
                args.push(format!("{mod_path}@{version}"));
            }
            let args: Vec<&str> = args.iter().map(String::as_str).collect();
            let raw = match go_list(&go, &args, &env).await {
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
    ) -> eyre::Result<GoModuleVersionMetadata> {
        let env = self.go_list_env(config).await?;
        let go = self.spawn_program(config, None, "go").await;
        let target = format!("{mod_path}@{version}");
        let raw = go_list(
            &go,
            &[
                "list",
                "-mod=readonly",
                "-m",
                "-json",
                "--",
                target.as_str(),
            ],
            &env,
        )
        .await
        .inspect_err(|err| {
            // Candidates that are not the module root miss routinely, so this
            // stays quiet here; the caller decides whether every candidate
            // missing amounts to a failure worth reporting.
            debug!("go list metadata failed for {target}: {err:#}");
        })?;
        serde_json::from_str::<GoModuleVersionMetadata>(&raw)
            .wrap_err_with(|| format!("failed to parse Go module metadata for {target}"))
    }

    /// Resolve `@latest` for the module-path candidates through the module proxies.
    async fn proxy_latest_stable(
        &self,
        proxies: &[GoProxy],
        candidates: &[String],
    ) -> LatestOutcome {
        for mod_path in candidates {
            match query_proxy_latest(proxies, &encode_module_path(mod_path)).await {
                ProxyVersionInfoResult::Found(info) => {
                    return classify_latest(mod_path, version_info_from_metadata(info));
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
    async fn go_latest_stable(&self, config: &Arc<Config>, candidates: &[String]) -> LatestOutcome {
        for mod_path in candidates {
            match self.fetch_go_module_latest_info(config, mod_path).await {
                Ok(Some(mut infos)) => {
                    if let Some(info) = infos.pop() {
                        return classify_latest(mod_path, info);
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
        let raw = go_list(
            &go,
            &[
                "list",
                "-mod=readonly",
                "-m",
                "-json",
                "--",
                latest.as_str(),
            ],
            &env,
        )
        .await
        .wrap_err_with(|| format!("failed to resolve latest Go module version for {mod_path}"))?;
        let info = serde_json::from_str::<GoModuleVersionMetadata>(&raw).wrap_err_with(|| {
            format!("failed to parse latest Go module metadata for {mod_path}")
        })?;
        Ok(Some(vec![version_info_from_metadata(info)]))
    }
}

/// Run a `go list` query with `env` as its whole environment, in
/// `CACHE/go-meta` rather than the directory mise was started in.
///
/// `go` takes the `go.mod` of its working directory or the nearest parent as
/// the main module, and a project's module would filter what the query
/// returns. The cache directory can sit inside a project, so it holds an empty
/// module of its own, which ends that search before it reaches a project file.
async fn go_list(go: &OsStr, args: &[&str], env: &BTreeMap<String, String>) -> Result<String> {
    const GO_META_MOD: &str = "module mise.invalid/go-meta\n";
    let dir = crate::dirs::CACHE.join("go-meta");
    crate::file::create_dir_all(&dir)?;
    let go_mod = dir.join("go.mod");
    if std::fs::read_to_string(&go_mod).ok().as_deref() != Some(GO_META_MOD) {
        crate::file::write_atomic(&go_mod, GO_META_MOD)?;
    }
    let program = match std::env::current_dir() {
        Ok(cwd) => program_outside_cwd(go, env, &cwd),
        Err(_) => go.to_os_string(),
    };
    let mut cmd = tokio::process::Command::new(program);
    cmd.env_clear().envs(env).current_dir(&dir);
    crate::cmd::cmd_read_async_command(cmd, &go.to_string_lossy(), args).await
}

/// `program` as a path that names the same executable once the child runs in
/// another directory. The child looks a bare name up in `env`'s PATH after it
/// changes directory, so a relative entry of that PATH, or a relative
/// `program`, would be read against the new directory instead of `cwd`. A bare
/// name found in no entry is returned as it is, for the child to report.
fn program_outside_cwd(
    program: &OsStr,
    env: &BTreeMap<String, String>,
    cwd: &std::path::Path,
) -> std::ffi::OsString {
    let path = std::path::Path::new(program);
    if path.is_absolute() {
        return program.to_os_string();
    }
    if path.components().count() > 1 {
        return cwd.join(path).into_os_string();
    }
    let Some(search) = env
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("PATH"))
        .map(|(_, value)| value)
    else {
        return program.to_os_string();
    };
    let names = crate::file::executable_names(&program.to_string_lossy());
    std::env::split_paths(search)
        // An empty or relative entry means a directory below `cwd`.
        .map(|dir| cwd.join(dir))
        .find_map(|dir| {
            names
                .iter()
                .map(|name| dir.join(name))
                // Spawnable rather than executable: on Windows a `go.ps1` may
                // come first, and `Command::new` cannot start it.
                .find(|candidate| candidate.is_file() && crate::file::is_spawnable(candidate))
        })
        .map_or_else(
            || program.to_os_string(),
            std::path::PathBuf::into_os_string,
        )
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

/// The longest `go:` tool name mise looks up.
///
/// Go sets no length limit, but real import paths stay far below this, and
/// every path prefix of the name becomes a candidate module: the cap keeps a
/// name from config that loads without trust from costing memory and requests
/// that grow with the square of its length.
const GO_PACKAGE_PATH_MAX_LEN: usize = 1024;

/// Check that `name` is a Go package path, or a package pattern with `...`
/// wildcards, by the rules `golang.org/x/mod/module.CheckImportPath` applies:
/// path elements of ASCII letters, digits and `-._~+` that are not empty and
/// neither begin nor end with a dot, and no leading dash. A name that passes
/// cannot be read as a flag by `go` and stays one path below the module
/// proxy's URL: it has no `..` element, `?`, `#`, `%` or `\`.
fn check_go_package_path(name: &str) -> std::result::Result<(), String> {
    if name.is_empty() {
        return Err("empty path".to_string());
    }
    if name.len() > GO_PACKAGE_PATH_MAX_LEN {
        return Err(format!(
            "longer than {GO_PACKAGE_PATH_MAX_LEN} bytes ({} bytes)",
            name.len()
        ));
    }
    if name.starts_with('-') {
        return Err("leading dash".to_string());
    }
    for elem in name.split('/') {
        if elem.is_empty() {
            return Err("empty path element".to_string());
        }
        if let Some(c) = elem
            .chars()
            .find(|c| !(c.is_ascii_alphanumeric() || "-._~+".contains(*c)))
        {
            return Err(format!("invalid character {c:?}"));
        }
        // A `...` wildcard may begin or end an element of a pattern; it is
        // never the `.` or `..` that the module proxy URL would resolve.
        if !elem.contains("...") && (elem.starts_with('.') || elem.ends_with('.')) {
            return Err(format!("path element {elem:?} begins or ends with a dot"));
        }
    }
    Ok(())
}

/// The prefixes of a package path checked by [`check_go_package_path`] that can
/// be module paths, deepest first: a module path has no `+` and no wildcard.
fn module_path_candidates(package_path: &str) -> Vec<String> {
    let parts: Vec<&str> = package_path.split('/').collect();
    (1..=parts.len())
        .rev()
        .filter(|&i| {
            parts[..i]
                .iter()
                .all(|elem| !elem.contains('+') && !elem.contains("..."))
        })
        .map(|i| parts[..i].join("/"))
        .collect()
}

enum ProxyListResult {
    Versions(Vec<String>),
    NotFound,
    /// A proxy listed more than [`GO_MODULE_VERSIONS_LIMIT`] versions. Unlike
    /// [`ProxyListResult::Error`], this does not hand the lookup to `go list`,
    /// which would fetch the same list again: mise lists no versions.
    TooMany,
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

/// Read a proxy's `@v/list` response, one version per line. Returns `None`
/// when it lists more than [`GO_MODULE_VERSIONS_LIMIT`] versions.
fn parse_proxy_version_list(body: &str) -> Option<Vec<String>> {
    let versions: Vec<String> = body
        .lines()
        .filter(|l| !l.is_empty())
        // One past the limit is enough to tell, without allocating the rest.
        .take(GO_MODULE_VERSIONS_LIMIT + 1)
        .map(|s| s.to_string())
        .collect();
    (versions.len() <= GO_MODULE_VERSIONS_LIMIT).then_some(versions)
}

async fn query_proxy_list(proxies: &[GoProxy], encoded_path: &str) -> ProxyListResult {
    let mut too_many = false;
    for proxy in proxies {
        let url = format!("{}/{}/@v/list", proxy.url, encoded_path);
        match HTTP_FETCH.get_text(&url).await {
            Ok(body) => {
                if let Some(versions) = parse_proxy_version_list(&body) {
                    return ProxyListResult::Versions(versions);
                }
                // Not a usable answer. `|` moves on to the next proxy as after
                // any error; otherwise the lookup ends here.
                // The URL stays out of the message: GOPROXY can hold credentials.
                warn!(
                    "ignoring Go module proxy version list for {encoded_path}: more than {GO_MODULE_VERSIONS_LIMIT} versions"
                );
                too_many = true;
                if proxy.fall_through == FallThrough::OnAnyError {
                    continue;
                }
                return ProxyListResult::TooMany;
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
                // An error after a refusal keeps the refusal: as an error it
                // would hand the lookup to `go list`, which asks the same
                // chain and gets the oversized list again.
                if too_many {
                    return ProxyListResult::TooMany;
                }
                return ProxyListResult::Error;
            }
        }
    }

    if too_many {
        ProxyListResult::TooMany
    } else {
        ProxyListResult::NotFound
    }
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
        match HTTP_FETCH
            .get_text_request(&url)
            .max_bytes(GO_PROXY_VERSION_INFO_MAX_BYTES)
            .send()
            .await
        {
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

/// Digest the effective Go module-routing settings for version-cache isolation,
/// or `None` for the default routing, which the shared versions host lists.
fn go_routing_cache_context(env: &BTreeMap<String, String>) -> Option<String> {
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
    if goproxy == DEFAULT_GOPROXY && goprivate.is_empty() && gonoproxy.is_empty() {
        return None;
    }
    Some(hash_to_str(&(goproxy, goprivate, gonoproxy)))
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
        join_set.spawn(mise_util::untrusted_hosts::inherit_trusted_scope(
            async move {
                let _permit = sem.acquire_owned().await.expect("semaphore closed");
                let endpoint = format!("{encoded}/@v/{version}.info");
                let info = query_proxy_version_metadata(proxies.as_slice(), &endpoint).await;
                (version, info)
            },
        ));
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

    #[cfg(unix)]
    #[test]
    fn go_list_program_is_found_before_changing_directory() {
        use std::os::unix::fs::PermissionsExt;

        let work = tempfile::tempdir().unwrap();
        let cwd = work.path();
        let bin = cwd.join("sdk/go/bin");
        std::fs::create_dir_all(&bin).unwrap();
        let go = bin.join("go");
        std::fs::write(&go, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&go, std::fs::Permissions::from_mode(0o755)).unwrap();
        let env_with = |path: &str| BTreeMap::from([("PATH".to_string(), path.to_string())]);
        let resolve = |program: &str, env: &BTreeMap<String, String>| {
            std::path::PathBuf::from(program_outside_cwd(OsStr::new(program), env, cwd))
        };

        // A relative PATH entry is read against the directory mise runs in.
        assert_eq!(resolve("go", &env_with("/nonexistent:sdk/go/bin")), go);
        // So is an empty one, which POSIX reads as the current directory.
        let here = env_with(":/nonexistent");
        assert_eq!(resolve("sdk/go/bin/go", &here), cwd.join("sdk/go/bin/go"));
        std::fs::copy(&go, cwd.join("go")).unwrap();
        assert_eq!(resolve("go", &here), cwd.join("go"));
        // A program that is a path already needs no lookup.
        assert_eq!(
            resolve("/usr/bin/go", &here),
            std::path::PathBuf::from("/usr/bin/go")
        );
        // A file without the execute bit is not the program.
        let plain = cwd.join("plain");
        std::fs::create_dir_all(&plain).unwrap();
        std::fs::write(plain.join("go"), "").unwrap();
        assert_eq!(resolve("go", &env_with("plain:sdk/go/bin")), go);
        // A name found nowhere is left for the child to report, as before.
        assert_eq!(
            resolve("go", &env_with("/nonexistent")),
            std::path::PathBuf::from("go")
        );
        assert_eq!(
            resolve("go", &BTreeMap::new()),
            std::path::PathBuf::from("go")
        );
    }

    #[cfg(windows)]
    #[test]
    fn go_list_program_is_one_windows_can_spawn() {
        let work = tempfile::tempdir().unwrap();
        let cwd = work.path();
        for (dir, name) in [("scripts", "go.ps1"), ("sdk", "go.exe")] {
            std::fs::create_dir_all(cwd.join(dir)).unwrap();
            std::fs::write(cwd.join(dir).join(name), "").unwrap();
        }
        let env = BTreeMap::from([("Path".to_string(), "scripts;sdk".to_string())]);
        assert_eq!(
            std::path::PathBuf::from(program_outside_cwd(OsStr::new("go"), &env, cwd)),
            cwd.join("sdk").join("go.exe")
        );
    }

    #[test]
    fn go_install_versions_only_retry_explicit_unprefixed_semver() {
        assert_eq!(
            go_install_versions("0.23.0", "latest"),
            ("v0.23.0".to_string(), None)
        );
        assert_eq!(
            go_install_versions("0.23.0", "0.23.0"),
            ("v0.23.0".to_string(), Some("0.23.0".to_string()))
        );
        assert_eq!(
            go_install_versions("0.23.0", "v0.23.0"),
            ("v0.23.0".to_string(), None)
        );
        assert_eq!(
            go_install_versions("v0.23.0", "v0.23.0"),
            ("v0.23.0".to_string(), None)
        );
        assert_eq!(
            go_install_versions("main", "main"),
            ("main".to_string(), None)
        );
    }

    #[test]
    fn transient_go_network_failures_are_recognized() {
        assert!(is_transient_go_network_failure(
            "go: x@v2.8.0: reading https://sum.golang.org/tile/8/0/x276/526: stream error: stream ID 59; INTERNAL_ERROR; received from peer"
        ));
        assert!(is_transient_go_network_failure(
            "dial tcp 1.2.3.4:443: i/o timeout"
        ));
        assert!(is_transient_go_network_failure(
            "reading https://proxy.golang.org/x/@v/list: 503 Service Unavailable"
        ));
        assert!(is_transient_go_network_failure(
            "go: x@v1.0.0: Get \"https://proxy.golang.org/x/@v/v1.0.0.info\": EOF"
        ));
        assert!(is_transient_go_network_failure(
            "reading https://sum.golang.org/latest: 500 Internal Server Error"
        ));
        assert!(!is_transient_go_network_failure(
            "./main.go:9:1: syntax error: unexpected EOF, expected }"
        ));
        assert!(!is_transient_go_network_failure(
            "go: x@2.8.0: unknown revision 2.8.0"
        ));
        assert!(!is_transient_go_network_failure(
            "dial tcp: lookup proxy.golang.org: no such host, connection refused"
        ));
        assert!(!is_transient_go_network_failure(""));
    }

    #[test]
    fn only_a_missing_go_version_allows_the_unprefixed_retry() {
        const TOOL: &str = "example.com/tool";
        // (stderr, package, version asked for, whether that version is missing)
        let cases: &[(&str, &str, &str, bool)] = &[
            (
                "go: example.com/tool@v1.2.3: example.com/tool@v1.2.3: invalid version: unknown revision v1.2.3",
                TOOL,
                "v1.2.3",
                true,
            ),
            (
                "go: example.com/tool@v1.2.3: reading https://proxy.golang.org/example.com/tool/@v/v1.2.3.info: 404 Not Found\n\tserver response: not found: example.com/tool@v1.2.3: invalid version: unknown revision v1.2.3",
                TOOL,
                "v1.2.3",
                true,
            ),
            (
                "go: example.com/tool@v1.2.3: reading https://goproxy.example.com/example.com/tool/@v/v1.2.3.info: 410 Gone",
                TOOL,
                "v1.2.3",
                true,
            ),
            (
                "go: example.com/tool@v1.2.3: reading https://goproxy.example.com/example.com/tool/@v/v1.2.3.info: 404 ",
                TOOL,
                "v1.2.3",
                true,
            ),
            // The module is the package path or a leading part of it.
            (
                "go: example.com/tool/cmd/x@v1.2.3: example.com/tool/cmd/x@v1.2.3: reading https://proxy.golang.org/example.com/tool/cmd/x/@v/v1.2.3.info: 404 Not Found\n\tserver response: not found: example.com/tool/cmd/x@v1.2.3: invalid version: unknown revision cmd/x/v1.2.3",
                "example.com/tool/cmd/x",
                "v1.2.3",
                true,
            ),
            (
                "go: example.com/tool/cmd/x@v1.2.3: example.com/tool@v1.2.3: reading https://proxy.golang.org/example.com/tool/@v/v1.2.3.info: 404 Not Found",
                "example.com/tool/cmd/x",
                "v1.2.3",
                true,
            ),
            // Version control names the revision of a module in a subdirectory
            // of its repository with that directory, without a major version
            // suffix.
            (
                "go: example.com/tool/sub@v1.2.3: example.com/tool/sub@v1.2.3: invalid version: unknown revision sub/v1.2.3",
                "example.com/tool/sub",
                "v1.2.3",
                true,
            ),
            (
                "go: example.com/tool/deep/cmd/x/v2@v2.0.0: example.com/tool/deep/cmd/x/v2@v2.0.0: invalid version: unknown revision deep/cmd/x/v2.0.0",
                "example.com/tool/deep/cmd/x/v2",
                "v2.0.0",
                true,
            ),
            // Only v2 and up is a major version suffix: `v1` and `v0` are
            // directories of the repository.
            (
                "go: example.com/tool/v1@v1.2.3: example.com/tool/v1@v1.2.3: invalid version: unknown revision v1/v1.2.3",
                "example.com/tool/v1",
                "v1.2.3",
                true,
            ),
            (
                "go: example.com/tool/v0@v0.2.3: example.com/tool/v0@v0.2.3: invalid version: unknown revision v0/v0.2.3",
                "example.com/tool/v0",
                "v0.2.3",
                true,
            ),
            (
                "go: example.com/tool/v1@v1.2.3: example.com/tool/v1@v1.2.3: invalid version: unknown revision tool/v1.2.3",
                "example.com/tool/v1",
                "v1.2.3",
                false,
            ),
            // A directory the module path does not end in is another module.
            (
                "go: example.com/tool/sub@v1.2.3: example.com/tool/sub@v1.2.3: invalid version: unknown revision other/v1.2.3",
                "example.com/tool/sub",
                "v1.2.3",
                false,
            ),
            (
                "go: example.com/tool/sub@v1.2.3: example.com/tool/sub@v1.2.3: invalid version: unknown revision /v1.2.3",
                "example.com/tool/sub",
                "v1.2.3",
                false,
            ),
            // Module proxies spell an upper-case letter as `!` and the letter.
            (
                "go: github.com/Example/tool@v1.2.3: reading https://proxy.golang.org/github.com/!example/tool/@v/v1.2.3.info: 404 Not Found",
                "github.com/Example/tool",
                "v1.2.3",
                true,
            ),
            // The unprefixed version can be missing as well.
            (
                "go: example.com/tool@1.2.3: example.com/tool@1.2.3: invalid version: unknown revision 1.2.3",
                TOOL,
                "1.2.3",
                true,
            ),
            // Go found the version and refused the download.
            (
                "go: example.com/tool@v1.2.3: example.com/tool@v1.2.3: verifying module: checksum mismatch\n\tdownloaded: h1:aaaa\n\tsum.golang.org: h1:bbbb\n\nSECURITY ERROR\nThis download does NOT match the one reported by the checksum server.",
                TOOL,
                "v1.2.3",
                false,
            ),
            (
                "go: example.com/tool@v1.2.3: verifying module: example.com/tool@v1.2.3: reading https://sum.golang.org/lookup/example.com/tool@v1.2.3: 404 Not Found\n\tserver response: not found: example.com/tool@v1.2.3: invalid version: unknown revision v1.2.3",
                TOOL,
                "v1.2.3",
                false,
            ),
            // A dependency that is missing, even at the same version number,
            // says nothing about the version asked for.
            (
                "go: example.com/tool@v1.2.3 requires\n\texample.com/dep@v0.4.0: invalid version: unknown revision v0.4.0",
                TOOL,
                "v1.2.3",
                false,
            ),
            (
                "go: example.com/tool@v1.2.3 requires\n\texample.com/dep@v1.2.3: invalid version: unknown revision v1.2.3",
                TOOL,
                "v1.2.3",
                false,
            ),
            (
                "go: example.com/tool@v1.2.3 requires\n\texample.com/dep@v1.2.3: reading https://proxy.golang.org/example.com/dep/@v/v1.2.3.info: 404 Not Found",
                TOOL,
                "v1.2.3",
                false,
            ),
            (
                "go: example.com/tool@v1.2.3 requires\n\texample.com/tool-extras@v1.2.3: reading https://proxy.golang.org/example.com/tool-extras/@v/v1.2.3.info: 404 Not Found",
                TOOL,
                "v1.2.3",
                false,
            ),
            // A parent module the tool requires at the same version is a
            // dependency too, though its path leads the package path.
            (
                "go: example.com/tool/cmd/x@v1.2.3 requires\n\texample.com/tool@v1.2.3: invalid version: unknown revision v1.2.3",
                "example.com/tool/cmd/x",
                "v1.2.3",
                false,
            ),
            (
                "go: example.com/tool/cmd/x@v1.2.3 requires\n\texample.com/tool@v1.2.3: reading https://proxy.golang.org/example.com/tool/@v/v1.2.3.info: 404 Not Found\n\tserver response: not found: example.com/tool@v1.2.3: invalid version: unknown revision v1.2.3",
                "example.com/tool/cmd/x",
                "v1.2.3",
                false,
            ),
            // A module whose path only ends in the module path is another
            // module, in the token and in the proxy URL.
            (
                "go: example.com/tool@v1.2.3 requires\n\tnotexample.com/tool@v1.2.3: invalid version: unknown revision v1.2.3",
                TOOL,
                "v1.2.3",
                false,
            ),
            (
                "go: example.com/tool@v1.2.3 requires\n\tgithub.com/foo/example.com/tool@v1.2.3: reading https://proxy.golang.org/github.com/foo/example.com/tool/@v/v1.2.3.info: 404 Not Found",
                TOOL,
                "v1.2.3",
                false,
            ),
            // A proxy answer for another module or with another status.
            (
                "go: example.com/tool@v1.2.3: example.com/tool@v1.2.3: reading https://proxy.golang.org/example.com/dep/@v/v1.2.3.info: 404 Not Found",
                TOOL,
                "v1.2.3",
                false,
            ),
            (
                "go: example.com/tool@v1.2.3: example.com/tool@v1.2.3: reading https://proxy.golang.org/example.com/tool/@v/v1.2.3.info: 4040 Unknown",
                TOOL,
                "v1.2.3",
                false,
            ),
            // Another version of the module, such as the one a retry asked for.
            (
                "go: example.com/tool@v1.2.3: example.com/tool@v1.2.4: invalid version: unknown revision v1.2.4",
                TOOL,
                "v1.2.3",
                false,
            ),
            // Neither a build failure nor a missing package is a missing version.
            (
                "./main.go:9:1: syntax error: unexpected EOF, expected }",
                TOOL,
                "v1.2.3",
                false,
            ),
            (
                "go: example.com/tool/cmd/x@v1.2.3: module example.com/tool@v1.2.3 found, but does not contain package example.com/tool/cmd/x",
                "example.com/tool/cmd/x",
                "v1.2.3",
                false,
            ),
            ("", TOOL, "v1.2.3", false),
        ];
        for (stderr, package, version, missing) in cases {
            assert_eq!(
                is_missing_go_version(stderr, package, version),
                *missing,
                "{package}@{version}: {stderr}"
            );
        }
    }

    #[test]
    fn go_install_retry_reports_original_error_first() {
        let error = go_install_retry_error(eyre!("original failure"), eyre!("retry failure"));
        let message = format!("{error:#}");
        assert!(message.starts_with("original failure"));
        assert!(message.contains("retry failure"));
    }

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

    #[tokio::test]
    async fn proxy_version_metadata_refuses_an_oversized_answer() {
        // Still valid JSON: the padding sits inside the date, which is kept.
        let oversized = format!(
            r#"{{"Version":"v1.0.1","Time":"2026-04-08T12:56:30Z{}"}}"#,
            " ".repeat(GO_PROXY_VERSION_INFO_MAX_BYTES)
        );
        let mut server = mockito::Server::new_async().await;
        let small = server
            .mock("GET", "/example.com/tool/@v/v1.0.0.info")
            .with_status(200)
            .with_body(r#"{"Version":"v1.0.0","Time":"2026-04-08T12:56:30Z"}"#)
            .create_async()
            .await;
        let large = server
            .mock("GET", "/example.com/tool/@v/v1.0.1.info")
            .with_status(200)
            .with_body(oversized)
            .create_async()
            .await;
        let proxies = parse_goproxy_value(&server.url());

        let found = query_proxy_version_metadata(&proxies, "example.com/tool/@v/v1.0.0.info").await;
        assert!(matches!(
            found,
            ProxyVersionInfoResult::Found(info)
                if info.time.as_deref() == Some("2026-04-08T12:56:30Z")
        ));
        let refused =
            query_proxy_version_metadata(&proxies, "example.com/tool/@v/v1.0.1.info").await;
        assert!(matches!(refused, ProxyVersionInfoResult::Error));
        small.assert_async().await;
        large.assert_async().await;
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
    fn proxy_version_lists_skip_blank_lines() {
        assert_eq!(
            parse_proxy_version_list("v1.0.0\n\nv1.1.0\n"),
            Some(vec!["v1.0.0".to_string(), "v1.1.0".to_string()])
        );
        assert_eq!(parse_proxy_version_list(""), Some(vec![]));
    }

    #[test]
    fn proxy_version_lists_past_the_limit_are_refused() {
        // Blank lines are not versions and do not count towards the limit.
        let at_limit = "1\n\n".repeat(GO_MODULE_VERSIONS_LIMIT);
        assert_eq!(
            parse_proxy_version_list(&at_limit).map(|versions| versions.len()),
            Some(GO_MODULE_VERSIONS_LIMIT)
        );
        assert_eq!(parse_proxy_version_list(&format!("{at_limit}1\n")), None);
    }

    #[tokio::test]
    async fn proxy_version_lists_past_the_limit_end_the_lookup() {
        let oversized = "v1.0.0\n".repeat(GO_MODULE_VERSIONS_LIMIT + 1);
        let mut hostile = mockito::Server::new_async().await;
        let listed = hostile
            .mock("GET", "/example.com/tool/@v/list")
            .with_status(200)
            .with_body(oversized)
            .expect(4)
            .create_async()
            .await;
        let mut fallback = mockito::Server::new_async().await;
        let fallback_list = fallback
            .mock("GET", "/example.com/tool/@v/list")
            .with_status(200)
            .with_body("v1.0.0\nv1.1.0\n")
            .create_async()
            .await;

        let only = parse_goproxy_value(&hostile.url());
        assert!(matches!(
            query_proxy_list(&only, "example.com/tool").await,
            ProxyListResult::TooMany
        ));
        // A later proxy without the module does not turn the refusal into a
        // miss, which would move on to other module paths or to `go list`.
        let mut missing = mockito::Server::new_async().await;
        let missing_list = missing
            .mock("GET", "/example.com/tool/@v/list")
            .with_status(404)
            .create_async()
            .await;
        let then_missing = parse_goproxy_value(&format!("{}|{}", hostile.url(), missing.url()));
        assert!(matches!(
            query_proxy_list(&then_missing, "example.com/tool").await,
            ProxyListResult::TooMany
        ));
        missing_list.assert_async().await;
        // Nor does a later proxy that fails, which would hand the lookup to
        // `go list` and so fetch the oversized list again.
        let mut unavailable = mockito::Server::new_async().await;
        let unavailable_list = unavailable
            .mock("GET", "/example.com/tool/@v/list")
            .with_status(503)
            // The HTTP client retries a 503.
            .expect_at_least(1)
            .create_async()
            .await;
        let then_failing = parse_goproxy_value(&format!("{}|{}", hostile.url(), unavailable.url()));
        assert!(matches!(
            query_proxy_list(&then_failing, "example.com/tool").await,
            ProxyListResult::TooMany
        ));
        unavailable_list.assert_async().await;
        // `|` moves on to the next proxy after any error, this one included.
        let chained = parse_goproxy_value(&format!("{}|{}", hostile.url(), fallback.url()));
        assert!(matches!(
            query_proxy_list(&chained, "example.com/tool").await,
            ProxyListResult::Versions(versions) if versions == ["v1.0.0", "v1.1.0"]
        ));
        listed.assert_async().await;
        fallback_list.assert_async().await;
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
    fn package_patterns_only_offer_module_paths_as_candidates() {
        assert_eq!(
            module_path_candidates("example.com/tool/c++/..."),
            vec!["example.com/tool", "example.com"]
        );
    }

    #[test]
    fn go_package_paths_are_accepted() {
        for name in [
            "github.com/example/tool/cmd/tool",
            "github.com/GoogleCloudPlatform/scion",
            "connectrpc.com/connect/v2/cmd/protoc-gen-connect-go",
            "example.com/tool/...",
            "example.com/tool/cmd/foo...",
            "example.com/c++/x_y~z",
            "cwd-test",
        ] {
            assert_eq!(check_go_package_path(name), Ok(()), "{name:?}");
        }
    }

    #[test]
    fn names_that_are_not_go_package_paths_are_refused() {
        for name in [
            "",
            "-debug-trace=../../.bashrc",
            "-x",
            "../../private/deploy-token.txt?",
            "./x",
            "example.com/../x",
            "/example.com/tool",
            "example.com/tool/",
            "example.com//tool",
            "example.com?/tool",
            "example.com#/tool",
            "%2e%2e/tool",
            "example.com\\tool",
            "example.com@v1/tool",
            "exa mple.com/tool",
            "example.com/tool.",
            "exämple.com/tool",
        ] {
            assert!(check_go_package_path(name).is_err(), "{name:?}");
        }
    }

    #[test]
    fn overlong_names_are_refused_before_splitting() {
        let at_limit = "a/".repeat(GO_PACKAGE_PATH_MAX_LEN / 2 - 1) + "aa";
        assert_eq!(at_limit.len(), GO_PACKAGE_PATH_MAX_LEN);
        assert_eq!(check_go_package_path(&at_limit), Ok(()));
        let over = format!("{at_limit}a");
        assert!(check_go_package_path(&over).is_err());
    }

    #[test]
    fn backend_for_an_invalid_name_refuses_every_lookup() {
        let backend = GoBackend::from_arg("go:-debug-trace=victim".into());
        let err = backend.module_candidates().unwrap_err().to_string();
        assert!(
            err.contains("is not a Go package path: leading dash"),
            "{err}"
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
        assert_eq!(
            default, None,
            "default routing is served by the versions host"
        );
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
            let context = context.expect("non-default routing has a context");
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
