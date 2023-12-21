use std::path::PathBuf;

use eyre::Result;

use crate::file::touch_dir;
use crate::shell::{get_shell, ShellType};
use crate::{dirs, env};

/// Initializes rtx in the current shell session
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

    /// Hide warnings such as when a tool is not installed
    #[clap(long, short)]
    quiet: bool,
}

impl Activate {
    pub fn run(self) -> Result<()> {
        let shell = get_shell(self.shell_type.or(self.shell))
            .expect("no shell provided. Run `rtx activate zsh` or similar");

        // touch ROOT to allow hook-env to run
        let _ = touch_dir(&dirs::DATA);

        let rtx_bin = if cfg!(target_os = "linux") {
            // linux dereferences symlinks, so use argv0 instead
            PathBuf::from(&*env::ARGV0)
        } else {
            env::RTX_BIN.clone()
        };
        let mut flags = vec![];
        if self.quiet {
            flags.push(" --quiet");
        }
        if self.status {
            flags.push(" --status");
        }
        let output = shell.activate(&rtx_bin, flags.join(""));
        rtxprint!("{output}");

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
