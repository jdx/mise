use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

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
    #[serde(skip)]
    pub bin_name: String,
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
        let tool_name = stub
            .tool
            .clone()
            .or_else(|| stub.opts.get("tool").map(|s| s.to_string()))
            .unwrap_or_else(|| stub_name.clone());

        // Determine bin name (what executable to run) - defaults to filename
        let bin_name = stub.bin.clone().unwrap_or_else(|| stub_name.clone());

        stub.tool_name = tool_name;
        stub.bin_name = bin_name;

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

        let options = ToolVersionOptions {
            os: self.os.clone(),
            install_env: self.install_env.clone(),
            opts,
        };

        ToolRequest::new_opts(backend_arg.into(), &self.version, options, source)
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

async fn find_cached_or_resolve_bin_path(
    toolset: &crate::toolset::Toolset,
    config: &std::sync::Arc<Config>,
    stub: &ToolStubFile,
    stub_path: &Path,
) -> Result<Option<PathBuf>> {
    // Generate cache key from file path and mtime
    let cache_key = BinPathCache::cache_key(stub_path)?;

    // Try to load from cache first
    if let Some(bin_path) = BinPathCache::load(&cache_key) {
        return Ok(Some(bin_path));
    }

    // Cache miss - resolve the binary path
    if let Some((backend, tv)) = toolset.which(config, &stub.bin_name).await {
        if let Some(bin_path) = backend.which(config, &tv, &stub.bin_name).await? {
            // Cache the result
            if let Err(e) = BinPathCache::save(&bin_path, &cache_key) {
                // Don't fail if caching fails, just log it
                warn!("Failed to cache binary path: {e}");
            }

            return Ok(Some(bin_path));
        }
    }

    Ok(None)
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
    if let Some(bin_path) =
        find_cached_or_resolve_bin_path(&toolset, &*config, stub, stub_path).await?
    {
        // Get the environment with proper PATH from toolset
        let env = toolset.env_with_path(config).await?;

        return crate::cli::exec::exec_program(bin_path, args, env);
    }

    bail!(
        "Tool '{}' or bin '{}' not found",
        stub.tool_name,
        stub.bin_name
    );
}

// [experimental] Execute a custom tool stub.
#[derive(Debug, Parser)]
pub struct ToolStub {
    /// The tool stub file to execute
    #[clap(value_name = "FILE")]
    pub file: PathBuf,

    /// Arguments to pass to the tool
    #[clap(trailing_var_arg = true, allow_hyphen_values = true)]
    pub args: Vec<String>,
}

impl ToolStub {
    pub async fn run(self) -> Result<()> {
        let stub = ToolStubFile::from_file(&self.file)?;
        let mut config = Config::get().await?;
        return execute_with_tool_request(&stub, &mut config, self.args, &self.file).await;
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
