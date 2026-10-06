//! Finds processes that are running from a tool's install directory.
//!
//! Pruning decides which versions are unused from tracked configs and tool
//! stubs. A process started before an upgrade, or before `mise use` moved a
//! config to a newer version, can still be running from a version that none of
//! them name any more. Removing that version deletes files the process may
//! still load, so a version a live process runs from is in use.

use std::collections::BTreeMap;
use std::fmt::{Display, Formatter};
use std::path::PathBuf;

/// A process running from an install directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunningProcess {
    pub pid: u32,
    pub name: String,
}

impl Display for RunningProcess {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "pid {} ({})", self.pid, self.name)
    }
}

/// Returns the processes running from each of `install_paths`, keyed by the
/// path as given. Paths no process runs from are absent.
///
/// A process runs from an install when its executable, or an absolute path in
/// its command line, is inside that directory. The command line covers
/// interpreted tools: their executable is the interpreter, often from another
/// install, while the script they run lives in the tool's own directory.
///
/// Processes are read from `/proc`, so this finds nothing on other platforms.
/// Processes that cannot be inspected, such as another user's, are skipped.
pub(crate) fn processes_running_from(
    install_paths: &[PathBuf],
) -> BTreeMap<PathBuf, Vec<RunningProcess>> {
    if install_paths.is_empty() {
        return BTreeMap::new();
    }
    imp::processes_running_from(install_paths)
}

#[cfg(target_os = "linux")]
mod imp {
    use std::collections::BTreeMap;
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    use std::path::{Path, PathBuf};

    use super::RunningProcess;

    pub(super) fn processes_running_from(
        install_paths: &[PathBuf],
    ) -> BTreeMap<PathBuf, Vec<RunningProcess>> {
        // The kernel records the executable with symlinks resolved, so match
        // the resolved install path as well as the one mise uses for it.
        let installs = install_paths
            .iter()
            .map(|path| {
                let resolved = path.canonicalize().ok().filter(|r| r != path);
                (path, resolved)
            })
            .collect::<Vec<_>>();
        let mut found: BTreeMap<PathBuf, Vec<RunningProcess>> = BTreeMap::new();
        let Ok(entries) = std::fs::read_dir("/proc") else {
            return found;
        };
        for entry in entries.flatten() {
            let Some(pid) = entry.file_name().to_str().and_then(|s| s.parse().ok()) else {
                continue;
            };
            let proc_dir = entry.path();
            let paths = process_paths(&proc_dir);
            if paths.is_empty() {
                continue;
            }
            for (install, resolved) in &installs {
                let runs_from = |root: &Path| paths.iter().any(|p| p.starts_with(root));
                if runs_from(install) || resolved.as_deref().is_some_and(runs_from) {
                    found
                        .entry((*install).clone())
                        .or_default()
                        .push(RunningProcess {
                            pid,
                            name: process_name(&proc_dir),
                        });
                }
            }
        }
        for processes in found.values_mut() {
            processes.sort_by_key(|p| p.pid);
        }
        found
    }

    /// The executable and the absolute command-line paths of a process.
    fn process_paths(proc_dir: &Path) -> Vec<PathBuf> {
        let mut paths = vec![];
        if let Ok(exe) = std::fs::read_link(proc_dir.join("exe")) {
            paths.push(exe);
        }
        if let Ok(cmdline) = std::fs::read(proc_dir.join("cmdline")) {
            paths.extend(
                cmdline
                    .split(|b| *b == 0)
                    .filter(|arg| arg.first() == Some(&b'/'))
                    .map(|arg| PathBuf::from(OsStr::from_bytes(arg))),
            );
        }
        paths
    }

    fn process_name(proc_dir: &Path) -> String {
        std::fs::read_to_string(proc_dir.join("comm"))
            .map(|comm| comm.trim_end().to_string())
            .unwrap_or_else(|_| "unknown".to_string())
    }
}

#[cfg(not(target_os = "linux"))]
mod imp {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use super::RunningProcess;

