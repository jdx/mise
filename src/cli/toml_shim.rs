use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::cli::args::ToolArg;
#[cfg(any(test, windows))]
use crate::cmd;
use crate::config::Config;
use crate::dirs;
#[cfg(not(test))]
use crate::env;
#[cfg(all(windows, not(test)))]
use crate::env::PATH_KEY;
use crate::file;
use crate::hash;
use crate::toolset::{InstallOptions, ToolRequest, ToolSource, ToolVersionOptions, ToolsetBuilder};
use clap::Parser;
#[cfg(any(test, windows))]
use color_eyre::eyre::eyre;
use color_eyre::eyre::{Result, bail};
use duct::IntoExecutablePath;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct TomlShimFile {
    #[serde(default = "default_version")]
    pub version: String,
    pub bin: Option<String>,  // defaults to filename if not specified
    pub tool: Option<String>, // explicit tool name override
    #[serde(default)]
    pub install_env: indexmap::IndexMap<String, String>,
    #[serde(default)]
    pub os: Option<Vec<String>>,
    #[serde(flatten)]
    pub opts: indexmap::IndexMap<String, String>,
    #[serde(skip)]
    pub tool_name: String,
    #[serde(skip)]
    pub bin_name: String,
}

fn default_version() -> String {
    "latest".to_string()
}

impl TomlShimFile {
    pub fn from_file(path: &Path) -> Result<Self> {
        let content = file::read_to_string(path)?;
        let mut shim: TomlShimFile = toml::from_str(&content)?;

        // Extract shim name from file name
        let shim_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| color_eyre::eyre::eyre!("Invalid shim file name"))?
            .to_string();

        // Determine tool name from tool field or derive from shim name
        let tool_name = shim
            .tool
            .clone()
            .or_else(|| shim.opts.get("tool").map(|s| s.to_string()))
            .unwrap_or_else(|| shim_name.clone());

        // Determine bin name (what executable to run) - defaults to filename
        let bin_name = shim.bin.clone().unwrap_or_else(|| shim_name.clone());

        shim.tool_name = tool_name;
        shim.bin_name = bin_name;

        Ok(shim)
    }

    // Create a ToolRequest directly using ToolVersionOptions
    pub fn to_tool_request(&self, shim_path: &Path) -> Result<ToolRequest> {
        use crate::cli::args::BackendArg;

        let backend_arg = BackendArg::from(&self.tool_name);
        let source = ToolSource::TomlShim(shim_path.to_path_buf());

        // Create ToolVersionOptions from our fields
        let mut opts = self.opts.clone();
        opts.shift_remove("tool"); // Remove tool field since it's handled separately

        let options = ToolVersionOptions {
            os: self.os.clone(),
            install_env: self.install_env.clone(),
            opts,
        };

        if options.is_empty() {
            // Simple case: no options, just version
            ToolRequest::new(backend_arg.into(), &self.version, source)
        } else {
            // Complex case: use ToolVersionOptions directly
            ToolRequest::new_opts(backend_arg.into(), &self.version, options, source)
        }
    }

    // Keep the old method for simple backends that work with ToolArg
    pub fn to_tool_arg(&self) -> Result<ToolArg> {
        // Check if we have any complex options that ToolArg can't handle
        if !self.install_env.is_empty() || self.os.is_some() {
            return Err(color_eyre::eyre::eyre!(
                "Complex options (install_env, os) not supported in ToolArg format. Use to_tool_request instead."
            ));
        }

        let mut tool_spec = format!("{}@{}", self.tool_name, self.version);

        // Filter out the 'tool' field and add remaining options
        let filtered_options: Vec<_> = self.opts.iter().filter(|(key, _)| *key != "tool").collect();

        if !filtered_options.is_empty() {
            let option_parts: Vec<String> = filtered_options
                .iter()
                .map(|(key, value)| format!("{key}={value}"))
                .collect();
            tool_spec.push_str(&format!(",{}", option_parts.join(",")));
        }

        tool_spec.parse::<ToolArg>()
    }
}

// Cache just stores the binary path as a raw string
// The mtime is already encoded in the cache key, so no need to store it

struct BinPathCache;

