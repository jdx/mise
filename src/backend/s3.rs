//! S3 backend for mise - downloads tools from Amazon S3 or S3-compatible storage
//!
//! S3 backend requires experimental mode to be enabled.
//!
//! This backend allows installing tools from private or public S3 buckets.
//! It supports version discovery via S3 object listing or manifest files.
//!
//! ## Configuration
//!
//! ```toml
//! [tools]
//! mytool = { version = "1.0.0", backend = "s3", url = "s3://bucket/tools/mytool-{version}.tar.gz" }
//!
//! # With version discovery from manifest
//! [tools.mytool]
//! backend = "s3"
//! version = "latest"
//! url = "s3://bucket/tools/mytool-{version}.tar.gz"
//! version_list_url = "s3://bucket/tools/versions.json"
//!
//! # With S3 listing-based discovery
//! [tools.mytool]
//! backend = "s3"
//! version = "latest"
//! url = "s3://bucket/tools/mytool-{version}.tar.gz"
//! version_prefix = "tools/mytool-"
//! version_regex = "mytool-([0-9.]+)"
//!
//! # With custom endpoint (MinIO, etc.)
//! [tools.mytool]
//! backend = "s3"
//! url = "s3://bucket/tools/mytool-{version}.tar.gz"
//! endpoint = "https://minio.internal:9000"
//! region = "us-east-1"
//! ```

/// S3 backend is experimental and requires `experimental = true` in settings
pub const EXPERIMENTAL: bool = true;

use crate::backend::backend_type::BackendType;
use crate::backend::static_helpers::{
    get_filename_from_url, install_artifact, lookup_with_fallback, template_string, verify_artifact,
};
use crate::backend::version_list;
use crate::backend::{Backend, VersionInfo};
use crate::cli::args::BackendArg;
use crate::config::{Config, Settings};
use crate::file;
use crate::hash;
use crate::install_context::InstallContext;
use crate::toolset::{ToolVersion, ToolVersionOptions};
use crate::ui::progress_report::SingleReport;
use async_trait::async_trait;
use aws_config::BehaviorVersion;
use aws_sdk_s3::Client as S3Client;
use eyre::{Result, bail, eyre};
use regex::Regex;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::OnceCell;
use url::Url;

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
pub struct S3Backend {
    ba: Arc<BackendArg>,
    /// Cached S3 client, lazily initialized
    client: OnceCell<S3Client>,
}

impl S3Backend {
    pub fn from_arg(ba: BackendArg) -> Self {
        Self {
            ba: Arc::new(ba),
            client: OnceCell::new(),
        }
    }

    /// Get or create the S3 client
    async fn get_client(&self, opts: &ToolVersionOptions) -> Result<&S3Client> {
        self.client
            .get_or_try_init(|| async {
                let region = lookup_with_fallback(opts, "region");
                let endpoint = lookup_with_fallback(opts, "endpoint");
                create_s3_client(region.as_deref(), endpoint.as_deref()).await
            })
            .await
    }

    /// Get option value with platform-specific fallback
    fn get_opt(opts: &ToolVersionOptions, key: &str) -> Option<String> {
        lookup_with_fallback(opts, key)
    }

    /// Resolve the download URL from options and version
    fn resolve_url(&self, tv: &ToolVersion, opts: &ToolVersionOptions) -> Result<String> {
        let url_template = Self::get_opt(opts, "url").ok_or_else(|| {
            eyre!(
                "S3 backend requires 'url' option. Example: url = \"s3://bucket/tool-{{version}}.tar.gz\""
            )
        })?;

        Ok(template_string(&url_template, tv))
    }

    /// Download an S3 object to a local file
    async fn download_object(
        &self,
        client: &S3Client,
        s3_url: &S3Url,
        dest: &Path,
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
        if let Some(pr) = pr {
            pr.set_length(content_length);
        }

        // Stream the body to the file
        let body = resp
            .body
            .collect()
            .await
            .map_err(|e| eyre!("Failed to read S3 response body: {e}"))?;
        let bytes = body.into_bytes();

        // Write to temp file then rename for atomic operation
        let tmp_path = dest.with_extension("tmp");
        file::write(&tmp_path, &bytes)?;
        std::fs::rename(&tmp_path, dest)?;

        if let Some(pr) = pr {
            pr.set_position(content_length);
        }

        Ok(())
    }

