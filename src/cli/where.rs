use color_eyre::eyre::Result;

use crate::cli::args::runtime::{RuntimeArg, RuntimeArgParser};
use crate::cli::command::Command;
use crate::config::Config;
use crate::errors::Error::VersionNotInstalled;
use crate::output::Output;

/// Display the installation path for a runtime
///
/// Must be installed.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP, hide = true)]
pub struct Where {
    /// runtime(s) to remove
    #[clap(required = true, value_parser = RuntimeArgParser)]
    runtime: RuntimeArg,

    /// the version prefix to use when querying the latest version
    /// same as the first argument after the "@"
    /// used for asdf compatibility
    #[clap(hide = true)]
    asdf_version: Option<String>,
}

impl Command for Where {
    fn run(self, config: Config, out: &mut Output) -> Result<()> {
        let prefix = match self.runtime.version.as_str() {
            "latest" => match self.asdf_version {
                Some(version) => version,
                None => "".to_string(),
            },
            v => v.into(),
        };

        let rtv = config
            .ts
            .find_by_prefix(&config.aliases, &self.runtime.plugin, prefix.as_str());

        match rtv {
            Some(rtv) => {
                rtxprintln!(out, "{}", rtv.install_path.to_string_lossy());
                Ok(())
            }
            None => Err(VersionNotInstalled(
                self.runtime.plugin,
                self.runtime.version,
            ))?,
        }
    }
}

const AFTER_LONG_HELP: &str = r#"
Examples:
  $ rtx where nodejs@20
  /Users/jdx/.local/share/rtx/installs/nodejs/20.13.0
"#;

#[cfg(test)]
mod test {
    use insta::assert_display_snapshot;
    use pretty_assertions::assert_str_eq;

    use crate::dirs;
    use crate::{assert_cli, assert_cli_err};

    #[test]
    fn test_where() {
        assert_cli!("plugin", "add", "shfmt");
        assert_cli!("install");
        let stdout = assert_cli!("where", "shfmt");
        assert_str_eq!(
            stdout.trim(),
            dirs::ROOT.join("installs/shfmt/3.5.2").to_string_lossy()
        );
    }

    #[test]
    fn test_where_alias() {
        assert_cli!("plugin", "add", "shfmt");
        assert_cli!("install", "shfmt@my/alias");
        let stdout = assert_cli!("where", "shfmt@my/alias");
        assert_str_eq!(
            stdout.trim(),
            dirs::ROOT.join("installs/shfmt/3.0.2").to_string_lossy()
        );
    }

    #[test]
    fn test_where_not_found() {
        let err = assert_cli_err!("where", "shfmt@1111");
        assert_display_snapshot!(err, @"[shfmt] version 1111 not installed");
    }
}
