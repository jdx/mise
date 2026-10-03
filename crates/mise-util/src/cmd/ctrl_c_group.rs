//! Ctrl+C for one timed-out command on Windows, which has no SIGTERM. A process in
//! a new process group ignores Ctrl+C, so mise leads the group itself ([`ARG`]),
//! re-enables Ctrl+C, and runs the command, which then gets only its own group's.

use std::ffi::{OsStr, OsString};
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::sync::atomic::{AtomicBool, Ordering};
use windows_sys::Win32::Foundation::{CloseHandle, FALSE, HANDLE, TRUE};
use windows_sys::Win32::System::Console::{
    CTRL_BREAK_EVENT, CTRL_C_EVENT, GenerateConsoleCtrlEvent, SetConsoleCtrlHandler,
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject,
};
use windows_sys::Win32::System::Threading::CREATE_NEW_PROCESS_GROUP;
use windows_sys::core::BOOL;

/// The private argument that makes mise act as the leader of a Ctrl+C group.
pub const ARG: &str = "__ctrl-c-group";

/// Which of the command's arguments were added with `raw_arg`, as a comma
/// separated list of indices. Its presence is also what marks a leader.
const RAW_ENV: &str = "__MISE_CTRL_C_GROUP_RAW";

/// Exit code of a leader that could not start the command at all.
const SPAWN_FAILED: i32 = 127;

/// Set once this leader has received Ctrl+C.
static INTERRUPTED: AtomicBool = AtomicBool::new(false);

/// Run the leader when this process was started as one, returning its exit code.
///
/// Both the argument and the variable are required, so neither a user typing
/// the argument nor a stray variable in the environment starts a leader.
pub fn try_run() -> Option<i32> {
    let (program, args, raw) =
        leader_command(std::env::args_os().skip(1), std::env::var_os(RAW_ENV))?;
    unsafe {
        // Undo the "ignore Ctrl+C" that CREATE_NEW_PROCESS_GROUP gave this
        // process; the command inherits the cleared state.
        SetConsoleCtrlHandler(None, FALSE);
        // Outlive Ctrl+C ourselves, so the command's exit code is what mise sees.
        SetConsoleCtrlHandler(Some(keep_running), TRUE);
    }
    let mut command = rebuild(&program, args, &raw);
    command.env_remove(RAW_ENV);
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(err) => {
            eprintln!(
                "mise: failed to execute command: {}: {err}",
                program.to_string_lossy()
            );
            return Some(SPAWN_FAILED);
        }
    };
    let job = join_job(&child);
    // Every exit code is kept, including 0xC000013A for an interrupted process.
    let code = child.wait().map_or(1, |status| status.code().unwrap_or(1));
    if let Some(job) = job {
        if INTERRUPTED.load(Ordering::Acquire) {
            // A descendant that ignored Ctrl+C would otherwise outlive the command,
            // and once this leader exits nothing could find it by its parent.
            unsafe { TerminateJobObject(job, code as u32) };
        } else {
            // Without Ctrl+C, what the command left running is meant to keep running.
            set_kill_on_close(job, false);
        }
    }
    Some(code)
}

/// A job holding the command and whatever it starts from now on, or None if it
/// cannot join one. A process it started before joining is not in the job.
///
/// The job ends its processes when it closes, which is when this leader exits or
/// is terminated. After a timeout's grace period the leader is terminated, and
/// `taskkill /T` cannot find a descendant whose parent has already exited.
fn join_job(child: &std::process::Child) -> Option<HANDLE> {
    unsafe {
        let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        if job.is_null() {
            return None;
        }
        if !set_kill_on_close(job, true)
            || AssignProcessToJobObject(job, child.as_raw_handle() as HANDLE) == FALSE
        {
            CloseHandle(job);
            return None;
        }
        Some(job)
    }
}

fn set_kill_on_close(job: HANDLE, kill: bool) -> bool {
    unsafe {
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        if kill {
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        }
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &info as *const _ as *const _,
            std::mem::size_of_val(&info) as u32,
        ) != FALSE
    }
}

