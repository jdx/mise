use clap::Subcommand;

mod build;
mod common;
mod push;
mod run;

/// [experimental] Build OCI container images from a mise.toml
///
/// Each tool becomes its own OCI layer, so bumping any single tool version
/// only invalidates one content-addressable blob — unlike a Dockerfile where
/// changing an early `RUN` invalidates every layer above it.
///
/// This command is experimental and requires `mise settings experimental=true`
/// (or `MISE_EXPERIMENTAL=1`). Behavior, flags, and output layout may change
/// in future releases.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment)]
pub struct Oci {
    #[clap(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Build(build::Build),
    Push(push::Push),
    Run(run::Run),
}

impl Commands {
    pub async fn run(self) -> eyre::Result<()> {
        match self {
            Self::Build(cmd) => cmd.run().await,
            Self::Push(cmd) => cmd.run().await,
            Self::Run(cmd) => cmd.run().await,
        }
    }
}

impl Oci {
    pub async fn run(self) -> eyre::Result<()> {
        self.command.run().await
    }
}
