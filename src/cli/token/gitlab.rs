use crate::gitlab;
use crate::tokens;

/// Show the GitLab token mise uses for a host
///
/// Shows the token and where it came from, to debug authentication. The token
/// is masked unless you pass --unmask.
#[derive(Debug, usage_rs::Args)]
#[usage(
    verbatim_doc_comment,
    example(
        r###"mise token gitlab
gitlab.com: glpa…xxxx (source: GITLAB_TOKEN)"###,
        help = "Show the token for gitlab.com"
    ),
    example(
        r###"mise token gitlab --unmask
gitlab.com: glpat-xxxxxxxxxxxx (source: GITLAB_TOKEN)"###,
        help = "Show the whole token"
    ),
    example(
        r###"mise token gitlab gitlab.mycompany.com
gitlab.mycompany.com: (none)"###,
        help = "Check a self-managed GitLab instance"
    )
)]
pub(super) struct Gitlab {
    /// GitLab hostname
    #[usage(default = "gitlab.com")]
    host: String,

    /// Show the full unmasked token
    #[usage(long)]
    unmask: bool,
}

impl Gitlab {
    pub(super) fn run(self) -> eyre::Result<()> {
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
