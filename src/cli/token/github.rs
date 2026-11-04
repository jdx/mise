use crate::github;
use crate::tokens;
use eyre::bail;

/// Show the GitHub token mise uses for a host
///
/// Shows the token and where it came from, to debug authentication. The token
/// is masked unless you pass --unmask or --raw.
#[derive(Debug, usage_rs::Args)]
#[usage(
    verbatim_doc_comment,
    example(
        r###"mise token github
github.com: ghp_…xxxx (source: GITHUB_TOKEN)"###,
        help = "Show the token for github.com"
    ),
    example(
        r###"mise token github --unmask
github.com: ghp_xxxxxxxxxxxx (source: GITHUB_TOKEN)"###,
        help = "Show the whole token"
    ),
    example(
        r###"mise token github github.mycompany.com
github.mycompany.com: (none)"###,
        help = "Check a GitHub Enterprise host"
    ),
    example(
        r###"mise token github --oauth --refresh
github.com: gho_…xxxx (source: GitHub OAuth)"###,
        help = "Get a new token through native GitHub OAuth"
    ),
    example(
        r###"GH_TOKEN=$(mise token github --raw) gh api user"###,
        help = "Pass the resolved token to another tool"
    )
)]
pub(crate) struct Github {
    /// GitHub hostname
    #[usage(default = "github.com")]
    pub(crate) host: String,

    /// Speak Git's credential helper protocol
    #[usage(long, hide = true)]
    pub(crate) git_credential: Option<String>,

    /// Use only native GitHub OAuth (a cached token, a refresh, or the
    /// device-code flow), ignoring other token sources
    #[usage(long)]
    pub(crate) oauth: bool,

    /// Print only the token, unmasked, for use in scripts
    #[usage(long)]
    pub(crate) raw: bool,

    /// With --oauth, get a new OAuth token even if the cached one has not
    /// expired, through the refresh-token grant or a new device-code flow.
    /// Use it after changing the GitHub App's installations or permissions:
    /// cached tokens keep their original access until they expire.
    #[usage(long, requires = "oauth")]
    pub(crate) refresh: bool,

    /// Show the full unmasked token
    #[usage(long)]
    pub(crate) unmask: bool,
}

impl Github {
    pub(crate) fn run(self) -> eyre::Result<()> {
        if let Some(operation) = &self.git_credential {
            return super::git_credential::run(operation);
        }
        let resolved = if self.oauth {
            Some((
                github::oauth::token(github::oauth::TokenRequest {
                    host: self.host.clone(),
                    allow_device_flow: true,
                    force_refresh: self.refresh,
                    ..Default::default()
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
