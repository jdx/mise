use color_eyre::eyre::{eyre, Result};
use console::style;
use indoc::formatdoc;
use once_cell::sync::Lazy;

use crate::cli;
use crate::cli::command::Command;
use crate::config::Config;
use crate::env;
use crate::output::Output;

/// Check rtx installation for possible problems.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP.as_str())]
pub struct Doctor {}

impl Command for Doctor {
    fn run(self, config: Config, _out: &mut Output) -> Result<()> {
        let mut checks = Vec::new();
        for plugin in config.plugins.values() {
            if !plugin.is_installed() {
                checks.push(format!("plugin {} is not installed", plugin.name));
                continue;
            }
        }

        if let Some(latest) = cli::version::check_for_new_version() {
            warn!(
                "new rtx version {} available, currently on {}",
                latest,
                env!("CARGO_PKG_VERSION")
            )
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

static AFTER_LONG_HELP: Lazy<String> = Lazy::new(|| {
    formatdoc! {r#"
    {}
      $ rtx doctor
      [WARN] plugin nodejs is not installed
    "#, style("Examples:").bold().underlined()}
});

#[cfg(test)]
mod tests {
    use crate::cli::tests::cli_run;

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