/// The program, arguments and raw-argument indices a leader was started with,
/// or None when these arguments and `raw_env` do not describe a leader.
fn leader_command(
    mut args: impl Iterator<Item = OsString>,
    raw_env: Option<OsString>,
) -> Option<(OsString, Vec<OsString>, Vec<usize>)> {
    if args.next().as_deref() != Some(OsStr::new(ARG)) {
        return None;
    }
    let raw = decode_raw(&raw_env?.to_string_lossy());
    let program = args.next()?;
    Some((program, args.collect(), raw))
}

unsafe extern "system" fn keep_running(ctrl_type: u32) -> BOOL {
    if ctrl_type == CTRL_C_EVENT || ctrl_type == CTRL_BREAK_EVENT {
        INTERRUPTED.store(true, Ordering::Release);
        TRUE
    } else {
        FALSE
    }
}

/// The command a leader runs: `raw` arguments go through `raw_arg` again, so
/// the command line is built exactly as it would have been without a leader.
fn rebuild(
    program: &OsStr,
    args: impl IntoIterator<Item = OsString>,
    raw: &[usize],
) -> std::process::Command {
    let mut command = std::process::Command::new(program);
    for (i, arg) in args.into_iter().enumerate() {
        if raw.contains(&i) {
            command.raw_arg(arg);
        } else {
            command.arg(arg);
        }
    }
    command
}

/// The running executable, which is what handles [`ARG`]: `MISE_BIN` follows
/// `__MISE_BIN`, which can name another mise.
fn leader_program() -> std::path::PathBuf {
    std::env::current_exe().unwrap_or_else(|_| crate::env::MISE_BIN.clone())
}

/// `command`, run through a leader of a new process group. Every argument is
/// passed as a plain one, which the leader's own argument parsing undoes.
pub(super) fn wrap(
    command: &tokio::process::Command,
    inherit_env: bool,
    raw: &[usize],
) -> tokio::process::Command {
    let original = command.as_std();
    let mut leader = tokio::process::Command::new(leader_program());
    leader
        .arg(ARG)
        .arg(original.get_program())
        .args(original.get_args());
    if let Some(dir) = original.get_current_dir() {
        leader.current_dir(dir);
    }
    copy_environment(original, inherit_env, leader.as_std_mut());
    leader.env(RAW_ENV, encode_raw(raw));
    leader.creation_flags(CREATE_NEW_PROCESS_GROUP);
    leader
}

/// Give `to` the environment `from` would have run with. `Command` cannot say
/// whether it was cleared, so `inherit_env` does.
fn copy_environment(
    from: &std::process::Command,
    inherit_env: bool,
    to: &mut std::process::Command,
) {
    if !inherit_env {
        to.env_clear();
    }
    for (key, value) in from.get_envs() {
        match value {
            Some(value) => to.env(key, value),
            None => to.env_remove(key),
        };
    }
}

/// Raise Ctrl+C in the group led by `pid`. False when it could not be raised,
/// for example because mise has no console.
pub(super) fn interrupt(pid: u32) -> bool {
    unsafe { GenerateConsoleCtrlEvent(CTRL_C_EVENT, pid) != FALSE }
}

