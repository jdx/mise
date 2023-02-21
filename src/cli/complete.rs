use std::io::Cursor;

use clap_complete::generate;
use color_eyre::eyre::Result;
use console::style;
use indoc::formatdoc;
use once_cell::sync::Lazy;

use crate::cli::command::Command;
use crate::cli::Cli;
use crate::config::Config;
use crate::output::Output;

/// generate shell completions
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP.as_str())]
pub struct Complete {
    /// shell type
    #[clap(long, short)]
    shell: clap_complete::Shell,
}

impl Command for Complete {
    fn run(self, _config: Config, out: &mut Output) -> Result<()> {
        let mut c = Cursor::new(Vec::new());
        generate(self.shell, &mut Cli::command(), "rtx", &mut c);
        rtxprintln!(out, "{}", String::from_utf8(c.into_inner()).unwrap());

        Ok(())
    }
}

static AFTER_LONG_HELP: Lazy<String> = Lazy::new(|| {
    formatdoc! {r#"
    {}
      $ rtx complete
    "#, style("Examples:").bold().underlined()}
});

// #[cfg(test)]
// mod tests {
//     use std::fs;
//
//     use insta::assert_snapshot;
//
//     use crate::{assert_cli, dirs};
//
//     #[test]
//     fn test_complete() {
//         let stdout = assert_cli!("complete", "-s", "zsh");
//         assert_snapshot!(stdout);
//     }
// }
