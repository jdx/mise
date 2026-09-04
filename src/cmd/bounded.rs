//! Bounded capture for non-interactive vendor generators and host probes.
use super::*;
use eyre::eyre;

impl CmdLineRunner<'_> {
    /// Capture a finite response, with one deadline for the process and pipes.
    /// This command owns its child tree even when mise itself is nested.
    pub(crate) async fn read_isolated(mut self, limit: usize) -> Result<String> {
        let _read_lock = RAW_LOCK.read().await;
        let timeout = self.timeout.unwrap_or(Duration::from_secs(5));
        self.cmd.kill_on_drop(true);
        self.cmd
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(unix)]
        {
            self.cmd.env(TASK_PGID_MANAGED_ENV, "1");
            self.cmd.process_group(0);
        }
        let mut child = self.spawn_async_with_etxtbsy_retry().await?;
        let _running = RunningPidGuard::new(child.id());
        let _tree = ChildTree::new(&child)?;
        let stdout = child.stdout.take().expect("piped stdout");
        let stderr = child.stderr.take().expect("piped stderr");
        let result = tokio::time::timeout(timeout, async {
            tokio::try_join!(child.wait(), capture(stdout, limit), capture(stderr, limit))
        })
        .await;
        let (status, stdout, stderr) = match result {
            Ok(Ok(output)) => output,
            Ok(Err(err)) => {
                drop(_tree);
                let _ = child.wait().await;
                return Err(err.into());
            }
            Err(_) => {
                drop(_tree);
                let _ = child.wait().await;
                bail!("timed out after {timeout:?}");
            }
        };
        if stdout.len().saturating_add(stderr.len()) > limit {
            bail!("command output exceeded {limit} bytes");
        }
        if !status.success() {
            bail!("command exited with non-zero status: {status}");
        }
        Ok(String::from_utf8(stdout)?.trim_end().to_string())
    }
}

async fn capture(mut stream: impl AsyncRead + Unpin, limit: usize) -> std::io::Result<Vec<u8>> {
    let mut output = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = stream.read(&mut buf).await?;
        if n == 0 {
            return Ok(output);
        }
        if output.len().saturating_add(n) > limit {
            return Err(std::io::Error::other(format!(
                "command output exceeded {limit} bytes"
            )));
        }
        output.extend_from_slice(&buf[..n]);
    }
}

#[cfg(unix)]
struct ChildTree(nix::unistd::Pid);
#[cfg(unix)]
impl ChildTree {
    fn new(child: &tokio::process::Child) -> Result<Self> {
        Ok(Self(nix::unistd::Pid::from_raw(
            child.id().ok_or_else(|| eyre!("child has no pid"))? as i32,
        )))
    }
}
#[cfg(unix)]
impl Drop for ChildTree {
    fn drop(&mut self) {
        let _ = nix::sys::signal::killpg(self.0, nix::sys::signal::Signal::SIGKILL);
    }
}

// A job closes the whole tree even after the direct child exits. Merely
// taskkilling the child's PID cannot find descendants once it has exited.
#[cfg(windows)]
struct ChildTree(usize);
#[cfg(windows)]
impl ChildTree {
    fn new(child: &tokio::process::Child) -> Result<Self> {
        use windows_sys::Win32::System::JobObjects::*;
        unsafe {
            let handle = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if handle.is_null() {
                return Err(std::io::Error::last_os_error().into());
            }
            let job = Self(handle as usize);
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const _,
                std::mem::size_of_val(&info) as u32,
            ) == 0
            {
                return Err(std::io::Error::last_os_error().into());
            }
            let process = child
                .raw_handle()
                .ok_or_else(|| eyre!("child has no handle"))?;
            if AssignProcessToJobObject(handle, process as _) == 0 {
                return Err(std::io::Error::last_os_error().into());
            }
            Ok(job)
        }
    }
}
#[cfg(windows)]
impl Drop for ChildTree {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0 as _);
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    #[tokio::test]
    async fn bounds_output_and_inherited_pipes() {
        let started = Instant::now();
        for script in ["yes x", "sleep 30 & exit 0", "sleep 30 & wait"] {
            let err = CmdLineRunner::new("/bin/sh")
                .args(["-c", script])
                .with_timeout(Duration::from_millis(150))
                .read_isolated(1024)
                .await
                .unwrap_err();
            assert!(
                err.to_string().contains("exceeded") || err.to_string().contains("timed out"),
                "{err}"
            );
        }
        assert!(started.elapsed() < Duration::from_secs(3));
    }

    #[tokio::test]
    async fn cleans_up_descendants_when_parent_exits() {
        let dir = tempfile::tempdir().unwrap();
        let pidfile = dir.path().join("child.pid");
        let script = "sleep 30 >/dev/null 2>&1 & echo $! > \"$PIDFILE\"; printf ok";
        let result = CmdLineRunner::new("/bin/sh")
            .args(["-c", script])
            .env("PIDFILE", &pidfile)
            .read_isolated(1024)
            .await
            .unwrap();
        assert_eq!(result, "ok");
        let pid: i32 = std::fs::read_to_string(pidfile)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        // A killed orphan can briefly be a zombie before init reaps it.
        let status = CmdLineRunner::new("/bin/ps")
            .args(["-o", "stat=", "-p", &pid.to_string()])
            .read()
            .await;
        assert!(status.is_err() || status.unwrap().trim().starts_with('Z'));
    }
}
