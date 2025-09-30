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

#[derive(Debug, Clone, Default, PartialEq)]
pub enum RequiredValue {
    #[default]
    False,
    True,
    Help(String),
}

impl RequiredValue {
    pub fn is_required(&self) -> bool {
        !matches!(self, RequiredValue::False)
    }

    pub fn help_text(&self) -> Option<&str> {
        match self {
            RequiredValue::Help(text) => Some(text.as_str()),
            _ => None,
        }
    }
}

impl<'de> serde::Deserialize<'de> for RequiredValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::{self, Visitor};
        use std::fmt;

        struct RequiredVisitor;

        impl<'de> Visitor<'de> for RequiredVisitor {
            type Value = RequiredValue;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a boolean or a string")
            }

            fn visit_bool<E>(self, value: bool) -> Result<RequiredValue, E>
            where
                E: de::Error,
            {
                Ok(if value {
                    RequiredValue::True
                } else {
                    RequiredValue::False
                })
            }

            fn visit_str<E>(self, value: &str) -> Result<RequiredValue, E>
            where
                E: de::Error,
            {
                Ok(RequiredValue::Help(value.to_string()))
            }

            fn visit_string<E>(self, value: String) -> Result<RequiredValue, E>
            where
                E: de::Error,
            {
                Ok(RequiredValue::Help(value))
            }
        }

        deserializer.deserialize_any(RequiredVisitor)
    }
}

