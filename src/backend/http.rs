use crate::backend::Backend;
use crate::backend::VersionInfo;
use crate::backend::backend_type::BackendType;
use crate::backend::options::BackendOptions;
use crate::backend::platform_target::PlatformTarget;
use crate::backend::prepared_install::{PreparedInstall, PreparedInstallPlan};
use crate::backend::runtime_path_for_install_path;
use crate::backend::static_helpers::{
    apply_rename_exe, clean_binary_name, ensure_plain_bin_name, ensure_safe_relative_bin_path,
    eval_checksum_expr, fetch_checksum_from_file, fetch_checksum_from_shasums,
    get_filename_from_url, rename_binary_name, shasums_has_entries, template_string,
    template_string_for_target,
};
use crate::backend::version_list;
use crate::cli::args::BackendArg;
use crate::config::Config;
use crate::config::Settings;
use crate::http::HTTP;
use crate::install_context::InstallContext;
use crate::lockfile::PlatformInfo;
use crate::runtime_symlinks::is_runtime_symlink;
use crate::toolset::ToolRequest;
use crate::toolset::ToolVersion;
use crate::toolset::ToolVersionOptions;
use crate::ui::progress_report::SingleReport;
use crate::{dirs, file, hash};
use async_trait::async_trait;
use eyre::{Result, WrapErr, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::fmt::Debug;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tempfile::TempDir;

// Constants
const HTTP_TARBALLS_DIR: &str = "http-tarballs";
const METADATA_FILE: &str = "metadata.json";

/// Metadata stored alongside cached extractions
#[derive(Debug, Serialize, Deserialize)]
struct CacheMetadata {
    url: String,
    checksum: Option<String>,
    size: u64,
    extracted_at: u64,
    platform: String,
}

/// Describes what type of content was extracted to cache
#[derive(Debug, Clone)]
enum ExtractionType {
    /// A single raw file (not an archive) with its filename
    RawFile { filename: String },
    /// An archive (tarball, zip, etc.) that was extracted
    Archive,
}

/// Information about a downloaded file's format
#[derive(Debug)]
struct FileInfo {
    /// Path with effective extension (after applying format option)
    effective_path: PathBuf,
    /// File extension
    extension: String,
    /// Detected archive format
    format: file::ExtractionFormat,
    /// Whether this is a compressed single binary (not a tar archive)
    is_compressed_binary: bool,
}

#[derive(Debug)]
struct CachePlan {
    key: String,
    file_info: FileInfo,
    strip_components: usize,
}

impl FileInfo {
    /// Analyze a file path and options to determine format information
    fn new(file_path: &Path, opts: &PreparedHttpInstall) -> Self {
        // Apply format config to determine effective extension
        let effective_path = if let Some(added_ext) = opts.format() {
            let mut path = file_path.to_path_buf();
            let current_ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            let new_ext = if current_ext.is_empty() {
                added_ext.to_string()
            } else {
                format!("{}.{}", current_ext, added_ext)
            };
            path.set_extension(new_ext);
            path
        } else {
            file_path.to_path_buf()
        };

        let file_name = effective_path.file_name().unwrap().to_string_lossy();
        let format = file::ExtractionFormat::from_file_name(&file_name);

        let extension = format.extension().unwrap_or_else(|| {
            effective_path
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string()
        });

        let is_compressed_binary = !format.is_archive() && format != file::ExtractionFormat::Raw;

        Self {
            effective_path,
            extension,
            format,
            is_compressed_binary,
        }
    }

    /// Get the filename portion of the effective path
    fn file_name(&self) -> String {
        self.effective_path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string()
    }

    /// Get the decompressed name (for compressed binaries)
    fn decompressed_name(&self) -> String {
        self.file_name()
            .trim_end_matches(&format!(".{}", self.extension))
            .to_string()
    }
}

#[derive(Debug, Clone)]
pub struct HttpBackend {
    ba: Arc<BackendArg>,
}

#[derive(Debug, Clone, Copy)]
struct HttpOptions<'a> {
    values: BackendOptions<'a>,
}

#[derive(Debug)]
struct PreparedHttpInstall {
    target: String,
    url: reqwest::Url,
    filename: String,
    lock_checksum: Option<PreparedChecksum>,
    lock_size: Option<u64>,
    configured_checksum: Option<PreparedChecksum>,
    configured_size: Option<u64>,
    lockfile_enabled: bool,
    format: Option<String>,
    strip_components: Option<usize>,
    bin: Option<String>,
    rename_exe: Option<toml::Value>,
    bin_path: Option<String>,
}

#[derive(Debug)]
struct PreparedHttpInstallPlan {
    backend: HttpBackend,
    spec: PreparedHttpInstall,
    staged_dir: TempDir,
    staged_file: PathBuf,
    staged_cache: PathBuf,
    cache_plan: CachePlan,
    extraction_type: ExtractionType,
    lock_checksum: Option<String>,
    lock_size: Option<u64>,
}

#[async_trait]
impl PreparedInstallPlan for PreparedHttpInstallPlan {
    async fn execute(
        self: Box<Self>,
        ctx: &InstallContext,
        tv: ToolVersion,
    ) -> Result<ToolVersion> {
        let Self {
            backend,
            spec,
            staged_dir,
            staged_file,
            staged_cache,
            cache_plan,
            extraction_type,
            lock_checksum,
            lock_size,
        } = *self;
        let tv = backend.install_prepared_http(
            ctx,
            tv,
            &spec,
            &staged_file,
            &staged_cache,
            &cache_plan,
            extraction_type,
            lock_checksum,
            lock_size,
        )?;
        drop(staged_dir);
        Ok(tv)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PreparedChecksum {
    algorithm: &'static str,
    digest: String,
}

impl PreparedChecksum {
    fn parse(value: &str) -> Result<Self> {
        let (algorithm, digest) = value
            .split_once(':')
            .ok_or_else(|| eyre::eyre!("Invalid checksum format: {value}"))?;
        let (algorithm, expected_len): (&'static str, usize) = match algorithm {
            "md5" => ("md5", 32),
            "sha1" => ("sha1", 40),
            "sha256" => ("sha256", 64),
            "blake3" => ("blake3", 64),
            "sha512" => ("sha512", 128),
            _ => bail!("Unknown checksum algorithm: {algorithm}"),
        };
        if digest.len() != expected_len || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            bail!("Invalid {algorithm} checksum: expected {expected_len} hexadecimal characters");
        }
        Ok(Self {
            algorithm,
            digest: digest.to_ascii_lowercase(),
        })
    }

    fn verify(&self, file: &Path, pr: Option<&dyn SingleReport>) -> Result<()> {
        hash::ensure_checksum(file, &self.digest, pr, self.algorithm)
    }
}

impl fmt::Display for PreparedChecksum {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.algorithm, self.digest)
    }
}

impl<'a> HttpOptions<'a> {
    fn new(raw: &'a ToolVersionOptions) -> Self {
        Self {
            values: BackendOptions::new(raw),
        }
    }

    fn checksum(&self) -> Option<String> {
        self.values.platform_string("checksum")
    }

    fn bin_path(&self) -> Option<String> {
        self.values.platform_string("bin_path")
    }

