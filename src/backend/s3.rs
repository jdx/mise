//! S3 backend for mise - downloads tools from Amazon S3 or S3-compatible storage
//!
//! This backend allows installing tools from private or public S3 buckets.
//! It supports version discovery via S3 object listing or manifest files.
//!
//! ## Configuration
//!
//! ```toml
//! [tools]
//! mytool = { version = "1.0.0", backend = "s3", url = "s3://bucket/tools/mytool-{{ version }}.tar.gz" }
//!
//! # With version discovery from manifest
//! [tools.mytool]
//! backend = "s3"
//! version = "latest"
//! url = "s3://bucket/tools/mytool-{{ version }}.tar.gz"
//! version_list_url = "s3://bucket/tools/versions.json"
//!
//! # With S3 listing-based discovery
//! [tools.mytool]
//! backend = "s3"
//! version = "latest"
//! url = "s3://bucket/tools/mytool-{{ version }}.tar.gz"
//! version_prefix = "tools/mytool-"
//! version_regex = "mytool-([0-9.]+)"
//!
//! # With custom endpoint (MinIO, etc.)
//! [tools.mytool]
//! backend = "s3"
//! url = "s3://bucket/tools/mytool-{{ version }}.tar.gz"
//! endpoint = "https://minio.internal:9000"
//! region = "us-east-1"
//! ```

pub(crate) const EXPERIMENTAL: bool = false;

use crate::args::BackendArg;
use crate::backend::backend_type::BackendType;
use crate::backend::options::BackendOptions;
use crate::backend::platform_target::PlatformTarget;
use crate::backend::static_helpers::{
    ArchiveLayout, get_filename_from_url, install_artifact, render_bin_path, template_string,
    template_string_for_target, verify_artifact,
};
use crate::backend::version_list;
use crate::backend::{Backend, VersionInfo, runtime_path_for_install_path};
use crate::config::{Config, Settings, SettingsExt};
use crate::file;
use crate::hash;
use crate::install_context::InstallContext;
use crate::lockfile::PlatformInfo;
use crate::timeout;
use crate::toolset::{ToolVersion, ToolVersionOptions};
use crate::ui::progress_report::SingleReport;
use async_trait::async_trait;
use aws_config::BehaviorVersion;
use aws_sdk_s3::Client as S3Client;
use aws_sdk_s3::primitives::ByteStream;
use eyre::{Result, bail, eyre};
use regex::Regex;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::sync::OnceCell;
use url::Url;

/// The most pages (up to 1000 keys each) a `version_prefix` listing may span before it fails,
/// so an endpoint that never answers with a last page cannot keep mise listing forever.
const MAX_LISTING_PAGES: usize = 100;

/// Parsed S3 URL components
#[derive(Debug, Clone)]
struct S3Url {
    bucket: String,
    key: String,
}

impl S3Url {
    /// Parse an S3 URL like "s3://bucket/path/to/object?region=us-west-2"
    fn parse(url_str: &str) -> Result<Self> {
        let url = Url::parse(url_str).map_err(|e| eyre!("Invalid S3 URL: {e}"))?;

        if url.scheme() != "s3" {
            bail!("URL must use s3:// scheme, got: {}", url.scheme());
        }

        let bucket = url
            .host_str()
            .ok_or_else(|| eyre!("S3 URL must include bucket name"))?
            .to_string();

        if bucket.is_empty() {
            bail!("S3 URL must include bucket name");
        }

        let key = url.path().trim_start_matches('/').to_string();

        Ok(Self { bucket, key })
    }
}

/// S3 backend for downloading tools from Amazon S3 or S3-compatible storage
#[derive(Debug)]
pub(crate) struct S3Backend {
    ba: Arc<BackendArg>,
    /// Cached S3 client, lazily initialized
    client: OnceCell<S3Client>,
}

/// The options of an S3 tool. `url` and `bin_path` are rendered for each version, so they are
/// read as stored, with what config load substituted escaped. Every other option is used as
/// written, so it is read from [`ToolVersionOptions::literal`].
#[derive(Debug, Clone)]
struct S3Options<'a> {
    values: BackendOptions<'a>,
    literal: ToolVersionOptions,
}

impl<'a> S3Options<'a> {
    fn new(raw: &'a ToolVersionOptions) -> Self {
        Self {
            values: BackendOptions::new(raw),
            literal: raw.literal(),
        }
    }

    fn plain(&self) -> BackendOptions<'_> {
        BackendOptions::new(&self.literal)
    }

    fn raw(&self) -> &'a ToolVersionOptions {
        self.values.raw()
    }

    fn url(&self) -> Option<String> {
        self.values.platform_string("url")
    }

    fn checksum(&self) -> Option<String> {
        self.plain().platform_string("checksum")
    }

    // Target-aware accessors for cross-platform `mise lock`. These resolve
    // `platforms.<key>.<opt>` for an arbitrary target rather than the host.
    fn url_for_target(&self, target: &PlatformTarget) -> Option<String> {
        self.values.platform_string_for_target("url", target)
    }

    fn checksum_for_target(&self, target: &PlatformTarget) -> Option<String> {
        self.plain().platform_string_for_target("checksum", target)
    }

    fn bin_path(&self) -> Option<String> {
        self.values.platform_string("bin_path")
    }

    fn region(&self) -> Option<String> {
        self.plain().platform_string("region")
    }

    fn endpoint(&self) -> Option<String> {
        self.plain().platform_string("endpoint")
    }

    fn version_list_url(&self) -> Option<String> {
        self.plain().platform_string("version_list_url")
    }

    fn version_prefix(&self) -> Option<String> {
        self.plain().platform_string("version_prefix")
    }

    fn version_regex(&self) -> Option<String> {
        self.plain().platform_string("version_regex")
    }

    fn version_json_path(&self) -> Option<String> {
        self.plain().platform_string("version_json_path")
    }

    fn version_expr(&self) -> Option<String> {
        self.plain().platform_string("version_expr")
    }

    fn url_platforms(&self) -> Vec<String> {
        self.values.available_platforms_with_key("url")
    }
}

