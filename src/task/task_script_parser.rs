use crate::config::{Config, Settings};
use crate::env_diff::EnvMap;
use crate::exit::exit;
use crate::shell::ShellType;
use crate::task::Task;
use crate::tera::get_tera;
use eyre::{Context, Result};
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
}

impl TaskScriptParser {
    pub fn new(dir: Option<PathBuf>) -> Self {
        TaskScriptParser { dir }
    }

    fn get_tera(&self) -> tera::Tera {
        get_tera(self.dir.as_deref())
    }

    fn render_script_with_context(
        tera: &mut tera::Tera,
        script: &str,
        ctx: &tera::Context,
    ) -> Result<String> {
        tera.render_str(script.trim(), ctx)
            .with_context(|| format!("Failed to render task script: {}", script))
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

    fn setup_tera_for_spec_parsing(&self) -> TeraSpecParsingResult {
        let mut tera = self.get_tera();
        let arg_order = Arc::new(Mutex::new(HashMap::new()));
        let input_args = Arc::new(Mutex::new(vec![]));
        let input_flags = Arc::new(Mutex::new(vec![]));

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
                    return Ok(tera::Value::Null);
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

                let default = Self::expect_opt_string(args.get("default"), "default")?;

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
                    ..Default::default()
                };
                arg.usage = arg.usage();

                input_args.lock().map_err(Self::lock_error)?.push(arg);

                Ok(tera::Value::Null)
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

                let default = Self::expect_opt_string(args.get("default"), "default")?;

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
                    arg: Some(usage::SpecArg {
                        name: name.clone(),
                        var,
                        choices,
                        ..Default::default()
                    }),
                };
                flag.usage = flag.usage();

                input_flags.lock().map_err(Self::lock_error)?.push(flag);

                Ok(tera::Value::Null)
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

                let default = Self::expect_opt_string(args.get("default"), "default")?;

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
                    arg: None,
                };
                flag.usage = flag.usage();

                input_flags.lock().map_err(Self::lock_error)?.push(flag);

                Ok(tera::Value::Null)
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
        let (mut tera, arg_order, input_args, input_flags) = self.setup_tera_for_spec_parsing();
        let tera_ctx = task.tera_ctx(config).await?;
        // Don't insert env for spec-only parsing to avoid expensive environment rendering
        // Render scripts to trigger spec collection via Tera template functions (arg/option/flag), but discard the results
        for script in scripts {
            Self::render_script_with_context(&mut tera, script, &tera_ctx)?;
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
        let mut spec = usage::Spec {
            cmd,
            ..Default::default()
        };
        spec.merge(task.usage.parse()?);

        Ok(spec)
    }

    pub async fn parse_run_scripts(
        &self,
        config: &Arc<Config>,
        task: &Task,
        scripts: &[String],
        env: &EnvMap,
    ) -> Result<(Vec<String>, usage::Spec)> {
        let (mut tera, arg_order, input_args, input_flags) = self.setup_tera_for_spec_parsing();
        let mut tera_ctx = task.tera_ctx(config).await?;
        tera_ctx.insert("env", &env);
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
        let mut spec = usage::Spec {
            cmd,
            ..Default::default()
        };
        spec.merge(task.usage.parse()?);

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
        let m = match usage::parse(spec, &args) {
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
            tera_ctx.insert("env", &env);
            out.push(Self::render_script_with_context(
                &mut tera, script, &tera_ctx,
            )?);
        }
        Ok(out)
    }
}

pub fn has_any_args_defined(spec: &usage::Spec) -> bool {
    !spec.cmd.args.is_empty() || !spec.cmd.flags.is_empty()
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
}
