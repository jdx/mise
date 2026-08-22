//! Thin fast-path reader for mise's exec cache (`src/exec_fastpath.rs`).
//!
//! PROTOTYPE — pairs with the `MISE_EXEC_CACHE=1` writer PR. mise itself can
//! serve exec-cache hits from `main()`, but it still pays the fat binary's
//! load cost (~4ms on linux-x64). This binary reads the same encrypted
//! records, validates them the same way, and execs the target with ~0.8ms of
//! total overhead. Anything it cannot serve execs the real mise binary with
//! argv0 preserved, so mise's own shim dispatch runs and (re)writes the
//! record this binary hits next time — self-priming, no coordination needed.
//!
//! Known prototype limits:
//! - the record struct is duplicated from `src/exec_fastpath.rs`; production
//!   would share it via a small crate
//! - bails to the fallback when `__MISE_DIFF` is set (no EnvDiff reversal, so
//!   the pristine env inside `mise activate`d shells can't be reconstructed);
//!   keying records on the raw env strings instead would remove this
//! - Windows compiles and falls back correctly but the fast path is untested

#![allow(unknown_lints)]
#![deny(dead_code_pub_in_binary, unreachable_pub)]

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::prelude::BASE64_STANDARD;
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Nonce};
use serde::Deserialize;

const KEY_SCOPE: &str = "exec-fastpath-v1";
/// The workspace mise version, embedded by build.rs: records are keyed by the
/// version of the mise binary that wrote them, and mise-exec ships alongside.
const MISE_VERSION: &str = env!("MISE_PAIRED_VERSION");

/// Mirror of `ExecCacheRecord` in `src/exec_fastpath.rs` (rmp-serde encodes
/// struct fields positionally, so field order must match exactly).
#[derive(Deserialize)]
struct ExecCacheRecord {
    version: String,
    #[allow(dead_code)]
    created_at: u64,
    expires_at: u64,
    env: Vec<(String, String)>,
    tool_paths: Vec<PathBuf>,
    files: Vec<(PathBuf, u64)>,
    dirs: Vec<(PathBuf, u64)>,
}

enum Mode {
    /// invoked via a shim named after the tool (argv0 dispatch)
    Shim(String),
    /// invoked as `mise-exec x -- <cmd> [args..]` (mise-compatible argv)
    Exec,
}

fn state_dir() -> PathBuf {
    if let Ok(d) = std::env::var("MISE_STATE_DIR") {
        return PathBuf::from(d);
    }
    if let Ok(d) = std::env::var("XDG_STATE_HOME") {
        return PathBuf::from(d).join("mise");
    }
    #[cfg(windows)]
    if let Ok(d) = std::env::var("LOCALAPPDATA") {
        return PathBuf::from(d).join("mise").join("state");
    }
    PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".local/state/mise")
}

fn mtime_millis(path: &Path) -> u64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn get_key() -> Option<[u8; 32]> {
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
    std::fs::read_to_string(state_dir().join("exec-cache.key"))
        .ok()
        .and_then(|s| decode(&s))
}

fn detect_mode(args: &[OsString]) -> Option<Mode> {
    let bin_name = Path::new(args.first()?).file_name()?.to_str()?;
    // same shape as mise's env::is_mise_binary: "mise" plus "mise." / "mise-"
    // prefixes ("mise-exec", "mise-exec.exe", "mise.exe", ...)
    let is_mise = bin_name == "mise"
        || bin_name
            .split_once(['.', '-'])
            .is_some_and(|(stem, _)| stem == "mise");
    if !is_mise {
        return Some(Mode::Shim(bin_name.to_string()));
    }
    if args.len() >= 4 && (args[1] == "x" || args[1] == "exec") && args[2] == "--" {
        return Some(Mode::Exec);
    }
    None
}

fn compute_key(mode: &Mode) -> Option<String> {
    let cwd = std::env::current_dir().ok()?;
    let mut hasher = blake3::Hasher::new();
    hasher.update(KEY_SCOPE.as_bytes());
    hasher.update(MISE_VERSION.as_bytes());
    hasher.update(match mode {
        Mode::Shim(_) => b"shim".as_slice(),
        Mode::Exec => b"exec".as_slice(),
    });
    hasher.update(cwd.to_string_lossy().as_bytes());
    // pristine env == current env here: try_fast bails when __MISE_DIFF is set.
    // Non-UTF-8 pairs are dropped, matching mise's env::vars_safe()
    let mise_vars: BTreeMap<String, String> = std::env::vars_os()
        .filter_map(|(k, v)| Some((k.into_string().ok()?, v.into_string().ok()?)))
        .filter(|(k, _)| k.starts_with("MISE_"))
        .collect();
    for (k, v) in &mise_vars {
        hasher.update(k.as_bytes());
        hasher.update(b"=");
        hasher.update(v.as_bytes());
        hasher.update(b"\0");
    }
    if let Ok(path) = std::env::var("PATH") {
        hasher.update(path.as_bytes());
    }
    Some(hex::encode(hasher.finalize().as_bytes()))
}

fn is_executable_file(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    path.is_file()
}

