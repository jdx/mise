use crate::env;
use std::cmp::PartialEq;
use std::collections::{BTreeSet, HashMap};
use std::fmt::{Debug, Display, Formatter};
use std::path::{Path, PathBuf};

use crate::config::config_file::{config_root, trust_check};
use crate::dirs;
use crate::env_diff::EnvMap;
use crate::file::display_path;
use crate::tera::{get_tera, tera_exec};
use eyre::{eyre, Context};
use indexmap::IndexMap;

mod file;
mod module;
mod path;
mod source;
mod venv;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct EnvDirectiveOptions {
    pub(crate) tools: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum EnvDirective {
    /// simple key/value pair
    Val(String, String, EnvDirectiveOptions),
    /// remove a key
    Rm(String, EnvDirectiveOptions),
    /// dotenv file
    File(String, EnvDirectiveOptions),
    /// add a path to the PATH
    Path(String, EnvDirectiveOptions),
    /// run a bash script and apply the resulting env diff
    Source(String, EnvDirectiveOptions),
    PythonVenv {
        path: String,
        create: bool,
        python: Option<String>,
        uv_create_args: Option<Vec<String>>,
        python_create_args: Option<Vec<String>>,
        options: EnvDirectiveOptions,
    },
    Module(String, toml::Value, EnvDirectiveOptions),
}

impl EnvDirective {
    pub fn options(&self) -> &EnvDirectiveOptions {
        match self {
            EnvDirective::Val(_, _, opts)
            | EnvDirective::Rm(_, opts)
            | EnvDirective::File(_, opts)
            | EnvDirective::Path(_, opts)
            | EnvDirective::Source(_, opts)
            | EnvDirective::PythonVenv { options: opts, .. }
            | EnvDirective::Module(_, _, opts) => opts,
        }
    }
}

impl From<(String, String)> for EnvDirective {
    fn from((k, v): (String, String)) -> Self {
        Self::Val(k, v, Default::default())
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
            EnvDirective::Val(k, v, _) => write!(f, "{k}={v}"),
            EnvDirective::Rm(k, _) => write!(f, "unset {k}"),
            EnvDirective::File(path, _) => write!(f, "dotenv {}", display_path(path)),
            EnvDirective::Path(path, _) => write!(f, "path_add {}", display_path(path)),
            EnvDirective::Source(path, _) => write!(f, "source {}", display_path(path)),
            EnvDirective::Module(name, _, _) => write!(f, "module {}", name),
            EnvDirective::PythonVenv {
                path,
                create,
                python,
                uv_create_args,
                python_create_args,
                ..
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

#[derive(Default)]
pub struct EnvResults {
    pub env: IndexMap<String, (String, PathBuf)>,
    pub env_remove: BTreeSet<String>,
    pub env_files: Vec<PathBuf>,
    pub env_paths: Vec<PathBuf>,
    pub env_scripts: Vec<PathBuf>,
}

impl EnvResults {
    pub fn resolve(
        mut ctx: tera::Context,
        initial: &EnvMap,
        input: Vec<(EnvDirective, PathBuf)>,
        tools: bool,
    ) -> eyre::Result<Self> {
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
        let mut paths: Vec<(PathBuf, PathBuf)> = Vec::new();
        let last_python_venv = input.iter().rev().find_map(|(d, _)| match d {
            EnvDirective::PythonVenv { .. } => Some(d),
            _ => None,
        });
        let input = input
            .iter()
            .fold(Vec::new(), |mut acc, (directive, source)| {
                // remove directives that need tools if we're not processing tool directives, or vice versa
                if directive.options().tools != tools {
                    return acc;
                }
                if let Some(d) = &last_python_venv {
                    if matches!(directive, EnvDirective::PythonVenv { .. }) && **d != *directive {
                        // skip venv directives if it's not the last one
                        return acc;
                    }
                }
                acc.push((directive.clone(), source.clone()));
                acc
            });
        for (directive, source) in input {
            let mut tera = get_tera(source.parent());
            tera.register_function(
                "exec",
                tera_exec(
                    source.parent().map(|d| d.to_path_buf()),
                    env.iter()
                        .map(|(k, (v, _))| (k.clone(), v.clone()))
                        .collect(),
                ),
            );
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
                EnvDirective::Val(k, v, _opts) => {
                    let v = r.parse_template(&ctx, &mut tera, &source, &v)?;
                    r.env_remove.remove(&k);
                    // trace!("resolve: inserting {:?}={:?} from {:?}", &k, &v, &source);
                    env.insert(k, (v, Some(source.clone())));
                }
                EnvDirective::Rm(k, _opts) => {
                    env.shift_remove(&k);
                    r.env_remove.insert(k);
                }
                EnvDirective::Path(input_str, _opts) => {
                    Self::path(&mut ctx, &mut tera, &mut r, &mut paths, source, input_str)?;
                }
                EnvDirective::File(input, _opts) => {
                    Self::file(
                        &mut ctx,
                        &mut tera,
                        &mut env,
                        &mut r,
                        normalize_path,
                        &source,
                        &config_root,
                        input,
                    )?;
                }
                EnvDirective::Source(input, _opts) => {
                    Self::source(
                        &mut ctx,
                        &mut tera,
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
                    options: _opts,
                } => {
                    Self::venv(
                        &mut ctx,
                        &mut tera,
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
                EnvDirective::Module(name, value, _opts) => {
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
        for (p, source) in paths {
            // trace!("resolve: entry: {:?}, source: {}", &entry, display_path(source));
            let config_root = source
                .parent()
                .map(Path::to_path_buf)
                .or_else(|| dirs::CWD.clone())
                .unwrap_or_default();
            env::split_paths(&p)
                .map(|s| normalize_path(&config_root, s))
                .for_each(|p| r.env_paths.push(p.clone()));
        }
        Ok(r)
    }

    fn parse_template(
        &self,
        ctx: &tera::Context,
        tera: &mut tera::Tera,
        path: &Path,
        input: &str,
    ) -> eyre::Result<String> {
        if !input.contains("{{") && !input.contains("{%") && !input.contains("{#") {
            return Ok(input.to_string());
        }
        trust_check(path)?;
        let output = tera
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
