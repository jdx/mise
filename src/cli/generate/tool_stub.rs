use crate::Result;
use crate::backend::asset_matcher::detect_platform_from_url;
use crate::backend::platform_target::PlatformTarget;
use crate::backend::static_helpers::get_filename_from_url;
use crate::cli::tool_stub::ToolStubFile;
use crate::config::Config;
use crate::file::{self, TarFormat, TarOptions};
use crate::http::HTTP;
use crate::lockfile::PlatformInfo;
use crate::minisign;
use crate::platform::Platform;
use crate::toolset::{ResolveOptions, ToolVersion};
use crate::ui::info;
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::ui::progress_report::SingleReport;
use clap::ValueHint;
use color_eyre::eyre::bail;
use humansize::{BINARY, format_size};
use indexmap::IndexMap;
use std::collections::BTreeMap;
use std::path::PathBuf;
use toml_edit::DocumentMut;
use xx::file::display_path;

/// Generate a tool stub for HTTP-based tools
///
/// This command generates tool stubs that can automatically download and execute
/// tools from HTTP URLs. It can detect checksums, file sizes, and binary paths
/// automatically by downloading and analyzing the tool.
///
/// When generating stubs with platform-specific URLs, the command will append new
/// platforms to existing stub files rather than overwriting them. This allows you
/// to incrementally build cross-platform tool stubs.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct ToolStub {
    /// Output file path for the tool stub
    #[clap(value_hint = ValueHint::FilePath)]
    pub output: PathBuf,

    /// Binary path within the extracted archive
    ///
    /// If not specified and the archive is downloaded, will auto-detect the most likely binary
    #[clap(long, short)]
    pub bin: Option<String>,

    /// Wrap stub in a bootstrap script that installs mise if not already present
    ///
    /// When enabled, generates a bash script that:
    /// 1. Checks if mise is installed at the expected path
    /// 2. If not, downloads and installs mise using the embedded installer
    /// 3. Executes the tool stub using mise
    #[clap(long, verbatim_doc_comment)]
    pub bootstrap: bool,

    /// Specify mise version for the bootstrap script
    ///
    /// By default, uses the latest version from the install script.
    /// Use this to pin to a specific version (e.g., "2025.1.0").
    #[clap(long, verbatim_doc_comment, requires = "bootstrap")]
    pub bootstrap_version: Option<String>,

    /// Fetch checksums and sizes for an existing tool stub file
    ///
    /// This reads an existing stub file and fills in any missing checksum/size fields
    /// by downloading the files. URLs must already be present in the stub.
    #[clap(long, conflicts_with_all = &["url", "platform_url", "version", "bin", "platform_bin", "skip_download", "lock"])]
    pub fetch: bool,

    /// HTTP backend type to use
    #[clap(long, default_value = "http")]
    pub http: String,

    /// Resolve and embed lockfile data (exact version + platform URLs/checksums)
    /// into an existing stub file for reproducible installs without runtime API calls
    #[clap(long, conflicts_with_all = &["url", "platform_url", "bin", "platform_bin", "fetch", "skip_download"])]
    pub lock: bool,

    /// Platform-specific binary paths in the format platform:path
    ///
    /// Examples: --platform-bin windows-x64:tool.exe --platform-bin linux-x64:bin/tool
    #[clap(long)]
    pub platform_bin: Vec<String>,

    /// Platform-specific URLs in the format platform:url or just url (auto-detect platform)
    ///
    /// When the output file already exists, new platforms will be appended to the existing
    /// platforms table. Existing platform URLs will be updated if specified again.
    ///
    /// If only a URL is provided (without platform:), the platform will be automatically
    /// detected from the URL filename.
    ///
    /// Examples:
    /// --platform-url linux-x64:https://...
    /// --platform-url https://nodejs.org/dist/v22.17.1/node-v22.17.1-darwin-arm64.tar.gz
    #[clap(long)]
    pub platform_url: Vec<String>,

    /// Skip downloading for checksum and binary path detection (faster but less informative)
    #[clap(long)]
    pub skip_download: bool,

    /// URL for downloading the tool
    ///
    /// Example: https://github.com/owner/repo/releases/download/v2.0.0/tool-linux-x64.tar.gz
    #[clap(long, short)]
    pub url: Option<String>,

    /// Version of the tool
    #[clap(long, default_value = "latest")]
    pub version: String,
}

