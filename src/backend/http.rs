use crate::backend::Backend;
use crate::backend::backend_type::BackendType;
use crate::backend::static_helpers::{
    clean_binary_name, get_filename_from_url, list_available_platforms_with_key,
    lookup_platform_key, template_string, verify_artifact,
};
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

#[derive(Debug, Serialize, Deserialize)]
struct CacheMetadata {
    url: String,
    checksum: Option<String>,
    size: u64,
    extracted_at: u64,
    platform: String,
}

#[derive(Debug)]
pub struct HttpBackend {
    ba: Arc<BackendArg>,
}

impl HttpBackend {
    pub fn from_arg(ba: BackendArg) -> Self {
        Self { ba: Arc::new(ba) }
    }

    /// Generate a cache key based on the actual file content (checksum) and extraction options
    fn get_file_based_cache_key(
        &self,
        file_path: &Path,
        opts: &ToolVersionOptions,
    ) -> Result<String> {
        let checksum = hash::file_hash_blake3(file_path, None)?;

        // Include extraction options in cache key to handle different extraction needs
        let mut cache_key_parts = vec![checksum.clone()];

        if let Some(strip_components) = opts.get("strip_components") {
            cache_key_parts.push(format!("strip_{strip_components}"));
        }

        let cache_key = cache_key_parts.join("_");
        debug!("Using file-based checksum as cache key: {}", cache_key);
        Ok(cache_key)
    }

    /// Get the path to the cached tarball directory
    fn get_cached_tarball_path(&self, cache_key: &str) -> PathBuf {
        dirs::CACHE.join("http-tarballs").join(cache_key)
    }

    /// Get the path to the extracted contents within the cache
    fn get_cached_extracted_path(&self, cache_key: &str) -> PathBuf {
        self.get_cached_tarball_path(cache_key)
    }

    /// Get the path to the metadata file
    fn get_cache_metadata_path(&self, cache_key: &str) -> PathBuf {
        self.get_cached_tarball_path(cache_key)
            .join("metadata.json")
    }

    /// Check if a tarball is already cached
    fn is_tarball_cached(&self, cache_key: &str) -> bool {
        let extracted_path = self.get_cached_extracted_path(cache_key);
        let metadata_path = self.get_cache_metadata_path(cache_key);
        extracted_path.exists() && metadata_path.exists()
    }

    /// Extract tarball to cache directory
    fn extract_to_cache(
        &self,
        file_path: &Path,
        cache_key: &str,
        url: &str,
        tv: &ToolVersion,
        opts: &ToolVersionOptions,
        pr: Option<&dyn SingleReport>,
    ) -> Result<()> {
        let cache_path = self.get_cached_tarball_path(cache_key);
        let extracted_path = self.get_cached_extracted_path(cache_key);
        let metadata_path = self.get_cache_metadata_path(cache_key);

        // Ensure parent directory exists (we'll atomically rename into it)
        if let Some(parent) = cache_path.parent() {
            file::create_dir_all(parent)?;
        }

        // Create a unique temporary directory for atomic extraction
        let pid = std::process::id();
        let now_ms = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
        let tmp_dir_name = format!("{}.tmp-{}-{}", cache_key, pid, now_ms);
        let tmp_extract_path = match cache_path.parent() {
            Some(parent) => parent.join(tmp_dir_name),
            None => cache_path.with_extension(format!("tmp-{}-{}", pid, now_ms)),
        };

        // Ensure any previous temp dir is removed
        if tmp_extract_path.exists() {
            let _ = file::remove_all(&tmp_extract_path);
        }

        // Perform extraction into the temp directory
        self.extract_artifact_to_cache(file_path, &tmp_extract_path, tv, opts, pr)?;

        // Replace any existing extracted cache atomically
        if extracted_path.exists() {
            file::remove_all(&extracted_path)?;
        }
        // Rename temp directory to the final cache path
        std::fs::rename(&tmp_extract_path, &extracted_path)?;

        // Only write metadata after the extracted directory is in place
        let metadata = CacheMetadata {
            url: url.to_string(),
            checksum: lookup_platform_key(opts, "checksum")
                .or_else(|| opts.get("checksum").cloned()),
            size: file_path.metadata()?.len(),
            extracted_at: SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
            platform: self.get_platform_key(),
        };

        let metadata_json = serde_json::to_string_pretty(&metadata)?;
        file::write(&metadata_path, metadata_json)?;

        Ok(())
    }