fn find_bin(paths: impl IntoIterator<Item = PathBuf>, name: &str) -> Option<PathBuf> {
    paths.into_iter().find_map(|p| {
        let candidate = p.join(name);
        if is_executable_file(&candidate) {
            return Some(candidate);
        }
        #[cfg(windows)]
        {
            let exe = p.join(format!("{name}.exe"));
            if is_executable_file(&exe) {
                return Some(exe);
            }
        }
        None
    })
}

/// Run the target: exec on unix (only returns on failure), spawn+wait on
/// Windows (returns the child's exit code).
fn run_child(mut cmd: Command) -> Result<i32, String> {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        Err(format!("exec failed: {}", cmd.exec()))
    }
    #[cfg(not(unix))]
    {
        let status = cmd.status().map_err(|e| format!("failed to spawn: {e}"))?;
        Ok(status.code().unwrap_or(1))
    }
}

fn try_fast(args: &[OsString]) -> Result<i32, String> {
    let mode = detect_mode(args).ok_or("not eligible")?;
    if std::env::var_os("__MISE_DIFF").is_some() {
        return Err("__MISE_DIFF set (activated shell); no EnvDiff reversal in prototype".into());
    }
    if matches!(mode, Mode::Shim(_)) && std::env::var_os("__MISE_SHIM").is_some() {
        return Err("nested shim".into());
    }
    let key = compute_key(&mode).ok_or("no key")?;
    let encrypted =
        std::fs::read(state_dir().join("exec-cache").join(&key)).map_err(|_| "no record")?;
    let enc_key = get_key().ok_or("no encryption key")?;
    let cipher = ChaCha20Poly1305::new_from_slice(&enc_key).map_err(|e| e.to_string())?;
    if encrypted.len() < 12 {
        return Err("record too short".into());
    }
    let decrypted = cipher
        .decrypt(Nonce::from_slice(&encrypted[..12]), &encrypted[12..])
        .map_err(|e| format!("decrypt: {e}"))?;
    let record: ExecCacheRecord =
        rmp_serde::from_slice(&decrypted).map_err(|e| format!("decode: {e}"))?;

    if record.version != MISE_VERSION {
        return Err("version mismatch".into());
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    if now > record.expires_at {
        return Err("expired".into());
    }
    for (path, mtime) in record.files.iter().chain(record.dirs.iter()) {
        if mtime_millis(path) != *mtime {
            return Err(format!("mtime changed: {}", path.display()));
        }
    }
    for dir in &record.tool_paths {
        if !dir.is_dir() {
            return Err("tool path missing".into());
        }
    }

    // the overlay applied onto the inherited env, mirroring mise's
    // exec_program set_var loop
    let mut overlay: BTreeMap<String, String> = record.env.into_iter().collect();
    let (bin, bin_args) = match &mode {
        Mode::Shim(name) => {
            // only serve bins provided by the toolset's paths; auto-install
            // and registry fallback belong to the real mise
            let bin =
                find_bin(record.tool_paths.iter().cloned(), name).ok_or("bin not in tool paths")?;
            overlay.insert("__MISE_SHIM".into(), "1".into());
            (bin, &args[1..])
        }
        Mode::Exec => {
            let name = args[3].to_str().ok_or("non-UTF-8 command name")?;
            let bin = if name.contains('/') || name.contains(std::path::MAIN_SEPARATOR) {
                PathBuf::from(name)
            } else {
                // like exec_program's lookup reorder: mise-added paths first
                let path = overlay.get("PATH").cloned().unwrap_or_default();
                find_bin(
                    record
                        .tool_paths
                        .iter()
                        .cloned()
                        .chain(std::env::split_paths(&path)),
                    name,
                )
                .ok_or("bin not on cached PATH")?
            };
            (bin, &args[4..])
        }
    };

    let mut cmd = Command::new(&bin);
    cmd.args(bin_args).envs(&overlay);
    run_child(cmd)
}

fn fallback(args: &[OsString]) -> Result<i32, String> {
    // Exec the real mise. For shim mode, preserve argv0 so mise runs its own
    // shim dispatch — and writes the record this binary hits next time.
    let fallback = std::env::var("MISE_EXEC_FALLBACK_BIN").unwrap_or_else(|_| "mise".into());
    let mut cmd = Command::new(&fallback);
    match detect_mode(args) {
        Some(Mode::Shim(name)) => {
            #[cfg(unix)]
            {
                use std::os::unix::process::CommandExt;
                cmd.arg0(&args[0]);
                let _ = &name;
            }
            #[cfg(not(unix))]
            cmd.arg("x").arg("--").arg(&name);
            cmd.args(&args[1..]);
        }
        _ => {
            cmd.args(&args[1..]);
        }
    }
    run_child(cmd).map_err(|e| format!("mise-exec: failed to run {fallback}: {e}"))
}

fn main() -> std::process::ExitCode {
    let args: Vec<OsString> = std::env::args_os().collect();
    let code = match try_fast(&args) {
        Ok(code) => code,
        Err(reason) => {
            if std::env::var_os("MISE_EXEC_CACHE_DEBUG").is_some() {
                eprintln!("[mise-exec] fallback: {reason}");
            }
            match fallback(&args) {
                Ok(code) => code,
                Err(err) => {
                    eprintln!("{err}");
                    1
                }
            }
        }
    };
    std::process::ExitCode::from(code.clamp(0, 255) as u8)
}
