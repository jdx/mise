use eyre::Result;
use itertools::sorted;
use std::env::consts::{ARCH, OS};

use crate::{backend, config, dirs, env, file};
use crate::{config::Config, env::PYENV_ROOT};

/// Symlinks all tool versions from an external tool into mise
///
/// For example, use this to import all pyenv installs into mise
///
/// This won't overwrite any existing installs but will overwrite any existing symlinks
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct SyncPython {
    /// Get tool versions from pyenv
    #[clap(long)]
    pyenv: bool,

    /// Sync tool versions with uv (2-way sync)
    #[clap(long)]
    uv: bool,
}

impl SyncPython {
    pub async fn run(self) -> Result<()> {
        if self.pyenv {
            self.pyenv().await?;
        }
        if self.uv {
            self.uv().await?;
        }
        let config = Config::get().await?;
        let ts = config.get_toolset().await?;
        config::rebuild_shims_and_runtime_symlinks(&config, ts, &[]).await?;
        Ok(())
    }

    async fn pyenv(&self) -> Result<()> {
        let python = backend::get(&"python".into()).unwrap();

        let pyenv_versions_path = PYENV_ROOT.join("versions");
        let installed_python_versions_path = dirs::INSTALLS.join("python");

        file::remove_symlinks_with_target_prefix(
            &installed_python_versions_path,
            &pyenv_versions_path,
        )?;

        let subdirs = file::dir_subdirs(&pyenv_versions_path)?;
        for v in sorted(subdirs) {
            if v.starts_with(".") {
                continue;
            }
            python.create_symlink(&v, &pyenv_versions_path.join(&v))?;
            miseprintln!("Synced python@{} from pyenv", v);
        }
        Ok(())
    }

    async fn uv(&self) -> Result<()> {
        let python = backend::get(&"python".into()).unwrap();
        let uv_versions_path = &*env::UV_PYTHON_INSTALL_DIR;
        let installed_python_versions_path = dirs::INSTALLS.join("python");

        file::remove_symlinks_with_target_prefix(
            &installed_python_versions_path,
            uv_versions_path,
        )?;

        let subdirs = file::dir_subdirs(uv_versions_path)?;
        for name in sorted(subdirs) {
            if name.starts_with(".") {
                continue;
            }
            // name is like cpython-3.13.1-macos-aarch64-none
            let v = name.split('-').nth(1).unwrap();
            if python
                .create_symlink(v, &uv_versions_path.join(&name))?
                .is_some()
            {
                miseprintln!("Synced python@{v} from uv to mise");
            }
        }

        let subdirs = file::dir_subdirs(&installed_python_versions_path)?;
        for v in sorted(subdirs) {
            if v.starts_with(".") {
                continue;
            }
            let src = installed_python_versions_path.join(&v);
            if src.is_symlink() {
                continue;
            }
            // ~/.local/share/uv/python/cpython-3.10.16-macos-aarch64-none
            // ~/.local/share/uv/python/cpython-3.13.0-linux-x86_64-gnu
            let os = OS;
            let arch = if cfg!(target_arch = "x86_64") {
                "x86_64-gnu"
            } else if cfg!(target_arch = "aarch64") {
                "aarch64-none"
            } else {
                ARCH
            };
            let dst = uv_versions_path.join(format!("cpython-{v}-{os}-{arch}"));
            if !dst.exists() {
                // TODO: uv doesn't support symlinked dirs
                // https://github.com/astral-sh/uv/blob/e65a273f1b6b7c3ab129d902e93adeda4da20636/crates/uv-python/src/managed.rs#L196
                file::clone_dir(&src, &dst)?;
                miseprintln!("Synced python@{v} from mise to uv");
            }
        }
        Ok(())
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>pyenv install 3.11.0</bold>
    $ <bold>mise sync python --pyenv</bold>
    $ <bold>mise use -g python@3.11.0</bold> - uses pyenv-provided python
    
    $ <bold>uv python install 3.11.0</bold>
    $ <bold>mise install python@3.10.0</bold>
    $ <bold>mise sync python --uv</bold>
    $ <bold>mise x python@3.11.0 -- python -V</bold> - uses uv-provided python
    $ <bold>uv run -p 3.10.0 -- python -V</bold> - uses mise-provided python
"#
);
