use crate::config::config_file::trust_check;
use crate::dirs;
use crate::env;
use crate::env_diff::EnvMap;
use crate::file::display_path;
use crate::tera::{get_tera, tera_exec};
use eyre::{Context, eyre};
use indexmap::IndexMap;
use itertools::Itertools;
use serde_json::Value;
use std::collections::{BTreeSet, HashMap};
use std::fmt::{Debug, Display, Formatter};
use std::path::{Path, PathBuf};
use std::{cmp::PartialEq, sync::Arc};

use super::Config;

mod file;
mod module;
mod path;
mod source;
mod venv;

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EnvDirectiveOptions {
    #[serde(default)]
    pub(crate) tools: bool,
    #[serde(default)]
    pub(crate) redact: bool,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum EnvDirective {
    /// simple key/value pair
    Val(String, String, EnvDirectiveOptions),
    /// env var from 1password
    OnePassword {
        key: String,
        vault: Option<String>,
        item: Option<String>,
        field: Option<String>,
        reference: Option<String>, // op://vault/item/field format
        options: EnvDirectiveOptions,
    },
    /// env var from keyring
    Keyring {
        key: String,
        service: Option<String>,
        account: Option<String>,
        options: EnvDirectiveOptions,
    },
    /// env var that must be defined elsewhere
    Required {
        key: String,
        options: EnvDirectiveOptions,
    },
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
            | EnvDirective::OnePassword { options: opts, .. }
            | EnvDirective::Keyring { options: opts, .. }
            | EnvDirective::Required { options: opts, .. }
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
            EnvDirective::OnePassword { key, reference, .. } => {
                if let Some(ref_str) = reference {
                    write!(
                        f,
                        "{key} = {{ onepassword = {{ reference = \"{}\" }} }}",
                        ref_str
                    )
                } else {
                    write!(f, "{key} = {{ onepassword = {{}} }}")
                }
            }
            EnvDirective::Keyring {
                key,
                service,
                account,
                ..
            } => match (service, account) {
                (Some(s), Some(a)) => write!(
                    f,
                    "{key} = {{ keyring = {{ service = \"{s}\", account = \"{a}\" }} }}"
                ),
                (Some(s), None) => write!(f, "{key} = {{ keyring = {{ service = \"{s}\" }} }}"),
                (None, Some(a)) => write!(f, "{key} = {{ keyring = {{ account = \"{a}\" }} }}"),
                (None, None) => write!(f, "{key} = {{ keyring = {{}} }}"),
            },
            EnvDirective::Required { key, .. } => write!(f, "{key} = {{ required = true }}"),
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
                // Filter directives based on tools setting
                let should_include = match &resolve_opts.tools {
                    ToolsFilter::ToolsOnly => directive.options().tools,
                    ToolsFilter::NonToolsOnly => !directive.options().tools,
                    ToolsFilter::Both => true,
                };

                if !should_include {
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
            let config_root = crate::config::config_file::config_root::config_root(&source);
            ctx.insert("cwd", &*dirs::CWD);
            ctx.insert("config_root", &config_root);
            let env_vars = env
                .iter()
                .map(|(k, (v, _))| (k.clone(), v.clone()))
                .collect::<EnvMap>();
            ctx.insert("env", &env_vars);

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
            // trace!("resolve: ctx.get('env'): {:#?}", &ctx.get("env"));
            match directive {
                EnvDirective::Val(k, v, _opts) => {
                    let v = r.parse_template(&ctx, &mut tera, &source, &v)?;
                    if resolve_opts.vars {
                        r.vars.insert(k, (v, source.clone()));
                    } else {
                        r.env_remove.remove(&k);
                        // trace!("resolve: inserting {:?}={:?} from {:?}", &k, &v, &source);
                        if redact {
                            r.redactions.push(k.clone());
                        }
                        env.insert(k, (v, Some(source.clone())));
                    }
                }
                EnvDirective::OnePassword {
                    key,
                    vault,
                    item,
                    field,
                    reference,
                    options: _opts,
                } => {
                    if !resolve_opts.vars {
                        if redact {
                            r.redactions.push(key.clone());
                        }
                        // Create a serializable config for later resolution
                        let config = serde_json::json!({
                            "provider": "onepassword",
                            "vault": vault,
                            "item": item,
                            "field": field,
                            "reference": reference
                        });
                        env.insert(
                            key.clone(),
                            (format!("__MISE_SECRET__:{}", config), Some(source.clone())),
                        );
                    }
                }
                EnvDirective::Keyring {
                    key,
                    service,
                    account,
                    options: _opts,
                } => {
                    if !resolve_opts.vars {
                        if redact {
                            r.redactions.push(key.clone());
                        }
                        let config = serde_json::json!({
                            "provider": "keyring",
                            "service": service,
                            "account": account
                        });
                        env.insert(
                            key.clone(),
                            (format!("__MISE_SECRET__:{}", config), Some(source.clone())),
                        );
                    }
                }
                EnvDirective::Required {
                    key,
                    options: _opts,
                } => {
                    if !resolve_opts.vars {
                        // Mark as requiring resolution from external source
                        env.insert(
                            key.clone(),
                            (format!("__MISE_REQUIRED__:{}", key), Some(source.clone())),
                        );
                    }
                }
                EnvDirective::Rm(k, _opts) => {
                    env.shift_remove(&k);
                    r.env_remove.insert(k);
                }
                EnvDirective::Path(input_str, _opts) => {
                    let path = Self::path(&mut ctx, &mut tera, &mut r, &source, input_str).await?;
                    paths.push((path.clone(), source.clone()));
                    // Don't modify PATH in env - just add to env_paths
                    // This allows consumers to control PATH ordering
                }
                EnvDirective::File(input, _opts) => {
                    let files = Self::file(
                        config,
                        &mut ctx,
                        &mut tera,
                        &mut r,
                        normalize_path,
                        &source,
                        &config_root,
                        input,
                    )
                    .await?;
                    for (f, new_env) in files {
                        r.env_files.push(f.clone());
                        for (k, v) in new_env {
                            if resolve_opts.vars {
                                r.vars.insert(k, (v, f.clone()));
                            } else {
                                if redact {
                                    r.redactions.push(k.clone());
                                }
                                env.insert(k, (v, Some(f.clone())));
                            }
                        }
                    }
                }
                EnvDirective::Source(input, _opts) => {
                    let files = Self::source(
                        &mut ctx,
                        &mut tera,
                        &mut paths,
                        &mut r,
                        normalize_path,
                        &source,
                        &config_root,
                        &env_vars,
                        input,
                    )?;
                    for (f, new_env) in files {
                        r.env_scripts.push(f.clone());
                        for (k, v) in new_env {
                            if resolve_opts.vars {
                                r.vars.insert(k, (v, f.clone()));
                            } else {
                                if redact {
                                    r.redactions.push(k.clone());
                                }
                                env.insert(k, (v, Some(f.clone())));
                            }
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
                    Self::venv(
                        config,
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
                    )
                    .await?;
                }
                EnvDirective::Module(name, value, _opts) => {
                    Self::module(&mut r, source, name, &value, redact).await?;
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
                .map(|s| normalize_path(&config_root, s))
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
