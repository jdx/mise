use std::ffi::{OsStr, OsString};
use std::fmt::{Display, Formatter};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::mpsc::channel;
use std::sync::{Mutex, RwLock};
use std::thread;

use color_eyre::Result;
use duct::{Expression, IntoExecutablePath};
use eyre::Context;
use signal_hook::consts::{SIGHUP, SIGINT, SIGQUIT, SIGTERM, SIGUSR1, SIGUSR2};
use signal_hook::iterator::Signals;

use crate::config::Settings;
use crate::env;
use crate::errors::Error::ScriptFailed;
use crate::file::display_path;
use crate::ui::progress_report::SingleReport;

/// Create a command with any number of of positional arguments, which may be
/// different types (anything that implements
/// [`Into<OsString>`](https://doc.rust-lang.org/std/convert/trait.From.html)).
/// See also the [`cmd`](fn.cmd.html) function, which takes a collection of
/// arguments.
///
/// # Example
///
/// ```
///     use std::path::Path;
///     use mise::cmd;
///
///     let arg1 = "foo";
///     let arg2 = "bar".to_owned();
///     let arg3 = Path::new("baz");
///
///     let output = cmd!("echo", arg1, arg2, arg3).read();
///
///     assert_eq!("foo bar baz", output.unwrap());
/// ```
#[macro_export]
macro_rules! cmd {
    ( $program:expr $(, $arg:expr )* $(,)? ) => {
        {
            use std::ffi::OsString;
            let args: std::vec::Vec<OsString> = std::vec![$( Into::<OsString>::into($arg) ),*];
            $crate::cmd::cmd($program, args)
        }
    };
}

/// Create a command with any number of of positional arguments, which may be
/// different types (anything that implements
/// [`Into<OsString>`](https://doc.rust-lang.org/std/convert/trait.From.html)).
/// See also the [`cmd`](fn.cmd.html) function, which takes a collection of
/// arguments.
///
/// # Example
///
/// ```
///     use std::path::Path;
///     use mise::cmd;
///
///     let arg1 = "foo";
///     let arg2 = "bar".to_owned();
///     let arg3 = Path::new("baz");
///
///     let output = cmd!("echo", arg1, arg2, arg3).read();
///
///     assert_eq!("foo bar baz", output.unwrap());
/// ```
pub fn cmd<T, U>(program: T, args: U) -> Expression
where
    T: IntoExecutablePath,
    U: IntoIterator,
    U::Item: Into<OsString>,
{
    let program = program.to_executable();
    let args: Vec<OsString> = args.into_iter().map(Into::<OsString>::into).collect();

    let display_name = program.to_string_lossy();
    let display_args = args
        .iter()
        .map(|s| s.to_string_lossy())
        .collect::<Vec<_>>()
        .join(" ");
    let display_command = [display_name.into(), display_args].join(" ");
    debug!("$ {display_command}");

    duct::cmd(program, args)
}

pub struct CmdLineRunner<'a> {
    cmd: Command,
    pr: Option<&'a dyn SingleReport>,
    stdin: Option<String>,
    prefix: String,
    raw: bool,
    pass_signals: bool,
}

static OUTPUT_LOCK: Mutex<()> = Mutex::new(());

impl<'a> CmdLineRunner<'a> {
    pub fn new<P: AsRef<OsStr>>(program: P) -> Self {
        let mut cmd = Command::new(program);
        cmd.stdin(Stdio::null());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        Self {
            cmd,
            pr: None,
            stdin: None,
            prefix: String::new(),
            raw: false,
            pass_signals: false,
        }
    }

    pub fn stdin<T: Into<Stdio>>(mut self, cfg: T) -> Self {
        self.cmd.stdin(cfg);
        self
    }

    pub fn stdout<T: Into<Stdio>>(mut self, cfg: T) -> Self {
        self.cmd.stdout(cfg);
        self
    }

    pub fn stderr<T: Into<Stdio>>(mut self, cfg: T) -> Self {
        self.cmd.stderr(cfg);
        self
    }

