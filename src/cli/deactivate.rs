use color_eyre::eyre::Result;
use console::style;
use indoc::formatdoc;
use once_cell::sync::Lazy;
use std::env::join_paths;

use crate::cli::command::Command;
use crate::config::Config;
use crate::env;
use crate::output::Output;
use crate::shell::{get_shell, ShellType};

/// Disable rtx for current shell session
///
/// This can be used to temporarily disable rtx in a shell session.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP.as_str())]
pub struct Deactivate {
    /// Shell type to generate the script for
    #[clap(long, short, hide = true)]
    shell: Option<ShellType>,

    /// Shell type to generate the script for
    #[clap()]
    shell_type: Option<ShellType>,
}

impl Command for Deactivate {
    fn run(self, _config: Config, out: &mut Output) -> Result<()> {
        let shell = get_shell(self.shell_type.or(self.shell))
            .expect("no shell provided, use `--shell=zsh`");

        let path = join_paths(&*env::PATH)?.to_string_lossy().to_string();
        let output = shell.deactivate(path);
        out.stdout.write(output);

        Ok(())
    }
}

static AFTER_LONG_HELP: Lazy<String> = Lazy::new(|| {
    formatdoc! {r#"
    {}
      $ eval "$(rtx deactivate bash)"
      $ eval "$(rtx deactivate zsh)"
      $ rtx deactivate fish | source
      $ execx($(rtx deactivate xonsh))
    "#, style("Examples:").bold().underlined()}
});

#[cfg(test)]
mod tests {

    use crate::assert_cli_snapshot;

    #[test]
    fn test_deactivate_zsh() {
        assert_cli_snapshot!("deactivate", "zsh");
    }

    #[test]
    fn test_deactivate_zsh_legacy() {
        assert_cli_snapshot!("deactivate", "-s", "zsh");
    }
}
