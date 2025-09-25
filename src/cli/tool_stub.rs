use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::backend::static_helpers::lookup_platform_key;
use crate::config::Config;
use crate::dirs;
use crate::file;
use crate::hash;
use crate::toolset::{InstallOptions, ToolRequest, ToolSource, ToolVersionOptions};
use clap::Parser;
use color_eyre::eyre::{Result, bail, eyre};
use eyre::ensure;
use serde::{Deserialize, Deserializer};
use toml::Value;

#[derive(Debug, Deserialize)]
pub struct ToolStubFile {
    #[serde(default = "default_version")]
    pub version: String,
    pub bin: Option<String>,  // defaults to filename if not specified
    pub tool: Option<String>, // explicit tool name override
    #[serde(default)]
    pub install_env: indexmap::IndexMap<String, String>,
    #[serde(default)]
    pub os: Option<Vec<String>>,
    #[serde(flatten, deserialize_with = "deserialize_tool_stub_options")]
    pub opts: indexmap::IndexMap<String, String>,
    #[serde(skip)]
    pub tool_name: String,
}

// Custom deserializer that converts TOML values to strings for storage in opts
fn deserialize_tool_stub_options<'de, D>(
    deserializer: D,
) -> Result<indexmap::IndexMap<String, String>, D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de::Error;

    let value = Value::deserialize(deserializer)?;
    let mut opts = indexmap::IndexMap::new();

    if let Value::Table(table) = value {
        for (key, val) in table {
            // Skip known special fields that are handled separately
            if matches!(
                key.as_str(),
                "version" | "bin" | "tool" | "install_env" | "os"
            ) {
                continue;
            }

            // Convert TOML values to strings for storage
            let string_value = match val {
                Value::String(s) => s,
                Value::Table(_) | Value::Array(_) => {
                    // For complex values (tables, arrays), serialize them as TOML strings
                    toml::to_string(&val).map_err(D::Error::custom)?
                }
                Value::Integer(i) => i.to_string(),
                Value::Float(f) => f.to_string(),
                Value::Boolean(b) => b.to_string(),
                Value::Datetime(dt) => dt.to_string(),
            };

            opts.insert(key, string_value);
        }
    }

    Ok(opts)
}

fn default_version() -> String {
    "latest".to_string()
}

fn has_http_backend_config(opts: &indexmap::IndexMap<String, String>) -> bool {
    // Check for top-level url
    if opts.contains_key("url") {
        return true;
    }

    // Check for platform-specific configs with urls
    for (key, value) in opts {
        if key.starts_with("platforms") && value.contains("url") {
            return true;
        }
    }

    false
}

impl ToolStubFile {
    pub fn from_file(path: &Path) -> Result<Self> {
        let content = file::read_to_string(path)?;
        let mut stub: ToolStubFile = toml::from_str(&content)?;

        // Extract stub name from file name
        let stub_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| eyre!("Invalid stub file name"))?
            .to_string();

        // Determine tool name from tool field or derive from stub name
        // If no tool is specified, default to HTTP backend if HTTP config is present
        let tool_name = stub
            .tool
            .clone()
            .or_else(|| stub.opts.get("tool").map(|s| s.to_string()))
            .unwrap_or_else(|| {
                if has_http_backend_config(&stub.opts) {
                    format!("http:{stub_name}")
                } else {
                    stub_name.clone()
                }
            });

        // Set bin to filename if not specified
        if stub.bin.is_none() {
            stub.bin = Some(stub_name.clone());
        }

        stub.tool_name = tool_name;

        Ok(stub)
    }

    // Create a ToolRequest directly using ToolVersionOptions
    pub fn to_tool_request(&self, stub_path: &Path) -> Result<ToolRequest> {
        use crate::cli::args::BackendArg;

        let backend_arg = BackendArg::from(&self.tool_name);
        let source = ToolSource::ToolStub(stub_path.to_path_buf());

        // Create ToolVersionOptions from our fields
        let mut opts = self.opts.clone();
        opts.shift_remove("tool"); // Remove tool field since it's handled separately

        // Add bin field if present
        if let Some(bin) = &self.bin {
            opts.insert("bin".to_string(), bin.clone());
        }

        let options = ToolVersionOptions {
            os: self.os.clone(),
            install_env: self.install_env.clone(),
            opts: opts.clone(),
        };

        // For HTTP backend with "latest" version, use URL+checksum hash as version for stability
        let version = if self.tool_name.starts_with("http:") && self.version == "latest" {
            if let Some(url) =
                lookup_platform_key(&options, "url").or_else(|| opts.get("url").cloned())
            {
                // Include checksum in hash calculation for better version stability
                let checksum = lookup_platform_key(&options, "checksum")
                    .or_else(|| opts.get("checksum").cloned())
                    .unwrap_or_default();
                let hash_input = format!("{url}:{checksum}");
                // Use first 8 chars of URL+checksum hash as version
                format!("url-{}", &hash::hash_to_str(&hash_input)[..8])
            } else {
                self.version.clone()
            }
        } else {
            self.version.clone()
        };

        ToolRequest::new_opts(backend_arg.into(), &version, options, source)
    }
}