    fn checksum_expr(&self) -> Option<&'a str> {
        self.values.str("checksum_expr")
    }

    // Target-aware accessors for cross-platform `mise lock`. These resolve
    // `platforms.<key>.<opt>` for an arbitrary target rather than the host.
    fn url_for_target(&self, target: &PlatformTarget) -> Option<String> {
        self.values.platform_string_for_target("url", target)
    }

    fn checksum_for_target(&self, target: &PlatformTarget) -> Option<String> {
        self.values.platform_string_for_target("checksum", target)
    }

    fn checksum_url_for_target(&self, target: &PlatformTarget) -> Option<String> {
        self.values
            .platform_string_for_target("checksum_url", target)
    }

    fn format_for_target(&self, target: &PlatformTarget) -> Option<String> {
        self.values.platform_string_for_target("format", target)
    }

    fn strip_components_for_target(&self, target: &PlatformTarget) -> Option<String> {
        self.values
            .platform_string_for_target("strip_components", target)
    }

    fn rename_exe_for_target(&self, target: &PlatformTarget) -> Option<String> {
        self.values.platform_string_for_target("rename_exe", target)
    }

    fn rename_exe_value_for_target(&self, target: &PlatformTarget) -> Option<&'a toml::Value> {
        self.values.platform_value_for_target("rename_exe", target)
    }

    fn bin_for_target(&self, target: &PlatformTarget) -> Option<String> {
        self.values.platform_string_for_target("bin", target)
    }

    fn bin_path_for_target(&self, target: &PlatformTarget) -> Option<String> {
        self.values.platform_string_for_target("bin_path", target)
    }

    fn size_for_target(&self, target: &PlatformTarget) -> Option<String> {
        self.values.platform_string_for_target("size", target)
    }

    fn version_list_url(&self) -> Option<&'a str> {
        self.values.str("version_list_url")
    }

    fn version_regex(&self) -> Option<&'a str> {
        self.values.str("version_regex")
    }

    fn version_json_path(&self) -> Option<&'a str> {
        self.values.str("version_json_path")
    }

    fn version_expr(&self) -> Option<&'a str> {
        self.values.str("version_expr")
    }

    fn url_platforms(&self) -> Vec<String> {
        self.values.available_platforms_with_key("url")
    }
}

impl PreparedHttpInstall {
    fn checksum(&self) -> Option<String> {
        self.lock_checksum
            .as_ref()
            .or(self.configured_checksum.as_ref())
            .map(ToString::to_string)
    }

    fn format(&self) -> Option<&str> {
        self.format.as_deref()
    }

    fn strip_components(&self) -> Option<usize> {
        self.strip_components
    }

    fn bin(&self) -> Option<&str> {
        self.bin.as_deref()
    }

    fn rename_exe(&self) -> Option<&str> {
        self.rename_exe.as_ref().and_then(toml::Value::as_str)
    }

    fn rename_exe_value(&self) -> Option<&toml::Value> {
        self.rename_exe.as_ref()
    }

    fn bin_path(&self) -> Option<&str> {
        self.bin_path.as_deref()
    }
}

impl HttpBackend {
    pub fn from_arg(ba: BackendArg) -> Self {
        Self { ba: Arc::new(ba) }
    }

    // -------------------------------------------------------------------------
    // Cache path helpers
    // -------------------------------------------------------------------------

    /// Get the http-tarballs directory in DATA (survives `mise cache clear`)
    fn tarballs_dir() -> PathBuf {
        dirs::DATA.join(HTTP_TARBALLS_DIR)
    }

    /// Get the path to a specific cache entry
    fn cache_path(&self, cache_key: &str) -> PathBuf {
        Self::tarballs_dir().join(cache_key)
    }

    /// Get the path to the metadata file for a cache entry
    fn metadata_path(&self, cache_key: &str) -> PathBuf {
        self.cache_path(cache_key).join(METADATA_FILE)
    }

    /// Check if a cache entry exists and is valid
    fn is_cached(&self, cache_key: &str) -> bool {
        self.cache_path(cache_key).exists() && self.metadata_path(cache_key).exists()
    }

    // -------------------------------------------------------------------------
    // Cache key generation
    // -------------------------------------------------------------------------

    /// Generate a cache key based on file content and extraction options
    fn cache_key(
        &self,
        file_path: &Path,
        opts: &PreparedHttpInstall,
        strip_components: usize,
    ) -> Result<String> {
        let checksum = hash::file_hash_blake3(file_path, None)?;

        // Include extraction options that affect output structure
        // Note: bin_path is NOT included - handled at symlink time for deduplication
        let mut parts = vec![checksum];

        if let Some(strip) = opts.strip_components() {
            parts.push(format!("strip_{strip}"));
        } else if strip_components > 0 {
            parts.push(format!("strip_{strip_components}"));
        }

        // Include rename_exe in cache key since it modifies the extracted content.
        // Use the raw value so the table form (multi-binary rename) is captured too;
        // `opts.rename_exe()` only stringifies the scalar form. `rename_cache_token`
        // keeps a readable name when it is path-safe and hashes anything else (the
        // table form, or a scalar with path separators / Windows-invalid characters)
        // so nothing unsafe reaches the cache directory name.
        if let Some(rename) = opts.rename_exe_value() {
            parts.push(format!("rename_{}", rename_cache_token(rename)));
            // When rename_exe is used, bin_path affects where the rename happens,
            // so different bin_path values result in different cached content
            if let Some(bin_path) = opts.bin_path() {
                parts.push(format!("binpath_{bin_path}"));
            }
        }

        let key = parts.join("_");
        debug!("Cache key: {}", key);
        Ok(key)
    }

    fn cache_plan(&self, file_path: &Path, opts: &PreparedHttpInstall) -> Result<CachePlan> {
        let file_info = FileInfo::new(file_path, opts);
        let strip_components = self.effective_strip_components(file_path, &file_info, opts)?;
        let key = self.cache_key(file_path, opts, strip_components)?;

        Ok(CachePlan {
            key,
            file_info,
            strip_components,
        })
    }

    fn effective_strip_components(
        &self,
        file_path: &Path,
        file_info: &FileInfo,
        opts: &PreparedHttpInstall,
    ) -> Result<usize> {
        let mut strip_components = opts.strip_components();

        // Auto-detect strip_components=1 for single-directory archives
        if strip_components.is_none()
            && !file_info.is_compressed_binary
            && file_info.format != file::ExtractionFormat::Raw
            && opts.bin_path().is_none()
            && file::should_strip_components(file_path, file_info.format).unwrap_or(false)
        {
            debug!("Auto-detected single directory archive, using strip_components=1");
            strip_components = Some(1);
        }

        Ok(strip_components.unwrap_or(0))
    }

    // -------------------------------------------------------------------------
    // Filename determination
    // -------------------------------------------------------------------------

    /// Determine the destination filename for a raw file or compressed binary.
    /// `bin`/`rename_exe` values are joined onto the extraction directory, so a
    /// path in either (`../evil`, `a/b`) would escape it and is rejected.
    fn dest_filename(
        &self,
        file_path: &Path,
        file_info: &FileInfo,
        opts: &PreparedHttpInstall,
    ) -> Result<String> {
        // Check for explicit bin name first
        if let Some(bin_name) = opts.bin() {
            ensure_safe_relative_bin_path("bin", bin_name)?;
            return Ok(bin_name.to_string());
        }
        if let Some(rename_to) = opts.rename_exe() {
            ensure_plain_bin_name("rename_exe", rename_to)?;
            let source_name = if file_info.is_compressed_binary {
                file_info.decompressed_name()
            } else {
                file_path.file_name().unwrap().to_string_lossy().to_string()
            };
            return Ok(rename_binary_name(&source_name, rename_to));
        }

        // Auto-clean the binary name
        let raw_name = if file_info.is_compressed_binary {
            file_info.decompressed_name()
        } else {
            file_path.file_name().unwrap().to_string_lossy().to_string()
        };

        Ok(clean_binary_name(&raw_name, Some(&self.ba.tool_name)))
    }

