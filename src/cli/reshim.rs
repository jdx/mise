use color_eyre::eyre::{eyre, Result};
use console::style;
use indoc::formatdoc;
use once_cell::sync::Lazy;

use crate::cli::command::Command;
use crate::config::Config;

use crate::output::Output;
use crate::shims;
use crate::toolset::ToolsetBuilder;

/// [experimental] rebuilds the shim farm
///
/// this requires that the shims_dir is set
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP.as_str())]
pub struct Reshim {
    #[clap(hide = true)]
    pub plugin: Option<String>,
    #[clap(hide = true)]
    pub version: Option<String>,
}

impl Command for Reshim {
    fn run(self, mut config: Config, _out: &mut Output) -> Result<()> {
        let ts = ToolsetBuilder::new().build(&mut config)?;

        if !config.settings.experimental {
            err_experimental()?;
        }

        shims::reshim(&mut config, &ts)
    }
}

fn err_experimental() -> Result<()> {
    return Err(eyre!(formatdoc!(
        r#"
                rtx is not configured to use experimental features.
                Please set the `{}` setting to `true`.
                "#,
        style("experimental").yellow()
    )));
}

static AFTER_LONG_HELP: Lazy<String> = Lazy::new(|| {
    formatdoc! {r#"
    {}
      $ rtx settings set experimental true
      $ rtx settings set shims_dir ~/.rtx/shims
      $ rtx reshim
      $ ~/.rtx/shims/node -v
      v18.0.0
    "#, style("Examples:").bold().underlined()}
});
