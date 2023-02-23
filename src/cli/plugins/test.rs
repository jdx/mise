use std::ffi::{OsStr, OsString};

use color_eyre::eyre::{eyre, Result};
use console::style;
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

/// Install a plugin and runtime and run command with it
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "x", verbatim_doc_comment, after_long_help = AFTER_LONG_HELP.as_str())]
pub struct PluginsTest {
    /// Runtime(s) to start
    ///
    /// e.g.: nodejs@20 python@3.10
    #[clap(value_parser = RuntimeArgParser)]
    runtime: Vec<RuntimeArg>,

    /// Command string to execute (same as --command)
    #[clap(last = true)]
    command: Option<Vec<OsString>>,
}

impl Command for Test {
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

static AFTER_LONG_HELP: Lazy<String> = Lazy::new(|| {
    formatdoc! {r#"
    {}
      rtx test nodejs@latest -- node --version
    "#, style("Examples:").bold().underlined()}
});

#[cfg(test)]
mod tests {
    use test_log::test;

    use crate::assert_cli;
    use crate::cli::tests::cli_run;

    #[test]
    fn test_exec_ok() {
        assert_cli!("test", "nodejs@latest", "--", "node", "--version");
    }
}
