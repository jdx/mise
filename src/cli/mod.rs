use clap::{FromArgMatches, Subcommand};
use color_eyre::Result;
use console::style;
use indoc::{formatdoc, indoc};
use log::LevelFilter;
use once_cell::sync::Lazy;

use crate::cli::command::Command;
use crate::config::Config;
use crate::config::MissingRuntimeBehavior::AutoInstall;
use crate::output::Output;

mod activate;
mod alias;
pub mod args;
mod asdf;
mod bin_paths;
mod cache;
pub mod command;
mod complete;
mod current;
mod deactivate;
mod direnv;
mod doctor;
mod env;
pub mod exec;
mod external;
mod global;
mod hook_env;
mod implode;
mod install;
mod latest;
mod local;
mod ls;
mod ls_remote;
#[cfg(feature = "clap_mangen")]
mod mangen;
mod plugins;
mod prune;
#[cfg(debug_assertions)]
mod render_help;
mod reshim;
mod self_update;
mod settings;
mod shell;
mod trust;
mod uninstall;
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
    Complete(complete::Complete),
    Current(current::Current),
    Deactivate(deactivate::Deactivate),
    Direnv(direnv::Direnv),
    Doctor(doctor::Doctor),
    Env(env::Env),
    Exec(exec::Exec),
    Global(global::Global),
    HookEnv(hook_env::HookEnv),
    Implode(implode::Implode),
    Install(install::Install),
    Latest(latest::Latest),
    Local(local::Local),
    Ls(ls::Ls),
    LsRemote(ls_remote::LsRemote),
    #[cfg(feature = "clap_mangen")]
    Mangen(mangen::Mangen),
    Plugins(plugins::Plugins),
    Prune(prune::Prune),
    Reshim(reshim::Reshim),
    SelfUpdate(self_update::SelfUpdate),
    Settings(settings::Settings),
    Shell(shell::Shell),
    Trust(trust::Trust),
    Uninstall(uninstall::Uninstall),
    Version(version::Version),
    Where(r#where::Where),
    Which(which::Which),

    #[cfg(debug_assertions)]
    RenderHelp(render_help::RenderHelp),
}

impl Commands {
    pub fn run(self, config: Config, out: &mut Output) -> Result<()> {
        match self {
            Self::Activate(cmd) => cmd.run(config, out),
            Self::Alias(cmd) => cmd.run(config, out),
            Self::Asdf(cmd) => cmd.run(config, out),
            Self::BinPaths(cmd) => cmd.run(config, out),
            Self::Cache(cmd) => cmd.run(config, out),
            Self::Complete(cmd) => cmd.run(config, out),
            Self::Current(cmd) => cmd.run(config, out),
            Self::Deactivate(cmd) => cmd.run(config, out),
            Self::Direnv(cmd) => cmd.run(config, out),
            Self::Doctor(cmd) => cmd.run(config, out),
            Self::Env(cmd) => cmd.run(config, out),
            Self::Exec(cmd) => cmd.run(config, out),
            Self::Global(cmd) => cmd.run(config, out),
            Self::HookEnv(cmd) => cmd.run(config, out),
            Self::Implode(cmd) => cmd.run(config, out),
            Self::Install(cmd) => cmd.run(config, out),
            Self::Latest(cmd) => cmd.run(config, out),
            Self::Local(cmd) => cmd.run(config, out),
            Self::Ls(cmd) => cmd.run(config, out),
            Self::LsRemote(cmd) => cmd.run(config, out),
            #[cfg(feature = "clap_mangen")]
            Self::Mangen(cmd) => cmd.run(config, out),
            Self::Plugins(cmd) => cmd.run(config, out),
            Self::Prune(cmd) => cmd.run(config, out),
            Self::Reshim(cmd) => cmd.run(config, out),
            Self::SelfUpdate(cmd) => cmd.run(config, out),
            Self::Settings(cmd) => cmd.run(config, out),
            Self::Shell(cmd) => cmd.run(config, out),
            Self::Trust(cmd) => cmd.run(config, out),
            Self::Uninstall(cmd) => cmd.run(config, out),
            Self::Version(cmd) => cmd.run(config, out),
            Self::Where(cmd) => cmd.run(config, out),
            Self::Which(cmd) => cmd.run(config, out),

            #[cfg(debug_assertions)]
            Self::RenderHelp(cmd) => cmd.run(config, out),
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
        let external_commands = external::commands(config);
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
                .author("Jeff Dickey <@jdxcode>")
                .long_about(LONG_ABOUT)
                .arg_required_else_help(true)
                .subcommand_required(true)
                .after_long_help(AFTER_LONG_HELP.as_str())
                .arg(args::log_level::Debug::arg())
                .arg(args::install_missing::InstallMissing::arg())
                .arg(args::jobs::Jobs::arg())
                .arg(args::log_level::LogLevel::arg())
                .arg(args::raw::Raw::arg())
                .arg(args::log_level::Trace::arg())
                .arg(args::verbose::Verbose::arg()),
        )
    }

