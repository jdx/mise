use atty::Stream;
use color_eyre::eyre::Result;
use indoc::formatdoc;
use once_cell::sync::Lazy;

use crate::cli::command::Command;
use crate::config::Config;
use crate::env;
use crate::output::Output;
use crate::shell::{get_shell, ShellType};
use crate::ui::color::Color;

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

    /// Hide the "rtx: <PLUGIN>@<VERSION>" message when changing directories
    #[clap(long, short)]
    quiet: bool,
}

impl Command for Activate {
    fn run(self, _config: Config, out: &mut Output) -> Result<()> {
        let shell = get_shell(self.shell_type.or(self.shell));

        if self.quiet {
            // TODO: it would probably be better to just set --quiet on `hook-env`
            // this will cause _all_ rtx commands to be quiet, not just the hook
            // however as of this writing I don't think RTX_QUIET impacts other commands
            rtxprintln!(out, "{}", shell.set_env("RTX_QUIET", "1"));
        }

        let exe = if cfg!(test) {
            "rtx".into()
        } else {
            env::RTX_EXE.to_path_buf()
        };
        let output = shell.activate(&exe);
        out.stdout.write(output);

        Ok(())
    }
}

static COLOR: Lazy<Color> = Lazy::new(|| Color::new(Stream::Stdout));
static AFTER_LONG_HELP: Lazy<String> = Lazy::new(|| {
    formatdoc! {r#"
    {}
        $ eval "$(rtx activate bash)"
        $ eval "$(rtx activate zsh)"
        $ rtx activate fish | source
        $ execx($(rtx activate xonsh))
    "#, COLOR.header("Examples:")}
});

#[cfg(test)]
mod test {
    use insta::assert_display_snapshot;

    use crate::assert_cli;

    #[test]
    fn test_activate_zsh() {
        let stdout = assert_cli!("activate", "zsh");
        assert_display_snapshot!(stdout);
    }

    #[test]
    fn test_activate_zsh_legacy() {
        let stdout = assert_cli!("activate", "-s", "zsh");
        assert_display_snapshot!(stdout);
    }
}