impl BinPathCache {
    fn cache_key(shim_path: &Path) -> Result<String> {
        let path_str = shim_path.to_string_lossy();
        let mtime = shim_path.metadata()?.modified()?;
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
        dirs::CACHE.join("toml-shims").join(cache_key)
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
    shim: &TomlShimFile,
    shim_path: &Path,
) -> Result<Option<PathBuf>> {
    // Generate cache key from file path and mtime
    let cache_key = BinPathCache::cache_key(shim_path)?;

    // Try to load from cache first
    if let Some(bin_path) = BinPathCache::load(&cache_key) {
        return Ok(Some(bin_path));
    }

    // Cache miss - resolve the binary path
    if let Some((backend, tv)) = toolset.which(config, &shim.bin_name).await {
        if let Some(bin_path) = backend.which(config, &tv, &shim.bin_name).await? {
            // Cache the result
            if let Err(e) = BinPathCache::save(&bin_path, &cache_key) {
                // Don't fail if caching fails, just log it
                log::warn!("Failed to cache binary path: {e}");
            }

            return Ok(Some(bin_path));
        }
    }

    Ok(None)
}

pub async fn execute_toml_shim(shim_path: &Path, args: Vec<String>) -> Result<()> {
    let shim = TomlShimFile::from_file(shim_path)?;
    let mut config = Config::get().await?;

    // First try to use the simple ToolArg approach (faster and cleaner)
    match shim.to_tool_arg() {
        Ok(tool_arg) => {
            // Simple case: no complex options, use ToolArg
            return execute_with_tool_arg(tool_arg, &shim, &mut config, args, shim_path).await;
        }
        Err(_) => {
            // Complex case: use direct ToolRequest approach
            return execute_with_tool_request(&shim, &mut config, args, shim_path).await;
        }
    }
}

async fn execute_with_tool_request(
    shim: &TomlShimFile,
    config: &mut std::sync::Arc<Config>,
    args: Vec<String>,
    shim_path: &Path,
) -> Result<()> {
    // Use direct ToolRequest creation with ToolVersionOptions
    let tool_request = shim.to_tool_request(shim_path)?;

    // Build a toolset with the tool request for caching compatibility
    let backend_arg = tool_request.ba().clone();
    let version_str = tool_request.version();

    // Create a ToolArg that represents our parsed tool
    let tool_spec = format!("{}@{}", backend_arg.short, version_str);
    let tool_arg: ToolArg = tool_spec.parse()?;

    // Build toolset and use caching
    let mut toolset = ToolsetBuilder::new()
        .with_args(&[tool_arg])
        .with_default_to_latest(true)
        .build(config)
        .await?;

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
        find_cached_or_resolve_bin_path(&toolset, &*config, shim, shim_path).await?
    {
        // Get the environment with proper PATH from toolset
        let env = toolset.env_with_path(config).await?;

        return execute_tool(bin_path, args, env);
    }

    bail!(
        "Tool '{}' or bin '{}' not found",
        shim.tool_name,
        shim.bin_name
    );
}

async fn execute_with_tool_arg(
    tool_arg: ToolArg,
    shim: &TomlShimFile,
    config: &mut std::sync::Arc<Config>,
    args: Vec<String>,
    shim_path: &Path,
) -> Result<()> {
    // Original simple approach for backends without complex nested options
    let mut toolset = ToolsetBuilder::new()
        .with_args(&[tool_arg])
        .with_default_to_latest(true)
        .build(config)
        .await?;

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
        find_cached_or_resolve_bin_path(&toolset, &*config, shim, shim_path).await?
    {
        // Get the environment with proper PATH from toolset
        let env = toolset.env_with_path(config).await?;

        return execute_tool(bin_path, args, env);
    }

    bail!(
        "Tool '{}' or bin '{}' not found",
        shim.tool_name,
        shim.bin_name
    );
}

#[cfg(all(not(test), unix))]
fn execute_tool<T, U>(program: T, args: U, env: BTreeMap<String, String>) -> Result<()>
where
    T: IntoExecutablePath,
    U: IntoIterator,
    U::Item: Into<OsString>,
{
    for (k, v) in env.iter() {
        env::set_var(k, v);
    }
    let args = args.into_iter().map(Into::into).collect::<Vec<_>>();
    let program = program.to_executable();
    let err = exec::Command::new(program.clone()).args(&args).exec();
    bail!("{:?} {err}", program.to_string_lossy())
}

#[cfg(all(windows, not(test)))]
fn execute_tool<T, U>(program: T, args: U, env: BTreeMap<String, String>) -> Result<()>
where
    T: IntoExecutablePath,
    U: IntoIterator,
    U::Item: Into<OsString>,
{
    let cwd = crate::dirs::CWD.clone().unwrap_or_default();
    let program = program.to_executable();
    let path = env.get(&*PATH_KEY).map(OsString::from);
    let program = which::which_in(program, path, cwd)?;
    let mut cmd = cmd::cmd(program, args);
    for (k, v) in env.iter() {
        cmd = cmd.env(k, v);
    }

    // Windows does not support exec in the same way as Unix,
    // so we emulate it instead by not handling Ctrl-C and letting
    // the child process deal with it instead.
    win_exec::set_ctrlc_handler()?;

    let res = cmd.unchecked().run()?;
    match res.status.code() {
        Some(0) => Ok(()),
        Some(code) => Err(eyre!("command failed: exit code {}", code)),
        None => Err(eyre!("command failed: terminated by signal")),
    }
}

#[cfg(test)]
fn execute_tool<T, U>(program: T, args: U, env: BTreeMap<String, String>) -> Result<()>
where
    T: IntoExecutablePath,
    U: IntoIterator,
    U::Item: Into<OsString>,
{
    let mut cmd = cmd::cmd(program, args);
    for (k, v) in env.iter() {
        cmd = cmd.env(k, v);
    }
    let res = cmd.unchecked().run()?;
    match res.status.code() {
        Some(0) => Ok(()),
        Some(code) => Err(eyre!("command failed: exit code {}", code)),
        None => Err(eyre!("command failed: terminated by signal")),
    }
}

#[cfg(all(windows, not(test)))]
mod win_exec {
    use color_eyre::eyre::{Result, eyre};
    use winapi::shared::minwindef::{BOOL, DWORD, FALSE, TRUE};
    use winapi::um::consoleapi::SetConsoleCtrlHandler;
    // Windows way of creating a process is to just go ahead and pop a new process
    // with given program and args into existence. But in unix-land, it instead happens
    // in a two-step process where you first fork the process and then exec the new program,
    // essentially replacing the current process with the new one.
    // We use Windows API to set a Ctrl-C handler that does nothing, essentially attempting
    // to emulate the ctrl-c behavior by not handling it ourselves, and propagating it to
    // the child process to handle it instead.
    // This is the same way cargo does it in cargo run.
    unsafe extern "system" fn ctrlc_handler(_: DWORD) -> BOOL {
        // This is a no-op handler to prevent Ctrl-C from terminating the process.
        // It allows the child process to handle Ctrl-C instead.
        TRUE
    }

