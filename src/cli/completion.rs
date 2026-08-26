use crate::cli::Cli;
use eyre::Result;
use std::ffi::OsString;
use strum::EnumString;

/// Answer mise's hidden completion protocol with its runtime completion metadata.
///
/// The tables compiled by usage-rs cover static commands, flags, choices, and path hints. mise
/// also augments those tables at runtime with `run=` completers and task commands mounted from
/// `mise tasks --usage`; those only exist in [`super::usage::completion_spec`]. Try that richer
/// spec first, preserving its path fallback marker, then leave only unsupported requests to the
/// compiled usage-rs tables.
pub(crate) fn completion_request(argv: &[OsString]) -> Option<String> {
    let request = usage_rs::complete::CompletionRequest::parse(argv)?;
    if request.candidates_for.is_some() {
        return Cli::completion_request(argv);
    }

    let spec = super::usage::completion_spec();
    let answer = usage_cli::complete_answer(
        &spec,
        &request.split.words,
        request.split.cword,
        request.shell.as_str(),
    );
    match answer {
        Ok(answer) => {
            let candidates = if answer.files {
                vec![]
            } else {
                answer
                    .candidates
                    .into_iter()
                    .map(|(value, description)| {
                        if description.is_empty() {
                            usage_rs::complete::Candidate::new(value)
                        } else {
                            usage_rs::complete::Candidate::described(value, description)
                        }
                    })
                    .collect()
            };
            let answer = usage_rs::complete::Completions {
                candidates,
                files: answer.files.then_some(usage_rs::complete::Files::Any),
            };
            Some(usage_rs::complete::render(&answer, request.shell))
        }
        _ => Cli::completion_request(argv),
    }
}

/// Generate shell completions
#[derive(Debug, usage_rs::Args)]
#[usage(aliases = ["complete", "completions"], verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub(crate) struct Completion {
    /// Shell type to generate completions for
    #[usage(required_unless = "shell_type", value_enum)]
    shell: Option<Shell>,

    /// Shell type to generate completions for
    #[usage(long = "shell", short = 's', hide = true, value_enum)]
    shell_type: Option<Shell>,

    /// Retained for compatibility with older completion generators.
    ///
    /// usage-rs's built-in bash script is self-contained, so this is now a no-op.
    #[usage(long, verbatim_doc_comment)]
    include_bash_completion_lib: bool,

    /// Retained for compatibility with older completion generators.
    ///
    /// Completions now always use usage-rs's built-in protocol, so this is a no-op.
    #[usage(long, verbatim_doc_comment, hide = true)]
    usage: bool,

    /// Install the script where this shell looks for it, instead of printing it
    ///
    /// Writes the script file and nothing else: no shell rc file and no PowerShell profile is
    /// edited. Where a shell needs a one-time line of its own — zsh's `fpath+=`, PowerShell's
    /// dot-source — it is printed for you to add.
    #[usage(long, verbatim_doc_comment, effect = "write")]
    install: bool,

    /// Replace a file at the target path that mise did not write
    #[usage(long, requires = "--install", effect = "write")]
    force: bool,
}

impl Completion {
    pub(crate) async fn run(self) -> Result<()> {
        let shell = self.shell.or(self.shell_type).unwrap();
        if self.install {
            return self.install_script(shell.into());
        }
        let script = Cli::completion_script(shell.into());
        miseprintln!("{}", script.trim());

        Ok(())
    }

    /// Put the script where this shell looks for it, and say what is left to do.
    ///
    /// The location comes from usage rather than from a table here, so `mise completion zsh
    /// --install` and `usage g completion zsh mise --install` cannot disagree about where a mise
    /// completion lives.
    fn install_script(&self, shell: usage_rs::complete::Shell) -> Result<()> {
        use usage_rs::install::{self, OnForeign, Wrote};

        let on_foreign = if self.force {
            OnForeign::Overwrite
        } else {
            OnForeign::Refuse
        };
        // The environment is described from this process rather than reached for inside the
        // resolver, which is what lets a test point the same code path somewhere harmless.
        let done = Cli::install_completion(shell, &install::Env::from_process(), on_foreign)
            .map_err(|err| match &err {
                install::Error::Foreign { .. } => eyre::eyre!(
                    "{err}\n\nPass --force to replace it, or redirect the script yourself."
                ),
                _ => eyre::Report::new(err),
            })?;

        // Everything here goes to stderr, so stdout stays empty under `--install`. The examples
        // below document `mise completion zsh > …`, and prose on stdout would land in that file.
        eprintln!("installing to {}", done.plan.path.display());
        if done.wrote == Wrote::Unchanged {
            eprintln!("already up to date");
        }
        if let Some(line) = done.plan.loading.instruction() {
            let file = match &done.plan.loading {
                install::Loading::Manual { file, .. } => file.as_str(),
                _ => "your shell's startup file",
            };
            eprintln!("\nadd this to {file}, once:\n\n{line}\n");
        }
        if let Some(note) = done.plan.note {
            eprintln!("note: {note}");
        }
        Ok(())
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    # put it where the shell looks, and print any one-time line it still needs
    $ <bold>mise completion zsh --install</bold>

    # or choose the path yourself
    $ <bold>mise completion bash > ~/.local/share/bash-completion/completions/mise</bold>
    $ <bold>mise completion zsh  > /usr/local/share/zsh/site-functions/_mise</bold>
    $ <bold>mise completion fish > ~/.config/fish/completions/mise.fish</bold>
    $ <bold>mise completion powershell >> $PROFILE</bold>
"#
);

#[derive(Debug, Clone, Copy, EnumString, strum::Display, usage_rs::ValueEnum)]
#[strum(serialize_all = "snake_case")]
#[usage(rename_all = "snake_case")]
enum Shell {
    Bash,
    Fish,
    #[strum(serialize = "powershell")]
    #[usage(name = "powershell", visible_alias = "pwsh")]
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
