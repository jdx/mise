use crate::cli::args::BackendArg;
use crate::cmd::CmdLineRunner;
use crate::config::config_file::trust_check;
use crate::config::env_directive::EnvResults;
use crate::config::{Config, SETTINGS};
use crate::env::PATH_KEY;
use crate::env_diff::EnvMap;
use crate::file::{display_path, which_non_pristine};
use crate::toolset::ToolsetBuilder;
use crate::Result;
use crate::{backend, env};
use indexmap::IndexMap;
use std::path::{Path, PathBuf};

impl EnvResults {
    #[allow(clippy::too_many_arguments)]
    pub fn venv(
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
        if !venv.exists() && create {
            // TODO: the toolset stuff doesn't feel like it's in the right place here
            // TODO: in fact this should probably be moved to execute at the same time as src/uv.rs runs in ts.env() instead of config.env()
            let config = Config::get();
            let ts = ToolsetBuilder::new().build(&config)?;
            let path = ts
                .list_paths()
                .into_iter()
                .chain(env::split_paths(&env_vars[&*PATH_KEY]))
                .collect::<Vec<_>>();
            let ba = BackendArg::from("python");
            let installed = ts
                .versions
                .get(&ba)
                .and_then(|tv| {
                    // if a python version is specified, check if that version is installed
                    // otherwise use the first since that's what `python3` will refer to
                    if let Some(v) = &python {
                        tv.versions.iter().find(|t| t.version.starts_with(v))
                    } else {
                        tv.versions.first()
                    }
                })
                .map(|tv| {
                    let backend = backend::get(&ba).unwrap();
                    backend.is_version_installed(tv, false)
                })
                // if no version is specified, we're assuming python3 is provided outside of mise
                // so return "true" here
                .unwrap_or(true);
            if !installed {
                warn!(
                                "no venv found at: {p}\n\n\
                                mise will automatically create the venv once all requested python versions are installed.\n\
                                To install the missing python versions and create the venv, please run:\n\
                                mise install",
                                p = display_path(&venv)
                            );
            } else {
                let has_uv_bin = ts.which("uv").is_some() || which_non_pristine("uv").is_some();
                let use_uv = !SETTINGS.python.venv_stdlib && has_uv_bin;
                let cmd = if use_uv {
                    info!("creating venv with uv at: {}", display_path(&venv));
                    let extra = SETTINGS
                        .python
                        .uv_venv_create_args
                        .as_ref()
                        .and_then(|a| match shell_words::split(a) {
                            Ok(a) => Some(a),
                            Err(err) => {
                                warn!("failed to split uv_venv_create_args: {}", err);
                                None
                            }
                        })
                        .or(uv_create_args)
                        .unwrap_or_default();
                    let mut cmd = CmdLineRunner::new("uv").args(["venv", &venv.to_string_lossy()]);
                    if let Some(python) = python {
                        cmd = cmd.args(["--python", &python]);
                    }
                    cmd.args(extra)
                } else {
                    info!("creating venv with stdlib at: {}", display_path(&venv));
                    let extra = SETTINGS
                        .python
                        .venv_create_args
                        .as_ref()
                        .and_then(|a| match shell_words::split(a) {
                            Ok(a) => Some(a),
                            Err(err) => {
                                warn!("failed to split venv_create_args: {}", err);
                                None
                            }
                        })
                        .or(python_create_args)
                        .unwrap_or_default();
                    let bin = format!("python{}", python.unwrap_or("3".into()));
                    CmdLineRunner::new(bin)
                        .args(["-m", "venv", &venv.to_string_lossy()])
                        .args(extra)
                }
                .envs(env_vars)
                .env(
                    PATH_KEY.to_string(),
                    env::join_paths(&path)?.to_string_lossy().to_string(),
                );
                cmd.execute()?;
            }
        }
        if venv.exists() {
            r.env_paths.insert(0, venv.join("bin"));
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
                "no venv found at: {p}\n\n\
                            To create a virtualenv manually, run:\n\
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
    use crate::config::env_directive::EnvDirective;
    use crate::tera::BASE_CONTEXT;
    use crate::test::replace_path;
    use insta::assert_debug_snapshot;
    use test_log::test;

    #[test]
    fn test_venv_path() {
        let env = EnvMap::new();
        let results = EnvResults::resolve(
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
                        options: Default::default(),
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
                        options: Default::default(),
                    },
                    Default::default(),
                ),
            ],
            false,
        )
        .unwrap();
        // expect order to be reversed as it processes directives from global to dir specific
        assert_debug_snapshot!(
            results.env_paths.into_iter().map(|p| replace_path(&p.display().to_string())).collect::<Vec<_>>(),
            @r#"
        [
            "~/bin",
            "/bin",
        ]
        "#
        );
    }
}
