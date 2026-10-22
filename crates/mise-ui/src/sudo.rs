//! The single place in mise that may elevate privileges.
//!
//! System package managers and declarative system mutations that require
//! root use this. Every elevated command logs its full argv before running,
//! never prompts for a password without a TTY, suspends the progress display
//! while it owns the terminal, and can be disabled entirely with
//! `system_packages.sudo = false`.

use std::io::Write;
use std::process::{Command, Output, Stdio};

use eyre::bail;

use crate::multi_progress_report::{MultiProgressReport, ProgressPauseGuard};
use eyre::Result;
use mise_settings::Settings;
use mise_util::cmd::CmdLineRunner;

pub fn is_root() -> bool {
    #[cfg(unix)]
    {
        nix::unistd::geteuid().is_root()
    }
    #[cfg(not(unix))]
    {
        false
    }
}

/// The command used to elevate: `system_packages.elevate_with` when set,
/// otherwise the first of `sudo`, `run0` and `doas` on `PATH`. Falls back to
/// `sudo` when none is installed so errors name the conventional tool.
pub fn elevator() -> String {
    if let Some(cmd) = Settings::get()
        .system_packages
        .elevate_with
        .as_deref()
        .map(str::trim)
        .filter(|cmd| !cmd.is_empty())
    {
        return cmd.to_string();
    }
    ["sudo", "run0", "doas"]
        .into_iter()
        .find(|cmd| mise_util::file::which(cmd).is_some())
        .unwrap_or("sudo")
        .to_string()
}

/// The flag that makes doas or run0 fail instead of prompting. mise adds it to
/// the real command rather than probing with `true`, because a rule may allow
/// the requested command without allowing anything else.
fn noninteractive_flag(elevator: &str) -> Option<&'static str> {
    match std::path::Path::new(elevator).file_name()?.to_str()? {
        "doas" => Some("-n"),
        "run0" => Some("--no-ask-password"),
        _ => None,
    }
}

fn is_sudo(elevator: &str) -> bool {
    std::path::Path::new(elevator)
        .file_name()
        .is_some_and(|name| name == "sudo")
}

/// The argv that [`run`] would execute, including the `sudo` prefix when
/// elevation would be used. For logging and `--dry-run`.
pub fn argv(program: &str, args: &[String]) -> Vec<String> {
    argv_with_env(program, args, &[])
}

pub fn argv_with_env(program: &str, args: &[String], envs: &[(String, String)]) -> Vec<String> {
    if !is_root() && Settings::get().system_packages.sudo {
        build_argv(program, args, envs, !console::user_attended_stderr())
    } else {
        std::iter::once(program.to_string())
            .chain(args.iter().cloned())
            .collect()
    }
}

