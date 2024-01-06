use std::path::{Path, PathBuf};

use clap::ValueHint;
use console::style;
use miette::{IntoDiagnostic, Result};
use path_absolutize::Absolutize;

use crate::file::{make_symlink, remove_all};

use crate::plugins::unalias_plugin;
use crate::{dirs, file};

/// Symlinks a plugin into mise
///
/// This is used for developing a plugin.
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "ln", verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct PluginsLink {
    /// The name of the plugin
    /// e.g.: node, ruby
    #[clap(verbatim_doc_comment)]
    name: String,

    /// The local path to the plugin
    /// e.g.: ./mise-node
    #[clap(value_hint = ValueHint::DirPath, verbatim_doc_comment)]
    path: Option<PathBuf>,

    /// Overwrite existing plugin
    #[clap(long, short = 'f')]
    force: bool,
}

impl PluginsLink {
    pub fn run(self) -> Result<()> {
        let (name, path) = match self.path {
            Some(path) => (self.name, path),
            None => {
                let path = PathBuf::from(PathBuf::from(&self.name).absolutize().into_diagnostic()?);
                let name = get_name_from_path(&path);
                (name, path)
            }
        };
        let name = unalias_plugin(&name);
        let path = path.absolutize().into_diagnostic()?;
        let symlink = dirs::PLUGINS.join(name);
        if symlink.exists() {
            if self.force {
                remove_all(&symlink)?;
            } else {
                return Err(miette!(
                    "plugin {} already exists, use --force to overwrite",
                    style(&name).blue().for_stderr()
                ));
            }
        }
        file::create_dir_all(&*dirs::PLUGINS)?;
        make_symlink(&path, &symlink)?;
        Ok(())
    }
}

fn get_name_from_path(path: &Path) -> String {
    let name = path.file_name().unwrap().to_str().unwrap();
    let name = name.strip_prefix("asdf-").unwrap_or(name);
    let name = name.strip_prefix("rtx-").unwrap_or(name);
    let name = name.strip_prefix("mise-").unwrap_or(name);
    unalias_plugin(name).to_string()
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>
  # essentially just `ln -s ./mise-node ~/.local/share/mise/plugins/node`
  $ <bold>mise plugins link node ./mise-node</bold>

  # infer plugin name as "node"
  $ <bold>mise plugins link ./mise-node</bold>
"#
);

#[cfg(test)]
mod tests {

    #[test]
    fn test_plugin_link() {
        assert_cli_snapshot!("plugin", "link", "-f", "tiny-link", "../data/plugins/tiny", @"");
        assert_cli_snapshot!("plugins", "ls", @r###"
        dummy
        tiny
        tiny-link
        "###);
        assert_cli_snapshot!("plugin", "uninstall", "tiny-link", @"");
    }
}
