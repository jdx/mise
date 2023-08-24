use color_eyre::eyre::Result;

use crate::cli::command::Command;
use crate::config::Config;
use crate::output::Output;
use crate::shims;
use crate::toolset::ToolsetBuilder;

/// rebuilds the shim farm
///
/// This creates new shims in ~/.local/share/rtx/shims for CLIs that have been added.
/// rtx will try to do this automatically for commands like `npm i -g` but there are
/// other ways to install things (like using yarn or pnpm for node) that rtx does
/// not know about and so it will be necessary to call this explicitly.
///
/// If you think rtx should automatically call this for a particular command, please
/// open an issue on the rtx repo. You can also setup a shell function to reshim
/// automatically (it's really fast so you don't need to worry about overhead):
///
/// npm() {
///   command npm "$@"
///   rtx reshim
/// }
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Reshim {
    #[clap(hide = true)]
    pub plugin: Option<String>,
    #[clap(hide = true)]
    pub version: Option<String>,
}

impl Command for Reshim {
    fn run(self, mut config: Config, _out: &mut Output) -> Result<()> {
        let ts = ToolsetBuilder::new().build(&mut config)?;

        shims::reshim(&config, &ts)
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>
  $ <bold>rtx reshim</bold>
  $ <bold>~/.local/share/rtx/shims/node -v</bold>
  v20.0.0
"#
);
