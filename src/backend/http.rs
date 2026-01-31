use crate::backend::Backend;
use crate::backend::VersionInfo;
use crate::backend::backend_type::BackendType;
use crate::backend::static_helpers::{
    clean_binary_name, get_filename_from_url, list_available_platforms_with_key,
    lookup_platform_key, rename_executable_in_dir, template_string, verify_artifact,
};
use crate::backend::version_list;
use crate::cli::args::BackendArg;
use crate::config::Config;
use crate::config::Settings;
use crate::http::HTTP;
use crate::install_context::InstallContext;
use crate::toolset::ToolVersion;
use crate::toolset::ToolVersionOptions;
use crate::ui::progress_report::SingleReport;
use crate::{dirs, file, hash};
use async_trait::async_trait;
use eyre::Result;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

// Constants
const HTTP_TARBALLS_DIR: &str = "http-tarballs";
const METADATA_FILE: &str = "metadata.json";

/// Helper to get an option value with platform-specific fallback
fn get_opt(opts: &ToolVersionOptions, key: &str) -> Option<String> {
    lookup_platform_key(opts, key).or_else(|| opts.get(key).cloned())
}

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
    format: file::TarFormat,
    /// Whether this is a compressed single binary (not a tar archive)
    is_compressed_binary: bool,
}

