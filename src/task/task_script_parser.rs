use crate::config::{Config, Settings};
use crate::env_diff::EnvMap;
use crate::exit::exit;
use crate::shell::ShellType;
use crate::task::Task;
use crate::tera::get_tera;
use eyre::{Context, Result};
use heck::ToSnakeCase;
use indexmap::IndexMap;
use itertools::Itertools;
use std::collections::{HashMap, HashSet};
use std::iter::once;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

type TeraSpecParsingResult = (
    tera::Tera,
    Arc<Mutex<HashMap<String, usize>>>,
    Arc<Mutex<Vec<usage::SpecArg>>>,
    Arc<Mutex<Vec<usage::SpecFlag>>>,
);

pub struct TaskScriptParser {
    dir: Option<PathBuf>,
    /// Extra vars to inject into the tera context (for monorepo task vars resolution)
    extra_vars: Option<IndexMap<String, String>>,
}

impl TaskScriptParser {
    pub fn new(dir: Option<PathBuf>) -> Self {
        TaskScriptParser {
            dir,
            extra_vars: None,
        }
    }

    pub fn with_extra_vars(mut self, vars: IndexMap<String, String>) -> Self {
        self.extra_vars = Some(vars);
        self
    }

    fn get_tera(&self) -> tera::Tera {
        get_tera(self.dir.as_deref())
    }

    /// Inject extra vars (from monorepo task config hierarchy) into the tera context
    fn inject_extra_vars(&self, tera_ctx: &mut tera::Context) {
        if let Some(extra_vars) = &self.extra_vars {
            // Merge extra_vars (base config-level vars from the config hierarchy) with any
            // vars already set in the context by task.tera_ctx() (which includes per-task
            // vars). Per-task vars take precedence over config-level vars.
            let existing: IndexMap<String, String> = tera_ctx
                .get("vars")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default();
            let mut merged = extra_vars.clone();
            merged.extend(existing);
            tera_ctx.insert("vars", &merged);
        }
    }

    fn render_script_with_context(
        tera: &mut tera::Tera,
        script: &str,
        ctx: &tera::Context,
    ) -> Result<String> {
        tera.render_str(script.trim(), ctx)
            .with_context(|| format!("Failed to render task script: {}", script))
    }

    fn render_usage_with_context(
        tera: &mut tera::Tera,
        usage: &str,
        ctx: &tera::Context,
    ) -> Result<String> {
        tera.render_str(usage.trim(), ctx)
            .with_context(|| format!("Failed to render task usage: {}", usage))
    }

    fn check_tera_args_deprecation(
        task_name: &str,
        args: &[usage::SpecArg],
        flags: &[usage::SpecFlag],
    ) {
        // Check if any args or flags were defined via Tera templates
        if args.is_empty() && flags.is_empty() {
            return;
        }

        deprecated_at!(
            "2026.5.0",
            "2027.5.0",
            "tera_template_task_args",
            "Task '{}' uses deprecated Tera template functions (arg(), option(), flag()) in run scripts. \
             Use the 'usage' field instead. See https://mise.jdx.dev/tasks/task-arguments.html",
            task_name
        );
    }

    // Helper functions for tera error handling
    fn expect_string(value: &tera::Value, param_name: &str) -> tera::Result<String> {
        value.as_str().map(|s| s.to_string()).ok_or_else(|| {
            tera::Error::msg(format!(
                "expected string for '{}', got {:?}",
                param_name, value
            ))
        })
    }

    fn expect_opt_string(
        value: Option<&tera::Value>,
        param_name: &str,
    ) -> tera::Result<Option<String>> {
        value
            .map(|v| Self::expect_string(v, param_name))
            .transpose()
    }

    fn expect_opt_bool(
        value: Option<&tera::Value>,
        param_name: &str,
    ) -> tera::Result<Option<bool>> {
        value.map(|v| Self::expect_bool(v, param_name)).transpose()
    }

    fn expect_bool(value: &tera::Value, param_name: &str) -> tera::Result<bool> {
        value.as_bool().ok_or_else(|| {
            tera::Error::msg(format!(
                "expected boolean for '{}', got {:?}",
                param_name, value
            ))
        })
    }

    fn expect_i64(value: &tera::Value, param_name: &str) -> tera::Result<i64> {
        value.as_i64().ok_or_else(|| {
            tera::Error::msg(format!(
                "expected integer for '{}', got {:?}",
                param_name, value
            ))
        })
    }

    fn expect_opt_i64(value: Option<&tera::Value>, param_name: &str) -> tera::Result<Option<i64>> {
        value.map(|v| Self::expect_i64(v, param_name)).transpose()
    }