impl ToolStub {
    pub async fn run(self) -> Result<()> {
        let stub_content = if self.fetch {
            self.fetch_checksums().await?
        } else if self.lock {
            self.lock_stub().await?
        } else {
            self.generate_stub().await?
        };

        if let Some(parent) = self.output.parent() {
            file::create_dir_all(parent)?;
        }

        file::write(&self.output, &stub_content)?;
        file::make_executable(&self.output)?;

        if self.fetch || self.lock {
            miseprintln!("Updated tool stub: {}", display_path(&self.output));
        } else {
            miseprintln!("Generated tool stub: {}", display_path(&self.output));
        }
        Ok(())
    }

    fn get_tool_name(&self) -> String {
        self.output
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("tool")
            .to_string()
    }

    async fn generate_stub(&self) -> Result<String> {
        // Validate that either URL or platform URLs are provided
        if self.url.is_none() && self.platform_url.is_empty() {
            bail!("Either --url or --platform-url must be specified");
        }

        // Read existing file if it exists
        let (existing_content, mut doc) = if self.output.exists() {
            let content = file::read_to_string(&self.output)?;
            let toml_content = extract_toml_from_stub(&content);

            let document = toml_content.parse::<DocumentMut>()?;
            (Some(content), document)
        } else {
            (None, DocumentMut::new())
        };

        // If file exists but we're trying to set a different version, bail
        if existing_content.is_some() && doc.get("version").is_some() {
            let existing_version = doc.get("version").and_then(|v| v.as_str()).unwrap_or("");
            if existing_version != self.version {
                bail!(
                    "Cannot change version of existing tool stub from {} to {}",
                    existing_version,
                    self.version
                );
            }
        }

        // Set or update version only if it's not "latest" (which should be the default)
        if self.version != "latest" {
            doc["version"] = toml_edit::value(&self.version);
        }

        // Get the stub filename (without path)
        let stub_filename = self
            .output
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("");

        // Update bin if provided and different from stub filename
        if let Some(bin) = &self.bin
            && bin != stub_filename
        {
            doc["bin"] = toml_edit::value(bin);
        }

        // We use toml_edit directly to preserve existing content

        // Handle URL or platform-specific URLs
        if let Some(url) = &self.url {
            doc["url"] = toml_edit::value(url);

            // Auto-detect checksum and size if not skipped
            if !self.skip_download {
                let mpr = MultiProgressReport::get();
                let (checksum, size, bin_path) = self.analyze_url(url, &mpr).await?;
                doc["checksum"] = toml_edit::value(&checksum);

                // Create size entry with human-readable comment
                let mut size_item = toml_edit::value(size as i64);
                if let Some(value) = size_item.as_value_mut() {
                    let formatted_comment = format_size_comment(size);
                    value.decor_mut().set_suffix(formatted_comment);
                }
                doc["size"] = size_item;

                if self.bin.is_none()
                    && let Some(detected_bin) = bin_path.as_ref()
                {
                    // Only set bin if it's different from the stub filename
                    if detected_bin != stub_filename {
                        doc["bin"] = toml_edit::value(detected_bin);
                    }
                }
            }
        }

        if !self.platform_url.is_empty() {
            let mpr = MultiProgressReport::get();

            // Ensure platforms table exists
            if doc.get("platforms").is_none() {
                let mut platforms_table = toml_edit::Table::new();
                platforms_table.set_implicit(true);
                doc["platforms"] = toml_edit::Item::Table(platforms_table);
            }
            let platforms = doc["platforms"].as_table_mut().unwrap();

            // Parse platform-specific bin paths
            let mut explicit_platform_bins = IndexMap::new();
            for platform_bin_spec in &self.platform_bin {
                let (platform, bin_path) = self.parse_platform_bin_spec(platform_bin_spec)?;
                explicit_platform_bins.insert(platform, bin_path);
            }

            // Track detected bin paths to see if they're all the same
            let mut detected_bin_paths = Vec::new();

            for platform_spec in &self.platform_url {
                let (platform, url) = self.parse_platform_spec(platform_spec)?;

                // Create or get platform table
                if platforms.get(&platform).is_none() {
                    platforms[&platform] = toml_edit::table();
                }
                let platform_table = platforms[&platform].as_table_mut().unwrap();

                // Set URL
                platform_table["url"] = toml_edit::value(&url);

                // Set platform-specific bin path if explicitly provided and different from stub filename
                if let Some(explicit_bin) = explicit_platform_bins.get(&platform)
                    && explicit_bin != stub_filename
                {
                    platform_table["bin"] = toml_edit::value(explicit_bin);
                }

                // Auto-detect checksum, size, and bin path if not skipped
                if !self.skip_download {
                    let (checksum, size, bin_path) = self.analyze_url(&url, &mpr).await?;
                    platform_table["checksum"] = toml_edit::value(&checksum);

                    // Create size entry with human-readable comment
                    let mut size_item = toml_edit::value(size as i64);
                    if let Some(value) = size_item.as_value_mut() {
                        let formatted_comment = format_size_comment(size);
                        value.decor_mut().set_suffix(formatted_comment);
                    }
                    platform_table["size"] = size_item;

                    // Track detected bin paths
                    if let Some(ref bp) = bin_path {
                        detected_bin_paths.push(bp.clone());
                    }

                    // Set bin path if not explicitly provided and we detected one different from stub filename
                    if !explicit_platform_bins.contains_key(&platform)
                        && self.bin.is_none()
                        && let Some(detected_bin) = bin_path.as_ref()
                        && detected_bin != stub_filename
                    {
                        platform_table["bin"] = toml_edit::value(detected_bin);
                    }
                }
            }

            // Check if we should set a global bin
            let should_set_global_bin = if self.bin.is_none() && !detected_bin_paths.is_empty() {
                let first_bin = &detected_bin_paths[0];
                detected_bin_paths.iter().all(|b| b == first_bin)
            } else {
                false
            };

            if should_set_global_bin {
                let global_bin = detected_bin_paths[0].clone();
                // Remove platform-specific bin entries since we'll have a global one
                for platform_spec in &self.platform_url {
                    let (platform, _) = self.parse_platform_spec(platform_spec)?;
                    if let Some(platform_table) = platforms.get_mut(&platform)
                        && let Some(table) = platform_table.as_table_mut()
                        && !explicit_platform_bins.contains_key(&platform)
                    {
                        table.remove("bin");
                    }
                }
                // Now set the global bin if different from stub filename
                if global_bin != stub_filename {
                    doc["bin"] = toml_edit::value(&global_bin);
                }
            }
        }

        let toml_content = doc.to_string();

        // Check if we should use bootstrap format:
        // 1. If --bootstrap flag is explicitly set
        // 2. If existing file was a bootstrap stub (preserve format when appending)
        let use_bootstrap = self.bootstrap
            || existing_content
                .as_ref()
                .map(|c| is_bootstrap_stub(c))
                .unwrap_or(false);

        if use_bootstrap {
            self.wrap_with_bootstrap(&toml_content).await
        } else {
            let mut content = vec![
                "#!/usr/bin/env -S mise tool-stub".to_string(),
                "".to_string(),
            ];

            content.push(toml_content);

            Ok(content.join("\n"))
        }
    }

