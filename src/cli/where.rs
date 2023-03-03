use color_eyre::eyre::Result;
use console::style;
use indoc::formatdoc;
use once_cell::sync::Lazy;

use crate::cli::args::runtime::{RuntimeArg, RuntimeArgParser, RuntimeArgVersion};
use crate::cli::command::Command;
use crate::config::Config;
use crate::errors::Error::VersionNotInstalled;
use crate::output::Output;
use crate::toolset::ToolsetBuilder;

/// Display the installation path for a runtime
///
/// Must be installed.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP.as_str())]
pub struct Where {
    /// Runtime(s) to look up
    /// e.g.: ruby@3
    /// if "@<PREFIX>" is specified, it will show the latest installed version that matches the prefix
    /// otherwise, it will show the current, active installed version
    #[clap(required = true, value_parser = RuntimeArgParser, verbatim_doc_comment)]
    runtime: RuntimeArg,

    /// the version prefix to use when querying the latest version
    /// same as the first argument after the "@"
    /// used for asdf compatibility
    #[clap(hide = true, verbatim_doc_comment)]
    asdf_version: Option<String>,
}

impl Command for Where {
    fn run(self, mut config: Config, out: &mut Output) -> Result<()> {
        let runtime = match self.runtime.version {
            RuntimeArgVersion::None => match self.asdf_version {
                Some(version) => self
                    .runtime
                    .with_version(RuntimeArgVersion::Version(version)),
                None => self.runtime,
            },
            _ => self.runtime,
        };

        let ts = ToolsetBuilder::new()
            .with_args(&[runtime.clone()])
            .build(&mut config);

        match ts.resolve_runtime_arg(&runtime) {
            Some(rtv) if rtv.is_installed() => {
                rtxprintln!(out, "{}", rtv.install_path.to_string_lossy());
                Ok(())
            }
            _ => Err(VersionNotInstalled(
                runtime.plugin.to_string(),
                runtime.version.to_string(),
            ))?,
        }
    }
}

static AFTER_LONG_HELP: Lazy<String> = Lazy::new(|| {
    formatdoc! {r#"
    {}
      # Show the latest installed version of nodejs
      # If it is is not installed, errors
      $ rtx where nodejs@18
      /home/jdx/.local/share/rtx/installs/nodejs/18.0.0

      # Show the current, active install directory of nodejs
      # Errors if nodejs is not referenced in any .tool-version file
      $ rtx where nodejs
      /home/jdx/.local/share/rtx/installs/nodejs/18.0.0
    "#, style("Examples:").bold().underlined()}
});

#[cfg(test)]
mod tests {
    use insta::assert_display_snapshot;
    use pretty_assertions::assert_str_eq;

    use crate::{assert_cli, assert_cli_err};
    use crate::{assert_cli_snapshot, dirs};

    #[test]
    fn test_where() {
        assert_cli!("install");
        let stdout = assert_cli!("where", "tiny");
        assert_str_eq!(
            stdout.trim(),
            dirs::ROOT.join("installs/tiny/3.1.0").to_string_lossy()
        );
    }

    #[test]
    fn test_where_asdf_style() {
        assert_cli!("install", "tiny@2", "tiny@3");
        assert_cli_snapshot!("where", "tiny", "2");
        assert_cli_snapshot!("where", "tiny", "3");
    }

    #[test]
    fn test_where_alias() {
        assert_cli!("install", "tiny@my/alias");
        let stdout = assert_cli!("where", "tiny@my/alias");
        assert_str_eq!(
            stdout.trim(),
            dirs::ROOT.join("installs/tiny/3.0.1").to_string_lossy()
        );
        assert_cli!("uninstall", "tiny@my/alias");
    }

    #[test]
    fn test_where_not_found() {
        let err = assert_cli_err!("where", "tiny@1111");
        assert_display_snapshot!(err, @"tiny@1111 not installed");
    }
}
