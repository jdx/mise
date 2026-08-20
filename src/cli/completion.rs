use crate::cli::Cli;
use eyre::Result;
use strum::EnumString;

/// Generate shell completions
#[derive(Debug, usage_rs::Args)]
#[command(aliases = ["complete", "completions"], verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Completion {
    /// Shell type to generate completions for
    #[arg(required_unless_present = "shell_type", value_enum)]
    shell: Option<Shell>,

    /// Shell type to generate completions for
    #[arg(long = "shell", short = 's', hide = true, value_enum)]
    shell_type: Option<Shell>,

    /// Retained for compatibility with older completion generators.
    ///
    /// usage-rs's built-in bash script is self-contained, so this is now a no-op.
    #[arg(long, verbatim_doc_comment)]
    include_bash_completion_lib: bool,

    /// Retained for compatibility with older completion generators.
    ///
    /// Completions now always use usage-rs's built-in protocol, so this is a no-op.
    #[arg(long, verbatim_doc_comment, hide = true)]
    usage: bool,
}

impl Completion {
    pub(crate) async fn run(self) -> Result<()> {
        let shell = self.shell.or(self.shell_type).unwrap();
        let script = Cli::completion_script(shell.into());
        miseprintln!("{}", script.trim());

        Ok(())
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise completion bash --include-bash-completion-lib > ~/.local/share/bash-completion/completions/mise</bold>
    $ <bold>mise completion zsh  > /usr/local/share/zsh/site-functions/_mise</bold>
    $ <bold>mise completion fish > ~/.config/fish/completions/mise.fish</bold>
    $ <bold>mise completion powershell >> $PROFILE</bold>
"#
);

#[derive(Debug, Clone, Copy, EnumString, strum::Display, usage_rs::ValueEnum)]
#[strum(serialize_all = "snake_case")]
#[value(rename_all = "snake_case")]
enum Shell {
    Bash,
    Fish,
    #[strum(serialize = "powershell")]
    #[value(name = "powershell", visible_alias = "pwsh")]
    Powershell,
    Zsh,
}

impl From<Shell> for usage_rs::complete::Shell {
    fn from(shell: Shell) -> Self {
        match shell {
            Shell::Bash => Self::Bash,
            Shell::Fish => Self::Fish,
            Shell::Powershell => Self::PowerShell,
            Shell::Zsh => Self::Zsh,
        }
    }
}

#[cfg(test)]
mod shell_name_tests {
    use super::*;
    use usage_rs::spec::ValueEnum;

    #[test]
    fn pwsh_is_accepted_as_powershell() {
        assert!(matches!(
            <Shell as ValueEnum>::from_choice("pwsh"),
            Some(Shell::Powershell)
        ));
        assert!(matches!(
            <Shell as ValueEnum>::from_choice("powershell"),
            Some(Shell::Powershell)
        ));
    }

    #[test]
    fn the_primary_names_are_unchanged() {
        // Only the *names* -- the alias is rendered into the CLI docs, so asserting it absent
        // here would state something false. This pins that adding it renamed nothing.
        let listed: Vec<&str> = Shell::DETAILS.iter().map(|choice| choice.value).collect();
        assert_eq!(listed, ["bash", "fish", "powershell", "zsh"]);
    }

    #[test]
    fn completion_script_calls_back_into_mise() {
        let script = Cli::completion_script(usage_rs::complete::Shell::Bash);
        assert!(script.contains("mise' __complete_word__"), "{script}");
        assert!(!script.contains("command usage"), "{script}");
    }
}
