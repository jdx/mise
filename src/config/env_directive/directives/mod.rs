use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::Result;
use crate::config::Config;
use crate::env_diff::EnvMap;
use indexmap::IndexMap;

use super::EnvResults;
use super::normalize::normalize_env_path;

pub struct ExecCtx<'a> {
    pub config: &'a Arc<Config>,
    pub r: &'a mut EnvResults,
    pub ctx: &'a mut tera::Context,
    pub tera: &'a mut tera::Tera,
    pub source: &'a Path,
    pub config_root: &'a Path,
    pub env: &'a mut IndexMap<String, (String, Option<PathBuf>)>,
    pub paths: &'a mut Vec<(PathBuf, PathBuf)>,
    pub redact: bool,
    pub vars_mode: bool,
}

pub async fn handle_val(exec: &mut ExecCtx<'_>, k: String, v: String) -> eyre::Result<()> {
    let v = exec
        .r
        .parse_template(exec.ctx, exec.tera, exec.source, &v)?;
    exec.r.apply_kv(
        exec.env,
        k,
        v,
        exec.source.to_path_buf(),
        exec.redact,
        exec.vars_mode,
    );
    Ok(())
}

pub fn handle_rm(exec: &mut ExecCtx<'_>, k: String) {
    exec.env.shift_remove(&k);
    exec.r.env_remove.insert(k);
}

pub async fn handle_path(exec: &mut ExecCtx<'_>, input: String) -> Result<()> {
    let path = EnvResults::path(exec.ctx, exec.tera, exec.r, exec.source, input).await?;
    exec.paths.push((path.clone(), exec.source.to_path_buf()));
    Ok(())
}

pub async fn handle_file(exec: &mut ExecCtx<'_>, input: String) -> Result<()> {
    let files = EnvResults::file(
        exec.config,
        exec.ctx,
        exec.tera,
        exec.r,
        normalize_env_path,
        exec.source,
        exec.config_root,
        input,
    )
    .await?;
    for (f, new_env) in files {
        exec.r.env_files.push(f.clone());
        for (k, v) in new_env {
            exec.r
                .apply_kv(exec.env, k, v, f.clone(), exec.redact, exec.vars_mode);
        }
    }
    Ok(())
}

pub fn handle_source(exec: &mut ExecCtx<'_>, env_vars: &EnvMap, input: String) -> Result<()> {
    let files = EnvResults::source(
        exec.ctx,
        exec.tera,
        exec.paths,
        exec.r,
        normalize_env_path,
        exec.source,
        exec.config_root,
        env_vars,
        input,
    )?;
    for (f, new_env) in files {
        exec.r.env_scripts.push(f.clone());
        for (k, v) in new_env {
            exec.r
                .apply_kv(exec.env, k, v, f.clone(), exec.redact, exec.vars_mode);
        }
    }
    Ok(())
}

pub async fn handle_venv(
    exec: &mut ExecCtx<'_>,
    env_vars: EnvMap,
    path: String,
    create: bool,
    python: Option<String>,
    uv_create_args: Option<Vec<String>>,
    python_create_args: Option<Vec<String>>,
) -> Result<()> {
    EnvResults::venv(
        exec.config,
        exec.ctx,
        exec.tera,
        exec.env,
        exec.r,
        normalize_env_path,
        exec.source,
        exec.config_root,
        env_vars,
        path,
        create,
        python,
        uv_create_args,
        python_create_args,
    )
    .await
}

pub async fn handle_module(exec: &mut ExecCtx<'_>, name: String, value: toml::Value) -> Result<()> {
    EnvResults::module(exec.r, exec.source.to_path_buf(), name, &value, exec.redact).await
}
