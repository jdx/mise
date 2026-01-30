use crate::Result;
use crate::cli::args::BackendArg;
use crate::cmd::CmdLineRunner;
use crate::config::config_file::trust_check;
use crate::config::env_directive::EnvResults;
use crate::config::{Config, Settings};
use crate::env_diff::EnvMap;
use crate::file::{display_path, which_non_pristine};
use crate::lock_file::LockFile;
use crate::toolset::Toolset;
use crate::{backend, plugins};
use indexmap::IndexMap;
use std::collections::{HashMap, HashSet};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Clone, Debug)]
pub struct Venv {
    pub venv_path: PathBuf,
    pub env: HashMap<String, String>,
}

pub(crate) fn load_venv(
    venv_root: &Path,
    extra_env: impl IntoIterator<Item = (String, String)>,
) -> Venv {
    #[cfg(windows)]
    let venv_bin_dir = "Scripts";
    #[cfg(not(windows))]
    let venv_bin_dir = "bin";

    let mut env = HashMap::new();
    env.extend(extra_env);
    env.insert(
        "VIRTUAL_ENV".to_string(),
        venv_root.to_string_lossy().to_string(),
    );
    Venv {
        venv_path: venv_root.join(venv_bin_dir),
        env,
    }
}

fn build_uv_venv_command<'a>(
    uv_bin: PathBuf,
    venv: &'a Path,
    python_path: Option<&'a str>,
    python: Option<&'a str>,
    uv_create_args: Option<Vec<String>>,
) -> CmdLineRunner<'a> {
    info!("creating venv with uv at: {}", display_path(venv));
    let extra = uv_create_args
        .or(Settings::get().python.uv_venv_create_args.clone())
        .unwrap_or_default();
    let mut cmd = CmdLineRunner::new(uv_bin).args(["venv", &venv.to_string_lossy()]);

    cmd = match (python_path, python) {
        // The selected mise managed python tool path from env._.python.venv.python or first in list
        (Some(python_path), _) => cmd.args(["--python", python_path]),
        // User specified in env._.python.venv.python but it's not in mise tools, so pass version number to uv
        (_, Some(python)) => cmd.args(["--python", python]),
        // Default to whatever uv wants to use
        _ => cmd,
    };
    cmd.args(extra)
}

fn build_stdlib_venv_command<'a>(
    venv: &'a Path,
    python_path: Option<&'a str>,
    python: Option<&'a str>,
    python_create_args: Option<Vec<String>>,
) -> CmdLineRunner<'a> {
    info!("creating venv with stdlib at: {}", display_path(venv));
    let extra = python_create_args
        .or(Settings::get().python.venv_create_args.clone())
        .unwrap_or_default();

    let bin = match (python_path, python) {
        // The selected mise managed python tool path from env._.python.venv.python or first in list
        (Some(python_path), _) => python_path.to_string(),
        // User specified in env._.python.venv.python but it's not in mise tools, so try to find it on path
        (_, Some(python)) => format!("python{python}"),
        // Default to whatever python3 points to on path
        _ => "python3".to_string(),
    };

    CmdLineRunner::new(bin)
        .args(["-m", "venv", &venv.to_string_lossy()])
        .args(extra)
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn create_python_venv(
    config: &Arc<Config>,
    ts: &Toolset,
    venv: &Path,
    env_vars: EnvMap,
    python: Option<&str>,
    uv_create_args: Option<Vec<String>>,
    python_create_args: Option<Vec<String>>,
    require_uv: bool,
) -> Result<bool> {
    let ba = BackendArg::from("python");
    let tv = ts.versions.get(&ba).and_then(|tv| {
        // if a python version is specified, check if that version is installed
        // otherwise use the first since that's what `python3` will refer to
        if let Some(v) = python {
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
        warn_once!(
            "no venv found at: {p}\n\n\
            mise will automatically create the venv once all requested python versions are installed.\n\
            To install the missing python versions and create the venv, please run:\n\
            `mise install`",
            p = display_path(venv)
        );
        return Ok(false);
    }

    let uv_bin = ts
        .which_bin(config, "uv")
        .await
        .or_else(|| which_non_pristine("uv"));

    if require_uv && uv_bin.is_none() {
        warn_once!(
            "uv is required to create the venv at {p} but is not installed",
            p = display_path(venv)
        );
        return Ok(false);
    }

    let use_uv = require_uv || (!Settings::get().python.venv_stdlib && uv_bin.is_some());
    let cmd = if use_uv {
        build_uv_venv_command(
            uv_bin.unwrap(),
            venv,
            python_path.as_deref(),
            python,
            uv_create_args,
        )
    } else {
        build_stdlib_venv_command(venv, python_path.as_deref(), python, python_create_args)
    }
    .envs(env_vars);
    cmd.execute()?;
    // Mark venv as stale so prepare knows to run
    crate::prepare::mark_output_stale(venv.to_path_buf());
    Ok(true)
}

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
            // Build a toolset with only Python and UV tools to avoid circular dependency deadlock.
            // When all tools are resolved (including go:* tools), those tools may need to access
            // the environment via dependency_toolset(), which tries to call config.env() again,
            // creating a circular wait since we're already in the middle of resolving the venv
            // directive as part of config.env().
            // By filtering to only Python/UV BEFORE resolution, we avoid resolving unrelated tools
            // that have their own dependencies and environment requirements.
            let trs = config.get_tool_request_set().await?;
            let mut filter = HashSet::new();
            filter.insert("python".to_string());
            filter.insert("uv".to_string());
            let filtered_trs = trs.filter_by_tool(filter);

            // Convert the filtered tool request set to a toolset and resolve only these tools
            let mut ts: Toolset = filtered_trs.into();
            // Ignore resolution errors for venv creation - if tools aren't available, we'll warn below
            let _ = ts.resolve(config).await;
            create_python_venv(
                config,
                &ts,
                &venv,
                env_vars,
                python.as_deref(),
                uv_create_args,
                python_create_args,
                false,
            )
            .await?;
        }
        drop(venv_lock);
        if venv.exists() {
            let Venv {
                venv_path,
                env: venv_env,
            } = load_venv(&venv, HashMap::new());
            r.env_paths.insert(0, venv_path);
            for (k, v) in venv_env {
                env.insert(k, (v, Some(source.to_path_buf())));
            }
        } else if !create {
            // The create "no venv found" warning is handled elsewhere
            warn_once!(
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
    use crate::config::env_directive::{
        EnvDirective, EnvDirectiveOptions, EnvResolveOptions, ToolsFilter,
    };
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
                            redact: Some(false),
                            required: crate::config::env_directive::RequiredValue::False,
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
                            redact: Some(false),
                            required: crate::config::env_directive::RequiredValue::False,
                        },
                    },
                    Default::default(),
                ),
            ],
            EnvResolveOptions {
                vars: false,
                tools: ToolsFilter::ToolsOnly,
                warn_on_missing_required: false,
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
