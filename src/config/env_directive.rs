use crate::config::config_file::trust_check;
use crate::dirs;
use crate::file::display_path;
use crate::tera::{get_tera, BASE_CONTEXT};
use eyre::Context;
use indexmap::IndexMap;
use std::collections::{BTreeSet, HashMap};
use std::fmt::Display;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub enum EnvDirective {
    /// simple key/value pair
    Val(String, String),
    /// remove a key
    Rm(String),
    /// dotenv file
    File(PathBuf),
    /// add a path to the PATH
    Path(PathBuf),
    /// run a bash script and apply the resulting env diff
    Source(PathBuf),
}

impl From<(String, String)> for EnvDirective {
    fn from((k, v): (String, String)) -> Self {
        Self::Val(k, v)
    }
}

impl From<(String, i64)> for EnvDirective {
    fn from((k, v): (String, i64)) -> Self {
        (k, v.to_string()).into()
    }
}

impl Display for EnvDirective {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EnvDirective::Val(k, v) => write!(f, "{k}={v}"),
            EnvDirective::Rm(k) => write!(f, "unset {k}"),
            EnvDirective::File(path) => write!(f, "dotenv {}", display_path(path)),
            EnvDirective::Path(path) => write!(f, "path_add {}", display_path(path)),
            EnvDirective::Source(path) => write!(f, "source {}", display_path(path)),
        }
    }
}

#[derive(Debug)]
pub struct EnvResults {
    pub env: IndexMap<String, (String, PathBuf)>,
    pub env_remove: BTreeSet<String>,
    pub env_files: Vec<PathBuf>,
    pub env_paths: Vec<PathBuf>,
    pub env_scripts: Vec<PathBuf>,
}

impl EnvResults {
    pub fn resolve(
        initial: &HashMap<String, String>,
        input: Vec<(EnvDirective, PathBuf)>,
    ) -> eyre::Result<Self> {
        let mut ctx = BASE_CONTEXT.clone();
        let mut env = initial
            .iter()
            .map(|(k, v)| (k.clone(), (v.clone(), None)))
            .collect::<IndexMap<_, _>>();
        let mut r = Self {
            env: Default::default(),
            env_remove: BTreeSet::new(),
            env_files: Vec::new(),
            env_paths: Vec::new(),
            env_scripts: Vec::new(),
        };
        for (directive, source) in input {
            let config_root = source.parent().unwrap();
            ctx.insert("config_root", config_root);
            ctx.insert("env", &env);
            let normalize_path = |s: String| {
                let s = s.strip_prefix("./").unwrap_or(&s);
                match s.strip_prefix("~/") {
                    Some(s) => dirs::HOME.join(s),
                    None if s.starts_with('/') => PathBuf::from(s),
                    None => config_root.join(s),
                }
            };
            match directive {
                EnvDirective::Val(k, v) => {
                    let v = r.parse_template(&ctx, &source, &v)?;
                    r.env_remove.remove(&k);
                    env.insert(k, (v, Some(source.clone())));
                }
                EnvDirective::Rm(k) => {
                    env.remove(&k);
                    r.env_remove.insert(k);
                }
                EnvDirective::Path(input) => {
                    let s = r.parse_template(&ctx, &source, input.to_string_lossy().as_ref())?;
                    let p = normalize_path(s);
                    r.env_paths.push(p.clone());
                }
                EnvDirective::File(input) => {
                    trust_check(&source)?;
                    let s = r.parse_template(&ctx, &source, input.to_string_lossy().as_ref())?;
                    let p = normalize_path(s);
                    r.env_files.push(p.clone());
                    let errfn = || eyre!("failed to parse dotenv file: {}", display_path(&p));
                    for item in dotenvy::from_path_iter(&p).wrap_err_with(errfn)? {
                        let (k, v) = item.wrap_err_with(errfn)?;
                        r.env_remove.remove(&k);
                        env.insert(k, (v, Some(p.clone())));
                    }
                }
                EnvDirective::Source(input) => {
                    trust_check(&source)?;
                    let s = r.parse_template(&ctx, &source, input.to_string_lossy().as_ref())?;
                    let p = normalize_path(s);
                    r.env_scripts.push(p.clone());
                    // TODO: run script and apply diff
                }
            };
        }
        for (k, (v, source)) in env {
            if let Some(source) = source {
                r.env.insert(k, (v, source));
            }
        }
        Ok(r)
    }

    fn parse_template(
        &self,
        ctx: &tera::Context,
        path: &Path,
        input: &str,
    ) -> eyre::Result<String> {
        if !input.contains("{{") && !input.contains("{%") && !input.contains("{#") {
            return Ok(input.to_string());
        }
        trust_check(path)?;
        let dir = path.parent();
        let output = get_tera(dir)
            .render_str(input, ctx)
            .wrap_err_with(|| eyre!("failed to parse template: '{input}'"))?;
        Ok(output)
    }
}