// Cache just stores the binary path as a raw string
// The mtime is already encoded in the cache key, so no need to store it

struct BinPathCache;

impl BinPathCache {
    fn cache_key(stub_path: &Path) -> Result<String> {
        let path_str = stub_path.to_string_lossy();
        let mtime = stub_path.metadata()?.modified()?;
        let mtime_str = format!(
            "{:?}",
            mtime
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
        );
        Ok(hash::hash_to_str(&format!("{path_str}:{mtime_str}")))
    }

    fn cache_file_path(cache_key: &str) -> PathBuf {
        dirs::CACHE.join("tool-stubs").join(cache_key)
    }

    fn load(cache_key: &str) -> Option<PathBuf> {
        let cache_path = Self::cache_file_path(cache_key);
        if !cache_path.exists() {
            return None;
        }

        match file::read_to_string(&cache_path) {
            Ok(content) => {
                let bin_path = PathBuf::from(content.trim());
                // Verify the cached binary still exists
                if bin_path.exists() {
                    Some(bin_path)
                } else {
                    // Clean up stale cache (missing binary)
                    let _ = file::remove_file(&cache_path);
                    None
                }
            }
            Err(_) => None,
        }
    }

    fn save(bin_path: &Path, cache_key: &str) -> Result<()> {
        let cache_path = Self::cache_file_path(cache_key);

        if let Some(parent) = cache_path.parent() {
            file::create_dir_all(parent)?;
        }

        file::write(&cache_path, bin_path.to_string_lossy().as_bytes())?;
        Ok(())
    }
}

fn find_tool_version(
    toolset: &crate::toolset::Toolset,
    config: &std::sync::Arc<Config>,
    tool_name: &str,
) -> Option<crate::toolset::ToolVersion> {
    for (_backend, tv) in toolset.list_current_installed_versions(config) {
        if tv.ba().full() == tool_name {
            return Some(tv);
        }
    }
    None
}

fn find_single_subdirectory(install_path: &Path) -> Option<PathBuf> {
    let Ok(entries) = std::fs::read_dir(install_path) else {
        return None;
    };

    let dirs: Vec<_> = entries
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false))
        .collect();

    if dirs.len() == 1 {
        Some(dirs[0].path())
    } else {
        None
    }
}

fn try_find_bin_in_path(base_path: &Path, bin: &str) -> Option<PathBuf> {
    let bin_path = base_path.join(bin);
    if bin_path.exists() && crate::file::is_executable(&bin_path) {
        Some(bin_path)
    } else {
        None
    }
}

fn list_executable_files(dir_path: &Path) -> Vec<String> {
    list_executable_files_recursive(dir_path, dir_path)
}

fn list_executable_files_recursive(base_path: &Path, current_path: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(current_path) else {
        return Vec::new();
    };

    let mut result = Vec::new();

    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        let filename = entry.file_name();
        let filename_str = filename.to_string_lossy();

        // Skip hidden files (starting with .)
        if filename_str.starts_with('.') {
            continue;
        }

        if path.is_dir() {
            // Recursively search subdirectories
            let subdir_files = list_executable_files_recursive(base_path, &path);
            result.extend(subdir_files);
        } else if path.is_file() && crate::file::is_executable(&path) {
            // Get the relative path from the base directory
            if let Ok(relative_path) = path.strip_prefix(base_path) {
                result.push(relative_path.to_string_lossy().to_string());
            }
        }
    }

    result
}

fn resolve_bin_with_path(
    toolset: &crate::toolset::Toolset,
    config: &std::sync::Arc<Config>,
    bin: &str,
    tool_name: &str,
) -> Option<PathBuf> {
    let tv = find_tool_version(toolset, config, tool_name)?;
    let install_path = tv.install_path();

    // Try direct path first
    if let Some(bin_path) = try_find_bin_in_path(&install_path, bin) {
        return Some(bin_path);
    }

    // If direct path doesn't work, try skipping a single top-level directory
    // This handles cases where the tarball has a single top-level directory
    let subdir_path = find_single_subdirectory(&install_path)?;
    try_find_bin_in_path(&subdir_path, bin)
}

