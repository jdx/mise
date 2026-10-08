use eyre::{Result, bail};

use crate::config::Config;
use crate::config::env_directive::prompt;

/// Ask for the prompt vars that have no saved answer
///
/// Enter accepts the suggested default. Each answer, accepted defaults
/// included, is saved and never asked for again; use `mise vars unset` to be
/// asked again. Needs a terminal. Asks about every unanswered `prompt` var in
/// the config files that apply here, or only the ones you name.
#[derive(Debug, usage_rs::Args)]
#[usage(
    verbatim_doc_comment,
    example(
        r###"mise vars prompt"###,
        help = "Ask for every unanswered prompt var"
    ),
    example(r###"mise vars prompt git_name"###, help = "Ask for git_name only")
)]
pub(super) struct VarsPrompt {
    /// Only ask for these vars
    #[usage(value_name = "NAME")]
    names: Vec<String>,
}

impl VarsPrompt {
    pub(super) async fn run(self) -> Result<()> {
        if !console::user_attended_stderr() {
            bail!("`mise vars prompt` needs an interactive terminal");
        }
        prompt::enable();
        prompt::only(&self.names);
        // Naming some vars must not fail on the `required` ones left out. With
        // no names, a var still missing after asking is a real error.
        if !self.names.is_empty() {
            prompt::tolerate_missing();
        }
        // Prompting happens while the vars resolve, so (re)load the config now.
        Config::reset().await?;
        let declared = prompt::declared();
        if let Some(unknown) = self.names.iter().find(|name| !declared.contains(*name)) {
            bail!("no [vars] entry named '{unknown}' declares a `prompt` here");
        }
        let targets: Vec<&String> = if self.names.is_empty() {
            declared.iter().collect()
        } else {
            self.names.iter().collect()
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
