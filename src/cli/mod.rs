use clap::{FromArgMatches, Subcommand};
use color_eyre::Result;
use confique::Partial;
use once_cell::sync::Lazy;

use crate::cli::self_update::SelfUpdate;
use crate::config::{Config, SettingsPartial};

mod activate;
mod alias;
pub mod args;
mod asdf;
mod bin_paths;
mod cache;
mod completion;
mod config;
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
    Config(config::Config),
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
    pub fn run(self, config: &Config) -> Result<()> {
        match self {
            Self::Activate(cmd) => cmd.run(),
            Self::Alias(cmd) => cmd.run(),
            Self::Asdf(cmd) => cmd.run(config),
            Self::BinPaths(cmd) => cmd.run(config),
            Self::Cache(cmd) => cmd.run(),
            Self::Completion(cmd) => cmd.run(),
            Self::Config(cmd) => cmd.run(),
            Self::Current(cmd) => cmd.run(config),
            Self::Deactivate(cmd) => cmd.run(config),
            Self::Direnv(cmd) => cmd.run(config),
            Self::Doctor(cmd) => cmd.run(config),
            Self::Env(cmd) => cmd.run(config),
            Self::EnvVars(cmd) => cmd.run(config),
            Self::Exec(cmd) => cmd.run(config),
            Self::Global(cmd) => cmd.run(config),
            Self::HookEnv(cmd) => cmd.run(config),
            Self::Implode(cmd) => cmd.run(),
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
            Self::Settings(cmd) => cmd.run(),
            Self::Shell(cmd) => cmd.run(),
            Self::Sync(cmd) => cmd.run(),
            Self::Trust(cmd) => cmd.run(),
            Self::Uninstall(cmd) => cmd.run(config),
            Self::Upgrade(cmd) => cmd.run(config),
            Self::Use(cmd) => cmd.run(config),
            Self::Version(cmd) => cmd.run(),
            Self::Where(cmd) => cmd.run(config),
            Self::Which(cmd) => cmd.run(),

            #[cfg(feature = "clap_complete")]
            Self::RenderCompletion(cmd) => cmd.run(),

            #[cfg(debug_assertions)]
            Self::RenderHelp(cmd) => cmd.run(),

            #[cfg(feature = "clap_mangen")]
            Self::RenderMangen(cmd) => cmd.run(),
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
        static COMMAND: Lazy<clap::Command> = Lazy::new(|| {
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
        });
        COMMAND.clone()
    }

    pub fn run(self, args: &Vec<String>) -> Result<()> {
        debug!("{}", &args.join(" "));
        let config = Config::try_get()?;
        if args[1..] == ["-v"] {
            // normally this would be considered --verbose
            return version::Version {}.run();
        }
        let matches = self.command.get_matches_from(args);
        if let Some((command, sub_m)) = matches.subcommand() {
            if command == "self-update" {
                return SelfUpdate::from_arg_matches(sub_m)?.run();
            }
            external::execute(&config, command, sub_m, self.external_commands)?;
        }
        let cmd = Commands::from_arg_matches(&matches)?;
        cmd.run(&config)
    }

    pub fn settings(self, args: &Vec<String>) -> SettingsPartial {
        let mut s = SettingsPartial::empty();
        if let Ok(m) = self.command.try_get_matches_from(args) {
            if let Some(true) = m.get_one::<bool>("yes") {
                s.yes = Some(true);
            }
            if let Some(true) = m.get_one::<bool>("quiet") {
                s.quiet = Some(true);
            }
            if *m.get_one::<u8>("verbose").unwrap() > 0 {
                s.verbose = Some(true);
            }
        }
        // if let Some(true) = m.get_one::<bool>("trace") {
        //     s.log_level = Some(true);
        // }
        // TODO: log_level/debug/trace

        s
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
    use crate::dirs;

    pub fn grep(output: String, pattern: &str) -> String {
        output
            .split('\n')
            .find(|line| line.contains(pattern))
            .map(|line| line.to_string())
            .unwrap()
            .trim()
            .replace(dirs::HOME.to_string_lossy().as_ref(), "~")
    }
}
