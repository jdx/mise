use eyre::Result;
use itertools::sorted;
use std::env::consts::{ARCH, OS};

use crate::{backend, config, dirs, env, file};
use crate::{config::Config, env::PYENV_ROOT};

use super::reconcile;

/// Symlink python versions installed by pyenv or uv into mise
///
/// Use this to make versions installed by another version manager available to
/// mise. It does not overwrite managed installs, runtime aliases, or links from
/// other providers. With --uv, it also links mise's python versions into uv.
/// Pass --pyenv, --uv, or both; with neither, nothing is linked.
#[derive(Debug, usage_rs::Args)]
#[usage(
    verbatim_doc_comment,
    example(
        r###"pyenv install 3.13.0
mise sync python --pyenv
mise use -g python@3.13.0"###,
        help = "Use a python that pyenv installed"
    ),
    example(
        r###"uv python install 3.13.0
mise install python@3.12.0
mise sync python --uv
mise exec python@3.13.0 -- python -V
uv run -p 3.12.0 -- python -V"###,
        help = "Share python versions with uv in both directions"
    )
)]
pub(super) struct SyncPython {
    /// Get tool versions from pyenv
    #[usage(long)]
    pyenv: bool,

    /// Link uv's python versions into mise and mise's python versions into uv
    #[usage(long)]
    uv: bool,
}

impl SyncPython {
    pub(super) async fn run(self) -> Result<()> {
        let python = backend::get(&"python".into()).unwrap();
        let mut providers = vec![];
        if self.pyenv {
            providers.push(self.pyenv_links()?);
        }
        if self.uv {
            providers.push(self.uv_links()?);
        }
        let mut changed = reconcile::reconcile_all(python.ba(), providers)?.into_iter();
        if self.pyenv {
            for v in changed.next().unwrap_or_default() {
                miseprintln!("Synced python@{} from pyenv", v);
            }
        }
        if self.uv {
            for v in changed.next().unwrap_or_default() {
                miseprintln!("Synced python@{v} from uv to mise");
            }
            self.sync_mise_installs_to_uv()?;
        }
        let config = Config::get().await?;
        let ts = config.get_toolset().await?;
        config::rebuild_shims_and_runtime_symlinks(
            &config,
            ts,
            &[],
            crate::lockfile::LockfileUpdateMode::Normal,
        )
        .await?;
        Ok(())
    }

    fn pyenv_links(&self) -> Result<reconcile::ProviderLinks> {
        let pyenv_versions_path = PYENV_ROOT.join("versions");

        let subdirs = file::dir_subdirs(&pyenv_versions_path)?;
        let mut links = vec![];
        for v in sorted(subdirs) {
            if v.starts_with(".") {
                continue;
            }
            links.push((v.clone(), pyenv_versions_path.join(&v)));
        }
        let ownership = reconcile::LinkOwnership::in_namespace(&pyenv_versions_path);
        Ok(reconcile::ProviderLinks::new(ownership, links))
    }

    fn uv_links(&self) -> Result<reconcile::ProviderLinks> {
        let uv_versions_path = &*env::UV_PYTHON_INSTALL_DIR;

        let subdirs = file::dir_subdirs(uv_versions_path)?;
        let mut links = vec![];
        for name in sorted(subdirs) {
            if name.starts_with(".") {
                continue;
            }
            // name is like cpython-3.13.1-macos-aarch64-none
            let Some(v) = name.split('-').nth(1).filter(|v| !v.is_empty()) else {
                debug!("skipping unrecognized uv python dir: {name}");
                continue;
            };
            links.push((v.to_string(), uv_versions_path.join(&name)));
        }
        let ownership = reconcile::LinkOwnership::in_namespace(uv_versions_path);
        Ok(reconcile::ProviderLinks::new(ownership, links))
    }

    fn sync_mise_installs_to_uv(&self) -> Result<()> {
        let uv_versions_path = &*env::UV_PYTHON_INSTALL_DIR;
        let installed_python_versions_path = dirs::INSTALLS.join("python");
        let subdirs = file::dir_subdirs(&installed_python_versions_path)?;
        for v in sorted(subdirs) {
            if v.starts_with(".") {
                continue;
            }
            let src = installed_python_versions_path.join(&v);
            // A version link into the identity layout stands for the installation
            // it names; any other link is someone else's.
            let src = match crate::install_layout::resolver::link_target(&src) {
                Some(install) => install,
                None if file::is_symlink_or_junction(&src) => continue,
                None => src,
            };
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
                if !uv_versions_path.exists() {
                    file::create_dir_all(uv_versions_path)?;
                }
                // TODO: uv doesn't support symlinked dirs
                // https://github.com/astral-sh/uv/blob/e65a273f1b6b7c3ab129d902e93adeda4da20636/crates/uv-python/src/managed.rs#L196
                file::clone_dir(&src, &dst)?;
                miseprintln!("Synced python@{v} from mise to uv");
            }
        }
        Ok(())
    }
}
