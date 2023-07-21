use color_eyre::eyre::Result;

use crate::cli::command::Command;
use crate::config::Config;
use crate::dirs;

use crate::env::RTX_EXE;
use crate::file::touch_dir;
use crate::output::Output;
use crate::shell::{get_shell, ShellType};

/// Initializes rtx in the current shell
///
/// This should go into your shell's rc file.
/// Otherwise, it will only take effect in the current session.
/// (e.g. ~/.zshrc, ~/.bashrc)
///
/// This is only intended to be used in interactive sessions, not scripts.
/// rtx is only capable of updating PATH when the prompt is displayed to the user.
/// For non-interactive use-cases, use shims instead.
///
/// Typically this can be added with something like the following:
///
///     echo 'eval "$(rtx activate)"' >> ~/.zshrc
///
/// However, this requires that "rtx" is in your PATH. If it is not, you need to
/// specify the full path like this:
///
///     echo 'eval "$(/path/to/rtx activate)"' >> ~/.zshrc
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
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

        // touch ROOT to allow hook-env to run
        let _ = touch_dir(&dirs::ROOT);

        let output = shell.activate(&RTX_EXE, self.status);
        out.stdout.write(output);

        Ok(())
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>
  $ <bold>eval "$(rtx activate bash)"</bold>
  $ <bold>eval "$(rtx activate zsh)"</bold>
  $ <bold>rtx activate fish | source</bold>
  $ <bold>execx($(rtx activate xonsh))</bold>
"#
);
