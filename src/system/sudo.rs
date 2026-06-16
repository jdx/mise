//! The single place in mise that may elevate privileges.
//!
//! System package managers and declarative system mutations that require
//! root use this. Every elevated command logs its full argv before running,
//! never prompts for a password without a TTY, and can be disabled entirely
//! with `system_packages.sudo = false`.

use std::process::{Command, Stdio};

use eyre::bail;

use crate::cmd::CmdLineRunner;
use crate::config::Settings;
use crate::result::Result;

pub(crate) fn is_root() -> bool {
    #[cfg(unix)]
    {
        nix::unistd::geteuid().is_root()
    }
    #[cfg(not(unix))]
    {
        false
    }
}

/// The argv that [`run`] would execute, including the `sudo` prefix when
/// elevation would be used. For logging and `--dry-run`.
pub(crate) fn argv(program: &str, args: &[String]) -> Vec<String> {
    argv_with_env(program, args, &[])
}

pub(crate) fn argv_with_env(
    program: &str,
    args: &[String],
    envs: &[(String, String)],
) -> Vec<String> {
    let mut argv = vec![];
    if !is_root() && Settings::get().system_packages.sudo {
        argv.push("sudo".to_string());
        // sudo resets the environment by default; pass env vars through
        // `env` so they reach the elevated command
        if !envs.is_empty() {
            argv.push("env".to_string());
            argv.extend(envs.iter().map(|(k, v)| format!("{k}={v}")));
        }
    }
    argv.push(program.to_string());
    argv.extend(args.iter().cloned());
    argv
}

/// Run `program args...`, elevating with sudo when not running as root.
///
/// - root: runs the command directly (containers/CI)
/// - interactive TTY: runs `sudo program args...` with inherited stdio so
///   sudo can prompt for a password
/// - non-interactive: only proceeds if sudo works without a password
///   (`sudo -n`); otherwise errors with the exact command to run manually
/// - `system_packages.sudo = false`: never elevates; errors if not root
pub(crate) fn run(program: &str, args: &[String], envs: &[(String, String)]) -> Result<()> {
    let argv = argv_with_env(program, args, envs);
    // the copy-pasteable fallback must include the env vars the automated
    // path would have set (e.g. DEBIAN_FRONTEND=noninteractive)
    let mut manual = vec!["sudo".to_string()];
    if !envs.is_empty() {
        manual.push("env".to_string());
        manual.extend(envs.iter().map(|(k, v)| format!("{k}={v}")));
    }
    manual.push(program.to_string());
    manual.extend(args.iter().cloned());
    let manual_cmd = manual.join(" ");
    if !is_root() {
        if !Settings::get().system_packages.sudo {
            bail!(
                "not running as root and system_packages.sudo is disabled. Run manually:\n  {manual_cmd}"
            );
        }
        if crate::file::which("sudo").is_none() {
            bail!(
                "sudo not found. Run as root:\n  {}",
                manual_cmd.trim_start_matches("sudo ")
            );
        }
        if !console::user_attended_stderr() {
            // no TTY to type a password into — only proceed if sudo is
            // passwordless (NOPASSWD/cached credentials), never hang
            let ok = Command::new("sudo")
                .args(["-n", "true"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
            if !ok {
                bail!(
                    "sudo requires a password but no TTY is available. Run manually:\n  {manual_cmd}"
                );
            }
        }
    }
    info!("$ {}", argv.join(" "));
    let mut cmd = CmdLineRunner::new(&argv[0]);
    for arg in &argv[1..] {
        cmd = cmd.arg(arg);
    }
    for (k, v) in envs {
        cmd = cmd.env(k, v);
    }
    // inherited stdio: sudo password prompts and apt progress go straight to
    // the user's terminal
    cmd.raw(true).execute()
}
