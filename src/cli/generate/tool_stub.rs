use crate::Result;
use crate::backend::static_helpers::get_filename_from_url;
use crate::config::Settings;
use crate::file::{self, TarFormat, TarOptions};
use crate::http::HTTP;
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::ui::progress_report::SingleReport;
use clap::ValueHint;
use color_eyre::eyre::bail;
use serde::Serialize;
use std::path::PathBuf;
use xx::file::display_path;

#[derive(Debug, Serialize)]
struct ToolStubConfig {
    version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    bin: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    blake3: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    size: Option<u64>,
    #[serde(skip_serializing_if = "indexmap::IndexMap::is_empty")]
    platforms: indexmap::IndexMap<String, PlatformConfig>,
}

#[derive(Debug, Serialize)]
struct PlatformConfig {
    url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    blake3: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    bin: Option<String>,
}

/// [experimental] Generate a tool stub for HTTP-based tools
///
/// This command generates tool stubs that can automatically download and execute
/// tools from HTTP URLs. It can detect checksums, file sizes, and binary paths
/// automatically by downloading and analyzing the tool.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct ToolStub {
    /// Output file path for the tool stub
    #[clap(value_hint = ValueHint::FilePath)]
    pub output: PathBuf,

    /// Version of the tool
    #[clap(long, default_value = "latest")]
    pub version: String,

    /// URL for downloading the tool
    ///
    /// Example: https://github.com/owner/repo/releases/download/v2.0.0/tool-linux-x64.tar.gz
    #[clap(long, short)]
    pub url: Option<String>,

    /// Platform-specific URLs in the format platform:url
    ///
    /// Examples: --platform-url linux-x64:https://... --platform-url darwin-arm64:https://...
    #[clap(long)]
    pub platform_url: Vec<String>,

    /// Platform-specific binary paths in the format platform:path
    ///
    /// Examples: --platform-bin windows-x64:tool.exe --platform-bin linux-x64:bin/tool
    #[clap(long)]
    pub platform_bin: Vec<String>,

    /// Binary path within the extracted archive
    ///
    /// If not specified and the archive is downloaded, will only auto-detect if an exact filename match is found
    #[clap(long, short)]
    pub bin: Option<String>,

    /// Skip downloading for checksum and binary path detection (faster but less informative)
    #[clap(long)]
    pub skip_download: bool,

    /// HTTP backend type to use
    #[clap(long, default_value = "http")]
    pub http: String,
}

impl ToolStub {
    pub async fn run(self) -> Result<()> {
        Settings::get().ensure_experimental("generate tool-stub")?;

        let stub_content = self.generate_stub().await?;

        if let Some(parent) = self.output.parent() {
            file::create_dir_all(parent)?;
        }

        file::write(&self.output, &stub_content)?;
        file::make_executable(&self.output)?;

        miseprintln!("Generated tool stub: {}", display_path(&self.output));
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

        let mut stub = ToolStubConfig {
            version: self.version.clone(),
            bin: self.bin.clone(),
            url: None,
            blake3: None,
            size: None,
            platforms: indexmap::IndexMap::new(),
        };

        // Handle URL and platform-specific URLs (both can be specified)
        if let Some(url) = &self.url {
            stub.url = Some(url.clone());

            // Auto-detect checksum and size if not skipped
            if !self.skip_download {
                let mpr = MultiProgressReport::get();
                if let Ok((checksum, size, bin_path)) = self.analyze_url(url, &mpr).await {
                    stub.blake3 = Some(checksum);
                    stub.size = Some(size);
                    if self.bin.is_none() {
                        stub.bin = bin_path;
                    }
                }
            }
        }

        if !self.platform_url.is_empty() {
            let mpr = MultiProgressReport::get();

            // Parse platform-specific bin paths
            let mut explicit_platform_bins = std::collections::HashMap::new();
            for platform_bin_spec in &self.platform_bin {
                let (platform, bin_path) = self.parse_platform_bin_spec(platform_bin_spec)?;
                explicit_platform_bins.insert(platform, bin_path);
            }

            for platform_spec in &self.platform_url {
                let (platform, url) = self.parse_platform_spec(platform_spec)?;
                let mut platform_config = PlatformConfig {
                    url: url.clone(),
                    blake3: None,
                    size: None,
                    bin: None,
                };

                // Set platform-specific bin path if explicitly provided
                if let Some(explicit_bin) = explicit_platform_bins.get(&platform) {
                    platform_config.bin = Some(explicit_bin.clone());
                }

                // Auto-detect checksum and size if not skipped
                if !self.skip_download {
                    if let Ok((checksum, size, _)) = self.analyze_url(&url, &mpr).await {
                        platform_config.blake3 = Some(checksum);
                        platform_config.size = Some(size);
                    }
                }

                stub.platforms.insert(platform, platform_config);
            }
        }

        // Validate that at least one URL type is provided
        if self.url.is_none() && self.platform_url.is_empty() {
            bail!("Either --url or --platform-url must be specified");
        }

        let toml_content = toml::to_string_pretty(&stub)?;

        let mut content = vec![
            "#!/usr/bin/env -S mise tool-stub".to_string(),
            format!("# {} tool stub", self.get_tool_name()),
            "".to_string(),
        ];

        content.push(toml_content);

        Ok(content.join("\n"))
    }