impl S3Backend {
    pub(crate) fn from_arg(ba: BackendArg) -> Self {
        Self {
            ba: Arc::new(ba),
            client: OnceCell::new(),
        }
    }

    /// Get or create the S3 client
    ///
    /// Every request it sends is signed with the AWS credentials of the process that runs mise,
    /// while the bucket, key and endpoint come from tool options. Safe mode loads those from
    /// config nobody reviewed, so it refuses the client and with it every S3 request.
    async fn get_client(&self, opts: &S3Options<'_>) -> Result<&S3Client> {
        Settings::ensure_not_safe("accessing S3 with your AWS credentials")?;
        self.client
            .get_or_try_init(|| async {
                let region = opts.region();
                let endpoint = opts.endpoint();
                create_s3_client(region.as_deref(), endpoint.as_deref()).await
            })
            .await
    }

    /// Resolve the download URL from options and version
    fn resolve_url(&self, tv: &ToolVersion, opts: &S3Options<'_>) -> Result<String> {
        let url_template = opts.url().ok_or_else(|| self.missing_url_error(opts))?;

        Ok(template_string(&url_template, tv))
    }

    /// Built in one place so the dry-run check and the install itself cannot drift apart.
    /// Names the platform it looked for and the ones the tool does declare, because the fix is
    /// usually to add that key rather than to pick a different tool.
    fn missing_url_error(&self, opts: &S3Options<'_>) -> eyre::Report {
        let platform_key = self.get_platform_key();
        let available = opts.url_platforms();
        if available.is_empty() {
            eyre::eyre!("S3 backend requires 'url' option")
        } else {
            eyre::eyre!(
                "No URL for platform {platform_key}. Available: {}. \
                 Provide 'url' or add 'platforms.{platform_key}.url'",
                available.join(", ")
            )
        }
    }

    fn lock_url_for_target(
        &self,
        opts: &S3Options<'_>,
        tv: &ToolVersion,
        target: &PlatformTarget,
    ) -> Option<String> {
        opts.url_for_target(target)
            .map(|template| template_string_for_target(&template, tv, target))
    }

    /// Download an S3 object to a local file
    ///
    /// The body goes to disk as it arrives, so memory use does not grow with the object.
    /// `max_bytes` refuses a larger object, for a caller that reads the file back into memory.
    async fn download_object(
        &self,
        client: &S3Client,
        s3_url: &S3Url,
        dest: &Path,
        max_bytes: Option<u64>,
        pr: Option<&dyn SingleReport>,
    ) -> Result<()> {
        debug!(
            "Downloading s3://{}/{} to {}",
            s3_url.bucket,
            s3_url.key,
            dest.display()
        );

        let resp = client
            .get_object()
            .bucket(&s3_url.bucket)
            .key(&s3_url.key)
            .send()
            .await
            .map_err(|e| handle_s3_error(e, &s3_url.bucket, &s3_url.key))?;

        // Get content length for progress reporting
        let content_length = resp.content_length().unwrap_or(0) as u64;
        if let Some(max) = max_bytes
            && content_length > max
        {
            return Err(too_large(s3_url, max));
        }
        if let Some(pr) = pr {
            pr.set_length(content_length);
        }

        // Stream the body to a staged file then rename for atomic operation.
        // The staging directory is deleted when it is dropped before the rename.
        let staged = stage_download(dest)?;
        write_body(resp.body, s3_url, &staged.file, max_bytes, pr).await?;
        persist_download(staged, dest)
    }

    /// Fetch versions from a manifest file URL
    async fn fetch_versions_from_manifest(
        &self,
        client: &S3Client,
        manifest_url: &str,
        opts: &S3Options<'_>,
    ) -> Result<Vec<String>> {
        let s3_url = S3Url::parse(manifest_url)?;

        // Download manifest to temp location
        let tmp_dir = tempfile::tempdir()?;
        let tmp_path = tmp_dir.path().join("versions_manifest");
        // The manifest is parsed in memory, so it gets the limit the HTTP client puts on a
        // version list it parses.
        let max_bytes = crate::http::MAX_BODY_BYTES as u64;
        self.download_object(client, &s3_url, &tmp_path, Some(max_bytes), None)
            .await?;

        // Read and parse the manifest
        let content = file::read_to_string(&tmp_path)?;
        let regex = opts.version_regex();
        let json_path = opts.version_json_path();
        let version_expr = opts.version_expr();

        version_list::parse_version_list(
            &content,
            regex.as_deref(),
            json_path.as_deref(),
            version_expr.as_deref(),
        )
    }

    /// Fetch versions by listing S3 objects
    async fn fetch_versions_from_listing(
        &self,
        client: &S3Client,
        bucket: &str,
        prefix: &str,
        version_regex: &str,
    ) -> Result<Vec<String>> {
        let regex =
            Regex::new(version_regex).map_err(|e| eyre!("Invalid version_regex pattern: {e}"))?;

        let mut versions = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let mut continuation_token: Option<String> = None;
        let mut pages = 0;

        loop {
            let mut request = client.list_objects_v2().bucket(bucket).prefix(prefix);

            if let Some(token) = continuation_token {
                request = request.continuation_token(token);
            }

            let response = request
                .send()
                .await
                .map_err(|e| handle_s3_error(e, bucket, prefix))?;

            if let Some(contents) = response.contents {
                for object in contents {
                    if let Some(key) = object.key {
                        // Extract version using regex
                        if let Some(captures) = regex.captures(&key) {
                            let version = captures
                                .get(1)
                                .or_else(|| captures.get(0))
                                .map(|m| m.as_str().to_string());
                            if let Some(v) = version
                                && seen.insert(v.clone())
                            {
                                versions.push(v);
                            }
                        }
                    }
                }
            }

            if response.is_truncated == Some(true) {
                pages += 1;
                // Without a token the next request would ask for the first page again.
                if response.next_continuation_token.is_none() {
                    bail!("S3 listing of s3://{bucket}/{prefix} is truncated without a token");
                }
                if pages >= MAX_LISTING_PAGES {
                    bail!(
                        "S3 listing of s3://{bucket}/{prefix} did not finish after {pages} pages"
                    );
                }
                continuation_token = response.next_continuation_token;
            } else {
                break;
            }
        }

        Ok(versions)
    }