    // -------------------------------------------------------------------------
    // Extraction type detection
    // -------------------------------------------------------------------------

    /// Detect extraction type from an existing cache directory
    /// This handles the case where a cache hit occurs but the original extraction
    /// used different options (e.g., different `bin` name)
    fn extraction_type_from_cache(&self, cache_key: &str, file_info: &FileInfo) -> ExtractionType {
        // For archives, we don't need to detect the filename
        if !file_info.is_compressed_binary && file_info.format != file::ExtractionFormat::Raw {
            return ExtractionType::Archive;
        }

        // For raw files, find the actual filename in the cache directory
        let cache_path = self.cache_path(cache_key);
        for entry in xx::file::ls(&cache_path).unwrap_or_default() {
            if let Some(name) = entry.file_name().map(|n| n.to_string_lossy().to_string()) {
                // Skip metadata file
                if name != METADATA_FILE {
                    return ExtractionType::RawFile { filename: name };
                }
            }
        }

        // Fallback: shouldn't happen if cache is valid, but use a sensible default
        ExtractionType::RawFile {
            filename: self.ba.tool_name.clone(),
        }
    }

    // -------------------------------------------------------------------------
    // Extraction
    // -------------------------------------------------------------------------

    /// Extract a single artifact to the given directory
    fn extract_artifact(
        &self,
        tv: &ToolVersion,
        dest: &Path,
        file_path: &Path,
        cache_plan: &CachePlan,
        opts: &PreparedHttpInstall,
        pr: Option<&dyn SingleReport>,
    ) -> Result<ExtractionType> {
        file::create_dir_all(dest)?;

        if cache_plan.file_info.is_compressed_binary {
            self.extract_compressed_binary(dest, file_path, &cache_plan.file_info, opts, pr)
        } else if cache_plan.file_info.format == file::ExtractionFormat::Raw {
            self.extract_raw_file(dest, file_path, &cache_plan.file_info, opts, pr)
        } else {
            self.extract_archive(tv, dest, file_path, cache_plan, opts, pr)
        }
    }

    /// Extract a compressed binary (gz, xz, bz2, zst)
    fn extract_compressed_binary(
        &self,
        dest: &Path,
        file_path: &Path,
        file_info: &FileInfo,
        opts: &PreparedHttpInstall,
        pr: Option<&dyn SingleReport>,
    ) -> Result<ExtractionType> {
        let filename = self.dest_filename(file_path, file_info, opts)?;
        let dest_file = dest.join(&filename);

        // Report extraction progress (no bytes - we don't know total for extraction)
        if let Some(pr) = pr {
            pr.set_message(format!("extract {}", file_info.file_name()));
        }

        file::decompress_file(file_path, &dest_file, file_info.format)?;

        file::make_executable(&dest_file)?;
        Ok(ExtractionType::RawFile { filename })
    }

    /// Extract a raw (uncompressed) file
    fn extract_raw_file(
        &self,
        dest: &Path,
        file_path: &Path,
        file_info: &FileInfo,
        opts: &PreparedHttpInstall,
        pr: Option<&dyn SingleReport>,
    ) -> Result<ExtractionType> {
        let filename = self.dest_filename(file_path, file_info, opts)?;
        let dest_file = dest.join(&filename);

        // Report extraction progress (no bytes - we don't know total for extraction)
        if let Some(pr) = pr {
            pr.set_message(format!("extract {}", file_info.file_name()));
        }

        file::copy(file_path, &dest_file)?;

        file::make_executable(&dest_file)?;
        Ok(ExtractionType::RawFile { filename })
    }

    /// Extract an archive (tar, zip, etc.)
    fn extract_archive(
        &self,
        _tv: &ToolVersion,
        dest: &Path,
        file_path: &Path,
        cache_plan: &CachePlan,
        opts: &PreparedHttpInstall,
        pr: Option<&dyn SingleReport>,
    ) -> Result<ExtractionType> {
        let extract_opts = file::ExtractOptions {
            strip_components: cache_plan.strip_components,
            pr,
            preserve_mtime: false,
        };

        file::extract_archive(file_path, dest, cache_plan.file_info.format, &extract_opts)?;

        // Handle rename_exe option for archives
        if let Some(rename_value) = opts.rename_exe_value() {
            // When bin_path is not explicitly set, auto-detect bin/ subdirectory to match
            // the same logic used by discover_bin_paths() for PATH construction
            let search_dir = if let Some(bin_path) = opts.bin_path() {
                dest.join(&bin_path)
            } else {
                let bin_dir = dest.join("bin");
                if bin_dir.is_dir() {
                    bin_dir
                } else {
                    dest.to_path_buf()
                }
            };
            // rsplit('/') always yields at least one element (the full string if no delimiter)
            let tool_name = self.ba.tool_name.rsplit('/').next().unwrap();
            apply_rename_exe(&search_dir, rename_value, Some(tool_name))?;
        }

        Ok(ExtractionType::Archive)
    }

    /// Write cache metadata file
    fn write_metadata(
        &self,
        cache_path: &Path,
        url: &str,
        checksum: Option<&str>,
        file_path: &Path,
    ) -> Result<()> {
        let metadata = CacheMetadata {
            url: url.to_string(),
            checksum: checksum.map(str::to_string),
            size: file_path.metadata()?.len(),
            extracted_at: SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
            platform: self.get_platform_key(),
        };

        let json = serde_json::to_string_pretty(&metadata)?;
        file::write(cache_path.join(METADATA_FILE), json)?;
        Ok(())
    }

    // -------------------------------------------------------------------------
    // Symlink creation
    // -------------------------------------------------------------------------

    /// Return the single path component used for the HTTP install symlink.
    fn install_version_name(tv: &ToolVersion, cache_key: &str) -> String {
        if tv.version == "latest" {
            Self::content_version_name(cache_key)
        } else if tv.version.is_empty() {
            "_implicit".to_string()
        } else {
            Self::sanitize_install_version_name(&tv.version, tv.tv_pathname())
        }
    }

    /// Return the absolute path where the HTTP install symlink should live.
    fn install_path_for(tv: &ToolVersion, cache_key: &str) -> PathBuf {
        tv.ba()
            .installs_path
            .join(Self::install_version_name(tv, cache_key))
    }

    /// Return the install path later lookups should check for this HTTP tool.
    fn lookup_install_path(tv: &ToolVersion) -> PathBuf {
        if let Some(path) = &tv.install_path {
            return path.clone();
        }
        if tv.version == "latest" {
            tv.install_path()
        } else {
            tv.ba()
                .installs_path
                .join(Self::install_version_name(tv, ""))
        }
    }

    /// Return a deterministic content-derived version name for `latest` installs.
    fn content_version_name(cache_key: &str) -> String {
        let short = &cache_key[..7.min(cache_key.len())];
        if short.is_empty() {
            "_implicit".to_string()
        } else {
            short.to_string()
        }
    }

