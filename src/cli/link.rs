use std::path::PathBuf;

use clap::ValueHint;
use color_eyre::eyre::{eyre, Result};
use console::style;
use eyre::bail;
use path_absolutize::Absolutize;

use crate::cli::args::ToolArg;
use crate::config::Config;
use crate::file;
use crate::file::{make_symlink, remove_all};

/// Symlinks a tool version into mise
///
/// Use this for adding installs either custom compiled outside
/// mise or built with a different tool.
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "ln", verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Link {
    /// Tool name and version to create a symlink for
    #[clap(value_name = "TOOL@VERSION")]
    tool: ToolArg,

    /// The local path to the tool version
    /// e.g.: ~/.nvm/versions/node/v20.0.0
    #[clap(value_hint = ValueHint::DirPath, verbatim_doc_comment)]
    path: PathBuf,

    /// Overwrite an existing tool version if it exists
    #[clap(long, short = 'f')]
    force: bool,
}

impl Link {
    pub fn run(self) -> Result<()> {
        let config = Config::try_get()?;
        let version = match self.tool.tvr {
            Some(ref tvr) => tvr.version(),
            None => bail!("must provide a version for {}", self.tool.style()),
        };
        let path = self.path.absolutize()?;
        if !path.exists() {
            warn!(
                "Target path {} does not exist",
                style(path.to_string_lossy()).cyan().for_stderr()
            );
        }
        let target = self.tool.backend.installs_path.join(version);
        if target.exists() {
            if self.force {
                remove_all(&target)?;
            } else {
                return Err(eyre!(
                    "Tool version {} already exists, use {} to overwrite",
                    self.tool.style(),
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
    # build node-20.0.0 with node-build and link it into mise
    $ <bold>node-build 20.0.0 ~/.nodes/20.0.0</bold>
    $ <bold>mise link node@20.0.0 ~/.nodes/20.0.0</bold>

    # have mise use the python version provided by Homebrew
    $ <bold>brew install node</bold>
    $ <bold>mise link node@brew $(brew --prefix node)</bold>
    $ <bold>mise use node@brew</bold>
"#
);

#[cfg(test)]
mod tests {
    use crate::file::create_dir_all;
    use crate::test::reset;
    use test_log::test;

    #[test]
    fn test_link() {
        reset();
        assert_cli!("install", "tiny@1.0.1", "tiny@2.1.0");
        assert_cli!("install", "tiny@3.0.1", "tiny@3.1.0");
        create_dir_all("../data/tmp/tiny").unwrap();
        assert_cli!("link", "tiny@9.8.7", "../data/tmp/tiny");
        assert_cli_snapshot!("ls", "tiny", @r###"
        tiny  1.0.1                                       
        tiny  2.1.0                                       
        tiny  3.0.1                                       
        tiny  3.1.0            ~/cwd/.test-tool-versions 3
        tiny  9.8.7 (symlink)
        "###);
        assert_cli!("uninstall", "tiny@9.8.7");
    }
}
