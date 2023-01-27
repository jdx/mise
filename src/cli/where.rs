use color_eyre::eyre::Result;

use crate::cli::args::runtime::{RuntimeArg, RuntimeArgParser};
use crate::cli::command::Command;
use crate::config::Config;
use crate::output::Output;
use crate::runtimes::RuntimeVersion;

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

        let prefix = config.resolve_alias(&self.runtime.plugin, prefix);
        let rtv = RuntimeVersion::find_by_version_prefix(&self.runtime.plugin, &prefix)?;

        rtxprintln!(out, "{}", rtv.install_path.to_string_lossy());
        Ok(())
    }
}

const AFTER_LONG_HELP: &str = r#"
Examples:
  $ rtx where nodejs@20
  /Users/jdx/.local/share/rtx/installs/nodejs/20.13.0
"#;

#[cfg(test)]
mod test {
    use pretty_assertions::assert_str_eq;

    use crate::assert_cli;
    use crate::dirs;

    use super::*;

    #[test]
    fn test_where() {
        assert_cli!("plugin", "add", "shfmt");
        assert_cli!("install");
        let Output { stdout, .. } = assert_cli!("where", "shfmt");
        assert_str_eq!(
            stdout.content.trim(),
            dirs::ROOT.join("installs/shfmt/3.6.0").to_string_lossy()
        );
    }

    #[test]
    fn test_where_alias() {
        assert_cli!("plugin", "add", "shfmt");
        assert_cli!("install", "shfmt@my/alias");
        let Output { stdout, .. } = assert_cli!("where", "shfmt@my/alias");
        assert_str_eq!(
            stdout.content.trim(),
            dirs::ROOT.join("installs/shfmt/3.0.2").to_string_lossy()
        );
    }
}
