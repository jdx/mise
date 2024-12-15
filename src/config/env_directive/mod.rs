use std::collections::{BTreeSet, HashMap};
use std::env;
use std::fmt::{Debug, Display, Formatter};
use std::path::{Path, PathBuf};
use std::str::FromStr;

use crate::config::config_file::{config_root, trust_check};
use crate::dirs;
use crate::env_diff::EnvMap;
use crate::file::display_path;
use crate::tera::{get_tera, BASE_CONTEXT};
use eyre::{eyre, Context};
use indexmap::IndexMap;
use serde::{Deserialize, Deserializer};

mod file;
mod module;
mod path;
mod source;
mod venv;

#[derive(Debug, Clone)]
pub enum PathEntry {
    Normal(PathBuf),
    Lazy(PathBuf),
}

impl From<&str> for PathEntry {
    fn from(s: &str) -> Self {
        let pb = PathBuf::from(s);
        Self::Normal(pb)
    }
}

impl FromStr for PathEntry {
    type Err = eyre::Error;

    fn from_str(s: &str) -> eyre::Result<Self> {
        let pb = PathBuf::from_str(s)?;
        Ok(Self::Normal(pb))
    }
}

impl AsRef<Path> for PathEntry {
    #[inline]
    fn as_ref(&self) -> &Path {
        match self {
            PathEntry::Normal(pb) => pb.as_ref(),
            PathEntry::Lazy(pb) => pb.as_ref(),
        }
    }
}

impl<'de> Deserialize<'de> for PathEntry {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        #[derive(Debug, Deserialize)]
        struct MapPathEntry {
            value: PathBuf,
        }

        #[derive(Debug, Deserialize)]
        #[serde(untagged)]
        enum Helper {
            Normal(PathBuf),
            Lazy(MapPathEntry),
        }

        Ok(match Helper::deserialize(deserializer)? {
            Helper::Normal(value) => Self::Normal(value),
            Helper::Lazy(this) => Self::Lazy(this.value),
        })
    }
}

#[derive(Debug, Clone)]
pub enum EnvDirective {
    /// simple key/value pair
    Val(String, String),
    /// remove a key
    Rm(String),
    /// dotenv file
    File(PathBuf),
    /// add a path to the PATH
    Path(PathEntry),
    /// run a bash script and apply the resulting env diff
    Source(PathBuf),
    PythonVenv {
        path: PathBuf,
        create: bool,
        python: Option<String>,
        uv_create_args: Option<Vec<String>>,
        python_create_args: Option<Vec<String>>,
    },
    Module(String, toml::Value),
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
            EnvDirective::Module(name, _) => write!(f, "module {}", name),
            EnvDirective::PythonVenv {
                path,
                create,
                python,
                uv_create_args,
                python_create_args,
            } => {
                write!(f, "python venv path={}", display_path(path))?;
                if *create {
                    write!(f, " create")?;
                }
                if let Some(python) = python {
                    write!(f, " python={}", python)?;
                }
                if let Some(args) = uv_create_args {
                    write!(f, " uv_create_args={:?}", args)?;
                }
                if let Some(args) = python_create_args {
                    write!(f, " python_create_args={:?}", args)?;
                }
                Ok(())
            }
        }
    }
}

pub struct EnvResults {
    pub env: IndexMap<String, (String, PathBuf)>,
    pub env_remove: BTreeSet<String>,
    pub env_files: Vec<PathBuf>,
    pub env_paths: Vec<PathBuf>,
    pub env_scripts: Vec<PathBuf>,
}

