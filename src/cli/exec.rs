use std::collections::HashMap;
use std::ffi::OsString;

use color_eyre::eyre::{eyre, Result};

use crate::cli::args::runtime::{RuntimeArg, RuntimeArgParser};
use crate::cli::command::Command;
//
#[cfg(test)]
use crate::cmd;
use crate::config::Config;
use crate::env;
use crate::output::Output;

/// execute a command with runtime(s) set
///
/// use this to avoid modifying the shell session or running ad-hoc commands with the rtx runtimes
/// set.
///
/// Runtimes will be loaded from .tool-versions, though they can be overridden with <RUNTIME> args
/// Note that only the plugin specified will be overriden, so if a `.tool-versions` file
/// includes "nodejs 20" but you run `rtx exec python@3.11`; it will still load nodejs@20.
///
/// The "--" separates runtimes from the commands to pass along to the subprocess.
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "x", verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
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
        let config = config.with_runtime_args(&self.runtime)?;
        config.ensure_installed()?;

        let (program, args) = parse_command(&env::SHELL, self.command, self.c);

        exec(program, args, config.env()?)
    }
}

#[cfg(not(test))]
fn exec(program: OsString, args: Vec<OsString>, env: HashMap<OsString, OsString>) -> Result<()> {
    for (k, v) in env.iter() {
        env::set_var(k, v);
    }
    let err = exec::Command::new(program.clone()).args(&args).exec();
    Err(eyre!("{:?} {}", program, err))
}

#[cfg(test)]
fn exec(program: OsString, args: Vec<OsString>, env: HashMap<OsString, OsString>) -> Result<()> {
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

const AFTER_LONG_HELP: &str = r#"
Examples:
  rtx exec nodejs@20 -- node ./app.js  # launch app.js using node-20.x
  rtx x nodejs@20 -- node ./app.js     # shorter alias

Specify command as a string:
  rtx exec nodejs@20 python@3.11 --command "node -v && python -V"
"#;

#[cfg(test)]
mod test {
    use crate::assert_cli;
    use crate::cli::test::cli_run;

    #[test]
    fn test_exec_ok() {
        assert_cli!("plugin", "a", "jq");
        assert_cli!("install");
        assert_cli!("exec", "--", "jq", "--version");
    }

    #[test]
    fn test_exec_fail() {
        assert_cli!("install");
        assert_cli!("install", "nodejs");
        let _ = cli_run(
            &vec!["rtx", "exec", "--", "node", "-e", "process.exit(1)"]
                .into_iter()
                .map(String::from)
                .collect::<Vec<String>>(),
        )
        .unwrap_err();
    }
}
