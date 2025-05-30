use std::collections::BTreeMap;
use std::ffi::OsString;

use clap::ValueHint;
use duct::IntoExecutablePath;
#[cfg(not(any(test, windows)))]
use eyre::{Result, bail};
#[cfg(any(test, windows))]
use eyre::{Result, eyre};

use crate::cli::args::ToolArg;
#[cfg(any(test, windows))]
use crate::cmd;
use crate::config::{Config, Settings};
use crate::env;
use crate::toolset::{InstallOptions, ToolsetBuilder};

/// Execute a command with tool(s) set
///
/// use this to avoid modifying the shell session or running ad-hoc commands with mise tools set.
///
/// Tools will be loaded from mise.toml, though they can be overridden with <RUNTIME> args
/// Note that only the plugin specified will be overridden, so if a `mise.toml` file
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
    pub command: Option<Vec<String>>,

    /// Command string to execute
    #[clap(short, long = "command", value_hint = ValueHint::CommandString, conflicts_with = "command")]
    pub c: Option<String>,

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
    #[async_backtrace::framed]
    pub async fn run(self) -> eyre::Result<()> {
        let mut config = Config::get().await?;
        let mut ts = measure!("toolset", {
            ToolsetBuilder::new()
                .with_args(&self.tool)
                .with_default_to_latest(true)
                .build(&config)
                .await?
        });
        let opts = InstallOptions {
            force: false,
            jobs: self.jobs,
            raw: self.raw,
            // prevent installing things in shims by checking for tty
            // also don't autoinstall if at least 1 tool is specified
            // in that case the user probably just wants that one tool
            missing_args_only: !self.tool.is_empty()
                || !Settings::get().exec_auto_install
                || !console::user_attended_stderr()
                || *env::__MISE_SHIM,
            resolve_options: Default::default(),
            ..Default::default()
        };
        measure!("install_arg_versions", {
            ts.install_missing_versions(&mut config, &opts).await?
        });
        measure!("notify_if_versions_missing", {
            ts.notify_if_versions_missing(&config).await;
        });

        let (program, mut args) = parse_command(&env::SHELL, &self.command, &self.c);
        let env = measure!("env_with_path", { ts.env_with_path(&config).await? });

        if program.rsplit('/').next() == Some("fish") {
            let mut cmd = vec![];
            for (k, v) in env.iter().filter(|(k, _)| *k != "PATH") {
                cmd.push(format!(
                    "set -gx {} {}",
                    shell_escape::escape(k.into()),
                    shell_escape::escape(v.into())
                ));
            }
            // TODO: env is being calculated twice with final_env and env_with_path
            let (_, env_results) = ts.final_env(&config).await?;
            for p in ts.list_final_paths(&config, env_results).await? {
                cmd.push(format!(
                    "fish_add_path -gm {}",
                    shell_escape::escape(p.to_string_lossy())
                ));
            }
            args.insert(0, cmd.join("\n"));
            args.insert(0, "-C".into());
        }

        time!("exec");
        self.exec(program, args, env)
    }

    #[cfg(all(not(test), unix))]
    fn exec<T, U>(&self, program: T, args: U, env: BTreeMap<String, String>) -> Result<()>
    where
        T: IntoExecutablePath,
        U: IntoIterator,
        U::Item: Into<OsString>,
    {
        for (k, v) in env.iter() {
            env::set_var(k, v);
        }
        let args = args.into_iter().map(Into::into).collect::<Vec<_>>();
        let program = program.to_executable();
        let err = exec::Command::new(program.clone()).args(&args).exec();
        bail!("{:?} {err}", program.to_string_lossy())
    }

    #[cfg(all(windows, not(test)))]
    fn exec<T, U>(&self, program: T, args: U, env: BTreeMap<String, String>) -> Result<()>
    where
        T: IntoExecutablePath,
        U: IntoIterator,
        U::Item: Into<OsString>,
    {
        let cwd = crate::dirs::CWD.clone().unwrap_or_default();
        let program = program.to_executable();
        let path = env.get(&*env::PATH_KEY).map(OsString::from);
        let program = which::which_in(program, path, cwd)?;
        let mut cmd = cmd::cmd(program, args);
        for (k, v) in env.iter() {
            cmd = cmd.env(k, v);
        }

        // Windows does not support exec in the same way as Unix,
        // so we emulate it instead by not handling Ctrl-C and letting
        // the child process deal with it instead.
        win_exec::set_ctrlc_handler()?;

        let res = cmd.unchecked().run()?;
        match res.status.code() {
            Some(0) => Ok(()),
            Some(code) => Err(eyre!("command failed: exit code {}", code)),
            None => Err(eyre!("command failed: terminated by signal")),
        }
    }

    #[cfg(test)]
    fn exec<T, U>(&self, program: T, args: U, env: BTreeMap<String, String>) -> Result<()>
    where
        T: IntoExecutablePath,
        U: IntoIterator,
        U::Item: Into<OsString>,
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

#[cfg(all(windows, not(test)))]
mod win_exec {
    use eyre::{Result, eyre};
    use winapi::shared::minwindef::{BOOL, DWORD, FALSE, TRUE};
    use winapi::um::consoleapi::SetConsoleCtrlHandler;
    // Windows way of creating a process is to just go ahead and pop a new process
    // with given program and args into existence. But in unix-land, it instead happens
    // in a two-step process where you first fork the process and then exec the new program,
    // essentially replacing the current process with the new one.
    // We use Windows API to set a Ctrl-C handler that does nothing, essentially attempting
    // to emulate the ctrl-c behavior by not handling it ourselves, and propagating it to
    // the child process to handle it instead.
    // This is the same way cargo does it in cargo run.
    unsafe extern "system" fn ctrlc_handler(_: DWORD) -> BOOL {
        // This is a no-op handler to prevent Ctrl-C from terminating the process.
        // It allows the child process to handle Ctrl-C instead.
        TRUE
    }

    pub(super) fn set_ctrlc_handler() -> Result<()> {
        if unsafe { SetConsoleCtrlHandler(Some(ctrlc_handler), TRUE) } == FALSE {
            Err(eyre!("Could not set Ctrl-C handler."))
        } else {
            Ok(())
        }
    }
}

fn parse_command(
    shell: &str,
    command: &Option<Vec<String>>,
    c: &Option<String>,
) -> (String, Vec<String>) {
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
