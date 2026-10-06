//! Finds processes that are running from a tool's install directory.
//!
//! Pruning decides which versions are unused from tracked configs and tool
//! stubs. A process started before an upgrade, or before `mise use` moved a
//! config to a newer version, can still be running from a version that none of
//! them name any more. Removing that version deletes files the process may
//! still load, so a version a live process runs from is in use.

use std::cell::OnceCell;
use std::collections::BTreeMap;
use std::fmt::{Display, Formatter};
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::toolset::ToolVersion;

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

/// Returns the processes running from each of `tvs`, keyed by install path.
/// Versions no process runs from are absent.
///
/// A process runs from an install when its executable, or an absolute path in
/// its command line, is inside that directory. The command line covers
/// interpreted tools: their executable is the interpreter, often from another
/// install, while the script they run lives in the tool's own directory.
///
/// A command-line path can go through one of the tool's runtime links, such as
/// `installs/node/latest`, which an upgrade retargets while the process keeps
/// running. A link that has not changed since the process started is followed.
/// Otherwise the version the process started from is unknown, so every one of
/// `tvs` for that tool counts as running until the process exits.
///
/// Processes are read from `/proc` on Linux, `libproc` and `sysctl` on macOS,
/// and the process APIs on Windows. Processes that cannot be inspected, such as
/// another user's, are skipped.
pub(crate) fn processes_running_from(
    tvs: &[&ToolVersion],
) -> BTreeMap<PathBuf, Vec<RunningProcess>> {
    if tvs.is_empty() {
        return BTreeMap::new();
    }
    let candidates = tvs
        .iter()
        .map(|tv| Candidate {
            path: tv.install_path(),
            links_dir: tv.ba().installs_path().to_path_buf(),
        })
        .collect::<Vec<_>>();
    scan(&candidates)
}

#[cfg(target_os = "linux")]
#[path = "running_linux.rs"]
mod sys;
#[cfg(target_os = "macos")]
#[path = "running_macos.rs"]
mod sys;
#[cfg(windows)]
#[path = "running_windows.rs"]
mod sys;
#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
mod sys {
    use super::Listing;

    pub(super) fn list() -> Listing {
        Listing::default()
    }

    pub(super) fn start_time(_pid: u32) -> Option<std::time::SystemTime> {
        None
    }
}

/// Process start times are only known to the second on some platforms.
const START_TIME_SLACK: Duration = Duration::from_secs(2);

/// Bounds following a chain of runtime links, such as `latest` -> `20` -> `20.1.0`.
const MAX_LINKS: usize = 8;

/// An install directory that pruning may remove.
pub(super) struct Candidate {
    /// The version's install directory.
    pub(super) path: PathBuf,
    /// The directory holding the tool's runtime links, such as `latest` or
    /// `20`, which may point at `path`.
    pub(super) links_dir: PathBuf,
}

/// What the platform could read about one process.
pub(super) struct Process {
    pub(super) pid: u32,
    pub(super) name: String,
    pub(super) exe: Option<PathBuf>,
    /// The absolute paths in the command line.
    pub(super) args: Vec<PathBuf>,
}

#[derive(Default)]
pub(super) struct Listing {
    pub(super) processes: Vec<Process>,
    /// Processes whose executable could not be read for lack of permission.
    pub(super) uninspected: usize,
}

struct Install<'a> {
    candidate: &'a Candidate,
    // The executable path is reported with symlinks resolved, so match the
    // resolved paths as well as the ones mise uses.
    roots: Vec<PathBuf>,
    links_dirs: Vec<PathBuf>,
}

impl Install<'_> {
    fn contains(&self, path: &Path) -> bool {
        let path = comparable(path);
        self.roots.iter().any(|root| path.starts_with(root))
    }
}

/// Windows paths ignore case and may carry a `\\?\` prefix.
fn comparable(path: &Path) -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(dunce::simplified(path).to_string_lossy().to_lowercase())
    } else {
        path.to_path_buf()
    }
}