    async fn wrap_with_bootstrap(&self, toml_content: &str) -> Result<String> {
        // Fetch and verify install.sh (same approach as bootstrap.rs)
        // Use versioned URL if a specific version is requested
        let url = if let Some(v) = &self.bootstrap_version {
            format!("https://mise.jdx.dev/v{v}/install.sh")
        } else {
            "https://mise.jdx.dev/install.sh".to_string()
        };
        let install = HTTP.get_text(&url).await?;
        let install_sig = HTTP.get_text(format!("{url}.minisig")).await?;
        minisign::verify(&minisign::MISE_PUB_KEY, install.as_bytes(), &install_sig)?;
        let install = info::indent_by(install, "        ");

        // Store TOML in a comment block - mise tool-stub will parse this from the script
        let commented_toml = toml_content
            .lines()
            .map(|line| format!("# {line}"))
            .collect::<Vec<_>>()
            .join("\n");

        Ok(format!(
            r##"#!/usr/bin/env bash
set -eu

# MISE_TOOL_STUB:
{commented_toml}
# :MISE_TOOL_STUB

__mise_tool_stub_bootstrap() {{
    # Check if mise is on PATH first
    if command -v mise &>/dev/null; then
        MISE_BIN="$(command -v mise)"
        return
    fi

    # Fall back to ~/.local/bin/mise
    MISE_BIN="$HOME/.local/bin/mise"
    if [ -f "$MISE_BIN" ]; then
        return
    fi

    # Install mise to ~/.local/bin
    install() {{
        local initial_working_dir="$PWD"
{install}
        cd -- "$initial_working_dir"
    }}
    local MISE_INSTALL_HELP=0
    install
}}
__mise_tool_stub_bootstrap

exec "$MISE_BIN" tool-stub "$0" "$@"
"##
        ))
    }

