use crate::gitlab;
use crate::tokens;

/// Display the GitLab token mise will use for a given host
///
/// Shows which token source mise would use, useful for debugging
/// authentication issues. The token is masked by default.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Token {
    /// GitLab hostname
    #[clap(default_value = "gitlab.com")]
    host: String,

    /// Show the full unmasked token
    #[clap(long)]
    unmask: bool,
}

impl Token {
    pub fn run(self) -> eyre::Result<()> {
        match gitlab::resolve_token(&self.host) {
            Some((token, source)) => {
                let display_token = if self.unmask {
                    token
                } else {
                    tokens::mask_token(&token)
                };
                miseprintln!("{}: {} (source: {})", self.host, display_token, source);
            }
            None => {
                miseprintln!("{}: (none)", self.host);
            }
        }
        Ok(())
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise gitlab token</bold>
    gitlab.com: glpa…xxxx (source: GITLAB_TOKEN)

    $ <bold>mise gitlab token --unmask</bold>
    gitlab.com: glpat-xxxxxxxxxxxx (source: GITLAB_TOKEN)

    $ <bold>mise gitlab token gitlab.mycompany.com</bold>
    gitlab.mycompany.com: (none)
"#
);