/// Quote words for a shell, leaving `NAME=value` assignments and `--opt=value`
/// options readable by quoting only the value.
fn join_quoted<S: AsRef<str>>(words: &[S]) -> String {
    words
        .iter()
        .map(|word| {
            let word = word.as_ref();
            match word.split_once('=') {
                Some((name, value))
                    if !name.is_empty()
                        && name
                            .chars()
                            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-')) =>
                {
                    format!("{name}={}", shell_words::quote(value))
                }
                _ => shell_words::quote(word).into_owned(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The command to give a user who must run it themselves: always elevated, and
/// without the no-prompt flag so a password can be typed.
fn manual_command(program: &str, args: &[String], envs: &[(String, String)]) -> String {
    join_quoted(&build_argv(program, args, envs, false))
}

fn build_argv(
    program: &str,
    args: &[String],
    envs: &[(String, String)],
    noninteractive: bool,
) -> Vec<String> {
    let elevator = elevator();
    let flag = noninteractive_flag(&elevator);
    let is_run0 = flag == Some("--no-ask-password");
    let mut argv = vec![elevator];
    argv.extend(flag.filter(|_| noninteractive).map(str::to_string));
    if is_run0 {
        // run0 passes env vars itself, so the authorized command stays the
        // program rather than `env`
        argv.extend(envs.iter().map(|(k, v)| format!("--setenv={k}={v}")));
    } else if !envs.is_empty() {
        // sudo and doas reset the environment by default; pass env vars
        // through `env` so they reach the elevated command
        argv.push("env".to_string());
        argv.extend(envs.iter().map(|(k, v)| format!("{k}={v}")));
    }
    argv.push(program.to_string());
    argv.extend(args.iter().cloned());
    argv
}

/// Elevation mode for helper subprocesses (e.g. the brew cask shim) that may
/// need sudo themselves. Mirrors [`run`]'s policy without running anything:
/// - `"interactive"`: sudo may prompt on the controlling TTY (also returned
///   when already root — the subprocess won't elevate at euid 0 anyway)
/// - `"noninteractive"`: no TTY; the subprocess must use `sudo -n` so it
///   fails instead of hanging on a password prompt
/// - `"deny"`: elevation is disabled or sudo is unavailable
#[cfg(unix)]
pub fn subprocess_mode() -> &'static str {
    if is_root() {
        "interactive"
    } else if !Settings::get().system_packages.sudo || mise_util::file::which("sudo").is_none() {
        // the brew cask shim only knows how to call sudo
        "deny"
    } else if console::user_attended_stderr() {
        "interactive"
    } else {
        "noninteractive"
    }
}

/// Suspend the animated progress display for as long as the returned guard is
/// held, because the child about to be spawned owns the terminal.
///
/// Every elevated helper here inherits stdio so that sudo's `Password:` prompt
/// reaches the user. The progress renderer repaints on its own interval,
/// though, so without this it overwrites the prompt on the next frame and the
/// command reads as hung — stdin is connected and typing the password blind
/// works, but nothing on screen says mise is waiting.
///
/// Unconditional, and held for the child's whole lifetime rather than just the
/// prompt. Whether sudo will prompt is not the deciding question: the child has
/// the terminal either way, so its own output would corrupt the display too.
/// This is what `--raw` already achieves by disabling the renderer outright.
/// [`MultiProgressReport::pause_progress`] is reference-counted and already
/// no-ops when no renderer is animating, so there is nothing to pre-filter.
fn pause_progress_for_child() -> Option<ProgressPauseGuard> {
    MultiProgressReport::try_get().map(|report| report.pause_progress())
}

/// Run `program args...`, elevating with sudo when not running as root.
///
/// - root: runs the command directly (containers/CI)
/// - interactive TTY: runs `sudo program args...` with inherited stdio so
///   sudo can prompt for a password
/// - non-interactive: only proceeds if sudo works without a password
///   (`sudo -n`); otherwise errors with the exact command to run manually
/// - `system_packages.sudo = false`: never elevates; errors if not root
pub fn run(program: &str, args: &[String], envs: &[(String, String)]) -> Result<()> {
    let argv = argv_with_env(program, args, envs);
    // the copy-pasteable fallback must include the env vars the automated
    // path would have set (e.g. DEBIAN_FRONTEND=noninteractive)
    let manual_cmd = manual_command(program, args, envs);
    ensure_elevation_available(&manual_cmd)?;
    attempt(&manual_cmd, || {
        let _progress_pause = pause_progress_for_child();
        log::info!("$ {}", argv.join(" "));
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
    })
}

/// Run `program args...` elevated, with the child's working directory bound to
/// `dir` via `fchdir` so relative arguments resolve from that exact directory
/// inode.
///
/// Elevation policy is identical to [`run`]. The difference is that callers can
/// pass *relative* names which cannot be redirected: a pathname handed to an
/// elevated recursive operation would be re-resolved by the child, so a
/// same-uid replacement of a component between validation and execution could
/// point it somewhere else. Binding the child's cwd to a descriptor the caller
/// already verified removes that window.
///
/// stdio is inherited so a sudo password prompt still reaches the terminal.
#[cfg(unix)]
pub fn run_in_dir<Fd: std::os::fd::AsFd>(program: &str, args: &[String], dir: Fd) -> Result<()> {
    use std::os::fd::AsRawFd;
    use std::os::unix::process::CommandExt;

    use eyre::WrapErr;

    let argv = argv_with_env(program, args, &[]);
    let manual_cmd = manual_command(program, args, &[]);
    ensure_elevation_available(&manual_cmd)?;
    attempt(&manual_cmd, || {
        let _progress_pause = pause_progress_for_child();
        log::info!("$ {}", argv.join(" "));
        let raw = dir.as_fd().as_raw_fd();
        let mut cmd = Command::new(&argv[0]);
        cmd.args(&argv[1..]);
        // SAFETY: `fchdir` is async-signal-safe and only alters the child's working
        // directory. `raw` stays open in the parent across the spawn, and CLOEXEC
        // (if set) only takes effect at exec, after pre_exec has run.
        unsafe {
            cmd.pre_exec(move || {
                if nix::libc::fchdir(raw) == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let status = cmd
            .status()
            .wrap_err_with(|| format!("failed to run {}", argv.join(" ")))?;
        if !status.success() {
            bail!("{} failed", argv.join(" "));
        }
        Ok(())
    })
}

/// Run an elevated command and capture its output.
///
/// Interactive callers authenticate with an inherited `sudo -v` first so a
/// password prompt is never hidden inside captured stderr. Non-interactive
/// callers retain [`ensure_elevation_available`]'s fail-fast `sudo -n` check.
pub fn output(program: &str, args: &[String], envs: &[(String, String)]) -> Result<Output> {
    let argv = argv_with_env(program, args, envs);
    let manual_cmd = manual_command(program, args, envs);
    ensure_elevation_available(&manual_cmd)?;
    let interactive =
        !is_root() && Settings::get().system_packages.sudo && console::user_attended_stderr();
    if interactive && is_sudo(&argv[0]) {
        let _progress_pause = pause_progress_for_child();
        CmdLineRunner::new(&argv[0]).arg("-v").raw(true).execute()?;
    }
    // doas and run0 have no `-v`; they prompt on the terminal while the command
    // runs, so keep the progress display off it and give them stdin
    let _progress_pause = (interactive && !is_sudo(&argv[0]))
        .then(pause_progress_for_child)
        .flatten();
    log::info!("$ {}", argv.join(" "));
    let mut cmd = Command::new(&argv[0]);
    if interactive && !is_sudo(&argv[0]) {
        cmd.stdin(Stdio::inherit());
    }
    let mut output = cmd
        .args(&argv[1..])
        .envs(envs.iter().map(|(key, value)| (key, value)))
        .output()?;
    if !output.status.success()
        && let Some(hint) = manual_hint(&manual_cmd)
    {
        // callers report the captured stderr, so put the retry hint there
        output
            .stderr
            .extend_from_slice(format!("\n{hint}").as_bytes());
    }
    Ok(output)
}

/// Run one elevated helper with its private payload on stdin.
///
/// Unlike [`run`], the payload is never included in argv, logs, or the manual
/// fallback. This is the transport used for typed privileged bootstrap plans.
pub fn run_with_input(program: &str, args: &[String], input: &[u8]) -> Result<()> {
    run_with_reader(program, args, input)
}

/// Stream a prepared payload without keeping tool archives in memory.
pub fn run_with_reader(
    program: &str,
    args: &[String],
    mut input: impl std::io::Read,
) -> Result<()> {
    let argv = argv(program, args);
    ensure_stdin_elevation_available()?;
    let _progress_pause = pause_progress_for_child();
    log::info!("$ {}", argv.join(" "));
    let mut child = Command::new(&argv[0])
        .args(&argv[1..])
        .stdin(Stdio::piped())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()?;
    let mut stdin = child.stdin.take().expect("piped stdin is available");
    // The helper may exit (and close its stdin) before the payload is fully
    // written; reap it and report its status ahead of the resulting copy error.
    let copied = std::io::copy(&mut input, &mut stdin);
    drop(stdin);
    let status = child.wait()?;
    if !status.success() {
        bail!(
            "elevated helper `{program} {}` failed with {status}",
            args.join(" ")
        );
    }
    copied?;
    Ok(())
}

/// Run one elevated helper with a private stdin payload and capture stdout.
/// Stderr remains attached to the terminal for sudo prompts and diagnostics.
pub fn run_with_input_output(program: &str, args: &[String], input: &[u8]) -> Result<Vec<u8>> {
    let argv = argv(program, args);
    ensure_stdin_elevation_available()?;
    let _progress_pause = pause_progress_for_child();
    log::info!("$ {}", argv.join(" "));
    let mut child = Command::new(&argv[0])
        .args(&argv[1..])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()?;
    child
        .stdin
        .take()
        .expect("piped stdin is available")
        .write_all(input)?;
    let output = child.wait_with_output()?;
    if !output.status.success() {
        bail!("elevated bootstrap helper failed with {}", output.status);
    }
    Ok(output.stdout)
}

fn attempt<T>(manual_cmd: &str, f: impl FnOnce() -> Result<T>) -> Result<T> {
    f().map_err(|err| with_manual_hint(err, manual_cmd))
}

/// doas and run0 get a no-prompt flag on the real command when mise has no TTY,
/// so a password requirement surfaces as the command failing. Say how to retry.
fn manual_hint(manual_cmd: &str) -> Option<String> {
    let elevator = elevator();
    if is_root() || console::user_attended_stderr() || noninteractive_flag(&elevator).is_none() {
        return None;
    }
    Some(format!(
        "if {elevator} needs a password, run manually:\n  {manual_cmd}"
    ))
}

fn with_manual_hint(err: eyre::Report, manual_cmd: &str) -> eyre::Report {
    match manual_hint(manual_cmd) {
        Some(hint) => err.wrap_err(hint),
        None => err,
    }
}

/// `manual_cmd` without its elevator, for running as root. run0's `--setenv`
/// options become the `env` prefix a root shell understands.
fn root_fallback(manual_cmd: &str, elevator: &str) -> String {
    // keep the command untouched if it does not parse as shell words
    let Ok(mut words) = shell_words::split(manual_cmd) else {
        return manual_cmd.to_string();
    };
    if words.first().is_some_and(|word| word == elevator) {
        words.remove(0);
    }
    let split = words
        .iter()
        .position(|w| !w.starts_with("--setenv="))
        .unwrap_or(words.len());
    let (envs, tail) = words.split_at(split);
    let tail = join_quoted(tail);
    if envs.is_empty() {
        tail
    } else {
        let envs = envs.iter().map(|w| w.trim_start_matches("--setenv="));
        format!("env {} {tail}", join_quoted(&envs.collect::<Vec<_>>()))
    }
}

/// Verify that the elevation command mise is configured to use can run
/// without violating mise's elevation policy or hanging without a TTY.
pub fn ensure_elevation_available(manual_cmd: &str) -> Result<()> {
    let elevator = elevator();
    check_elevation(
        &elevator,
        &format!("Run manually:\n  {manual_cmd}"),
        &format!("Run as root:\n  {}", root_fallback(manual_cmd, &elevator)),
    )
}

/// Like [`ensure_elevation_available`] for a subprocess that calls `sudo`
/// itself and never sees `system_packages.elevate_with`, such as an AUR
/// helper. Only `sudo` is checked, because only `sudo` will be used.
pub fn ensure_sudo_available(manual_cmd: &str) -> Result<()> {
    check_elevation(
        "sudo",
        &format!("Run manually:\n  {manual_cmd}"),
        &format!("Run as root:\n  {manual_cmd}"),
    )
}

/// Like [`ensure_elevation_available`] for helpers that read their payload
/// from stdin. The helper cannot be run on its own, so the hint is to rerun
/// the mise command that needs it as root instead of printing its argv.
fn ensure_stdin_elevation_available() -> Result<()> {
    const HINT: &str = "Re-run this mise command as root. The privileged helper \
        receives its plan on stdin, so it cannot be run by itself.";
    check_elevation(&elevator(), HINT, HINT)
}

fn check_elevation(elevator: &str, manual_hint: &str, root_hint: &str) -> Result<()> {
    if is_root() {
        return Ok(());
    }
    if !Settings::get().system_packages.sudo {
        bail!("not running as root and system_packages.sudo is disabled. {manual_hint}");
    }
    if mise_util::file::which(elevator).is_none() {
        bail!("{elevator} not found. {root_hint}");
    }
    if !console::user_attended_stderr() {
        let ok = if is_sudo(elevator) {
            Command::new(elevator)
                .args(["-n", "true"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map(|status| status.success())
                .unwrap_or(false)
        } else {
            // doas and run0 get a no-prompt flag on the real command instead
            noninteractive_flag(elevator).is_some()
        };
        if !ok {
            bail!("{elevator} requires a password but no TTY is available. {manual_hint}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_fallback_turns_run0_setenv_into_env() {
        assert_eq!(
            root_fallback(
                "run0 --setenv=A=1 --setenv=B=2 apt-get -y install x",
                "run0"
            ),
            "env A=1 B=2 apt-get -y install x"
        );
        assert_eq!(
            root_fallback("sudo env A=1 apt-get update", "sudo"),
            "env A=1 apt-get update"
        );
        assert_eq!(
            root_fallback("mise install --system", "sudo"),
            "mise install --system"
        );
    }

    #[test]
    fn noninteractive_flag_knows_each_elevator() {
        assert_eq!(noninteractive_flag("/usr/bin/doas"), Some("-n"));
        assert_eq!(noninteractive_flag("run0"), Some("--no-ask-password"));
        assert_eq!(noninteractive_flag("sudo"), None);
        assert_eq!(noninteractive_flag("pkexec"), None);
        assert!(is_sudo("/usr/bin/sudo"));
        assert!(!is_sudo("run0"));
    }

    /// Each elevated helper binds the guard for the rest of its body, so the
    /// renderer stays suspended until the child exits rather than only while
    /// the guard is constructed. Nested acquisitions — a `with_sudo_fallback`
    /// retry inside a flight step — must not resume it early either.
    #[test]
    fn pause_progress_for_child_suspends_until_every_guard_drops() {
        let report = MultiProgressReport::get();
        let baseline = report.progress_suspension_depth();

        let outer = pause_progress_for_child();
        assert!(outer.is_some(), "a live report must hand back a guard");
        assert_eq!(report.progress_suspension_depth(), baseline + 1);

        let inner = pause_progress_for_child();
        assert_eq!(report.progress_suspension_depth(), baseline + 2);

        drop(inner);
        assert_eq!(
            report.progress_suspension_depth(),
            baseline + 1,
            "the outer child still owns the terminal"
        );

        drop(outer);
        assert_eq!(report.progress_suspension_depth(), baseline);
    }
}
