use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::dirs;
use crate::file;
use crate::toolset::{InstallOptions, ToolSource};
use color_eyre::eyre::{Result, bail};
use eyre::ensure;
use sha2::{Digest, Sha256};

pub(crate) use crate::tool_stub::ToolStubFile;

// Cache just stores the binary path as a raw string
// The stub's path and content are encoded in the cache key, so no need to store them

struct BinPathCache;

impl BinPathCache {
    /// Identifies a stub by where it is and what it says.
    ///
    /// The path is made absolute because a stub is usually run as `./bin/tool`, which the
    /// stubs of other projects share. The content is hashed instead of the mtime because
    /// archives and checkouts choose the mtime, so unrelated stubs, or the old and new
    /// version of one stub, can share it. Only a regular file has a key: a pipe or
    /// `/dev/stdin` is consumed by the first read, so it cannot be read again to find it.
    ///
    /// `content` is the text the stub was read as. A run hashes the text it parsed, so an entry
    /// saved after the install names the stub that was installed for, even when the file
    /// changed meanwhile.
    ///
    /// The stub's mise.lock is hashed too: it can pin the version a stub's request resolves
    /// to, and `mise lock --bump` or `mise generate tool-stub --lock` change that pin without
    /// touching the stub.
    fn cache_key(stub_path: &Path, content: &[u8]) -> Result<Option<String>> {
        if !Self::is_cacheable(stub_path) {
            return Ok(None);
        }
        // The cache is read before normal startup, so settings that fail to load mean no
        // cache: startup then reports what is wrong instead of this panicking.
        let Ok(settings) = crate::config::Settings::try_get() else {
            return Ok(None);
        };
        let path = std::path::absolute(stub_path)?;
        let mut hasher = Sha256::new();
        hasher.update(path.as_os_str().as_encoded_bytes());
        hasher.update([0]);
        hasher.update(content);
        // Read like `Lockfile::read` reads it: only when lockfiles are on, and only a regular
        // file, with the bounded reader, since a lockfile name that is a pipe or a device
        // would never finish reading.
        if settings.lockfile_enabled()
            && let Some((lockfile, _)) = crate::lockfile::lockfile_path_for_tool_stub(stub_path)
            && lockfile.is_file()
            && let Ok(lock) = file::read_config_to_string(&lockfile)
        {
            hasher.update([0]);
            hasher.update(lockfile.as_os_str().as_encoded_bytes());
            hasher.update([0]);
            hasher.update(lock);
        }
        Ok(Some(hex::encode(hasher.finalize())))
    }