    /// Extract artifact to cache directory (similar to install_artifact but for cache)
    fn extract_artifact_to_cache(
        &self,
        file_path: &Path,
        cache_path: &Path,
        tv: &ToolVersion,
        opts: &ToolVersionOptions,
        pr: Option<&dyn SingleReport>,
    ) -> Result<()> {
        let mut strip_components = opts.get("strip_components").and_then(|s| s.parse().ok());

        file::create_dir_all(cache_path)?;

        // Use TarFormat for format detection
        let ext = file_path.extension().and_then(|s| s.to_str()).unwrap_or("");
        let format = file::TarFormat::from_ext(ext);

        // Get file extension and detect format
        let file_name = file_path.file_name().unwrap().to_string_lossy();

        // Check if it's a compressed binary (not a tar archive)
        let is_compressed_binary =
            !file_name.contains(".tar") && matches!(ext, "gz" | "xz" | "bz2" | "zst");

        if is_compressed_binary {
            // Handle compressed single binary
            let decompressed_name = file_name.trim_end_matches(&format!(".{}", ext));

            // Determine the destination path
            let (dest_dir, dest_filename) = if let Some(bin_path_template) = opts.get("bin_path") {
                // If bin_path is specified, use it as directory
                let bin_path = template_string(bin_path_template, tv);
                let bin_dir = cache_path.join(&bin_path);
                (bin_dir, std::ffi::OsString::from(decompressed_name))
            } else if let Some(bin_name) = opts.get("bin") {
                // If bin is specified, rename the file to this name
                (cache_path.to_path_buf(), std::ffi::OsString::from(bin_name))
            } else {
                // Always auto-clean binary names by removing OS/arch suffixes
                let cleaned_name = clean_binary_name(decompressed_name, Some(&self.ba.tool_name));
                (
                    cache_path.to_path_buf(),
                    std::ffi::OsString::from(cleaned_name),
                )
            };

            // Create the destination directory
            file::create_dir_all(&dest_dir)?;

            // Construct full destination path
            let dest = dest_dir.join(&dest_filename);

            match ext {
                "gz" => file::un_gz(file_path, &dest)?,
                "xz" => file::un_xz(file_path, &dest)?,
                "bz2" => file::un_bz2(file_path, &dest)?,
                "zst" => file::un_zst(file_path, &dest)?,
                _ => unreachable!(),
            }

            file::make_executable(&dest)?;
        } else if format == file::TarFormat::Raw {
            // For raw files, determine the destination
            let (dest_dir, dest_filename) = if let Some(bin_path_template) = opts.get("bin_path") {
                // If bin_path is specified, use it as directory
                let bin_path = template_string(bin_path_template, tv);
                let bin_dir = cache_path.join(&bin_path);
                (bin_dir, file_path.file_name().unwrap().to_os_string())
            } else if let Some(bin_name) = opts.get("bin") {
                // If bin is specified, rename the file to this name
                (cache_path.to_path_buf(), std::ffi::OsString::from(bin_name))
            } else {
                // Always auto-clean binary names by removing OS/arch suffixes
                let original_name = file_path.file_name().unwrap().to_string_lossy();
                let cleaned_name = clean_binary_name(&original_name, Some(&self.ba.tool_name));
                (
                    cache_path.to_path_buf(),
                    std::ffi::OsString::from(cleaned_name),
                )
            };

            // Create the destination directory
            file::create_dir_all(&dest_dir)?;

            // Construct full destination path
            let dest = dest_dir.join(&dest_filename);

            file::copy(file_path, &dest)?;
            file::make_executable(&dest)?;
        } else {
            // Auto-detect if we need strip_components=1 before extracting
            // Only auto-strip if strip_components is not set AND bin_path is not explicitly configured
            if strip_components.is_none() && opts.get("bin_path").is_none() {
                if let Ok(should_strip) = file::should_strip_components(file_path, format) {
                    if should_strip {
                        debug!(
                            "Auto-detected single directory archive, extracting with strip_components=1"
                        );
                        strip_components = Some(1);
                    }
                }
            }

            let tar_opts = file::TarOptions {
                format,
                strip_components: strip_components.unwrap_or(0),
                pr,
                preserve_mtime: false, // Bump mtime when extracting to cache
            };

            // Extract with determined strip_components
            file::untar(file_path, cache_path, &tar_opts)?;
        }

        Ok(())
    }