    pub(super) fn set_ctrlc_handler() -> Result<()> {
        if unsafe { SetConsoleCtrlHandler(Some(ctrlc_handler), TRUE) } == FALSE {
            Err(eyre!("Could not set Ctrl-C handler."))
        } else {
            Ok(())
        }
    }
}

// [experimental ]
#[derive(Debug, Parser)]
pub struct TomlShim {
    /// The TOML shim file to execute
    #[clap(value_name = "FILE")]
    pub file: PathBuf,

    /// Arguments to pass to the tool
    #[clap(trailing_var_arg = true, allow_hyphen_values = true)]
    pub args: Vec<String>,
}

impl TomlShim {
    pub async fn run(self) -> Result<()> {
        execute_toml_shim(&self.file, self.args).await
    }
}

pub(crate) async fn short_circuit_shim(args: &[String]) -> Result<()> {
    // Early return if no args or not enough args for a shim
    if args.is_empty() {
        return Ok(());
    }

    // Check if the first argument looks like a TOML shim file path
    let potential_shim_path = std::path::Path::new(&args[0]);

    // Only proceed if it's an existing file with a reasonable extension
    if !potential_shim_path.exists() {
        return Ok(());
    }

    // Generate cache key from file path and mtime
    let cache_key = BinPathCache::cache_key(potential_shim_path)?;

    // Check if we have a cached binary path
    if let Some(bin_path) = BinPathCache::load(&cache_key) {
        // We have a cached binary path, execute it directly
        let shim_args = if args.len() > 1 {
            args[1..].to_vec()
        } else {
            vec![]
        };

        // Create a minimal environment - for short circuit we don't set up full toolset environment
        // This is a trade-off for performance vs completeness
        let env = std::env::vars().collect::<BTreeMap<String, String>>();

        // Execute the cached binary directly
        return execute_tool(bin_path, shim_args, env);
    }

    // No cache hit, return Ok(()) to continue with normal processing
    Ok(())
}
