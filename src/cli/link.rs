use std::path::PathBuf;

use clap::ValueHint;
use color_eyre::eyre::{eyre, Result};
use console::style;
use path_absolutize::Absolutize;

use crate::cli::args::tool::{ToolArg, ToolArgParser};
use crate::cli::command::Command;
use crate::config::Config;
use crate::file::{make_symlink, remove_all};
use crate::output::Output;
use crate::{dirs, file};

/// Symlinks a tool version into rtx
///
/// Use this for adding installs either custom compiled outside
/// rtx or built with a different tool.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Link {
    /// Tool name and version to create a symlink for
    #[clap(value_name = "TOOL@VERSION", value_parser = ToolArgParser)]
    tool: ToolArg,

    /// The local path to the tool version
    /// e.g.: ~/.nvm/versions/node/v20.0.0
    #[clap(value_hint = ValueHint::DirPath, verbatim_doc_comment)]
    path: PathBuf,

    /// Overwrite an existing tool version if it exists
    #[clap(long, short = 'f')]
    force: bool,
}

impl Command for Link {
    fn run(self, mut config: Config, _out: &mut Output) -> Result<()> {
        let version = match self.tool.tvr {
            Some(ref tvr) => tvr.version(),
            None => {
                return Err(eyre!(
                    "must provide a version for {}",
                    style(&self.tool).cyan().for_stderr()
                ));
            }
        };
        let path = self.path.absolutize()?;
        if !path.exists() {
            warn!(
                "Target path {} does not exist",
                style(path.to_string_lossy()).cyan().for_stderr()
            );
        }
        let target = dirs::INSTALLS.join(&self.tool.plugin).join(version);
        if target.exists() {
            if self.force {
                remove_all(&target)?;
            } else {
                return Err(eyre!(
                    "Tool version {} already exists, use {} to overwrite",
                    style(&self.tool).cyan().for_stderr(),
                    style("--force").yellow().for_stderr()
                ));
            }
        }
        file::create_dir_all(target.parent().unwrap())?;
        make_symlink(&path, &target)?;

        config.rebuild_shims_and_runtime_symlinks()
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>
  # build node-20.0.0 with node-build and link it into rtx
  $ <bold>node-build 20.0.0 ~/.nodes/20.0.0</bold>
  $ <bold>rtx link node@20.0.0 ~/.nodes/20.0.0</bold>

  # have rtx use the python version provided by Homebrew
  $ <bold>brew install node</bold>
  $ <bold>rtx link node@brew $(brew --prefix node)</bold>
  $ <bold>rtx use node@brew</bold>
"#
);

#[cfg(test)]
mod tests {
    use crate::file::create_dir_all;
    use crate::{assert_cli, assert_cli_snapshot};

    #[test]
    fn test_link() {
        create_dir_all("../data/tmp/tiny").unwrap();
        assert_cli!("link", "tiny@9.8.7", "../data/tmp/tiny");
        assert_cli_snapshot!("ls", "tiny");
        assert_cli!("uninstall", "tiny@9.8.7");
    }
}