    /// Create symlink from install directory to cache
    fn create_install_symlink(
        &self,
        tv: &ToolVersion,
        cache_path: &Path,
        cache_key: &str,
    ) -> Result<()> {
        // Determine the appropriate version name for the symlink
        let version_name = if tv.version == "latest" || tv.version.is_empty() {
            // Use content-based versioning for implicit versions
            &cache_key[..7.min(cache_key.len())]
        } else {
            // Use the original version name for explicit versions
            &tv.version
        };

        let version_install_path = tv.ba().installs_path.join(version_name);

        // Remove existing install path if it exists
        if version_install_path.exists() {
            file::remove_all(&version_install_path)?;
        }

        // Create parent directory for symlink
        if let Some(parent) = version_install_path.parent() {
            file::create_dir_all(parent)?;
        }

        // Create symlink
        file::make_symlink(cache_path, &version_install_path)?;

        Ok(())
    }

    /// Verify checksum if specified (moved from trait implementation)
    fn verify_checksum(
        &self,
        ctx: &InstallContext,
        tv: &mut ToolVersion,
        file_path: &Path,
    ) -> Result<()> {
        let settings = Settings::get();
        let filename = file_path.file_name().unwrap().to_string_lossy().to_string();
        let lockfile_enabled = settings.lockfile && settings.experimental;

        // Get the platform key for this tool and platform
        let platform_key = self.get_platform_key();

        // Get or create asset info for this platform
        let platform_info = tv.lock_platforms.entry(platform_key.clone()).or_default();

        if let Some(checksum) = &platform_info.checksum {
            ctx.pr.set_message(format!("checksum {filename}"));
            if let Some((algo, check)) = checksum.split_once(':') {
                hash::ensure_checksum(file_path, check, Some(ctx.pr.as_ref()), algo)?;
            } else {
                return Err(eyre::eyre!("Invalid checksum: {checksum}"));
            }
        } else if lockfile_enabled {
            ctx.pr.set_message(format!("generate checksum {filename}"));
            let hash = hash::file_hash_blake3(file_path, Some(ctx.pr.as_ref()))?;
            platform_info.checksum = Some(format!("blake3:{hash}"));
        }

        // Handle size verification and generation
        if let Some(expected_size) = platform_info.size {
            ctx.pr.set_message(format!("verify size {filename}"));
            let actual_size = file_path.metadata()?.len();
            if actual_size != expected_size {
                return Err(eyre::eyre!(
                    "Size mismatch for {}: expected {}, got {}",
                    filename,
                    expected_size,
                    actual_size
                ));
            }
        } else if lockfile_enabled {
            ctx.pr.set_message(format!("record size {filename}"));
            let size = file_path.metadata()?.len();
            platform_info.size = Some(size);
        }
        Ok(())
    }
}

#[async_trait]
impl Backend for HttpBackend {
    fn get_type(&self) -> BackendType {
        BackendType::Http
    }

    fn ba(&self) -> &Arc<BackendArg> {
        &self.ba
    }

    async fn _list_remote_versions(&self, _config: &Arc<Config>) -> Result<Vec<String>> {
        // Http backend doesn't support remote version listing
        Ok(vec![])
    }