    /// Sanitize a requested version into a path component without collapsing identities.
    fn sanitize_install_version_name(raw_version: &str, version_name: String) -> String {
        let sanitized = match version_name.replace('\\', "-").as_str() {
            "." => "_".to_string(),
            ".." => "__".to_string(),
            name => name.to_string(),
        };
        if sanitized == raw_version {
            sanitized
        } else {
            let hash = hash::hash_sha256_to_str(raw_version);
            format!("{}-{}", sanitized, &hash[..7])
        }
    }

    /// Create install symlink(s) from install directory to cache
    fn create_install_symlink(
        &self,
        tv: &ToolVersion,
        cache_key: &str,
        extraction_type: &ExtractionType,
        opts: &PreparedHttpInstall,
    ) -> Result<()> {
        let cache_path = self.cache_path(cache_key);

        // Determine version name for install path
        let install_path = Self::install_path_for(tv, cache_key);

        // Clean up existing install
        if install_path.exists() {
            file::remove_all(&install_path)?;
        }
        if let Some(parent) = install_path.parent() {
            file::create_dir_all(parent)?;
        }

        // Handle raw files with bin_path specially for deduplication
        if let ExtractionType::RawFile { filename } = extraction_type
            && let Some(bin_path) = opts.bin_path()
        {
            let dest_dir = install_path.join(&bin_path);
            file::create_dir_all(&dest_dir)?;

            let cached_file = cache_path.join(filename);
            let install_file = dest_dir.join(filename);
            file::make_symlink(&cached_file, &install_file)?;
            return Ok(());
        }

        // Default: symlink entire install path to cache
        file::make_symlink(&cache_path, &install_path)?;
        Ok(())
    }

    /// Create additional symlink for latest versions
    fn create_version_alias_symlink(&self, tv: &ToolVersion, cache_key: &str) -> Result<()> {
        if tv.version != "latest" {
            return Ok(());
        }

        let content_version = Self::content_version_name(cache_key);
        let original_path = tv.ba().installs_path.join(&tv.version);
        let content_path = tv.ba().installs_path.join(&content_version);

        if original_path.exists() {
            file::remove_all(&original_path)?;
        }
        if let Some(parent) = original_path.parent() {
            file::create_dir_all(parent)?;
        }

        file::make_symlink(&content_path, &original_path)?;
        Ok(())
    }

    // -------------------------------------------------------------------------
    // Checksum verification
    // -------------------------------------------------------------------------

    fn verify_staged_artifact(
        &self,
        ctx: &InstallContext,
        file_path: &Path,
        spec: &PreparedHttpInstall,
    ) -> Result<(Option<String>, Option<u64>)> {
        if let Some(checksum) = &spec.configured_checksum {
            ctx.pr.next_operation();
            checksum.verify(file_path, Some(ctx.pr.as_ref()))?;
        }
        if let Some(expected_size) = spec.configured_size {
            let actual_size = file_path.metadata()?.len();
            if actual_size != expected_size {
                let filename = file_path.file_name().unwrap().to_string_lossy();
                bail!("Size mismatch for {filename}: expected {expected_size}, got {actual_size}");
            }
        }

        let filename = file_path.file_name().unwrap().to_string_lossy();
        if spec.lockfile_enabled || spec.lock_checksum.is_some() {
            ctx.pr.next_operation();
        }
        let lock_checksum = if let Some(checksum) = &spec.lock_checksum {
            ctx.pr.set_message(format!("checksum {filename}"));
            checksum.verify(file_path, Some(ctx.pr.as_ref()))?;
            Some(checksum.to_string())
        } else if spec.lockfile_enabled {
            ctx.pr.set_message(format!("generate checksum {filename}"));
            let h = hash::file_hash_blake3(file_path, Some(ctx.pr.as_ref()))?;
            Some(format!("blake3:{h}"))
        } else {
            None
        };

        let lock_size = if let Some(expected_size) = spec.lock_size {
            ctx.pr.set_message(format!("verify size {filename}"));
            let actual_size = file_path.metadata()?.len();
            if actual_size != expected_size {
                bail!("Size mismatch for {filename}: expected {expected_size}, got {actual_size}");
            }
            Some(expected_size)
        } else if spec.lockfile_enabled {
            Some(file_path.metadata()?.len())
        } else {
            None
        };

        Ok((lock_checksum, lock_size))
    }

    fn apply_lock_contract(
        &self,
        tv: &mut ToolVersion,
        spec: &PreparedHttpInstall,
        checksum: Option<String>,
        size: Option<u64>,
    ) {
        let platform_info = tv.lock_platforms.entry(spec.target.clone()).or_default();
        platform_info.url = Some(spec.url.to_string());
        if let Some(checksum) = checksum {
            platform_info.checksum = Some(checksum);
        }
        if let Some(size) = size {
            platform_info.size = Some(size);
        }
    }

    // -------------------------------------------------------------------------
    // Version listing
    // -------------------------------------------------------------------------

    /// Fetch versions from version_list_url if configured
    async fn fetch_versions(&self, config: &Arc<Config>) -> Result<Vec<String>> {
        let raw_opts = config.get_tool_opts_with_overrides(&self.ba).await?;
        let opts = HttpOptions::new(&raw_opts);

        let url = match opts.version_list_url() {
            Some(url) => url.to_string(),
            None => return Ok(vec![]),
        };

        let regex = opts.version_regex();
        let json_path = opts.version_json_path();
        let version_expr = opts.version_expr();

        version_list::fetch_versions(&url, regex, json_path, version_expr).await
    }

    // -------------------------------------------------------------------------
    // Cross-platform lock resolution
    // -------------------------------------------------------------------------

    fn prepare_http_target(
        &self,
        tv: &ToolVersion,
        target: &PlatformTarget,
        locked: Option<&PlatformInfo>,
    ) -> Result<PreparedHttpInstall> {
        let raw_opts = tv.request.options();
        let opts = HttpOptions::new(&raw_opts);
        let locked_url = locked.and_then(|info| info.url.clone());
        let configured_url = self.lock_url_for_target(&opts, tv, target);
        let replaying = locked_url.is_some();
        let url_value = locked_url
            .clone()
            .or_else(|| configured_url.clone())
            .ok_or_else(|| {
                let available = opts.url_platforms();
                if available.is_empty() {
                    eyre::eyre!("Http backend requires 'url' option")
                } else {
                    eyre::eyre!(
                        "No URL for platform {}. Available: {}. Provide 'url' or add 'platforms.{}.url'",
                        target.to_key(),
                        available.join(", "),
                        target.to_key()
                    )
                }
            })?;
        let url = reqwest::Url::parse(&url_value)
            .wrap_err_with(|| format!("Invalid HTTP install URL: {url_value}"))?;
        if !matches!(url.scheme(), "http" | "https") {
            bail!(
                "Unsupported HTTP install URL scheme '{}': {url}",
                url.scheme()
            );
        }
        let filename = get_filename_from_url(url.as_str());
        if filename.is_empty() {
            bail!("HTTP install URL has no artifact filename: {url}");
        }
        ensure_plain_bin_name("HTTP artifact filename", &filename)?;

        let strip_components = opts
            .strip_components_for_target(target)
            .map(|strip| {
                strip
                    .parse::<usize>()
                    .map_err(|_| eyre::eyre!("Invalid strip_components value: {strip}"))
            })
            .transpose()?;

        let configured_matches = configured_url
            .as_deref()
            .and_then(|value| reqwest::Url::parse(value).ok())
            .is_some_and(|configured| configured == url);
        let locked_checksum = locked.and_then(|info| info.checksum.as_deref());
        let locked_size = locked.and_then(|info| info.size);
        let use_configured_checksum =
            !replaying || (locked_checksum.is_none() && configured_matches);
        let use_configured_size = !replaying || (locked_size.is_none() && configured_matches);

        let configured_size = if use_configured_size {
            opts.size_for_target(target)
                .map(|size| {
                    size.parse::<u64>()
                        .map_err(|_| eyre::eyre!("Invalid size value: {size}"))
                })
                .transpose()?
        } else {
            None
        };
        let configured_checksum = if use_configured_checksum {
            opts.checksum_for_target(target)
                .map(|checksum| PreparedChecksum::parse(&checksum))
                .transpose()?
        } else {
            None
        };
        let lock_checksum = locked_checksum.map(PreparedChecksum::parse).transpose()?;

        let bin = opts.bin_for_target(target);
        if let Some(bin) = &bin {
            ensure_safe_relative_bin_path("bin", bin)?;
        }
        let bin_path = opts
            .bin_path_for_target(target)
            .map(|template| template_string_for_target(&template, tv, target));
        if let Some(bin_path) = &bin_path {
            ensure_safe_relative_bin_path("bin_path", bin_path)?;
        }
        let rename_exe = opts.rename_exe_value_for_target(target).cloned();
        if let Some(rename_exe) = &rename_exe {
            Self::validate_rename_exe(rename_exe)?;
        }

        Ok(PreparedHttpInstall {
            target: target.to_key(),
            url,
            filename,
            lock_checksum,
            lock_size: locked_size,
            configured_checksum,
            configured_size,
            lockfile_enabled: Settings::get().lockfile_enabled(),
            format: opts.format_for_target(target),
            strip_components,
            bin,
            rename_exe,
            bin_path,
        })
    }

