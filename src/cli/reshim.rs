use eyre::Result;

use crate::config::Config;
use crate::shims;
use crate::toolset::ToolsetBuilder;

/// Create shims for executables provided by installed tools
///
/// Run this when an executable was added to an existing installation outside
/// mise, for example after a language package manager installed a CLI globally.
/// It rebuilds your shim directory (~/.local/share/mise/shims by default).
/// `--system` rebuilds the system shim directory used by `mise install --system`.
///
/// Shims are created for all installed versions. When invoked, a shim picks the
/// version to run from the current config. `--force` rebuilds mise-owned shims;
/// it does not turn unrelated files into mise-owned shims.
/// See https://mise.jdx.dev/dev-tools/shims.html
#[derive(Debug, usage_rs::Args)]
#[usage(
    verbatim_doc_comment,
    example(
        r###"mise reshim
~/.local/share/mise/shims/node -v
v24.11.0"###,
        help = "Rebuild shims, then run node through its shim"
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

    /// Rebuild the system shims, for tools installed with `mise install --system`
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
