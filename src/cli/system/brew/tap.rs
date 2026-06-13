use eyre::Result;

/// Tap a Homebrew formula repository
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct SystemBrewTap {
    /// Tap name, e.g. `owner/repo`
    tap: String,

    /// Git URL for non-GitHub or otherwise custom taps
    #[clap(value_hint = clap::ValueHint::Url)]
    url: Option<String>,

    /// Print the command that would run without running it
    #[clap(long, short = 'n')]
    dry_run: bool,
}

impl SystemBrewTap {
    pub async fn run(self) -> Result<()> {
        crate::system::packages::brew::tap(&self.tap, self.url.as_deref(), self.dry_run).await
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise bootstrap packages brew tap railwaycat/emacsmacport</bold>
    $ <bold>mise bootstrap packages brew tap acme/tools https://git.example.com/acme/homebrew-tools.git</bold>
"#
);