pub(super) fn scan(candidates: &[Candidate]) -> BTreeMap<PathBuf, Vec<RunningProcess>> {
    let installs = candidates
        .iter()
        .map(|candidate| Install {
            candidate,
            roots: with_resolved(&candidate.path),
            links_dirs: with_resolved(&candidate.links_dir),
        })
        .collect::<Vec<_>>();
    let mut found: BTreeMap<PathBuf, Vec<RunningProcess>> = BTreeMap::new();
    let listing = sys::list();
    for process in &listing.processes {
        let started = OnceCell::new();
        let started = || *started.get_or_init(|| sys::start_time(process.pid));
        for install in &installs {
            if runs_from(install, process.exe.as_deref(), &process.args, &started) {
                found
                    .entry(install.candidate.path.clone())
                    .or_default()
                    .push(RunningProcess {
                        pid: process.pid,
                        name: process.name.clone(),
                    });
            }
        }
    }
    if listing.uninspected > 0 {
        debug!(
            "could not read the executable of {} processes; pruning cannot tell whether they run from a tool version",
            listing.uninspected
        );
    }
    for processes in found.values_mut() {
        processes.sort_by_key(|p| p.pid);
    }
    found
}

fn runs_from(
    install: &Install,
    exe: Option<&Path>,
    args: &[PathBuf],
    started: &dyn Fn() -> Option<SystemTime>,
) -> bool {
    if exe.is_some_and(|exe| install.contains(exe)) || args.iter().any(|a| install.contains(a)) {
        return true;
    }
    args.iter().any(|arg| {
        install.links_dirs.iter().any(|links_dir| {
            let Some((entry, rest)) = split_at_child(arg, links_dir) else {
                return false;
            };
            match follow_link(&entry) {
                // A real version directory, which the check above covered.
                Followed::NotALink => false,
                Followed::Link { target, changed }
                    if started().is_some_and(|start| changed + START_TIME_SLACK < start) =>
                {
                    install.contains(&target.join(rest))
                }
                // The link changed after the process started through it, or
                // cannot be followed, so it may have named this version then.
                Followed::Link { .. } | Followed::Broken => true,
            }
        })
    })
}

fn with_resolved(path: &Path) -> Vec<PathBuf> {
    let mut paths = vec![comparable(path)];
    if let Ok(resolved) = path.canonicalize() {
        let resolved = comparable(&resolved);
        if resolved != paths[0] {
            paths.push(resolved);
        }
    }
    paths
}

/// Splits `path` into the child of `dir` it goes through and the rest.
fn split_at_child(path: &Path, dir: &Path) -> Option<(PathBuf, PathBuf)> {
    let (path, dir) = (comparable(path), comparable(dir));
    let mut components = path.strip_prefix(&dir).ok()?.components();
    let Some(Component::Normal(child)) = components.next() else {
        return None;
    };
    Some((dir.join(child), components.as_path().to_path_buf()))
}

enum Followed {
    NotALink,
    /// The directory the link names now, and when the most recently
    /// changed link along the way was written.
    Link {
        target: PathBuf,
        changed: SystemTime,
    },
    Broken,
}

fn follow_link(path: &Path) -> Followed {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => {}
        _ => return Followed::NotALink,
    }
    let mut current = path.to_path_buf();
    let mut changed = UNIX_EPOCH;
    for _ in 0..MAX_LINKS {
        let Ok(meta) = std::fs::symlink_metadata(&current) else {
            return Followed::Broken;
        };
        if !meta.file_type().is_symlink() {
            return match current.canonicalize() {
                Ok(target) => Followed::Link {
                    target: comparable(&target),
                    changed,
                },
                Err(_) => Followed::Broken,
            };
        }
        let (Ok(modified), Ok(target), Some(parent)) = (
            meta.modified(),
            std::fs::read_link(&current),
            current.parent(),
        ) else {
            return Followed::Broken;
        };
        changed = changed.max(modified);
        current = parent.join(target);
    }
    Followed::Broken
}

#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
mod tests {
    use std::path::Path;
    use std::process::{Child, Command, Stdio};
    use std::time::{Duration, SystemTime};

    use filetime::FileTime;

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
                    std::thread::sleep(Duration::from_millis(20));
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

    /// Writes a script into `install` that runs until its stdin closes,
    /// without starting a child that would outlive the test.
    fn install_with_script(install: &Path) {
        std::fs::create_dir_all(install.join("bin")).unwrap();
        std::fs::write(install.join("bin/tool.sh"), "read -r line\n").unwrap();
    }

    /// Runs `script` through `sh`, so the executable is outside the install
    /// and only the command line names the script.
    fn run_script(script: &Path) -> Running {
        spawn(Command::new("sh").arg(script).stdin(Stdio::piped()))
    }