    /// Fetch versions from a manifest file URL
    async fn fetch_versions_from_manifest(
        &self,
        client: &S3Client,
        manifest_url: &str,
        opts: &ToolVersionOptions,
    ) -> Result<Vec<String>> {
        let s3_url = S3Url::parse(manifest_url)?;

        // Download manifest to temp location
        let tmp_dir = tempfile::tempdir()?;
        let tmp_path = tmp_dir.path().join("versions_manifest");
        self.download_object(client, &s3_url, &tmp_path, None)
            .await?;

        // Read and parse the manifest
        let content = file::read_to_string(&tmp_path)?;
        let regex = Self::get_opt(opts, "version_regex");
        let json_path = Self::get_opt(opts, "version_json_path");
        let version_expr = Self::get_opt(opts, "version_expr");

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
        let mut continuation_token: Option<String> = None;

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
                                && !versions.contains(&v)
                            {
                                versions.push(v);
                            }
                        }
                    }
                }
            }

            if response.is_truncated == Some(true) {
                continuation_token = response.next_continuation_token;
            } else {
                break;
            }
        }

        Ok(versions)
    }

    /// Fetch versions using the configured method (manifest or listing)
    async fn fetch_versions(&self, config: &Arc<Config>) -> Result<Vec<String>> {
        let opts = config.get_tool_opts(&self.ba).await?.unwrap_or_default();

        // Try manifest-based version discovery first
        if let Some(manifest_url) = Self::get_opt(&opts, "version_list_url") {
            let client = self.get_client(&opts).await?;
            return self
                .fetch_versions_from_manifest(client, &manifest_url, &opts)
                .await;
        }

        // Try S3 listing-based version discovery
        if let Some(version_prefix) = Self::get_opt(&opts, "version_prefix") {
            let version_regex = Self::get_opt(&opts, "version_regex")
                .unwrap_or_else(|| r"([0-9]+\.[0-9]+\.[0-9]+)".to_string());

            // Extract bucket from url option
            let url_template = Self::get_opt(&opts, "url")
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
        let lockfile_enabled = settings.lockfile;

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
pub fn install_time_option_keys() -> Vec<String> {
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
    fn get_type(&self) -> BackendType {
        BackendType::S3
    }

    fn ba(&self) -> &Arc<BackendArg> {
        &self.ba
    }

    async fn install_operation_count(&self, tv: &ToolVersion, _ctx: &InstallContext) -> usize {
        let opts = tv.request.options();
        super::http_install_operation_count(
            Self::get_opt(&opts, "checksum").is_some(),
            &self.get_platform_key(),
            tv,
        )
    }

    async fn _list_remote_versions(&self, config: &Arc<Config>) -> Result<Vec<VersionInfo>> {
        let versions = self.fetch_versions(config).await?;
        Ok(versions
            .into_iter()
            .map(|v| VersionInfo {
                version: v,
                ..Default::default()
            })
            .collect())
    }

    async fn install_version_(
        &self,
        ctx: &InstallContext,
        mut tv: ToolVersion,
    ) -> Result<ToolVersion> {
        Settings::get().ensure_experimental("s3 backend")?;
        let opts = tv.request.options();

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
        let lockfile_enabled = settings.lockfile;
        let has_lockfile_checksum = tv
            .lock_platforms
            .get(&platform_key)
            .and_then(|p| p.checksum.as_ref())
            .is_some();

        // Download from S3
        ctx.pr.set_message(format!("download {filename}"));
        file::create_dir_all(tv.download_path())?;
        self.download_object(client, &s3_url, &file_path, Some(ctx.pr.as_ref()))
            .await?;

        // Verify artifact (checksum/size from options)
        if Self::get_opt(&opts, "checksum").is_some() {
            ctx.pr.next_operation();
        }
        verify_artifact(&tv, &file_path, &opts, Some(ctx.pr.as_ref()))?;

        // Verify/generate lockfile checksum (before extraction for security)
        if lockfile_enabled || has_lockfile_checksum {
            ctx.pr.next_operation();
        }
        self.verify_checksum(ctx, &mut tv, &file_path)?;

        // Extract and install
        ctx.pr.next_operation();
        ctx.pr.set_message("extract".into());
        install_artifact(&tv, &file_path, &opts, Some(ctx.pr.as_ref()))?;

        Ok(tv)
    }

    async fn list_bin_paths(
        &self,
        _config: &Arc<Config>,
        tv: &ToolVersion,
    ) -> Result<Vec<PathBuf>> {
        let opts = tv.request.options();

        // Check for explicit bin_path
        if let Some(bin_path_template) = lookup_with_fallback(&opts, "bin_path") {
            let bin_path = template_string(&bin_path_template, tv);
            return Ok(vec![tv.install_path().join(bin_path)]);
        }

        // Check for bin directory
        let bin_dir = tv.install_path().join("bin");
        if bin_dir.exists() {
            return Ok(vec![bin_dir]);
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
            Ok(vec![tv.install_path()])
        } else {
            Ok(paths)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
