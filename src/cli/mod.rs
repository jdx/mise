use crate::cli::self_update::SelfUpdate;
use clap::{FromArgMatches, Subcommand};
use color_eyre::Result;

use crate::config::Config;

mod activate;
mod alias;
pub mod args;
mod asdf;
mod bin_paths;
mod cache;
mod completion;
mod current;
mod deactivate;
mod direnv;
mod doctor;
mod env;
mod env_vars;
pub mod exec;
mod external;
mod global;
mod hook_env;
mod implode;
mod install;
mod latest;
mod link;
mod local;
mod ls;
mod ls_remote;
mod outdated;
mod plugins;
mod prune;
#[cfg(feature = "clap_complete")]
mod render_completion;
#[cfg(debug_assertions)]
mod render_help;
#[cfg(feature = "clap_mangen")]
mod render_mangen;
mod reshim;
mod self_update;
mod settings;
mod shell;
mod sync;
mod trust;
mod uninstall;
mod upgrade;
mod r#use;
pub mod version;
mod r#where;
mod r#which;

pub struct Cli {
    command: clap::Command,
    external_commands: Vec<clap::Command>,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    Activate(activate::Activate),
    Alias(alias::Alias),
    Asdf(asdf::Asdf),
    BinPaths(bin_paths::BinPaths),
    Cache(cache::Cache),
    Completion(completion::Completion),
    Current(current::Current),
    Deactivate(deactivate::Deactivate),
    Direnv(direnv::Direnv),
    Doctor(doctor::Doctor),
    Env(env::Env),
    EnvVars(env_vars::EnvVars),
    Exec(exec::Exec),
    Global(global::Global),
    HookEnv(hook_env::HookEnv),
    Implode(implode::Implode),
    Install(install::Install),
    Latest(latest::Latest),
    Link(link::Link),
    Local(local::Local),
    Ls(ls::Ls),
    LsRemote(ls_remote::LsRemote),
    Outdated(outdated::Outdated),
    Plugins(plugins::Plugins),
    Prune(prune::Prune),
    Reshim(reshim::Reshim),
    Settings(settings::Settings),
    Shell(shell::Shell),
    Sync(sync::Sync),
    Trust(trust::Trust),
    Uninstall(uninstall::Uninstall),
    Upgrade(upgrade::Upgrade),
    Use(r#use::Use),
    Version(version::Version),
    Where(r#where::Where),
    Which(which::Which),

    #[cfg(feature = "clap_complete")]
    RenderCompletion(render_completion::RenderCompletion),

    #[cfg(debug_assertions)]
    RenderHelp(render_help::RenderHelp),

    #[cfg(feature = "clap_mangen")]
    RenderMangen(render_mangen::RenderMangen),
}

impl Commands {
    pub fn run(self, config: Config) -> Result<()> {
        match self {
            Self::Activate(cmd) => cmd.run(config),
            Self::Alias(cmd) => cmd.run(config),
            Self::Asdf(cmd) => cmd.run(config),
            Self::BinPaths(cmd) => cmd.run(config),
            Self::Cache(cmd) => cmd.run(config),
            Self::Completion(cmd) => cmd.run(config),
            Self::Current(cmd) => cmd.run(config),
            Self::Deactivate(cmd) => cmd.run(config),
            Self::Direnv(cmd) => cmd.run(config),
            Self::Doctor(cmd) => cmd.run(config),
            Self::Env(cmd) => cmd.run(config),
            Self::EnvVars(cmd) => cmd.run(config),
            Self::Exec(cmd) => cmd.run(config),
            Self::Global(cmd) => cmd.run(config),
            Self::HookEnv(cmd) => cmd.run(config),
            Self::Implode(cmd) => cmd.run(config),
            Self::Install(cmd) => cmd.run(config),
            Self::Latest(cmd) => cmd.run(config),
            Self::Link(cmd) => cmd.run(config),
            Self::Local(cmd) => cmd.run(config),
            Self::Ls(cmd) => cmd.run(config),
            Self::LsRemote(cmd) => cmd.run(config),
            Self::Outdated(cmd) => cmd.run(config),
            Self::Plugins(cmd) => cmd.run(config),
            Self::Prune(cmd) => cmd.run(config),
            Self::Reshim(cmd) => cmd.run(config),
            Self::Settings(cmd) => cmd.run(config),
            Self::Shell(cmd) => cmd.run(config),
            Self::Sync(cmd) => cmd.run(config),
            Self::Trust(cmd) => cmd.run(config),
            Self::Uninstall(cmd) => cmd.run(config),
            Self::Upgrade(cmd) => cmd.run(config),
            Self::Use(cmd) => cmd.run(config),
            Self::Version(cmd) => cmd.run(config),
            Self::Where(cmd) => cmd.run(config),
            Self::Which(cmd) => cmd.run(config),

            #[cfg(feature = "clap_complete")]
            Self::RenderCompletion(cmd) => cmd.run(config),

            #[cfg(debug_assertions)]
            Self::RenderHelp(cmd) => cmd.run(config),

            #[cfg(feature = "clap_mangen")]
            Self::RenderMangen(cmd) => cmd.run(config),
        }
    }
}

impl Cli {
    pub fn new() -> Self {
        Self {
            command: Self::command(),
            external_commands: vec![],
        }
    }

    pub fn new_with_external_commands(config: &Config) -> Self {
        let mut external_commands = external::commands(config);
        if SelfUpdate::is_available() {
            external_commands.push(SelfUpdate::command());
        }
        Self {
            command: Self::command().subcommands(external_commands.clone()),
            external_commands,
        }
    }

    pub fn command() -> clap::Command {
        Commands::augment_subcommands(
            clap::Command::new("rtx")
                .version(version::VERSION.to_string())
                .about(env!("CARGO_PKG_DESCRIPTION"))
                .author("Jeff Dickey <@jdx>")
                .long_about(LONG_ABOUT)
                .arg_required_else_help(true)
                .subcommand_required(true)
                .after_long_help(AFTER_LONG_HELP)
                .arg(args::log_level::Debug::arg())
                .arg(args::log_level::LogLevel::arg())
                .arg(args::log_level::Trace::arg())
                .arg(args::quiet::Quiet::arg())
                .arg(args::verbose::Verbose::arg())
                .arg(args::yes::Yes::arg()),
        )
    }

    pub fn run(self, mut config: Config, args: &Vec<String>) -> Result<()> {
        debug!("{}", &args.join(" "));
        if args[1..] == ["-v"] {
            // normally this would be considered --verbose
            return version::Version {}.run(config);
        }
        let matches = self.command.get_matches_from(args);
        if let Some(true) = matches.get_one::<bool>("yes") {
            config.settings.yes = true;
        }
        if let Some(true) = matches.get_one::<bool>("quiet") {
            config.settings.quiet = true;
        }
        if *matches.get_one::<u8>("verbose").unwrap() > 0 {
            config.settings.verbose = true;
        }
        if config.settings.raw {
            config.settings.jobs = 1;
            config.settings.verbose = true;
        }
        if let Some((command, sub_m)) = matches.subcommand() {
            if command == "self-update" {
                return SelfUpdate::from_arg_matches(sub_m)?.run(config);
            }
            external::execute(&config, command, sub_m, self.external_commands)?;
        }
        let cmd = Commands::from_arg_matches(&matches)?;
        cmd.run(config)
    }
}

impl Default for Cli {
    fn default() -> Self {
        Self::new()
    }
}

const LONG_ABOUT: &str = indoc! {"
rtx is a tool for managing runtime versions. https://github.com/jdx/rtx

It's a replacement for tools like nvm, nodenv, rbenv, rvm, chruby, pyenv, etc.
that works for any language. It's also great for managing linters/tools like
jq and shellcheck.

It is inspired by asdf and uses asdf's plugin ecosystem under the hood:
https://asdf-vm.com/"};

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>
  $ <bold>rtx install node@20.0.0</bold>       Install a specific node version
  $ <bold>rtx install node@20.0</bold>         Install a version matching a prefix
  $ <bold>rtx install node</bold>              Install the node version defined in
                                  .tool-versions or .rtx.toml
  $ <bold>rtx use node@20</bold>               Use node-20.x in current project
  $ <bold>rtx use -g node@20</bold>            Use node-20.x as default
  $ <bold>rtx use node@latest</bold>           Use latest node in current directory
  $ <bold>rtx use -g node@system</bold>        Use system node everywhere unless overridden
  $ <bold>rtx x node@20 -- node app.js</bold>  Run `node app.js` with node-20.x on PATH
"#
);

#[cfg(test)]
pub mod tests {
    use color_eyre::{Section, SectionExt};

    use crate::dirs;
    use crate::env;
    use crate::output::tests::{STDERR, STDOUT};

    use super::*;

    pub fn cli_run(args: &Vec<String>) -> Result<()> {
        *env::ARGS.write().unwrap() = args.clone();
        STDOUT.lock().unwrap().clear();
        STDERR.lock().unwrap().clear();
        let config = Config::load()?;
        Cli::new_with_external_commands(&config)
            .run(config, args)
            .with_section(|| format!("{}", args.join(" ").header("Command:")))?;

        Ok(())
    }

    #[macro_export]
    macro_rules! assert_cli {
        ($($args:expr),+) => {{
            let args = &vec!["rtx".into(), $($args.into()),+];
            $crate::cli::tests::cli_run(args).unwrap();
            let output = $crate::output::tests::STDOUT.lock().unwrap().join("\n");
            console::strip_ansi_codes(&output).trim().to_string()
        }};
    }

    #[macro_export]
    macro_rules! assert_cli_snapshot {
        ($($args:expr),+) => {{
            let args = &vec!["rtx".into(), $($args.into()),+];
            $crate::cli::tests::cli_run(args).unwrap();
            let output = $crate::output::tests::STDOUT.lock().unwrap().join("\n");
            let output = console::strip_ansi_codes(&output.trim()).to_string();
            let output = output.replace($crate::dirs::HOME.to_string_lossy().as_ref(), "~");
            let output = $crate::test::replace_path(&output);
            insta::assert_snapshot!(output);
        }};
    }

    #[macro_export]
    macro_rules! assert_cli_snapshot_stderr {
        ($($args:expr),+) => {{
            let args = &vec!["rtx".into(), $($args.into()),+];
            $crate::cli::tests::cli_run(args).unwrap();
            let output = $crate::output::tests::STDERR.lock().unwrap().join("\n");
            let output = console::strip_ansi_codes(&output.trim()).to_string();
            let output = output.replace($crate::dirs::HOME.to_string_lossy().as_ref(), "~");
            let output = $crate::test::replace_path(&output);
            insta::assert_snapshot!(output);
        }};
    }

    #[macro_export]
    macro_rules! assert_cli_err {
        ($($args:expr),+) => {{
            let args = &vec!["rtx".into(), $($args.into()),+];
            $crate::cli::tests::cli_run(args).unwrap_err()
        }};
    }

    pub fn grep(output: String, pattern: &str) -> String {
        output
            .split('\n')
            .find(|line| line.contains(pattern))
            .map(|line| line.to_string())
            .unwrap()
            .replace(dirs::HOME.to_string_lossy().as_ref(), "~")
    }
}
