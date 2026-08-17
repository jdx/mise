use crate::cli::args::BackendArg;
use crate::config::env_directive::venv::{PythonVenvOptions, Venv, create_python_venv, load_venv};
use crate::config::{Config, Settings};
use crate::env_diff::EnvMap;
use crate::file::display_path;
use crate::toolset::Toolset;
use crate::{dirs, env, file};
use eyre::Result;
use std::path::{Path, PathBuf};
use std::sync::LazyLock as Lazy;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::OnceCell;

// use a mutex to prevent deadlocks that occurs due to reentrantly initialization
// when resolving the venv path or env vars
static UV_VENV: Lazy<OnceCell<Option<Venv>>> = Lazy::new(Default::default);
const UV_PROJECT_ENVIRONMENT: &str = "UV_PROJECT_ENVIRONMENT";

pub async fn uv_venv(config: &Arc<Config>, ts: &Toolset) -> Result<&'static Option<Venv>> {
    UV_VENV
        .get_or_try_init(async || {
            let settings = Settings::get();
            let uv_auto = settings.python.uv_venv_auto;
            if !uv_auto.should_source() {
                return Ok(None);
            }
            let Some(uv_root) = uv_root() else {
                return Ok(None);
            };
            let venv_path = uv_venv_path(config, &uv_root);
            if !venv_path.exists() {
                if uv_auto.should_create() {
                    match create_python_venv(
                        config,
                        ts,
                        &venv_path,
                        EnvMap::new(),
                        PythonVenvOptions {
                            require_uv: true,
                            ..Default::default()
                        },
                    )
                    .await
                    {
                        Ok(true) => {}                // venv created successfully, fall through to load it
                        Ok(false) => return Ok(None), // uv not available, venv not created
                        Err(err) => {
                            warn_once!(
                                "uv venv creation failed at: {p}\n\n{err}",
                                p = display_path(&venv_path)
                            );
                            return Ok(None);
                        }
                    }
                } else {
                    if !deps_uv_enabled(config, &uv_root)? {
                        warn_once!(
                            "uv venv not found at: {p}\n\n\
                            To create it, run a `uv` command like `uv sync` or `uv venv`. \
                            Alternatively, enable `[deps.uv]` and run `mise deps`.",
                            p = display_path(&venv_path)
                        );
                    }
                    return Ok(None);
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
            Ok(Some(load_venv(&venv_path, extra_env)))
        })
        .await
}

pub(crate) fn uv_root() -> Option<PathBuf> {
    file::find_up(dirs::CWD.as_ref()?, &["uv.lock"]).map(|p| p.parent().unwrap().to_path_buf())
}

pub(crate) fn uv_venv_path(config: &Config, uv_root: &Path) -> PathBuf {
    let project_environment = if let Some(env_results) = config.env_results_cached() {
        if let Some((value, _)) = env_results.env.get(UV_PROJECT_ENVIRONMENT) {
            Some(value.as_str())
        } else if env_results.env_remove.contains(UV_PROJECT_ENVIRONMENT) {
            None
        } else {
            env::PRISTINE_ENV
                .get(UV_PROJECT_ENVIRONMENT)
                .map(String::as_str)
        }
    } else {
        env::PRISTINE_ENV
            .get(UV_PROJECT_ENVIRONMENT)
            .map(String::as_str)
    };

    resolve_uv_venv_path(uv_root, project_environment)
}

fn resolve_uv_venv_path(uv_root: &Path, project_environment: Option<&str>) -> PathBuf {
    let path = project_environment
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(".venv"));
    if path.is_absolute() {
        path
    } else {
        uv_root.join(path)
    }
}

fn deps_uv_enabled(config: &Config, uv_root: &Path) -> Result<bool> {
    for cf in config.config_files.values() {
        if cf.config_root() != uv_root {
            continue;
        }
        if cf
            .deps_config()?
            .is_some_and(|deps| deps.providers.contains_key("uv"))
        {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_uv_venv_path() {
        let root = std::env::current_dir().unwrap().join("project");
        let absolute = std::env::current_dir().unwrap().join("absolute-venv");

        assert_eq!(resolve_uv_venv_path(&root, None), root.join(".venv"));
        assert_eq!(
            resolve_uv_venv_path(&root, Some("custom-venv")),
            root.join("custom-venv")
        );
        assert_eq!(
            resolve_uv_venv_path(&root, Some(absolute.to_str().unwrap())),
            absolute
        );
        assert_eq!(resolve_uv_venv_path(&root, Some("")), root.join(".venv"));
    }
}