    fn expect_array<'a>(
        value: &'a tera::Value,
        param_name: &str,
    ) -> tera::Result<&'a Vec<tera::Value>> {
        value.as_array().ok_or_else(|| {
            tera::Error::msg(format!(
                "expected array for '{}', got {:?}",
                param_name, value
            ))
        })
    }

    fn expect_opt_array<'a>(
        value: Option<&'a tera::Value>,
        param_name: &str,
    ) -> tera::Result<Option<&'a Vec<tera::Value>>> {
        value.map(|v| Self::expect_array(v, param_name)).transpose()
    }

    fn lock_error(e: impl std::fmt::Display) -> tera::Error {
        tera::Error::msg(format!("failed to lock: {}", e))
    }

    fn setup_tera_for_spec_parsing(&self, task: &Task) -> TeraSpecParsingResult {
        let mut tera = self.get_tera();
        let arg_order = Arc::new(Mutex::new(HashMap::new()));
        let input_args = Arc::new(Mutex::new(vec![]));
        let input_flags = Arc::new(Mutex::new(vec![]));
        // override throw function to do nothing
        tera.register_function("throw", {
            move |_args: &HashMap<String, tera::Value>| -> tera::Result<tera::Value> {
                Ok(tera::Value::Null)
            }
        });
        // render args, options, and flags as null
        // these functions are only used to collect the spec
        tera.register_function("arg", {
            let input_args = input_args.clone();
            let arg_order = arg_order.clone();
            move |args: &HashMap<String, tera::Value>| -> tera::Result<tera::Value> {
                let i = Self::expect_i64(
                    args.get("i").unwrap_or(&tera::Value::from(
                        input_args.lock().map_err(Self::lock_error)?.len(),
                    )),
                    "i",
                )? as usize;

                let required =
                    Self::expect_opt_bool(args.get("required"), "required")?.unwrap_or(true);
                let var = Self::expect_opt_bool(args.get("var"), "var")?.unwrap_or(false);
                let name =
                    Self::expect_opt_string(args.get("name"), "name")?.unwrap_or(i.to_string());

                let mut arg_order = arg_order.lock().map_err(Self::lock_error)?;

                if arg_order.contains_key(&name) {
                    trace!("already seen {name}");
                    return Ok(tera::Value::String("".to_string()));
                }
                arg_order.insert(name.clone(), i);

                let usage =
                    Self::expect_opt_string(args.get("usage"), "usage")?.unwrap_or_default();
                let help = Self::expect_opt_string(args.get("help"), "help")?;
                let help_long = Self::expect_opt_string(args.get("help_long"), "help_long")?;
                let help_md = Self::expect_opt_string(args.get("help_md"), "help_md")?;

                let var_min =
                    Self::expect_opt_i64(args.get("var_min"), "var_min")?.map(|v| v as usize);
                let var_max =
                    Self::expect_opt_i64(args.get("var_max"), "var_max")?.map(|v| v as usize);

                let hide = Self::expect_opt_bool(args.get("hide"), "hide")?.unwrap_or(false);

                let default = Self::expect_opt_string(args.get("default"), "default")?
                    .map(|s| vec![s])
                    .unwrap_or_default();

                let choices = Self::expect_opt_array(args.get("choices"), "choices")?
                    .map(|array| {
                        tera::Result::Ok(usage::SpecChoices {
                            choices: array
                                .iter()
                                .map(|v| Self::expect_string(v, "choice"))
                                .collect::<Result<Vec<String>, tera::Error>>()?,
                        })
                    })
                    .transpose()?;

                let env = Self::expect_opt_string(args.get("env"), "env")?;

                let help_first_line = match &help {
                    Some(h) => {
                        if h.is_empty() {
                            None
                        } else {
                            h.lines().next().map(|line| line.to_string())
                        }
                    }
                    None => None,
                };

                let mut arg = usage::SpecArg {
                    name: name.clone(),
                    usage,
                    help_first_line,
                    help,
                    help_long,
                    help_md,
                    required,
                    var,
                    var_min,
                    var_max,
                    hide,
                    default,
                    choices,
                    env,
                    ..Default::default()
                };
                arg.usage = arg.usage();

                input_args.lock().map_err(Self::lock_error)?.push(arg);
                Ok(tera::Value::String("".to_string()))
            }
        });

        tera.register_function("option", {
            let input_flags = input_flags.clone();
            move |args: &HashMap<String, tera::Value>| -> tera::Result<tera::Value> {
                let name = match args.get("name") {
                    Some(n) => Self::expect_string(n, "name")?,
                    None => return Err(tera::Error::msg("missing required 'name' parameter")),
                };

                let short = args
                    .get("short")
                    .map(|s| s.to_string().chars().collect())
                    .unwrap_or_default();

                let long = match args.get("long") {
                    Some(l) => {
                        let s = Self::expect_string(l, "long")?;
                        s.split_whitespace().map(|s| s.to_string()).collect()
                    }
                    None => vec![name.clone()],
                };

                let default = Self::expect_opt_string(args.get("default"), "default")?
                    .map(|s| vec![s])
                    .unwrap_or_default();

                let var = Self::expect_opt_bool(args.get("var"), "var")?.unwrap_or(false);

                let deprecated = Self::expect_opt_string(args.get("deprecated"), "deprecated")?;
                let help = Self::expect_opt_string(args.get("help"), "help")?;
                let help_long = Self::expect_opt_string(args.get("help_long"), "help_long")?;
                let help_md = Self::expect_opt_string(args.get("help_md"), "help_md")?;

                let hide = Self::expect_opt_bool(args.get("hide"), "hide")?.unwrap_or(false);

                let global = Self::expect_opt_bool(args.get("global"), "global")?.unwrap_or(false);

                let count = Self::expect_opt_bool(args.get("count"), "count")?.unwrap_or(false);

                let usage =
                    Self::expect_opt_string(args.get("usage"), "usage")?.unwrap_or_default();

                let required =
                    Self::expect_opt_bool(args.get("required"), "required")?.unwrap_or(false);

                let negate = Self::expect_opt_string(args.get("negate"), "negate")?;

                let choices = match args.get("choices") {
                    Some(c) => {
                        let array = Self::expect_array(c, "choices")?;
                        let mut choices_vec = Vec::new();
                        for choice in array {
                            let s = Self::expect_string(choice, "choice")?;
                            choices_vec.push(s);
                        }
                        Some(usage::SpecChoices {
                            choices: choices_vec,
                        })
                    }
                    None => None,
                };

                let env = Self::expect_opt_string(args.get("env"), "env")?;

                let help_first_line = match &help {
                    Some(h) => {
                        if h.is_empty() {
                            None
                        } else {
                            h.lines().next().map(|line| line.to_string())
                        }
                    }
                    None => None,
                };

                let mut flag = usage::SpecFlag {
                    name: name.clone(),
                    short,
                    long,
                    default,
                    var,
                    var_min: None,
                    var_max: None,
                    hide,
                    global,
                    count,
                    deprecated,
                    help_first_line,
                    help,
                    usage,
                    help_long,
                    help_md,
                    required,
                    negate,
                    env: env.clone(),
                    arg: Some(usage::SpecArg {
                        name: name.clone(),
                        var,
                        choices,
                        env,
                        ..Default::default()
                    }),
                };
                flag.usage = flag.usage();

                input_flags.lock().map_err(Self::lock_error)?.push(flag);

                Ok(tera::Value::String("".to_string()))
            }
        });

        tera.register_function("flag", {
            let input_flags = input_flags.clone();
            move |args: &HashMap<String, tera::Value>| -> tera::Result<tera::Value> {
                let name = match args.get("name") {
                    Some(n) => Self::expect_string(n, "name")?,
                    None => return Err(tera::Error::msg("missing required 'name' parameter")),
                };

                let short = args
                    .get("short")
                    .map(|s| s.to_string().chars().collect())
                    .unwrap_or_default();

                let long = match args.get("long") {
                    Some(l) => {
                        let s = Self::expect_string(l, "long")?;
                        s.split_whitespace().map(|s| s.to_string()).collect()
                    }
                    None => vec![name.clone()],
                };

                let default = Self::expect_opt_string(args.get("default"), "default")?
                    .map(|s| vec![s])
                    .unwrap_or_default();

                let var = Self::expect_opt_bool(args.get("var"), "var")?.unwrap_or(false);

                let deprecated = Self::expect_opt_string(args.get("deprecated"), "deprecated")?;
                let help = Self::expect_opt_string(args.get("help"), "help")?;
                let help_long = Self::expect_opt_string(args.get("help_long"), "help_long")?;
                let help_md = Self::expect_opt_string(args.get("help_md"), "help_md")?;

                let hide = Self::expect_opt_bool(args.get("hide"), "hide")?.unwrap_or(false);

                let global = Self::expect_opt_bool(args.get("global"), "global")?.unwrap_or(false);

                let count = Self::expect_opt_bool(args.get("count"), "count")?.unwrap_or(false);

                let usage =
                    Self::expect_opt_string(args.get("usage"), "usage")?.unwrap_or_default();

                let required =
                    Self::expect_opt_bool(args.get("required"), "required")?.unwrap_or(false);

                let negate = Self::expect_opt_string(args.get("negate"), "negate")?;

                let choices = match args.get("choices") {
                    Some(c) => {
                        let array = Self::expect_array(c, "choices")?;
                        let mut choices_vec = Vec::new();
                        for choice in array {
                            let s = Self::expect_string(choice, "choice")?;
                            choices_vec.push(s);
                        }
                        Some(usage::SpecChoices {
                            choices: choices_vec,
                        })
                    }
                    None => None,
                };

                let env = Self::expect_opt_string(args.get("env"), "env")?;

                let help_first_line = match &help {
                    Some(h) => {
                        if h.is_empty() {
                            None
                        } else {
                            h.lines().next().map(|line| line.to_string())
                        }
                    }
                    None => None,
                };

                // Create SpecArg when any arg-level properties are set (choices, env)
                // This matches the behavior of option() which always creates SpecArg
                let arg = if choices.is_some() || env.is_some() {
                    Some(usage::SpecArg {
                        name: name.clone(),
                        var,
                        choices,
                        env: env.clone(),
                        ..Default::default()
                    })
                } else {
                    None
                };

                let mut flag = usage::SpecFlag {
                    name: name.clone(),
                    short,
                    long,
                    default,
                    var,
                    var_min: None,
                    var_max: None,
                    hide,
                    global,
                    count,
                    deprecated,
                    help_first_line,
                    help,
                    usage,
                    help_long,
                    help_md,
                    required,
                    negate,
                    env,
                    arg,
                };
                flag.usage = flag.usage();

                input_flags.lock().map_err(Self::lock_error)?.push(flag);

                Ok(tera::Value::String("".to_string()))
            }
        });

        tera.register_function("task_source_files", {
            let sources = Arc::new(task.sources.clone());

            move |_: &HashMap<String, tera::Value>| -> tera::Result<tera::Value> {
               if sources.is_empty() {
                   trace!("tera::render::resolve_task_sources `task_source_files` called in task with empty sources array");
                   return Ok(tera::Value::Array(Default::default()));
               };

                let mut resolved = Vec::with_capacity(sources.len());

                for pattern in sources.iter() {
                    // pattern is considered a tera template string if it contains opening tags:
                    // - "{#" for comments
                    // - "{{" for expressions
                    // - "{%" for statements
                    if pattern.contains("{#") || pattern.contains("{{") || pattern.contains("{%") {
                        trace!(
                            "tera::render::resolve_task_sources including tera template string in resolved task sources: {pattern}"
                        );
                        resolved.push(tera::Value::String(pattern.clone()));
                        continue;
                    }

                    match glob::glob_with(
                        pattern,
                        glob::MatchOptions {
                            case_sensitive: false,
                            require_literal_separator: false,
                            require_literal_leading_dot: false,
                        },
                    ) {
                        Err(error) => {
                            warn!(
                                "tera::render::resolve_task_sources including '{pattern}' in resolved task sources, ignoring glob parsing error: {error:#?}"
                            );
                            resolved.push(tera::Value::String(pattern.clone()));
                        }
                        Ok(expanded) => {
                            let mut source_found = false;

                            for path in expanded {
                                source_found = true;

                                match path {
                                    Ok(path) => {
                                        let source = path.display();
                                        trace!(
                                            "tera::render::resolve_task_sources resolved source from pattern '{pattern}': {source}"
                                        );
                                        resolved.push(tera::Value::String(source.to_string()));
                                    }
                                    Err(error) => {
                                        let source = error.path().display();
                                        warn!(
                                            "tera::render::resolve_task_sources omitting '{source}' from resolved task sources due to: {:#?}",
                                            error.error()
                                        );
                                    }
                                }
                            }

                            if !source_found {
                                warn!(
                                    "tera::render::resolve_task_sources no source file(s) resolved for pattern: '{pattern}'"
                                );
                            }
                        }
                    }
                }

                Ok(tera::Value::Array(resolved))
            }
        });

        (tera, arg_order, input_args, input_flags)
    }

    pub async fn parse_run_scripts_for_spec_only(
        &self,
        config: &Arc<Config>,
        task: &Task,
        scripts: &[String],
    ) -> Result<usage::Spec> {
        let (mut tera, arg_order, input_args, input_flags) = self.setup_tera_for_spec_parsing(task);
        let mut tera_ctx = task.tera_ctx(config).await?;
        // First render the usage field to collect the spec
        let rendered_usage = Self::render_usage_with_context(&mut tera, &task.usage, &tera_ctx)?;
        let spec_from_field: usage::Spec = rendered_usage.parse()?;

        if Settings::get().task.disable_spec_from_run_scripts {
            return Ok(spec_from_field);
        }

        // Make the arg/flag names available as snake_case in the template context, using
        // default values from the spec (or sensible fallbacks when no default is provided).
        let usage_ctx = Self::make_usage_ctx_from_spec_defaults(&spec_from_field);
        tera_ctx.insert("usage", &usage_ctx);

        // Don't insert env for spec-only parsing to avoid expensive environment rendering
        // Render scripts to trigger spec collection via Tera template functions
        // (arg/option/flag), but discard the results. Ignore rendering errors since we only
        // care about collecting arg/flag definitions from the deprecated Tera syntax.
        for script in scripts {
            let _ = Self::render_script_with_context(&mut tera, script, &tera_ctx);
        }
        let mut cmd = usage::SpecCommand::default();
        // TODO: ensure no gaps in args, e.g.: 1,2,3,4,5
        let arg_order = arg_order.lock().unwrap();
        cmd.args = input_args
            .lock()
            .unwrap()
            .iter()
            .cloned()
            .sorted_by_key(|arg| {
                arg_order
                    .get(&arg.name)
                    .unwrap_or_else(|| panic!("missing arg order for {}", arg.name.as_str()))
            })
            .collect();
        cmd.flags = input_flags.lock().unwrap().clone();

        // Check for deprecated Tera template args usage
        Self::check_tera_args_deprecation(&task.name, &cmd.args, &cmd.flags);

        let mut spec = usage::Spec {
            cmd,
            ..Default::default()
        };
        spec.merge(spec_from_field);

        Ok(spec)
    }

    pub async fn parse_run_scripts(
        &self,
        config: &Arc<Config>,
        task: &Task,
        scripts: &[String],
        env: &EnvMap,
    ) -> Result<(Vec<String>, usage::Spec)> {
        let (mut tera, arg_order, input_args, input_flags) = self.setup_tera_for_spec_parsing(task);
        let mut tera_ctx = task.tera_ctx(config).await?;
        self.inject_extra_vars(&mut tera_ctx);
        tera_ctx.insert("env", &env);
        // First render the usage field to collect the spec and build a default
        // usage map, so that `{{ usage.* }}` references in run scripts do not
        // fail during this initial parsing phase (e.g. for inline tasks).
        let rendered_usage = Self::render_usage_with_context(&mut tera, &task.usage, &tera_ctx)?;
        let spec_from_field: usage::Spec = rendered_usage.parse()?;
        let usage_ctx = Self::make_usage_ctx_from_spec_defaults(&spec_from_field);
        tera_ctx.insert("usage", &usage_ctx);

        let scripts = scripts
            .iter()
            .map(|s| Self::render_script_with_context(&mut tera, s, &tera_ctx))
            .collect::<Result<Vec<String>>>()?;
        let mut cmd = usage::SpecCommand::default();
        // TODO: ensure no gaps in args, e.g.: 1,2,3,4,5
        let arg_order = arg_order.lock().unwrap();
        cmd.args = input_args
            .lock()
            .unwrap()
            .iter()
            .cloned()
            .sorted_by_key(|arg| {
                arg_order
                    .get(&arg.name)
                    .unwrap_or_else(|| panic!("missing arg order for {}", arg.name.as_str()))
            })
            .collect();
        cmd.flags = input_flags.lock().unwrap().clone();

        // Check for deprecated Tera template args usage
        Self::check_tera_args_deprecation(&task.name, &cmd.args, &cmd.flags);
        let mut spec = usage::Spec {
            cmd,
            ..Default::default()
        };
        spec.merge(spec_from_field);

        Ok((scripts, spec))
    }

    pub async fn parse_run_scripts_with_args(
        &self,
        config: &Arc<Config>,
        task: &Task,
        scripts: &[String],
        env: &EnvMap,
        args: &[String],
        spec: &usage::Spec,
    ) -> Result<Vec<String>> {
        let args = vec!["".to_string()]
            .into_iter()
            .chain(args.iter().cloned())
            .collect::<Vec<_>>();
        // Pass env vars to Parser so it can resolve env= defaults in usage specs
        // This is needed for monorepo tasks where child config env vars aren't in the process env
        let env_map: std::collections::HashMap<String, String> =
            env.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        let m = match usage::Parser::new(spec).with_env(env_map).parse(&args) {
            Ok(m) => m,
            Err(e) => {
                // just print exactly what usage returns so the error output isn't double-wrapped
                // this could be displaying help or a parse error
                eprintln!("{}", format!("{e}").trim_end());
                exit(1);
            }
        };

        let mut out: Vec<String> = vec![];
        for script in scripts {
            let shell_type = shell_from_shebang(script)
                .or(task.shell())
                .unwrap_or(Settings::get().default_inline_shell()?)[0]
                .parse()
                .ok();
            let escape = {
                move |v: &usage::parse::ParseValue| match v {
                    usage::parse::ParseValue::MultiString(_) => {
                        // these are already escaped
                        v.to_string()
                    }
                    _ => match shell_type {
                        Some(ShellType::Zsh | ShellType::Bash | ShellType::Fish) => {
                            shell_words::quote(&v.to_string()).to_string()
                        }
                        _ => v.to_string(),
                    },
                }
            };
            let mut tera = self.get_tera();
            tera.register_function("arg", {
                {
                    let usage_args = m.args.clone();
                    move |args: &HashMap<String, tera::Value>| -> tera::Result<tera::Value> {
                        let seen_args = Arc::new(Mutex::new(HashSet::new()));
                        {
                            let mut seen_args = seen_args.lock().unwrap();
                            let i = args
                                .get("i")
                                .map(|i| i.as_i64().unwrap() as usize)
                                .unwrap_or_else(|| seen_args.len());
                            let name = args
                                .get("name")
                                .map(|n| n.as_str().unwrap().to_string())
                                .unwrap_or(i.to_string());
                            seen_args.insert(name.clone());
                            Ok(tera::Value::String(
                                usage_args
                                    .iter()
                                    .find(|(arg, _)| arg.name == name)
                                    .map(|(_, value)| escape(value))
                                    .unwrap_or("".to_string()),
                            ))
                        }
                    }
                }
            });
            let flag_func = {
                |default_value: String| {
                    let usage_flags = m.flags.clone();
                    move |args: &HashMap<String, tera::Value>| -> tera::Result<tera::Value> {
                        let name = args
                            .get("name")
                            .map(|n| n.as_str().unwrap().to_string())
                            .unwrap();
                        Ok(tera::Value::String(
                            usage_flags
                                .iter()
                                .find(|(flag, _)| flag.name == name)
                                .map(|(_, value)| escape(value))
                                .unwrap_or(default_value.clone()),
                        ))
                    }
                }
            };
            tera.register_function("option", flag_func("".to_string()));
            tera.register_function("flag", flag_func(false.to_string()));
            let mut tera_ctx = task.tera_ctx(config).await?;
            self.inject_extra_vars(&mut tera_ctx);
            tera_ctx.insert("env", &env);
            let mut usage_map = Self::make_usage_ctx_from_spec_defaults(spec);
            usage_map.extend(Self::make_usage_ctx(&m));
            tera_ctx.insert("usage", &usage_map);
            out.push(Self::render_script_with_context(
                &mut tera, script, &tera_ctx,
            )?);
        }
        Ok(out)
    }

    fn make_usage_ctx(usage: &usage::parse::ParseOutput) -> HashMap<String, tera::Value> {
        let mut usage_ctx: HashMap<String, tera::Value> = HashMap::new();

        // These values are not escaped or shell-quoted.
        let to_tera_value = |val: &usage::parse::ParseValue| -> tera::Value {
            use tera::Value;
            use usage::parse::ParseValue::*;
            match val {
                MultiBool(v) => Value::Number(serde_json::Number::from(v.len())),
                MultiString(v) => {
                    Value::Array(v.iter().map(|s| Value::String(s.clone())).collect())
                }
                Bool(v) => Value::Bool(*v),
                String(v) => Value::String(v.clone()),
            }
        };

        // The names are converted to snake_case (hyphens become underscores).
        // For example, a flag like "--dry-run" becomes accessible as {{ usage.dry_run }}.
        for (arg, val) in &usage.args {
            let tera_val = to_tera_value(val);
            usage_ctx.insert(arg.name.to_snake_case(), tera_val);
        }
        for (flag, val) in &usage.flags {
            let tera_val = to_tera_value(val);
            usage_ctx.insert(flag.name.to_snake_case(), tera_val);
        }

        // expose selected subcommand as {{ usage.cmd }}
        if let Some(subcmd) = subcommand_name_from_parse(&usage.cmds) {
            usage_ctx.insert("cmd".to_string(), tera::Value::String(subcmd));
        }

        usage_ctx
    }

    /// Build a usage context hashmap from a `usage::Spec` using default values
    /// or sensible fallbacks. Recurses into subcommands so that `{{ usage.X }}`
    /// references don't error during the initial template render (which is only
    /// used for deprecated spec collection — actual execution re-renders via
    /// `parse_run_scripts_with_args` with real parsed values).
    pub fn make_usage_ctx_from_spec_defaults(spec: &usage::Spec) -> HashMap<String, tera::Value> {
        let mut usage_ctx: HashMap<String, tera::Value> = HashMap::new();

        fn collect_cmd_defaults(cmd: &usage::SpecCommand, ctx: &mut HashMap<String, tera::Value>) {
            for arg in &cmd.args {
                let name = arg.name.to_snake_case();
                if ctx.contains_key(&name) {
                    continue;
                }
                let value = if arg.var {
                    let defaults: Vec<tera::Value> = arg
                        .default
                        .iter()
                        .map(|s| tera::Value::String(s.clone()))
                        .collect();
                    tera::Value::Array(defaults)
                } else if let Some(default) = arg.default.first() {
                    tera::Value::String(default.clone())
                } else {
                    tera::Value::String(String::new())
                };
                ctx.insert(name, value);
            }

            for flag in &cmd.flags {
                let name = flag.name.to_snake_case();
                if ctx.contains_key(&name) {
                    continue;
                }
                let value = if flag.var {
                    let defaults: Vec<tera::Value> = flag
                        .default
                        .iter()
                        .map(|s| tera::Value::String(s.clone()))
                        .collect();
                    tera::Value::Array(defaults)
                } else if flag.count {
                    tera::Value::Number(serde_json::Number::from(0))
                } else if let Some(default) = flag.default.first() {
                    default
                        .parse::<bool>()
                        .map_or_else(|_| tera::Value::String(default.clone()), tera::Value::Bool)
                } else {
                    tera::Value::Bool(false)
                };
                ctx.insert(name, value);
            }

            // SpecCommand::subcommands is an IndexMap, so iteration order is deterministic
            for subcmd in cmd.subcommands.values() {
                collect_cmd_defaults(subcmd, ctx);
            }
        }

        collect_cmd_defaults(&spec.cmd, &mut usage_ctx);

        if !spec.cmd.subcommands.is_empty() {
            usage_ctx.insert("cmd".to_string(), tera::Value::String(String::new()));
        }

        usage_ctx
    }
}

