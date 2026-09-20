//! Running a Windows user service that carries an environment.
//!
//! Task Scheduler's task XML has no environment block, so a service that
//! sets `environment` cannot name its program directly: something has to
//! apply the environment first. That something is mise. The task's action
//! runs `mise bootstrap __service-exec <name>`, which reads the launch
//! `apply` stored beside the definition, starts the service with that
//! environment, and waits for it.
//!
//! Waiting is the point. Task Scheduler tracks a task by the process it
//! started, so a launcher that applied the environment and exited would
//! take `MultipleInstancesPolicy`, `/end`, and restart-on-failure with it —
//! the task would read as finished while the service was still running.
//! `cmd.exe /c set … && …`, which this replaces, waited for the same
//! reason, and held the console Task Scheduler allocated for as long as it
//! did. This gives that console up before starting the service, and starts
//! the service without one, so neither process puts a window on the
//! desktop.

use eyre::{Result, bail};

// only the Windows action reads a launch back; elsewhere the tests do
#[cfg(any(windows, test))]
use crate::system::scheduled_tasks::{ServiceLaunch, launch_path};

/// Run the service registered under `name`, whose launch must hash to
/// `digest`. Returns once it exits, with its exit code.
#[cfg(windows)]
pub(crate) fn run(name: &str, digest: &str) -> Result<i32> {
    let launch = read_launch(name, digest)?;
    // The console Task Scheduler allocated for this process alone, which
    // the service is about to be started without. A `__service-exec` run by
    // hand in a terminal shares that terminal's console and keeps it.
    crate::windows_console::detach_if_unattended();
    let mut child = spawn(&launch)?;
    // The service dies with this process, however this process dies: a
    // `/end` terminates the launcher without running any code here, and an
    // orphaned service would go on holding whatever the restarted one needs.
    let _confined = confine(&child);
    // Windows always has one; anything but zero is the failure
    // `RestartOnFailure` acts on.
    Ok(child.wait()?.code().unwrap_or(1))
}

/// Only Task Scheduler registers this action: systemd and launchd both set
/// a service's environment themselves, so nothing has to carry it for them.
#[cfg(not(windows))]
pub(crate) fn run(_name: &str, _digest: &str) -> Result<i32> {
    bail!("user services run through mise only on windows")
}

/// The launch `apply` stored, refused unless it is the one the registered
/// action names. The action carries the digest, so an edited launch — or
/// one left over from a definition that was replaced — does not run under a
/// task that was registered for something else.
#[cfg(any(windows, test))]
fn read_launch(name: &str, digest: &str) -> Result<ServiceLaunch> {
    let path = launch_path(name);
    let Ok(stored) = std::fs::read_to_string(&path) else {
        bail!(
            "user service '{name}' has no stored launch at {}; run `mise bootstrap services apply`",
            crate::file::display_path(&path)
        );
    };
    if crate::hash::hash_blake3_to_str(&stored) != digest {
        bail!(
            "user service '{name}' is registered for a different environment than {} holds; run `mise bootstrap services apply`",
            crate::file::display_path(&path)
        );
    }
    Ok(serde_json::from_str(&stored)?)
}

#[cfg(windows)]
fn spawn(launch: &ServiceLaunch) -> Result<std::process::Child> {
    use std::os::windows::process::CommandExt;
    // CREATE_NO_WINDOW: a console program with no console window, which is
    // what a service wants. Without it Windows would give this child a
    // console of its own, since the launcher just gave up the one it had.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let mut cmd = std::process::Command::new(&launch.program);
    if !launch.args.is_empty() {
        // Verbatim, because this is already a command line: Task Scheduler
        // handed the declared one to the program unchanged, and quoting it
        // again here would change where the program sees its arguments end.
        cmd.raw_arg(&launch.args);
    }
    cmd.envs(&launch.environment)
        .creation_flags(CREATE_NO_WINDOW);
    Ok(cmd.spawn()?)
}

/// A job object the service is put in, killed when its last handle closes.
/// Holding the handle for the launcher's lifetime is what ties the two
/// together: whether the launcher returns, panics, or is terminated by
/// `schtasks /end`, the handle closes with it and takes the service along.
#[cfg(windows)]
struct Job(windows_sys::Win32::Foundation::HANDLE);

#[cfg(windows)]
impl Drop for Job {
    fn drop(&mut self) {
        // SAFETY: the handle came from `CreateJobObjectW` and is closed once.
        unsafe { windows_sys::Win32::Foundation::CloseHandle(self.0) };
    }
}

#[cfg(windows)]
fn confine(child: &std::process::Child) -> Option<Job> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject,
    };

    // SAFETY: an unnamed job object with default security.
    let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
    if job.is_null() {
        return None;
    }
    let job = Job(job);
    let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    // SAFETY: `limits` is a live value of the type the information class names.
    let set = unsafe {
        SetInformationJobObject(
            job.0,
            JobObjectExtendedLimitInformation,
            std::ptr::from_ref(&limits).cast(),
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
    };
    if set == 0 {
        return None;
    }
    // SAFETY: the child is alive — it has not been waited on — so its handle
    // is valid for the length of this call.
    if unsafe { AssignProcessToJobObject(job.0, child.as_raw_handle()) } == 0 {
        return None;
    }
    Some(job)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_launch_must_be_the_one_the_action_names() {
        let name = "service-exec-digest";
        let path = launch_path(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let launch = ServiceLaunch {
            program: "agent.exe".to_string(),
            args: "--serve".to_string(),
            environment: [("RUST_LOG".to_string(), "info".to_string())]
                .into_iter()
                .collect(),
        };
        let stored = serde_json::to_string(&launch).unwrap();
        std::fs::write(&path, &stored).unwrap();

        let digest = crate::hash::hash_blake3_to_str(&stored);
        assert_eq!(read_launch(name, &digest).unwrap(), launch);

        // what an edit behind mise's back looks like: the action still names
        // the environment the task was registered with, and this is not it
        let err = read_launch(name, "0").unwrap_err().to_string();
        assert!(
            err.contains("registered for a different environment"),
            "{err}"
        );

        std::fs::remove_file(&path).unwrap();
        let err = read_launch(name, &digest).unwrap_err().to_string();
        assert!(err.contains("no stored launch"), "{err}");
    }
}
