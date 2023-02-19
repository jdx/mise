use atty::Stream;
use std::ffi::{OsStr, OsString};

use color_eyre::eyre::{eyre, Result};
use duct::IntoExecutablePath;
use indexmap::IndexMap;
use indoc::formatdoc;
use once_cell::sync::Lazy;

use crate::cli::args::runtime::{RuntimeArg, RuntimeArgParser};
use crate::cli::command::Command;
//
#[cfg(test)]
use crate::cmd;
use crate::config::Config;
use crate::env;
use crate::output::Output;
use crate::toolset::ToolsetBuilder;
use crate::ui::color::Color;

/// execute a command with runtime(s) set
///
/// use this to avoid modifying the shell session or running ad-hoc commands with the rtx runtimes
/// set.
///
/// Runtimes will be loaded from .tool-versions, though they can be overridden with <RUNTIME> args
/// Note that only the plugin specified will be overridden, so if a `.tool-versions` file
/// includes "nodejs 20" but you run `rtx exec python@3.11`; it will still load nodejs@20.
///
/// The "--" separates runtimes from the commands to pass along to the subprocess.
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "x", verbatim_doc_comment, after_long_help = AFTER_LONG_HELP.as_str())]
pub struct Exec {
    /// runtime(s) to start
    ///
    /// e.g.: nodejs@20 python@3.10
    #[clap(value_parser = RuntimeArgParser)]
    runtime: Vec<RuntimeArg>,

    /// the command string to execute (same as --command)
    #[clap(conflicts_with = "c", required_unless_present = "c", last = true)]
    command: Option<Vec<OsString>>,

    /// the command string to execute
    #[clap(short, long = "command", conflicts_with = "command")]
    c: Option<OsString>,
}

impl Command for Exec {
    fn run(self, config: Config, _out: &mut Output) -> Result<()> {
        let ts = ToolsetBuilder::new()
            .with_args(&self.runtime)
            .with_install_missing()
            .build(&config);

        let (program, args) = parse_command(&env::SHELL, self.command, self.c);
        let mut env = ts.env();
        env.insert("PATH".into(), ts.path_env());

        exec(program, args, env)
    }
}

#[cfg(not(test))]
fn exec<T, U, E>(program: T, args: U, env: IndexMap<E, E>) -> Result<()>
where
    T: IntoExecutablePath,
    U: IntoIterator,
    U::Item: Into<OsString>,
    E: AsRef<OsStr>,
{
    for (k, v) in env.iter() {
        env::set_var(k, v);
    }
    let args = args.into_iter().map(Into::into).collect::<Vec<_>>();
    let program = program.to_executable();
    let err = exec::Command::new(program.clone()).args(&args).exec();
    Err(eyre!("{:?} {}", program.to_string_lossy(), err.to_string()))
}

#[cfg(test)]
fn exec<T, U, E>(program: T, args: U, env: IndexMap<E, E>) -> Result<()>
where
    T: IntoExecutablePath,
    U: IntoIterator,
    U::Item: Into<OsString>,
    E: AsRef<OsStr>,
{
    let mut cmd = cmd::cmd(program, args);
    for (k, v) in env.iter() {
        cmd = cmd.env(k, v);
    }
    let res = cmd.unchecked().run()?;
    match res.status.code().unwrap_or(1) {
        0 => Ok(()),
        code => Err(eyre!("command failed with exit code {}", code)),
    }
}

fn parse_command(
    shell: &str,
    command: Option<Vec<OsString>>,
    c: Option<OsString>,
) -> (OsString, Vec<OsString>) {
    match (&command, &c) {
        (Some(command), _) => {
            let (program, args) = command.split_first().unwrap();

            (program.clone(), args.into())
        }
        _ => (shell.into(), vec!["-c".into(), c.unwrap()]),
    }
}

static COLOR: Lazy<Color> = Lazy::new(|| Color::new(Stream::Stdout));
static AFTER_LONG_HELP: Lazy<String> = Lazy::new(|| {
    formatdoc! {r#"
    {}
      rtx exec nodejs@20 -- node ./app.js  # launch app.js using node-20.x
      rtx x nodejs@20 -- node ./app.js     # shorter alias

      # Specify command as a string:
      rtx exec nodejs@20 python@3.11 --command "node -v && python -V"
    "#, COLOR.header("Examples:")}
});

#[cfg(test)]
mod tests {
    use crate::assert_cli;
    use crate::cli::tests::cli_run;
    use test_log::test;

    #[test]
    fn test_exec_ok() {
        assert_cli!("exec", "--", "jq", "--version");
    }

    #[test]
    fn test_exec_fail() {
        let _ = cli_run(
            &vec!["rtx", "exec", "--", "jq", "-invalid"]
                .into_iter()
                .map(String::from)
                .collect::<Vec<String>>(),
        )
        .unwrap_err();
    }
}
