use crate::cli::args::BackendArg;
use crate::cmd::CmdLineRunner;
use crate::config::{Config, SETTINGS};
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::{dirs, file};
use eyre::Result;
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug)]
pub struct Venv {
    pub venv_path: PathBuf,
    pub env: HashMap<String, String>,
}

pub static UV_VENV: Lazy<Option<Venv>> = Lazy::new(|| {
    if !SETTINGS.python.uv_venv_auto {
        return None;
    }
    if let (Some(venv_path), Some(uv_path)) = (venv_path(), uv_path()) {
        match get_or_create_venv(venv_path, uv_path) {
            Ok(venv) => return Some(venv),
            Err(e) => {
                warn!("uv venv failed: {e}");
            }
        }
    }
    None
});

fn get_or_create_venv(venv_path: PathBuf, uv_path: PathBuf) -> Result<Venv> {
    SETTINGS.ensure_experimental("uv venv auto")?;
    let mut venv = Venv {
        env: Default::default(),
        venv_path: venv_path.join("bin"),
    };
    if let Some(python_tv) = Config::get()
        .get_toolset()?
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
            .with_pr(pr.as_ref())
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
fn uv_path() -> Option<PathBuf> {
    Config::get()
        .get_toolset()
        .ok()?
        .which_bin("uv")
        .or_else(|| which::which("uv").ok())
}