impl serde::Serialize for RequiredValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            RequiredValue::False => serializer.serialize_bool(false),
            RequiredValue::True => serializer.serialize_bool(true),
            RequiredValue::Help(text) => serializer.serialize_str(text),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EnvDirectiveOptions {
    #[serde(default)]
    pub(crate) tools: bool,
    #[serde(default)]
    pub(crate) redact: Option<bool>,
    #[serde(default)]
    pub(crate) required: RequiredValue,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum EnvDirective {
    /// simple key/value pair
    Val(String, String, EnvDirectiveOptions),
    /// remove a key
    Rm(String, EnvDirectiveOptions),
    /// Required variable that must be defined elsewhere
    Required(String, EnvDirectiveOptions),
    /// dotenv file
    File(String, EnvDirectiveOptions),
    /// add a path to the PATH
    Path(String, EnvDirectiveOptions),
    /// run a bash script and apply the resulting env diff
    Source(String, EnvDirectiveOptions),
    /// [experimental] age-encrypted value
    Age {
        key: String,
        value: String,
        format: Option<AgeFormat>,
        options: EnvDirectiveOptions,
    },
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
            | EnvDirective::Required(_, opts)
            | EnvDirective::File(_, opts)
            | EnvDirective::Path(_, opts)
            | EnvDirective::Source(_, opts)
            | EnvDirective::Age { options: opts, .. }
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
            EnvDirective::Required(k, _) => write!(f, "{k} (required)"),
            EnvDirective::File(path, _) => write!(f, "_.file = \"{}\"", display_path(path)),
            EnvDirective::Path(path, _) => write!(f, "_.path = \"{}\"", display_path(path)),
            EnvDirective::Source(path, _) => write!(f, "_.source = \"{}\"", display_path(path)),
            EnvDirective::Age { key, format, .. } => {
                write!(f, "{key} (age-encrypted")?;
                if let Some(fmt) = format {
                    let fmt_str = match fmt {
                        AgeFormat::Zstd => "zstd",
                        AgeFormat::Raw => "raw",
                    };
                    write!(f, ", {fmt_str}")?;
                }
                write!(f, ")")
            }
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

#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub enum AgeFormat {
    #[serde(rename = "zstd")]
    Zstd,
    #[serde(rename = "raw")]
    #[default]
    Raw,
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
    pub warn_on_missing_required: bool,
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
        let filtered_input = input
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

        // Save filtered_input for validation after processing
        let filtered_input_for_validation = filtered_input.clone();

        for (directive, source) in filtered_input {
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
                        if redact.unwrap_or(false) {
                            r.redactions.push(k.clone());
                        }
                        env.insert(k, (v, Some(source.clone())));
                    }
                }
                EnvDirective::Rm(k, _opts) => {
                    env.shift_remove(&k);
                    r.env_remove.insert(k);
                }
                EnvDirective::Required(_k, _opts) => {
                    // Required directives don't set any value - they only validate during validation phase
                    // The actual value must come from the initial environment or a later config file
                }
                EnvDirective::Age {
                    key: ref k,
                    ref options,
                    ..
                } => {
                    // Decrypt age-encrypted value
                    let mut decrypted_v = crate::agecrypt::decrypt_age_directive(&directive)
                        .await
                        .map_err(|e| eyre!("[experimental] Failed to decrypt {}: {}", k, e))?;

                    // Parse as template after decryption
                    decrypted_v = r.parse_template(&ctx, &mut tera, &source, &decrypted_v)?;

                    if resolve_opts.vars {
                        r.vars.insert(k.clone(), (decrypted_v, source.clone()));
                    } else {
                        r.env_remove.remove(k);
                        // Handle redaction for age-encrypted values
                        // We're already in the EnvDirective::Age match arm, so we know this is an Age directive

                        // For age-encrypted values, we default to redacting for security
                        // With nullable redact, we can now distinguish between:
                        // - None: not specified (default for age is to redact for security)
                        // - Some(true): explicitly redact
                        // - Some(false): explicitly don't redact
                        debug!("Age directive {}: redact = {:?}", k, options.redact);
                        match options.redact {
                            Some(false) => {
                                // User explicitly set redact = false - don't redact
                                debug!(
                                    "Age directive {}: NOT redacting (explicit redact = false)",
                                    k
                                );
                            }
                            Some(true) | None => {
                                // Either explicitly redact or use age default (redact for security)
                                debug!(
                                    "Age directive {}: redacting (redact = {:?})",
                                    k, options.redact
                                );
                                r.redactions.push(k.clone());
                            }
                        }
                        env.insert(k.clone(), (decrypted_v, Some(source.clone())));
                    }
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
                                if redact.unwrap_or(false) {
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
                                if redact.unwrap_or(false) {
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
                    Self::module(&mut r, source, name, &value, redact.unwrap_or(false)).await?;
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

        // Validate required environment variables
        Self::validate_required_env_vars(
            &filtered_input_for_validation,
            initial,
            &r,
            resolve_opts.warn_on_missing_required,
        )?;

        Ok(r)
    }

    fn validate_required_env_vars(
        input: &[(EnvDirective, PathBuf)],
        initial: &EnvMap,
        env_results: &EnvResults,
        warn_mode: bool,
    ) -> eyre::Result<()> {
        let mut required_vars = Vec::new();

        // Collect all required environment variables with their options
        for (directive, source) in input {
            match directive {
                EnvDirective::Val(key, _, options) if options.required.is_required() => {
                    required_vars.push((key.clone(), source.clone(), options.required.clone()));
                }
                EnvDirective::Required(key, options) => {
                    required_vars.push((key.clone(), source.clone(), options.required.clone()));
                }
                _ => {}
            }
        }

        // Check if required variables are defined
        for (var_name, declaring_source, required_value) in required_vars {
            // Variable must be defined either:
            // 1. In the initial environment (before mise runs), OR
            // 2. In a config file processed later than the one declaring it as required
            let is_predefined = initial.contains_key(&var_name);

            let is_defined_later = if let Some((_, var_source)) = env_results.env.get(&var_name) {
                // Check if the variable comes from a different config file
                var_source != &declaring_source
            } else {
                false
            };

            if !is_predefined && !is_defined_later {
                let base_message = format!(
                    "Required environment variable '{}' is not defined. It must be set before mise runs or in a later config file. (Required in: {})",
                    var_name,
                    display_path(declaring_source)
                );

                let message = if let Some(help) = required_value.help_text() {
                    format!("{}\nHelp: {}", base_message, help)
                } else {
                    base_message
                };

                if warn_mode {
                    warn!("{}", message);
                } else {
                    return Err(eyre!("{}", message));
                }
            }
        }

        Ok(())
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