    pub fn prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = prefix.into();
        self
    }

    pub fn current_dir<P: AsRef<Path>>(mut self, dir: P) -> Self {
        self.cmd.current_dir(dir);
        self
    }

    pub fn env_clear(mut self) -> Self {
        self.cmd.env_clear();
        self
    }

    pub fn env<K, V>(mut self, key: K, val: V) -> Self
    where
        K: AsRef<OsStr>,
        V: AsRef<OsStr>,
    {
        self.cmd.env(key, val);
        self
    }
    pub fn envs<I, K, V>(mut self, vars: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: AsRef<OsStr>,
        V: AsRef<OsStr>,
    {
        self.cmd.envs(vars);
        self
    }

    pub fn prepend_path(mut self, paths: Vec<PathBuf>) -> eyre::Result<Self> {
        let existing = self
            .get_env("PATH")
            .map(|c| c.to_owned())
            .unwrap_or_else(|| env::var_os("PATH").unwrap());
        let paths = paths
            .into_iter()
            .chain(env::split_paths(&existing))
            .collect::<Vec<_>>();
        self.cmd.env("PATH", env::join_paths(paths)?);
        Ok(self)
    }

    fn get_env(&self, key: &str) -> Option<&OsStr> {
        for (k, v) in self.cmd.get_envs() {
            if k == key {
                return v;
            }
        }
        None
    }

    pub fn arg<S: AsRef<OsStr>>(mut self, arg: S) -> Self {
        self.cmd.arg(arg.as_ref());
        self
    }

    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.cmd.args(args);
        self
    }

    pub fn with_pr(mut self, pr: &'a dyn SingleReport) -> Self {
        self.pr = Some(pr);
        self
    }
    pub fn with_raw(&mut self) -> &mut Self {
        self.raw = true;
        self
    }

    pub fn with_pass_signals(&mut self) -> &mut Self {
        self.pass_signals = true;
        self
    }

    pub fn stdin_string(mut self, input: impl Into<String>) -> Self {
        self.cmd.stdin(Stdio::piped());
        self.stdin = Some(input.into());
        self
    }

    #[allow(clippy::readonly_write_lock)]
    pub fn execute(mut self) -> Result<()> {
        static RAW_LOCK: RwLock<()> = RwLock::new(());
        let read_lock = RAW_LOCK.read().unwrap();
        let settings = &Settings::try_get()?;
        debug!("$ {}", self);
        if settings.raw || self.raw {
            drop(read_lock);
            let _write_lock = RAW_LOCK.write().unwrap();
            return self.execute_raw();
        }
        let mut cp = self
            .cmd
            .spawn()
            .wrap_err_with(|| format!("failed to execute command: {self}"))?;
        let (tx, rx) = channel();
        if let Some(stdout) = cp.stdout.take() {
            thread::spawn({
                let tx = tx.clone();
                move || {
                    for line in BufReader::new(stdout).lines() {
                        let line = line.unwrap();
                        tx.send(ChildProcessOutput::Stdout(line)).unwrap();
                    }
                }
            });
        }
        if let Some(stderr) = cp.stderr.take() {
            thread::spawn({
                let tx = tx.clone();
                move || {
                    for line in BufReader::new(stderr).lines() {
                        let line = line.unwrap();
                        tx.send(ChildProcessOutput::Stderr(line)).unwrap();
                    }
                }
            });
        }
        if let Some(text) = self.stdin.take() {
            let mut stdin = cp.stdin.take().unwrap();
            thread::spawn(move || {
                stdin.write_all(text.as_bytes()).unwrap();
            });
        }
        let mut sighandle = None;
        if self.pass_signals {
            let mut signals =
                Signals::new([SIGINT, SIGTERM, SIGTERM, SIGHUP, SIGQUIT, SIGUSR1, SIGUSR2])?;
            sighandle = Some(signals.handle());
            let tx = tx.clone();
            thread::spawn(move || {
                for sig in &mut signals {
                    tx.send(ChildProcessOutput::Signal(sig)).unwrap();
                }
            });
        }
        let id = cp.id();
        thread::spawn(move || {
            let status = cp.wait().unwrap();
            if let Some(sighandle) = sighandle {
                sighandle.close();
            }
            tx.send(ChildProcessOutput::ExitStatus(status)).unwrap();
        });
        let mut combined_output = vec![];
        let mut status = None;
        for line in rx {
            match line {
                ChildProcessOutput::Stdout(line) => {
                    self.on_stdout(&line);
                    combined_output.push(line);
                }
                ChildProcessOutput::Stderr(line) => {
                    self.on_stderr(&line);
                    combined_output.push(line);
                }
                ChildProcessOutput::ExitStatus(s) => {
                    status = Some(s);
                }
                ChildProcessOutput::Signal(sig) => {
                    if sig != SIGINT {
                        cmd!("kill", format!("-{sig}"), id.to_string()).run()?;
                    }
                }
            }
        }
        let status = status.unwrap();

        if !status.success() {
            self.on_error(combined_output.join("\n"), status)?;
        }

        Ok(())
    }

    fn execute_raw(mut self) -> Result<()> {
        let status = self.cmd.spawn()?.wait()?;
        match status.success() {
            true => Ok(()),
            false => self.on_error(String::new(), status),
        }
    }

    fn on_stdout(&self, line: &str) {
        let _lock = OUTPUT_LOCK.lock().unwrap();
        if let Some(pr) = self.pr {
            if !line.trim().is_empty() {
                pr.set_message(line.into())
            }
        } else if console::colors_enabled() {
            println!("{}{line}\x1b[0m", self.prefix);
        } else {
            println!("{}{line}", self.prefix);
        }
    }

    fn on_stderr(&self, line: &str) {
        let _lock = OUTPUT_LOCK.lock().unwrap();
        match self.pr {
            Some(pr) => {
                if !line.trim().is_empty() {
                    pr.println(line.into())
                }
            }
            None => {
                if console::colors_enabled() {
                    eprintln!("{}{line}\x1b[0m", self.prefix);
                } else {
                    eprintln!("{}{line}", self.prefix);
                }
            }
        }
    }

    fn on_error(&self, output: String, status: ExitStatus) -> Result<()> {
        let settings = Settings::try_get()?;
        match self.pr {
            Some(pr) => {
                error!("{} failed", self.get_program());
                if !settings.verbose && !output.trim().is_empty() {
                    pr.println(output);
                }
            }
            None => {
                // eprintln!("{}", output);
            }
        }
        Err(ScriptFailed(self.get_program(), Some(status)))?
    }

    fn get_program(&self) -> String {
        display_path(PathBuf::from(self.cmd.get_program()))
    }

    fn get_args(&self) -> Vec<String> {
        self.cmd
            .get_args()
            .map(|s| s.to_string_lossy().to_string())
            .collect::<Vec<_>>()
    }
}

impl Display for CmdLineRunner<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let args = self.get_args().join(" ");
        write!(f, "{} {args}", self.get_program())
    }
}

enum ChildProcessOutput {
    Stdout(String),
    Stderr(String),
    ExitStatus(ExitStatus),
    Signal(i32),
}

#[cfg(test)]
mod tests {
    use crate::cmd;

    #[test]
    fn test_cmd() {
        let output = cmd!("echo", "foo", "bar").read().unwrap();
        assert_eq!("foo bar", output);
    }
}
