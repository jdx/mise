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

use std::path::Path;

// only the Windows action reads a launch back; elsewhere the tests do
#[cfg(any(windows, test))]
use crate::system::scheduled_tasks::ServiceLaunch;

/// Run the service registered under `name`, whose launch must hash to
/// `digest`. Returns once it exits, with its exit code.
#[cfg(windows)]
pub(crate) fn run(name: &str, launch: &Path, digest: &str) -> Result<i32> {
    let launch = read_launch(name, launch, digest)?;
    // The console Task Scheduler allocated for this process alone was
    // hidden right after the parser recognized this command
    // (`Commands::runs_unattended`), ahead of settings, config, and any
    // automatic update — the window is closable for as long as it is up,
    // and closing it would kill the launcher before the service started.
    use std::os::windows::process::CommandExt;
    let mut cmd = std::process::Command::new(&launch.program);
    if !launch.args.is_empty() {
        // Verbatim, because this is already a command line: Task Scheduler
        // handed the declared one to the program unchanged, and quoting it
        // again here would change where the program sees its arguments end.
        cmd.raw_arg(&launch.args);
    }
    cmd.envs(&launch.environment);

    // The service and everything it starts must die with this process,
    // however this process dies: `schtasks /end` terminates the launcher
    // without running any code here, and an orphan would go on holding what
    // the restarted service needs — the watcher's `git` children hold the
    // shadow repository. `windows_job` starts the child suspended so no
    // descendant can slip out before the job takes it, and a failure to
    // confine is a failure to run: an unconfined service is exactly the one
    // this is meant to prevent.
    //
    // No creation flags of its own: the service inherits the console hidden
    // above, so it stays hidden and so does everything it starts. Asking for
    // `CREATE_NO_WINDOW` would give the service a console of its own, and a
    // program started by a process whose console it does not share is handed
    // a fresh one — which is how each `git` the watcher runs would get a
    // window, the very problem hiding rather than detaching avoids.
    let (mut child, job) = crate::windows_job::spawn(&mut cmd, 0)?;
    let code = match child.wait() {
        // Windows always has an exit code; anything but zero is the failure
        // `RestartOnFailure` acts on.
        Ok(status) => status.code().unwrap_or(1),
        Err(err) => {
            job.kill();
            return Err(err.into());
        }
    };
    drop(job);
    Ok(code)
}

/// Only Task Scheduler registers this action: systemd and launchd both set
/// a service's environment themselves, so nothing has to carry it for them.
#[cfg(not(windows))]
pub(crate) fn run(_name: &str, _launch: &Path, _digest: &str) -> Result<i32> {
    bail!("user services run through mise only on windows")
}

/// The launch `apply` stored, refused unless it is the one the registered
/// action names. The action carries the digest, so an edited launch — or
/// one left over from a definition that was replaced — does not run under a
/// task that was registered for something else.
#[cfg(any(windows, test))]
fn read_launch(name: &str, path: &Path, digest: &str) -> Result<ServiceLaunch> {
    let Ok(stored) = std::fs::read_to_string(path) else {
        bail!(
            "user service '{name}' has no stored launch at {}; run `mise bootstrap services apply`",
            crate::file::display_path(path)
        );
    };
    if crate::hash::hash_blake3_to_str(&stored) != digest {
        bail!(
            "user service '{name}' is registered for a different environment than {} holds; run `mise bootstrap services apply`",
            crate::file::display_path(path)
        );
    }
    Ok(serde_json::from_str(&stored)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_launch_must_be_the_one_the_action_names() {
        let name = "service-exec-digest";
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mise-history.launch.json");
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
        assert_eq!(read_launch(name, &path, &digest).unwrap(), launch);

        // what an edit behind mise's back looks like: the action still names
        // the environment the task was registered with, and this is not it
        let err = read_launch(name, &path, "0").unwrap_err().to_string();
        assert!(
            err.contains("registered for a different environment"),
            "{err}"
        );

        std::fs::remove_file(&path).unwrap();
        let err = read_launch(name, &path, &digest).unwrap_err().to_string();
        assert!(err.contains("no stored launch"), "{err}");
    }
}
