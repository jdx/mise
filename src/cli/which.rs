use color_eyre::eyre::{eyre, Result};
use console::style;
use indoc::formatdoc;
use once_cell::sync::Lazy;

use crate::cli::command::Command;
use crate::config::Config;

use crate::output::Output;
use crate::toolset::ToolsetBuilder;

/// shows the plugin that a bin points to
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP.as_str())]
pub struct Which {
    #[clap()]
    pub bin_name: String,
}

impl Command for Which {
    fn run(self, config: Config, _out: &mut Output) -> Result<()> {
        let ts = ToolsetBuilder::new().build(&config);

        if !config.settings.experimental {
            err_experimental()?;
        }

        match ts.which(&self.bin_name) {
            Some(rtv) => {
                println!("{}", rtv.which(&self.bin_name)?.unwrap().display());
                Ok(())
            }
            None => Err(eyre!("{} not found", self.bin_name)),
        }
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
      $ rtx which node
      /home/username/.local/share/rtx/installs/nodejs/18.0.0/bin/node
    "#, style("Examples:").bold().underlined()}
});
