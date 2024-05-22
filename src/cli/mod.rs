use clap::{FromArgMatches, Subcommand};
use color_eyre::Result;

use crate::config::Settings;
use crate::{logger, migrate, shims};

mod activate;
mod alias;
pub mod args;
mod asdf;
pub mod backends;
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
mod generate;
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
mod registry;
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
mod tasks;
mod trust;
mod uninstall;
mod unset;
mod upgrade;
mod usage;
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
    Backends(backends::Backends),
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
    Generate(generate::Generate),
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
    Registry(registry::Registry),
    Reshim(reshim::Reshim),
    Run(run::Run),
    SelfUpdate(self_update::SelfUpdate),
    Set(set::Set),
    Settings(settings::Settings),
    Shell(shell::Shell),
    Sync(sync::Sync),
    Tasks(tasks::Tasks),
    Trust(trust::Trust),
    Uninstall(uninstall::Uninstall),
    Unset(unset::Unset),
    Upgrade(upgrade::Upgrade),
    Usage(usage::Usage),
    Use(r#use::Use),
    Version(version::Version),
    Watch(watch::Watch),
    Where(r#where::Where),
    Which(which::Which),

    #[cfg(debug_assertions)]
    RenderHelp(render_help::RenderHelp),

    #[cfg(feature = "clap_mangen")]
    RenderMangen(render_mangen::RenderMangen),
}

impl Commands {
    pub async fn run(self) -> Result<()> {
        match self {
            Self::Activate(cmd) => cmd.run().await,
            Self::Alias(cmd) => cmd.run().await,
            Self::Asdf(cmd) => cmd.run().await,
            Self::Backends(cmd) => cmd.run().await,
            Self::BinPaths(cmd) => cmd.run().await,
            Self::Cache(cmd) => cmd.run().await,
            Self::Completion(cmd) => cmd.run().await,
            Self::Config(cmd) => cmd.run().await,
            Self::Current(cmd) => cmd.run().await,
            Self::Deactivate(cmd) => cmd.run().await,
            Self::Direnv(cmd) => cmd.run().await,
            Self::Doctor(cmd) => cmd.run().await,
            Self::Env(cmd) => cmd.run().await,
            Self::Exec(cmd) => cmd.run().await,
            Self::Generate(cmd) => cmd.run().await,
            Self::Global(cmd) => cmd.run().await,
            Self::HookEnv(cmd) => cmd.run().await,
            Self::HookNotFound(cmd) => cmd.run().await,
            Self::Implode(cmd) => cmd.run().await,
            Self::Install(cmd) => cmd.run().await,
            Self::Latest(cmd) => cmd.run().await,
            Self::Link(cmd) => cmd.run().await,
            Self::Local(cmd) => cmd.run().await,
            Self::Ls(cmd) => cmd.run().await,
            Self::LsRemote(cmd) => cmd.run().await,
            Self::Outdated(cmd) => cmd.run().await,
            Self::Plugins(cmd) => cmd.run().await,
            Self::Prune(cmd) => cmd.run().await,
            Self::Registry(cmd) => cmd.run().await,
            Self::Reshim(cmd) => cmd.run().await,
            Self::Run(cmd) => cmd.run().await,
            Self::SelfUpdate(cmd) => cmd.run().await,
            Self::Set(cmd) => cmd.run().await,
            Self::Settings(cmd) => cmd.run().await,
            Self::Shell(cmd) => cmd.run().await,
            Self::Sync(cmd) => cmd.run().await,
            Self::Tasks(cmd) => cmd.run().await,
            Self::Trust(cmd) => cmd.run().await,
            Self::Uninstall(cmd) => cmd.run().await,
            Self::Unset(cmd) => cmd.run().await,
            Self::Upgrade(cmd) => cmd.run().await,
            Self::Usage(cmd) => cmd.run().await,
            Self::Use(cmd) => cmd.run().await,
            Self::Version(cmd) => cmd.run().await,
            Self::Watch(cmd) => cmd.run().await,
            Self::Where(cmd) => cmd.run().await,
            Self::Which(cmd) => cmd.run().await,

            #[cfg(debug_assertions)]
            Self::RenderHelp(cmd) => cmd.run().await,

            #[cfg(feature = "clap_mangen")]
            Self::RenderMangen(cmd) => cmd.run().await,
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
                .arg(args::CdArg::arg())
                .arg(args::DebugArg::arg())
                .arg(args::LogLevelArg::arg())
                .arg(args::QuietArg::arg())
                .arg(args::TraceArg::arg())
                .arg(args::VerboseArg::arg())
                .arg(args::YesArg::arg()),
        )
    }

    pub async fn run(args: &Vec<String>) -> Result<()> {
        crate::env::ARGS.write().unwrap().clone_from(args);
        shims::handle_shim().await?;
        version::print_version_if_requested(args)?;

        let matches = Self::command()
            .try_get_matches_from(args)
            .unwrap_or_else(|_| {
                Self::command()
                    .subcommands(external::commands())
                    .get_matches_from(args)
            });
        Settings::add_cli_matches(&matches);
        logger::init();
        migrate::run();
        debug!("ARGS: {}", &args.join(" "));
        match Commands::from_arg_matches(&matches) {
            Ok(cmd) => cmd.run().await,
            Err(err) => matches
                .subcommand()
                .ok_or(err)
                .map(|(command, sub_m)| external::execute(&command.into(), sub_m))?,
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
    $ <bold>mise install node@20</bold>           Install a version matching a prefix
    $ <bold>mise install node</bold>              Install the node version defined in config
    $ <bold>mise install</bold>                   Install all plugins/tools defined in config

    $ <bold>mise install cargo:ripgrep            Install something via cargo
    $ <bold>mise install npm:prettier             Install something via npm

    $ <bold>mise use node@20</bold>               Use node-20.x in current project
    $ <bold>mise use -g node@20</bold>            Use node-20.x as default
    $ <bold>mise use node@latest</bold>           Use latest node in current directory
    $ <bold>mise use -g node@system</bold>        Use system node everywhere unless overridden

    $ <bold>mise up --interactive</bold>          Show a menu to upgrade tools

    $ <bold>mise x -- npm install</bold>          `npm install` w/ config loaded into PATH
    $ <bold>mise x node@20 -- node app.js</bold>  `node app.js` w/ config + node-20.x on PATH

    $ <bold>mise set NODE_ENV=production</bold>   Set NODE_ENV=production in config

    $ <bold>mise run build</bold>                 Run `build` tasks
    $ <bold>mise watch build</bold>               Run `build` tasks repeatedly when files change

    $ <bold>mise settings</bold>                  Show settings in use
    $ <bold>mise settings set color 0</bold>      Disable color by modifying global config file
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
            .unwrap_or_else(|| panic!("pattern not found: {}", pattern))
            .trim()
            .replace(dirs::HOME.to_string_lossy().as_ref(), "~")
    }
}
