use clap::{FromArgMatches, Subcommand};
use miette::{IntoDiagnostic, Result};

use crate::config::{Config, Settings};
use crate::{logger, migrate, shims};

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
pub mod exec;
mod external;
mod global;
mod hook_env;
mod hook_not_found;
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
mod run;
mod self_update;
mod set;
mod settings;
mod shell;
mod sync;
mod task;
mod trust;
mod uninstall;
mod unset;
mod upgrade;
mod r#use;
pub mod version;
mod watch;
mod r#where;
mod r#which;

pub struct Cli {}

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
    Exec(exec::Exec),
    Global(global::Global),
    HookEnv(hook_env::HookEnv),
    HookNotFound(hook_not_found::HookNotFound),
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
    Run(run::Run),
    SelfUpdate(self_update::SelfUpdate),
    Set(set::Set),
    Settings(settings::Settings),
    Shell(shell::Shell),
    Sync(sync::Sync),
    Task(task::Task),
    Trust(trust::Trust),
    Uninstall(uninstall::Uninstall),
    Upgrade(upgrade::Upgrade),
    Unset(unset::Unset),
    Use(r#use::Use),
    Version(version::Version),
    Watch(watch::Watch),
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
    pub fn run(self) -> Result<()> {
        match self {
            Self::Activate(cmd) => cmd.run(),
            Self::Alias(cmd) => cmd.run(),
            Self::Asdf(cmd) => cmd.run(),
            Self::BinPaths(cmd) => cmd.run(),
            Self::Cache(cmd) => cmd.run(),
            Self::Completion(cmd) => cmd.run(),
            Self::Config(cmd) => cmd.run(),
            Self::Current(cmd) => cmd.run(),
            Self::Deactivate(cmd) => cmd.run(),
            Self::Direnv(cmd) => cmd.run(),
            Self::Doctor(cmd) => cmd.run(),
            Self::Env(cmd) => cmd.run(),
            Self::Exec(cmd) => cmd.run(),
            Self::Global(cmd) => cmd.run(),
            Self::HookEnv(cmd) => cmd.run(),
            Self::HookNotFound(cmd) => cmd.run(),
            Self::Implode(cmd) => cmd.run(),
            Self::Install(cmd) => cmd.run(),
            Self::Latest(cmd) => cmd.run(),
            Self::Link(cmd) => cmd.run(),
            Self::Local(cmd) => cmd.run(),
            Self::Ls(cmd) => cmd.run(),
            Self::LsRemote(cmd) => cmd.run(),
            Self::Outdated(cmd) => cmd.run(),
            Self::Plugins(cmd) => cmd.run(),
            Self::Prune(cmd) => cmd.run(),
            Self::Reshim(cmd) => cmd.run(),
            Self::Run(cmd) => cmd.run(),
            Self::SelfUpdate(cmd) => cmd.run(),
            Self::Set(cmd) => cmd.run(),
            Self::Settings(cmd) => cmd.run(),
            Self::Shell(cmd) => cmd.run(),
            Self::Sync(cmd) => cmd.run(),
            Self::Task(cmd) => cmd.run(),
            Self::Trust(cmd) => cmd.run(),
            Self::Uninstall(cmd) => cmd.run(),
            Self::Unset(cmd) => cmd.run(),
            Self::Upgrade(cmd) => cmd.run(),
            Self::Use(cmd) => cmd.run(),
            Self::Version(cmd) => cmd.run(),
            Self::Watch(cmd) => cmd.run(),
            Self::Where(cmd) => cmd.run(),
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
    pub fn command() -> clap::Command {
        Commands::augment_subcommands(
            clap::Command::new("mise")
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
                .arg(args::cd::Cd::arg())
                .arg(args::quiet::Quiet::arg())
                .arg(args::verbose::Verbose::arg())
                .arg(args::yes::Yes::arg()),
        )
    }

    pub fn run(args: &Vec<String>) -> Result<()> {
        *crate::env::ARGS.write().unwrap() = args.clone();
        shims::handle_shim()?;
        version::print_version_if_requested(args);

        let matches = Self::command()
            .try_get_matches_from(args)
            .unwrap_or_else(|_| {
                Self::command()
                    .subcommands(external::commands(Config::get().as_ref()))
                    .get_matches_from(args)
            });
        Settings::add_cli_matches(&matches);
        if let Ok(settings) = Settings::try_get() {
            logger::init(&settings);
        }
        migrate::run();
        debug!("ARGS: {}", &args.join(" "));
        match Commands::from_arg_matches(&matches) {
            Ok(cmd) => cmd.run(),
            Err(err) => matches
                .subcommand()
                .ok_or(err)
                .map(|(command, sub_m)| external::execute(command, sub_m))
                .into_diagnostic()?,
        }
    }
}

const LONG_ABOUT: &str = indoc! {"
mise is a tool for managing runtime versions. https://github.com/jdx/mise

It's a replacement for tools like nvm, nodenv, rbenv, rvm, chruby, pyenv, etc.
that works for any language. It's also great for managing linters/tools like
jq and shellcheck.

It is inspired by asdf and uses asdf's plugin ecosystem under the hood:
https://asdf-vm.com/"};

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>
  $ <bold>mise install node@20.0.0</bold>       Install a specific node version
  $ <bold>mise install node@20.0</bold>         Install a version matching a prefix
  $ <bold>mise install node</bold>              Install the node version defined in
                                  .tool-versions or .mise.toml
  $ <bold>mise use node@20</bold>               Use node-20.x in current project
  $ <bold>mise use -g node@20</bold>            Use node-20.x as default
  $ <bold>mise use node@latest</bold>           Use latest node in current directory
  $ <bold>mise use -g node@system</bold>        Use system node everywhere unless overridden
  $ <bold>mise x node@20 -- node app.js</bold>  Run `node app.js` with node-20.x on PATH
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
