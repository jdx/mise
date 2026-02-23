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
use crate::prepare::{PrepareEngine, PrepareOptions};
use crate::toolset::env_cache::CachedEnv;
use crate::toolset::{InstallOptions, ResolveOptions, ToolsetBuilder};

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

    /// Bypass the environment cache and recompute the environment
    #[clap(long)]
    pub fresh_env: bool,

    /// Skip automatic dependency preparation
    #[clap(long)]
    pub no_prepare: bool,

    /// Directly pipe stdin/stdout/stderr from plugin to user
    /// Sets --jobs=1
    #[clap(long, overrides_with = "jobs")]
    pub raw: bool,
}

impl Exec {
    #[async_backtrace::framed]
    pub async fn run(self) -> eyre::Result<()> {
        // Temporarily unset cache key to force fresh env computation
        if self.fresh_env {
            env::reset_env_cache_key();
        }

        let mut config = Config::get().await?;

        // Check if any tool arg explicitly specified @latest
        // If so, resolve to the actual latest version from the registry (not just latest installed)
        let has_explicit_latest = self
            .tool
            .iter()
            .any(|t| t.tvr.as_ref().is_some_and(|tvr| tvr.version() == "latest"));

        let resolve_options = if has_explicit_latest {
            ResolveOptions {
                latest_versions: true,
                use_locked_version: false,
                ..Default::default()
            }
        } else {
            Default::default()
        };

        let mut ts = measure!("toolset", {
            ToolsetBuilder::new()
                .with_args(&self.tool)
                .with_default_to_latest(true)
                .with_resolve_options(resolve_options.clone())
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
                || *env::__MISE_SHIM,
            skip_auto_install: !Settings::get().exec_auto_install || !Settings::get().auto_install,
            resolve_options,
            ..Default::default()
        };
        let (_, missing) = measure!("install_arg_versions", {
            ts.install_missing_versions(&mut config, &opts).await?
        });

        // If we installed new versions for explicit @latest, re-resolve to pick up the installed versions
        if has_explicit_latest {
            ts.resolve_with_opts(&config, &opts.resolve_options).await?;
        }

        measure!("notify_if_versions_missing", {
            ts.notify_missing_versions(missing);
        });

        let (program, mut args) = parse_command(&env::SHELL, &self.command, &self.c);

        // On Unix, resolve the program to its full mise-installed path if it is
        // provided by a mise-managed tool. This prevents infinite recursion when
        // a wrapper script (e.g., .devcontainer/bin/tool) calls `mise x -- tool`:
        // without this, execvp would find the wrapper again (since it precedes
        // the mise install path in PATH) and loop until E2BIG.
        // On Windows, exec_program uses which::which_in with PATHEXT to resolve
        // the correct executable (.cmd/.exe), so we skip this to avoid passing
        // a pre-resolved path that bypasses Windows-specific extension handling.
        #[cfg(unix)]
        let program = if !program.contains('/') {
            if let Some(bin) = ts.which_bin(&config, &program).await {
                bin.to_string_lossy().to_string()
            } else {
                program
            }
        } else {
            program
        };

        let mut env = measure!("env_with_path", { ts.env_with_path(&config).await? });

        // Run auto-enabled prepare steps (unless --no-prepare)
        if !self.no_prepare {
            let engine = PrepareEngine::new(&config)?;
            engine
                .run(PrepareOptions {
                    auto_only: true, // Only run providers with auto=true
                    env: env.clone(),
                    ..Default::default()
                })
                .await?;
        }

        // Ensure MISE_ENV is set in the spawned shell if it was specified via -E flag
        if !env::MISE_ENV.is_empty() {
            env.insert("MISE_ENV".to_string(), env::MISE_ENV.join(","));
        }

        // Ensure cache key is propagated to subprocesses for env caching
        if Settings::get().env_cache && !self.fresh_env {
            let key = CachedEnv::ensure_encryption_key();
            env.insert("__MISE_ENV_CACHE_KEY".to_string(), key);
        }

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
        exec_program(program, args, env)
    }
}

#[cfg(all(not(test), unix))]
pub fn exec_program<T, U>(program: T, args: U, env: BTreeMap<String, String>) -> Result<()>
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
    // Strip shims directory from PATH for program resolution only, to prevent
    // recursive shim execution. Wrapper scripts may call `mise x -- tool`,
    // which re-enters Exec. If shims remain in PATH (due to
    // not_found_auto_install), the wrapper is found again instead of the real
    // tool, causing an infinite loop that grows PATH until E2BIG.
    // The child process still inherits the full PATH (with shims) so
    // subprocesses can find tools via shims.
    let program = if program.to_string_lossy().contains('/') {
        // Already a path, no need to resolve
        program
    } else {
        let cwd = crate::dirs::CWD.clone().unwrap_or_default();
        let lookup_path = env.get(&*env::PATH_KEY).map(|path_val| {
            let shims_dir = &*crate::dirs::SHIMS;
            let filtered: Vec<_> = std::env::split_paths(&OsString::from(path_val))
                .filter(|p| p != shims_dir)
                .collect();
            std::env::join_paths(&filtered).unwrap()
        });
        match which::which_in(&program, lookup_path, cwd) {
            Ok(resolved) => resolved.into_os_string(),
            Err(_) => program, // Fall back to original if resolution fails
        }
    };
    let err = exec::Command::new(program.clone()).args(&args).exec();
    bail!("{:?} {err}", program.to_string_lossy())
}

