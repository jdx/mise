use crate::github;

/// Authorize native GitHub OAuth device-flow tokens
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, hide = true)]
pub struct Login {
    /// GitHub hostname
    #[clap(default_value = "github.com")]
    host: String,
}

impl Login {
    pub fn run(self) -> eyre::Result<()> {
        let token = github::oauth::token(github::oauth::TokenRequest {
            host: self.host.clone(),
            force_device_flow: true,
        })?;
        miseprintln!(
            "{}: {} (source: GitHub OAuth)",
            self.host,
            crate::tokens::mask_token(&token)
        );
        Ok(())
    }
}
