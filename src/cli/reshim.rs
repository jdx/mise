use eyre::Result;

use crate::config::Config;
use crate::shims;
use crate::toolset::ToolsetBuilder;

/// rebuilds the shim farm
///
/// This creates new shims in ~/.local/share/mise/shims for CLIs that have been added.
/// mise will try to do this automatically for commands like `npm i -g` but there are
/// other ways to install things (like using yarn or pnpm for node) that mise does
/// not know about and so it will be necessary to call this explicitly.
///
/// If you think mise should automatically call this for a particular command, please
/// open an issue on the mise repo. You can also setup a shell function to reshim
/// automatically (it's really fast so you don't need to worry about overhead):
///
/// npm() {
///   command npm "$@"
///   mise reshim
/// }
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Reshim {
    #[clap(hide = true)]
    pub plugin: Option<String>,
    #[clap(hide = true)]
    pub version: Option<String>,
}

impl Reshim {
    pub fn run(self) -> Result<()> {
        let config = Config::try_get()?;
        let ts = ToolsetBuilder::new().build(&config)?;

        shims::reshim(&ts)
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise reshim</bold>
    $ <bold>~/.local/share/mise/shims/node -v</bold>
    v20.0.0
"#
);
