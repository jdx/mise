use eyre::Result;

mod forget;
mod pins;

/// The signers mise accepts packslips from
///
/// A tool installed with the `packslip:` backend is verified against the
/// identity its project name implies, and mise then remembers which signer
/// it accepted, the way SSH remembers hosts. A later release from another
/// signer, a weaker scheme, a repackager where the vendor signed before, or
/// one that drops build provenance is refused until a person says so.
#[derive(Debug, usage_rs::Args)]
#[usage(verbatim_doc_comment)]
pub(crate) struct Packslip {
    #[usage(subcommand)]
    command: Option<Commands>,
}

#[derive(Debug, usage_rs::Subcommands)]
enum Commands {
    Forget(forget::PackslipForget),
    Pins(pins::PackslipPins),
}

impl Commands {
    fn run(self) -> Result<()> {
        match self {
            Self::Forget(cmd) => cmd.run(),
            Self::Pins(cmd) => cmd.run(),
        }
    }
}

impl Packslip {
    pub(crate) async fn run(self) -> Result<()> {
        self.command
            .unwrap_or(Commands::Pins(pins::PackslipPins::default()))
            .run()
    }
}
