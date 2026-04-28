//! `mise wings <verb>` — manage authentication against the
//! [mise-wings](https://mise-wings.en.dev) asset cache.
//!
//! Four operations:
//!
//!   - [`login`] — exchange a Clerk frontend session JWT for
//!     a wings session JWT (+ refresh token), persist locally.
//!   - [`logout`] — revoke every active wings session for the
//!     calling user; delete the local credentials file.
//!   - [`whoami`] — print the active user / org / token expiry.
//!   - [`status`] — verify credentials are live by hitting an
//!     authenticated proxy endpoint.
//!
//! With `wings.enabled = true` AND credentials present, mise's
//! HTTP client transparently routes `npm`/`gh`/`gh-api` URLs
//! through the wings cache subdomains. No behavior change
//! without both halves of the gate, so an `mise wings login`
//! that doesn't also flip `wings.enabled` is a no-op until the
//! user opts in (typically in `mise.toml`).

use clap::Subcommand;
use eyre::Result;

mod login;
mod logout;
mod status;
mod whoami;

/// Manage `mise wings` authentication
///
/// `mise-wings` is a paid asset cache for tool installs. Run
/// `mise wings login` once to authenticate; subsequent installs
/// route through the regional cache automatically when
/// `wings.enabled` is set.
///
/// Bare `mise wings` with no subcommand prints the same status
/// summary as `mise wings status`.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment)]
pub struct Wings {
    #[clap(subcommand)]
    command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Login(login::Login),
    Logout(logout::Logout),
    Status(status::Status),
    Whoami(whoami::Whoami),
}

impl Commands {
    pub async fn run(self) -> Result<()> {
        match self {
            Self::Login(cmd) => cmd.run().await,
            Self::Logout(cmd) => cmd.run().await,
            Self::Status(cmd) => cmd.run().await,
            Self::Whoami(cmd) => cmd.run().await,
        }
    }
}

impl Wings {
    pub async fn run(self) -> Result<()> {
        match self.command {
            Some(cmd) => cmd.run().await,
            None => status::Status::default().run().await,
        }
    }
}
