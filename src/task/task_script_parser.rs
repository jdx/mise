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

    pub async fn parse_run_scripts(
        &self,
        config: &Arc<Config>,
        task: &Task,
        scripts: &[String],
        env: &EnvMap,
    ) -> Result<(Vec<String>, usage::Spec)> {
        let mut tera = self.get_tera();
        let arg_order = Arc::new(Mutex::new(HashMap::new()));
        let input_args = Arc::new(Mutex::new(vec![]));
        // render args, options, and flags as null
        // these functions are only used to collect the spec
        tera.register_function("arg", {
            {
                let input_args = input_args.clone();
                let arg_order = arg_order.clone();
                move |args: &HashMap<String, tera::Value>| -> tera::Result<tera::Value> {
                    let i = args
                        .get("i")
                        .map(|i| i.as_i64().unwrap() as usize)
                        .unwrap_or_else(|| input_args.lock().unwrap().len());
                    let required = args
                        .get("required")
                        .map(|r| r.as_bool().unwrap())
                        .unwrap_or(true);
                    let var = args
                        .get("var")
                        .map(|r| r.as_bool().unwrap())
                        .unwrap_or(false);
                    let name = args
                        .get("name")
                        .map(|n| n.as_str().unwrap().to_string())
                        .unwrap_or(i.to_string());
                    let mut arg_order = arg_order.lock().unwrap();
                    if arg_order.contains_key(&name) {
                        trace!("already seen {name}");
                        return Ok(tera::Value::Null);
                    }
                    arg_order.insert(name.clone(), i);
                    let usage = args.get("usage").map(|r| r.to_string()).unwrap_or_default();
                    let help = args.get("help").map(|r| r.to_string());
                    let help_long = args.get("help_long").map(|r| r.to_string());
                    let help_md = args.get("help_md").map(|r| r.to_string());
                    let var_min = args.get("var_min").map(|r| r.as_i64().unwrap() as usize);
                    let var_max = args.get("var_max").map(|r| r.as_i64().unwrap() as usize);
                    let hide = args
                        .get("hide")
                        .map(|r| r.as_bool().unwrap())
                        .unwrap_or(false);
                    let default = args.get("default").map(|d| d.as_str().unwrap().to_string());
                    let choices = args.get("choices").map(|c| {
                        let choices = c
                            .as_array()
                            .unwrap()
                            .iter()
                            .map(|c| c.as_str().unwrap().to_string())
                            .collect();
                        usage::SpecChoices { choices }
                    });
                    let mut arg = usage::SpecArg {
                        name: name.clone(),
                        usage,
                        help_first_line: help
                            .clone()
                            .map(|h| h.lines().next().unwrap().to_string()),
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
                    input_args.lock().unwrap().push(arg);
                    Ok(tera::Value::Null)
                }
            }
        });
        let input_flags = Arc::new(Mutex::new(vec![]));
        tera.register_function("option", {
            {
                let input_flags = input_flags.clone();
                move |args: &HashMap<String, tera::Value>| -> tera::Result<tera::Value> {
                    let name = args
                        .get("name")
                        .map(|n| n.as_str().unwrap().to_string())
                        .unwrap();
                    let short = args
                        .get("short")
                        .map(|s| s.to_string().chars().collect())
                        .unwrap_or_default();
                    let long = args
                        .get("long")
                        .map(|l| {
                            l.as_str()
                                .unwrap()
                                .split_whitespace()
                                .map(|s| s.to_string())
                                .collect()
                        })
                        .unwrap_or_else(|| vec![name.clone()]);
                    let default = args.get("default").map(|d| d.as_str().unwrap().to_string());
                    let var = args
                        .get("var")
                        .map(|r| r.as_bool().unwrap())
                        .unwrap_or(false);
                    let deprecated = args.get("deprecated").map(|r| r.to_string());
                    let help = args.get("help").map(|r| r.to_string());
                    let help_long = args.get("help_long").map(|r| r.to_string());
                    let help_md = args.get("help_md").map(|r| r.to_string());
                    let hide = args
                        .get("hide")
                        .map(|r| r.as_bool().unwrap())
                        .unwrap_or(false);
                    let global = args
                        .get("global")
                        .map(|r| r.as_bool().unwrap())
                        .unwrap_or(false);
                    let count = args
                        .get("count")
                        .map(|r| r.as_bool().unwrap())
                        .unwrap_or(false);
                    let usage = args.get("usage").map(|r| r.to_string()).unwrap_or_default();
                    let required = args
                        .get("required")
                        .map(|r| r.as_bool().unwrap())
                        .unwrap_or(false);
                    let negate = args.get("negate").map(|r| r.to_string());
                    let choices = args.get("choices").map(|c| {
                        let choices = c
                            .as_array()
                            .unwrap()
                            .iter()
                            .map(|c| c.as_str().unwrap().to_string())
                            .collect();
                        usage::SpecChoices { choices }
                    });
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
                        help_first_line: help
                            .clone()
                            .map(|h| h.lines().next().unwrap().to_string()),
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
                    input_flags.lock().unwrap().push(flag);
                    Ok(tera::Value::Null)
                }
            }
        });
        tera.register_function("flag", {
            {
                let input_flags = input_flags.clone();
                move |args: &HashMap<String, tera::Value>| -> tera::Result<tera::Value> {
                    let name = args
                        .get("name")
                        .map(|n| n.as_str().unwrap().to_string())
                        .unwrap();
                    let short = args
                        .get("short")
                        .map(|s| s.to_string().chars().collect())
                        .unwrap_or_default();
                    let long = args
                        .get("long")
                        .map(|l| {
                            l.as_str()
                                .unwrap()
                                .split_whitespace()
                                .map(|s| s.to_string())
                                .collect()
                        })
                        .unwrap_or_else(|| vec![name.clone()]);
                    let default = args.get("default").map(|d| d.as_str().unwrap().to_string());
                    let var = args
                        .get("var")
                        .map(|r| r.as_bool().unwrap())
                        .unwrap_or(false);
                    let deprecated = args.get("deprecated").map(|r| r.to_string());
                    let help = args.get("help").map(|r| r.to_string());
                    let help_long = args.get("help_long").map(|r| r.to_string());
                    let help_md = args.get("help_md").map(|r| r.to_string());
                    let hide = args
                        .get("hide")
                        .map(|r| r.as_bool().unwrap())
                        .unwrap_or(false);
                    let global = args
                        .get("global")
                        .map(|r| r.as_bool().unwrap())
                        .unwrap_or(false);
                    let count = args
                        .get("count")
                        .map(|r| r.as_bool().unwrap())
                        .unwrap_or(false);
                    let usage = args.get("usage").map(|r| r.to_string()).unwrap_or_default();
                    let required = args
                        .get("required")
                        .map(|r| r.as_bool().unwrap())
                        .unwrap_or(false);
                    let negate = args.get("negate").map(|r| r.to_string());
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
                        help_first_line: help
                            .clone()
                            .map(|h| h.lines().next().unwrap().to_string()),
                        help,
                        usage,
                        help_long,
                        help_md,
                        required,
                        negate,
                        arg: None,
                    };
                    flag.usage = flag.usage();
                    input_flags.lock().unwrap().push(flag);
                    Ok(tera::Value::Null)
                }
            }
        });
        let mut tera_ctx = task.tera_ctx(config).await?;
        tera_ctx.insert("env", &env);
        let scripts = scripts
            .iter()
            .map(|s| {
                tera.render_str(s.trim(), &tera_ctx)
                    .wrap_err_with(|| s.to_string())
            })
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
                {
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
                                .unwrap_or("false".to_string()),
                        ))
                    }
                }
            };
            tera.register_function("option", flag_func.clone());
            tera.register_function("flag", flag_func);
            let mut tera_ctx = task.tera_ctx(config).await?;
            tera_ctx.insert("env", &env);
            out.push(
                tera.render_str(script, &tera_ctx)
                    .wrap_err_with(|| script.clone())?,
            );
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
}
