use crate::config::config_file::trust_check;
use crate::dirs;
use crate::env;
use crate::env_diff::EnvMap;
use crate::file::display_path;
use crate::path_env::PathEnv;
use crate::tera::{contains_template_syntax, get_tera, render_str, tera_exec};
use eyre::{Context, eyre};
use indexmap::IndexMap;
use itertools::Itertools;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::{Debug, Display, Formatter};
use std::path::{Path, PathBuf};
use std::{cmp::PartialEq, sync::Arc};

use super::{Config, Settings};

mod file;
mod module;
mod path;
mod source;
pub(crate) mod venv;

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
    /// use a fallback value if the key is not already set
    Default(String, String, EnvDirectiveOptions),
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
            | EnvDirective::Default(_, _, opts)
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
            EnvDirective::Default(k, v, _) => write!(f, "{k} default={v}"),
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
    /// Files to watch for cache invalidation (from modules and _.source directives)
    pub watch_files: Vec<PathBuf>,
    /// True if any directive declared cacheable=false or is a dynamic module
    pub has_uncacheable: bool,
}

#[derive(Debug, Clone, Default)]
pub enum ToolsFilter {
    ToolsOnly,
    #[default]
    NonToolsOnly,
    Both,
    /// `tools = true` directives, but only plain `Val` (`KEY = value`) entries.
    /// Env *modules* (PythonVenv/Module/Source/File/Path) are skipped because they
    /// may reference tools outside a partial (dependency) toolset and would error.
    /// Used by `dependency_env` so a dependent tool's install sees `tools = true`
    /// value vars like `CLOUDSDK_PYTHON = "{{ tools.python.path }}/..."`. (#10282)
    ToolsOnlyVals,
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
        let mut r = Self::default();
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
                    ToolsFilter::ToolsOnlyVals => {
                        directive.options().tools && matches!(directive, EnvDirective::Val(..))
                    }
                };

                if !should_include {
                    return acc;
                }

                if let Some(d) = &last_python_venv
                    && matches!(directive, EnvDirective::PythonVenv { .. })
                    && **d != *directive
                {
                    // skip venv directives if it's not the last one
                    return acc;
                }
                acc.push((directive.clone(), source.clone()));
                acc
            });

        // Save filtered_input for validation after processing
        let filtered_input_for_validation = filtered_input.clone();

        for (directive, source) in filtered_input {
            let mut tera = None;
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

            let context_vars: EnvMap = if let Some(Value::Object(existing_vars)) = ctx.get("vars") {
                existing_vars
                    .iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                    .collect()
            } else {
                EnvMap::new()
            };

            let mut vars = context_vars.clone();
            vars.extend(r.vars.iter().map(|(k, (v, _))| (k.clone(), v.clone())));

            ctx.insert("vars", &vars);
            let redact = directive.options().redact;
            // trace!("resolve: ctx.get('env'): {:#?}", &ctx.get("env"));
            match directive {
                EnvDirective::Val(k, v, _opts) => {
                    let v = r.parse_template(&ctx, &mut tera, &source, &env_vars, &v)?;

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
                EnvDirective::Default(k, v, _opts) => {
                    if resolve_opts.vars {
                        if let Some((v, _)) = r.vars.get(&k).filter(|(v, _)| !v.is_empty()) {
                            if redact.unwrap_or(false) {
                                r.redactions.push(k.clone());
                            }
                            r.vars.insert(k, (v.clone(), source.clone()));
                            continue;
                        }
                        if let Some(v) = env::PRISTINE_ENV.get(&k).filter(|v| !v.is_empty()) {
                            if redact.unwrap_or(false) {
                                r.redactions.push(k.clone());
                            }
                            r.vars.insert(k, (v.clone(), source.clone()));
                            continue;
                        }
                    } else if env.get(&k).is_some_and(|(v, _)| !v.is_empty()) {
                        if redact.unwrap_or(false) {
                            r.redactions.push(k.clone());
                        }
                        continue;
                    }

                    let v = r.parse_template(&ctx, &mut tera, &source, &env_vars, &v)?;

                    if resolve_opts.vars {
                        r.vars.insert(k, (v, source.clone()));
                    } else {
                        r.env_remove.remove(&k);
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
                    let res = crate::agecrypt::decrypt_age_directive(&directive).await;
                    let decrypted_v = match res {
                        Ok(decrypted_v) => {
                            // Parse as template after decryption
                            r.parse_template(&ctx, &mut tera, &source, &env_vars, &decrypted_v)?
                        }
                        Err(e) if Settings::get().age.strict => {
                            return Err(e)
                                .wrap_err(eyre!("[experimental] Failed to decrypt {}", k));
                        }
                        Err(e) => {
                            debug!(
                                "[experimental] Age decryption failed for {} but continuing in non-strict mode: {}",
                                k, e
                            );
                            // continue to the next directive
                            continue;
                        }
                    };

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
                    let path =
                        Self::path(&mut ctx, &mut tera, &mut r, &source, &env_vars, input_str)
                            .await?;
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
                        &env_vars,
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
                        &env_vars,
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
                        &env_vars,
                        &config_root,
                        env_vars.clone(),
                        path,
                        create,
                        python,
                        uv_create_args,
                        python_create_args,
                    )
                    .await?;
                }
                EnvDirective::Module(name, value, _opts) => {
                    let mut env_map: IndexMap<String, String> = env
                        .iter()
                        .map(|(k, (v, _))| (k.clone(), v.clone()))
                        .collect();
                    // Incorporate _.path entries accumulated so far into PATH
                    // so that cmd.exec in the plugin can find tools on PATH.
                    if !paths.is_empty() {
                        let existing_path =
                            env_map.get(&*env::PATH_KEY).cloned().unwrap_or_default();
                        let mut path_env = PathEnv::from_path_str(&existing_path);
                        for (p, path_source) in &paths {
                            let config_root =
                                crate::config::config_file::config_root::config_root(path_source);
                            for s in env::split_paths(p) {
                                path_env.add(normalize_path(&config_root, s));
                            }
                        }
                        env_map.insert(env::PATH_KEY.to_string(), path_env.to_string());
                    }
                    if log::log_enabled!(log::Level::Trace) {
                        if let Some(path) = env_map.get(&*env::PATH_KEY) {
                            trace!("module {name}: PATH={path}");
                        } else {
                            trace!("module {name}: no PATH in env_map");
                        }
                    }
                    let env_before: IndexMap<String, (String, PathBuf)> = r.env.clone();
                    Self::module(&mut r, config, source, name, &value, redact, env_map).await?;
                    // Merge entries that this module call added or changed into
                    // the local `env` so they are visible in the Tera context
                    // for subsequent directives.  Keys unchanged in `r.env`
                    // (same value before and after this call) are skipped, which
                    // preserves any Val/File/Source override in `env` applied
                    // after a prior module emitted the same value.  When a
                    // module emits a *different* value the merge writes it
                    // through — "later directive wins", consistent with all
                    // other directive pairs.
                    for (k, (v, src)) in &r.env {
                        let added_or_changed = match env_before.get(k) {
                            Some((old_v, _)) => old_v != v,
                            None => true,
                        };
                        if added_or_changed {
                            env.insert(k.clone(), (v.clone(), Some(src.clone())));
                        }
                    }
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
                EnvDirective::Default(key, _, options) if options.required.is_required() => {
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
        tera: &mut Option<tera::Tera>,
        path: &Path,
        exec_env: &EnvMap,
        input: &str,
    ) -> eyre::Result<String> {
        let mut output = input.to_string();

        // Step 1: Tera template expansion
        if contains_template_syntax(input) {
            trust_check(path)?;
            let tera = tera.get_or_insert_with(|| {
                let mut tera = get_tera(path.parent());
                tera.register_function(
                    "exec",
                    tera_exec(path.parent().map(|d| d.to_path_buf()), exec_env.clone()),
                );
                tera
            });
            output = render_str(tera, input, ctx)
                .wrap_err_with(|| eyre!("failed to parse template: '{input}'"))?;
        }

        // Step 2: Shell-style $VAR expansion
        if output.contains('$') {
            debug_assert!(
                !env!("CARGO_PKG_VERSION").starts_with("2026.7"),
                "change env_shell_expand default to true and remove this warning"
            );
            match Settings::get().env_shell_expand {
                Some(true) => {
                    let env_vars: BTreeMap<String, String> = ctx
                        .get("env")
                        .and_then(|v| serde_json::from_value(v.clone()).ok())
                        .unwrap_or_default();
                    let mut missing_vars = Vec::new();
                    output = shell_expand_env(&output, &env_vars, &mut missing_vars);
                    for var in missing_vars {
                        warn_once!(
                            "env var '{var}' is not defined and will be left unexpanded. \
                             Use ${{{var}:-}} to default to an empty string and suppress \
                             this warning."
                        );
                    }
                }
                Some(false) => {}
                None => {
                    warn_once!(
                        "env value contains '$' which will be expanded in a future release. \
                         Set `env_shell_expand = true` to opt in or `env_shell_expand = false` to \
                         keep current behavior and suppress this warning."
                    );
                }
            }
        }

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

fn shell_expand_env(
    input: &str,
    env_vars: &BTreeMap<String, String>,
    missing_vars: &mut Vec<String>,
) -> String {
    let mut output = String::with_capacity(input.len());
    let mut chars = input.char_indices().peekable();

    while let Some((idx, ch)) = chars.next() {
        if ch != '$' {
            output.push(ch);
            continue;
        }

        match chars.peek().copied() {
            Some((_, '$')) => {
                chars.next();
                output.push('$');
            }
            Some((_, '{')) => {
                chars.next();
                if let Some((end, expr)) = read_braced_expr(input, idx + 2) {
                    output.push_str(&expand_braced_expr(
                        expr,
                        &input[idx..=end],
                        env_vars,
                        missing_vars,
                    ));
                    while chars.peek().is_some_and(|(i, _)| *i <= end) {
                        chars.next();
                    }
                } else {
                    output.push_str(&input[idx..]);
                    break;
                }
            }
            Some((_, next)) if is_var_start(next) => {
                let start = chars.peek().map(|(i, _)| *i).unwrap_or(idx + 1);
                let mut end = start;
                while let Some((i, next)) = chars.peek().copied() {
                    if is_var_char(next) {
                        end = i + next.len_utf8();
                        chars.next();
                    } else {
                        break;
                    }
                }
                let var = &input[start..end];
                if let Some(value) = env_vars.get(var) {
                    output.push_str(value);
                } else {
                    missing_vars.push(var.to_string());
                    output.push_str(&input[idx..end]);
                }
            }
            _ => output.push('$'),
        }
    }

    output
}

fn read_braced_expr(input: &str, start: usize) -> Option<(usize, &str)> {
    let mut depth = 1;
    let mut command_depth = 0;
    let mut chars = input[start..].char_indices().peekable();

    while let Some((offset, ch)) = chars.next() {
        let idx = start + offset;
        if command_depth > 0 {
            match ch {
                '(' => command_depth += 1,
                ')' => command_depth -= 1,
                _ => {}
            }
            continue;
        }

        match ch {
            '$' if chars.peek().is_some_and(|(_, next)| *next == '{') => {
                chars.next();
                depth += 1;
            }
            '$' if chars.peek().is_some_and(|(_, next)| *next == '(') => {
                chars.next();
                command_depth = 1;
            }
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some((idx, &input[start..idx]));
                }
            }
            _ => {}
        }
    }

    None
}

fn expand_braced_expr(
    expr: &str,
    original: &str,
    env_vars: &BTreeMap<String, String>,
    missing_vars: &mut Vec<String>,
) -> String {
    let Some(var_end) = expr
        .char_indices()
        .take_while(|(_, ch)| is_var_char(*ch))
        .last()
        .map(|(idx, ch)| idx + ch.len_utf8())
    else {
        return original.to_string();
    };

    let var = &expr[..var_end];
    if !var.chars().next().is_some_and(is_var_start) {
        return original.to_string();
    }

    match expr.get(var_end..) {
        Some("") => match env_vars.get(var) {
            Some(value) => value.to_string(),
            None => {
                missing_vars.push(var.to_string());
                original.to_string()
            }
        },
        Some(rest) if rest.starts_with(":-") => {
            let default = &rest[2..];
            match env_vars.get(var) {
                Some(value) if !value.is_empty() => value.to_string(),
                _ => shell_expand_env(default, env_vars, missing_vars),
            }
        }
        Some(rest) if rest.starts_with('-') => {
            let default = &rest[1..];
            match env_vars.get(var) {
                Some(value) => value.to_string(),
                None => shell_expand_env(default, env_vars, missing_vars),
            }
        }
        _ => original.to_string(),
    }
}

fn is_var_start(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphabetic()
}

fn is_var_char(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphanumeric()
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::env_diff::EnvMap;
    use crate::tera::BASE_CONTEXT;

    /// `ToolsFilter::ToolsOnlyVals` must select only `tools = true` `Val`
    /// directives — excluding `tools = false` vars and `tools = true` *modules*
    /// (here a `Path`). This is what `dependency_env` relies on to surface vars
    /// like `CLOUDSDK_PYTHON = "{{ tools.python.path }}/..."` during a dependent
    /// tool's install without running env modules on a partial toolset. (#10282)
    #[tokio::test]
    async fn test_tools_only_vals_filter() {
        let env = EnvMap::new();
        let config = Config::get().await.unwrap();
        let tools = EnvDirectiveOptions {
            tools: true,
            ..Default::default()
        };
        let results = EnvResults::resolve(
            &config,
            BASE_CONTEXT.clone(),
            &env,
            vec![
                // tools = true Val -> included
                (
                    EnvDirective::Val("TOOLS_VAL".into(), "yes".into(), tools.clone()),
                    PathBuf::from("/config"),
                ),
                // tools = false Val -> excluded
                (
                    EnvDirective::Val("PLAIN_VAL".into(), "no".into(), Default::default()),
                    PathBuf::from("/config"),
                ),
                // tools = true module (Path) -> excluded
                (
                    EnvDirective::Path("/should/not/appear".into(), tools.clone()),
                    PathBuf::from("/config"),
                ),
            ],
            EnvResolveOptions {
                vars: false,
                tools: ToolsFilter::ToolsOnlyVals,
                warn_on_missing_required: false,
            },
        )
        .await
        .unwrap();
        let keys: Vec<String> = results.env.keys().cloned().collect();
        assert_eq!(keys, vec!["TOOLS_VAL".to_string()]);
        assert!(results.env_paths.is_empty());
    }
}
