mod build;
mod common;
mod push;
mod run;

/// [experimental] Build, push, and run OCI images of a project's tools
///
/// Each tool version becomes its own image layer, so changing one tool version
/// rebuilds that tool's layer instead of the whole image.
///
/// Requires `mise settings experimental=true` (or `MISE_EXPERIMENTAL=1`).
/// Behavior, flags, and output layout may change in future releases.
/// See https://mise.jdx.dev/dev-tools/mise-oci.html
#[derive(Debug, usage_rs::Args)]
#[usage(verbatim_doc_comment)]
pub(crate) struct Oci {
    #[usage(subcommand)]
    command: Commands,
}

#[derive(Debug, usage_rs::Subcommands)]
enum Commands {
    Build(build::Build),
    Push(push::Push),
    Run(run::Run),
}

impl Commands {
    pub(crate) async fn run(self) -> eyre::Result<()> {
        match self {
            Self::Build(cmd) => cmd.run().await,
            Self::Push(cmd) => cmd.run().await,
            Self::Run(cmd) => cmd.run().await,
        }
    }
}

impl Oci {
    pub(crate) async fn run(self) -> eyre::Result<()> {
        self.command.run().await
    }
}