    /// Whether the stub at `stub_path` can be read again to find its entry: a regular file
    /// that is not a file descriptor such as `/dev/stdin`. Stdin redirected from a file is a
    /// regular file, but on macOS reading it moves the offset the later parse reads from, and
    /// two stubs redirected in would share one key.
    fn is_cacheable(stub_path: &Path) -> bool {
        let is_fd = |path: &Path| path.starts_with("/dev") || path.starts_with("/proc");
        stub_path.is_file()
            && !std::path::absolute(stub_path).is_ok_and(|path| is_fd(&path))
            && !stub_path.canonicalize().is_ok_and(|path| is_fd(&path))
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

    /// Saves `bin_path` under `cache_key`, the key of the stub at `stub_path` read as `content`,
    /// unless that key changed while the run resolved. The resolve reads the stub's mise.lock
    /// again, so a `mise lock` that rewrote it after the key hashed it could have picked
    /// another pin than the key names. Returns whether it saved.
    ///
    /// A lockfile that changed and then changed back between the two reads is not noticed.
    fn save_if_unchanged(
        bin_path: &Path,
        cache_key: &str,
        stub_path: &Path,
        content: &[u8],
    ) -> Result<bool> {
        if Self::cache_key(stub_path, content)?.as_deref() != Some(cache_key) {
            debug!(
                "not caching the binary of {}: its key changed",
                stub_path.display()
            );
            return Ok(false);
        }
        Self::save(bin_path, cache_key)?;
        Ok(true)
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
        if tv.ba().short == tool_name {
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
    // The bin the install named the binary it downloaded after: the bin of the platform whose
    // url was downloaded, else the top-level one, else the stub's file name (without extension)
    stub.bin_for_this_platform().unwrap_or_else(|| {
        stub_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(&stub.tool_name)
            .to_string()
    })
}

async fn find_cached_or_resolve_bin_path(
    toolset: &crate::toolset::Toolset,
    config: &std::sync::Arc<Config>,
    stub: &ToolStubFile,
    stub_path: &Path,
    cache_key: Option<&StubCacheKey>,
) -> Result<Result<PathBuf, BinPathError>> {
    // Try to load from cache first
    if let Some(bin_path) = cache_key.and_then(|key| BinPathCache::load(&key.key)) {
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
        if let Some(key) = cache_key
            && let Err(e) =
                BinPathCache::save_if_unchanged(&bin_path, &key.key, stub_path, &key.content)
        {
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
    cache_key: Option<&StubCacheKey>,
) -> Result<()> {
    // Use direct ToolRequest creation with ToolVersionOptions
    let tool_request = stub.to_tool_request(stub_path)?;

    // Create a toolset directly and add the tool request with its options
    let source = ToolSource::ToolStub(stub_path.to_path_buf());
    let mut toolset = crate::toolset::Toolset::new(source);
    toolset.add_version(tool_request);

    // Resolve the toolset to populate current versions. Lock data comes
    // from the stub's project mise.lock (see `lockfile_path_for_tool_stub`).
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

    let (_, missing) = toolset
        .install_missing_versions(config, &install_opts)
        .await?;
    toolset.notify_missing_versions(missing);

    // Find the binary path using cache
    match find_cached_or_resolve_bin_path(&toolset, &*config, stub, stub_path, cache_key).await? {
        Ok(bin_path) => {
            // env_with_path already has _.path and filters outer install dirs.
            // Prepend backend dependency bins (e.g. node for npm); see #7777.
            let (mut env, env_remove) = toolset.env_with_path_and_removals(config).await?;
            if let Some((backend, _tv)) = toolset.list_current_installed_versions(config).first() {
                let dep_paths = backend
                    .dependency_toolset(config)
                    .await?
                    .list_paths(config)
                    .await;
                if !dep_paths.is_empty() {
                    let path = env
                        .get(crate::env::PATH_KEY.as_str())
                        .cloned()
                        .unwrap_or_default();
                    let mut paths = dep_paths;
                    paths.extend(std::env::split_paths(&path));
                    env.insert(
                        crate::env::PATH_KEY.to_string(),
                        std::env::join_paths(paths)?.to_string_lossy().to_string(),
                    );
                }
            }
            crate::cli::exec::exec_program(
                bin_path,
                args,
                env,
                env_remove,
                &Default::default(),
                false,
                &Default::default(),
            )
            .await
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

/// Run a tool stub file
///
/// A tool stub is an executable file whose shebang is
/// `#!/usr/bin/env -S mise tool-stub` and whose body is TOML naming a tool,
/// version, and executable. Running the stub runs this command, which installs
/// the tool if needed and runs it with the stub's arguments. You rarely call it
/// directly; create stubs with `mise generate tool-stub`.
///
/// ```toml
/// #!/usr/bin/env -S mise tool-stub
/// # Node.js 24
///
/// tool = "node"
/// version = "24"
/// bin = "node"
/// ```
///
/// See https://mise.jdx.dev/dev-tools/tool-stubs.html
#[derive(Debug, usage_rs::Args)]
#[usage(
    disable_help_flag = true,
    disable_version_flag = true,
    verbatim_doc_comment
)]
pub(crate) struct ToolStub {
    /// Path to the TOML tool stub file to execute
    ///
    /// Every field is optional: `version` defaults to `latest`, `tool` defaults
    /// to the stub's file name (or `http:<file name>` when the stub sets `url` or
    /// platform URLs), and `bin` defaults to the stub's file name. A stub can
    /// also set backend-specific options.
    #[usage(value_name = "FILE", double_dash = "automatic")]
    pub file: PathBuf,

    /// Arguments to pass to the tool
    ///
    /// Every argument after the stub path is passed to the tool unchanged,
    /// including a literal `--`.
    #[usage(trailing_var_arg = true, allow_hyphen_values = true)]
    pub args: Vec<String>,
}

impl ToolStub {
    pub(crate) async fn run(self) -> Result<()> {
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

        let stub_path = invoked_stub_path(&self.file);
        let content = stub_content(&stub_path)?;
        let cache_key =
            BinPathCache::cache_key(&stub_path, content.as_bytes())?.map(|key| StubCacheKey {
                key,
                content: content.clone().into_bytes(),
            });
        let stub = ToolStubFile::from_content(&stub_path, content)?;

        // Track the stub so `mise prune` knows its tool is still needed. Stubs
        // can live anywhere, so execution time is the only chance to find them.
        // absolute() rather than canonicalize() so a symlinked stub keeps its
        // invoked filename, which the tool name can be derived from.
        track_stub_use(&stub_path);

        let mut config = Config::get().await?;

        return execute_with_tool_request(&stub, &mut config, args, &stub_path, cache_key.as_ref())
            .await;
    }
}

/// A stub's cache key, with the text it was computed from: the entry is saved only when that
/// text still gives the same key after the run resolved (see [`BinPathCache::save_if_unchanged`]).
struct StubCacheKey {
    key: String,
    content: Vec<u8>,
}

/// The stub the command line names at `path`, made absolute against the directory mise started
/// in. A shebang passes the stub as the user ran it, often `./bin/tool`, and `--cd` or `MISE_CD`
/// moves mise to another directory before the run reads it, where that name is another stub.
/// The fast path and the run read, key and hand off this one path.
fn invoked_stub_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        return path.to_path_buf();
    }
    crate::env::original_cwd()
        .map(|cwd| cwd.join(path))
        .unwrap_or_else(|| path.to_path_buf())
}

/// Tracks `file` as a stub that ran.
fn track_stub_use(file: &Path) {
    let stub_path = std::path::absolute(file).unwrap_or_else(|_| file.to_path_buf());
    if let Err(err) = crate::config::tracking::Tracker::track_stub(&stub_path) {
        crate::warn!("tracking tool stub: {err:#}");
    }
}

/// The stub the fast path read and found no cache entry for, kept so `ToolStub::run` parses
/// the text the fast path hashed instead of reading the file a second time.
static READ_BY_FAST_PATH: std::sync::Mutex<Option<(PathBuf, String)>> = std::sync::Mutex::new(None);

/// The text of the stub at `path`: what the fast path read, when it read this stub, or the file.
fn stub_content(path: &Path) -> Result<String> {
    let read = READ_BY_FAST_PATH
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .take();
    match read {
        Some((read_path, content)) if read_path == path => Ok(content),
        _ => file::read_to_string(path),
    }
}

/// Reads a stub for the fast path, or `None` to skip the cache: the opened file must be a
/// regular file no larger than a config file, judged on the handle, so a pipe swapped in
/// after `is_cacheable` cannot hang startup and a huge file is not loaded before the cache
/// is even checked. The normal path reports what is wrong with such a file.
fn read_stub_for_cache(path: &Path) -> Option<String> {
    file::read_config_to_string(path).ok()
}

pub(crate) async fn short_circuit_stub(args: &[String]) -> Result<()> {
    // Early return if no args or not enough args for a stub
    if args.is_empty() {
        return Ok(());
    }

    // Check if the first argument looks like a tool stub file path
    let potential_stub_path = &invoked_stub_path(Path::new(&args[0]));

    // Only proceed if it's an existing file with a reasonable extension
    if !potential_stub_path.exists() {
        return Ok(());
    }

    if !BinPathCache::is_cacheable(potential_stub_path) {
        return Ok(());
    }
    let Some(content) = read_stub_for_cache(potential_stub_path) else {
        return Ok(());
    };
    let Some(cache_key) = BinPathCache::cache_key(potential_stub_path, content.as_bytes())? else {
        return Ok(());
    };

    // Check if we have a cached binary path
    if let Some(bin_path) = BinPathCache::load(&cache_key) {
        // This path skips `ToolStub::run`, so it records the use itself, or an
        // expired entry would let `mise prune` delete the tool it runs.
        track_stub_use(potential_stub_path);
        let args = args[1..].to_vec();
        return crate::cli::exec::exec_program(
            bin_path,
            args,
            BTreeMap::new(),
            Default::default(),
            &Default::default(),
            false,
            &Default::default(),
        )
        .await;
    }

    // No cache hit: continue with normal processing, which parses the text read here.
    *READ_BY_FAST_PATH
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) =
        Some((potential_stub_path.clone(), content));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_stub(path: &Path, version: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, format!("tool = \"dummy\"\nversion = \"{version}\"\n")).unwrap();
        // what `git archive` or a copy leaves behind
        let mtime = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000);
        std::fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(mtime)
            .unwrap();
    }

    fn key_on_disk(path: &Path) -> Result<Option<String>> {
        BinPathCache::cache_key(path, &std::fs::read(path)?)
    }

    #[test]
    fn the_fast_path_skips_a_stub_larger_than_a_config_file() {
        let tmp = tempfile::tempdir().unwrap();
        let stub = tmp.path().join("bin/scan");
        write_stub(&stub, "1.0.0");
        assert!(read_stub_for_cache(&stub).is_some());
        // sparse, so the test writes nothing
        std::fs::File::options()
            .write(true)
            .open(&stub)
            .unwrap()
            .set_len(file::CONFIG_FILE_LIMIT + 1)
            .unwrap();
        assert!(read_stub_for_cache(&stub).is_none());
    }

    #[test]
    fn a_run_parses_the_text_the_fast_path_read() {
        let tmp = tempfile::tempdir().unwrap();
        let stub = tmp.path().join("bin/scan");
        let other = tmp.path().join("bin/other");
        write_stub(&stub, "2.0.0");
        write_stub(&other, "3.0.0");
        let read = "tool = \"dummy\"\nversion = \"1.0.0\"\n".to_string();
        *READ_BY_FAST_PATH.lock().unwrap() = Some((stub.clone(), read.clone()));
        // another stub reads its own file
        assert!(stub_content(&other).unwrap().contains("3.0.0"));
        *READ_BY_FAST_PATH.lock().unwrap() = Some((stub.clone(), read.clone()));
        assert_eq!(stub_content(&stub).unwrap(), read);
        // taken once: a later read is of the file
        assert!(stub_content(&stub).unwrap().contains("2.0.0"));
    }

    #[test]
    fn cache_key_names_the_text_that_was_parsed() {
        let tmp = tempfile::tempdir().unwrap();
        let stub = tmp.path().join("bin/scan");
        write_stub(&stub, "1.0.0");
        let parsed = std::fs::read(&stub).unwrap();
        let installed_for = key_on_disk(&stub).unwrap();
        // the file changes while the tool it named installs
        write_stub(&stub, "2.0.0");
        assert_eq!(
            installed_for,
            BinPathCache::cache_key(&stub, &parsed).unwrap()
        );
        assert_ne!(installed_for, key_on_disk(&stub).unwrap());
    }

    #[test]
    fn cache_key_tells_apart_stubs_with_the_same_mtime() {
        let tmp = tempfile::tempdir().unwrap();
        let acme = tmp.path().join("acme/bin/scan");
        let other = tmp.path().join("other/bin/scan");
        write_stub(&acme, "2.0.0");
        write_stub(&other, "1.0.0");
        let acme_key = key_on_disk(&acme).unwrap().unwrap();
        assert_ne!(Some(&acme_key), key_on_disk(&other).unwrap().as_ref());

        // the same stub in another project is another stub: it has its own mise.lock
        write_stub(&other, "2.0.0");
        assert_ne!(Some(&acme_key), key_on_disk(&other).unwrap().as_ref());

        // replaced in place without a newer mtime
        write_stub(&acme, "1.0.0");
        assert_ne!(Some(&acme_key), key_on_disk(&acme).unwrap().as_ref());
        write_stub(&acme, "2.0.0");
        assert_eq!(Some(acme_key), key_on_disk(&acme).unwrap());

        // a directory, or a pipe that cannot be read twice, has no entry
        assert_eq!(None, BinPathCache::cache_key(tmp.path(), b"").unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn cache_key_skips_file_descriptors() {
        let tmp = tempfile::tempdir().unwrap();
        let stub = tmp.path().join("bin/scan");
        write_stub(&stub, "1.0.0");
        let file = std::fs::File::open(&stub).unwrap();
        let fd = std::path::PathBuf::from(format!("/dev/fd/{}", {
            use std::os::fd::AsRawFd;
            file.as_raw_fd()
        }));
        assert!(fd.is_file());
        assert!(!BinPathCache::is_cacheable(&fd));
        assert!(BinPathCache::is_cacheable(&stub));
    }

    #[test]
    fn cache_key_changes_with_the_stubs_lockfile() {
        let tmp = tempfile::tempdir().unwrap();
        // The test harness overrides the config filenames.
        std::fs::write(tmp.path().join(".test.mise.toml"), "[tools]\n").unwrap();
        let stub = tmp.path().join("bin/scan");
        write_stub(&stub, "latest");
        let unlocked = key_on_disk(&stub).unwrap();
        std::fs::write(
            tmp.path().join("mise.lock"),
            "[[tools.dummy]]\nversion = \"1.0.0\"\n",
        )
        .unwrap();
        let locked = key_on_disk(&stub).unwrap();
        assert_ne!(unlocked, locked);
        // a bump rewrites only the lockfile
        std::fs::write(
            tmp.path().join("mise.lock"),
            "[[tools.dummy]]\nversion = \"2.0.0\"\n",
        )
        .unwrap();
        assert_ne!(locked, key_on_disk(&stub).unwrap());
    }

    #[test]
    fn no_entry_is_saved_when_the_lockfile_changed_during_the_run() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join(".test.mise.toml"), "[tools]\n").unwrap();
        let stub = tmp.path().join("bin/scan");
        write_stub(&stub, "latest");
        let lockfile = tmp.path().join("mise.lock");
        std::fs::write(&lockfile, "[[tools.dummy]]\nversion = \"1.0.0\"\n").unwrap();
        let content = std::fs::read(&stub).unwrap();
        let key = BinPathCache::cache_key(&stub, &content).unwrap().unwrap();
        let bin = tmp.path().join("dummy");
        // `mise lock --bump` while the run resolved: the key names the 1.0.0 pin, and the
        // binary found may be another version's
        std::fs::write(&lockfile, "[[tools.dummy]]\nversion = \"2.0.0\"\n").unwrap();
        assert!(!BinPathCache::save_if_unchanged(&bin, &key, &stub, &content).unwrap());
        assert_eq!(None, BinPathCache::load(&key));
        // an unchanged lockfile keeps the entry
        let key = BinPathCache::cache_key(&stub, &content).unwrap().unwrap();
        std::fs::write(&bin, "").unwrap();
        assert!(BinPathCache::save_if_unchanged(&bin, &key, &stub, &content).unwrap());
        assert_eq!(Some(bin), BinPathCache::load(&key));
        std::fs::remove_file(BinPathCache::cache_file_path(&key)).unwrap();
    }

    #[test]
    fn a_relative_stub_is_the_one_in_the_directory_mise_started_in() {
        let started_in = crate::env::original_cwd().unwrap();
        assert_eq!(
            started_in.join("bin/tool"),
            invoked_stub_path(Path::new("bin/tool"))
        );
        assert_eq!(
            PathBuf::from("/opt/bin/tool"),
            invoked_stub_path(Path::new("/opt/bin/tool"))
        );
    }

    #[cfg(unix)]
    #[test]
    fn cache_key_skips_a_lockfile_that_is_not_a_regular_file() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join(".test.mise.toml"), "[tools]\n").unwrap();
        let stub = tmp.path().join("bin/scan");
        write_stub(&stub, "latest");
        let unlocked = key_on_disk(&stub).unwrap();
        // reading a pipe with no writer would never return
        let status = std::process::Command::new("mkfifo")
            .arg(tmp.path().join("mise.lock"))
            .status()
            .unwrap();
        assert!(status.success());
        assert_eq!(unlocked, key_on_disk(&stub).unwrap());
    }
}
