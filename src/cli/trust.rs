use std::path::PathBuf;

use color_eyre::eyre::Result;
use console::style;
use indoc::formatdoc;
use once_cell::sync::Lazy;

use crate::cli::command::Command;
use crate::cli::local;
use crate::config::{config_file, Config};
use crate::output::Output;

/// Marks a config file as trusted
///
/// This means rtx will parse the file with potentially dangerous
/// features enabled.
///
/// This includes:
/// - environment variables
/// - templates
/// - `path:` plugin versions
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP.as_str())]
pub struct Trust {
    /// The config file to trust
    pub config_file: Option<String>,

    /// No longer trust this config
    #[clap(long)]
    pub untrust: bool,
}

impl Command for Trust {
    fn run(self, _config: Config, out: &mut Output) -> Result<()> {
        let path = match &self.config_file {
            Some(filename) => PathBuf::from(filename),
            None => local::get_parent_path()?,
        };
        if self.untrust {
            config_file::untrust(&path)?;
            rtxprintln!(out, "untrusted {}", &path.canonicalize()?.display());
        } else {
            config_file::trust(&path)?;
            rtxprintln!(out, "trusted {}", &path.canonicalize()?.display());
        }
        Ok(())
    }
}

static AFTER_LONG_HELP: Lazy<String> = Lazy::new(|| {
    formatdoc! {r#"
    {}
      # trusts ~/some_dir/.rtx.toml
      rtx trust ~/some_dir/.rtx.toml

      # trusts .rtx.toml in the current or parent directory
      rtx trust
    "#, style("Examples:").bold().underlined()}
});

#[cfg(test)]
mod tests {
    use crate::assert_cli_snapshot;

    #[test]
    fn test_trust() {
        assert_cli_snapshot!("trust");
        assert_cli_snapshot!("trust", "--untrust");
        assert_cli_snapshot!("trust", ".test-tool-versions");
    }
}
