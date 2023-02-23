use color_eyre::eyre::Result;
use console::style;
use indoc::formatdoc;
use once_cell::sync::Lazy;

use crate::cli::command::Command;
use crate::config::Config;
use crate::env;
use crate::output::Output;
use crate::shell::{get_shell, ShellType};

/// Enables rtx to automatically modify runtimes when changing directory
///
/// This should go into your shell's rc file.
/// Otherwise, it will only take effect in the current session.
/// (e.g. ~/.bashrc)
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP.as_str())]
pub struct Activate {
    /// Shell type to generate the script for
    #[clap(long, short, hide = true)]
    shell: Option<ShellType>,

    /// Shell type to generate the script for
    #[clap()]
    shell_type: Option<ShellType>,

    /// Show "rtx: <PLUGIN>@<VERSION>" message when changing directories
    #[clap(long)]
    status: bool,

    /// noop
    #[clap(long, short, hide = true)]
    quiet: bool,
}

impl Command for Activate {
    fn run(self, _config: Config, out: &mut Output) -> Result<()> {
        let shell = get_shell(self.shell_type.or(self.shell))
            .expect("no shell provided, use `--shell=zsh`");

        let exe = if cfg!(test) {
            "rtx".into()
        } else {
            env::RTX_EXE.to_path_buf()
        };
        let output = shell.activate(&exe, self.status);
        out.stdout.write(output);

        Ok(())
    }
}

static AFTER_LONG_HELP: Lazy<String> = Lazy::new(|| {
    formatdoc! {r#"
    {}
        $ eval "$(rtx activate bash)"
        $ eval "$(rtx activate zsh)"
        $ rtx activate fish | source
        $ execx($(rtx activate xonsh))
    "#, style("Examples:").bold().underlined()}
});

#[cfg(test)]
mod tests {

    use crate::assert_cli_snapshot;

    #[test]
    fn test_activate_zsh() {
        assert_cli_snapshot!("activate", "zsh");
    }

    #[test]
    fn test_activate_zsh_legacy() {
        assert_cli_snapshot!("activate", "-s", "zsh");
    }
}
