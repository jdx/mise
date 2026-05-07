use crate::cli::token::github::Github;

/// Display the GitHub token mise will use for a given host
///
/// Shows which token source mise would use, useful for debugging
/// authentication issues. The token is masked by default.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP, hide = true)]
pub struct Token {
    /// GitHub hostname
    #[clap(default_value = "github.com")]
    host: String,

    /// Force native GitHub OAuth device flow instead of normal token resolution
    #[clap(long)]
    oauth: bool,

    /// Print only the token value
    #[clap(long)]
    raw: bool,

    /// Show the full unmasked token
    #[clap(long)]
    unmask: bool,
}

impl Token {
    pub fn run(self) -> eyre::Result<()> {
        Github::from(self).run()
    }
}

impl From<Token> for Github {
    fn from(t: Token) -> Self {
        Github {
            host: t.host,
            oauth: t.oauth,
            raw: t.raw,
            unmask: t.unmask,
        }
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
