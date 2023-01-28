use color_eyre::eyre::Result;
use indoc::indoc;

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
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Activate {
    /// Shell type to generate the script for
    #[clap(long, short)]
    shell: Option<ShellType>,
}

impl Command for Activate {
    fn run(self, _config: Config, out: &mut Output) -> Result<()> {
        if !*env::RTX_DISABLE_DIRENV_WARNING && env::DIRENV_DIR.is_some() {
            warn!(indoc! {r#"
                `rtx activate` may conflict with direnv!
                       See https://github.com/jdxcode/rtx#direnv for more information.
                       Disable this warning with RTX_DISABLE_DIRENV_WARNING=1
                "#});
        }

        let shell = get_shell(self.shell);

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

const AFTER_LONG_HELP: &str = r#"
Examples:
    $ eval "$(rtx activate -s bash)"
    $ eval "$(rtx activate -s zsh)"
    $ rtx activate -s fish | source
"#;

#[cfg(test)]
mod test {
    use insta::assert_display_snapshot;

    use crate::assert_cli;

    use super::*;

    #[test]
    fn test_activate_zsh() {
        let Output { stdout, .. } = assert_cli!("activate", "-s", "zsh");
        assert_display_snapshot!(stdout.content);
    }
}
