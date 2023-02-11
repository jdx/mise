use atty::Stream;
use color_eyre::eyre::{eyre, Result};
use indoc::formatdoc;
use once_cell::sync::Lazy;

use crate::cli::command::Command;
use crate::config::Config;
use crate::env;
use crate::output::Output;
use crate::ui::color::Color;

/// Check rtx installation for possible problems.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP.as_str())]
pub struct Doctor {}

impl Command for Doctor {
    fn run(self, config: Config, _out: &mut Output) -> Result<()> {
        let mut checks = Vec::new();
        for plugin in config.ts.list_plugins() {
            if !plugin.is_installed() {
                checks.push(format!("plugin {} is not installed", plugin.name));
                continue;
            }
        }

        if env::var("__RTX_DIFF").is_err() {
            checks.push(
                "rtx is not activated, run `rtx help activate` for setup instructions".to_string(),
            );
        }

        for check in &checks {
            error!("{}", check);
        }

        if checks.is_empty() {
            Ok(())
        } else {
            Err(eyre!("{} problems found", checks.len()))
        }
    }
}

static COLOR: Lazy<Color> = Lazy::new(|| Color::new(Stream::Stdout));
static AFTER_LONG_HELP: Lazy<String> = Lazy::new(|| {
    formatdoc! {r#"
    {}
      $ rtx doctor
      [WARN] plugin nodejs is not installed
    "#, COLOR.header("Examples:")}
});

#[cfg(test)]
mod test {
    use crate::cli::test::cli_run;

    #[test]
    fn test_doctor() {
        let _ = cli_run(
            &vec!["rtx", "doctor"]
                .into_iter()
                .map(String::from)
                .collect::<Vec<String>>(),
        );
    }
}
