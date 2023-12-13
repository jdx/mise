use std::path::PathBuf;

use color_eyre::eyre::Result;
use itertools::sorted;

use crate::config::Config;
use crate::env::{NODENV_ROOT, NVM_DIR};
use crate::file;
use crate::plugins::PluginName;
use crate::{cmd, dirs};

/// Symlinks all tool versions from an external tool into rtx
///
/// For example, use this to import all Homebrew node installs into rtx
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct SyncNode {
    #[clap(flatten)]
    _type: SyncNodeType,
}

#[derive(Debug, clap::Args)]
#[group(required = true)]
pub struct SyncNodeType {
    /// Get tool versions from Homebrew
    #[clap(long)]
    brew: bool,

    /// Get tool versions from nvm
    #[clap(long)]
    nvm: bool,

    /// Get tool versions from nodenv
    #[clap(long)]
    nodenv: bool,
}

impl SyncNode {
    pub fn run(self, config: Config) -> Result<()> {
        if self._type.brew {
            self.run_brew(config)?;
        } else if self._type.nvm {
            self.run_nvm(config)?;
        } else if self._type.nodenv {
            self.run_nodenv(config)?;
        }
        Ok(())
    }

    fn run_brew(self, config: Config) -> Result<()> {
        let tool = config.get_or_create_plugin(&PluginName::from("node"));

        let brew_prefix = PathBuf::from(cmd!("brew", "--prefix").read()?).join("opt");
        let installed_versions_path = dirs::INSTALLS.join("node");

        file::remove_symlinks_with_target_prefix(&installed_versions_path, &brew_prefix)?;

        let subdirs = file::dir_subdirs(&brew_prefix)?;
        for entry in sorted(subdirs) {
            if !entry.starts_with("node@") {
                continue;
            }
            let v = entry.trim_start_matches("node@");
            tool.create_symlink(v, &brew_prefix.join(&entry))?;
            rtxprintln!("Synced node@{} from Homebrew", v);
        }

        config.rebuild_shims_and_runtime_symlinks()
    }

    fn run_nvm(self, config: Config) -> Result<()> {
        let tool = config.get_or_create_plugin(&PluginName::from("node"));

        let nvm_versions_path = NVM_DIR.join("versions").join("node");
        let installed_versions_path = dirs::INSTALLS.join("node");

        file::remove_symlinks_with_target_prefix(&installed_versions_path, &nvm_versions_path)?;

        let subdirs = file::dir_subdirs(&nvm_versions_path)?;
        for entry in sorted(subdirs) {
            let v = entry.trim_start_matches('v');
            tool.create_symlink(v, &nvm_versions_path.join(&entry))?;
            rtxprintln!("Synced node@{} from nvm", v);
        }

        config.rebuild_shims_and_runtime_symlinks()
    }

    fn run_nodenv(self, config: Config) -> Result<()> {
        let tool = config.get_or_create_plugin(&PluginName::from("node"));

        let nodenv_versions_path = NODENV_ROOT.join("versions");
        let installed_versions_path = dirs::INSTALLS.join("node");

        file::remove_symlinks_with_target_prefix(&installed_versions_path, &nodenv_versions_path)?;

        let subdirs = file::dir_subdirs(&nodenv_versions_path)?;
        for v in sorted(subdirs) {
            tool.create_symlink(&v, &nodenv_versions_path.join(&v))?;
            rtxprintln!("Synced node@{} from nodenv", v);
        }

        config.rebuild_shims_and_runtime_symlinks()
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>
  $ <bold>brew install node@18 node@20</bold>
  $ <bold>rtx sync node --brew</bold>
  $ <bold>rtx use -g node@18</bold> - uses Homebrew-provided node
"#
);