    fn validate_rename_exe(value: &toml::Value) -> Result<()> {
        match value {
            toml::Value::String(name) => ensure_plain_bin_name("rename_exe", name),
            toml::Value::Table(entries) => {
                for target in entries.values() {
                    if let Some(target) = target.as_str() {
                        ensure_plain_bin_name("rename_exe", target)?;
                    }
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    async fn prepare_http_install(
        &self,
        ctx: &InstallContext,
        tv: &ToolVersion,
    ) -> Result<PreparedInstall> {
        let target = PlatformTarget::from_current();
        let locked = tv.lock_platforms.get(&target.to_key());
        if ctx.locked && locked.and_then(|info| info.url.as_ref()).is_none() {
            bail!(
                "No lockfile URL found for {} on platform {} (--locked mode)\n\
                 hint: Run `mise lock` to generate lockfile URLs, or disable locked mode",
                tv.style(),
                target.to_key()
            );
        }
        let spec = self.prepare_http_target(tv, &target, locked)?;
        file::create_dir_all(Self::tarballs_dir())?;
        let staged_dir = tempfile::Builder::new()
            .prefix(".mise-http-stage-")
            .tempdir_in(Self::tarballs_dir())?;
        let download_dir = staged_dir.path().join("download");
        file::create_dir_all(&download_dir)?;
        let staged_file = download_dir.join(&spec.filename);

        ctx.pr.set_message(format!("download {}", spec.filename));
        HTTP.download_file(spec.url.clone(), &staged_file, Some(ctx.pr.as_ref()))
            .await?;
        let (lock_checksum, lock_size) = self.verify_staged_artifact(ctx, &staged_file, &spec)?;
        let cache_plan = self.cache_plan(&staged_file, &spec)?;
        let cache_checksum = lock_checksum.clone().or_else(|| spec.checksum());
        let staged_cache = staged_dir.path().join("cache");
        ctx.pr.next_operation();
        ctx.pr.set_message("extracting to staging".into());
        let extraction_type = self.extract_artifact(
            tv,
            &staged_cache,
            &staged_file,
            &cache_plan,
            &spec,
            Some(ctx.pr.as_ref()),
        )?;
        self.write_metadata(
            &staged_cache,
            spec.url.as_str(),
            cache_checksum.as_deref(),
            &staged_file,
        )?;

        Ok(PreparedInstall::prepared(PreparedHttpInstallPlan {
            backend: self.clone(),
            spec,
            staged_dir,
            staged_file,
            staged_cache,
            cache_plan,
            extraction_type,
            lock_checksum,
            lock_size,
        }))
    }

    /// Resolve the artifact URL for a target platform during `mise lock`.
    /// Renders `os()`/`arch()` for the target rather than the host.
    fn lock_url_for_target(
        &self,
        opts: &HttpOptions<'_>,
        tv: &ToolVersion,
        target: &PlatformTarget,
    ) -> Option<String> {
        opts.url_for_target(target)
            .map(|template| template_string_for_target(&template, tv, target))
    }

    /// Resolve a published checksum for a target platform without downloading
    /// the artifact. Tries, in order: a checksum configured directly for the
    /// platform, a manifest evaluated via `checksum_expr`, a SHASUMS file keyed
    /// by filename, then an individual checksum file. Returns `None`
    /// (best-effort) when no published checksum is available.
    async fn resolve_lock_checksum(
        &self,
        opts: &HttpOptions<'_>,
        tv: &ToolVersion,
        target: &PlatformTarget,
        url: &str,
    ) -> Option<String> {
        // 1. Checksum declared directly for this platform.
        if let Some(checksum) = opts.checksum_for_target(target) {
            return Some(checksum);
        }

        // 2. Fetch from a declared checksum source.
        let checksum_url_template = opts.checksum_url_for_target(target)?;
        let checksum_url = template_string_for_target(&checksum_url_template, tv, target);
        let filename = get_filename_from_url(url);

        // 2a. Manifest with an extraction expression. The expression returns an
        // `algo:hash` string. The manifest is the same across platforms, so use
        // the cached fetch.
        if let Some(expr) = opts.checksum_expr() {
            let body = match HTTP.get_text_cached(&checksum_url).await {
                Ok(body) => body,
                Err(e) => {
                    debug!("failed to fetch checksum manifest {checksum_url}: {e}");
                    return None;
                }
            };
            let vars = [
                ("version", tv.version.as_str()),
                ("os", target.os_name()),
                ("arch", target.arch_name()),
                ("url", url),
                ("filename", filename.as_str()),
            ];
            return eval_checksum_expr(expr, &body, &vars);
        }

        // 2b. Checksum file: a SHASUMS list (filename match) first, then an
        // individual checksum file. The algorithm is detected from its name.
        if let Some(checksum) = fetch_checksum_from_shasums(&checksum_url, &filename).await {
            return Some(checksum);
        }
        // A SHASUMS list that has entries but none matching our artifact is a
        // naming mismatch, not an individual checksum file. Falling back to the
        // individual-file scan would return the first hash in the list — another
        // platform's checksum — and silently lock it. Bail so the platform is
        // reported unresolved instead.
        if shasums_has_entries(&checksum_url).await {
            debug!(
                "checksum_url {checksum_url} is a SHASUMS list with no entry for {filename}; \
                 not falling back to a first-hash scan"
            );
            return None;
        }
        let file_algo = crate::backend::asset_matcher::detect_checksum_algorithm(
            &get_filename_from_url(&checksum_url),
        );
        fetch_checksum_from_file(&checksum_url, &file_algo).await
    }

    fn install_prepared_http(
        &self,
        ctx: &InstallContext,
        mut tv: ToolVersion,
        spec: &PreparedHttpInstall,
        staged_file: &Path,
        staged_cache: &Path,
        cache_plan: &CachePlan,
        prepared_extraction_type: ExtractionType,
        lock_checksum: Option<String>,
        lock_size: Option<u64>,
    ) -> Result<ToolVersion> {
        self.apply_lock_contract(&mut tv, spec, lock_checksum, lock_size);

        if Settings::get().always_keep_download {
            let download_path = tv.download_path().join(&spec.filename);
            file::create_dir_all(tv.download_path())?;
            file::copy(staged_file, download_path)?;
        }

        let cache_path = self.cache_path(&cache_plan.key);
        let _lock = crate::lock_file::get(&cache_path, ctx.force)?;

        let extraction_type = if self.is_cached(&cache_plan.key) {
            ctx.pr.set_message("using cached tarball".into());
            ctx.pr.set_length(1);
            ctx.pr.set_position(1);
            self.extraction_type_from_cache(&cache_plan.key, &cache_plan.file_info)
        } else {
            ctx.pr.set_message("publishing prepared cache".into());
            if cache_path.exists() {
                file::remove_all(&cache_path)?;
            }
            std::fs::rename(staged_cache, &cache_path)?;
            prepared_extraction_type
        };

        self.create_install_symlink(&tv, &cache_plan.key, &extraction_type, spec)?;
        self.create_version_alias_symlink(&tv, &cache_plan.key)?;
        tv.install_path = Some(Self::install_path_for(&tv, &cache_plan.key));

        Ok(tv)
    }
}

/// Returns install-time-only option keys for HTTP backend.
pub fn install_time_option_keys() -> Vec<String> {
    vec![
        "url".into(),
        "checksum".into(),
        "version_list_url".into(),
        "version_regex".into(),
        "version_json_path".into(),
        "version_expr".into(),
        "format".into(),
        "rename_exe".into(),
        "checksum_url".into(),
        "checksum_expr".into(),
    ]
}

#[async_trait]
impl Backend for HttpBackend {
    fn get_type(&self) -> BackendType {
        BackendType::Http
    }

    fn ba(&self) -> &Arc<BackendArg> {
        &self.ba
    }

    fn mark_prereleases_from_version_pattern(&self) -> bool {
        true
    }

    fn remote_version_listing_tool_option_keys(&self) -> &'static [&'static str] {
        &[
            "version_list_url",
            "version_regex",
            "version_json_path",
            "version_expr",
        ]
    }

    async fn install_operation_count(&self, tv: &ToolVersion, _ctx: &InstallContext) -> usize {
        let raw_opts = tv.request.options();
        let opts = HttpOptions::new(&raw_opts);
        let target = PlatformTarget::from_current();
        let platform_key = target.to_key();
        let locked = tv.lock_platforms.get(&platform_key);
        let locked_url = locked.and_then(|info| info.url.as_deref());
        let configured_url = self.lock_url_for_target(&opts, tv, &target);
        let configured_matches =
            locked_url
                .zip(configured_url.as_deref())
                .is_some_and(|(locked, configured)| {
                    reqwest::Url::parse(locked).ok() == reqwest::Url::parse(configured).ok()
                });
        let has_configured_checksum = opts.checksum().is_some()
            && (locked_url.is_none()
                || (locked.and_then(|info| info.checksum.as_ref()).is_none()
                    && configured_matches));
        super::http_install_operation_count(has_configured_checksum, &platform_key, tv)
    }

    /// Options that affect which artifact is downloaded, resolved for the target
    /// platform so cross-platform lockfile entries match install-time lookups.
    fn resolve_lockfile_options(
        &self,
        request: &ToolRequest,
        target: &PlatformTarget,
    ) -> Result<BTreeMap<String, String>> {
        let raw_opts = request.options();
        let opts = HttpOptions::new(&raw_opts);
        let mut result = BTreeMap::new();
        if let Some(format) = opts.format_for_target(target) {
            result.insert("format".to_string(), format);
        }
        if let Some(strip) = opts.strip_components_for_target(target) {
            result.insert("strip_components".to_string(), strip);
        }
        if let Some(rename) = opts.rename_exe_for_target(target) {
            result.insert("rename_exe".to_string(), rename);
        }
        Ok(result)
    }

    /// Resolve URL + published checksum for a target platform during `mise lock`,
    /// without downloading the artifact. Best-effort: a platform with no
    /// resolvable URL fails closed (`Err`) so the lock run reports it as skipped
    /// rather than writing nothing under a success count; a missing checksum
    /// yields a url-only entry.
    async fn resolve_lock_info(
        &self,
        tv: &ToolVersion,
        target: &PlatformTarget,
    ) -> Result<PlatformInfo> {
        let raw_opts = tv.request.options();
        let opts = HttpOptions::new(&raw_opts);
        let prepared = self.prepare_http_target(tv, target, None).map_err(|err| {
            eyre::eyre!(
                "no URL configured for {} on {}; skipping: {err}",
                self.ba.full(),
                target.to_key()
            )
        })?;
        let url = prepared.url;

        let checksum = self
            .resolve_lock_checksum(&opts, tv, target, url.as_str())
            .await;

        // A checksum source was configured but produced nothing for this target
        // (manifest miss, SHASUMS naming mismatch, unreachable file, ...). The
        // url-only entry is still written, but surface it so it isn't a silent
        // drop of checksum verification.
        if checksum.is_none() && opts.checksum_url_for_target(target).is_some() {
            warn!(
                "could not resolve a checksum for {} on {}; locking the URL without checksum verification",
                self.ba.full(),
                target.to_key()
            );
        }

        Ok(PlatformInfo {
            url: Some(url.to_string()),
            checksum,
            ..Default::default()
        })
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

    async fn prepare_install(
        &self,
        ctx: &InstallContext,
        tv: &ToolVersion,
    ) -> Result<PreparedInstall> {
        self.prepare_http_install(ctx, tv).await
    }

    async fn install_version_(&self, ctx: &InstallContext, tv: ToolVersion) -> Result<ToolVersion> {
        let prepared = self.prepare_http_install(ctx, &tv).await?;
        prepared.execute(self, ctx, tv).await
    }

    fn is_version_installed(
        &self,
        _config: &Arc<Config>,
        tv: &ToolVersion,
        check_symlink: bool,
    ) -> bool {
        match tv.request {
            ToolRequest::System { .. } => true,
            _ => {
                let install_path = Self::lookup_install_path(tv);
                install_path.exists()
                    && !self.incomplete_file_path(tv).exists()
                    && (!check_symlink || !is_runtime_symlink(&install_path))
            }
        }
    }

    async fn list_bin_paths(
        &self,
        _config: &Arc<Config>,
        tv: &ToolVersion,
    ) -> Result<Vec<PathBuf>> {
        let raw_opts = tv.request.options();
        let opts = HttpOptions::new(&raw_opts);
        let install_path = Self::lookup_install_path(tv);
        let mut tv = tv.clone();
        tv.install_path = Some(install_path.clone());

        // Check for explicit bin_path
        if let Some(bin_path_template) = opts.bin_path() {
            let bin_path = template_string(&bin_path_template, &tv);
            return Ok(vec![runtime_path_for_install_path(
                &tv,
                install_path.join(bin_path),
            )]);
        }

        // Check for bin directory
        let bin_dir = install_path.join("bin");
        if bin_dir.exists() {
            return Ok(vec![runtime_path_for_install_path(
                &tv,
                install_path.join("bin"),
            )]);
        }

        // Search subdirectories for bin directories
        let mut paths = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&install_path) {
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
            Ok(vec![runtime_path_for_install_path(&tv, install_path)])
        } else {
            Ok(paths
                .into_iter()
                .map(|path| runtime_path_for_install_path(&tv, path))
                .collect())
        }
    }
}

/// Produce a cache-key token for a `rename_exe` value. The value is always hashed
/// rather than embedded, so nothing can leak into or alias the cache directory
/// name — not the table form's glob/brace syntax, not path separators or
/// Windows-invalid characters, and not trailing dots/spaces that Windows strips
/// (which would otherwise make `tool.` and `tool` share a cache entry). The key
/// still differs whenever the rename config differs, which is all it needs to do;
/// the human-readable part of the cache key is the file checksum, not this token.
fn rename_cache_token(rename: &toml::Value) -> String {
    hash::hash_blake3_to_str(&rename.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::args::BackendResolution;
    use crate::toolset::{ToolRequest, ToolSource};

    fn http_test_tv_with_options(version: &str, options: ToolVersionOptions) -> ToolVersion {
        let backend = Arc::new(BackendArg::new_raw(
            "http-absolute-version".to_string(),
            Some("http:absolute-version".to_string()),
            "absolute-version".to_string(),
            None,
            BackendResolution::new(true),
        ));
        let request = ToolRequest::Version {
            backend,
            version: version.to_string(),
            options,
            source: ToolSource::Argument,
        };
        ToolVersion::new(request, version.to_string())
    }

    fn http_test_tv(version: &str) -> ToolVersion {
        http_test_tv_with_options(version, ToolVersionOptions::default())
    }

    fn http_test_backend() -> HttpBackend {
        HttpBackend::from_arg(BackendArg::new_raw(
            "http-absolute-version".to_string(),
            Some("http:absolute-version".to_string()),
            "absolute-version".to_string(),
            None,
            BackendResolution::new(true),
        ))
    }

    fn prepared_http_options(raw: &str) -> PreparedHttpInstall {
        let raw = crate::toolset::parse_tool_options(raw);
        let opts = HttpOptions::new(&raw);
        let target = PlatformTarget::from_current();
        PreparedHttpInstall {
            target: target.to_key(),
            url: reqwest::Url::parse("https://example.com/artifact").unwrap(),
            filename: "artifact".to_string(),
            lock_checksum: None,
            lock_size: None,
            configured_checksum: None,
            configured_size: None,
            lockfile_enabled: false,
            format: opts.format_for_target(&target),
            strip_components: opts
                .strip_components_for_target(&target)
                .map(|value| value.parse().unwrap()),
            bin: opts.bin_for_target(&target),
            rename_exe: opts.rename_exe_value_for_target(&target).cloned(),
            bin_path: opts.bin_path_for_target(&target),
        }
    }

    fn version_hash(version: &str) -> String {
        crate::hash::hash_sha256_to_str(version)[..7].to_string()
    }

    #[test]
    fn template_string_for_target_renders_target_os_arch() {
        let tv = http_test_tv("0.40.0");
        let template =
            r#"sentinel_{{ version }}_{{ os(macos="darwin") }}_{{ arch(x64="amd64") }}.zip"#;
        let linux = PlatformTarget::new(crate::platform::Platform::parse("linux-x64").unwrap());
        assert_eq!(
            template_string_for_target(template, &tv, &linux),
            "sentinel_0.40.0_linux_amd64.zip"
        );
        let win = PlatformTarget::new(crate::platform::Platform::parse("windows-x64").unwrap());
        assert_eq!(
            template_string_for_target(template, &tv, &win),
            "sentinel_0.40.0_windows_amd64.zip"
        );
    }

    #[test]
    fn prepared_http_install_prefers_locked_artifact_contract() {
        let configured_checksum = format!("sha256:{}", "1".repeat(64));
        let locked_checksum = format!("sha256:{}", "2".repeat(64));
        let options = crate::toolset::parse_tool_options(&format!(
            "url=https://example.com/current,checksum={configured_checksum},size=7,format=tar.gz,strip_components=1,bin=mytool,bin_path=tools/bin"
        ));
        let tv = http_test_tv_with_options("1.0.0", options);
        let target = PlatformTarget::from_current();
        let locked = PlatformInfo {
            url: Some("https://example.com/locked".to_string()),
            checksum: Some(locked_checksum.clone()),
            size: Some(42),
            ..Default::default()
        };

        let prepared = http_test_backend()
            .prepare_http_target(&tv, &target, Some(&locked))
            .unwrap();

        assert_eq!(prepared.url.as_str(), "https://example.com/locked");
        assert_eq!(
            prepared.lock_checksum.map(|value| value.to_string()),
            Some(locked_checksum)
        );
        assert_eq!(prepared.lock_size, Some(42));
        assert_eq!(prepared.configured_checksum, None);
        assert_eq!(prepared.configured_size, None);
        assert_eq!(prepared.format.as_deref(), Some("tar.gz"));
        assert_eq!(prepared.strip_components, Some(1));
        assert_eq!(prepared.bin.as_deref(), Some("mytool"));
        assert_eq!(prepared.bin_path.as_deref(), Some("tools/bin"));
    }

    #[test]
    fn prepared_http_install_uses_configured_integrity_for_same_locked_url() {
        let configured_checksum = format!("sha256:{}", "3".repeat(64));
        let options = crate::toolset::parse_tool_options(&format!(
            "url=https://example.com/artifact,checksum={configured_checksum},size=7"
        ));
        let tv = http_test_tv_with_options("1.0.0", options);
        let target = PlatformTarget::from_current();
        let locked = PlatformInfo {
            url: Some("https://example.com/artifact".to_string()),
            ..Default::default()
        };

        let prepared = http_test_backend()
            .prepare_http_target(&tv, &target, Some(&locked))
            .unwrap();

        assert_eq!(
            prepared.configured_checksum.map(|value| value.to_string()),
            Some(configured_checksum)
        );
        assert_eq!(prepared.configured_size, Some(7));
    }

    #[test]
    fn prepared_checksum_rejects_unusable_values() {
        assert!(PreparedChecksum::parse("sha256:short").is_err());
        assert!(PreparedChecksum::parse(&format!("unknown:{}", "0".repeat(64))).is_err());
        assert!(PreparedChecksum::parse(&format!("sha256:{}", "g".repeat(64))).is_err());
    }

    #[test]
    fn prepared_http_install_rejects_decoded_filename_traversal() {
        for url in [
            "https://example.com/%2E%2E%2Fvictim",
            "https://example.com/%2Ftmp%2Fvictim",
        ] {
            let options = crate::toolset::parse_tool_options(&format!("url={url}"));
            let tv = http_test_tv_with_options("1.0.0", options);
            let target = PlatformTarget::from_current();
            let err = http_test_backend()
                .prepare_http_target(&tv, &target, None)
                .unwrap_err();

            assert!(
                err.to_string().contains("HTTP artifact filename"),
                "unexpected error for {url}: {err:?}"
            );
        }
    }

    #[test]
    fn prepared_rename_exe_preserves_exact_non_glob_source_names() {
        let rename_exe = toml::Value::Table(
            [("tool[".to_string(), toml::Value::String("tool".to_string()))]
                .into_iter()
                .collect(),
        );

        HttpBackend::validate_rename_exe(&rename_exe).unwrap();
    }

    #[test]
    fn install_symlink_path_uses_sanitized_version_pathname() {
        let version = "/outside-root/mise-http-version-out/selected-prefix";
        let tv = http_test_tv(version);
        let version_name = HttpBackend::install_version_name(&tv, "abcdef123456");

        assert_eq!(
            version_name,
            format!(
                "-outside-root-mise-http-version-out-selected-prefix-{}",
                version_hash(version)
            )
        );
        assert!(!Path::new(&version_name).is_absolute());
    }

    #[test]
    fn install_symlink_path_sanitizes_parent_version() {
        let version = "..";
        let tv = http_test_tv(version);
        let version_name = HttpBackend::install_version_name(&tv, "abcdef123456");

        assert_eq!(version_name, format!("__-{}", version_hash(version)));
        assert!(
            Path::new(&version_name)
                .components()
                .all(|c| matches!(c, std::path::Component::Normal(_)))
        );
    }

    #[test]
    fn install_symlink_path_sanitizes_windows_separators() {
        let version = r"..\..\outside-root\mise-http-version-out\selected-prefix";
        let tv = http_test_tv(version);
        let version_name = HttpBackend::install_version_name(&tv, "abcdef123456");

        assert_eq!(
            version_name,
            format!(
                "..-..-outside-root-mise-http-version-out-selected-prefix-{}",
                version_hash(version)
            )
        );
        assert!(!version_name.contains('\\'));
    }

    #[test]
    fn install_symlink_path_sanitizes_windows_unc_paths() {
        let version = r"\\server\share";
        let tv = http_test_tv(version);
        let version_name = HttpBackend::install_version_name(&tv, "abcdef123456");

        assert_eq!(
            version_name,
            format!("--server-share-{}", version_hash(version))
        );
        assert!(!version_name.contains('\\'));
    }

    #[test]
    fn install_symlink_path_preserves_distinct_sanitized_versions() {
        let slash = HttpBackend::install_version_name(&http_test_tv("a/b"), "abcdef123456");
        let colon = HttpBackend::install_version_name(&http_test_tv("a:b"), "abcdef123456");
        let backslash = HttpBackend::install_version_name(&http_test_tv(r"a\b"), "abcdef123456");
        let dash = HttpBackend::install_version_name(&http_test_tv("a-b"), "abcdef123456");

        assert_eq!(dash, "a-b");
        assert_ne!(slash, dash);
        assert_ne!(colon, dash);
        assert_ne!(backslash, dash);
        assert_ne!(slash, colon);
        assert_ne!(slash, backslash);
        assert_ne!(colon, backslash);
    }

    #[test]
    fn latest_install_symlink_still_uses_content_version() {
        let tv = http_test_tv("latest");
        let version_name = HttpBackend::install_version_name(&tv, "abcdef123456");

        assert_eq!(version_name, "abcdef1");
    }

    #[test]
    fn empty_install_symlink_uses_implicit_version() {
        let tv = http_test_tv("");
        let version_name = HttpBackend::install_version_name(&tv, "abcdef123456");

        assert_eq!(version_name, "_implicit");
    }

    #[test]
    fn empty_install_path_uses_implicit_version_path() {
        let tv = http_test_tv("");
        let install_path = HttpBackend::install_path_for(&tv, "abcdef123456");

        assert_eq!(install_path, tv.ba().installs_path.join("_implicit"));
        assert_ne!(install_path, tv.ba().installs_path);
    }

    #[test]
    fn lookup_install_path_matches_sanitized_install_path() {
        let version = "/outside-root/mise-http-version-out/selected-prefix";
        let tv = http_test_tv(version);
        let install_path = HttpBackend::install_path_for(&tv, "abcdef123456");
        let lookup_path = HttpBackend::lookup_install_path(&tv);

        assert_eq!(lookup_path, install_path);
    }

    #[test]
    fn dest_filename_uses_decompressed_name_for_rename_exe_extension() {
        let backend = HttpBackend {
            ba: Arc::new(BackendArg::new_raw(
                "http-code2prompt".to_string(),
                Some("http:code2prompt".to_string()),
                "code2prompt".to_string(),
                None,
                BackendResolution::new(true),
            )),
        };
        let opts = prepared_http_options("rename_exe=code2prompt");
        let file_path = Path::new("code2prompt-x86_64-pc-windows-msvc.exe.gz");
        let file_info = FileInfo::new(file_path, &opts);

        assert!(file_info.is_compressed_binary);
        assert_eq!(
            backend.dest_filename(file_path, &file_info, &opts).unwrap(),
            "code2prompt.exe"
        );
    }

    #[test]
    fn dest_filename_rejects_path_traversal_in_bin_and_rename_exe() {
        let backend = HttpBackend {
            ba: Arc::new(BackendArg::new_raw(
                "http-mytool".to_string(),
                Some("http:mytool".to_string()),
                "mytool".to_string(),
                None,
                BackendResolution::new(true),
            )),
        };
        let file_path = Path::new("mytool-linux-x64");

        for (opt, expected) in [
            (r#"bin="../evil""#, "safe relative path"),
            (r#"rename_exe="a/b""#, "plain file name"),
        ] {
            let opts = prepared_http_options(opt);
            let file_info = FileInfo::new(file_path, &opts);
            let err = backend
                .dest_filename(file_path, &file_info, &opts)
                .unwrap_err();
            assert!(
                err.to_string().contains(expected),
                "unexpected error for {opt}: {err}"
            );
        }

        let opts = prepared_http_options(r#"bin="bin/mytool""#);
        let file_info = FileInfo::new(file_path, &opts);
        assert_eq!(
            backend.dest_filename(file_path, &file_info, &opts).unwrap(),
            "bin/mytool"
        );
    }

    #[test]
    fn cache_key_is_path_safe_for_scalar_and_table_rename_exe() {
        let backend = HttpBackend {
            ba: Arc::new(BackendArg::new_raw(
                "http-mytool".to_string(),
                Some("http:mytool".to_string()),
                "mytool".to_string(),
                None,
                BackendResolution::new(true),
            )),
        };

        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), b"archive-contents").unwrap();

        // Characters that are illegal or unsafe in Windows path components and
        // must never appear in a cache directory name derived from rename_exe.
        let illegal = ['*', '"', '{', '}', '<', '>', ':', '|', '?', '\\', '/', ' '];

        let key_for = |opt: &str| {
            let opts = prepared_http_options(opt);
            backend.cache_key(tmp.path(), &opts, 0).unwrap()
        };

        // Scalar, table, and adversarial values all yield path-safe keys.
        for opt in [
            "rename_exe=plz",
            r#"rename_exe="foo/bar:baz""#,
            r#"rename_exe={ "ols-*" = "ols", "odinfmt-*" = "odinfmt" }"#,
        ] {
            let key = key_for(opt);
            assert!(key.contains("rename_"), "key: {key}");
            assert!(!key.contains(illegal), "unsafe cache key for {opt}: {key}");
        }

        // Different rename configs must produce different keys...
        assert_ne!(key_for("rename_exe=plz"), key_for("rename_exe=other"));
        assert_ne!(
            key_for(r#"rename_exe={ "ols-*" = "ols" }"#),
            key_for(r#"rename_exe={ "ols-*" = "renamed" }"#)
        );

        // ...including values that only differ by a trailing dot or space, which
        // Windows would otherwise collapse to the same path component.
        assert_ne!(key_for("rename_exe=tool"), key_for(r#"rename_exe="tool.""#));
        assert_ne!(key_for("rename_exe=tool"), key_for(r#"rename_exe="tool ""#));
    }
}
