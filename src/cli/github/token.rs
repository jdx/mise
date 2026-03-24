use crate::github;

/// Display the GitHub token mise will use for a given host
///
/// Shows which token source mise would use, useful for debugging
/// authentication issues. The token is masked by default.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Token {
    /// GitHub hostname
    #[clap(default_value = "github.com")]
    host: String,

    /// Show the full unmasked token
    #[clap(long)]
    unmask: bool,
}

impl Token {
    pub fn run(self) -> eyre::Result<()> {
        match github::resolve_token(&self.host) {
            Some((token, source)) => {
                let display_token = if self.unmask {
                    token
                } else {
                    mask_token(&token)
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

fn mask_token(token: &str) -> String {
    let len = token.len();
    if len <= 4 {
        "*".repeat(len)
    } else if len <= 8 {
        format!("{}…", &token[..4])
    } else {
        format!("{}…{}", &token[..4], &token[len - 4..])
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise github token</bold>
    github.com: ghp_…xxxx (source: GITHUB_TOKEN)

    $ <bold>mise github token --unmask</bold>
    github.com: ghp_xxxxxxxxxxxx (source: GITHUB_TOKEN)

    $ <bold>mise github token github.mycompany.com</bold>
    github.mycompany.com: (none)
"#
);
