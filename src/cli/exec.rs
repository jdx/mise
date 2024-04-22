use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};

use clap::ValueHint;
use duct::IntoExecutablePath;
use eyre::Result;

use crate::cli::args::ToolArg;
#[cfg(test)]
use crate::cmd;
use crate::config::Config;
use crate::env;
use crate::toolset::{InstallOptions, ToolsetBuilder};

/// Execute a command with tool(s) set
///
/// use this to avoid modifying the shell session or running ad-hoc commands with mise tools set.
///
/// Tools will be loaded from .mise.toml/.tool-versions, though they can be overridden with <RUNTIME> args
/// Note that only the plugin specified will be overridden, so if a `.tool-versions` file
/// includes "node 20" but you run `mise exec python@3.11`; it will still load node@20.
///
/// The "--" separates runtimes from the commands to pass along to the subprocess.
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "x", verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Exec {
    /// Tool(s) to start
    /// e.g.: node@20 python@3.10
    #[clap(value_name = "TOOL@VERSION")]
    pub tool: Vec<ToolArg>,

    /// Command string to execute (same as --command)
    #[clap(conflicts_with = "c", required_unless_present = "c", last = true)]
    pub command: Option<Vec<OsString>>,

    /// Command string to execute
    #[clap(short, long = "command", value_hint = ValueHint::CommandString, conflicts_with = "command")]
    pub c: Option<OsString>,

    /// Number of jobs to run in parallel
    /// [default: 4]
    #[clap(long, short, env = "MISE_JOBS", verbatim_doc_comment)]
    pub jobs: Option<usize>,

    /// Directly pipe stdin/stdout/stderr from plugin to user
    /// Sets --jobs=1
    #[clap(long, overrides_with = "jobs")]
    pub raw: bool,
}

impl Exec {
    pub fn run(self) -> Result<()> {
        let config = Config::try_get()?;
        let mut ts = ToolsetBuilder::new()
            .with_args(&self.tool)
            .with_default_to_latest(true)
            .build(&config)?;
        let opts = InstallOptions {
            force: false,
            jobs: self.jobs,
            raw: self.raw,
            latest_versions: false,
        };
        ts.install_arg_versions(&config, &opts)?;
        ts.notify_if_versions_missing();

        let (program, args) = parse_command(&env::SHELL, &self.command, &self.c);
        let env = ts.env_with_path(&config)?;

        self.exec(program, args, env)
    }

    #[cfg(not(test))]
    fn exec<T, U, E>(&self, program: T, args: U, env: BTreeMap<E, E>) -> Result<()>
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
        bail!("{:?} {err}", program.to_string_lossy())
    }

    #[cfg(test)]
    fn exec<T, U, E>(&self, program: T, args: U, env: BTreeMap<E, E>) -> Result<()>
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
        match res.status.code() {
            Some(0) => Ok(()),
            Some(code) => Err(eyre!("command failed: exit code {}", code)),
            None => Err(eyre!("command failed: terminated by signal")),
        }
    }
}

fn parse_command(
    shell: &str,
    command: &Option<Vec<OsString>>,
    c: &Option<OsString>,
) -> (OsString, Vec<OsString>) {
    match (&command, &c) {
        (Some(command), _) => {
            let (program, args) = command.split_first().unwrap();

            (program.clone(), args.into())
        }
        _ => (shell.into(), vec!["-c".into(), c.clone().unwrap()]),
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise exec node@20 -- node ./app.js</bold>  # launch app.js using node-20.x
    $ <bold>mise x node@20 -- node ./app.js</bold>     # shorter alias

    # Specify command as a string:
    $ <bold>mise exec node@20 python@3.11 --command "node -v && python -V"</bold>

    # Run a command in a different directory:
    $ <bold>mise x -C /path/to/project node@20 -- node ./app.js</bold>
"#
);

#[cfg(test)]
mod tests {
    use std::env;

    #[test]
    fn test_exec_ok() {
        assert_cli!("exec", "--", "echo");
    }

    #[test]
    fn test_exec_fail() {
        let err = assert_cli_err!("exec", "--", "exit", "1");
        assert_snapshot!(err);
    }

    #[test]
    fn test_exec_cd() {
        let cwd = env::current_dir().unwrap();
        assert_cli!("exec", "-C", "/tmp", "--", "pwd");
        env::set_current_dir(cwd).unwrap();
    }
}
