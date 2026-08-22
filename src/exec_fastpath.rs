//! Pre-runtime fast path for shim and plain `mise x -- <cmd>` invocations.
//!
//! When `MISE_EXEC_CACHE=1`, a successful shim/exec resolution writes an
//! encrypted record of the final environment plus everything that influenced
//! it: every loaded config file, every watch file, and every config search
//! directory, each with its mtime. The next invocation consults the record
//! from `main()` before the tokio runtime, clap, logging, or config exist.
//! On a hit it re-validates every recorded mtime, rebuilds the exact env the
//! slow path produced, and execs the target directly. Any doubt — a changed
//! mtime, a new config file appearing in an ancestor dir, a missing tool dir,
//! an expired TTL, a decryption failure — falls through to the normal path,
//! which rewrites the record.
//!
//! Records never key on caller-specific state they can't validate: the cache
//! key covers the mise version, cwd, all pristine `MISE_*` variables, and the
//! pristine `PATH`, so a change to any of those simply selects a different
//! (likely absent) record.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::config::{Config, Settings};
use crate::env_diff::EnvMap;
use crate::toolset::env_cache::{CachedEnv, decrypt_data, encrypt_data};
use crate::{dirs, env, hook_env};

const KEY_SCOPE: &str = "exec-fastpath-v1";

fn debug_enabled() -> bool {
    std::env::var_os("MISE_EXEC_CACHE_DEBUG").is_some()
}

macro_rules! fastpath_debug {
    ($($arg:tt)*) => {
        if debug_enabled() {
            eprintln!("[exec-fastpath] {}", format!($($arg)*));
        }
    };
}

pub(crate) fn enabled() -> bool {
    env::var_is_true("MISE_EXEC_CACHE")
}

#[derive(Debug, Serialize, Deserialize)]
struct ExecCacheRecord {
    /// mise version that wrote the record
    version: String,
    created_at: u64,
    expires_at: u64,
    /// the env overlay the slow path passes to exec_program (includes PATH);
    /// applied onto the inherited env exactly like exec_program's set_var loop
    env: Vec<(String, String)>,
    /// PATH entries mise added (ordered); shims may only serve bins from these
    tool_paths: Vec<PathBuf>,
    /// files that influenced the resolution: (path, mtime millis, 0 = absent)
    files: Vec<(PathBuf, u64)>,
    /// config search dirs: a new config file appearing bumps the dir mtime
    dirs: Vec<(PathBuf, u64)>,
}

#[derive(Debug, PartialEq)]
enum Mode {
    /// invoked via a shim symlink; the tool name comes from argv0
    Shim(String),
    /// invoked as plain `mise x -- <cmd> [args..]` / `mise exec -- <cmd> [args..]`
    Exec,
}

impl Mode {
    fn tag(&self) -> &'static str {
        match self {
            Mode::Shim(_) => "shim",
            Mode::Exec => "exec",
        }
    }
}

/// Strictly detect a fast-path-eligible invocation from raw args. Anything
/// with flags, tool@version args, or `-c` forms returns None so the slow path
/// keeps its exact semantics.
///
/// Takes args explicitly (from `std::env::args()` on the pre-runtime read
/// side) — `env::ARGS`/`MISE_BIN_NAME` are only populated later by `Cli::run`.
fn detect_mode(args: &[String]) -> Option<Mode> {
    let argv0 = args.first()?;
    let bin_name = Path::new(argv0).file_name()?.to_str()?;
    if !env::is_mise_binary(bin_name) {
        // shims never contain path separators
        if bin_name.contains('/') || bin_name.contains('\\') {
            return None;
        }
        return Some(Mode::Shim(bin_name.to_string()));
    }
    if args.len() >= 4
        && (args[1] == "x" || args[1] == "exec")
        && args[2] == "--"
        && !args[3].is_empty()
    {
        return Some(Mode::Exec);
    }
    None
}

fn mtime_millis(path: &Path) -> u64 {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn cache_dir() -> PathBuf {
    dirs::STATE.join("exec-cache")
}

fn key_file() -> PathBuf {
    dirs::STATE.join("exec-cache.key")
}

/// Encryption key: prefer the session key from the env (same one env_cache
/// uses), fall back to a persistent 0600 key file so shims outside activated
/// shells still get the fast path.
fn get_key(create: bool) -> Option<[u8; 32]> {
    use base64::Engine;
    use base64::prelude::BASE64_STANDARD;
    let decode = |s: &str| -> Option<[u8; 32]> {
        BASE64_STANDARD
            .decode(s.trim())
            .ok()
            .and_then(|b| b.try_into().ok())
    };
    if let Ok(s) = std::env::var("__MISE_ENV_CACHE_KEY")
        && let Some(k) = decode(&s)
    {
        return Some(k);
    }
    let kf = key_file();
    if let Ok(s) = fs::read_to_string(&kf)
        && let Some(k) = decode(&s)
    {
        return Some(k);
    }
    if !create {
        return None;
    }
    let encoded = CachedEnv::generate_encryption_key();
    let key = decode(&encoded)?;
    fs::create_dir_all(*dirs::STATE).ok()?;
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&kf)
            .ok()?;
        f.write_all(encoded.as_bytes()).ok()?;
    }
    #[cfg(not(unix))]
    fs::write(&kf, &encoded).ok()?;
    Some(key)
}