    fn parse_platform_spec(&self, spec: &str) -> Result<(String, String)> {
        // Check if this is a URL first (auto-detect case)
        if spec.starts_with("http://") || spec.starts_with("https://") {
            // Format: url (auto-detect platform)
            let url = spec.to_string();

            if let Some(detected_platform) = detect_platform_from_url(&url) {
                let platform = detected_platform.to_platform_string();
                debug!("Auto-detected platform '{}' from URL: {}", platform, url);
                Ok((platform, url))
            } else {
                bail!(
                    "Could not auto-detect platform from URL: {}. Please specify explicitly using 'platform:url' format.",
                    url
                );
            }
        } else {
            // Format: platform:url
            let parts: Vec<&str> = spec.splitn(2, ':').collect();
            if parts.len() != 2 {
                bail!(
                    "Platform spec must be in format 'platform:url' or just 'url' (for auto-detection). Got: {}",
                    spec
                );
            }

            let platform = parts[0].to_string();
            let url = parts[1].to_string();
            Ok((platform, url))
        }
    }

    fn parse_platform_bin_spec(&self, spec: &str) -> Result<(String, String)> {
        let parts: Vec<&str> = spec.splitn(2, ':').collect();
        if parts.len() != 2 {
            bail!(
                "Platform bin spec must be in format 'platform:path', got: {}",
                spec
            );
        }

        let platform = parts[0].to_string();
        let bin_path = parts[1].to_string();

        Ok((platform, bin_path))
    }

