use std::path::Path;

use eyre::Result;

use crate::config::Settings;
use crate::file::remove_all;
use crate::ui::prompt;
use crate::{dirs, env, file};
use std::collections::BTreeSet;

/// Remove the mise CLI and all related data
///
/// Deletes the mise executable and the data (including installed tools), state,
/// and cache directories. It also deletes the system-wide shared directory used by
/// `mise install --system` (MISE_SYSTEM_DATA_DIR, default /usr/local/share/mise),
/// which other users on the machine may rely on. It asks before each removal;
/// --yes skips the prompts. The config directory stays unless you pass --config.
///
/// If a package manager such as Homebrew or apt installed mise, uninstall it with
/// that package manager instead. Remove the `mise activate` line from your shell
/// startup file yourself. See
/// https://mise.jdx.dev/installing-mise.html#uninstalling.
#[derive(Debug, usage_rs::Args)]
#[usage(
    verbatim_doc_comment,
    example("mise implode --dry-run", help = "List what would be removed")
)]
pub(crate) struct Implode {
    /// Print what would be removed, without removing anything
    #[usage(long, short = 'n', verbatim_doc_comment)]
    dry_run: bool,

    /// Also remove the config directory
    #[usage(long, verbatim_doc_comment)]
    config: bool,
}

impl Implode {
    pub(super) fn is_dry_run(&self) -> bool {
        self.dry_run
    }

    pub(crate) fn run(self) -> Result<()> {
        let mut files: BTreeSet<&Path> = [*dirs::STATE, *dirs::DATA, *dirs::CACHE, &*env::MISE_BIN]
            .into_iter()
            .collect();
        if self.config {
            files.insert(&dirs::CONFIG);
        }
        // include system data dir (e.g. /usr/local/share/mise) used by `mise install --system`
        let system_data: &Path = &env::MISE_SYSTEM_DATA_DIR;
        files.insert(system_data);
        for f in files.into_iter().filter(|d| d.exists()) {
            if self.dry_run {
                miseprintln!("rm -rf {}", f.display());
            }

            if self.confirm_remove(f)? {
                if f.is_dir() {
                    remove_all(f)?;
                } else {
                    file::remove_file(f)?;
                }
            }
        }

        Ok(())
    }

    fn confirm_remove(&self, f: &Path) -> Result<bool> {
        let settings = Settings::try_get()?;
        if self.dry_run {
            Ok(false)
        } else if settings.yes {
            Ok(true)
        } else {
            prompt::confirm_destructive(format!("remove {} ?", f.display()), "mise implode")
        }
    }
}