async fn resolve_bin_simple(
    toolset: &crate::toolset::Toolset,
    config: &std::sync::Arc<Config>,
    bin: &str,
) -> Result<Option<PathBuf>> {
    if let Some((backend, tv)) = toolset.which(config, bin).await {
        backend.which(config, &tv, bin).await
    } else {
        Ok(None)
    }
}

fn is_bin_path(bin: &str) -> bool {
    bin.contains('/') || bin.contains('\\')
}

#[derive(Debug)]
enum BinPathError {
    ToolNotFound(String),
    BinNotFound {
        tool_name: String,
        bin: String,
        available_bins: Vec<String>,
    },
}

fn resolve_platform_specific_bin(stub: &ToolStubFile, stub_path: &Path) -> String {
    // Try to find platform-specific bin field first
    let platform_key = get_current_platform_key();

    // Check for platform-specific bin field: platforms.{platform}.bin
    let platform_bin_key = format!("platforms.{platform_key}.bin");
    if let Some(platform_bin) = stub.opts.get(&platform_bin_key) {
        return platform_bin.to_string();
    }

    // Fall back to global bin field
    if let Some(bin) = &stub.bin {
        return bin.to_string();
    }

    // Finally, fall back to stub filename (without extension)
    stub_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(&stub.tool_name)
        .to_string()
}

fn get_current_platform_key() -> String {
    use crate::config::Settings;
    let settings = Settings::get();
    format!("{}-{}", settings.os(), settings.arch())
}

async fn find_cached_or_resolve_bin_path(
    toolset: &crate::toolset::Toolset,
    config: &std::sync::Arc<Config>,
    stub: &ToolStubFile,
    stub_path: &Path,
) -> Result<Result<PathBuf, BinPathError>> {
    // Generate cache key from file path and mtime
    let cache_key = BinPathCache::cache_key(stub_path)?;

    // Try to load from cache first
    if let Some(bin_path) = BinPathCache::load(&cache_key) {
        return Ok(Ok(bin_path));
    }

    // Cache miss - resolve the binary path
    let bin = resolve_platform_specific_bin(stub, stub_path);
    let bin_path = if is_bin_path(&bin) {
        resolve_bin_with_path(toolset, config, &bin, &stub.tool_name)
    } else {
        resolve_bin_simple(toolset, config, &bin).await?
    };

    if let Some(bin_path) = bin_path {
        // Cache the result
        if let Err(e) = BinPathCache::save(&bin_path, &cache_key) {
            // Don't fail if caching fails, just log it
            crate::warn!("Failed to cache binary path: {e}");
        }

        return Ok(Ok(bin_path));
    }

    // Determine the specific error
    if is_bin_path(&bin) {
        // For path-based bins, check if the tool exists first
        if let Some(tv) = find_tool_version(toolset, config, &stub.tool_name) {
            let install_path = tv.install_path();
            // List all executable files recursively from the install path
            let available_bins = list_executable_files(&install_path);

            Ok(Err(BinPathError::BinNotFound {
                tool_name: stub.tool_name.clone(),
                bin: bin.to_string(),
                available_bins,
            }))
        } else {
            Ok(Err(BinPathError::ToolNotFound(stub.tool_name.clone())))
        }
    } else {
        // For simple bin names, first check if the tool itself exists
        if let Some(tv) = find_tool_version(toolset, config, &stub.tool_name) {
            // Tool exists, list its available executables
            let available_bins = list_executable_files(&tv.install_path());
            Ok(Err(BinPathError::BinNotFound {
                tool_name: stub.tool_name.clone(),
                bin: bin.to_string(),
                available_bins,
            }))
        } else {
            // Tool doesn't exist
            Ok(Err(BinPathError::ToolNotFound(stub.tool_name.clone())))
        }
    }
}

