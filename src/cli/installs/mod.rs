use eyre::{Result, bail};

use crate::config::Settings;

mod ls;
mod migrate;
mod select;

/// [experimental] Inspect and choose identity-layout installations
///
/// With `install_layout = "identity"`, each installation lives in its own
/// `<label>-<hash>` directory, and several installations of one version can
/// exist side by side (different options, platforms, pinned artifacts, or a
/// refreshed copy). These commands list them, choose which one mise uses when
/// no lockfile pins one, and move installations made before you turned the
/// layout on into it. With no subcommand, runs `mise installs ls`. See
/// https://mise.jdx.dev/dev-tools/install-layout.html.
#[derive(Debug, usage_rs::Args)]
#[usage(verbatim_doc_comment)]
pub(crate) struct Installs {
    #[usage(subcommand)]
    command: Option<Commands>,
}

#[derive(Debug, usage_rs::Subcommands)]
enum Commands {
    Ls(ls::InstallsLs),
    Migrate(migrate::InstallsMigrate),
    Select(select::InstallsSelect),
}

impl Installs {
    pub(crate) async fn run(self) -> Result<()> {
        Settings::get().ensure_experimental("mise installs")?;
        if !crate::install_layout::resolver::enabled() {
            bail!(
                "the identity install layout is off; turn it on with `install_layout = \"identity\"`"
            );
        }
        match self
            .command
            .unwrap_or(Commands::Ls(ls::InstallsLs::default()))
        {
            Commands::Ls(cmd) => cmd.run().await,
            Commands::Migrate(cmd) => cmd.run().await,
            Commands::Select(cmd) => cmd.run(),
        }
    }
}