impl EnvResults {
    pub fn resolve(initial: &EnvMap, input: Vec<(EnvDirective, PathBuf)>) -> eyre::Result<Self> {
        let mut ctx = BASE_CONTEXT.clone();
        // trace!("resolve: input: {:#?}", &input);
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
        let normalize_path = |config_root: &Path, p: PathBuf| {
            let p = p.strip_prefix("./").unwrap_or(&p);
            match p.strip_prefix("~/") {
                Ok(p) => dirs::HOME.join(p),
                _ if p.is_relative() => config_root.join(p),
                _ => p.to_path_buf(),
            }
        };
        let mut paths: Vec<(PathEntry, PathBuf)> = Vec::new();
        for (directive, source) in input.clone() {
            // trace!(
            //     "resolve: directive: {:?}, source: {:?}",
            //     &directive,
            //     &source
            // );
            let config_root = config_root(&source);
            ctx.insert("cwd", &*dirs::CWD);
            ctx.insert("config_root", &config_root);
            let env_vars = env
                .iter()
                .map(|(k, (v, _))| (k.clone(), v.clone()))
                .collect::<EnvMap>();
            ctx.insert("env", &env_vars);
            // trace!("resolve: ctx.get('env'): {:#?}", &ctx.get("env"));
            match directive {
                EnvDirective::Val(k, v) => {
                    let v = r.parse_template(&ctx, &source, &v)?;
                    r.env_remove.remove(&k);
                    // trace!("resolve: inserting {:?}={:?} from {:?}", &k, &v, &source);
                    env.insert(k, (v, Some(source.clone())));
                }
                EnvDirective::Rm(k) => {
                    env.shift_remove(&k);
                    r.env_remove.insert(k);
                }
                EnvDirective::Path(input_str) => {
                    Self::path(&mut ctx, &mut r, &mut paths, source, input_str)?;
                }
                EnvDirective::File(input) => {
                    Self::file(
                        &mut ctx,
                        &mut env,
                        &mut r,
                        normalize_path,
                        &source,
                        &config_root,
                        input,
                    )?;
                }
                EnvDirective::Source(input) => {
                    Self::source(
                        &mut ctx,
                        &mut env,
                        &mut r,
                        normalize_path,
                        &source,
                        &config_root,
                        &env_vars,
                        input,
                    );
                }
                EnvDirective::PythonVenv {
                    path,
                    create,
                    python,
                    uv_create_args,
                    python_create_args,
                } => {
                    Self::venv(
                        &mut ctx,
                        &mut env,
                        &mut r,
                        normalize_path,
                        &source,
                        &config_root,
                        env_vars,
                        path,
                        create,
                        python,
                        uv_create_args,
                        python_create_args,
                    )?;
                }
                EnvDirective::Module(name, value) => {
                    Self::module(&mut r, source, name, &value)?;
                }
            };
        }
        let env_vars = env
            .iter()
            .map(|(k, (v, _))| (k.clone(), v.clone()))
            .collect::<HashMap<_, _>>();
        ctx.insert("env", &env_vars);
        for (k, (v, source)) in env {
            if let Some(source) = source {
                r.env.insert(k, (v, source));
            }
        }
        // trace!("resolve: paths: {:#?}", &paths);
        // trace!("resolve: ctx.env: {:#?}", &ctx.get("env"));
        for (entry, source) in paths {
            // trace!("resolve: entry: {:?}, source: {}", &entry, display_path(source));
            let config_root = source
                .parent()
                .map(Path::to_path_buf)
                .or_else(|| dirs::CWD.clone())
                .unwrap_or_default();
            let s = match entry {
                PathEntry::Normal(pb) => pb.to_string_lossy().to_string(),
                PathEntry::Lazy(pb) => {
                    // trace!("resolve: s: {:?}", &s);
                    r.parse_template(&ctx, &source, pb.to_string_lossy().as_ref())?
                }
            };
            env::split_paths(&s)
                .map(|s| normalize_path(&config_root, s))
                .for_each(|p| r.env_paths.push(p.clone()));
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
impl Debug for EnvResults {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let mut ds = f.debug_struct("EnvResults");
        if !self.env.is_empty() {
            ds.field("env", &self.env);
        }
        if !self.env_remove.is_empty() {
            ds.field("env_remove", &self.env_remove);
        }
        if !self.env_files.is_empty() {
            ds.field("env_files", &self.env_files);
        }
        if !self.env_paths.is_empty() {
            ds.field("env_paths", &self.env_paths);
        }
        if !self.env_scripts.is_empty() {
            ds.field("env_scripts", &self.env_scripts);
        }
        ds.finish()
    }
}