pub fn has_any_args_defined(spec: &usage::Spec) -> bool {
    !spec.cmd.args.is_empty() || !spec.cmd.flags.is_empty() || !spec.cmd.subcommands.is_empty()
}

/// Check if the spec has any usage directives at all (args, flags, subcommands, or metadata
/// like long_about/before_help/after_help). Used to decide whether to show usage-based help
/// vs the generic task help.
///
/// Note: before_help_long is excluded because populate_spec_metadata()
/// sets it automatically for tasks with dependencies.
pub fn has_any_usage_spec(spec: &usage::Spec) -> bool {
    has_any_args_defined(spec)
        || spec.about.is_some()
        || spec.about_long.is_some()
        || spec.about_md.is_some()
        || spec.cmd.help_long.is_some()
        || spec.cmd.help_md.is_some()
        || spec.cmd.before_help.is_some()
        || spec.cmd.after_help.is_some()
        || spec.cmd.after_help_long.is_some()
        || !spec.cmd.examples.is_empty()
}

/// Extract the selected subcommand name from parsed commands.
/// `cmds[0]` is the root command; subsequent entries are subcommands.
pub fn subcommand_name_from_parse(cmds: &[usage::SpecCommand]) -> Option<String> {
    if cmds.len() > 1 {
        let names: Vec<String> = cmds.iter().skip(1).map(|c| c.name.clone()).collect();
        Some(names.join(" "))
    } else {
        None
    }
}

