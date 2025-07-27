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
    /// Examples: --platform linux-x64:https://... --platform darwin-arm64:https://...
    #[clap(long, short)]
    pub platform: Vec<String>,

    /// Binary path within the extracted archive
    ///
    /// If not specified, will attempt to auto-detect the binary
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
        let mut stub = ToolStubConfig {
            version: self.version.clone(),
            bin: self.bin.clone(),
            url: None,
            blake3: None,
            size: None,
            platforms: indexmap::IndexMap::new(),
        };

        // Handle URL or platform-specific URLs
        if let Some(url) = &self.url {
            stub.url = Some(url.clone());

            // Auto-detect checksum, size, and binary path if not skipped
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
        } else if !self.platform.is_empty() {
            let mpr = MultiProgressReport::get();
            let mut platform_bin_paths: Vec<Option<String>> = Vec::new();

            for platform_spec in &self.platform {
                let (platform, url) = self.parse_platform_spec(platform_spec)?;
                let mut platform_config = PlatformConfig {
                    url: url.clone(),
                    blake3: None,
                    size: None,
                    bin: None,
                };

                // Auto-detect checksum, size, and binary path if not skipped
                if !self.skip_download {
                    if let Ok((checksum, size, bin_path)) = self.analyze_url(&url, &mpr).await {
                        platform_config.blake3 = Some(checksum);
                        platform_config.size = Some(size);
                        platform_config.bin = bin_path.clone();
                        platform_bin_paths.push(bin_path);
                    } else {
                        platform_bin_paths.push(None);
                    }
                } else {
                    platform_bin_paths.push(None);
                }

                stub.platforms.insert(platform, platform_config);
            }

            // Determine if we should use global bin or platform-specific bins
            if self.bin.is_none() && !platform_bin_paths.is_empty() {
                // Check if all detected bin paths are the same
                let unique_bins: std::collections::HashSet<_> =
                    platform_bin_paths.iter().flatten().collect();

                if unique_bins.len() <= 1 {
                    // All platforms have the same bin path (or no bin paths detected)
                    // Use global bin field and remove from platform configs
                    if let Some(common_bin) = unique_bins.into_iter().next() {
                        stub.bin = Some(common_bin.clone());
                        // Remove bin from all platform configs since it's now global
                        for (_, platform_config) in stub.platforms.iter_mut() {
                            platform_config.bin = None;
                        }
                    }
                }
                // If unique_bins.len() > 1, keep platform-specific bin fields as they differ
            }
        } else {
            bail!("Either --url or --platform must be specified");
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
        let pr = mpr.add(&format!("download {}", filename));

        // Download using mise's HTTP client
        HTTP.download_file(url, &archive_path, Some(&pr)).await?;

        // Read the file to calculate checksum and size
        let bytes = file::read(&archive_path)?;
        let size = bytes.len() as u64;
        let checksum = blake3::hash(&bytes).to_hex().to_string();

        // Detect binary path if this is an archive
        let bin_path = if self.is_archive_format(url) {
            // Update progress message for extraction and reuse the same progress reporter
            pr.set_message(format!("extract {}", filename));
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
        file::untar(&archive_path, &extracted_dir, &tar_opts)?;

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

        // Smart binary selection with prioritization
        let tool_name = self.get_tool_name();
        let selected_exe = self.select_best_binary(&executables, &tool_name)?;

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

    fn select_best_binary(&self, executables: &[String], tool_name: &str) -> Result<String> {
        if executables.is_empty() {
            bail!("No executable files found in archive");
        }

        // Extract just the base tool name (remove any suffixes like -test)
        let base_tool_name = tool_name
            .split('-')
            .next()
            .unwrap_or(tool_name)
            .split('_')
            .next()
            .unwrap_or(tool_name);

        // Score each executable based on priority criteria
        let mut scored_executables: Vec<(String, i32)> = executables
            .iter()
            .map(|exe| {
                let path = std::path::Path::new(exe);
                let filename = path.file_name().and_then(|f| f.to_str()).unwrap_or(exe);
                let stem = path
                    .file_stem()
                    .and_then(|f| f.to_str())
                    .unwrap_or(filename);

                let mut score = 0;

                // Priority 1: Exact filename match (highest priority)
                if stem == base_tool_name || filename == base_tool_name {
                    score += 1000;
                } else if stem == tool_name || filename == tool_name {
                    score += 900;
                }

                // Priority 2: Filename starts with tool name
                if stem.starts_with(base_tool_name) {
                    score += 500;
                } else if stem.starts_with(tool_name) {
                    score += 400;
                }

                // Priority 3: Prefer shorter paths (bin/tool vs lib/node_modules/tool/bin/tool)
                let path_depth = exe.matches('/').count();
                score += 100 - (path_depth as i32 * 10);

                // Priority 4: Prefer bin/ directory
                if exe.starts_with("bin/") {
                    score += 200;
                }

                // Priority 5: Avoid obvious non-main binaries
                let lower_filename = filename.to_lowercase();
                if lower_filename.contains("test")
                    || lower_filename.contains("spec")
                    || lower_filename.contains("example")
                {
                    score -= 100;
                }

                // Priority 6: Prefer executables without extensions or with common executable extensions
                if !filename.contains('.')
                    || filename.ends_with(".exe")
                    || filename.ends_with(".sh")
                {
                    score += 50;
                } else if filename.ends_with(".js")
                    || filename.ends_with(".py")
                    || filename.ends_with(".rb")
                {
                    score += 25;
                }

                (exe.clone(), score)
            })
            .collect();

        // Sort by score (highest first)
        scored_executables.sort_by(|a, b| b.1.cmp(&a.1));

        // Return the highest-scored executable
        Ok(scored_executables[0].0.clone())
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    Generate a tool stub for a single URL:
    $ <bold>mise generate tool-stub ./bin/gh --url "https://github.com/cli/cli/releases/download/v2.336.0/gh_2.336.0_linux_amd64.tar.gz"</bold>

    Generate a tool stub with platform-specific URLs:
    $ <bold>mise generate tool-stub ./bin/rg \
        --platform linux-x64:https://github.com/BurntSushi/ripgrep/releases/download/14.0.3/ripgrep-14.0.3-x86_64-unknown-linux-musl.tar.gz \
        --platform darwin-arm64:https://github.com/BurntSushi/ripgrep/releases/download/14.0.3/ripgrep-14.0.3-aarch64-apple-darwin.tar.gz</bold>

    Generate without downloading (faster):
    $ <bold>mise generate tool-stub ./bin/tool --url "https://example.com/tool.tar.gz" --skip-download</bold>
"#
);