/// Cache key from inputs knowable in microseconds, before config exists.
fn compute_key(mode: &Mode) -> Option<String> {
    let cwd = std::env::current_dir().ok()?;
    let mut hasher = blake3::Hasher::new();
    hasher.update(KEY_SCOPE.as_bytes());
    hasher.update(env!("CARGO_PKG_VERSION").as_bytes());
    hasher.update(mode.tag().as_bytes());
    hasher.update(cwd.to_string_lossy().as_bytes());
    // all pristine MISE_* vars affect config discovery/templates; BTreeMap is
    // already sorted so the hash is stable
    for (k, v) in env::PRISTINE_ENV.iter() {
        if k.starts_with("MISE_") {
            hasher.update(k.as_bytes());
            hasher.update(b"=");
            hasher.update(v.as_bytes());
            hasher.update(b"\0");
        }
    }
    // record env_add bakes in the final PATH, which was derived from the
    // pristine PATH — so the pristine PATH must be part of the key
    if let Some(path) = env::PRISTINE_ENV.get(&*env::PATH_KEY) {
        hasher.update(path.as_bytes());
    }
    Some(hex::encode(hasher.finalize().as_bytes()))
}

fn is_executable_file(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    path.is_file()
}

fn find_bin(paths: impl IntoIterator<Item = PathBuf>, name: &str) -> Option<PathBuf> {
    paths
        .into_iter()
        .map(|p| p.join(name))
        .find(|p| is_executable_file(p))
}

/// The pre-runtime entry point, called from `main()` before tokio, clap,
/// logging, or config. Returns only on a miss; a hit execs the target.
pub(crate) fn try_run() -> Option<std::process::ExitCode> {
    #[cfg(unix)]
    {
        if !enabled() {
            return None;
        }
        match try_run_inner() {
            Ok(()) => None,
            Err(reason) => {
                fastpath_debug!("miss: {reason}");
                None
            }
        }
    }
    #[cfg(not(unix))]
    None
}

#[cfg(unix)]
fn try_run_inner() -> Result<(), String> {
    // env::ARGS is only populated later by Cli::run; read the OS args directly
    let args: Vec<String> = std::env::args().collect();
    let mode = detect_mode(&args).ok_or("not an eligible invocation")?;
    // a shim invoked from inside a mise-exec'd process: let the slow path
    // handle recursion detection and error reporting
    if matches!(mode, Mode::Shim(_)) && std::env::var_os("__MISE_SHIM").is_some() {
        return Err("nested shim invocation".into());
    }

    let key = compute_key(&mode).ok_or("could not compute key")?;
    let cache_file = cache_dir().join(&key);
    let encrypted = fs::read(&cache_file).map_err(|_| "no record".to_string())?;
    let enc_key = get_key(false).ok_or("no encryption key")?;
    let decrypted = decrypt_data(&encrypted, &enc_key).map_err(|e| format!("decrypt: {e}"))?;
    let record: ExecCacheRecord =
        rmp_serde::from_slice(&decrypted).map_err(|e| format!("decode: {e}"))?;

    if record.version != env!("CARGO_PKG_VERSION") {
        return Err("version mismatch".into());
    }
    if now_secs() > record.expires_at {
        let _ = fs::remove_file(&cache_file);
        return Err("expired".into());
    }
    for (path, mtime) in record.files.iter().chain(record.dirs.iter()) {
        let current = mtime_millis(path);
        if current != *mtime {
            return Err(format!(
                "mtime changed: {} ({} -> {})",
                path.display(),
                mtime,
                current
            ));
        }
    }
    for dir in &record.tool_paths {
        if !dir.is_dir() {
            return Err(format!("tool path missing: {}", dir.display()));
        }
    }

    // the overlay applied onto the inherited env, mirroring exec_program
    let mut overlay: EnvMap = record.env.iter().cloned().collect();

    let (bin, bin_args) = match &mode {
        Mode::Shim(name) => {
            // only serve bins provided by mise-managed paths; anything else
            // (auto-install, registry fallback) belongs to the slow path
            let bin = find_bin(record.tool_paths.iter().cloned(), name)
                .ok_or_else(|| format!("{name} not in tool paths"))?;
            overlay.insert("__MISE_SHIM".into(), "1".into());
            (bin, args[1..].to_vec())
        }
        Mode::Exec => {
            let name = &args[3];
            let bin = if name.contains('/') {
                let p = PathBuf::from(name);
                if !is_executable_file(&p) {
                    return Err(format!("{name} not executable"));
                }
                p
            } else {
                // like exec_program's lookup reorder: mise-added paths first,
                // then the overlay PATH
                let path = overlay.get(&*env::PATH_KEY).cloned().unwrap_or_default();
                find_bin(
                    record
                        .tool_paths
                        .iter()
                        .cloned()
                        .chain(std::env::split_paths(&path)),
                    name,
                )
                .ok_or_else(|| format!("{name} not on cached PATH"))?
            };
            (bin, args[4..].to_vec())
        }
    };

    fastpath_debug!(
        "hit: exec {} ({} overlay vars)",
        bin.display(),
        overlay.len()
    );
    use std::os::unix::process::CommandExt;
    let err = std::process::Command::new(&bin)
        .args(&bin_args)
        .envs(&overlay)
        .exec();
    // exec only returns on failure; fall through to the slow path
    Err(format!("exec failed: {err}"))
}