    fn candidate(path: &Path) -> Candidate {
        Candidate {
            path: path.to_path_buf(),
            links_dir: path.parent().unwrap().to_path_buf(),
        }
    }

    fn pids(found: &BTreeMap<PathBuf, Vec<RunningProcess>>, install: &Path) -> Vec<u32> {
        found
            .get(install)
            .map(|processes| processes.iter().map(|p| p.pid).collect())
            .unwrap_or_default()
    }

    fn link_written_at(link: &Path, at: SystemTime) {
        let at = FileTime::from_system_time(at);
        filetime::set_symlink_file_times(link, at, at).unwrap();
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn lists_every_process_on_macos() {
        let listed = sys::list().processes.len();
        let ps = Command::new("ps")
            .args(["-A", "-o", "pid="])
            .output()
            .unwrap();
        let total = String::from_utf8_lossy(&ps.stdout).lines().count();
        // Some processes cannot be inspected, but a prefix of the list would
        // fall far short of this.
        assert!(listed * 2 > total, "listed {listed} of {total} processes");
    }

    #[test]
    fn finds_a_process_by_its_executable() {
        let tmp = tempfile::tempdir().unwrap();
        let install = tmp.path().join("installs/tool/1.0.0");
        let idle = tmp.path().join("installs/tool/2.0.0");
        std::fs::create_dir_all(&idle).unwrap();
        let sleep = install_with_sleep(&install);
        let mut child = spawn(Command::new(&sleep).arg("30"));

        let found = scan(&[candidate(&install), candidate(&idle)]);
        assert_eq!(pids(&found, &install), vec![child.0.id()]);
        assert_eq!(found[&install][0].name, "sleep");
        assert!(!found.contains_key(&idle));

        child.0.kill().unwrap();
        child.0.wait().unwrap();
        assert!(!scan(&[candidate(&install)]).contains_key(&install));
    }

    #[test]
    fn finds_an_interpreted_tool_by_its_command_line() {
        let tmp = tempfile::tempdir().unwrap();
        let install = tmp.path().join("installs/tool/1.0.0");
        install_with_script(&install);
        let child = run_script(&install.join("bin/tool.sh"));

        let found = scan(&[candidate(&install)]);
        assert_eq!(pids(&found, &install), vec![child.0.id()]);
    }

    #[test]
    fn matches_whole_path_components() {
        let tmp = tempfile::tempdir().unwrap();
        let running = tmp.path().join("installs/tool/1.0.10");
        let sibling = tmp.path().join("installs/tool/1.0.1");
        std::fs::create_dir_all(&sibling).unwrap();
        let sleep = install_with_sleep(&running);
        let _child = spawn(Command::new(&sleep).arg("30"));

        let found = scan(&[candidate(&sibling)]);
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

        let found = scan(&[candidate(&install)]);
        assert_eq!(pids(&found, &install), vec![child.0.id()]);
    }

    #[test]
    fn follows_a_runtime_link_that_has_not_changed() {
        let tmp = tempfile::tempdir().unwrap();
        let tool = tmp.path().join("installs/tool");
        let used = tool.join("1.0.0");
        let other = tool.join("2.0.0");
        install_with_script(&used);
        install_with_script(&other);
        let link = tool.join("1");
        std::os::unix::fs::symlink("./1.0.0", &link).unwrap();
        link_written_at(&link, SystemTime::now() - Duration::from_secs(3600));
        let child = run_script(&link.join("bin/tool.sh"));

        let found = scan(&[candidate(&used), candidate(&other)]);
        assert_eq!(pids(&found, &used), vec![child.0.id()]);
        assert!(!found.contains_key(&other));
    }

    #[test]
    fn keeps_every_version_a_retargeted_link_may_have_named() {
        let tmp = tempfile::tempdir().unwrap();
        let tool = tmp.path().join("installs/tool");
        let old = tool.join("1.0.0");
        let new = tool.join("2.0.0");
        install_with_script(&old);
        install_with_script(&new);
        let latest = tool.join("latest");
        std::os::unix::fs::symlink("./1.0.0", &latest).unwrap();
        link_written_at(&latest, SystemTime::now() - Duration::from_secs(3600));
        let child = run_script(&latest.join("bin/tool.sh"));

        // An upgrade retargets the link while the process keeps running from
        // the version it started with.
        std::fs::remove_file(&latest).unwrap();
        std::os::unix::fs::symlink("./2.0.0", &latest).unwrap();

        let found = scan(&[candidate(&old)]);
        assert_eq!(pids(&found, &old), vec![child.0.id()]);
    }
}

#[cfg(all(test, windows))]
mod windows_tests {
    use std::path::Path;
    use std::process::{Child, Command, Stdio};

