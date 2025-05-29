use std::path::PathBuf;

use eyre::Result;
use itertools::sorted;

use crate::{
    backend, cmd,
    config::{self, Config},
    dirs, file,
};

/// Symlinks all ruby tool versions from an external tool into mise
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct SyncRuby {
    #[clap(flatten)]
    _type: SyncRubyType,
}

#[derive(Debug, clap::Args)]
#[group(required = true, multiple = true)]
pub struct SyncRubyType {
    /// Get tool versions from Homebrew
    #[clap(long)]
    brew: bool,
}

impl SyncRuby {
    pub async fn run(self) -> Result<()> {
        if self._type.brew {
            self.run_brew().await?;
        }
        let config = Config::reset().await?;
        let ts = config.get_toolset().await?;
        config::rebuild_shims_and_runtime_symlinks(&config, ts, &[]).await?;
        Ok(())
    }

    async fn run_brew(&self) -> Result<()> {
        let ruby = backend::get(&"ruby".into()).unwrap();

        let brew_prefix = PathBuf::from(cmd!("brew", "--prefix").read()?).join("opt");
        let installed_versions_path = dirs::INSTALLS.join("ruby");

        file::remove_symlinks_with_target_prefix(&installed_versions_path, &brew_prefix)?;

        let subdirs = file::dir_subdirs(&brew_prefix)?;
        for entry in sorted(subdirs) {
            if entry.starts_with(".") {
                continue;
            }
            if !entry.starts_with("ruby@") {
                continue;
            }
            let v = entry.trim_start_matches("ruby@");
            if ruby.create_symlink(v, &brew_prefix.join(&entry))?.is_some() {
                miseprintln!("Synced ruby@{} from Homebrew", v);
            }
        }
        Ok(())
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>brew install ruby</bold>
    $ <bold>mise sync ruby --brew</bold>
    $ <bold>mise use -g ruby</bold> - Use the latest version of Ruby installed by Homebrew
"#
);
