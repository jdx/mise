use std::io::{BufRead, Write};

/// Supply mise's GitHub token to Git's credential protocol.
#[derive(Debug, usage_rs::Args)]
#[usage(hide = true)]
pub(crate) struct GitCredential {
    operation: String,
}

impl GitCredential {
    pub(crate) fn run(&self) -> eyre::Result<()> {
        if self.operation != "get" {
            return Ok(());
        }
        let mut protocol = String::new();
        let mut host = String::new();
        for line in std::io::stdin().lock().lines() {
            let line = line?;
            if line.is_empty() {
                break;
            }
            if let Some(value) = line.strip_prefix("protocol=") {
                protocol = value.to_owned();
            } else if let Some(value) = line.strip_prefix("host=") {
                host = value.to_owned();
            }
        }
        if protocol == "https"
            && host == "github.com"
            && let Some((token, _)) = crate::github::resolve_token_for_git(&host)
            && !token.is_empty()
            && !token.contains(['\n', '\r', '\0'])
        {
            // Write directly: normal output logging/redaction must never see it.
            write!(
                std::io::stdout().lock(),
                "username=x-access-token\npassword={token}\n\n"
            )?;
        }
        Ok(())
    }
}