#[cfg(all(windows, not(test)))]
pub fn exec_program<T, U>(program: T, args: U, env: BTreeMap<String, String>) -> Result<()>
where
    T: IntoExecutablePath,
    U: IntoIterator,
    U::Item: Into<OsString>,
{
    for (k, v) in env.iter() {
        env::set_var(k, v);
    }
    let cwd = crate::dirs::CWD.clone().unwrap_or_default();
    let program = program.to_executable();
    // Strip shims directory from PATH for program resolution only, to prevent
    // recursive shim execution. On Windows, "file" mode shim scripts call
    // `mise x -- tool`, which re-enters Exec. If shims remain in PATH (due to
    // not_found_auto_install), which::which_in resolves "tool" back to the shim,
    // causing an infinite loop. The child process still inherits the full PATH
    // (with shims) so subprocesses can find tools via shims.
    let lookup_path = env.get(&*env::PATH_KEY).map(|path_val| {
        // Compare with ~ expansion, normalized separators, and case-insensitive
        // to handle Windows path variations (e.g. ~/.local/share/mise\shims vs
        // C:\Users\user\.local\share\mise\shims)
        let shims_normalized = crate::dirs::SHIMS
            .to_string_lossy()
            .to_lowercase()
            .replace('/', "\\");
        let filtered: Vec<_> = std::env::split_paths(&OsString::from(path_val))
            .filter(|p| {
                let expanded = crate::file::replace_path(p);
                expanded.to_string_lossy().to_lowercase().replace('/', "\\") != shims_normalized
            })
            .collect();
        std::env::join_paths(&filtered).unwrap()
    });
    let program = which::which_in(program, lookup_path, cwd)?;
    let cmd = cmd::cmd(program, args);

    // Windows does not support exec in the same way as Unix,
    // so we emulate it instead by not handling Ctrl-C and letting
    // the child process deal with it instead.
    win_exec::set_ctrlc_handler()?;

    let res = cmd.unchecked().run()?;
    match res.status.code() {
        Some(code) => {
            std::process::exit(code);
        }
        None => Err(eyre!("command failed: terminated by signal")),
    }
}

#[cfg(test)]
pub fn exec_program<T, U>(program: T, args: U, env: BTreeMap<String, String>) -> Result<()>
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
        _ => (
            shell.into(),
            vec![env::SHELL_COMMAND_FLAG.into(), c.clone().unwrap()],
        ),
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
