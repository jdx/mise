use eyre::Result;

/// Untap Homebrew formula repositories
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, visible_aliases = ["remove", "rm"], after_long_help = AFTER_LONG_HELP)]
pub struct SystemBrewUntap {
    /// Tap name(s), e.g. `owner/repo`
    #[clap(required = true)]
    taps: Vec<String>,

    /// Print the command that would run without running it
    #[clap(long, short = 'n')]
    dry_run: bool,
}

impl SystemBrewUntap {
    pub async fn run(self) -> Result<()> {
        crate::system::packages::brew::untap(&self.taps, self.dry_run).await
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise bootstrap packages brew untap railwaycat/emacsmacport</bold>
"#
);