impl FileInfo {
    /// Analyze a file path and options to determine format information
    fn new(file_path: &Path, opts: &ToolVersionOptions) -> Self {
        // Apply format config to determine effective extension
        let effective_path = if let Some(added_ext) = get_opt(opts, "format") {
            let mut path = file_path.to_path_buf();
            let current_ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            let new_ext = if current_ext.is_empty() {
                added_ext
            } else {
                format!("{}.{}", current_ext, added_ext)
            };
            path.set_extension(new_ext);
            path
        } else {
            file_path.to_path_buf()
        };

        let extension = effective_path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();

        let format = file::TarFormat::from_ext(&extension);

        let file_name = effective_path.file_name().unwrap().to_string_lossy();
        let is_compressed_binary = !file_name.contains(".tar")
            && matches!(extension.as_str(), "gz" | "xz" | "bz2" | "zst");

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
    fn cache_key(&self, file_path: &Path, opts: &ToolVersionOptions) -> Result<String> {
        let checksum = hash::file_hash_blake3(file_path, None)?;

        // Include extraction options that affect output structure
        // Note: bin_path is NOT included - handled at symlink time for deduplication
        let mut parts = vec![checksum];

        if let Some(strip) = get_opt(opts, "strip_components") {
            parts.push(format!("strip_{strip}"));
        }

        // Include rename_exe in cache key since it modifies the extracted content
        if let Some(rename) = get_opt(opts, "rename_exe") {
            parts.push(format!("rename_{rename}"));
            // When rename_exe is used, bin_path affects where the rename happens,
            // so different bin_path values result in different cached content
            if let Some(bin_path) = get_opt(opts, "bin_path") {
                parts.push(format!("binpath_{bin_path}"));
            }
        }

        let key = parts.join("_");
        debug!("Cache key: {}", key);
        Ok(key)
    }

    // -------------------------------------------------------------------------
    // Filename determination
    // -------------------------------------------------------------------------

    /// Determine the destination filename for a raw file or compressed binary
    fn dest_filename(
        &self,
        file_path: &Path,
        file_info: &FileInfo,
        opts: &ToolVersionOptions,
    ) -> String {
        // Check for explicit bin name first
        if let Some(bin_name) = get_opt(opts, "bin") {
            return bin_name;
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
        if !file_info.is_compressed_binary && file_info.format != file::TarFormat::Raw {
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
        cache_key: &str,
        url: &str,
        opts: &ToolVersionOptions,
        pr: Option<&dyn SingleReport>,
    ) -> Result<ExtractionType> {
        let cache_path = self.cache_path(cache_key);

        // Ensure parent directory exists
        file::create_dir_all(Self::tarballs_dir())?;

        // Create unique temp directory for atomic extraction
        let tmp_path = Self::tarballs_dir().join(format!(
            "{}.tmp-{}-{}",
            cache_key,
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis()
        ));

        // Clean up any stale temp directory
        if tmp_path.exists() {
            let _ = file::remove_all(&tmp_path);
        }

        // Perform extraction
        let extraction_type = self.extract_artifact(tv, &tmp_path, file_path, opts, pr)?;

        // Atomic replace
        if cache_path.exists() {
            file::remove_all(&cache_path)?;
        }
        std::fs::rename(&tmp_path, &cache_path)?;

        // Write metadata
        self.write_metadata(cache_key, url, file_path, opts)?;

        Ok(extraction_type)
    }

    /// Extract a single artifact to the given directory
    fn extract_artifact(
        &self,
        tv: &ToolVersion,
        dest: &Path,
        file_path: &Path,
        opts: &ToolVersionOptions,
        pr: Option<&dyn SingleReport>,
    ) -> Result<ExtractionType> {
        file::create_dir_all(dest)?;

        let file_info = FileInfo::new(file_path, opts);

        if file_info.is_compressed_binary {
            self.extract_compressed_binary(dest, file_path, &file_info, opts, pr)
        } else if file_info.format == file::TarFormat::Raw {
            self.extract_raw_file(dest, file_path, &file_info, opts, pr)
        } else {
            self.extract_archive(tv, dest, file_path, &file_info, opts, pr)
        }
    }

    /// Extract a compressed binary (gz, xz, bz2, zst)
    fn extract_compressed_binary(
        &self,
        dest: &Path,
        file_path: &Path,
        file_info: &FileInfo,
        opts: &ToolVersionOptions,
        pr: Option<&dyn SingleReport>,
    ) -> Result<ExtractionType> {
        let filename = self.dest_filename(file_path, file_info, opts);
        let dest_file = dest.join(&filename);

        // Report extraction progress (no bytes - we don't know total for extraction)
        if let Some(pr) = pr {
            pr.set_message(format!("extract {}", file_info.file_name()));
        }

        match file_info.extension.as_str() {
            "gz" => file::un_gz(file_path, &dest_file)?,
            "xz" => file::un_xz(file_path, &dest_file)?,
            "bz2" => file::un_bz2(file_path, &dest_file)?,
            "zst" => file::un_zst(file_path, &dest_file)?,
            _ => unreachable!(),
        }

        file::make_executable(&dest_file)?;
        Ok(ExtractionType::RawFile { filename })
    }

    /// Extract a raw (uncompressed) file
    fn extract_raw_file(
        &self,
        dest: &Path,
        file_path: &Path,
        file_info: &FileInfo,
        opts: &ToolVersionOptions,
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
        file_info: &FileInfo,
        opts: &ToolVersionOptions,
        pr: Option<&dyn SingleReport>,
    ) -> Result<ExtractionType> {
        let mut strip_components: Option<usize> =
            get_opt(opts, "strip_components").and_then(|s| s.parse().ok());

        // Auto-detect strip_components=1 for single-directory archives
        if strip_components.is_none()
            && get_opt(opts, "bin_path").is_none()
            && file::should_strip_components(file_path, file_info.format).unwrap_or(false)
        {
            debug!("Auto-detected single directory archive, using strip_components=1");
            strip_components = Some(1);
        }

        let tar_opts = file::TarOptions {
            format: file_info.format,
            strip_components: strip_components.unwrap_or(0),
            pr,
            preserve_mtime: false,
        };

        file::untar(file_path, dest, &tar_opts)?;

        // Handle rename_exe option for archives
        if let Some(rename_to) = get_opt(opts, "rename_exe") {
            let search_dir = if let Some(bin_path_template) = get_opt(opts, "bin_path") {
                let bin_path = template_string(&bin_path_template, tv);
                dest.join(&bin_path)
            } else {
                dest.to_path_buf()
            };
            // rsplit('/') always yields at least one element (the full string if no delimiter)
            let tool_name = self.ba.tool_name.rsplit('/').next().unwrap();
            rename_executable_in_dir(&search_dir, &rename_to, Some(tool_name))?;
        }

        Ok(ExtractionType::Archive)
    }

    /// Write cache metadata file
    fn write_metadata(
        &self,
        cache_key: &str,
        url: &str,
        file_path: &Path,
        opts: &ToolVersionOptions,
    ) -> Result<()> {
        let metadata = CacheMetadata {
            url: url.to_string(),
            checksum: get_opt(opts, "checksum"),
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

    /// Create install symlink(s) from install directory to cache
    fn create_install_symlink(
        &self,
        tv: &ToolVersion,
        cache_key: &str,
        extraction_type: &ExtractionType,
        opts: &ToolVersionOptions,
    ) -> Result<()> {
        let cache_path = self.cache_path(cache_key);

        // Determine version name for install path
        let version_name = if tv.version == "latest" || tv.version.is_empty() {
            &cache_key[..7.min(cache_key.len())] // Content-based versioning
        } else {
            &tv.version
        };

        let install_path = tv.ba().installs_path.join(version_name);

        // Clean up existing install
        if install_path.exists() {
            file::remove_all(&install_path)?;
        }
        if let Some(parent) = install_path.parent() {
            file::create_dir_all(parent)?;
        }

        // Handle raw files with bin_path specially for deduplication
        if let ExtractionType::RawFile { filename } = extraction_type
            && let Some(bin_path_template) = get_opt(opts, "bin_path")
        {
            let bin_path = template_string(&bin_path_template, tv);
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

    /// Create additional symlink for implicit versions (latest, empty)
    fn create_version_alias_symlink(&self, tv: &ToolVersion, cache_key: &str) -> Result<()> {
        if tv.version != "latest" && !tv.version.is_empty() {
            return Ok(());
        }

        let content_version = &cache_key[..7.min(cache_key.len())];
        let original_path = tv.ba().installs_path.join(&tv.version);
        let content_path = tv.ba().installs_path.join(content_version);

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

    /// Verify or generate checksum for lockfile support
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
                .ok_or_else(|| eyre::eyre!("Invalid checksum format: {checksum}"))?;
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
                return Err(eyre::eyre!(
                    "Size mismatch for {filename}: expected {expected_size}, got {actual_size}"
                ));
            }
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
        let opts = if !self.ba.opts().contains_key("version_list_url") {
            config.get_tool_opts(&self.ba).await?.unwrap_or_default()
        } else {
            self.ba.opts()
        };

        let url = match opts.get("version_list_url") {
            Some(url) => url.clone(),
            None => return Ok(vec![]),
        };

        let regex = opts.get("version_regex").map(|s| s.as_str());
        let json_path = opts.get("version_json_path").map(|s| s.as_str());
        let version_expr = opts.get("version_expr").map(|s| s.as_str());

        version_list::fetch_versions(&url, regex, json_path, version_expr).await
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

    async fn install_operation_count(&self, tv: &ToolVersion, _ctx: &InstallContext) -> usize {
        let opts = tv.request.options();
        super::http_install_operation_count(
            get_opt(&opts, "checksum").is_some(),
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
        let opts = tv.request.options();

        // Get URL template
        let url_template = get_opt(&opts, "url").ok_or_else(|| {
            let platform_key = self.get_platform_key();
            let available = list_available_platforms_with_key(&opts, "url");
            if !available.is_empty() {
                eyre::eyre!(
                    "No URL for platform {platform_key}. Available: {}. \
                     Provide 'url' or add 'platforms.{platform_key}.url'",
                    available.join(", ")
                )
            } else {
                eyre::eyre!("Http backend requires 'url' option")
            }
        })?;

        let url = template_string(&url_template, &tv);

        // Download
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

        ctx.pr.set_message(format!("download {filename}"));
        HTTP.download_file(&url, &file_path, Some(ctx.pr.as_ref()))
            .await?;

        // Verify artifact (checksum if provided)
        if get_opt(&opts, "checksum").is_some() {
            ctx.pr.next_operation();
        }
        verify_artifact(&tv, &file_path, &opts, Some(ctx.pr.as_ref()))?;

        // Generate cache key
        let cache_key = self.cache_key(&file_path, &opts)?;
        let file_info = FileInfo::new(&file_path, &opts);

        // Acquire lock and extract or reuse cache
        let cache_path = self.cache_path(&cache_key);
        let _lock = crate::lock_file::get(&cache_path, ctx.force)?;

        // Determine extraction type based on whether we're using cache or extracting fresh
        // On cache hit, we need to detect the actual filename from the cache (which may differ
        // from current options if a previous extraction used different `bin` name)
        ctx.pr.next_operation();
        let extraction_type = if self.is_cached(&cache_key) {
            ctx.pr.set_message("using cached tarball".into());
            // Report extraction operation as complete (instant since we're using cache)
            ctx.pr.set_length(1);
            ctx.pr.set_position(1);
            self.extraction_type_from_cache(&cache_key, &file_info)
        } else {
            ctx.pr.set_message("extracting to cache".into());
            self.extract_to_cache(
                &tv,
                &file_path,
                &cache_key,
                &url,
                &opts,
                Some(ctx.pr.as_ref()),
            )?
        };

        // Create symlinks
        self.create_install_symlink(&tv, &cache_key, &extraction_type, &opts)?;
        self.create_version_alias_symlink(&tv, &cache_key)?;

        // Verify checksum for lockfile
        if lockfile_enabled || has_lockfile_checksum {
            ctx.pr.next_operation();
        }
        self.verify_checksum(ctx, &mut tv, &file_path)?;

        Ok(tv)
    }

    async fn list_bin_paths(
        &self,
        _config: &Arc<Config>,
        tv: &ToolVersion,
    ) -> Result<Vec<PathBuf>> {
        let opts = tv.request.options();

        // Check for explicit bin_path
        if let Some(bin_path_template) = get_opt(&opts, "bin_path") {
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
