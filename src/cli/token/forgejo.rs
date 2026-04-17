use crate::forgejo;
use crate::tokens;

/// Display the Forgejo token mise will use for a given host
///
/// Shows which token source mise would use, useful for debugging
/// authentication issues. The token is masked by default.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Forgejo {
    /// Forgejo hostname
    #[clap(default_value = "codeberg.org")]
    host: String,

    /// Show the full unmasked token
    #[clap(long)]
    unmask: bool,
}

impl Forgejo {
    pub fn run(self) -> eyre::Result<()> {
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

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise token forgejo</bold>
    codeberg.org: a180…61f6 (source: FORGEJO_TOKEN)

    $ <bold>mise token forgejo --unmask</bold>
    codeberg.org: a18099ca69064be387fbe37b8ad1d333758361f6 (source: FORGEJO_TOKEN)

    $ <bold>mise token forgejo forgejo.mycompany.com</bold>
    forgejo.mycompany.com: (none)
"#
);
