//! Start suspended so no descendant can escape before assignment to the job.

use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::os::windows::process::CommandExt;
use std::process::{Child, Command};

use eyre::{Result, bail};
use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject,
};
use windows_sys::Win32::System::Threading::{
    CREATE_SUSPENDED, OpenThread, ResumeThread, THREAD_SUSPEND_RESUME,
};

pub(super) struct Job(OwnedHandle);

impl Job {
    fn new() -> Result<Self> {
        // SAFETY: no borrowed pointers or inherited handle are supplied.
        let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if handle.is_null() {
            return Err(std::io::Error::last_os_error().into());
        }
        // SAFETY: this newly created valid handle has exactly one owner.
        let job = Self(unsafe { OwnedHandle::from_raw_handle(handle) });
        let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // SAFETY: the initialized information buffer is valid for its stated size.
        if unsafe {
            SetInformationJobObject(
                job.0.as_raw_handle(),
                JobObjectExtendedLimitInformation,
                (&info as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                std::mem::size_of_val(&info) as u32,
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(job)
    }

    fn start(&self, child: &Child) -> Result<()> {
        // SAFETY: both handles remain owned and alive throughout assignment.
        if unsafe { AssignProcessToJobObject(self.0.as_raw_handle(), child.as_raw_handle()) } == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        // std does not expose the primary thread handle. The suspended child
        // has not executed and cannot have created additional threads yet.
        // SAFETY: no pointer arguments; validate the returned snapshot handle.
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
        if snapshot == INVALID_HANDLE_VALUE {
            return Err(std::io::Error::last_os_error().into());
        }
        // SAFETY: the valid snapshot handle is newly created and uniquely owned.
        let snapshot = unsafe { OwnedHandle::from_raw_handle(snapshot) };
        let mut entry = THREADENTRY32 {
            dwSize: std::mem::size_of::<THREADENTRY32>() as u32,
            ..Default::default()
        };
        // SAFETY: the snapshot is live and entry is initialized and writable.
        let mut found = unsafe { Thread32First(snapshot.as_raw_handle(), &mut entry) } != 0;
        while found {
            if entry.th32OwnerProcessID == child.id() {
                // SAFETY: request only resume access to the child's thread.
                let thread = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID) };
                if thread.is_null() {
                    return Err(std::io::Error::last_os_error().into());
                }
                // SAFETY: the returned handle is valid and uniquely owned.
                let thread = unsafe { OwnedHandle::from_raw_handle(thread) };
                // SAFETY: the child is already assigned to our kill-on-close job.
                if unsafe { ResumeThread(thread.as_raw_handle()) } == u32::MAX {
                    return Err(std::io::Error::last_os_error().into());
                }
                return Ok(());
            }
            // SAFETY: the snapshot and writable entry remain valid.
            found = unsafe { Thread32Next(snapshot.as_raw_handle(), &mut entry) } != 0;
        }
        bail!("could not find the suspended description command's primary thread")
    }

    pub(super) fn kill(&self) {
        // SAFETY: the owned handle always refers to this command's job.
        unsafe { TerminateJobObject(self.0.as_raw_handle(), 1) };
    }
}

pub(super) fn spawn(command: &mut Command) -> Result<(Child, Job)> {
    let job = Job::new()?;
    command.creation_flags(CREATE_SUSPENDED);
    let mut child = command.spawn()?;
    if let Err(error) = job.start(&child) {
        // A failure must not leave either a suspended child or an unowned tree.
        let _ = child.kill();
        let _ = child.wait();
        return Err(error);
    }
    Ok((child, job))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::process::Stdio;
    use std::time::Duration;

    #[test]
    fn job_ends_descendants_after_the_shell_exits() {
        let mut command = Command::new("cmd");
        command.args(["/C", "start \"\" /B ping -n 30 127.0.0.1"]);
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let (mut child, job) = spawn(&mut command).unwrap();
        let mut output = child.stdout.take().unwrap();
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut bytes = vec![];
            let _ = output.read_to_end(&mut bytes);
            let _ = sender.send(());
        });
        assert!(child.wait().unwrap().success());
        assert!(receiver.recv_timeout(Duration::from_millis(200)).is_err());
        job.kill();
        receiver.recv_timeout(Duration::from_secs(5)).unwrap();
    }
}