fn shell_from_shebang(script: &str) -> Option<Vec<String>> {
    let shebang = script.lines().next()?.strip_prefix("#!")?;
    let shebang = shebang.strip_prefix("/usr/bin/env -S").unwrap_or(shebang);
    let shebang = shebang.strip_prefix("/usr/bin/env").unwrap_or(shebang);
    let mut parts = shebang.split_whitespace();
    let shell = parts.next()?;
    let args = parts.map(|s| s.to_string()).collect_vec();
    Some(once(shell.to_string()).chain(args).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[tokio::test]
    async fn test_task_parse_arg() {
        let config = Config::get().await.unwrap();
        let task = Task::default();
        let parser = TaskScriptParser::new(None);
        let scripts = vec!["echo {{ arg(i=0, name='foo') }}".to_string()];
        let (parsed_scripts, spec) = parser
            .parse_run_scripts(&config, &task, &scripts, &Default::default())
            .await
            .unwrap();
        assert_eq!(parsed_scripts, vec!["echo "]);
        let arg0 = spec.cmd.args.first().unwrap();
        assert_eq!(arg0.name, "foo");

        let parsed_scripts = parser
            .parse_run_scripts_with_args(
                &config,
                &task,
                &scripts,
                &Default::default(),
                &["abc".to_string()],
                &spec,
            )
            .await
            .unwrap();
        assert_eq!(parsed_scripts, vec!["echo abc"]);
    }

    #[tokio::test]
    async fn test_task_parse_multi_use_arg() {
        let config = Config::get().await.unwrap();
        let task = Task::default();
        let parser = TaskScriptParser::new(None);
        let scripts = vec![
            "echo {{ arg(name='foo') }}; echo {{ arg(name='bar') }}; echo {{ arg(name='foo') }}"
                .to_string(),
        ];
        let (parsed_scripts, spec) = parser
            .parse_run_scripts(&config, &task, &scripts, &Default::default())
            .await
            .unwrap();
        assert_eq!(parsed_scripts, vec!["echo ; echo ; echo "]);
        let arg0 = spec.cmd.args.first().unwrap();
        let arg1 = spec.cmd.args.get(1).unwrap();
        assert_eq!(arg0.name, "foo");
        assert_eq!(arg1.name, "bar");
        assert_eq!(spec.cmd.args.len(), 2);

        let parsed_scripts = parser
            .parse_run_scripts_with_args(
                &config,
                &task,
                &scripts,
                &Default::default(),
                &["abc".to_string(), "def".to_string()],
                &spec,
            )
            .await
            .unwrap();
        assert_eq!(parsed_scripts, vec!["echo abc; echo def; echo abc"]);
    }

    #[tokio::test]
    async fn test_task_parse_arg_var() {
        let config = Config::get().await.unwrap();
        let task = Task::default();
        let parser = TaskScriptParser::new(None);
        let scripts = vec!["echo {{ arg(var=true) }}".to_string()];
        let (parsed_scripts, spec) = parser
            .parse_run_scripts(&config, &task, &scripts, &Default::default())
            .await
            .unwrap();
        assert_eq!(parsed_scripts, vec!["echo "]);
        let arg0 = spec.cmd.args.first().unwrap();
        assert_eq!(arg0.name, "0");

        let parsed_scripts = parser
            .parse_run_scripts_with_args(
                &config,
                &task,
                &scripts,
                &Default::default(),
                &["abc".to_string(), "def".to_string()],
                &spec,
            )
            .await
            .unwrap();
        assert_eq!(parsed_scripts, vec!["echo abc def"]);
    }

    #[tokio::test]
    async fn test_task_parse_flag() {
        let config = Config::get().await.unwrap();
        let task = Task::default();
        let parser = TaskScriptParser::new(None);
        let scripts = vec!["echo {{ flag(name='foo') }}".to_string()];
        let (parsed_scripts, spec) = parser
            .parse_run_scripts(&config, &task, &scripts, &Default::default())
            .await
            .unwrap();
        assert_eq!(parsed_scripts, vec!["echo "]);
        let flag = spec.cmd.flags.iter().find(|f| &f.name == "foo").unwrap();
        assert_eq!(&flag.name, "foo");

        let parsed_scripts = parser
            .parse_run_scripts_with_args(
                &config,
                &task,
                &scripts,
                &Default::default(),
                &["--foo".to_string()],
                &spec,
            )
            .await
            .unwrap();
        assert_eq!(parsed_scripts, vec!["echo true"]);

        let scripts = vec!["echo {{ flag(name='foo') }}".to_string()];
        let (parsed_scripts, spec) = parser
            .parse_run_scripts(&config, &task, &scripts, &Default::default())
            .await
            .unwrap();
        assert_eq!(parsed_scripts, vec!["echo "]);
        let parsed_scripts = parser
            .parse_run_scripts_with_args(&config, &task, &scripts, &Default::default(), &[], &spec)
            .await
            .unwrap();
        assert_eq!(parsed_scripts, vec!["echo false"]);
    }

    #[tokio::test]
    async fn test_task_parse_option() {
        let config = Config::get().await.unwrap();
        let task = Task::default();
        let parser = TaskScriptParser::new(None);
        let scripts = vec!["echo {{ option(name='foo') }}".to_string()];
        let (parsed_scripts, spec) = parser
            .parse_run_scripts(&config, &task, &scripts, &Default::default())
            .await
            .unwrap();
        assert_eq!(parsed_scripts, vec!["echo "]);
        let option = spec.cmd.flags.iter().find(|f| &f.name == "foo").unwrap();
        assert_eq!(&option.name, "foo");

        let parsed_scripts = parser
            .parse_run_scripts_with_args(
                &config,
                &task,
                &scripts,
                &Default::default(),
                &["--foo".to_string(), "abc".to_string()],
                &spec,
            )
            .await
            .unwrap();
        assert_eq!(parsed_scripts, vec!["echo abc"]);

        let parsed_scripts = parser
            .parse_run_scripts_with_args(&config, &task, &scripts, &Default::default(), &[], &spec)
            .await
            .unwrap();
        assert_eq!(parsed_scripts, vec!["echo "]);
    }

    #[tokio::test]
    async fn test_task_nested_template() {
        let config = Config::get().await.unwrap();
        let task = Task::default();
        let parser = TaskScriptParser::new(None);
        let scripts =
            vec!["echo {% if flag(name=env.FLAG_NAME) == 'true' %}TRUE{% endif %}".to_string()];
        let env = EnvMap::from_iter(vec![("FLAG_NAME".to_string(), "foo".to_string())]);
        let (parsed_scripts, spec) = parser
            .parse_run_scripts(&config, &task, &scripts, &env)
            .await
            .unwrap();
        assert_eq!(parsed_scripts, vec!["echo "]);
        let flag = spec.cmd.flags.first().unwrap();
        assert_eq!(&flag.name, "foo");

        let parsed_scripts = parser
            .parse_run_scripts_with_args(
                &config,
                &task,
                &scripts,
                &env,
                &["--foo".to_string()],
                &spec,
            )
            .await
            .unwrap();
        assert_eq!(parsed_scripts, vec!["echo TRUE"]);
    }

    #[tokio::test]
    async fn test_task_parse_empty_help() {
        let config = Config::get().await.unwrap();
        let task = Task::default();
        let parser = TaskScriptParser::new(None);

        // Test with empty help string for arg
        let scripts = vec!["echo {{ arg(name='foo', help='') }}".to_string()];
        let (parsed_scripts, spec) = parser
            .parse_run_scripts(&config, &task, &scripts, &Default::default())
            .await
            .unwrap();
        assert_eq!(parsed_scripts, vec!["echo "]);
        let arg = spec.cmd.args.first().unwrap();
        assert_eq!(arg.name, "foo");
        assert_eq!(arg.help, Some("".to_string()));
        assert_eq!(arg.help_first_line, None);

        // Test with empty help string for option
        let scripts = vec!["echo {{ option(name='bar', help='') }}".to_string()];
        let (parsed_scripts, spec) = parser
            .parse_run_scripts(&config, &task, &scripts, &Default::default())
            .await
            .unwrap();
        assert_eq!(parsed_scripts, vec!["echo "]);
        let option = spec.cmd.flags.iter().find(|f| &f.name == "bar").unwrap();
        assert_eq!(&option.name, "bar");
        assert_eq!(option.help, Some("".to_string()));
        assert_eq!(option.help_first_line, None);

        // Test with empty help string for flag
        let scripts = vec!["echo {{ flag(name='baz', help='') }}".to_string()];
        let (parsed_scripts, spec) = parser
            .parse_run_scripts(&config, &task, &scripts, &Default::default())
            .await
            .unwrap();
        assert_eq!(parsed_scripts, vec!["echo "]);
        let flag = spec.cmd.flags.iter().find(|f| &f.name == "baz").unwrap();
        assert_eq!(&flag.name, "baz");
        assert_eq!(flag.help, Some("".to_string()));
        assert_eq!(flag.help_first_line, None);
    }

    #[tokio::test]
    async fn test_task_parse_option_env() {
        let config = Config::get().await.unwrap();
        let task = Task::default();
        let parser = TaskScriptParser::new(None);
        let scripts = vec!["echo {{ option(name='profile', env='BUILD_PROFILE') }}".to_string()];
        let (parsed_scripts, spec) = parser
            .parse_run_scripts(&config, &task, &scripts, &Default::default())
            .await
            .unwrap();
        assert_eq!(parsed_scripts, vec!["echo "]);
        let option = spec
            .cmd
            .flags
            .iter()
            .find(|f| &f.name == "profile")
            .unwrap();
        assert_eq!(&option.name, "profile");
        assert_eq!(option.env, Some("BUILD_PROFILE".to_string()));
        // Verify the nested SpecArg also has the env field set
        let arg = option.arg.as_ref().unwrap();
        assert_eq!(arg.env, Some("BUILD_PROFILE".to_string()));
    }

    #[tokio::test]
    async fn test_task_parse_task_source_files() {
        let cases: &[(&[&str], &str, &str)] = &[
            (&[], "echo {{ task_source_files() }}", "echo []"),
            (
                &["**/filetask"],
                "echo {{ task_source_files() | first }}",
                "echo .mise/tasks/filetask", // created by constructor in `src/test.rs`, guaranteed to exist
            ),
            (
                &["nonexistent/*.xyz"],
                "echo {{ task_source_files() }}",
                "echo []",
            ),
            (
                &["../../Cargo.toml"],
                "echo {{ task_source_files() | first }}",
                "echo ../../Cargo.toml",
            ),
            (
                &[concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml")],
                "echo {{ task_source_files() | first }}",
                concat!("echo ", env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"),
            ),
            #[cfg(not(windows))] // TODO: this cases panics on windows currently
            (
                &["{{ env.HOME }}/file.txt", "src/*.rs"],
                "echo {{ task_source_files() | first }}",
                "echo {{ env.HOME }}/file.txt",
            ),
            (
                &["[invalid"],
                "echo {{ task_source_files() | first }}",
                "echo [invalid",
            ),
            (
                &[
                    concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"),
                    concat!(env!("CARGO_MANIFEST_DIR"), "/README.md"),
                ],
                "{% for file in task_source_files() %}echo {{ file }}; {% endfor %}",
                concat!(
                    "echo ",
                    env!("CARGO_MANIFEST_DIR"),
                    "/Cargo.toml; echo ",
                    env!("CARGO_MANIFEST_DIR"),
                    "/README.md; ",
                ),
            ),
        ];

        for (sources, template, expected) in cases {
            let (sources, template, expected) = (*sources, *template, *expected);

            let (mut task, scripts, parser, config) = (
                Task::default(),
                vec![template.into()],
                TaskScriptParser::new(None),
                Config::get().await.unwrap(),
            );

            task.sources = sources.iter().map(ToString::to_string).collect();

            let (parsed, _) = parser
                .parse_run_scripts(&config, &task, &scripts, &Default::default())
                .await
                .unwrap();

            #[cfg(windows)]
            let expected = expected.replace("/", r"\"); // 🙄

            assert_eq!(parsed, vec![expected]);
        }
    }

    #[tokio::test]
    async fn test_task_usage_hashmap() {
        let task = Task::default();
        let parser = TaskScriptParser::new(None);

        // Manually construct a spec with one arg ("foo") and one flag ("bar")
        // so this test does not rely on run-script parsing.
        let mut cmd = usage::SpecCommand::default();
        cmd.args.push(usage::SpecArg {
            name: "foo".to_string(),
            ..Default::default()
        });
        cmd.flags.push(usage::SpecFlag {
            name: "bar".to_string(),
            // Ensure the flag is recognized as `--bar` by the usage parser
            long: vec!["bar".to_string()],
            ..Default::default()
        });
        let spec = usage::Spec {
            cmd,
            ..Default::default()
        };

        let config = Config::get().await.unwrap();

        // Now test that the usage hashmap is accessible in templates when values are provided
        let scripts_with_usage = vec!["echo arg:{{ usage.foo }} flag:{{ usage.bar }}".to_string()];

        let parsed_scripts = parser
            .parse_run_scripts_with_args(
                &config,
                &task,
                &scripts_with_usage,
                &Default::default(),
                &["test_value".to_string(), "--bar".to_string()],
                &spec,
            )
            .await
            .unwrap();

        // The usage hashmap should contain the parsed values
        // For a string arg, it should be "test_value"
        // For a bool flag, it should be "true"
        assert_eq!(parsed_scripts, vec!["echo arg:test_value flag:true"]);

        // Test without the flag – usage.foo should still be available, but usage.bar
        // should be undefined (accessing it in the template would error), so we only
        // reference usage.foo here.
        let scripts_with_usage_arg_only = vec!["echo arg:{{ usage.foo }}".to_string()];
        let parsed_scripts = parser
            .parse_run_scripts_with_args(
                &config,
                &task,
                &scripts_with_usage_arg_only,
                &Default::default(),
                &["test_value2".to_string()],
                &spec,
            )
            .await
            .unwrap();

        assert_eq!(parsed_scripts, vec!["echo arg:test_value2"]);

        // flag defaults to false when not provided
        let scripts_with_missing_flag = vec!["echo flag:{{ usage.bar }}".to_string()];
        let parsed_scripts = parser
            .parse_run_scripts_with_args(
                &config,
                &task,
                &scripts_with_missing_flag,
                &Default::default(),
                &["only_arg_value".to_string()], // no --bar flag provided
                &spec,
            )
            .await
            .unwrap();
        assert_eq!(parsed_scripts, vec!["echo flag:false"]);
    }

    #[tokio::test]
    async fn test_task_usage_multistring() {
        let task = Task::default();
        let parser = TaskScriptParser::new(None);

        // Manually construct a spec with a var=true arg so usage-lib will produce a MultiString value
        let mut cmd = usage::SpecCommand::default();
        cmd.args.push(usage::SpecArg {
            name: "tags".to_string(),
            var: true,
            ..Default::default()
        });
        let spec = usage::Spec {
            cmd,
            ..Default::default()
        };

        let config = Config::get().await.unwrap();

        // The script only uses the usage map, it does not rely on run-script parsing to build the spec
        let scripts_with_usage = vec![
            "echo count={{ usage.tags | length }} first={{ usage.tags[0] }} second={{ usage.tags[1] }}"
                .to_string(),
        ];
        let parsed_scripts = parser
            .parse_run_scripts_with_args(
                &config,
                &task,
                &scripts_with_usage,
                &Default::default(),
                &["one".to_string(), "two".to_string()],
                &spec,
            )
            .await
            .unwrap();

        assert_eq!(
            parsed_scripts,
            vec!["echo count=2 first=one second=two"],
            "expected MultiString arg to be exposed as an array in the usage map"
        );
    }

    /// Parse a usage spec and render a template with the given args.
    async fn render_usage(usage_kdl: &str, template: &str, args: &[&str]) -> String {
        let config = Config::get().await.unwrap();
        let task = Task {
            usage: usage_kdl.to_string(),
            ..Default::default()
        };
        let parser = TaskScriptParser::new(None);
        let scripts = vec![template.to_string()];
        let (_, spec) = parser
            .parse_run_scripts(&config, &task, &scripts, &Default::default())
            .await
            .unwrap();
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let parsed = parser
            .parse_run_scripts_with_args(
                &config,
                &task,
                &scripts,
                &Default::default(),
                &args,
                &spec,
            )
            .await
            .unwrap();
        parsed.into_iter().next().unwrap()
    }

    /// Same as `render_usage` but with a custom env map.
    async fn render_usage_with_env(
        usage_kdl: &str,
        template: &str,
        args: &[&str],
        env: &EnvMap,
    ) -> String {
        let config = Config::get().await.unwrap();
        let task = Task {
            usage: usage_kdl.to_string(),
            ..Default::default()
        };
        let parser = TaskScriptParser::new(None);
        let scripts = vec![template.to_string()];
        let (_, spec) = parser
            .parse_run_scripts(&config, &task, &scripts, env)
            .await
            .unwrap();
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        parser
            .parse_run_scripts_with_args(&config, &task, &scripts, env, &args, &spec)
            .await
            .unwrap()
            .into_iter()
            .next()
            .unwrap()
    }

    #[tokio::test]
    async fn test_usage_arg_renders() {
        // Required arg
        assert_eq!(
            render_usage(r#"arg "<file>""#, "echo {{ usage.file }}", &["test.txt"]).await,
            "echo test.txt"
        );
        // Multiple args
        assert_eq!(
            render_usage(
                "arg \"<src>\"\narg \"<dst>\"",
                "echo {{ usage.src }} {{ usage.dst }}",
                &["a", "b"]
            )
            .await,
            "echo a b"
        );
        // Default value
        assert_eq!(
            render_usage(
                r#"arg "<file>" default="f.txt""#,
                "echo {{ usage.file }}",
                &[]
            )
            .await,
            "echo f.txt"
        );
        assert_eq!(
            render_usage(
                r#"arg "<file>" default="f.txt""#,
                "echo {{ usage.file }}",
                &["o.txt"]
            )
            .await,
            "echo o.txt"
        );
        // Variadic
        assert_eq!(
            render_usage(
                r#"arg "<file>" var=#true"#,
                "echo {{ usage.file | join(sep=' ') }}",
                &["a", "b", "c"]
            )
            .await,
            "echo a b c"
        );
        // Double-dash required
        assert_eq!(
            render_usage(
                r#"arg "<file>" double_dash="required""#,
                "echo {{ usage.file }}",
                &["--", "f.txt"]
            )
            .await,
            "echo f.txt"
        );
    }

    #[tokio::test]
    async fn test_usage_arg_defaults_for_unprovided() {
        // Optional arg defaults to empty string (not an error)
        assert_eq!(
            render_usage(r#"arg "[file]""#, "echo '{{ usage.file }}'", &[]).await,
            "echo ''"
        );
        assert_eq!(
            render_usage(r#"arg "[file]""#, "echo '{{ usage.file }}'", &["x"]).await,
            "echo 'x'"
        );
        // Variadic optional defaults to empty array
        assert_eq!(
            render_usage(
                r#"arg "[file]" var=#true"#,
                "echo {{ usage.file | length }}",
                &[]
            )
            .await,
            "echo 0"
        );
    }

    #[tokio::test]
    async fn test_usage_arg_env() {
        let env = EnvMap::from_iter(vec![("MY_FILE".to_string(), "env.txt".to_string())]);
        assert_eq!(
            render_usage_with_env(
                r#"arg "<file>" env="MY_FILE""#,
                "echo {{ usage.file }}",
                &[],
                &env
            )
            .await,
            "echo env.txt"
        );
    }

    #[tokio::test]
    async fn test_usage_flag_renders() {
        // Short + long with value
        assert_eq!(
            render_usage(
                r#"flag "-u --user <user>""#,
                "echo {{ usage.user }}",
                &["--user", "alice"]
            )
            .await,
            "echo alice"
        );
        assert_eq!(
            render_usage(
                r#"flag "-u --user <user>""#,
                "echo {{ usage.user }}",
                &["-u", "bob"]
            )
            .await,
            "echo bob"
        );
        // Default value
        assert_eq!(
            render_usage(
                r#"flag "--file <file>" default="f.txt""#,
                "echo {{ usage.file }}",
                &[]
            )
            .await,
            "echo f.txt"
        );
        // Boolean default true
        assert_eq!(
            render_usage(
                r#"flag "--color" default=#true"#,
                "echo {{ usage.color }}",
                &[]
            )
            .await,
            "echo true"
        );
        // Choices
        assert_eq!(
            render_usage(
                "flag \"--shell <shell>\" {\n    choices \"bash\" \"zsh\" \"fish\"\n}",
                "echo {{ usage.shell }}",
                &["--shell", "zsh"]
            )
            .await,
            "echo zsh"
        );
        // Variadic flag
        assert_eq!(
            render_usage(
                r#"flag "--include <pattern>" var=#true"#,
                "echo {{ usage.include | join(sep=',') }}",
                &["--include", "*.rs", "--include", "*.toml"]
            )
            .await,
            "echo *.rs,*.toml"
        );
        // Negate flag
        assert_eq!(
            render_usage(
                r#"flag "--color" negate="--no-color" default=#true"#,
                "echo {{ usage.color }}",
                &[]
            )
            .await,
            "echo true"
        );
        assert_eq!(
            render_usage(
                r#"flag "--color" negate="--no-color" default=#true"#,
                "echo {{ usage.color }}",
                &["--no-color"]
            )
            .await,
            "echo false"
        );
        // Hyphenated -> snake_case
        assert_eq!(
            render_usage(
                r#"flag "--dry-run""#,
                "echo {{ usage.dry_run }}",
                &["--dry-run"]
            )
            .await,
            "echo true"
        );
    }

    #[tokio::test]
    async fn test_usage_flag_defaults_for_unprovided() {
        // Boolean flag defaults to false (not an error)
        assert_eq!(
            render_usage(
                r#"flag "-f --force""#,
                "echo {{ usage.force }}",
                &["--force"]
            )
            .await,
            "echo true"
        );
        assert_eq!(
            render_usage(r#"flag "-f --force""#, "echo {{ usage.force }}", &[]).await,
            "echo false"
        );
        // Count flag defaults to 0
        assert_eq!(
            render_usage(
                r#"flag "-v --verbose" count=#true"#,
                "echo {{ usage.verbose }}",
                &["-vvv"]
            )
            .await,
            "echo 3"
        );
    }

    #[tokio::test]
    async fn test_usage_cmd_renders() {
        let spec = "cmd \"install\" {\n    arg \"<package>\"\n    flag \"--force\"\n}\ncmd \"remove\" {\n    arg \"<package>\"\n}";
        // Subcommand routing
        assert_eq!(
            render_usage(
                spec,
                "echo {{ usage.cmd }} {{ usage.package }} {{ usage.force }}",
                &["install", "foo", "--force"]
            )
            .await,
            "echo install foo true"
        );
        // Different subcommand
        let spec2 =
            "cmd \"build\" {\n    arg \"<target>\"\n}\ncmd \"test\" {\n    arg \"<target>\"\n}";
        assert_eq!(
            render_usage(
                spec2,
                "echo {{ usage.cmd }}:{{ usage.target }}",
                &["test", "all"]
            )
            .await,
            "echo test:all"
        );
        assert_eq!(
            render_usage(
                spec2,
                "echo {{ usage.cmd }}:{{ usage.target }}",
                &["build", "release"]
            )
            .await,
            "echo build:release"
        );
        // No subcommand selected — cmd defaults to empty
        let spec3 = "arg \"<name>\"\ncmd \"sub\" {\n    arg \"<x>\"\n}";
        assert_eq!(
            render_usage(
                spec3,
                "echo cmd={{ usage.cmd }} name={{ usage.name }}",
                &["hello"]
            )
            .await,
            "echo cmd= name=hello"
        );
    }

    #[tokio::test]
    async fn test_usage_combined() {
        let spec = "arg \"<src>\"\narg \"[dst]\" default=\"out.txt\"\nflag \"-f --force\"\nflag \"-v --verbose\" count=#true\nflag \"--mode <mode>\" {\n    choices \"copy\" \"move\" \"link\"\n}";
        assert_eq!(
            render_usage(
                spec,
                "echo {{ usage.src }} {{ usage.dst }} {{ usage.force }} {{ usage.mode }}",
                &["input.txt", "--force", "--mode", "copy"]
            )
            .await,
            "echo input.txt out.txt true copy"
        );
    }

    #[tokio::test]
    async fn test_usage_script_directives() {
        fn parse_script_from_str(script: &str) -> usage::Spec {
            use std::io::Write;
            let mut tmp = tempfile::NamedTempFile::new().unwrap();
            tmp.write_all(script.as_bytes()).unwrap();
            tmp.flush().unwrap();
            usage::Spec::parse_script(tmp.path()).unwrap()
        }
        // Flag + arg from #USAGE directives
        let spec = parse_script_from_str(
            "#!/usr/bin/env bash\n#USAGE flag \"--user <user>\"\n#USAGE arg \"<file>\"\necho hello\n",
        );
        assert_eq!(spec.cmd.flags[0].name, "user");
        assert_eq!(spec.cmd.args[0].name, "file");
        // Multi-line KDL with choices
        let spec = parse_script_from_str(
            "#!/usr/bin/env bash\n#USAGE flag \"--shell <shell>\" {\n#USAGE     choices \"bash\" \"zsh\" \"fish\"\n#USAGE }\necho hello\n",
        );
        assert_eq!(
            spec.cmd.flags[0]
                .arg
                .as_ref()
                .unwrap()
                .choices
                .as_ref()
                .unwrap()
                .choices,
            vec!["bash", "zsh", "fish"]
        );
    }

    #[tokio::test]
    async fn test_usage_empty_and_spec_only() {
        // Empty spec
        let config = Config::get().await.unwrap();
        let task = Task {
            usage: "".to_string(),
            ..Default::default()
        };
        let parser = TaskScriptParser::new(None);
        let (parsed, spec) = parser
            .parse_run_scripts(
                &config,
                &task,
                &["echo hello".to_string()],
                &Default::default(),
            )
            .await
            .unwrap();
        assert_eq!(parsed, vec!["echo hello"]);
        assert!(spec.cmd.args.is_empty() && spec.cmd.flags.is_empty());
        // Spec-only parsing
        let task = Task {
            usage: "arg \"<file>\"\nflag \"--verbose\"".to_string(),
            ..Default::default()
        };
        let spec = parser
            .parse_run_scripts_for_spec_only(&config, &task, &["echo {{ usage.file }}".to_string()])
            .await
            .unwrap();
        assert_eq!(spec.cmd.args.len(), 1);
        assert_eq!(spec.cmd.flags.len(), 1);
    }
}
