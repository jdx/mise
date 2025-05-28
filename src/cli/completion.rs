use crate::cmd::cmd;
use crate::config::Config;
use crate::toolset::ToolsetBuilder;
use clap::ValueEnum;
use clap::builder::PossibleValue;
use eyre::Result;
use strum::EnumString;

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

    /// Always use usage for completions.
    /// Currently, usage is the default for fish and bash but not zsh since it has a few quirks
    /// to work out first.
    ///
    /// This requires the `usage` CLI to be installed.
    /// https://usage.jdx.dev
    #[clap(long, verbatim_doc_comment, hide = true)]
    usage: bool,

    /// Include the bash completion library in the bash completion script
    ///
    /// This is required for completions to work in bash, but it is not included by default
    /// you may source it separately or enable this flag to include it in the script.
    #[clap(long, verbatim_doc_comment)]
    include_bash_completion_lib: bool,
}

impl Completion {
    pub async fn run(self) -> Result<()> {
        let shell = self.shell.or(self.shell_type).unwrap();

        let script = match self.call_usage(shell).await {
            Ok(script) => script,
            Err(e) => {
                debug!("usage command failed, falling back to prerendered completions");
                debug!("error: {e:?}");
                self.prerendered(shell)
            }
        };
        miseprintln!("{}", script.trim());

        Ok(())
    }

    async fn call_usage(&self, shell: Shell) -> Result<String> {
        let config = Config::get().await?;
        let toolset = ToolsetBuilder::new().build(&config).await?;
        let mut args = vec![
            "generate".into(),
            "completion".into(),
            shell.to_string(),
            "mise".into(),
            "--usage-cmd".into(),
            "mise usage".into(),
            "--cache-key".into(),
            env!("CARGO_PKG_VERSION").into(),
        ];
        if self.include_bash_completion_lib {
            args.push("--include-bash-completion-lib".into());
        }
        let output = cmd("usage", args)
            .full_env(toolset.full_env(&config).await?)
            .read()?;
        Ok(output)
    }

    fn prerendered(&self, shell: Shell) -> String {
        match shell {
            Shell::Bash => include_str!("../../completions/mise.bash"),
            Shell::Fish => include_str!("../../completions/mise.fish"),
            Shell::Zsh => include_str!("../../completions/_mise"),
        }
        .to_string()
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise completion bash > ~/.local/share/bash-completion/completions/mise</bold>
    $ <bold>mise completion zsh  > /usr/local/share/zsh/site-functions/_mise</bold>
    $ <bold>mise completion fish > ~/.config/fish/completions/mise.fish</bold>
"#
);

#[derive(Debug, Clone, Copy, EnumString, strum::Display)]
#[strum(serialize_all = "snake_case")]
enum Shell {
    Bash,
    Fish,
    Zsh,
}

impl ValueEnum for Shell {
    fn value_variants<'a>() -> &'a [Self] {
        &[Self::Bash, Self::Fish, Self::Zsh]
    }
    fn to_possible_value(&self) -> Option<PossibleValue> {
        Some(PossibleValue::new(self.to_string()))
    }
}
