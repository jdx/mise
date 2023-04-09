use color_eyre::Result;
use std::ffi::OsString;
use std::io::{BufRead, BufReader};
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

pub fn run_by_line_to_pr(settings: &Settings, cmd: Command, pr: &mut ProgressReport) -> Result<()> {
    run_by_line(
        settings,
        cmd,
        |output| {
            pr.error();
            if !settings.verbose && !output.trim().is_empty() {
                pr.println(output);
            }
        },
        |line| {
            if !line.trim().is_empty() {
                pr.set_message(line.into());
            }
        },
        |line| {
            if !line.trim().is_empty() {
                pr.println(line.into());
            }
        },
    )
}

pub fn run_by_line<'a, F1, F2, F3>(
    settings: &Settings,
    mut cmd: Command,
    on_error: F1,
    on_stdout: F2,
    on_stderr: F3,
) -> Result<()>
where
    F1: Fn(String),
    F2: Fn(&str) + Send + Sync + 'a,
    F3: Fn(&str) + Send + Sync,
{
    let program = cmd.get_program().to_string_lossy().to_string();
    if settings.raw {
        let status = cmd.spawn()?.wait()?;
        match status.success() {
            true => Ok(()),
            false => {
                on_error(String::new());
                Err(ScriptFailed(program, Some(status)).into())
            }
        }
    } else {
        let mut cp = cmd
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
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
                    on_stdout(&line);
                    combined_output.push(line);
                }
                ChildProcessOutput::Stderr(line) => {
                    on_stderr(&line);
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
            on_error(combined_output.join("\n"));
            Err(ScriptFailed(program, Some(status)))?;
        }

        Ok(())
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