    /// Fetch versions using the configured method (manifest or listing)
    async fn fetch_versions(&self, config: &Arc<Config>) -> Result<Vec<String>> {
        let raw_opts = config.get_tool_opts_with_overrides(&self.ba).await?;
        let opts = S3Options::new(&raw_opts);

        // Try manifest-based version discovery first
        if let Some(manifest_url) = opts.version_list_url() {
            let client = self.get_client(&opts).await?;
            return self
                .fetch_versions_from_manifest(client, &manifest_url, &opts)
                .await;
        }

        // Try S3 listing-based version discovery
        if let Some(version_prefix) = opts.version_prefix() {
            let version_regex = opts
                .version_regex()
                .unwrap_or_else(|| r"([0-9]+\.[0-9]+\.[0-9]+)".to_string());

            // Extract bucket from url option
            let url_template = opts
                .url()
                .ok_or_else(|| eyre!("S3 backend requires 'url' option for version listing"))?;
            let s3_url = S3Url::parse(&url_template)?;

            let client = self.get_client(&opts).await?;
            return self
                .fetch_versions_from_listing(
                    client,
                    &s3_url.bucket,
                    &version_prefix,
                    &version_regex,
                )
                .await;
        }

        // No version discovery configured - return empty without needing S3 client
        Ok(vec![])
    }

    /// Verify checksum and generate lockfile info
    fn verify_checksum(
        &self,
        ctx: &InstallContext,
        tv: &mut ToolVersion,
        file_path: &Path,
    ) -> Result<()> {
        let settings = Settings::get();
        let filename = file_path.file_name().unwrap().to_string_lossy();
        let lockfile_enabled = settings.lockfile_enabled();

        let platform_key = self.get_platform_key();
        let platform_info = tv.lock_platforms.entry(platform_key).or_default();

        // Verify or generate checksum
        if let Some(checksum) = &platform_info.checksum {
            ctx.pr.set_message(format!("checksum {filename}"));
            let (algo, check) = checksum
                .split_once(':')
                .ok_or_else(|| eyre!("Invalid checksum format: {checksum}"))?;
            hash::ensure_checksum(file_path, check, Some(ctx.pr.as_ref()), algo)?;
        } else if lockfile_enabled {
            ctx.pr.set_message(format!("generate checksum {filename}"));
            let h = hash::file_hash_blake3(file_path, Some(ctx.pr.as_ref()))?;
            platform_info.checksum = Some(format!("blake3:{h}"));
        }

        // Verify or record size
        if let Some(expected_size) = platform_info.size {
            ctx.pr.set_message(format!("verify size {filename}"));
            let actual_size = file_path.metadata()?.len();
            if actual_size != expected_size {
                return Err(eyre!(
                    "Size mismatch for {filename}: expected {expected_size}, got {actual_size}"
                ));
            }
        } else if lockfile_enabled {
            platform_info.size = Some(file_path.metadata()?.len());
        }

        Ok(())
    }
}

/// Returns install-time-only option keys for S3 backend.
pub(crate) fn install_time_option_keys() -> Vec<String> {
    vec![
        "url".into(),
        "checksum".into(),
        "bin_path".into(),
        "version_list_url".into(),
        "version_regex".into(),
        "version_json_path".into(),
        "version_expr".into(),
        "version_prefix".into(),
        "format".into(),
        "region".into(),
        "endpoint".into(),
    ]
}

/// Create an S3 client with the given configuration
async fn create_s3_client(region: Option<&str>, endpoint: Option<&str>) -> Result<S3Client> {
    let mut config_loader = aws_config::defaults(BehaviorVersion::latest());

    if let Some(region) = region {
        config_loader = config_loader.region(aws_config::Region::new(region.to_string()));
    }

    let sdk_config = config_loader.load().await;

    let mut s3_config = aws_sdk_s3::config::Builder::from(&sdk_config);

    if let Some(endpoint) = endpoint {
        s3_config = s3_config.endpoint_url(endpoint).force_path_style(true);
    }

    Ok(S3Client::from_conf(s3_config.build()))
}

/// Where an S3 object is downloaded before it is renamed to its destination:
/// `file`, in a new directory beside the destination that is deleted, with
/// whatever is in it, when this is dropped.
struct StagedDownload {
    dir: tempfile::TempDir,
    file: PathBuf,
}

/// A new, uniquely named directory beside `dest` to download into, so that no
/// name `dest` can have (one ending in `.tmp` included) makes it `dest` itself.
///
/// The directory is the private part: on unix it is created reachable by its
/// owner only, so the partial download is private while the file in it is
/// created like any other, with the mode `dest` has always had. The
/// name is short and fixed rather than built from `dest`'s, which may already
/// be close to the filesystem's limit on a name. Its first character differs
/// from `dest`'s, so the two can never be the same name.
fn stage_download(dest: &Path) -> Result<StagedDownload> {
    let parent = match dest.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir,
        _ => Path::new("."),
    };
    let dest_is_dotted = dest
        .file_name()
        .is_some_and(|name| name.as_encoded_bytes().starts_with(b"."));
    let prefix = if dest_is_dotted {
        "_mise-s3-"
    } else {
        ".mise-s3-"
    };
    let mut builder = tempfile::Builder::new();
    builder.prefix(prefix).suffix(".tmp");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(std::fs::Permissions::from_mode(0o700));
    }
    let dir = builder.tempdir_in(parent)?;
    let file = dir.path().join("download");
    Ok(StagedDownload { dir, file })
}

