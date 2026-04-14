use crate::github;
use crate::tokens;

/// Display the GitHub token mise will use for a given host
///
/// Shows which token source mise would use, useful for debugging
/// authentication issues. The token is masked by default.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Github {
    /// GitHub hostname
    #[clap(default_value = "github.com")]
    host: String,

    /// Show the full unmasked token
    #[clap(long)]
    unmask: bool,
}

impl Github {
    pub fn run(self) -> eyre::Result<()> {
        match github::resolve_token(&self.host) {
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

    $ <bold>mise token github</bold>
    github.com: ghp_…xxxx (source: GITHUB_TOKEN)

    $ <bold>mise token github --unmask</bold>
    github.com: ghp_xxxxxxxxxxxx (source: GITHUB_TOKEN)

    $ <bold>mise token github github.mycompany.com</bold>
    github.mycompany.com: (none)
"#
);
