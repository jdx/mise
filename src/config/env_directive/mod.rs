use crate::config::config_file::trust_check;
use crate::dirs;
use crate::env;
use crate::env_diff::EnvMap;
use crate::file::display_path;
use eyre::{Context, eyre};
use indexmap::IndexMap;
use itertools::Itertools;
use serde_json::Value;
use std::collections::{BTreeSet, HashMap};
use std::fmt::{Debug, Display, Formatter};
use std::path::{Path, PathBuf};
use std::{cmp::PartialEq, sync::Arc};

use super::Config;

mod engine;
mod file;
mod module;
mod normalize;
mod path;
mod source;
mod template;
mod venv;

use normalize::normalize_env_path;

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
pub struct EnvDirectiveOptions {
    pub(crate) tools: bool,
    pub(crate) redact: bool,
}

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

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
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
            EnvDirective::File(path, _) => write!(f, "_.file = \"{}\"", display_path(path)),
            EnvDirective::Path(path, _) => write!(f, "_.path = \"{}\"", display_path(path)),
            EnvDirective::Source(path, _) => write!(f, "_.source = \"{}\"", display_path(path)),
            EnvDirective::Module(name, _, _) => write!(f, "module {name}"),
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
                    write!(f, " python={python}")?;
                }
                if let Some(args) = uv_create_args {
                    write!(f, " uv_create_args={args:?}")?;
                }
                if let Some(args) = python_create_args {
                    write!(f, " python_create_args={args:?}")?;
                }
                Ok(())
            }
        }
    }
}

#[derive(Default, Clone)]
pub struct EnvResults {
    pub env: IndexMap<String, (String, PathBuf)>,
    pub vars: IndexMap<String, (String, PathBuf)>,
    pub env_remove: BTreeSet<String>,
    pub env_files: Vec<PathBuf>,
    pub env_paths: Vec<PathBuf>,
    pub env_scripts: Vec<PathBuf>,
    pub redactions: Vec<String>,
    pub tool_add_paths: Vec<PathBuf>,
}

#[derive(Debug, Clone)]
pub enum ToolsFilter {
    ToolsOnly,
    NonToolsOnly,
    Both,
}

impl Default for ToolsFilter {
    fn default() -> Self {
        Self::NonToolsOnly
    }
}

pub struct EnvResolveOptions {
    pub vars: bool,
    pub tools: ToolsFilter,
}

impl EnvResults {
    fn apply_kv(
        &mut self,
        env: &mut IndexMap<String, (String, Option<PathBuf>)>,
        key: String,
        value: String,
        source: PathBuf,
        redact: bool,
        vars_mode: bool,
    ) {
        if vars_mode {
            self.vars.insert(key, (value, source));
            return;
        }

        self.env_remove.remove(&key);
        if redact {
            self.redactions.push(key.clone());
        }
        env.insert(key, (value, Some(source)));
    }

