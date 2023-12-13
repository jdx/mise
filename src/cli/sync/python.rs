use color_eyre::eyre::Result;
use itertools::sorted;

use crate::config::Config;
use crate::dirs;
use crate::env::PYENV_ROOT;
use crate::file;

use crate::plugins::PluginName;

/// Symlinks all tool versions from an external tool into rtx
///
/// For example, use this to import all pyenv installs into rtx
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct SyncPython {
    /// Get tool versions from pyenv
    #[clap(long, required = true)]
    pyenv: bool,
}

impl SyncPython {
    pub fn run(self, config: Config) -> Result<()> {
        let python = config.get_or_create_plugin(&PluginName::from("python"));

        let pyenv_versions_path = PYENV_ROOT.join("versions");
        let installed_python_versions_path = dirs::INSTALLS.join("python");

        file::remove_symlinks_with_target_prefix(
            &installed_python_versions_path,
            &pyenv_versions_path,
        )?;

        let subdirs = file::dir_subdirs(&pyenv_versions_path)?;
        for v in sorted(subdirs) {
            python.create_symlink(&v, &pyenv_versions_path.join(&v))?;
            rtxprintln!("Synced python@{} from pyenv", v);
        }

        config.rebuild_shims_and_runtime_symlinks()
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>
  $ <bold>pyenv install 3.11.0</bold>
  $ <bold>rtx sync python --pyenv</bold>
  $ <bold>rtx use -g python@3.11.0</bold> - uses pyenv-provided python
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
