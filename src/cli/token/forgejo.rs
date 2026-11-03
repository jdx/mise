use crate::forgejo;
use crate::tokens;

/// Show the Forgejo token mise uses for a host
///
/// Shows the token and where it came from, to debug authentication. The token
/// is masked unless you pass --unmask.
#[derive(Debug, usage_rs::Args)]
#[usage(
    verbatim_doc_comment,
    example(
        r###"mise token forgejo
codeberg.org: xxxx…xxxx (source: FORGEJO_TOKEN)"###,
        help = "Show the token for codeberg.org"
    ),
    example(
        r###"mise token forgejo --unmask
codeberg.org: xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx (source: FORGEJO_TOKEN)"###,
        help = "Show the whole token"
    ),
    example(
        r###"mise token forgejo forgejo.mycompany.com
forgejo.mycompany.com: (none)"###,
        help = "Check a self-hosted Forgejo instance"
    )
)]
pub(super) struct Forgejo {
    /// Forgejo hostname
    #[usage(default = "codeberg.org")]
    host: String,

    /// Show the full unmasked token
    #[usage(long)]
    unmask: bool,
}

impl Forgejo {
    pub(super) fn run(self) -> eyre::Result<()> {
        match forgejo::resolve_token(&self.host) {
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
