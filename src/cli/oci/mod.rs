use clap::Subcommand;

mod build;

/// Build OCI container images from a mise.toml
///
/// Each tool becomes its own OCI layer, so bumping any single tool version
/// only invalidates one content-addressable blob — unlike a Dockerfile where
/// changing an early `RUN` invalidates every layer above it.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment)]
pub struct Oci {
    #[clap(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Build(build::Build),
}

impl Commands {
    pub async fn run(self) -> eyre::Result<()> {
        match self {
            Self::Build(cmd) => cmd.run().await,
        }
    }
}

impl Oci {
    pub async fn run(self) -> eyre::Result<()> {
        self.command.run().await
    }
}
