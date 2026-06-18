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
    /// The profile this directive belongs to, if any.  Set programmatically
    /// when parsing `[env.profiles.<name>]` / `[vars.profiles.<name>]` sub-tables
    /// with experimental mode enabled.  Never read from TOML.
    #[serde(skip)]
    pub(crate) profile: Option<String>,
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

    /// Sets the `profile` tag on this directive.
    /// Used by the parser to tag directives parsed from
    /// `[env.profiles.<name>]` / `[vars.profiles.<name>]` sub-tables.
    pub fn set_profile(&mut self, profile: String) {
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
            | EnvDirective::Module(_, _, opts) => opts.profile = Some(profile),
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

/// Filter and reorder a flat directive stream for active-profile evaluation.
///
/// Implements the "later-wins" ordering for inline environment profiles
/// (`[env.profiles.<name>]` / `[vars.profiles.<name>]`).
///
/// ## Ordering contract (mirrors separate-file "later wins")
///
/// The reorder is performed **per source file**, grouping by `PathBuf` using
/// stable **first-seen** order (a path's block is anchored at its first
/// occurrence; later entries with the same path join that block).
/// Cross-file order is preserved so that child-directory config files keep
/// higher precedence than parent-directory files regardless of profile tags.
/// Within each per-file block:
///
/// 1. Directives with `profile == None` (base directives) are kept in their
///    original relative order and placed FIRST.
/// 2. Directives with `profile == Some(name)` where `name` is NOT present in
///    `active_envs` are DROPPED entirely.
/// 3. Remaining profile-tagged directives are grouped by profile name and
///    appended AFTER the base directives, ordered by their profile name's
///    position in `active_envs` (earlier index → emitted first → lower
///    precedence under a later-wins resolver).
/// 4. Within the same profile, original relative order is preserved.
///
/// Non-contiguous interleaving (e.g. `[A, B, A]`) is handled correctly: all
/// entries for a path are merged into that path's block, which is anchored at
/// the path's first occurrence in the stream.
///
/// ## Call sites
/// All top-level evaluation sites pass `&env::MISE_ENV_WITH_AUTO` directly as
/// `active_envs` (not a snapshot), so the result varies with the active env
/// list at call time.  Sub-resolvers do not call `resolve_for_config` (they
/// call raw `resolve` internally) and must not call this function again.
///
/// The function is intentionally pure (no global state), making it trivially
/// unit-testable without touching global env vars.
pub fn filter_and_order_by_profiles(
    directives: Vec<(EnvDirective, PathBuf)>,
    active_envs: &[String],
) -> Vec<(EnvDirective, PathBuf)> {
    // Build a fast lookup: profile_name -> index in active_envs.
    let profile_index: HashMap<&str, usize> = active_envs
        .iter()
        .enumerate()
        .map(|(i, name)| (name.as_str(), i))
        .collect();

    // Group entries by source PathBuf using stable first-seen order.
    // `file_order` maps each PathBuf to its index of first appearance.
    // `file_blocks` accumulates (base_entries, profile_buckets) per path.
    let mut file_order: HashMap<&PathBuf, usize> = HashMap::new();
    let mut file_blocks: Vec<(
        Vec<(EnvDirective, PathBuf)>,                  // base entries
        BTreeMap<usize, Vec<(EnvDirective, PathBuf)>>, // profile buckets keyed by active_envs index
    )> = Vec::new();

    for item in &directives {
        let path = &item.1;
        let block_idx = match file_order.get(path) {
            Some(&idx) => idx,
            None => {
                let idx = file_blocks.len();
                file_order.insert(path, idx);
                file_blocks.push((Vec::new(), BTreeMap::new()));
                idx
            }
        };
        let (base, profile_buckets) = &mut file_blocks[block_idx];
        match item.0.options().profile.as_deref() {
            None => base.push(item.clone()),
            Some(name) => {
                // Drop directives for inactive profiles.
                if let Some(&idx) = profile_index.get(name) {
                    profile_buckets.entry(idx).or_default().push(item.clone());
                }
            }
        }
    }

    // Emit blocks in first-seen order: base entries first, then profiles ordered
    // by their index in active_envs (BTreeMap iteration is ascending by key).
    let mut result: Vec<(EnvDirective, PathBuf)> = Vec::with_capacity(directives.len());
    for (base, profile_buckets) in file_blocks {
        result.extend(base);
        for (_idx, bucket) in profile_buckets {
            result.extend(bucket);
        }
    }

    result
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

    /// Top-level evaluation entry point for config-file and task-level directive
    /// streams.  This wrapper applies `filter_and_order_by_profiles` with the
    /// currently active env list (`&env::MISE_ENV_WITH_AUTO`) before delegating
    /// to `resolve`.
    ///
    /// **All top-level evaluation sites** (config load_env, resolve_vars, toolset
    /// env, and every task env/vars evaluation path) MUST route through this
    /// function rather than calling `resolve` directly.  Sub-resolvers (e.g. the
    /// path, venv, and file directive handlers) invoke `resolve` internally via
    /// recursion or helper calls, and must NOT call this function again
    /// (double-filtering is incorrect).
    ///
    /// The `active_envs` slice is read at call time from `env::MISE_ENV_WITH_AUTO`
    /// (not a snapshot), so the result naturally varies when MISE_ENV changes
    /// between calls.
    pub async fn resolve_for_config(
        config: &Arc<Config>,
        ctx: tera::Context,
        initial: &EnvMap,
        input: Vec<(EnvDirective, PathBuf)>,
        resolve_opts: EnvResolveOptions,
    ) -> eyre::Result<Self> {
        // Apply active-profile filtering and per-file reordering so that
        // child-directory config files retain higher precedence than parent-directory
        // files even when profile-tagged directives are present.
        let filtered = filter_and_order_by_profiles(input, &env::MISE_ENV_WITH_AUTO);
        Self::resolve(config, ctx, initial, filtered, resolve_opts).await
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

    // ── filter_and_order_by_profiles unit tests ───────────────────────────

    /// Helper to build a Val directive optionally tagged with a profile.
    fn val(key: &str, value: &str, profile: Option<&str>) -> (EnvDirective, PathBuf) {
        let opts = EnvDirectiveOptions {
            profile: profile.map(|s| s.to_string()),
            ..Default::default()
        };
        (
            EnvDirective::Val(key.to_string(), value.to_string(), opts),
            PathBuf::from("/config"),
        )
    }

    /// Extract (key, value) pairs from the output for easy assertions.
    fn kv(items: &[(EnvDirective, PathBuf)]) -> Vec<(&str, &str)> {
        items
            .iter()
            .map(|(d, _)| match d {
                EnvDirective::Val(k, v, _) => (k.as_str(), v.as_str()),
                _ => ("?", "?"),
            })
            .collect()
    }

    /// With no active envs the output must be exactly the base directives in
    /// their original order; any profile-tagged directives are dropped.
    #[test]
    fn test_profiles_empty_active_list_drops_profile_directives() {
        let input = vec![
            val("BASE1", "b1", None),
            val("BASE2", "b2", None),
            val("PROFILE_VAR", "pv", Some("prod")),
        ];
        let result = filter_and_order_by_profiles(input, &[]);
        assert_eq!(kv(&result), vec![("BASE1", "b1"), ("BASE2", "b2")]);
    }

    /// Profile-tagged directive for an INACTIVE profile must be dropped.
    #[test]
    fn test_profiles_inactive_profile_dropped() {
        let input = vec![
            val("BASE", "base_val", None),
            val("CI_VAR", "ci_val", Some("ci")),       // inactive
            val("PROD_VAR", "prod_val", Some("prod")), // active
        ];
        let result = filter_and_order_by_profiles(input, &["prod".to_string()]);
        assert_eq!(
            kv(&result),
            vec![("BASE", "base_val"), ("PROD_VAR", "prod_val")]
        );
    }

    /// Base directives must always precede profile directives in the output.
    #[test]
    fn test_profiles_base_always_before_profiles() {
        let input = vec![
            val("PROFILE_FIRST", "pf", Some("dev")), // profile appears before base in input
            val("BASE", "base_val", None),
        ];
        let result = filter_and_order_by_profiles(input, &["dev".to_string()]);
        let pairs = kv(&result);
        // BASE must come first regardless of input order
        assert_eq!(pairs[0], ("BASE", "base_val"));
        assert_eq!(pairs[1], ("PROFILE_FIRST", "pf"));
    }

    /// With active envs ["ci", "prod"], a var defined in both profiles must end
    /// up with the "prod" entry LAST (higher index wins under later-wins semantics).
    #[test]
    fn test_profiles_multi_env_last_wins_ordering() {
        let active: Vec<String> = vec!["ci".to_string(), "prod".to_string()];
        let input = vec![
            val("SHARED", "ci_value", Some("ci")),
            val("SHARED", "prod_value", Some("prod")),
        ];
        let result = filter_and_order_by_profiles(input, &active);
        // "ci" has index 0, "prod" has index 1 → ci emitted first, prod last
        let pairs = kv(&result);
        assert_eq!(
            pairs,
            vec![("SHARED", "ci_value"), ("SHARED", "prod_value")]
        );
    }

    /// Within a single profile, original relative order of directives must be
    /// preserved.
    #[test]
    fn test_profiles_within_profile_order_preserved() {
        let active = vec!["dev".to_string()];
        let input = vec![
            val("A", "1", Some("dev")),
            val("B", "2", Some("dev")),
            val("C", "3", Some("dev")),
        ];
        let result = filter_and_order_by_profiles(input, &active);
        assert_eq!(kv(&result), vec![("A", "1"), ("B", "2"), ("C", "3")]);
    }

    /// Pure-base input (no profile tags at all) must pass through unchanged.
    #[test]
    fn test_profiles_no_profile_tags_passthrough() {
        let input = vec![val("X", "1", None), val("Y", "2", None)];
        let result = filter_and_order_by_profiles(input.clone(), &["any".to_string()]);
        assert_eq!(kv(&result), kv(&input));
    }

    /// Three active envs: ordering must follow index position, not alphabetical
    /// order of profile names.
    #[test]
    fn test_profiles_three_active_envs_ordering() {
        // "c", "a", "b" — deliberately non-alphabetical to ensure we use index
        let active: Vec<String> = vec!["c".to_string(), "a".to_string(), "b".to_string()];
        let input = vec![
            val("BASE", "base", None),
            val("VAR", "b_val", Some("b")), // index 2
            val("VAR", "a_val", Some("a")), // index 1
            val("VAR", "c_val", Some("c")), // index 0
        ];
        let result = filter_and_order_by_profiles(input, &active);
        // Expected: BASE, then c (idx=0), then a (idx=1), then b (idx=2)
        assert_eq!(
            kv(&result),
            vec![
                ("BASE", "base"),
                ("VAR", "c_val"),
                ("VAR", "a_val"),
                ("VAR", "b_val"),
            ]
        );
    }

    // Helper: like val() but with a specific source path (to simulate multi-file streams).
    fn val_at(
        key: &str,
        value: &str,
        profile: Option<&str>,
        path: &str,
    ) -> (EnvDirective, PathBuf) {
        let opts = EnvDirectiveOptions {
            profile: profile.map(|s| s.to_string()),
            ..Default::default()
        };
        (
            EnvDirective::Val(key.to_string(), value.to_string(), opts),
            PathBuf::from(path),
        )
    }

    /// CROSS-FILE: a child-file base directive must still come AFTER a parent-file
    /// active-profile directive in the output so the later-wins resolver picks the
    /// child base value.
    ///
    /// Scenario (MISE_ENV=prod):
    ///   parent file block: [env] FOO=parent_base, [env.profiles.prod] FOO=parent_prod
    ///   child  file block: [env] FOO=child_base
    ///
    /// load_env emits parent before child (child later = higher precedence).
    /// After filter_and_order_by_profiles the stream should be:
    ///   parent_base, parent_prod, child_base
    /// so that the later-wins resolver ends up with child_base for FOO.
    #[test]
    fn test_profiles_cross_file_child_base_wins_over_parent_profile() {
        let active = vec!["prod".to_string()];
        let parent = "/project/mise.toml";
        let child = "/project/sub/mise.toml";

        // Entries as emitted by load_env (parent block first, child block last):
        let input = vec![
            val_at("FOO", "parent_base", None, parent),
            val_at("FOO", "parent_prod", Some("prod"), parent),
            val_at("FOO", "child_base", None, child),
        ];

        let result = filter_and_order_by_profiles(input, &active);

        // Per-file reorder:
        //   parent block: [parent_base] ++ [parent_prod]
        //   child  block: [child_base]  ++ (no active profiles)
        // Full output: parent_base, parent_prod, child_base
        let pairs = kv(&result);
        assert_eq!(
            pairs,
            vec![
                ("FOO", "parent_base"),
                ("FOO", "parent_prod"),
                ("FOO", "child_base"),
            ],
            "child base must appear last so it wins under later-wins semantics"
        );
    }

    /// CROSS-FILE: a child-file active-profile directive must win over a parent-file
    /// base directive.
    ///
    /// Scenario (MISE_ENV=prod):
    ///   parent file block: [env] FOO=parent_base
    ///   child  file block: [env] FOO=child_base, [env.profiles.prod] FOO=child_prod
    ///
    /// Expected output order: parent_base, child_base, child_prod
    /// (child_prod is last → wins).
    #[test]
    fn test_profiles_cross_file_child_profile_wins_over_parent_base() {
        let active = vec!["prod".to_string()];
        let parent = "/project/mise.toml";
        let child = "/project/sub/mise.toml";

        let input = vec![
            val_at("FOO", "parent_base", None, parent),
            val_at("FOO", "child_base", None, child),
            val_at("FOO", "child_prod", Some("prod"), child),
        ];

        let result = filter_and_order_by_profiles(input, &active);

        // Per-file reorder:
        //   parent block: [parent_base]
        //   child  block: [child_base, child_prod]
        let pairs = kv(&result);
        assert_eq!(
            pairs,
            vec![
                ("FOO", "parent_base"),
                ("FOO", "child_base"),
                ("FOO", "child_prod"),
            ],
            "child active-profile must be last so it wins under later-wins semantics"
        );
    }

    /// CROSS-FILE: within-file base-before-profile and profile ordering by index
    /// must both hold even in a multi-file stream.
    #[test]
    fn test_profiles_cross_file_within_file_ordering_preserved() {
        let active = vec!["ci".to_string(), "prod".to_string()];
        let parent = "/project/mise.toml";
        let child = "/project/sub/mise.toml";

        // parent block: base + prod profile
        // child  block: ci profile + base  (interleaved to stress the reorder)
        let input = vec![
            val_at("A", "parent_base", None, parent),
            val_at("A", "parent_prod", Some("prod"), parent),
            val_at("A", "child_ci", Some("ci"), child),
            val_at("A", "child_base", None, child),
        ];

        let result = filter_and_order_by_profiles(input, &active);

        // Per-file reorder:
        //   parent block: [parent_base(base)] ++ [parent_prod(prod, idx=1)]
        //   child  block: [child_base(base)]  ++ [child_ci(ci, idx=0)]
        // Full stream: parent_base, parent_prod, child_base, child_ci
        let pairs = kv(&result);
        assert_eq!(
            pairs,
            vec![
                ("A", "parent_base"),
                ("A", "parent_prod"),
                ("A", "child_base"),
                ("A", "child_ci"),
            ]
        );
    }

    // ── FIX 4: non-contiguous input ──────────────────────────────────────────

    /// NON-CONTIGUOUS: a stream [A, B, A] (path A reappears after path B) must
    /// not panic and must group all entries for path A into A's block (anchored at
    /// first occurrence) and keep B after A.
    ///
    /// Input: A_base, B_base, A_prod  (A reappears after B)
    /// active_envs = ["prod"]
    /// Expected output: A_base, A_prod, B_base
    ///   — A's block is anchored at its first occurrence (position 0), B follows.
    #[test]
    fn test_profiles_non_contiguous_aba_grouped_at_first_occurrence() {
        let path_a = "/project/a.toml";
        let path_b = "/project/b.toml";
        let active = vec!["prod".to_string()];

        // Non-contiguous: A, B, A
        let input = vec![
            val_at("A_BASE", "a_base", None, path_a),
            val_at("B_BASE", "b_base", None, path_b),
            val_at("A_PROD", "a_prod", Some("prod"), path_a), // A reappears after B
        ];

        // Must not panic (no debug_assert contiguity guard)
        let result = filter_and_order_by_profiles(input, &active);

        // A's block (anchored at first occurrence): [A_BASE, A_PROD]
        // B's block: [B_BASE]
        // Full output: A_BASE, A_PROD, B_BASE
        assert_eq!(
            kv(&result),
            vec![
                ("A_BASE", "a_base"),
                ("A_PROD", "a_prod"),
                ("B_BASE", "b_base")
            ],
            "non-contiguous A entries must be grouped into A's block (anchored at first occurrence)"
        );
    }

    // ── FIX 8: platform auto-env profile activates when in active_envs ────────

    /// A profile whose name matches a platform auto-env name (e.g. "linux")
    /// must be kept and ordered correctly when that name appears in active_envs.
    /// This test is platform-independent because it passes active_envs explicitly.
    #[test]
    fn test_profiles_platform_autoenv_name_activates() {
        let active = vec!["linux".to_string()];
        let input = vec![
            val("BASE", "base_val", None),
            val("LINUX_VAR", "linux_val", Some("linux")), // matches platform auto-env name
            val("OTHER_VAR", "other_val", Some("macos")), // inactive platform name
        ];

        let result = filter_and_order_by_profiles(input, &active);

        // Only base + linux (active) should remain
        assert_eq!(
            kv(&result),
            vec![("BASE", "base_val"), ("LINUX_VAR", "linux_val")],
            "platform auto-env profile 'linux' must activate when present in active_envs"
        );
    }

    /// Multiple platform auto-env names in active_envs: ordering must follow
    /// their index position in active_envs, not profile name order.
    #[test]
    fn test_profiles_platform_autoenv_ordering_by_index() {
        // Simulate unix + macos + macos-aarch64 auto-envs
        let active = vec![
            "unix".to_string(),
            "macos".to_string(),
            "macos-aarch64".to_string(),
        ];
        let input = vec![
            val("VAR", "macos_aarch64_val", Some("macos-aarch64")), // index 2
            val("VAR", "unix_val", Some("unix")),                   // index 0
            val("VAR", "macos_val", Some("macos")),                 // index 1
        ];

        let result = filter_and_order_by_profiles(input, &active);

        // Ordered by active_envs index: unix(0), macos(1), macos-aarch64(2)
        assert_eq!(
            kv(&result),
            vec![
                ("VAR", "unix_val"),
                ("VAR", "macos_val"),
                ("VAR", "macos_aarch64_val"),
            ],
            "platform auto-env profiles must be ordered by their index in active_envs"
        );
    }
}
