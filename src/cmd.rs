use std::ffi::{OsStr, OsString};
use std::fmt::{Display, Formatter};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::mpsc::channel;
use std::thread;

use crate::config::Settings;
use color_eyre::Result;
use duct::{Expression, IntoExecutablePath};
use eyre::Context;

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
///     use rtx::cmd;
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
///     use rtx::cmd;
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
}

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
        }
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

    pub fn prepend_path_env(&mut self, path: PathBuf) -> &mut Self {
        let k: OsString = "PATH".into();
        let existing = self
            .get_env(&k)
            .map(|c| c.to_owned())
            .unwrap_or_else(|| env::var_os("PATH").unwrap());
        let mut paths = env::split_paths(&existing).collect::<Vec<_>>();
        paths.insert(0, path);
        self.cmd.env("PATH", env::join_paths(paths).unwrap());
        self
    }

    fn get_env(&self, key: &OsString) -> Option<&OsStr> {
        for (k, v) in self.cmd.get_envs() {
            if k != key {
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

    pub fn stdin_string(mut self, input: impl Into<String>) -> Self {
        self.cmd.stdin(Stdio::piped());
        self.stdin = Some(input.into());
        self
    }

    pub fn execute(mut self) -> Result<()> {
        let settings = &Settings::try_get()?;
        debug!("$ {}", self);
        if settings.raw {
            return self.execute_raw();
        }
        let mut cp = self
            .cmd
            .spawn()
            .wrap_err_with(|| format!("failed to execute command: {self}"))?;
        let stdout = BufReader::new(cp.stdout.take().unwrap());
        let stderr = BufReader::new(cp.stderr.take().unwrap());
        let (tx, rx) = channel();
        thread::spawn({
            let tx = tx.clone();
            move || {
                for line in stdout.lines() {
                    let line = line.unwrap();
                    tx.send(ChildProcessOutput::Stdout(line)).unwrap();
                }
                tx.send(ChildProcessOutput::Done).unwrap();
            }
        });
        thread::spawn({
            let tx = tx.clone();
            move || {
                for line in stderr.lines() {
                    let line = line.unwrap();
                    tx.send(ChildProcessOutput::Stderr(line)).unwrap();
                }
                tx.send(ChildProcessOutput::Done).unwrap();
            }
        });
        if let Some(text) = self.stdin.take() {
            let mut stdin = cp.stdin.take().unwrap();
            thread::spawn(move || {
                stdin.write_all(text.as_bytes()).unwrap();
            });
        }
        thread::spawn(move || {
            let status = cp.wait().unwrap();
            tx.send(ChildProcessOutput::ExitStatus(status)).unwrap();
            tx.send(ChildProcessOutput::Done).unwrap();
        });
        let mut combined_output = vec![];
        let mut wait_for_count = 3;
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
                ChildProcessOutput::Done => {
                    wait_for_count -= 1;
                    if wait_for_count == 0 {
                        break;
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
        if !line.trim().is_empty() {
            if let Some(pr) = self.pr {
                pr.set_message(line.into())
            }
        }
    }

    fn on_stderr(&self, line: &str) {
        if !line.trim().is_empty() {
            match self.pr {
                Some(pr) => pr.println(line.into()),
                None => eprintln!("{}", line),
            }
        }
    }

    fn on_error(&self, output: String, status: ExitStatus) -> Result<()> {
        let settings = Settings::try_get()?;
        match self.pr {
            Some(pr) => {
                pr.error(format!("{} failed", self.get_program()));
                if !settings.verbose && !output.trim().is_empty() {
                    pr.println(output);
                }
            }
            None => {
                eprintln!("{}", output);
            }
        }
        Err(ScriptFailed(self.get_program(), Some(status)))?
    }

    fn get_program(&self) -> String {
        display_path(&PathBuf::from(self.cmd.get_program()))
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
    Done,
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
