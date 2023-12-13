use std::path::PathBuf;

use clap::ValueHint;
use color_eyre::eyre::Result;

use crate::cli::local;
use crate::config::{config_file, Config};

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
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Trust {
    /// The config file to trust
    #[clap(value_hint = ValueHint::FilePath, verbatim_doc_comment)]
    pub config_file: Option<String>,

    /// No longer trust this config
    #[clap(long)]
    pub untrust: bool,
}

impl Trust {
    pub fn run(self, _config: Config) -> Result<()> {
        let path = match &self.config_file {
            Some(filename) => PathBuf::from(filename),
            None => local::get_parent_path()?,
        };
        if self.untrust {
            config_file::untrust(&path)?;
            rtxprintln!("untrusted {}", &path.canonicalize()?.display());
        } else {
            config_file::trust(&path)?;
            rtxprintln!("trusted {}", &path.canonicalize()?.display());
        }
        Ok(())
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>
  # trusts ~/some_dir/.rtx.toml
  $ <bold>rtx trust ~/some_dir/.rtx.toml</bold>

  # trusts .rtx.toml in the current or parent directory
  $ <bold>rtx trust</bold>
"#
);

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