    async fn analyze_url(
        &self,
        url: &str,
        mpr: &std::sync::Arc<crate::ui::multi_progress_report::MultiProgressReport>,
    ) -> Result<(String, u64, Option<String>)> {
        // Create a temporary directory for download and extraction
        let temp_dir = tempfile::tempdir()?;
        let filename = get_filename_from_url(url);
        let archive_path = temp_dir.path().join(&filename);

        // Create one progress reporter for the entire operation
        let pr = mpr.add(&format!("download {filename}"));

        // Download using mise's HTTP client
        HTTP.download_file(url, &archive_path, Some(pr.as_ref()))
            .await?;

        // Read the file to calculate checksum and size
        let bytes = file::read(&archive_path)?;
        let size = bytes.len() as u64;
        let checksum = format!("blake3:{}", blake3::hash(&bytes).to_hex());

        // Detect binary path if this is an archive
        let bin_path = if self.is_archive_format(url) {
            // Update progress message for extraction and reuse the same progress reporter
            pr.set_message(format!("extract {filename}"));
            match self
                .extract_and_find_binary(&archive_path, &temp_dir, &filename, pr.as_ref())
                .await
            {
                Ok(path) => {
                    pr.finish();
                    Some(path)
                }
                Err(_) => {
                    pr.finish();
                    None
                }
            }
        } else {
            // For single binary files, just use the tool name
            pr.finish();
            Some(self.get_tool_name())
        };

        Ok((checksum, size, bin_path))
    }

    async fn extract_and_find_binary(
        &self,
        archive_path: &std::path::Path,
        temp_dir: &tempfile::TempDir,
        _filename: &str,
        pr: &dyn SingleReport,
    ) -> Result<String> {
        // Try to extract and find executables
        let extracted_dir = temp_dir.path().join("extracted");
        std::fs::create_dir_all(&extracted_dir)?;

        // Try extraction using mise's built-in extraction logic (reuse the passed progress reporter)
        let tar_opts = TarOptions {
            format: TarFormat::Auto,
            strip_components: 0,
            pr: Some(pr),
            ..Default::default()
        };
        file::untar(archive_path, &extracted_dir, &tar_opts)?;

        // Check if strip_components would be applied during actual installation
        let format = TarFormat::from_ext(
            &archive_path
                .extension()
                .unwrap_or_default()
                .to_string_lossy(),
        );
        let will_strip = file::should_strip_components(archive_path, format)?;

        // Find executable files
        let executables = self.find_executables(&extracted_dir)?;
        if executables.is_empty() {
            bail!("No executable files found in archive");
        }

        // Look for exact filename match only
        let tool_name = self.get_tool_name();
        let selected_exe = self.find_exact_binary_match(&executables, &tool_name)?;

        // If strip_components will be applied, remove the first path component
        if will_strip {
            let path = std::path::Path::new(&selected_exe);
            if let Ok(stripped) = path.strip_prefix(path.components().next().unwrap()) {
                let stripped_str = stripped.to_string_lossy().to_string();
                // Don't return empty string if stripping removed everything
                if !stripped_str.is_empty() {
                    return Ok(stripped_str);
                }
            }
        }

        Ok(selected_exe)
    }

    fn is_archive_format(&self, url: &str) -> bool {
        // Check if the URL appears to be an archive format that mise can extract
        url.ends_with(".tar.gz")
            || url.ends_with(".tgz")
            || url.ends_with(".tar.xz")
            || url.ends_with(".txz")
            || url.ends_with(".tar.bz2")
            || url.ends_with(".tbz2")
            || url.ends_with(".tar.zst")
            || url.ends_with(".tzst")
            || url.ends_with(".zip")
            || url.ends_with(".7z")
    }

    fn find_executables(&self, dir: &std::path::Path) -> Result<Vec<String>> {
        let mut executables = Vec::new();

        for entry in walkdir::WalkDir::new(dir) {
            let entry = entry?;
            if entry.file_type().is_file() {
                let path = entry.path();
                if file::is_executable(path)
                    && let Ok(relative_path) = path.strip_prefix(dir)
                {
                    executables.push(relative_path.to_string_lossy().to_string());
                }
            }
        }

        Ok(executables)
    }

