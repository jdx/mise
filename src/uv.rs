use crate::cli::args::BackendArg;
use crate::config::env_directive::venv::{Venv, create_python_venv, load_venv};
use crate::config::{Config, Settings};
use crate::env_diff::EnvMap;
use crate::file::display_path;
use crate::toolset::Toolset;
use crate::{dirs, file};
use std::path::{Path, PathBuf};
use std::sync::LazyLock as Lazy;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::OnceCell;

// use a mutex to prevent deadlocks that occurs due to reentrantly initialization
// when resolving the venv path or env vars
static UV_VENV: Lazy<OnceCell<Option<Venv>>> = Lazy::new(Default::default);

pub async fn uv_venv(config: &Arc<Config>, ts: &Toolset) -> &'static Option<Venv> {
    UV_VENV
        .get_or_init(async || {
            let settings = Settings::get();
            let uv_auto = settings.python.uv_venv_auto;
            if !uv_auto.should_source() {
                return None;
            }
            let uv_root = uv_root()?;
            let venv_path = uv_root.join(".venv");
            if !venv_path.exists() {
                if uv_auto.should_create() {
                    if let Err(err) = create_python_venv(
                        config,
                        ts,
                        &venv_path,
                        EnvMap::new(),
                        None,
                        None,
                        None,
                        true,
                    )
                    .await
                    {
                        warn_once!(
                            "uv venv creation failed at: {p}\n\n{err}",
                            p = display_path(&venv_path)
                        );
                        return None;
                    }
                    // venv created successfully, fall through to load it
                } else {
                    if !prepare_uv_enabled(config, &uv_root) {
                        warn_once!(
                            "uv venv not found at: {p}\n\n\
                            To create it, run a `uv` command like `uv sync` or `uv venv`. \
                            Alternatively, enable `[prepare.uv]` and run `mise prepare`.",
                            p = display_path(&venv_path)
                        );
                    }
                    return None;
                }
            }

            let mut extra_env = HashMap::new();
            // Set UV_PYTHON for legacy behavior
            if uv_auto.is_legacy_true()
                && let Some(python_tv) = ts.versions.get(&BackendArg::from("python"))
                && let Some(tv) = python_tv.versions.first()
            {
                extra_env.insert("UV_PYTHON".to_string(), tv.version.to_string());
            }
            Some(load_venv(&venv_path, extra_env))
        })
        .await
}

fn uv_root() -> Option<PathBuf> {
    file::find_up(dirs::CWD.as_ref()?, &["uv.lock"]).map(|p| p.parent().unwrap().to_path_buf())
}

fn prepare_uv_enabled(config: &Config, uv_root: &Path) -> bool {
    config.config_files.values().any(|cf| {
        if cf.config_root() != uv_root {
            return false;
        }
        cf.prepare_config()
            .is_some_and(|prepare| prepare.providers.contains_key("uv"))
    })
}
