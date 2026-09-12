use eyre::Result;

/// Show the companies sponsoring mise and the jdx.dev open source tools
#[derive(Debug, usage_rs::Args)]
pub(crate) struct Sponsors;

impl Sponsors {
    pub(crate) fn run(&self) -> Result<()> {
        miseprintln!(
            "mise and the jdx.dev open source tools are sponsored by:\n\n  entire.io - https://entire.io\n  Omacom Foundation - https://omarchy.org/patrons/\n\nView all sponsors: https://jdx.dev/sponsors.html"
        );
        Ok(())
    }
}