    fn find_exact_binary_match(&self, executables: &[String], tool_name: &str) -> Result<String> {
        if executables.is_empty() {
            bail!("No executable files found in archive");
        }

        // Look for exact filename matches (with or without extensions)
        for exe in executables {
            let path = std::path::Path::new(exe);
            if let Some(filename) = path.file_name().and_then(|f| f.to_str()) {
                // Check exact filename match
                if filename == tool_name {
                    return Ok(exe.clone());
                }
                // Check filename without extension
                if let Some(stem) = path.file_stem().and_then(|f| f.to_str())
                    && stem == tool_name
                {
                    return Ok(exe.clone());
                }
            }
        }

        // No exact match found, try to find the most likely binary
        let selected_exe = self.find_best_binary_match(executables, tool_name)?;
        Ok(selected_exe)
    }

    fn find_best_binary_match(&self, executables: &[String], tool_name: &str) -> Result<String> {
        // Strategy 1: Look for executables in common bin directories
        let bin_executables: Vec<_> = executables
            .iter()
            .filter(|exe| {
                let path = std::path::Path::new(exe);
                path.parent()
                    .and_then(|p| p.file_name())
                    .and_then(|n| n.to_str())
                    .map(|n| n == "bin" || n == "sbin")
                    .unwrap_or(false)
            })
            .collect();

        // If there's exactly one executable in a bin directory, use it
        if bin_executables.len() == 1 {
            return Ok(bin_executables[0].clone());
        }

        // Strategy 2: Look for executables that contain the tool name
        let name_matches: Vec<_> = executables
            .iter()
            .filter(|exe| {
                let path = std::path::Path::new(exe);
                path.file_name()
                    .and_then(|f| f.to_str())
                    .map(|f| f.to_lowercase().contains(&tool_name.to_lowercase()))
                    .unwrap_or(false)
            })
            .collect();

        // If there's exactly one match containing the tool name, use it
        if name_matches.len() == 1 {
            return Ok(name_matches[0].clone());
        }

        // Strategy 3: If there's only one executable total, use it
        if executables.len() == 1 {
            return Ok(executables[0].clone());
        }

        // No good match found, provide helpful error message
        let mut exe_list = executables.to_vec();
        exe_list.sort();

        bail!(
            "Could not determine which executable to use for '{}'. Available executables:\n  {}\n\nUse --bin to specify the correct binary path.",
            tool_name,
            exe_list.join("\n  ")
        );
    }

    async fn lock_stub(&self) -> Result<String> {
        if !self.output.exists() {
            bail!(
                "Tool stub file does not exist: {}",
                display_path(&self.output)
            );
        }

        let mut stub = ToolStubFile::from_file(&self.output)?;
        let config = Config::get().await?;

        // Allow --version to override the version in the stub for bumping
        if self.version != "latest" {
            stub.version = self.version.clone();
        }

        // Create tool request and resolve version
        let request = stub.to_tool_request(&self.output)?;
        let backend = request.ba().backend()?;
        let resolve_opts = ResolveOptions {
            use_locked_version: false,
            ..Default::default()
        };
        let tv = ToolVersion::resolve(&config, request, &resolve_opts).await?;

        // Resolve lock info for each common platform (including variants)
        let mut lock_platforms: BTreeMap<String, PlatformInfo> = BTreeMap::new();
        for p in Platform::common_platforms() {
            for platform in backend.platform_variants(&p) {
                let target = PlatformTarget::new(platform);
                match backend.resolve_lock_info(&tv, &target).await {
                    Ok(info) if info.url.is_some() => {
                        lock_platforms.insert(target.to_key(), info);
                    }
                    _ => {} // Skip platforms without lock info
                }
            }
        }

        // Read existing stub and update TOML
        let content = file::read_to_string(&self.output)?;
        let toml_content = extract_toml_from_stub(&content);
        let mut doc = toml_content.parse::<DocumentMut>()?;

        // Pin exact version
        doc["version"] = toml_edit::value(&tv.version);

        // Write [lock.platforms.*] sections
        doc.remove("lock");
        let mut lock_table = toml_edit::Table::new();
        let mut platforms_table = toml_edit::Table::new();
        for (platform_key, info) in &lock_platforms {
            let mut pt = toml_edit::Table::new();
            if let Some(url) = &info.url {
                pt["url"] = toml_edit::value(url);
            }
            if let Some(checksum) = &info.checksum {
                pt["checksum"] = toml_edit::value(checksum);
            }
            platforms_table[platform_key] = toml_edit::Item::Table(pt);
        }
        lock_table["platforms"] = toml_edit::Item::Table(platforms_table);
        doc["lock"] = toml_edit::Item::Table(lock_table);

        // Reconstruct with shebang
        let toml_content = doc.to_string();
        Ok(format!(
            "#!/usr/bin/env -S mise tool-stub\n\n{toml_content}"
        ))
    }

