use crate::cmd::cmd;
use crate::config::Config;
use crate::toolset::{ConfigScope, ResolveOptions, ToolsetBuilder};
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

    /// Include the bash completion library in the bash completion script
    ///
    /// This is required for completions to work in bash, but it is not included by default
    /// you may source it separately or enable this flag to enable it in the script.
    #[clap(long, verbatim_doc_comment)]
    include_bash_completion_lib: bool,

    /// Always use usage for completions.
    /// Currently, usage is the default for fish and bash but not zsh since it has a few quirks
    /// to work out first.
    ///
    /// This requires the `usage` CLI to be installed.
    /// https://usage.jdx.dev
    #[clap(long, verbatim_doc_comment, hide = true)]
    usage: bool,
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
        let args = self.usage_args(shell);

        // Prefer an explicitly available usage binary without loading any mise configuration.
        // This is both the cheapest path and avoids project trust prompts during lazy completion.
        match cmd("usage", &args).read() {
            Ok(output) => return Ok(output),
            Err(err) => debug!("usage command from PATH failed: {err:?}"),
        }

        // A globally managed usage binary is still useful, but project configuration must not be
        // consulted while the shell is bootstrapping completion. Offline resolution also prevents
        // shell startup from fetching versions.
        let config = Config::load_global().await?;
        let toolset = ToolsetBuilder::new()
            .with_scope(ConfigScope::GlobalOnly)
            .with_resolve_options(ResolveOptions {
                offline: true,
                ..Default::default()
            })
            .build(&config)
            .await?;
        Ok(cmd("usage", args)
            .full_env(toolset.full_env(&config).await?)
            .read()?)
    }

    fn usage_args(&self, shell: Shell) -> Vec<String> {
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
        args
    }

    fn prerendered(&self, shell: Shell) -> String {
        match shell {
            Shell::Bash => include_str!("../../completions/mise.bash"),
            Shell::Fish => include_str!("../../completions/mise.fish"),
            Shell::Powershell => include_str!("../../completions/mise.ps1"),
            Shell::Zsh => include_str!("../../completions/_mise"),
        }
        .to_string()
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

#[derive(Debug, Clone, Copy, EnumString, strum::Display)]
#[strum(serialize_all = "snake_case")]
enum Shell {
    Bash,
    Fish,
    #[strum(serialize = "powershell")]
    Powershell,
    Zsh,
}

impl ValueEnum for Shell {
    fn value_variants<'a>() -> &'a [Self] {
        &[Self::Bash, Self::Fish, Self::Powershell, Self::Zsh]
    }
    fn to_possible_value(&self) -> Option<PossibleValue> {
        let value = PossibleValue::new(self.to_string());
        Some(match self {
            // `mise activate` names this shell `pwsh`, and the two commands are run one after the
            // other during setup, so whichever name a user learns first is rejected by the other.
            // The names differ only because the two lists come from different places, not because
            // they mean different shells.
            //
            // `mise usage` renders aliases, so this shows up in the generated CLI docs as a
            // choice alongside `powershell` rather than being hidden.
            Self::Powershell => value.alias("pwsh"),
            _ => value,
        })
    }
}

#[cfg(test)]
mod shell_name_tests {
    use super::*;

    #[test]
    fn pwsh_is_accepted_as_powershell() {
        assert!(matches!(
            <Shell as ValueEnum>::from_str("pwsh", true),
            Ok(Shell::Powershell)
        ));
        assert!(matches!(
            <Shell as ValueEnum>::from_str("powershell", true),
            Ok(Shell::Powershell)
        ));
    }

    #[test]
    fn the_primary_names_are_unchanged() {
        // Only the *names* -- the alias is rendered into the CLI docs, so asserting it absent
        // here would state something false. This pins that adding it renamed nothing.
        let listed: Vec<String> = Shell::value_variants()
            .iter()
            .filter_map(|v| v.to_possible_value())
            .map(|pv| pv.get_name().to_string())
            .collect();
        assert_eq!(listed, ["bash", "fish", "powershell", "zsh"]);
    }

    #[test]
    fn usage_arguments_preserve_completion_options() {
        let completion = Completion {
            shell: None,
            shell_type: None,
            include_bash_completion_lib: true,
            usage: false,
        };
        let args = completion.usage_args(Shell::Bash);

        assert_eq!(
            args,
            [
                "generate",
                "completion",
                "bash",
                "mise",
                "--usage-cmd",
                "mise usage",
                "--cache-key",
                env!("CARGO_PKG_VERSION"),
                "--include-bash-completion-lib",
            ]
        );
    }
}
