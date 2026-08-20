use crate::Result;
use crate::cli::args::BackendArg;
use crate::cmd::CmdLineRunner;
use crate::config::env_directive::{EnvDirectiveContext, EnvResults};
use crate::config::{Config, Settings};
use crate::env_diff::EnvMap;
use crate::file::display_path;
use crate::lock_file::LockFile;
use crate::registry::tool_enabled;
use crate::toolset::Toolset;
use crate::{backend, plugins};
use indexmap::IndexMap;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Clone, Debug)]
pub struct Venv {
    pub venv_path: PathBuf,
    pub env: HashMap<String, String>,
}

#[derive(Default)]
pub(crate) struct PythonVenvOptions {
    /// `_.python.venv.python` — the user asked for this version by name, so a miss is an error
    /// they should see rather than something to paper over.
    pub(crate) python: Option<String>,
    /// The python the caller's toolset has active, filled in by [`EnvResults::venv`]. A
    /// *preference*, not a request: if the config-derived toolset cannot offer it we fall back to
    /// the previous behaviour instead of failing, because the user never named this version.
    pub(crate) active_python: Option<String>,
    pub(crate) uv_create_args: Option<Vec<String>>,
    pub(crate) python_create_args: Option<Vec<String>>,
    pub(crate) require_uv: bool,
}

