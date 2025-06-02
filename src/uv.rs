use crate::cli::args::BackendArg;
use crate::cmd::CmdLineRunner;
use crate::config::{Config, Settings};
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::{Result, toolset::Toolset};
use crate::{dirs, file};
use std::path::PathBuf;
use std::sync::LazyLock as Lazy;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::OnceCell;

#[derive(Clone, Debug)]
pub struct Venv {
    pub venv_path: PathBuf,
    pub env: HashMap<String, String>,
}

// use a mutex to prevent deadlocks that occurs due to reentrantly initialization
// when resolving the venv path or env vars
static UV_VENV: Lazy<OnceCell<Option<Venv>>> = Lazy::new(Default::default);

pub async fn uv_venv(config: &Arc<Config>, ts: &Toolset) -> &'static Option<Venv> {
    UV_VENV
        .get_or_init(async || {
            let settings = Settings::get();
            if !settings.python.uv_venv_auto {
                return None;
            }
            let (Some(venv_path), Some(uv_path)) = (venv_path(), uv_path(config, ts).await) else {
                return None;
            };
            match get_or_create_venv(ts, venv_path, uv_path).await {
                Ok(venv) => Some(venv),
                Err(e) => {
                    warn!("uv venv failed: {e}");
                    None
                }
            }
        })
        .await
}

async fn get_or_create_venv(ts: &Toolset, venv_path: PathBuf, uv_path: PathBuf) -> Result<Venv> {
    Settings::get().ensure_experimental("uv venv auto")?;
    let mut venv = Venv {
        env: Default::default(),
        venv_path: venv_path.join("bin"),
    };
    if let Some(python_tv) = ts
        .versions
        .get(&BackendArg::from("python"))
        .and_then(|tvl| tvl.versions.first())
    {
        venv.env
            .insert("UV_PYTHON".to_string(), python_tv.version.to_string());
    }
    if !venv_path.exists() {
        let mpr = MultiProgressReport::get();
        let pr = mpr.add("Creating uv venv");
        let mut cmd = CmdLineRunner::new(uv_path)
            .current_dir(uv_root().unwrap())
            .with_pr(&pr)
            .envs(&venv.env)
            .arg("venv");
        if !log::log_enabled!(log::Level::Debug) {
            cmd = cmd.arg("--quiet");
        }
        cmd.execute()?;
    }
    venv.env.insert(
        "VIRTUAL_ENV".to_string(),
        venv_path.to_string_lossy().to_string(),
    );
    Ok(venv)
}

fn uv_root() -> Option<PathBuf> {
    file::find_up(dirs::CWD.as_ref()?, &["uv.lock"]).map(|p| p.parent().unwrap().to_path_buf())
}
fn venv_path() -> Option<PathBuf> {
    Some(uv_root()?.join(".venv"))
}
async fn uv_path(config: &Arc<Config>, ts: &Toolset) -> Option<PathBuf> {
    if let Some(uv_path) = ts.which_bin(config, "uv").await {
        return Some(uv_path);
    }
    which::which("uv").ok()
}
