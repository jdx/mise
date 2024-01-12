use std::fmt::Display;

use clap::builder::PossibleValue;
use clap::ValueEnum;
use eyre::Result;

/// Generate shell completions
#[derive(Debug, clap::Args)]
#[clap(aliases = ["complete", "completions"], verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Completion {
    /// Shell type to generate completions for
    #[clap(required_unless_present = "shell_type")]
    shell: Option<Shell>,

    /// Shell type to generate completions for
    #[clap(long = "shell", short = 's', hide = true)]
    shell_type: Option<Shell>,
}

impl Completion {
    pub fn run(self) -> Result<()> {
        let c = match self.shell.or(self.shell_type).unwrap() {
            Shell::Bash => include_str!("../../completions/mise.bash"),
            Shell::Fish => include_str!("../../completions/mise.fish"),
            Shell::Zsh => include_str!("../../completions/_mise"),
        };
        miseprintln!("{}", c.trim());

        Ok(())
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>
  $ <bold>mise completion bash > /etc/bash_completion.d/mise</bold>
  $ <bold>mise completion zsh  > /usr/local/share/zsh/site-functions/_mise</bold>
  $ <bold>mise completion fish > ~/.config/fish/completions/mise.fish</bold>
"#
);

#[derive(Debug, Clone)]
enum Shell {
    Bash,
    Fish,
    Zsh,
}

impl ValueEnum for Shell {
    fn value_variants<'a>() -> &'a [Self] {
        &[Self::Bash, Self::Fish, Self::Zsh]
    }
    fn from_str(input: &str, _ignore_case: bool) -> std::result::Result<Self, String> {
        match input {
            "bash" => Ok(Self::Bash),
            "fish" => Ok(Self::Fish),
            "zsh" => Ok(Self::Zsh),
            _ => Err(format!("unknown shell type: {}", input)),
        }
    }
    fn to_possible_value(&self) -> Option<PossibleValue> {
        Some(PossibleValue::new(self.to_string()))
    }
}

impl Display for Shell {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Bash => write!(f, "bash"),
            Self::Fish => write!(f, "fish"),
            Self::Zsh => write!(f, "zsh"),
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_completion() {
        assert_cli!("completion", "zsh");
        assert_cli!("completion", "bash");
        assert_cli!("completion", "fish");
    }
}
