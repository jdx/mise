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

/// Generate shell completions
#[derive(Debug, clap::Args)]
#[clap(alias = "complete", verbatim_doc_comment, after_long_help = AFTER_LONG_HELP.as_str())]
pub struct Completion {
    /// Shell type to generate completions for
    #[clap()]
    shell_type: Option<clap_complete::Shell>,

    /// Shell type to generate completions for
    #[clap(long, short)]
    shell: Option<clap_complete::Shell>,
}

impl Command for Completion {
    fn run(self, _config: Config, out: &mut Output) -> Result<()> {
        let shell_type = self.shell_type.or(self.shell);
        let shell_type = match shell_type {
            Some(clap_complete::Shell::Bash) => clap_complete::Shell::Bash,
            Some(clap_complete::Shell::Zsh) => clap_complete::Shell::Zsh,
            Some(clap_complete::Shell::Fish) => clap_complete::Shell::Fish,
            _ => clap_complete::Shell::Zsh,
        };

        let mut c = Cursor::new(Vec::new());
        generate(shell_type, &mut Cli::command(), "rtx", &mut c);
        rtxprintln!(out, "{}", String::from_utf8(c.into_inner()).unwrap());

        Ok(())
    }
}

static AFTER_LONG_HELP: Lazy<String> = Lazy::new(|| {
    formatdoc! {r#"
    {}
      $ rtx completion -s bash > /etc/bash_completion.d/rtx
      $ rtx completion -s zsh  > /usr/local/share/zsh/site-functions/_rtx
      $ rtx completion -s fish > ~/.config/fish/completions/rtx.fish
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
//     fn test_completion() {
//         let stdout = assert_cli!("completion", "-s", "zsh");
//         assert_snapshot!(stdout);
//     }
// }
