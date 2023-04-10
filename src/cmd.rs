use color_eyre::Result;
use std::ffi::{OsStr, OsString};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::mpsc::channel;
use std::thread;

use crate::config::Settings;
use crate::errors::Error::ScriptFailed;
use crate::ui::progress_report::ProgressReport;
use duct::{Expression, IntoExecutablePath};

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
    settings: &'a Settings,
    pr: Option<&'a ProgressReport>,
    stdin: Option<String>,
}
impl<'a> CmdLineRunner<'a> {
    pub fn new<P: AsRef<OsStr>>(settings: &'a Settings, program: P) -> Self {
        let mut cmd = Command::new(program);
        cmd.stdin(Stdio::null());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        Self {
            cmd,
            settings,
            pr: None,
            stdin: None,
        }
    }

    pub fn env_clear(&mut self) -> &mut Self {
        self.cmd.env_clear();
        self
    }

    pub fn envs<I, K, V>(&mut self, vars: I) -> &mut Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: AsRef<OsStr>,
        V: AsRef<OsStr>,
    {
        self.cmd.envs(vars);
        self
    }

    pub fn arg<S: AsRef<OsStr>>(&mut self, arg: S) -> &mut Self {
        self.cmd.arg(arg.as_ref());
        self
    }

    pub fn with_pr(&mut self, pr: &'a ProgressReport) -> &mut Self {
        self.pr = Some(pr);
        self
    }

    pub fn stdin_string(&mut self, input: impl Into<String>) -> &mut Self {
        self.cmd.stdin(Stdio::piped());
        self.stdin = Some(input.into());
        self
    }

    pub fn execute(mut self) -> Result<()> {
        debug!("$ {} {}", self.get_program(), self.get_args().join(" "));
        if self.settings.raw {
            return self.execute_raw();
        }
        let mut cp = self.cmd.spawn()?;
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
                pr.println(line)
            }
        }
    }

    fn on_stderr(&self, line: &str) {
        if !line.trim().is_empty() {
            match self.pr {
                Some(pr) => pr.error(),
                None => eprintln!("{}", line),
            }
        }
    }

    fn on_error(&self, output: String, status: ExitStatus) -> Result<()> {
        match self.pr {
            Some(pr) => {
                pr.error();
                if !self.settings.verbose && !output.trim().is_empty() {
                    pr.println(output);
                }
            }
            None => {
                eprintln!("{}", output);
            }
        }
        let program = self.cmd.get_program().to_string_lossy().to_string();
        Err(ScriptFailed(program, Some(status)))?
    }

    fn get_program(&self) -> String {
        self.cmd.get_program().to_string_lossy().to_string()
    }

    fn get_args(&self) -> Vec<String> {
        self.cmd
            .get_args()
            .map(|s| s.to_string_lossy().to_string())
            .collect::<Vec<_>>()
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