/// Move a finished download to `dest`. If that fails, `staged` is dropped,
/// which removes it.
///
/// `std::fs::rename` rather than tempfile's `persist`: only std turns a path
/// past `MAX_PATH` into the extended-length form Windows needs without
/// long-path support, and tempfile's call would fail on one only once the
/// whole object has been downloaded.
fn persist_download(staged: StagedDownload, dest: &Path) -> Result<()> {
    std::fs::rename(&staged.file, dest).map_err(|err| {
        eyre!(
            "failed to move {} to {}: {err}",
            file::display_path(&staged.file),
            file::display_path(dest)
        )
    })?;
    // Only the empty directory is left, which this removes.
    drop(staged.dir);
    Ok(())
}

/// Write an S3 object body to `path` chunk by chunk, failing once it passes `max_bytes`.
/// Counted as the bytes arrive, because a response need not announce its length.
async fn write_body(
    mut body: ByteStream,
    s3_url: &S3Url,
    path: &Path,
    max_bytes: Option<u64>,
    pr: Option<&dyn SingleReport>,
) -> Result<()> {
    let mut file = tokio::fs::File::create(path).await?;
    let mut written = 0u64;
    while let Some(chunk) = body
        .try_next()
        .await
        .map_err(|e| eyre!("Failed to read S3 response body: {e}"))?
    {
        written += chunk.len() as u64;
        if let Some(max) = max_bytes
            && written > max
        {
            return Err(too_large(s3_url, max));
        }
        file.write_all(&chunk).await?;
        if let Some(pr) = pr {
            pr.inc(chunk.len() as u64);
        }
    }
    // `tokio::fs::File` buffers writes and dropping one does not flush
    file.flush().await?;
    Ok(())
}

fn too_large(s3_url: &S3Url, max_bytes: u64) -> eyre::Report {
    eyre!(
        "s3://{}/{} is larger than {}",
        s3_url.bucket,
        s3_url.key,
        bytesize::ByteSize::b(max_bytes).display().iec()
    )
}

/// Convert S3 SDK errors to user-friendly error messages
fn handle_s3_error<E: std::fmt::Debug>(err: E, bucket: &str, key: &str) -> eyre::Report {
    let err_str = format!("{err:?}");

    if err_str.contains("NoSuchKey") {
        eyre!("S3 object not found: s3://{bucket}/{key}. Check the URL and version.")
    } else if err_str.contains("NoSuchBucket") {
        eyre!("S3 bucket not found: {bucket}. Check the bucket name.")
    } else if err_str.contains("AccessDenied") || err_str.contains("Forbidden") {
        eyre!(
            "Access denied to S3 bucket '{bucket}'. Check your AWS credentials and IAM permissions.\n\
             Ensure AWS_ACCESS_KEY_ID and AWS_SECRET_ACCESS_KEY are set, or use IAM roles."
        )
    } else if err_str.contains("InvalidAccessKeyId") {
        eyre!("Invalid AWS access key. Check your AWS_ACCESS_KEY_ID environment variable.")
    } else if err_str.contains("SignatureDoesNotMatch") {
        eyre!("AWS signature mismatch. Check your AWS_SECRET_ACCESS_KEY environment variable.")
    } else if err_str.contains("timeout") || err_str.contains("Timeout") {
        eyre!("S3 request timed out. Check your network connection and endpoint URL.")
    } else {
        eyre!("S3 error: {err:?}")
    }
}

#[async_trait]
impl Backend for S3Backend {
    fn option_spec(&self) -> Option<crate::backend::OptionSpec> {
        Some(crate::backend::OptionSpec::with_platforms(
            &["tool_s3_options"],
            &["tool_s3_platforms"],
        ))
    }

    fn get_type(&self) -> BackendType {
        BackendType::S3
    }

    fn ba(&self) -> &Arc<BackendArg> {
        &self.ba
    }

    fn mark_prereleases_from_version_pattern(&self) -> bool {
        true
    }

    /// `AWS_ENDPOINT_URL` points the AWS SDK at another S3-compatible service
    /// than the one the shared versions host listed.
    async fn remote_version_cache_context(&self, _config: &Arc<Config>) -> Result<Option<String>> {
        Ok(["AWS_ENDPOINT_URL_S3", "AWS_ENDPOINT_URL"]
            .into_iter()
            .find_map(|key| std::env::var(key).ok().filter(|value| !value.is_empty()))
            .map(|endpoint| hash::hash_to_str(&endpoint)))
    }