    pub(super) fn processes_running_from(
        _install_paths: &[PathBuf],
    ) -> BTreeMap<PathBuf, Vec<RunningProcess>> {
        BTreeMap::new()
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use std::path::Path;
    use std::process::{Child, Command};

    use super::*;

    /// Kills the child when a test ends, including when an assertion fails.
    struct Running(Child);

    impl Drop for Running {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    fn spawn(cmd: &mut Command) -> Running {
        // Copying an executable and starting it right away can fail with
        // ETXTBSY while another test thread forks with the copy still open.
        for _ in 0..50 {
            match cmd.spawn() {
                Ok(child) => return Running(child),
                Err(err) if err.kind() == std::io::ErrorKind::ExecutableFileBusy => {
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                Err(err) => panic!("failed to spawn {cmd:?}: {err}"),
            }
        }
        panic!("{cmd:?} stayed busy");
    }

    fn system_sleep() -> PathBuf {
        ["/usr/bin/sleep", "/bin/sleep"]
            .into_iter()
            .map(PathBuf::from)
            .find(|p| p.exists())
            .expect("sleep binary")
    }

    /// Copies sleep into `install`. The copy keeps its name because multi-call
    /// coreutils and busybox pick the program to run from it.
    fn install_with_sleep(install: &Path) -> PathBuf {
        let bin = install.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let sleep = bin.join("sleep");
        std::fs::copy(system_sleep(), &sleep).unwrap();
        sleep
    }

    fn pids(found: &BTreeMap<PathBuf, Vec<RunningProcess>>, install: &Path) -> Vec<u32> {
        found
            .get(install)
            .map(|processes| processes.iter().map(|p| p.pid).collect())
            .unwrap_or_default()
    }

    #[test]
    fn finds_a_process_by_its_executable() {
        let tmp = tempfile::tempdir().unwrap();
        let install = tmp.path().join("installs/tool/1.0.0");
        let idle = tmp.path().join("installs/tool/2.0.0");
        std::fs::create_dir_all(&idle).unwrap();
        let sleep = install_with_sleep(&install);
        let mut child = spawn(Command::new(&sleep).arg("30"));

        let found = processes_running_from(&[install.clone(), idle.clone()]);
        assert_eq!(pids(&found, &install), vec![child.0.id()]);
        assert_eq!(found[&install][0].name, "sleep");
        assert!(!found.contains_key(&idle));

        child.0.kill().unwrap();
        child.0.wait().unwrap();
        assert!(!processes_running_from(std::slice::from_ref(&install)).contains_key(&install));
    }

    #[test]
    fn finds_an_interpreted_tool_by_its_command_line() {
        let tmp = tempfile::tempdir().unwrap();
        let install = tmp.path().join("installs/tool/1.0.0");
        std::fs::create_dir_all(install.join("bin")).unwrap();
        let script = install.join("bin/tool.sh");
        // The trailing command keeps the shell from replacing itself with
        // sleep, so the shell stays the process whose executable is outside
        // the install and whose command line names the script.
        std::fs::write(&script, "sleep 30\nexit 0\n").unwrap();
        let child = spawn(Command::new("sh").arg(&script));

        let found = processes_running_from(std::slice::from_ref(&install));
        assert!(pids(&found, &install).contains(&child.0.id()));
    }

    #[test]
    fn matches_whole_path_components() {
        let tmp = tempfile::tempdir().unwrap();
        let running = tmp.path().join("installs/tool/1.0.10");
        let sibling = tmp.path().join("installs/tool/1.0.1");
        std::fs::create_dir_all(&sibling).unwrap();
        let sleep = install_with_sleep(&running);
        let _child = spawn(Command::new(&sleep).arg("30"));

        let found = processes_running_from(std::slice::from_ref(&sibling));
        assert!(!found.contains_key(&sibling));
    }

    #[test]
    fn finds_a_process_through_a_symlinked_install_path() {
        let tmp = tempfile::tempdir().unwrap();
        let real = tmp.path().join("real/1.0.0");
        let sleep = install_with_sleep(&real);
        let linked = tmp.path().join("linked");
        std::os::unix::fs::symlink(tmp.path().join("real"), &linked).unwrap();
        let install = linked.join("1.0.0");
        let child = spawn(Command::new(&sleep).arg("30"));

        let found = processes_running_from(std::slice::from_ref(&install));
        assert_eq!(pids(&found, &install), vec![child.0.id()]);
    }
}