    fn parse_platform_spec(&self, spec: &str) -> Result<(String, String)> {
        let parts: Vec<&str> = spec.splitn(2, ':').collect();
        if parts.len() != 2 {
            bail!(
                "Platform spec must be in format 'platform:url', got: {}",
                spec
            );
        }

        let platform = parts[0].to_string();
        let url = parts[1].to_string();

        Ok((platform, url))
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
        miseprintln!("Downloading {} to analyze...", url);

        // Create a temporary directory for download and extraction
        let temp_dir = tempfile::tempdir()?;
        let filename = get_filename_from_url(url);
        let archive_path = temp_dir.path().join(&filename);

        // Create one progress reporter for the entire operation
        let pr = mpr.add(&format!("download {filename}"));

        // Download using mise's HTTP client
        HTTP.download_file(url, &archive_path, Some(&pr)).await?;

        // Read the file to calculate checksum and size
        let bytes = file::read(&archive_path)?;
        let size = bytes.len() as u64;
        let checksum = blake3::hash(&bytes).to_hex().to_string();

        // Detect binary path if this is an archive
        let bin_path = if self.is_archive_format(url) {
            // Update progress message for extraction and reuse the same progress reporter
            pr.set_message(format!("extract {filename}"));
            match self
                .extract_and_find_binary(&archive_path, &temp_dir, &filename, &pr)
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
        pr: &Box<dyn SingleReport>,
    ) -> Result<String> {
        // Try to extract and find executables
        let extracted_dir = temp_dir.path().join("extracted");
        std::fs::create_dir_all(&extracted_dir)?;

        // Try extraction using mise's built-in extraction logic (reuse the passed progress reporter)
        let tar_opts = TarOptions {
            format: TarFormat::Auto,
            strip_components: 0,
            pr: Some(pr),
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
                return Ok(stripped.to_string_lossy().to_string());
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
                if file::is_executable(path) {
                    if let Ok(relative_path) = path.strip_prefix(dir) {
                        executables.push(relative_path.to_string_lossy().to_string());
                    }
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
                if let Some(stem) = path.file_stem().and_then(|f| f.to_str()) {
                    if stem == tool_name {
                        return Ok(exe.clone());
                    }
                }
            }
        }

        // No exact match found, provide helpful error message
        let mut exe_list = executables.to_vec();
        exe_list.sort();

        bail!(
            "No executable found with exact filename '{}'. Available executables:\n  {}",
            tool_name,
            exe_list.join("\n  ")
        );
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

    Generate with platform-specific binary paths:
    $ <bold>mise generate tool-stub ./bin/tool \
        --platform-url linux-x64:https://example.com/tool-linux.tar.gz \
        --platform-url windows-x64:https://example.com/tool-windows.zip \
        --platform-bin windows-x64:tool.exe</bold>

    Generate without downloading (faster):
    $ <bold>mise generate tool-stub ./bin/tool --url "https://example.com/tool.tar.gz" --skip-download</bold>
"#
);