    fn remote_version_listing_tool_option_keys(&self) -> &'static [&'static str] {
        &[
            "version_list_url",
            "version_regex",
            "version_json_path",
            "version_expr",
            "version_prefix",
            "url",
            "region",
            "endpoint",
        ]
    }

    /// How versions are discovered. By the time something is installed the version is
    /// concrete and the artifact comes from `url`, so none of these change what is installed.
    ///
    /// `bin_path` is not listed: it decides where a raw file or an archive lands in the
    /// install directory, not only which directory goes on PATH.
    fn identity_ignored_options(&self) -> &'static [&'static str] {
        &[
            "version_list_url",
            "version_regex",
            "version_json_path",
            "version_expr",
            "version_prefix",
        ]
    }

    async fn install_operation_count(&self, tv: &ToolVersion, _ctx: &InstallContext) -> usize {
        let raw_opts = tv.request.options();
        let opts = S3Options::new(&raw_opts);
        super::http_install_operation_count(opts.checksum().is_some(), &self.get_platform_key(), tv)
    }

    /// Resolve URL + configured checksum for a target platform during `mise lock`,
    /// without downloading the artifact. Fails closed when no URL is configured
    /// for the target so the lock run reports it as skipped rather than writing
    /// nothing under a success count.
    async fn resolve_lock_info(
        &self,
        tv: &ToolVersion,
        target: &PlatformTarget,
    ) -> Result<PlatformInfo> {
        let raw_opts = tv.request.options();
        let opts = S3Options::new(&raw_opts);

        let Some(url) = self.lock_url_for_target(&opts, tv, target) else {
            return Err(eyre!(
                "no URL configured for {} on {}; skipping",
                self.ba.full(),
                target.to_key()
            ));
        };

        Ok(PlatformInfo {
            url: Some(url),
            checksum: opts.checksum_for_target(target),
            ..Default::default()
        })
    }

    async fn _list_remote_versions(&self, config: &Arc<Config>) -> Result<Vec<VersionInfo>> {
        // These requests go through the AWS SDK, not mise's HTTP client, so the lookup budget
        // that client enforces has to be applied here. It also ends a listing or a manifest
        // download from an endpoint that stops answering.
        let versions = timeout::run_with_timeout_async(
            async || self.fetch_versions(config).await,
            Settings::get().fetch_remote_versions_timeout(),
        )
        .await?;
        Ok(versions
            .into_iter()
            .map(|v| VersionInfo {
                version: v,
                ..Default::default()
            })
            .collect())
    }

    /// An s3 tool with no URL for this platform cannot be installed, and the options say so
    /// without a single request. `--dry-run` used to answer "would install" for exactly the case
    /// the real install rejects on its first line (the same gap http and aqua closed in #12568).
    async fn verify_install_feasible(&self, _ctx: &InstallContext, tv: &ToolVersion) -> Result<()> {
        let raw_opts = tv.request.options();
        let opts = S3Options::new(&raw_opts);
        match opts.url() {
            Some(_) => Ok(()),
            None => Err(self.missing_url_error(&opts)),
        }
    }

    async fn install_version_(
        &self,
        ctx: &InstallContext,
        mut tv: ToolVersion,
    ) -> Result<ToolVersion> {
        let raw_opts = tv.request.options();
        let opts = S3Options::new(&raw_opts);

        // Resolve URL template
        let url = self.resolve_url(&tv, &opts)?;
        let s3_url = S3Url::parse(&url)?;

        // Get S3 client
        let client = self.get_client(&opts).await?;

        // Prepare download path
        let filename = get_filename_from_url(&url);
        let file_path = tv.download_path().join(&filename);

        // Record URL in lock platforms
        let platform_key = self.get_platform_key();
        tv.lock_platforms
            .entry(platform_key.clone())
            .or_default()
            .url = Some(url.clone());

        // For lockfile checksum verification
        let settings = Settings::get();
        let lockfile_enabled = settings.lockfile_enabled();
        let has_lockfile_checksum = tv
            .lock_platforms
            .get(&platform_key)
            .and_then(|p| p.checksum.as_ref())
            .is_some();

        // Download from S3
        ctx.pr.set_message(format!("download {filename}"));
        file::create_dir_all(tv.download_path())?;
        self.download_object(client, &s3_url, &file_path, None, Some(ctx.pr.as_ref()))
            .await?;

        // Verify artifact (checksum/size from options)
        if opts.checksum().is_some() {
            ctx.pr.next_operation();
        }
        verify_artifact(&tv, &file_path, opts.raw(), Some(ctx.pr.as_ref()))?;

        // Verify/generate lockfile checksum (before extraction for security)
        if lockfile_enabled || has_lockfile_checksum {
            ctx.pr.next_operation();
        }
        self.verify_checksum(ctx, &mut tv, &file_path)?;

        // Extract and install
        ctx.pr.next_operation();
        ctx.pr.set_message("extract".into());
        install_artifact(
            &tv,
            &file_path,
            opts.raw(),
            ArchiveLayout::Guessed,
            Some(ctx.pr.as_ref()),
        )?;

        Ok(tv)
    }

    async fn list_bin_paths(
        &self,
        _config: &Arc<Config>,
        tv: &ToolVersion,
    ) -> Result<Vec<PathBuf>> {
        let raw_opts = tv.request.options();
        let opts = S3Options::new(&raw_opts);

        // Check for explicit bin_path
        if let Some(bin_path_template) = opts.bin_path() {
            let bin_path = render_bin_path(&bin_path_template, tv)?;
            return Ok(vec![runtime_path_for_install_path(
                tv,
                tv.install_path().join(bin_path),
            )]);
        }

        // Check for bin directory
        let bin_dir = tv.install_path().join("bin");
        if bin_dir.exists() {
            return Ok(vec![runtime_path_for_install_path(tv, bin_dir)]);
        }

        // Search subdirectories for bin directories
        let mut paths = Vec::new();
        if let Ok(entries) = std::fs::read_dir(tv.install_path()) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    let sub_bin = path.join("bin");
                    if sub_bin.exists() {
                        paths.push(sub_bin);
                    }
                }
            }
        }

        if paths.is_empty() {
            Ok(vec![tv.runtime_path()])
        } else {
            Ok(paths
                .into_iter()
                .map(|path| runtime_path_for_install_path(tv, path))
                .collect())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::args::BackendResolution;
    use crate::platform::Platform;
    use crate::toolset::{ToolRequest, ToolSource};
    use indexmap::IndexMap;

    fn s3_test_backend() -> S3Backend {
        S3Backend::from_arg(BackendArg::new_raw(
            "s3-xtool".to_string(),
            Some("s3:xtool".to_string()),
            "xtool".to_string(),
            None,
            BackendResolution::new(true),
        ))
    }

    fn s3_test_tv(version: &str, options: ToolVersionOptions) -> ToolVersion {
        let backend = Arc::new(BackendArg::new_raw(
            "s3-xtool".to_string(),
            Some("s3:xtool".to_string()),
            "xtool".to_string(),
            None,
            BackendResolution::new(true),
        ));
        let request =
            ToolRequest::new_with_options(backend, version, options, ToolSource::Argument).unwrap();
        ToolVersion::new(request, version.to_string())
    }

    fn target(key: &str) -> PlatformTarget {
        PlatformTarget::new(Platform::parse(key).unwrap())
    }

    #[test]
    fn test_identity_options_ignore_version_listing_options() {
        use crate::backend::static_helpers::test_identity_options;
        let backend = s3_test_backend();
        let identity = |options: &[(&str, &str)]| {
            test_identity_options(
                &backend,
                "1.0.0",
                &[
                    &[("url", "s3://bucket/tools/xtool-{{version}}.tar.gz")],
                    options,
                ]
                .concat(),
            )
        };
        let base = identity(&[]);
        assert_eq!(
            base.get("url").map(String::as_str),
            Some("s3://bucket/tools/xtool-{{version}}.tar.gz")
        );

        for (key, value) in [
            ("version_list_url", "s3://bucket/tools/versions.json"),
            ("version_prefix", "tools/xtool-"),
            ("version_regex", "xtool-([0-9.]+)"),
            ("version_json_path", "versions"),
            ("version_expr", "split(body, \"\\n\")"),
        ] {
            assert_eq!(
                identity(&[(key, value)]),
                base,
                "{key} only discovers versions"
            );
        }

        for (key, value) in [
            ("bin_path", "tool/bin"),
            ("strip_components", "2"),
            ("region", "eu-west-1"),
            ("endpoint", "https://minio.internal:9000"),
        ] {
            assert_ne!(
                identity(&[(key, value)]),
                base,
                "{key} changes what is installed"
            );
        }
    }

    #[test]
    fn test_lock_url_for_target_platforms_table() {
        let mut opts = IndexMap::new();
        opts.insert(
            "platforms".to_string(),
            toml::from_str::<toml::Value>(
                r#"
[linux-x64]
url = "s3://bucket/tools/xtool-{{version}}-linux-x64.tar.gz"

[macos-arm64]
url = "s3://bucket/tools/xtool-{{version}}-macos-arm64.tar.gz"
checksum = "sha256:abc123"
"#,
            )
            .unwrap(),
        );
        let tool_opts = ToolVersionOptions {
            opts: opts.into(),
            ..Default::default()
        };
        let backend = s3_test_backend();
        let tv = s3_test_tv("1.0.0", tool_opts);
        let raw_opts = tv.request.options();
        let opts = S3Options::new(&raw_opts);

        assert_eq!(
            backend.lock_url_for_target(&opts, &tv, &target("linux-x64")),
            Some("s3://bucket/tools/xtool-1.0.0-linux-x64.tar.gz".to_string())
        );
        assert_eq!(
            backend.lock_url_for_target(&opts, &tv, &target("macos-arm64")),
            Some("s3://bucket/tools/xtool-1.0.0-macos-arm64.tar.gz".to_string())
        );
        // no URL configured for this platform
        assert_eq!(
            backend.lock_url_for_target(&opts, &tv, &target("windows-x64")),
            None
        );

        // per-platform checksum is picked up for the target
        assert_eq!(
            opts.checksum_for_target(&target("macos-arm64")),
            Some("sha256:abc123".to_string())
        );
        assert_eq!(opts.checksum_for_target(&target("linux-x64")), None);
    }

    #[test]
    fn test_lock_url_for_target_base_template() {
        let mut opts = IndexMap::new();
        opts.insert(
            "url".to_string(),
            toml::Value::String(
                "s3://bucket/tools/xtool-{{ version }}-{{ os() }}-{{ arch() }}.tar.gz".to_string(),
            ),
        );
        let tool_opts = ToolVersionOptions {
            opts: opts.into(),
            ..Default::default()
        };
        let backend = s3_test_backend();
        let tv = s3_test_tv("2.1.0", tool_opts);
        let raw_opts = tv.request.options();
        let opts = S3Options::new(&raw_opts);

        // os()/arch() render for the target platform, not the host
        assert_eq!(
            backend.lock_url_for_target(&opts, &tv, &target("linux-arm64")),
            Some("s3://bucket/tools/xtool-2.1.0-linux-arm64.tar.gz".to_string())
        );
        assert_eq!(
            backend.lock_url_for_target(&opts, &tv, &target("macos-x64")),
            Some("s3://bucket/tools/xtool-2.1.0-macos-x64.tar.gz".to_string())
        );
    }

    #[test]
    fn test_s3_url_parse_basic() {
        let url = S3Url::parse("s3://my-bucket/path/to/file.tar.gz").unwrap();
        assert_eq!(url.bucket, "my-bucket");
        assert_eq!(url.key, "path/to/file.tar.gz");
    }

    #[test]
    fn test_s3_url_parse_with_query_params() {
        // Query params are parsed but region/endpoint come from tool options
        let url = S3Url::parse("s3://my-bucket/path/to/file.tar.gz?region=us-west-2").unwrap();
        assert_eq!(url.bucket, "my-bucket");
        assert_eq!(url.key, "path/to/file.tar.gz");
    }

    #[test]
    fn test_s3_url_parse_root_key() {
        let url = S3Url::parse("s3://bucket/file.tar.gz").unwrap();
        assert_eq!(url.bucket, "bucket");
        assert_eq!(url.key, "file.tar.gz");
    }

    #[test]
    fn test_s3_url_parse_deep_path() {
        let url = S3Url::parse("s3://bucket/path/to/mytool-1.0.0.tar.gz").unwrap();
        assert_eq!(url.bucket, "bucket");
        assert_eq!(url.key, "path/to/mytool-1.0.0.tar.gz");
    }

    #[test]
    fn test_s3_url_invalid_scheme() {
        let result = S3Url::parse("https://bucket/path");
        assert!(result.is_err());
    }

    #[test]
    fn test_s3_url_missing_bucket() {
        let result = S3Url::parse("s3:///path/to/file");
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_write_body_stops_at_max_bytes() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("versions_manifest");
        let s3_url = S3Url::parse("s3://bucket/tools/versions.txt").unwrap();
        let body = || ByteStream::from_static(b"1.0.0\n1.1.0\n");

        // at the limit, and with none
        for max_bytes in [Some(12), None] {
            write_body(body(), &s3_url, &path, max_bytes, None)
                .await
                .unwrap();
            assert_eq!(std::fs::read(&path).unwrap(), b"1.0.0\n1.1.0\n");
        }

        let err = write_body(body(), &s3_url, &path, Some(11), None)
            .await
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            "s3://bucket/tools/versions.txt is larger than 11 B"
        );
    }

    /// A client for a local S3-compatible endpoint, with fixed credentials so no
    /// credential provider runs.
    fn s3_test_client(endpoint: &str) -> S3Client {
        let config = aws_sdk_s3::Config::builder()
            .behavior_version(BehaviorVersion::latest())
            .region(aws_sdk_s3::config::Region::new("us-east-1"))
            .credentials_provider(aws_sdk_s3::config::Credentials::new(
                "test", "test", None, None, "test",
            ))
            .retry_config(aws_sdk_s3::config::retry::RetryConfig::disabled())
            .endpoint_url(endpoint)
            .force_path_style(true)
            .build();
        S3Client::from_conf(config)
    }

    /// A `ListObjectsV2` page that says more follow, after `next_token` if it names one.
    fn s3_test_truncated_page(next_token: Option<&str>) -> String {
        let next_token = next_token
            .map(|token| format!("<NextContinuationToken>{token}</NextContinuationToken>"))
            .unwrap_or_default();
        format!(
            "<ListBucketResult><Contents><Key>tools/xtool-1.0.0</Key></Contents>\
             <IsTruncated>true</IsTruncated>{next_token}</ListBucketResult>"
        )
    }

    // An endpoint that answers every page with `IsTruncated` and a token is followed for at
    // most MAX_LISTING_PAGES pages.
    #[tokio::test]
    async fn test_fetch_versions_from_listing_is_bounded() {
        let mut server = mockito::Server::new_async().await;
        let pages = server
            .mock("GET", "/bucket/")
            .match_query(mockito::Matcher::Any)
            .with_status(200)
            .with_header("content-type", "application/xml")
            .with_body(s3_test_truncated_page(Some("next")))
            .expect(MAX_LISTING_PAGES)
            .create_async()
            .await;

        let err = s3_test_backend()
            .fetch_versions_from_listing(
                &s3_test_client(&server.url()),
                "bucket",
                "tools/xtool-",
                "xtool-([0-9.]+)",
            )
            .await
            .unwrap_err()
            .to_string();
        pages.assert_async().await;
        assert!(
            err.contains(&format!("did not finish after {MAX_LISTING_PAGES} pages")),
            "{err}"
        );
    }

    // A truncated page with no token would restart the listing from its first page.
    #[tokio::test]
    async fn test_fetch_versions_from_listing_rejects_truncated_page_without_token() {
        let mut server = mockito::Server::new_async().await;
        let page = server
            .mock("GET", "/bucket/")
            .match_query(mockito::Matcher::Any)
            .with_status(200)
            .with_header("content-type", "application/xml")
            .with_body(s3_test_truncated_page(None))
            .expect(1)
            .create_async()
            .await;

        let err = s3_test_backend()
            .fetch_versions_from_listing(
                &s3_test_client(&server.url()),
                "bucket",
                "tools/xtool-",
                "xtool-([0-9.]+)",
            )
            .await
            .unwrap_err()
            .to_string();
        page.assert_async().await;
        assert!(err.contains("is truncated without a token"), "{err}");
    }

    // A listing that ends is read across its pages, each version once.
    #[tokio::test]
    async fn test_fetch_versions_from_listing_follows_pages() {
        let mut server = mockito::Server::new_async().await;
        // mockito answers with the first mock that matches, so the second page goes first.
        let last = server
            .mock("GET", "/bucket/")
            .match_query(mockito::Matcher::UrlEncoded(
                "continuation-token".into(),
                "p2".into(),
            ))
            .with_status(200)
            .with_header("content-type", "application/xml")
            .with_body(
                "<ListBucketResult><Contents><Key>tools/xtool-1.0.0</Key></Contents>\
                 <Contents><Key>tools/xtool-1.1.0</Key></Contents>\
                 <IsTruncated>false</IsTruncated></ListBucketResult>",
            )
            .expect(1)
            .create_async()
            .await;
        let first = server
            .mock("GET", "/bucket/")
            .match_query(mockito::Matcher::Any)
            .with_status(200)
            .with_header("content-type", "application/xml")
            .with_body(s3_test_truncated_page(Some("p2")))
            .expect(1)
            .create_async()
            .await;

        let versions = s3_test_backend()
            .fetch_versions_from_listing(
                &s3_test_client(&server.url()),
                "bucket",
                "tools/xtool-",
                "xtool-([0-9.]+)",
            )
            .await
            .unwrap();
        assert_eq!(versions, ["1.0.0", "1.1.0"]);
        first.assert_async().await;
        last.assert_async().await;
    }

    #[tokio::test]
    async fn test_download_object_refuses_an_object_over_max_bytes() {
        let body = "1.0.0\n1.1.0\n";
        let mut server = mockito::Server::new_async().await;
        // One reply announces its length, the other is chunked and does not.
        let sized = server
            .mock("GET", "/bucket/sized.txt")
            .match_query(mockito::Matcher::Any)
            .with_status(200)
            .with_body(body)
            .expect(3)
            .create_async()
            .await;
        let chunked = server
            .mock("GET", "/bucket/chunked.txt")
            .match_query(mockito::Matcher::Any)
            .with_status(200)
            .with_chunked_body(move |w| w.write_all(body.as_bytes()))
            .expect(3)
            .create_async()
            .await;
        let client = s3_test_client(&server.url());
        let backend = s3_test_backend();
        let tmp = tempfile::tempdir().unwrap();

        for key in ["sized.txt", "chunked.txt"] {
            let s3_url = S3Url::parse(&format!("s3://bucket/{key}")).unwrap();
            let dest = tmp.path().join(key);
            for max_bytes in [Some(12), None] {
                backend
                    .download_object(&client, &s3_url, &dest, max_bytes, None)
                    .await
                    .unwrap();
                assert_eq!(std::fs::read_to_string(&dest).unwrap(), body);
            }
            std::fs::remove_file(&dest).unwrap();
            let err = backend
                .download_object(&client, &s3_url, &dest, Some(11), None)
                .await
                .unwrap_err();
            assert_eq!(
                err.to_string(),
                format!("s3://bucket/{key} is larger than 11 B")
            );
            assert!(!dest.exists());
            assert!(!dest.with_extension("tmp").exists());
        }
        sized.assert_async().await;
        chunked.assert_async().await;
    }

    // The message `--dry-run` now produces before claiming it would install. Built by
    // `missing_url_error` so the dry-run check and the install itself cannot drift apart; this
    // asserts both of its branches, which are two different mistakes with two different fixes.
    #[test]
    fn missing_url_error_names_this_platform_and_the_ones_declared() {
        let backend = s3_test_backend();

        let declared = crate::toolset::parse_tool_options("platforms_linux_x64_url=s3://bucket/t");
        let declared = S3Options::new(&declared);
        let err = backend.missing_url_error(&declared).to_string();
        assert!(err.contains(&backend.get_platform_key()), "{err}");
        assert!(err.contains("linux-x64"), "{err}");

        // A tool that declares no URL anywhere is a different mistake: there is no list to print
        // and nothing platform-specific to say.
        let none = ToolVersionOptions::default();
        let none = S3Options::new(&none);
        assert_eq!(
            backend.missing_url_error(&none).to_string(),
            "S3 backend requires 'url' option"
        );
    }

    #[test]
    fn test_stage_download_is_never_the_destination() {
        let dir = tempfile::tempdir().unwrap();
        for name in [
            "tool.tar.gz",
            "tool.tmp",
            "tool",
            ".tool.tmp",
            ".mise-s3-x.tmp",
        ] {
            let dest = dir.path().join(name);
            let staged = stage_download(&dest).unwrap();
            let staging_dir = staged.dir.path().to_path_buf();
            assert_ne!(staging_dir, dest);
            assert_eq!(staging_dir.parent(), dest.parent());
            assert!(staging_dir.is_dir());
            assert_eq!(staged.file.parent(), Some(staging_dir.as_path()));
            // A download that fails drops the staging directory, which removes
            // it with the partial file and leaves nothing under the
            // destination's name.
            std::fs::write(&staged.file, "partial").unwrap();
            drop(staged);
            assert!(!staging_dir.exists());
            assert!(!dest.exists());
        }
        let dest = dir.path().join("tool.tmp");
        let staged = stage_download(&dest).unwrap();
        let staging_dir = staged.dir.path().to_path_buf();
        std::fs::write(&staged.file, "done").unwrap();
        persist_download(staged, &dest).unwrap();
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "done");
        assert!(!staging_dir.exists());
    }

    #[test]
    fn test_persist_download_removes_the_staged_file_when_the_rename_fails() {
        let dir = tempfile::tempdir().unwrap();
        // A directory with an entry in it cannot be replaced by a file.
        let dest = dir.path().join("tool.tar.gz");
        std::fs::create_dir_all(dest.join("inside")).unwrap();
        let staged = stage_download(&dest).unwrap();
        let staging_dir = staged.dir.path().to_path_buf();
        std::fs::write(&staged.file, "done").unwrap();
        assert!(persist_download(staged, &dest).is_err());
        assert!(!staging_dir.exists());
        assert!(dest.join("inside").is_dir());
    }

    #[test]
    fn test_stage_download_fits_beside_a_long_name() {
        // 255 bytes is the longest name common filesystems allow; a staging
        // name built from it would not fit.
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join(format!("{}.tar.gz", "a".repeat(248)));
        assert_eq!(dest.file_name().unwrap().len(), 255);
        let staged = stage_download(&dest).unwrap();
        assert!(staged.dir.path().file_name().unwrap().len() < 32);
        std::fs::write(&staged.file, "done").unwrap();
        persist_download(staged, &dest).unwrap();
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "done");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_staged_download_is_private_until_it_is_in_place() {
        use std::os::unix::fs::PermissionsExt;
        let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("tool.tar.gz");
        let staged = stage_download(&dest).unwrap();
        // Other users cannot reach the partial download.
        assert_eq!(mode(staged.dir.path()), 0o700);
        // `write_body` creates the file the way it always has, so once it is
        // in place it has the mode any new file beside it gets.
        tokio::fs::File::create(&staged.file).await.unwrap();
        persist_download(staged, &dest).unwrap();
        let reference = dir.path().join("reference");
        std::fs::write(&reference, "").unwrap();
        assert_eq!(mode(&dest), mode(&reference));
    }
}
