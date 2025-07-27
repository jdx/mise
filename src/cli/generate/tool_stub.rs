use crate::Result;
use crate::config::Settings;
use crate::file::{self, TarOptions, TarFormat};
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

            // Auto-detect checksum and size if not skipped
            if !self.skip_download {
                if let Ok((checksum, size)) = self.detect_checksum_and_size(url).await {
                    stub.blake3 = Some(checksum);
                    stub.size = Some(size);
                }
            }

            // Auto-detect binary path if not specified
            if self.bin.is_none() && !self.skip_download {
                if let Ok(bin_path) = self.detect_binary_path(url).await {
                    stub.bin = Some(bin_path);
                }
            }
        } else if !self.platform.is_empty() {
            for platform_spec in &self.platform {
                let (platform, url) = self.parse_platform_spec(platform_spec)?;
                let mut platform_config = PlatformConfig {
                    url: url.clone(),
                    blake3: None,
                    size: None,
                };

                // Auto-detect checksum and size if not skipped
                if !self.skip_download {
                    if let Ok((checksum, size)) = self.detect_checksum_and_size(&url).await {
                        platform_config.blake3 = Some(checksum);
                        platform_config.size = Some(size);
                    }
                }

                stub.platforms.insert(platform, platform_config);
            }

            // Auto-detect binary path from first platform if not specified
            if self.bin.is_none() && !self.skip_download {
                if let Some((_, platform_config)) = stub.platforms.iter().next() {
                    if let Ok(bin_path) = self.detect_binary_path(&platform_config.url).await {
                        stub.bin = Some(bin_path);
                    }
                }
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
            bail!("Platform spec must be in format 'platform:url', got: {}", spec);
        }

        let platform = parts[0].to_string();
        let url = parts[1].to_string();

        Ok((platform, url))
    }

    async fn detect_checksum_and_size(&self, url: &str) -> Result<(String, u64)> {
        miseprintln!("Downloading {} to detect checksum and size...", url);

        let response = reqwest::get(url).await?;
        if !response.status().is_success() {
            bail!("Failed to download {}: {}", url, response.status());
        }

        let bytes = response.bytes().await?;
        let size = bytes.len() as u64;

        // Calculate BLAKE3 checksum
        let checksum = blake3::hash(&bytes).to_hex().to_string();

        Ok((checksum, size))
    }

    async fn detect_binary_path(&self, url: &str) -> Result<String> {
        miseprintln!("Downloading {} to detect binary path...", url);

        let response = reqwest::get(url).await?;
        if !response.status().is_success() {
            bail!("Failed to download {}: {}", url, response.status());
        }

        let bytes = response.bytes().await?;

        // Create a temporary directory for extraction
        let temp_dir = tempfile::tempdir()?;
        let archive_path = temp_dir.path().join("archive");

        // Write the downloaded file
        std::fs::write(&archive_path, &bytes)?;

        // Try to extract and find executables
        let extracted_dir = temp_dir.path().join("extracted");
        std::fs::create_dir_all(&extracted_dir)?;

        // Try extraction using mise's built-in extraction logic
        if self.is_archive_format(url) {
            let tar_opts = TarOptions {
                format: TarFormat::Auto,
                strip_components: 0,
                pr: None,
            };
            file::untar(&archive_path, &extracted_dir, &tar_opts)?;
        } else {
            // Assume it's a single binary file
            return Ok(self.get_tool_name());
        }

        // Find executable files
        let executables = self.find_executables(&extracted_dir)?;
        if executables.is_empty() {
            bail!("No executable files found in archive");
        }

        // Prefer files with the tool name, otherwise take the first one
        let tool_name = self.get_tool_name();
        for exe in &executables {
            if exe.contains(&tool_name) {
                return Ok(exe.clone());
            }
        }

        Ok(executables[0].clone())
    }

    fn is_archive_format(&self, url: &str) -> bool {
        // Check if the URL appears to be an archive format that mise can extract
        url.ends_with(".tar.gz") || url.ends_with(".tgz") ||
        url.ends_with(".tar.xz") || url.ends_with(".txz") ||
        url.ends_with(".tar.bz2") || url.ends_with(".tbz2") ||
        url.ends_with(".tar.zst") || url.ends_with(".tzst") ||
        url.ends_with(".zip") ||
        url.ends_with(".7z")
    }

    fn find_executables(&self, dir: &std::path::Path) -> Result<Vec<String>> {
        let mut executables = Vec::new();

        for entry in walkdir::WalkDir::new(dir) {
            let entry = entry?;
            if entry.file_type().is_file() {
                let path = entry.path();
                if file::is_executable(path) {
                    if let Some(relative_path) = path.strip_prefix(dir).ok() {
                        executables.push(relative_path.to_string_lossy().to_string());
                    }
                }
            }
        }

        Ok(executables)
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