    async fn fetch_checksums(&self) -> Result<String> {
        // Read the existing stub file
        if !self.output.exists() {
            bail!(
                "Tool stub file does not exist: {}",
                display_path(&self.output)
            );
        }

        let content = file::read_to_string(&self.output)?;
        let toml_content = extract_toml_from_stub(&content);
        let mut doc = toml_content.parse::<DocumentMut>()?;
        let mpr = MultiProgressReport::get();

        // Process top-level URL if present
        if let Some(url) = doc.get("url").and_then(|v| v.as_str()) {
            // Only fetch if checksum is missing
            if doc.get("checksum").is_none() {
                let (checksum, size, _) = self.analyze_url(url, &mpr).await?;
                doc["checksum"] = toml_edit::value(&checksum);

                // Create size entry with human-readable comment
                let mut size_item = toml_edit::value(size as i64);
                if let Some(value) = size_item.as_value_mut() {
                    let formatted_comment = format_size_comment(size);
                    value.decor_mut().set_suffix(formatted_comment);
                }
                doc["size"] = size_item;
            }
        }

        // Process platform-specific URLs
        if let Some(platforms) = doc.get_mut("platforms").and_then(|p| p.as_table_mut()) {
            for (platform_name, platform_value) in platforms.iter_mut() {
                if let Some(platform_table) = platform_value.as_table_mut()
                    && let Some(url) = platform_table.get("url").and_then(|v| v.as_str())
                {
                    // Only fetch if checksum is missing for this platform
                    if platform_table.get("checksum").is_none() {
                        match self.analyze_url(url, &mpr).await {
                            Ok((checksum, size, _)) => {
                                platform_table["checksum"] = toml_edit::value(&checksum);

                                // Create size entry with human-readable comment
                                let mut size_item = toml_edit::value(size as i64);
                                if let Some(value) = size_item.as_value_mut() {
                                    let formatted_comment = format_size_comment(size);
                                    value.decor_mut().set_suffix(formatted_comment);
                                }
                                platform_table["size"] = size_item;
                            }
                            Err(e) => {
                                // Log error but continue with other platforms
                                eprintln!(
                                    "Warning: Failed to fetch checksum for platform '{platform_name}': {e}"
                                );
                            }
                        }
                    }
                }
            }
        }

        let toml_content = doc.to_string();

        // Check if original was a bootstrap stub and preserve that format
        if is_bootstrap_stub(&content) {
            self.wrap_with_bootstrap(&toml_content).await
        } else {
            let mut output = vec![
                "#!/usr/bin/env -S mise tool-stub".to_string(),
                "".to_string(),
            ];

            output.push(toml_content);

            Ok(output.join("\n"))
        }
    }
}

/// Check if content is a bootstrap stub
fn is_bootstrap_stub(content: &str) -> bool {
    content.contains("# MISE_TOOL_STUB:") && content.contains("# :MISE_TOOL_STUB")
}

fn format_size_comment(bytes: u64) -> String {
    format!(" # {}", format_size(bytes, BINARY))
}