/// Whether `_.python.venv` should do anything, given the tool allow/deny settings.
///
/// The directive exists to put a python on PATH, so turning python off has to turn it off too —
/// otherwise `mise which python` reports the tool as absent while `VIRTUAL_ENV` and the venv's
/// bin directory are still exported, which is the state #4690 reported.
///
/// Goes through the same [`tool_enabled`] every other consumer of these settings uses, so the
/// allowlist form is covered as well: `enable_tools = ["node"]` leaves python out, and the venv
/// stops with it.
fn python_venv_enabled(
    enable_tools: Option<&BTreeSet<String>>,
    disable_tools: &BTreeSet<String>,
) -> bool {
    tool_enabled(enable_tools, disable_tools, &"python".to_string())
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

pub(crate) async fn create_python_venv(
    config: &Arc<Config>,
    ts: &Toolset,
    venv: &Path,
    env_vars: EnvMap,
    options: PythonVenvOptions,
) -> Result<bool> {
    let PythonVenvOptions {
        python,
        active_python,
        uv_create_args,
        python_create_args,
        require_uv,
    } = options;
    let python = python.as_deref();
    let ba = BackendArg::from("python");
    let tv = ts.versions.get(&ba).and_then(|tv| {
        // if a python version is specified, check if that version is installed
        // otherwise use the first since that's what `python3` will refer to
        if let Some(v) = python {
            tv.versions.iter().find(|t| t.version.starts_with(v))
        } else if let Some(v) = &active_python {
            // the caller's active python, which this toolset may not list at all — it was rebuilt
            // from the config files. Falling back keeps a `--tool` version that is absent from
            // `[tools]` working exactly as it did before (#5281).
            //
            // Matched exactly, unlike the branch above: that one compares against whatever partial
            // version the user wrote in `_.python.venv.python`, while this is already resolved on
            // both sides. A prefix match here would let `3.12.0` select a configured `3.12.0a1`.
            tv.versions
                .iter()
                .find(|t| t.version == *v)
                .or_else(|| tv.versions.first())
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

    let uv_bin = if !require_uv && Settings::get().python.venv_stdlib {
        None
    } else if let Some(uv_bin) = ts.which_bin_spawnable(config, "uv").await {
        Some(uv_bin)
    } else {
        // Commands such as `mise x tiny@3` can provide a caller toolset that does not
        // include the configured uv version. Resolve a uv-only toolset as a fallback so
        // an installed configured uv remains available for automatic venv creation.
        let trs = config.get_tool_request_set().await?;
        let filtered_trs = trs.filter_by_tool(HashSet::from(["uv".to_string()]));
        let mut uv_ts: Toolset = filtered_trs.into();
        let _ = uv_ts.resolve(config).await;
        uv_ts
            .which_bin_spawnable(config, "uv")
            .await
            .or_else(|| backend::which_no_shims_spawnable("uv"))
    };

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
    // Mark venv as stale so deps knows to run
    crate::deps::mark_output_stale(venv.to_path_buf());
    Ok(true)
}

/// The version of the python the caller's toolset has active, if any.
///
/// `create_python_venv` resolves its own python/uv-only toolset to avoid a circular wait (see
/// below), and that toolset is built from the config files — so it lists every `[tools] python`
/// entry in config order and knows nothing about `--tool`. Feeding this back in as the `python`
/// option makes it select the same interpreter the rest of the run is using, and costs nothing
/// when there is no override: the caller's toolset then holds the same first entry.
fn active_python_version(toolset: Option<&Toolset>) -> Option<String> {
    let tvl = toolset?.versions.get(&BackendArg::from("python"))?;
    Some(tvl.versions.first()?.version.clone())
}

impl EnvResults {
    pub(super) async fn venv(
        ctx: &mut EnvDirectiveContext<'_>,
        env: &mut IndexMap<String, (String, Option<PathBuf>)>,
        path: String,
        create: bool,
        mut options: PythonVenvOptions,
    ) -> Result<()> {
        trace!("python venv: {} create={create}", display_path(&path));
        let settings = Settings::get();
        if !python_venv_enabled(settings.enable_tools().as_ref(), &settings.disable_tools()) {
            // Before the creation branch as well as the activation one: with python turned off the
            // venv would fail to build anyway, and "declined to run" is a different thing from
            // "tried and could not".
            debug!("python venv skipped: the python tool is disabled");
            return Ok(());
        }
        ctx.trust_check_source()?;
        let venv = ctx.parse_template(&path)?;
        let venv = ctx.normalize_path(venv.into());
        let venv_lock = LockFile::new(&venv).lock()?;
        // Record whichever python the caller actually has active. The toolset rebuilt below comes
        // from the config files, so on its own it cannot see a CLI override — `mise run --tool
        // python@3.12` would silently build the venv from the first `[tools] python` entry (#5281).
        options.active_python = active_python_version(ctx.toolset);
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
            let trs = ctx.config.get_tool_request_set().await?;
            let mut filter = HashSet::new();
            filter.insert("python".to_string());
            filter.insert("uv".to_string());
            let filtered_trs = trs.filter_by_tool(filter);

            // Convert the filtered tool request set to a toolset and resolve only these tools
            let mut ts: Toolset = filtered_trs.into();
            // Ignore resolution errors for venv creation - if tools aren't available, we'll warn below
            let _ = ts.resolve(ctx.config).await;
            create_python_venv(ctx.config, &ts, &venv, ctx.exec_env.clone(), options).await?;
        }
        drop(venv_lock);
        if venv.exists() {
            let Venv {
                venv_path,
                env: venv_env,
            } = load_venv(&venv, HashMap::new());
            ctx.results.env_paths.insert(0, venv_path);
            for (k, v) in venv_env {
                env.insert(k, (v, Some(ctx.source.to_path_buf())));
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
                            expand: false,
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
                            expand: false,
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

// Separate from `tests` above because that module is unix-only and these are not: the gate is
// plain set arithmetic, and it is worth running everywhere the gate runs.
#[cfg(test)]
mod venv_enabled_tests {
    use super::*;

    fn set(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn disable_tools_turns_the_venv_off() {
        assert!(!python_venv_enabled(None, &set(&["python"])));
        // an unrelated tool being disabled changes nothing
        assert!(python_venv_enabled(None, &set(&["node"])));
        assert!(python_venv_enabled(None, &set(&[])));
    }

    #[test]
    fn enable_tools_is_an_allowlist_and_covers_the_venv_too() {
        // the non-obvious half: an allowlist that omits python disables the venv, even though
        // nothing named python appears in `disable_tools`
        assert!(!python_venv_enabled(Some(&set(&["node"])), &set(&[])));
        assert!(python_venv_enabled(Some(&set(&["python"])), &set(&[])));
        // an empty allowlist is "no tools at all", not "no opinion"
        assert!(!python_venv_enabled(Some(&set(&[])), &set(&[])));
    }
}