    use super::*;

    /// Kills the child when a test ends, including when an assertion fails.
    struct Running(Child);

    impl Drop for Running {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    fn candidate(path: &Path) -> Candidate {
        Candidate {
            path: path.to_path_buf(),
            links_dir: path.parent().unwrap().to_path_buf(),
        }
    }

    fn pids(found: &BTreeMap<PathBuf, Vec<RunningProcess>>, install: &Path) -> Vec<u32> {
        found
            .get(install)
            .map(|processes| processes.iter().map(|p| p.pid).collect())
            .unwrap_or_default()
    }

    /// A copy of cmd.exe inside `install` that waits on its stdin.
    fn run_copy_of_cmd(install: &Path) -> Running {
        let bin = install.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let cmd = bin.join("cmd.exe");
        let system = std::env::var_os("ComSpec").unwrap();
        std::fs::copy(system, &cmd).unwrap();
        Running(
            Command::new(&cmd)
                .args(["/c", "pause"])
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .spawn()
                .unwrap(),
        )
    }

    /// Writes a batch file into `install` and runs it with the system cmd.exe,
    /// so the executable is outside the install and only the command line
    /// names the script.
    fn run_script(install: &Path) -> Running {
        let bin = install.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let script = bin.join("tool with space.cmd");
        std::fs::write(&script, "@pause\r\n").unwrap();
        Running(
            Command::new(std::env::var_os("ComSpec").unwrap())
                .arg("/c")
                .arg(&script)
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .spawn()
                .unwrap(),
        )
    }

    #[test]
    fn finds_a_process_by_its_executable() {
        let tmp = tempfile::tempdir().unwrap();
        let install = tmp.path().join("installs/tool/1.0.0");
        let idle = tmp.path().join("installs/tool/2.0.0");
        std::fs::create_dir_all(&idle).unwrap();
        let child = run_copy_of_cmd(&install);
        std::thread::sleep(std::time::Duration::from_millis(500));

        let found = scan(&[candidate(&install), candidate(&idle)]);
        assert_eq!(pids(&found, &install), vec![child.0.id()]);
        assert!(!found.contains_key(&idle));
    }

    #[test]
    fn finds_a_script_by_its_command_line_in_any_case() {
        let tmp = tempfile::tempdir().unwrap();
        let install = tmp.path().join("installs/tool/1.0.0");
        let child = run_script(&install);
        std::thread::sleep(std::time::Duration::from_millis(500));

        let upper = PathBuf::from(install.to_string_lossy().to_uppercase());
        let found = scan(&[candidate(&upper)]);
        assert_eq!(pids(&found, &upper), vec![child.0.id()]);
    }

    #[test]
    fn matches_whole_path_components() {
        let tmp = tempfile::tempdir().unwrap();
        let running = tmp.path().join("installs/tool/1.0.10");
        let sibling = tmp.path().join("installs/tool/1.0.1");
        std::fs::create_dir_all(&sibling).unwrap();
        let _child = run_copy_of_cmd(&running);
        std::thread::sleep(std::time::Duration::from_millis(500));

        assert!(!scan(&[candidate(&sibling)]).contains_key(&sibling));
    }

    #[test]
    fn splits_quoted_arguments() {
        let paths = sys::absolute_paths(r#"node "C:\a b\c.js" --flag C:\d\e relative"#);
        assert_eq!(
            paths,
            vec![PathBuf::from(r"C:\a b\c.js"), PathBuf::from(r"C:\d\e")]
        );
    }

    #[test]
    fn splits_escaped_quotes() {
        // An escaped quote does not end the quoted title, so the script that
        // follows is still its own argument.
        let paths = sys::absolute_paths(r#"node --title "a\"b" C:\tools\x\1.0.0\app.js"#);
        assert_eq!(paths, vec![PathBuf::from(r"C:\tools\x\1.0.0\app.js")]);
        // Two backslashes before a quote are one backslash, and the quote
        // closes the argument.
        let paths = sys::absolute_paths(r#""C:\a b\\" C:\d\e"#);
        assert_eq!(
            paths,
            vec![PathBuf::from(r"C:\a b\"), PathBuf::from(r"C:\d\e")]
        );
    }
}