/// Called by the slow path (src/cli/exec.rs) right before exec_program, once
/// the final env is fully computed. Failures only cost the cache, never the
/// command.
pub(crate) async fn maybe_write_record(
    config: &Arc<Config>,
    ts: &crate::toolset::Toolset,
    final_env: &EnvMap,
) {
    if !enabled() || !cfg!(unix) {
        return;
    }
    let args: Vec<String> = std::env::args().collect();
    let Some(mode) = detect_mode(&args) else {
        return;
    };
    if let Err(e) = write_record(config, ts, final_env, &mode).await {
        fastpath_debug!("write failed: {e}");
    } else {
        fastpath_debug!("wrote record ({})", mode.tag());
    }
}

async fn write_record(
    config: &Arc<Config>,
    ts: &crate::toolset::Toolset,
    final_env: &EnvMap,
    mode: &Mode,
) -> eyre::Result<()> {
    // the overlay verbatim; caller/session-specific shim markers stay out of
    // the record — the read side re-adds them for its own invocation
    let overlay: Vec<(String, String)> = final_env
        .iter()
        .filter(|(k, _)| !k.starts_with("__MISE_SHIM"))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();

    // the toolset's actual bin paths — the same set which_shim resolves from
    let tool_paths: Vec<PathBuf> = ts.list_paths(config).await;

    // everything that influenced the resolution, with mtimes
    let mut files: Vec<(PathBuf, u64)> = vec![];
    let mut seen = BTreeSet::new();
    for path in config.config_files.keys() {
        if seen.insert(path.clone()) {
            files.push((path.clone(), mtime_millis(path)));
        }
    }
    for path in hook_env::get_watch_files(config.watch_files().await?)? {
        if seen.insert(path.clone()) {
            let mtime = mtime_millis(&path);
            files.push((path, mtime));
        }
    }

    // config search dirs: a new config file appearing in an ancestor of cwd
    // (or the global config dir) must invalidate the record
    let mut dir_mtimes: Vec<(PathBuf, u64)> = vec![];
    let cwd = std::env::current_dir()?;
    for dir in cwd.ancestors() {
        dir_mtimes.push((dir.to_path_buf(), mtime_millis(dir)));
    }
    dir_mtimes.push((dirs::CONFIG.to_path_buf(), mtime_millis(&dirs::CONFIG)));

    let now = now_secs();
    let record = ExecCacheRecord {
        version: env!("CARGO_PKG_VERSION").to_string(),
        created_at: now,
        expires_at: now + Settings::get().env_cache_ttl().as_secs(),
        env: overlay,
        tool_paths,
        files,
        dirs: dir_mtimes,
    };

    let key = compute_key(mode).ok_or_else(|| eyre::eyre!("could not compute key"))?;
    let enc_key = get_key(true).ok_or_else(|| eyre::eyre!("could not obtain encryption key"))?;
    let serialized = rmp_serde::to_vec(&record)?;
    let encrypted = encrypt_data(&serialized, &enc_key)?;
    let dir = cache_dir();
    crate::file::create_dir_all(&dir)?;
    crate::file::write(dir.join(&key), &encrypted)?;
    Ok(())
}
