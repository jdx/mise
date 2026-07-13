use crate::backend::Backend;
use crate::backend::VersionInfo;
use crate::backend::backend_type::BackendType;
use crate::backend::options::BackendOptions;
use crate::backend::platform_target::PlatformTarget;
use crate::backend::prepared_install::{PreparedHttpInstall, PreparedInstall, SuccessfulInstall};
use crate::backend::runtime_path_for_install_path;
use crate::backend::static_helpers::{
    clean_binary_name, eval_checksum_expr, fetch_checksum_from_file, fetch_checksum_from_shasums,
    get_filename_from_url, rename_binary_name, rename_executable_in_dir, shasums_has_entries,
    template_string, template_string_for_target, verify_checksum_str,
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
use eyre::{Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt::Debug;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

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

#[derive(Debug)]
pub struct HttpBackend {
    ba: Arc<BackendArg>,
}

#[derive(Debug, Clone, Copy)]
struct HttpOptions<'a> {
    values: BackendOptions<'a>,
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
    fn checksum(&self) -> Option<&str> {
        self.configured_checksum.as_deref()
    }

    fn format(&self) -> Option<&str> {
        self.format.as_deref()
    }

    fn strip_components(&self) -> Option<&str> {
        self.strip_components.as_deref()
    }

    fn bin(&self) -> Option<&str> {
        self.bin.as_deref()
    }

    fn rename_exe(&self) -> Option<&str> {
        self.rename_exe.as_deref()
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

        // Include rename_exe in cache key since it modifies the extracted content
        if let Some(rename) = opts.rename_exe() {
            parts.push(format!("rename_{rename}"));
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
        let mut strip_components: Option<usize> = opts
            .strip_components()
            .map(|s| {
                s.parse::<usize>()
                    .map_err(|_| eyre::eyre!("Invalid strip_components value: {s}"))
            })
            .transpose()?;

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

    /// Determine the destination filename for a raw file or compressed binary
    fn dest_filename(
        &self,
        file_path: &Path,
        file_info: &FileInfo,
        opts: &PreparedHttpInstall,
    ) -> String {
        // Check for explicit bin name first
        if let Some(bin_name) = opts.bin() {
            return bin_name.to_string();
        }
        if let Some(rename_to) = opts.rename_exe() {
            let source_name = if file_info.is_compressed_binary {
                file_info.decompressed_name()
            } else {
                file_path.file_name().unwrap().to_string_lossy().to_string()
            };
            return rename_binary_name(&source_name, rename_to);
        }

        // Auto-clean the binary name
        let raw_name = if file_info.is_compressed_binary {
            file_info.decompressed_name()
        } else {
            file_path.file_name().unwrap().to_string_lossy().to_string()
        };

        clean_binary_name(&raw_name, Some(&self.ba.tool_name))
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

    /// Extract artifact to cache with atomic rename
    fn extract_to_cache(
        &self,
        tv: &ToolVersion,
        file_path: &Path,
        cache_plan: &CachePlan,
        url: &str,
        opts: &PreparedHttpInstall,
        pr: Option<&dyn SingleReport>,
    ) -> Result<ExtractionType> {
        let cache_path = self.cache_path(&cache_plan.key);

        // Ensure parent directory exists
        file::create_dir_all(Self::tarballs_dir())?;

        // Create unique temp directory for atomic extraction
        let tmp_path = Self::tarballs_dir().join(format!(
            "{}.tmp-{}-{}",
            cache_plan.key,
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis()
        ));

        // Clean up any stale temp directory
        if tmp_path.exists() {
            let _ = file::remove_all(&tmp_path);
        }

        // Perform extraction
        let extraction_type =
            self.extract_artifact(tv, &tmp_path, file_path, cache_plan, opts, pr)?;

        // Atomic replace
        if cache_path.exists() {
            file::remove_all(&cache_path)?;
        }
        std::fs::rename(&tmp_path, &cache_path)?;

        // Write metadata
        self.write_metadata(&cache_plan.key, url, file_path, opts)?;

        Ok(extraction_type)
    }

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
        let filename = self.dest_filename(file_path, file_info, opts);
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
        let filename = self.dest_filename(file_path, file_info, opts);
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
        tv: &ToolVersion,
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
        if let Some(rename_to) = opts.rename_exe() {
            // When bin_path is not explicitly set, auto-detect bin/ subdirectory to match
            // the same logic used by discover_bin_paths() for PATH construction
            let search_dir = if let Some(bin_path_template) = opts.bin_path() {
                let bin_path = template_string(bin_path_template, tv);
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
            rename_executable_in_dir(&search_dir, rename_to, Some(tool_name))?;
        }

        Ok(ExtractionType::Archive)
    }

    /// Write cache metadata file
    fn write_metadata(
        &self,
        cache_key: &str,
        url: &str,
        file_path: &Path,
        opts: &PreparedHttpInstall,
    ) -> Result<()> {
        let metadata = CacheMetadata {
            url: url.to_string(),
            checksum: opts.checksum().map(str::to_string),
            size: file_path.metadata()?.len(),
            extracted_at: SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
            platform: self.get_platform_key(),
        };

        let json = serde_json::to_string_pretty(&metadata)?;
        file::write(self.metadata_path(cache_key), json)?;
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
            && let Some(bin_path_template) = opts.bin_path()
        {
            let bin_path = template_string(bin_path_template, tv);
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

    fn verify_prepared_artifact(
        &self,
        ctx: &InstallContext,
        file_path: &Path,
        spec: &PreparedHttpInstall,
    ) -> Result<()> {
        if let Some(checksum) = &spec.configured_checksum {
            verify_checksum_str(file_path, checksum, Some(ctx.pr.as_ref()))?;
        }
        if let Some(expected_size) = spec.configured_size {
            let actual_size = file_path.metadata()?.len();
            if actual_size != expected_size {
                let filename = file_path.file_name().unwrap().to_string_lossy();
                bail!("Size mismatch for {filename}: expected {expected_size}, got {actual_size}");
            }
        }
        Ok(())
    }

    /// Verify or enrich the lock contract selected during preparation.
    fn verify_lock_contract(
        &self,
        ctx: &InstallContext,
        tv: &mut ToolVersion,
        file_path: &Path,
        spec: &PreparedHttpInstall,
    ) -> Result<()> {
        let settings = Settings::get();
        let filename = file_path.file_name().unwrap().to_string_lossy();
        let lockfile_enabled = settings.lockfile_enabled();

        let platform_info = tv.lock_platforms.entry(spec.target.clone()).or_default();
        platform_info.url = Some(spec.url.clone());

        // Verify or generate checksum
        if let Some(checksum) = &spec.lock_checksum {
            ctx.pr.set_message(format!("checksum {filename}"));
            verify_checksum_str(file_path, checksum, Some(ctx.pr.as_ref()))?;
            platform_info.checksum = Some(checksum.clone());
        } else if lockfile_enabled {
            ctx.pr.set_message(format!("generate checksum {filename}"));
            let h = hash::file_hash_blake3(file_path, Some(ctx.pr.as_ref()))?;
            platform_info.checksum = Some(format!("blake3:{h}"));
        }

        // Verify or record size
        if let Some(expected_size) = spec.lock_size {
            ctx.pr.set_message(format!("verify size {filename}"));
            let actual_size = file_path.metadata()?.len();
            if actual_size != expected_size {
                return Err(eyre::eyre!(
                    "Size mismatch for {filename}: expected {expected_size}, got {actual_size}"
                ));
            }
            platform_info.size = Some(expected_size);
        } else if lockfile_enabled {
            platform_info.size = Some(file_path.metadata()?.len());
        }

        Ok(())
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
        let replaying = locked_url.is_some();
        let url = locked_url
            .or_else(|| self.lock_url_for_target(&opts, tv, target))
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

        let strip_components = opts.strip_components_for_target(target);
        if let Some(strip) = &strip_components {
            strip
                .parse::<usize>()
                .map_err(|_| eyre::eyre!("Invalid strip_components value: {strip}"))?;
        }

        let configured_size = if replaying {
            None
        } else {
            opts.size_for_target(target)
                .map(|size| {
                    size.parse::<u64>()
                        .map_err(|_| eyre::eyre!("Invalid size value: {size}"))
                })
                .transpose()?
        };
        let configured_checksum = if replaying {
            None
        } else {
            opts.checksum_for_target(target)
        };

        for checksum in [
            locked.and_then(|info| info.checksum.as_deref()),
            configured_checksum.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            if !checksum.contains(':') {
                bail!("Invalid checksum format: {checksum}");
            }
        }

        Ok(PreparedHttpInstall {
            target: target.to_key(),
            url,
            lock_checksum: locked.and_then(|info| info.checksum.clone()),
            lock_size: locked.and_then(|info| info.size),
            configured_checksum,
            configured_size,
            format: opts.format_for_target(target),
            strip_components,
            bin: opts.bin_for_target(target),
            rename_exe: opts.rename_exe_for_target(target),
            bin_path: opts.bin_path_for_target(target),
        })
    }

    fn prepare_http_install(
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
        Ok(PreparedInstall::http(
            self.prepare_http_target(tv, &target, locked)?,
        ))
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

    async fn install_prepared_http(
        &self,
        ctx: &InstallContext,
        mut tv: ToolVersion,
        spec: &PreparedHttpInstall,
    ) -> Result<ToolVersion> {
        let url = spec.url.clone();
        let filename = get_filename_from_url(&url);
        let file_path = tv.download_path().join(&filename);

        let lockfile_enabled = Settings::get().lockfile_enabled();
        let has_lockfile_checksum = spec.lock_checksum.is_some();

        ctx.pr.set_message(format!("download {filename}"));
        HTTP.download_file(&url, &file_path, Some(ctx.pr.as_ref()))
            .await?;

        if spec.configured_checksum.is_some() {
            ctx.pr.next_operation();
        }
        self.verify_prepared_artifact(ctx, &file_path, spec)?;

        // Validate the locked artifact contract before populating the shared
        // extraction cache or exposing any install symlink. Enrichment remains
        // in-memory until the successful result is returned to the writer.
        if lockfile_enabled || has_lockfile_checksum {
            ctx.pr.next_operation();
        }
        self.verify_lock_contract(ctx, &mut tv, &file_path, spec)?;

        let cache_plan = self.cache_plan(&file_path, spec)?;
        let cache_path = self.cache_path(&cache_plan.key);
        let _lock = crate::lock_file::get(&cache_path, ctx.force)?;

        ctx.pr.next_operation();
        let extraction_type = if self.is_cached(&cache_plan.key) {
            ctx.pr.set_message("using cached tarball".into());
            ctx.pr.set_length(1);
            ctx.pr.set_position(1);
            self.extraction_type_from_cache(&cache_plan.key, &cache_plan.file_info)
        } else {
            ctx.pr.set_message("extracting to cache".into());
            self.extract_to_cache(
                &tv,
                &file_path,
                &cache_plan,
                &url,
                spec,
                Some(ctx.pr.as_ref()),
            )?
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
        let platform_key = self.get_platform_key();
        let replaying = tv
            .lock_platforms
            .get(&platform_key)
            .and_then(|info| info.url.as_ref())
            .is_some();
        let has_configured_checksum = !replaying && opts.checksum().is_some();
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

        let checksum = self.resolve_lock_checksum(&opts, tv, target, &url).await;

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
            url: Some(url),
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

    fn prepare_install(&self, ctx: &InstallContext, tv: &ToolVersion) -> Result<PreparedInstall> {
        self.prepare_http_install(ctx, tv)
    }

    async fn install_prepared_version_(
        &self,
        ctx: &InstallContext,
        tv: ToolVersion,
        prepared: PreparedInstall,
    ) -> Result<SuccessfulInstall> {
        let tv = self
            .install_prepared_http(ctx, tv, prepared.http_spec()?)
            .await?;
        Ok(SuccessfulInstall::new(tv, prepared))
    }

    async fn install_version_(&self, ctx: &InstallContext, tv: ToolVersion) -> Result<ToolVersion> {
        let prepared = self.prepare_http_install(ctx, &tv)?;
        Ok(self
            .install_prepared_version_(ctx, tv, prepared)
            .await?
            .into_tool_version())
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
        let options = crate::toolset::parse_tool_options(
            "url=https://example.com/current,checksum=sha256:current,size=7,format=tar.gz,strip_components=1,bin=mytool,bin_path=tools/bin",
        );
        let tv = http_test_tv_with_options("1.0.0", options);
        let target = PlatformTarget::from_current();
        let locked = PlatformInfo {
            url: Some("https://example.com/locked".to_string()),
            checksum: Some("sha256:locked".to_string()),
            size: Some(42),
            ..Default::default()
        };

        let prepared = http_test_backend()
            .prepare_http_target(&tv, &target, Some(&locked))
            .unwrap();

        assert_eq!(prepared.url, "https://example.com/locked");
        assert_eq!(prepared.lock_checksum.as_deref(), Some("sha256:locked"));
        assert_eq!(prepared.lock_size, Some(42));
        assert_eq!(prepared.configured_checksum, None);
        assert_eq!(prepared.configured_size, None);
        assert_eq!(prepared.format.as_deref(), Some("tar.gz"));
        assert_eq!(prepared.strip_components.as_deref(), Some("1"));
        assert_eq!(prepared.bin.as_deref(), Some("mytool"));
        assert_eq!(prepared.bin_path.as_deref(), Some("tools/bin"));
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
        let opts = PreparedHttpInstall {
            target: PlatformTarget::from_current().to_key(),
            url: "https://example.com/code2prompt.gz".to_string(),
            lock_checksum: None,
            lock_size: None,
            configured_checksum: None,
            configured_size: None,
            format: None,
            strip_components: None,
            bin: None,
            rename_exe: Some("code2prompt".to_string()),
            bin_path: None,
        };
        let file_path = Path::new("code2prompt-x86_64-pc-windows-msvc.exe.gz");
        let file_info = FileInfo::new(file_path, &opts);

        assert!(file_info.is_compressed_binary);
        assert_eq!(
            backend.dest_filename(file_path, &file_info, &opts),
            "code2prompt.exe"
        );
    }
}