    pub async fn resolve(
        config: &Arc<Config>,
        mut ctx: tera::Context,
        initial: &EnvMap,
        input: Vec<(EnvDirective, PathBuf)>,
        resolve_opts: EnvResolveOptions,
    ) -> eyre::Result<Self> {
        // trace!("resolve: input: {:#?}", &input);
        let mut env = initial
            .iter()
            .map(|(k, v)| (k.clone(), (v.clone(), None)))
            .collect::<IndexMap<_, _>>();
        let mut r = Self {
            env: Default::default(),
            vars: Default::default(),
            env_remove: BTreeSet::new(),
            env_files: Vec::new(),
            env_paths: Vec::new(),
            env_scripts: Vec::new(),
            redactions: Vec::new(),
            tool_add_paths: Vec::new(),
        };
        let mut paths: Vec<(PathBuf, PathBuf)> = Vec::new();
        let input = engine::DirectivePlanner::plan(&input, &resolve_opts);
        for (directive, source) in input {
            // trace!(
            //     "resolve: directive: {:?}, source: {:?}",
            //     &directive,
            //     &source
            // );
            let config_root = crate::config::config_file::config_root::config_root(&source);
            ctx.insert("cwd", &*dirs::CWD);
            ctx.insert("config_root", &config_root);
            let env_vars = env
                .iter()
                .map(|(k, (v, _))| (k.clone(), v.clone()))
                .collect::<EnvMap>();
            ctx.insert("env", &env_vars);

            let mut tera = template::build_tera_for_source(&source, &env_vars);

            let mut vars: EnvMap = if let Some(Value::Object(existing_vars)) = ctx.get("vars") {
                existing_vars
                    .iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                    .collect()
            } else {
                EnvMap::new()
            };

            vars.extend(r.vars.iter().map(|(k, (v, _))| (k.clone(), v.clone())));

            ctx.insert("vars", &vars);
            let redact = directive.options().redact;
            let exec = ExecCtx {
                config,
                r: &mut r,
                ctx: &mut ctx,
                tera: &mut tera,
                source: &source,
                config_root: &config_root,
                env: &mut env,
                paths: &mut paths,
                redact,
                vars_mode: resolve_opts.vars,
            };
            // trace!("resolve: ctx.get('env'): {:#?}", &ctx.get("env"));
            match directive {
                EnvDirective::Val(k, v, _opts) => {
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
                }
                EnvDirective::Rm(k, _opts) => {
                    exec.env.shift_remove(&k);
                    exec.r.env_remove.insert(k);
                }
                EnvDirective::Path(input_str, _opts) => {
                    let path =
                        EnvResults::path(exec.ctx, exec.tera, exec.r, exec.source, input_str)
                            .await?;
                    exec.paths.push((path.clone(), exec.source.to_path_buf()));
                    // Don't modify PATH in env - just add to env_paths
                    // This allows consumers to control PATH ordering
                }
                EnvDirective::File(input, _opts) => {
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
                }
                EnvDirective::Source(input, _opts) => {
                    let files = EnvResults::source(
                        exec.ctx,
                        exec.tera,
                        exec.paths,
                        exec.r,
                        normalize_env_path,
                        exec.source,
                        exec.config_root,
                        &env_vars,
                        input,
                    )?;
                    for (f, new_env) in files {
                        exec.r.env_scripts.push(f.clone());
                        for (k, v) in new_env {
                            exec.r
                                .apply_kv(exec.env, k, v, f.clone(), exec.redact, exec.vars_mode);
                        }
                    }
                }
                EnvDirective::PythonVenv {
                    path,
                    create,
                    python,
                    uv_create_args,
                    python_create_args,
                    options: _opts,
                } => {
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
                    .await?;
                }
                EnvDirective::Module(name, value, _opts) => {
                    Self::module(exec.r, exec.source.to_path_buf(), name, &value, exec.redact)
                        .await?;
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
        for (source, paths) in &paths.iter().chunk_by(|(_, source)| source) {
            // Use the computed config_root (project root for nested configs) for path resolution
            // to be consistent with other env directives like _.source and _.file
            let config_root = crate::config::config_file::config_root::config_root(source);
            let paths = paths.map(|(p, _)| p).collect_vec();
            let mut paths = paths
                .iter()
                .rev()
                .flat_map(|path| env::split_paths(path))
                .map(|s| normalize_env_path(&config_root, s))
                .collect::<Vec<_>>();
            // r.env_paths is already reversed and paths should prepend r.env_paths
            paths.reverse();
            paths.extend(r.env_paths);
            r.env_paths = paths;
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

    pub fn is_empty(&self) -> bool {
        self.env.is_empty()
            && self.vars.is_empty()
            && self.env_remove.is_empty()
            && self.env_files.is_empty()
            && self.env_paths.is_empty()
            && self.env_scripts.is_empty()
            && self.tool_add_paths.is_empty()
    }
}

impl Debug for EnvResults {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let mut ds = f.debug_struct("EnvResults");
        if !self.env.is_empty() {
            ds.field("env", &self.env.keys().collect::<Vec<_>>());
        }
        if !self.vars.is_empty() {
            ds.field("vars", &self.vars.keys().collect::<Vec<_>>());
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
        if !self.tool_add_paths.is_empty() {
            ds.field("tool_add_paths", &self.tool_add_paths);
        }
        ds.finish()
    }
}
