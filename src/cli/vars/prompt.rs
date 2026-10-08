use eyre::{Result, bail};

use crate::config::Config;
use crate::config::env_directive::prompt;

/// Ask for the prompt vars that have no saved answer, or set answers directly
///
/// Enter accepts the suggested default. Each answer, accepted defaults
/// included, is saved and never asked for again; use `mise vars unset` to be
/// asked again. With no arguments, asks about every unanswered `prompt` var in
/// the config files that apply here. `NAME` asks for only that var;
/// `NAME=VALUE` saves the answer without asking, as `mise set` does for an
/// env var, so it needs no terminal.
#[derive(Debug, usage_rs::Args)]
#[usage(
    verbatim_doc_comment,
    example(
        r###"mise vars prompt"###,
        help = "Ask for every unanswered prompt var"
    ),
    example(r###"mise vars prompt git_name"###, help = "Ask for git_name only"),
    example(
        r###"mise vars prompt git_name="Ada Lovelace""###,
        help = "Set the answer for git_name without asking"
    )
)]
pub(super) struct VarsPrompt {
    /// Vars to ask for (`NAME`) or answers to save (`NAME=VALUE`)
    #[usage(value_name = "NAME[=VALUE]")]
    args: Vec<String>,
}

impl VarsPrompt {
    pub(super) async fn run(self) -> Result<()> {
        let mut names = Vec::new();
        for arg in &self.args {
            match arg.split_once('=') {
                Some(("", _)) => bail!("expected NAME or NAME=VALUE, got '{arg}'"),
                Some((name, value)) => prompt::set(name, value)?,
                None => names.push(arg.clone()),
            }
        }
        // Only answers were given: they are saved, and nothing is left to ask.
        if names.is_empty() && !self.args.is_empty() {
            return Ok(());
        }
        if !console::user_attended_stderr() {
            bail!("`mise vars prompt` needs an interactive terminal");
        }
        prompt::enable();
        prompt::only(&names);
        // Naming some vars must not fail on the `required` ones left out. With
        // no names, a var still missing after asking is a real error.
        if !names.is_empty() {
            prompt::tolerate_missing();
        }
        // Prompting happens while the vars resolve, so (re)load the config now.
        Config::reset().await?;
        let declared = prompt::declared();
        if let Some(unknown) = names.iter().find(|name| !declared.contains(*name)) {
            bail!("no [vars] entry named '{unknown}' declares a `prompt` here");
        }
        let targets: Vec<&String> = if names.is_empty() {
            declared.iter().collect()
        } else {
            names.iter().collect()
        };
        let unanswered: Vec<&str> = targets
            .into_iter()
            .filter(|name| prompt::saved(name).is_none())
            .map(String::as_str)
            .collect();
        if !unanswered.is_empty() {
            bail!("no answer given for {}", unanswered.join(", "));
        }
        let answered = prompt::answered();
        if answered.is_empty() && declared.is_empty() {
            miseprintln!("No [vars] entry declares a `prompt` here.");
        } else if answered.is_empty() {
            miseprintln!("Nothing to ask: every prompt var already has an answer.");
        } else {
            miseprintln!(
                "Saved {} to {}",
                answered.join(", "),
                prompt::answers_path().display()
            );
        }
        Ok(())
    }
}