    async fn install_version_(
        &self,
        ctx: &InstallContext,
        mut tv: ToolVersion,
    ) -> Result<ToolVersion> {
        let opts = tv.request.options();

        // Use the new helper to get platform-specific URL first, then fall back to general URL
        let url_template = lookup_platform_key(&opts, "url")
            .or_else(|| opts.get("url").cloned())
            .ok_or_else(|| {
                let platform_key = self.get_platform_key();
                let available = list_available_platforms_with_key(&opts, "url");
                if !available.is_empty() {
                    let list = available.join(", ");
                    eyre::eyre!(
                        "No URL configured for platform {platform_key}. Available platforms: {list}. Provide 'url' or add 'platforms.{platform_key}.url'"
                    )
                } else {
                    eyre::eyre!("Http backend requires 'url' option")
                }
            })?;

        // Template the URL with actual values
        let url = template_string(&url_template, &tv);

        // Download
        let filename = get_filename_from_url(&url);
        let file_path = tv.download_path().join(&filename);

        // Store the asset URL in the tool version
        let platform_key = self.get_platform_key();
        let platform_info = tv.lock_platforms.entry(platform_key).or_default();
        platform_info.url = Some(url.clone());

        ctx.pr.set_message(format!("download {filename}"));
        HTTP.download_file(&url, &file_path, Some(ctx.pr.as_ref()))
            .await?;

        // Verify (shared)
        verify_artifact(&tv, &file_path, &opts, Some(ctx.pr.as_ref()))?;

        // Generate cache key - always use Blake3 hash of the file for consistency
        // This ensures that the same file content always gets the same cache key
        // regardless of whether a checksum was provided or what algorithm was used
        let cache_key = self.get_file_based_cache_key(&file_path, &opts)?;
        let cached_extracted_path = self.get_cached_extracted_path(&cache_key);

        // Acquire a cache-level lock to serialize extraction for this cache key
        let _cache_lock = crate::lock_file::get(&cached_extracted_path, ctx.force)?;

        // Check if tarball is already cached
        if self.is_tarball_cached(&cache_key) {
            ctx.pr.set_message("using cached tarball".into());
        } else {
            ctx.pr.set_message("extracting to cache".into());
            self.extract_to_cache(
                &file_path,
                &cache_key,
                &url,
                &tv,
                &opts,
                Some(ctx.pr.as_ref()),
            )?;
        }

        // Create symlink from install directory to cache
        let content_version = &cache_key[..7.min(cache_key.len())]; // First 7 chars like git
        self.create_install_symlink(&tv, &cached_extracted_path, &cache_key)?;

        // For implicit versions, also create a symlink with the original version name
        // pointing to our content-based version to maintain compatibility
        if tv.version == "latest" || tv.version.is_empty() {
            let original_install_path = tv.ba().installs_path.join(&tv.version);
            let content_install_path = tv.ba().installs_path.join(content_version);

            // Remove any existing directory at the original path
            if original_install_path.exists() {
                file::remove_all(&original_install_path)?;
            }

            // Create parent directory if needed
            if let Some(parent) = original_install_path.parent() {
                file::create_dir_all(parent)?;
            }

            // Create symlink from original version to content-based version
            file::make_symlink(&content_install_path, &original_install_path)?;
        }

        // Verify checksum if specified
        self.verify_checksum(ctx, &mut tv, &file_path)?;

        Ok(tv)
    }

    async fn list_bin_paths(
        &self,
        _config: &Arc<Config>,
        tv: &ToolVersion,
    ) -> Result<Vec<std::path::PathBuf>> {
        let opts = tv.request.options();
        if let Some(bin_path_template) = opts.get("bin_path") {
            let bin_path = template_string(bin_path_template, tv);
            Ok(vec![tv.install_path().join(bin_path)])
        } else {
            // Look for bin directory in the install path
            let bin_path = tv.install_path().join("bin");
            if bin_path.exists() {
                Ok(vec![bin_path])
            } else {
                // Look for bin directory in subdirectories (for extracted archives)
                let mut paths = Vec::new();
                if let Ok(entries) = std::fs::read_dir(tv.install_path()) {
                    for entry in entries.flatten() {
                        let entry_path = entry.path();
                        // Only check directories, not files
                        if entry_path.is_dir() {
                            let sub_bin_path = entry_path.join("bin");
                            if sub_bin_path.exists() {
                                paths.push(sub_bin_path);
                            }
                        }
                    }
                }
                if !paths.is_empty() {
                    Ok(paths)
                } else {
                    Ok(vec![tv.install_path()])
                }
            }
        }
    }
}
