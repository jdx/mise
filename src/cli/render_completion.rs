use clap::Args;
use std::io::Cursor;

use clap_complete::generate;
use miette::Result;

use crate::shell::completions;

/// Generate shell completions
#[derive(Debug, Args)]
#[clap(hide = true, verbatim_doc_comment)]
pub struct RenderCompletion {
    /// Shell type to generate completions for
    #[clap(required_unless_present = "shell_type")]
    shell: Option<clap_complete::Shell>,

    /// Shell type to generate completions for
    #[clap(long = "shell", short = 's', hide = true)]
    shell_type: Option<clap_complete::Shell>,
}

impl RenderCompletion {
    pub fn run(self) -> Result<()> {
        let shell = self.shell.or(self.shell_type).unwrap();

        let mut cmd = crate::cli::Cli::command();

        let script = match shell {
            clap_complete::Shell::Zsh => completions::zsh_complete(&cmd)?,
            clap_complete::Shell::Fish => completions::fish_complete(&cmd)?,
            _ => {
                let mut c = Cursor::new(Vec::new());
                generate(shell, &mut cmd, "mise", &mut c);
                String::from_utf8(c.into_inner()).unwrap()
            }
        };
        miseprintln!("{}", script.trim());

        Ok(())
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn test_completion() {
        assert_cli!("render-completion", "bash");
        assert_cli!("render-completion", "fish");
        assert_cli!("render-completion", "zsh");
    }
}
