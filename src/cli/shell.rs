use color_eyre::eyre::{eyre, Result};

use console::style;
use indoc::formatdoc;
use once_cell::sync::Lazy;

use crate::cli::args::runtime::{RuntimeArg, RuntimeArgParser};
use crate::cli::command::Command;
use crate::config::Config;
use crate::dirs;

use crate::file::touch_dir;

use crate::output::Output;
use crate::shell::get_shell;
use crate::toolset::{ToolSource, ToolsetBuilder};

/// Sets a tool version for the current shell session
///
/// Only works in a session where rtx is already activated.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP.as_str())]
pub struct Shell {
    /// Runtime version(s) to use
    #[clap(value_parser = RuntimeArgParser)]
    runtime: Vec<RuntimeArg>,
}

impl Command for Shell {
    fn run(self, mut config: Config, out: &mut Output) -> Result<()> {
        let ts = ToolsetBuilder::new()
            .with_install_missing()
            .with_args(&self.runtime)
            .build(&mut config)?;
        if !config.is_activated() {
            err_inactive()?;
        }
        let shell = get_shell(None).expect("no shell detected");

        for rtv in ts.list_current_installed_versions() {
            let source = &ts.versions.get(&rtv.plugin.name).unwrap().source;
            if matches!(source, ToolSource::Argument) {
                let k = format!("RTX_{}_VERSION", rtv.plugin.name.to_uppercase());
                rtxprintln!(out, "{}", shell.set_env(&k, &rtv.version));
            }
        }
        touch_dir(&dirs::ROOT)?;

        Ok(())
    }
}

fn err_inactive() -> Result<()> {
    return Err(eyre!(formatdoc!(
        r#"
                rtx is not activated in this shell session.
                Please run `{}` first in your shell rc file.
                "#,
        style("rtx activate").yellow()
    )));
}

static AFTER_LONG_HELP: Lazy<String> = Lazy::new(|| {
    formatdoc! {r#"
    {}
      $ rtx shell nodejs@18
      $ node -v
      v18.0.0
    "#, style("Examples:").bold().underlined()}
});

#[cfg(test)]
mod tests {
    use insta::assert_display_snapshot;
    use std::env;

    use crate::{assert_cli_err, assert_cli_snapshot};

    #[test]
    fn test_shell() {
        let err = assert_cli_err!("shell", "tiny@1.0.1");
        assert_display_snapshot!(err);
        env::set_var("__RTX_DIFF", "");
        env::set_var("RTX_SHELL", "zsh");
        assert_cli_snapshot!("shell", "tiny@1.0.1");
        env::remove_var("__RTX_DIFF");
        env::remove_var("RTX_SHELL");
    }
}
