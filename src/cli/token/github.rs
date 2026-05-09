use crate::github;
use crate::tokens;
use eyre::bail;

/// Display the GitHub token mise will use for a given host
///
/// Shows which token source mise would use, useful for debugging
/// authentication issues. The token is masked by default.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Github {
    /// GitHub hostname
    #[clap(default_value = "github.com")]
    pub(crate) host: String,

    /// [experimental] Resolve only via the native GitHub OAuth source (cache,
    /// refresh, or device-code flow), bypassing other token sources
    #[clap(long)]
    pub(crate) oauth: bool,

    /// Print only the token value
    #[clap(long)]
    pub(crate) raw: bool,

    /// Show the full unmasked token
    #[clap(long)]
    pub(crate) unmask: bool,
}

impl Github {
    pub fn run(self) -> eyre::Result<()> {
        let resolved = if self.oauth {
            Some((
                github::oauth::token(github::oauth::TokenRequest {
                    host: self.host.clone(),
                    allow_device_flow: true,
                })?,
                github::TokenSource::GithubOauth,
            ))
        } else {
            github::resolve_token(&self.host)
        };
        match resolved {
            Some((token, source)) => {
                if self.raw {
                    miseprintln!("{token}");
                    return Ok(());
                }
                let display_token = if self.unmask {
                    token
                } else {
                    tokens::mask_token(&token)
                };
                miseprintln!("{}: {} (source: {})", self.host, display_token, source);
            }
            None => {
                if self.raw {
                    bail!("no GitHub token found for {}", self.host);
                }
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
