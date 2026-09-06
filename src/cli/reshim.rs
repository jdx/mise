use eyre::Result;

use crate::config::Config;
use crate::shims;
use crate::toolset::ToolsetBuilder;

/// Create shims for the executables of currently installed tools
///
/// This creates shims in the user shim directory for executables that have been added since
/// the last reshim. With `--system`, it rebuilds the system shim farm instead.
/// mise runs this automatically after commands like `npm i -g`, but other ways of installing
/// executables (such as yarn or pnpm for node) are not detected, so call this explicitly then.
///
/// If you think mise should automatically call this for a particular command, please
/// open an issue on the mise repo. You can also set up a shell function to reshim
/// automatically (it's really fast so you don't need to worry about overhead):
///
///     npm() {
///       command npm "$@"
///       mise reshim
///     }
///
/// Note that this creates shims for _all_ installed tools, not just the ones that are
/// currently active in mise.toml.
#[derive(Debug, usage_rs::Args)]
#[usage(
    verbatim_doc_comment,
    example(
        r###"mise reshim
~/.local/share/mise/shims/node -v
v20.0.0"###
    )
)]
pub(crate) struct Reshim {
    #[usage(hide = true)]
    pub tool: Option<String>,
    #[usage(hide = true)]
    pub version: Option<String>,

    /// Rebuild all mise-owned shims
    #[usage(long, short)]
    pub force: bool,

    /// Rebuild the system shim farm
    #[usage(long)]
    pub system: bool,
}

impl Reshim {
    pub(crate) async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        let ts = ToolsetBuilder::new().build(&config).await?;

        let scope = if self.system {
            shims::ShimScope::System
        } else {
            shims::ShimScope::User
        };
        shims::reshim_for(&config, &ts, self.force, scope).await
    }
}
