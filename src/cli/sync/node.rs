use std::path::PathBuf;

use color_eyre::eyre::Result;
use itertools::sorted;

use crate::cli::command::Command;
use crate::config::Config;
use crate::file;
use crate::output::Output;
use crate::plugins::PluginName;
use crate::{cmd, dirs};

/// Symlinks all tool versions from an external tool into rtx
///
/// For example, use this to import all Homebrew node installs into rtx
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct SyncNode {
    /// Get tool versions from Homebrew
    #[clap(long, required = true)]
    brew: bool,
}

impl Command for SyncNode {
    fn run(self, mut config: Config, out: &mut Output) -> Result<()> {
        let tool = config.get_or_create_tool(&PluginName::from("node"));

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
            rtxprintln!(out, "Synced node@{} from Homebrew", v);
        }

        config.rebuild_shims_and_runtime_symlinks()
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>
  $ <bold>brew install node@18 node@20</bold>
  $ <bold>rtx sync node --brew</bold>
  $ <bold>rtx use -g node@18</bold> uses Homebrew-provided node
"#
);

#[cfg(test)]
mod tests {
    use crate::assert_cli;

    #[test]
    fn test_pyenv() {
        assert_cli!("sync", "python", "--pyenv");
    }
}