/// Extract TOML content from a stub file (handles both regular and bootstrap stubs)
fn extract_toml_from_stub(content: &str) -> String {
    // Check if this is a bootstrap stub by looking for comment markers
    if is_bootstrap_stub(content) {
        // Bootstrap stub: extract TOML between comment markers
        let start_marker = "# MISE_TOOL_STUB:";
        let end_marker = "# :MISE_TOOL_STUB";

        if let (Some(start_pos), Some(end_pos)) =
            (content.find(start_marker), content.find(end_marker))
            && start_pos < end_pos
        {
            let between = &content[start_pos + start_marker.len()..end_pos];
            return between
                .lines()
                .map(|line| line.strip_prefix("# ").unwrap_or(line))
                .collect::<Vec<_>>()
                .join("\n")
                .trim()
                .to_string();
        }
        String::new()
    } else {
        // Regular stub: skip shebang and comments at the start
        content
            .lines()
            .skip_while(|line| line.starts_with('#') || line.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    Generate a tool stub for a single URL:
    $ <bold>mise generate tool-stub ./bin/gh --url "https://github.com/cli/cli/releases/download/v2.336.0/gh_2.336.0_linux_amd64.tar.gz"</bold>

    Generate a tool stub with platform-specific URLs:
    $ <bold>mise generate tool-stub ./bin/rg \
        --platform-url linux-x64:https://github.com/BurntSushi/ripgrep/releases/download/14.0.3/ripgrep-14.0.3-x86_64-unknown-linux-musl.tar.gz \
        --platform-url darwin-arm64:https://github.com/BurntSushi/ripgrep/releases/download/14.0.3/ripgrep-14.0.3-aarch64-apple-darwin.tar.gz</bold>

    Append additional platforms to an existing stub:
    $ <bold>mise generate tool-stub ./bin/rg \
        --platform-url linux-x64:https://example.com/rg-linux.tar.gz</bold>
    $ <bold>mise generate tool-stub ./bin/rg \
        --platform-url darwin-arm64:https://example.com/rg-darwin.tar.gz</bold>
    # The stub now contains both platforms

    Use auto-detection for platform from URL:
    $ <bold>mise generate tool-stub ./bin/node \
        --platform-url https://nodejs.org/dist/v22.17.1/node-v22.17.1-darwin-arm64.tar.gz</bold>
    # Platform 'macos-arm64' will be auto-detected from the URL

    Generate with platform-specific binary paths:
    $ <bold>mise generate tool-stub ./bin/tool \
        --platform-url linux-x64:https://example.com/tool-linux.tar.gz \
        --platform-url windows-x64:https://example.com/tool-windows.zip \
        --platform-bin windows-x64:tool.exe</bold>

    Generate without downloading (faster):
    $ <bold>mise generate tool-stub ./bin/tool --url "https://example.com/tool.tar.gz" --skip-download</bold>

    Fetch checksums for an existing stub:
    $ <bold>mise generate tool-stub ./bin/jq --fetch</bold>
    # This will read the existing stub and download files to fill in any missing checksums/sizes

    Generate a bootstrap stub that installs mise if needed:
    $ <bold>mise generate tool-stub ./bin/tool --url "https://example.com/tool.tar.gz" --bootstrap</bold>
    # The stub will check for mise and install it automatically before running the tool

    Generate a bootstrap stub with a pinned mise version:
    $ <bold>mise generate tool-stub ./bin/tool --url "https://example.com/tool.tar.gz" --bootstrap --bootstrap-version 2025.1.0</bold>

    Lock an existing tool stub with pinned version and platform URLs/checksums:
    $ <bold>mise generate tool-stub ./bin/node --lock</bold>

    Bump the version in a locked stub:
    $ <bold>mise generate tool-stub ./bin/node --lock --version 22</bold>
    # Resolves the latest node 22.x, pins it, and updates platform URLs/checksums
"#
);