async fn execute_with_tool_request(
    stub: &ToolStubFile,
    config: &mut std::sync::Arc<Config>,
    args: Vec<String>,
    stub_path: &Path,
) -> Result<()> {
    // Use direct ToolRequest creation with ToolVersionOptions
    let tool_request = stub.to_tool_request(stub_path)?;

    // Create a toolset directly and add the tool request with its options
    let source = ToolSource::ToolStub(stub_path.to_path_buf());
    let mut toolset = crate::toolset::Toolset::new(source);
    toolset.add_version(tool_request);

    // Resolve the toolset to populate current versions
    toolset.resolve(config).await?;

    // Ensure we have current versions after resolving
    ensure!(
        !toolset.list_current_versions().is_empty(),
        "No current versions found after resolving toolset"
    );

    // Install the tool if it's missing
    let install_opts = InstallOptions {
        force: false,
        jobs: None,
        raw: false,
        missing_args_only: false,
        resolve_options: Default::default(),
        ..Default::default()
    };

    toolset
        .install_missing_versions(config, &install_opts)
        .await?;
    toolset.notify_if_versions_missing(config).await;

    // Find the binary path using cache
    match find_cached_or_resolve_bin_path(&toolset, &*config, stub, stub_path).await? {
        Ok(bin_path) => {
            // Get the environment with proper PATH from toolset
            let env = toolset.env_with_path(config).await?;

            crate::cli::exec::exec_program(bin_path, args, env)
        }
        Err(e) => match e {
            BinPathError::ToolNotFound(tool_name) => {
                bail!("Tool '{}' not found", tool_name);
            }
            BinPathError::BinNotFound {
                tool_name,
                bin,
                available_bins,
            } => {
                if available_bins.is_empty() {
                    bail!(
                        "Tool '{}' does not have an executable named '{}'",
                        tool_name,
                        bin
                    );
                } else {
                    bail!(
                        "Tool '{}' does not have an executable named '{}'. Available executables: {}",
                        tool_name,
                        bin,
                        available_bins.join(", ")
                    );
                }
            }
        },
    }
}

/// Execute a tool stub
///
/// Tool stubs are executable files containing TOML configuration that specify
/// which tool to run and how to run it. They provide a convenient way to create
/// portable, self-contained executables that automatically manage tool installation
/// and execution.
///
/// A tool stub consists of:
/// - A shebang line: #!/usr/bin/env -S mise tool-stub
/// - TOML configuration specifying the tool, version, and options
/// - Optional comments describing the tool's purpose
///
/// Example stub file:
///   #!/usr/bin/env -S mise tool-stub
///   # Node.js v20 development environment
///   
///   tool = "node"
///   version = "20.0.0"
///   bin = "node"
///
/// The stub will automatically install the specified tool version if missing
/// and execute it with any arguments passed to the stub.
///
/// For more information, see: https://mise.jdx.dev/dev-tools/tool-stubs.html
#[derive(Debug, Parser)]
#[clap(disable_help_flag = true, disable_version_flag = true)]
pub struct ToolStub {
    /// Path to the TOML tool stub file to execute
    ///
    /// The stub file must contain TOML configuration specifying the tool
    /// and version to run. At minimum, it should specify a 'version' field.
    /// Other common fields include 'tool', 'bin', and backend-specific options.
    #[clap(value_name = "FILE")]
    pub file: PathBuf,

    /// Arguments to pass to the tool
    ///
    /// All arguments after the stub file path will be forwarded to the
    /// underlying tool. Use '--' to separate mise arguments from tool arguments
    /// if needed.
    #[clap(trailing_var_arg = true, allow_hyphen_values = true)]
    pub args: Vec<String>,
}

impl ToolStub {
    pub async fn run(self) -> Result<()> {
        // Ignore clap parsing and use raw args from env::ARGS to avoid version flag interception
        let file_str = self.file.to_string_lossy();

        // Find our file in the global args and take everything after it
        let args = {
            let global_args = crate::env::ARGS.read().unwrap();
            let file_str_ref: &str = file_str.as_ref();
            if let Some(file_pos) = global_args.iter().position(|arg| arg == file_str_ref) {
                global_args.get(file_pos + 1..).unwrap_or(&[]).to_vec()
            } else {
                vec![]
            }
        }; // Drop the lock before await

        let stub = ToolStubFile::from_file(&self.file)?;
        let mut config = Config::get().await?;
        return execute_with_tool_request(&stub, &mut config, args, &self.file).await;
    }
}

pub(crate) async fn short_circuit_stub(args: &[String]) -> Result<()> {
    // Early return if no args or not enough args for a stub
    if args.is_empty() {
        return Ok(());
    }

    // Check if the first argument looks like a tool stub file path
    let potential_stub_path = std::path::Path::new(&args[0]);

    // Only proceed if it's an existing file with a reasonable extension
    if !potential_stub_path.exists() {
        return Ok(());
    }

    // Generate cache key from file path and mtime
    let cache_key = BinPathCache::cache_key(potential_stub_path)?;

    // Check if we have a cached binary path
    if let Some(bin_path) = BinPathCache::load(&cache_key) {
        let args = args[1..].to_vec();
        return crate::cli::exec::exec_program(bin_path, args, BTreeMap::new());
    }

    // No cache hit, return Ok(()) to continue with normal processing
    Ok(())
}