fn encode_raw(raw: &[usize]) -> String {
    raw.iter()
        .map(usize::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

fn decode_raw(value: &str) -> Vec<usize> {
    value.split(',').filter_map(|i| i.parse().ok()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_indices_round_trip() {
        for raw in [vec![], vec![0], vec![1, 2, 5]] {
            assert_eq!(decode_raw(&encode_raw(&raw)), raw);
        }
    }

    #[test]
    fn a_leader_needs_both_its_argument_and_its_variable() {
        let args = |list: &[&str]| {
            list.iter()
                .map(OsString::from)
                .collect::<Vec<_>>()
                .into_iter()
        };
        let raw = || Some(OsString::from("1"));
        assert_eq!(leader_command(args(&[ARG, "cmd", "/c"]), None), None);
        assert_eq!(leader_command(args(&["run", "cmd"]), raw()), None);
        assert_eq!(leader_command(args(&[ARG]), raw()), None);
        assert_eq!(
            leader_command(args(&[ARG, "cmd", "/d", "/c x"]), raw()),
            Some((
                OsString::from("cmd"),
                vec![OsString::from("/d"), OsString::from("/c x")],
                vec![1]
            ))
        );
        // The test binary itself was not started as a leader.
        assert_eq!(try_run(), None);
    }

    #[test]
    fn wrapping_passes_the_command_to_the_leader() {
        let mut command = tokio::process::Command::new("cmd");
        command
            .arg("/d")
            .raw_arg("/s /c \"echo \"a b\" & echo x\"")
            .current_dir(std::env::temp_dir())
            .env("KEPT", "1");
        let leader = wrap(&command, true, &[1]);
        let leader = leader.as_std();
        assert_eq!(leader.get_program(), leader_program().as_os_str());
        let args: Vec<_> = leader.get_args().collect();
        assert_eq!(
            args,
            [
                OsStr::new(ARG),
                OsStr::new("cmd"),
                OsStr::new("/d"),
                OsStr::new("/s /c \"echo \"a b\" & echo x\"")
            ]
        );
        assert_eq!(
            leader.get_current_dir(),
            Some(std::env::temp_dir().as_path())
        );
        let envs: Vec<_> = leader.get_envs().collect();
        assert!(envs.contains(&(OsStr::new("KEPT"), Some(OsStr::new("1")))));
        assert!(envs.contains(&(OsStr::new(RAW_ENV), Some(OsStr::new("1")))));
    }

    /// The environment a command sees, as `cmd /c set` prints it, lowercased
    /// because Windows variable names are case-insensitive.
    fn environment_seen(from: &std::process::Command, inherit_env: bool) -> String {
        let mut to = std::process::Command::new("cmd");
        to.args(["/d", "/c", "set"]);
        copy_environment(from, inherit_env, &mut to);
        let output = to.output().unwrap();
        String::from_utf8_lossy(&output.stdout).to_lowercase()
    }

    #[test]
    fn the_leader_gets_the_commands_environment() {
        // Removed variables stay removed when the environment is inherited.
        let mut inherited = std::process::Command::new("cmd");
        inherited.env("KEPT", "1").env_remove("OS");
        let seen = environment_seen(&inherited, true);
        assert!(seen.contains("kept=1"), "{seen}");
        assert!(seen.contains("systemroot="), "{seen}");
        assert!(!seen.contains("os=windows_nt"), "{seen}");

        // A cleared environment passes on only what was set explicitly.
        let mut cleared = std::process::Command::new("cmd");
        cleared.env_clear().env("KEPT", "1");
        let seen = environment_seen(&cleared, false);
        assert!(seen.contains("kept=1"), "{seen}");
        assert!(!seen.contains("systemroot="), "{seen}");
    }

    /// What the leader rebuilds must run exactly like the original command,
    /// including a `raw_arg` that std would otherwise have quoted.
    #[test]
    fn a_rebuilt_command_runs_like_the_original() {
        let body = r#"/s /c "echo "a b" & echo %MISE_CTRL_C_GROUP_TEST% & echo ^&""#;
        let run = |command: &mut std::process::Command| {
            let output = command
                .env("MISE_CTRL_C_GROUP_TEST", "it's 100%")
                .output()
                .unwrap();
            assert!(output.status.success(), "{output:?}");
            String::from_utf8(output.stdout).unwrap()
        };
        let mut original = std::process::Command::new("cmd");
        original.arg("/d").raw_arg(body);
        let args = [OsString::from("/d"), OsString::from(body)];
        let mut rebuilt = rebuild(OsStr::new("cmd"), args, &[1]);
        let expected = run(&mut original);
        assert!(expected.contains("\"a b\""), "{expected}");
        assert_eq!(run(&mut rebuilt), expected);
    }
}
