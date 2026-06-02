use eyre::Result;

/// Show the companies sponsoring mise and the en.dev project family
#[derive(Debug, clap::Args)]
pub struct Sponsors;

impl Sponsors {
    pub fn run(&self) -> Result<()> {
        miseprintln!(
            "mise and the en.dev project family are sponsored by:\n\n  37signals - https://37signals.com\n\nView all sponsors: https://en.dev/sponsors.html"
        );
        Ok(())
    }
}
