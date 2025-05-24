use crate::cli::args::BackendArg;
use crate::cmd::CmdLineRunner;
use crate::config::{Config, SETTINGS};
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::{dirs, file};
use eyre::Result;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::LazyLock as Lazy;
use tokio::sync::OnceCell;

#[derive(Clone, Debug)]
pub struct Venv {
    pub venv_path: PathBuf,
    pub env: HashMap<String, String>,
}

// use a mutex to prevent deadlocks that occurs due to reentrantly initialization
// when resolving the venv path or env vars
static UV_VENV: Lazy<OnceCell<Option<Venv>>> = Lazy::new(Default::default);

pub async fn uv_venv() -> Option<Venv> {
    if let Some(venv) = UV_VENV.get() {
        return venv.clone();
    }
    if !SETTINGS.python.uv_venv_auto {
        UV_VENV.set(None).unwrap();
        return None;
    }
    if let (Some(venv_path), Some(uv_path)) = (venv_path(), uv_path().await) {
        match get_or_create_venv(venv_path, uv_path).await {
            Ok(venv) => {
                UV_VENV.set(Some(venv.clone())).unwrap();
                return Some(venv);
            }
            Err(e) => {
                warn!("uv venv failed: {e}");
            }
        }
    }
    UV_VENV.set(None).unwrap();
    None
}

async fn get_or_create_venv(venv_path: PathBuf, uv_path: PathBuf) -> Result<Venv> {
    SETTINGS.ensure_experimental("uv venv auto")?;
    let mut venv = Venv {
        env: Default::default(),
        venv_path: venv_path.join("bin"),
    };
    if let Some(python_tv) = Config::get()
        .await
        .get_toolset()
        .await?
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
async fn uv_path() -> Option<PathBuf> {
    let config = Config::try_get().await.ok()?;
    let ts = config.get_toolset().await.ok()?;
    if let Some(uv_path) = ts.which_bin("uv").await {
        return Some(uv_path);
    }
    which::which("uv").ok()
}
