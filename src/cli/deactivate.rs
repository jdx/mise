use color_eyre::eyre::Result;

use crate::cli::command::Command;
use crate::config::Config;
use crate::output::Output;
use crate::shell::{get_shell, ShellType};

/// disable rtx for current shell session
///
/// This can be used to temporarily disable rtx in a shell session.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Deactivate {
    /// shell type to generate the script for
    #[clap(long, short, hide = true)]
    shell: Option<ShellType>,

    /// shell type to generate the script for
    #[clap()]
    shell_type: Option<ShellType>,
}

impl Command for Deactivate {
    fn run(self, _config: Config, out: &mut Output) -> Result<()> {
        let shell = get_shell(self.shell_type.or(self.shell));

        let output = shell.deactivate();
        out.stdout.write(output);

        Ok(())
    }
}

const AFTER_LONG_HELP: &str = r#"
Examples:
    $ eval "$(rtx deactivate bash)"
    $ eval "$(rtx deactivate zsh)"
    $ rtx deactivate fish | source
    $ execx($(rtx deactivate xonsh))
"#;

#[cfg(test)]
mod test {
    use insta::assert_display_snapshot;

    use crate::assert_cli;

    #[test]
    fn test_deactivate_zsh() {
        std::env::set_var("NO_COLOR", "1");
        let stdout = assert_cli!("deactivate", "zsh");
        assert_display_snapshot!(stdout);
    }

    #[test]
    fn test_deactivate_zsh_legacy() {
        std::env::set_var("NO_COLOR", "1");
        let stdout = assert_cli!("deactivate", "-s", "zsh");
        assert_display_snapshot!(stdout);
    }
}
