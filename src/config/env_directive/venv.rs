use crate::Result;
use crate::cli::args::BackendArg;
use crate::cmd::CmdLineRunner;
use crate::config::config_file::trust_check;
use crate::config::env_directive::EnvResults;
use crate::config::{Config, Settings};
use crate::env_diff::EnvMap;
use crate::file::{display_path, which_non_pristine};
use crate::lock_file::LockFile;
use crate::toolset::ToolsetBuilder;
use crate::{backend, plugins};
use indexmap::IndexMap;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

impl EnvResults {
    #[allow(clippy::too_many_arguments)]
    pub async fn venv(
        config: &Arc<Config>,
        ctx: &mut tera::Context,
        tera: &mut tera::Tera,
        env: &mut IndexMap<String, (String, Option<PathBuf>)>,
        r: &mut EnvResults,
        normalize_path: fn(&Path, PathBuf) -> PathBuf,
        source: &Path,
        config_root: &Path,
        env_vars: EnvMap,
        path: String,
        create: bool,
        python: Option<String>,
        uv_create_args: Option<Vec<String>>,
        python_create_args: Option<Vec<String>>,
    ) -> Result<()> {
        trace!("python venv: {} create={create}", display_path(&path));
        trust_check(source)?;
        let venv = r.parse_template(ctx, tera, source, &path)?;
        let venv = normalize_path(config_root, venv.into());
        let venv_lock = LockFile::new(&venv).lock()?;
        if !venv.exists() && create {
            // TODO: the toolset stuff doesn't feel like it's in the right place here
            // TODO: in fact this should probably be moved to execute at the same time as src/uv.rs runs in ts.env() instead of config.env()
            let ts = Box::pin(ToolsetBuilder::new().build(config)).await?;
            let ba = BackendArg::from("python");
            let tv = ts.versions.get(&ba).and_then(|tv| {
                // if a python version is specified, check if that version is installed
                // otherwise use the first since that's what `python3` will refer to
                if let Some(v) = &python {
                    tv.versions.iter().find(|t| t.version.starts_with(v))
                } else {
                    tv.versions.first()
                }
            });
            let python_path = tv.map(|tv| {
                plugins::core::python::python_path(tv)
                    .to_string_lossy()
                    .to_string()
            });
            let installed = if let Some(tv) = tv {
                let backend = backend::get(&ba).unwrap();
                backend.is_version_installed(config, tv, false)
            } else {
                // if no version is specified, we're assuming python3 is provided outside of mise so return "true" here
                true
            };
            if !installed {
                warn!(
                    "no venv found at: {p}\n\n\
                    mise will automatically create the venv once all requested python versions are installed.\n\
                    To install the missing python versions and create the venv, please run:\n\
                    mise install",
                    p = display_path(&venv)
                );
            } else {
                let uv_bin = ts
                    .which_bin(config, "uv")
                    .await
                    .or_else(|| which_non_pristine("uv"));
                let use_uv = !Settings::get().python.venv_stdlib && uv_bin.is_some();
                let cmd = if use_uv {
                    info!("creating venv with uv at: {}", display_path(&venv));
                    let extra = Settings::get()
                        .python
                        .uv_venv_create_args
                        .clone()
                        .or(uv_create_args)
                        .unwrap_or_default();
                    let mut cmd =
                        CmdLineRunner::new(uv_bin.unwrap()).args(["venv", &venv.to_string_lossy()]);

                    cmd = match (python_path, python) {
                        // The selected mise managed python tool path from env._.python.venv.python or first in list
                        (Some(python_path), _) => cmd.args(["--python", &python_path]),
                        // User specified in env._.python.venv.python but it's not in mise tools, so pass version number to uv
                        (_, Some(python)) => cmd.args(["--python", &python]),
                        // Default to whatever uv wants to use
                        _ => cmd,
                    };
                    cmd.args(extra)
                } else {
                    info!("creating venv with stdlib at: {}", display_path(&venv));
                    let extra = Settings::get()
                        .python
                        .venv_create_args
                        .clone()
                        .or(python_create_args)
                        .unwrap_or_default();

                    let bin = match (python_path, python) {
                        // The selected mise managed python tool path from env._.python.venv.python or first in list
                        (Some(python_path), _) => python_path,
                        // User specified in env._.python.venv.python but it's not in mise tools, so try to find it on path
                        (_, Some(python)) => format!("python{python}"),
                        // Default to whatever python3 points to on path
                        _ => "python3".to_string(),
                    };

                    CmdLineRunner::new(bin)
                        .args(["-m", "venv", &venv.to_string_lossy()])
                        .args(extra)
                }
                .envs(env_vars);
                cmd.execute()?;
            }
        }
        drop(venv_lock);
        if venv.exists() {
            r.env_paths
                .insert(0, venv.join(if cfg!(windows) { "Scripts" } else { "bin" }));
            env.insert(
                "VIRTUAL_ENV".into(),
                (
                    venv.to_string_lossy().to_string(),
                    Some(source.to_path_buf()),
                ),
            );
        } else if !create {
            // The create "no venv found" warning is handled elsewhere
            warn!(
                "no venv found at: {p}
To create a virtualenv manually, run:
python -m venv {p}",
                p = display_path(&venv)
            );
        }
        Ok(())
    }
}

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use super::*;
    use crate::config::env_directive::{EnvDirective, EnvDirectiveOptions, EnvResolveOptions};
    use crate::tera::BASE_CONTEXT;
    use crate::test::replace_path;
    use insta::assert_debug_snapshot;

    #[tokio::test]
    async fn test_venv_path() {
        let env = EnvMap::new();
        let config = Config::get().await.unwrap();
        let results = EnvResults::resolve(
            &config,
            BASE_CONTEXT.clone(),
            &env,
            vec![
                (
                    EnvDirective::PythonVenv {
                        path: "/".into(),
                        create: false,
                        python: None,
                        uv_create_args: None,
                        python_create_args: None,
                        options: EnvDirectiveOptions {
                            tools: true,
                            redact: false,
                        },
                    },
                    Default::default(),
                ),
                (
                    EnvDirective::PythonVenv {
                        path: "./".into(),
                        create: false,
                        python: None,
                        uv_create_args: None,
                        python_create_args: None,
                        options: EnvDirectiveOptions {
                            tools: true,
                            redact: false,
                        },
                    },
                    Default::default(),
                ),
            ],
            EnvResolveOptions {
                tools: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        // expect order to be reversed as it processes directives from global to dir specific
        assert_debug_snapshot!(
            results.env_paths.into_iter().map(|p| replace_path(&p.display().to_string())).collect::<Vec<_>>(),
            @r#"
        [
            "~/bin",
        ]
        "#
        );
    }
}