    pub fn run(self, mut config: Config, args: &Vec<String>, out: &mut Output) -> Result<()> {
        debug!("{}", &args.join(" "));
        if args[1..] == ["-v"] {
            // normally this would be considered --verbose
            return version::Version {}.run(config, out);
        }
        let matches = self.command.get_matches_from(args);
        if let Some(log_level) = matches.get_one::<LevelFilter>("log-level") {
            config.settings.log_level = *log_level;
        }
        if let Some(true) = matches.get_one::<bool>("debug") {
            config.settings.log_level = LevelFilter::Debug;
        }
        if let Some(true) = matches.get_one::<bool>("trace") {
            config.settings.log_level = LevelFilter::Trace;
        }
        if let Some(jobs) = matches.get_one::<usize>("jobs") {
            config.settings.jobs = *jobs;
        }
        if let Some(raw) = matches.get_one::<bool>("raw") {
            config.settings.raw = *raw;
        }
        if let Some(true) = matches.get_one::<bool>("install-missing") {
            config.settings.missing_runtime_behavior = AutoInstall;
        }
        if *matches.get_one::<u8>("verbose").unwrap() > 0 {
            config.settings.verbose = true;
        }
        if config.settings.raw {
            config.settings.jobs = 1;
            config.settings.verbose = true;
        }
        if let Some((command, sub_m)) = matches.subcommand() {
            external::execute(&config, command, sub_m, self.external_commands)?;
        }
        Commands::from_arg_matches(&matches)?.run(config, out)
    }
}

const LONG_ABOUT: &str = indoc! {"
rtx is a tool for managing runtime versions. https://github.com/jdxcode/rtx

It's a replacement for tools like nvm, nodenv, rbenv, rvm, chruby, pyenv, etc.
that works for any language. It's also great for managing linters/tools like
jq and shellcheck.

It is inspired by asdf and uses asdf's plugin ecosystem under the hood:
https://asdf-vm.com/"};

static AFTER_LONG_HELP: Lazy<String> = Lazy::new(|| {
    formatdoc! { "
    {}
      rtx install nodejs@18.0.0       Install a specific node version
      rtx install nodejs@18.0         Install a version matching a prefix
      rtx install nodejs              Install the node version defined in
                                      .tool-versions or .rtx.toml
      rtx local nodejs@18             Use node-18.x in current project
      rtx global nodejs@18            Use node-18.x as default
      rtx local nodejs@latest         Use latest node in current directory
      rtx global nodejs@system        Use system node everywhere unless overridden
      rtx x nodejs@18 -- node app.js  Run `node app.js` with PATH pointing to
                                      node-18.x
", style("Examples:").bold().underlined()  }
});

#[cfg(test)]
pub mod tests {
    use crate::dirs;
    use color_eyre::{Section, SectionExt};

    use super::*;

    pub fn cli_run(args: &Vec<String>) -> Result<Output> {
        let config = Config::load()?;
        let mut out = Output::tracked();
        Cli::new_with_external_commands(&config)
            .run(config, args, &mut out)
            .with_section(|| format!("{}", args.join(" ").header("Command:")))?;

        Ok(out)
    }

    #[macro_export]
    macro_rules! assert_cli {
        ($($args:expr),+) => {{
            let args = &vec!["rtx".into(), $($args.into()),+];
            let output = $crate::cli::tests::cli_run(args).unwrap().stdout.content;
            console::strip_ansi_codes(&output).to_string()
        }};
    }

    #[macro_export]
    macro_rules! assert_cli_snapshot {
        ($($args:expr),+) => {{
            let args = &vec!["rtx".into(), $($args.into()),+];
            let output = $crate::cli::tests::cli_run(args).unwrap().stdout.content;
            let output = console::strip_ansi_codes(&output).to_string();
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
