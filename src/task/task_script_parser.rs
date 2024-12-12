use crate::config::{Config, SETTINGS};
use crate::shell::ShellType;
use crate::task::Task;
use crate::tera::{get_tera, BASE_CONTEXT};
use eyre::Result;
use indexmap::IndexMap;
use itertools::Itertools;
use std::collections::HashMap;
use std::iter::once;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use usage::parse::ParseValue;
use xx::regex;

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

    pub fn parse_run_scripts(
        &self,
        config_root: &Option<PathBuf>,
        scripts: &[String],
    ) -> Result<(Vec<String>, usage::Spec)> {
        let mut tera = self.get_tera();
        let arg_order = Arc::new(Mutex::new(HashMap::new()));
        let input_args = Arc::new(Mutex::new(vec![]));
        let template_key = |name| format!("MISE_TASK_ARG:{name}:MISE_TASK_ARG");
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
                        return Ok(tera::Value::String(template_key(name)))
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
                    };
                    arg.usage = arg.usage();
                    input_args.lock().unwrap().push(arg);
                    Ok(tera::Value::String(template_key(name)))
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
                    Ok(tera::Value::String(template_key(name)))
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
                    Ok(tera::Value::String(template_key(name)))
                }
            }
        });
        let mut ctx = BASE_CONTEXT.clone();
        ctx.insert("config_root", config_root);
        let mut vars = IndexMap::new();
        ctx.insert("vars", &vars);
        for (k, v) in &Config::get().vars {
            vars.insert(k.clone(), tera.render_str(v, &ctx).unwrap());
            ctx.insert("vars", &vars);
        }
        let scripts = scripts
            .iter()
            .map(|s| tera.render_str(s.trim(), &ctx).unwrap())
            .collect();
        let mut cmd = usage::SpecCommand::default();
        // TODO: ensure no gaps in args, e.g.: 1,2,3,4,5
        let arg_order = arg_order.lock().unwrap();
        cmd.args = input_args
            .lock()
            .unwrap()
            .iter()
            .cloned()
            .sorted_by_key(|arg| arg_order.get(&arg.name).unwrap_or_else(|| panic!("missing arg order for {}", arg.name.as_str())))
            .collect();
        cmd.flags = input_flags.lock().unwrap().clone();
        let spec = usage::Spec {
            cmd,
            ..Default::default()
        };

        Ok((scripts, spec))
    }
}

pub fn replace_template_placeholders_with_args(
    task: &Task,
    spec: &usage::Spec,
    scripts: &[String],
    args: &[String],
) -> Result<Vec<String>> {
    let args = vec!["".to_string()]
        .into_iter()
        .chain(args.iter().cloned())
        .collect::<Vec<_>>();
    let m = usage::parse(spec, &args).map_err(|e| eyre::eyre!(e.to_string()))?;
    let mut out = vec![];
    let re = regex!(r"MISE_TASK_ARG:(\w+):MISE_TASK_ARG");
    for script in scripts {
        let shell_type: Option<ShellType> = shell_from_shebang(script)
            .or(task.shell())
            .unwrap_or(SETTINGS.default_inline_shell()?)[0]
            .parse()
            .ok();
        let escape = |v: &ParseValue| match v {
            ParseValue::MultiString(_) => {
                // these are already escaped
                v.to_string()
            }
            _ => match shell_type {
                Some(ShellType::Zsh | ShellType::Bash | ShellType::Fish) => {
                    shell_words::quote(&v.to_string()).to_string()
                }
                _ => v.to_string(),
            },
        };
        let mut script = script.clone();
        for (arg, value) in &m.args {
            script = script.replace(
                &format!("MISE_TASK_ARG:{}:MISE_TASK_ARG", arg.name),
                &escape(value),
            );
        }
        for (flag, value) in &m.flags {
            script = script.replace(
                &format!("MISE_TASK_ARG:{}:MISE_TASK_ARG", flag.name),
                &escape(value),
            );
        }
        script = re.replace_all(&script, "").to_string();
        out.push(script);
    }
    Ok(out)
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

    #[test]
    fn test_task_parse_arg() {
        let task = Task::default();
        let parser = TaskScriptParser::new(None);
        let scripts = vec!["echo {{ arg(i=0, name='foo') }}".to_string()];
        let (scripts, spec) = parser.parse_run_scripts(&None, &scripts).unwrap();
        assert_eq!(scripts, vec!["echo MISE_TASK_ARG:foo:MISE_TASK_ARG"]);
        let arg0 = spec.cmd.args.first().unwrap();
        assert_eq!(arg0.name, "foo");

        let scripts =
            replace_template_placeholders_with_args(&task, &spec, &scripts, &["abc".to_string()])
                .unwrap();
        assert_eq!(scripts, vec!["echo abc"]);
    }

    #[test]
    fn test_task_parse_multi_use_arg() {
        let task = Task::default();
        let parser = TaskScriptParser::new(None);
        let scripts = vec!["echo {{ arg(name='foo') }}; echo {{ arg(name='bar') }}; echo {{ arg(name='foo') }}".to_string()];
        let (scripts, spec) = parser.parse_run_scripts(&None, &scripts).unwrap();
        assert_eq!(scripts, vec!["echo MISE_TASK_ARG:foo:MISE_TASK_ARG; echo MISE_TASK_ARG:bar:MISE_TASK_ARG; echo MISE_TASK_ARG:foo:MISE_TASK_ARG"]);
        let arg0 = spec.cmd.args.first().unwrap();
        let arg1 = spec.cmd.args.iter().nth(1).unwrap();
        assert_eq!(arg0.name, "foo");
        assert_eq!(arg1.name, "bar");
        assert_eq!(spec.cmd.args.len(), 2);

        let scripts =
            replace_template_placeholders_with_args(&task, &spec, &scripts, &["abc".to_string(), "def".to_string()])
                .unwrap();
        assert_eq!(scripts, vec!["echo abc; echo def; echo abc"]);
    }

    #[test]
    fn test_task_parse_arg_var() {
        let task = Task::default();
        let parser = TaskScriptParser::new(None);
        let scripts = vec!["echo {{ arg(var=true) }}".to_string()];
        let (scripts, spec) = parser.parse_run_scripts(&None, &scripts).unwrap();
        assert_eq!(scripts, vec!["echo MISE_TASK_ARG:0:MISE_TASK_ARG"]);
        let arg0 = spec.cmd.args.first().unwrap();
        assert_eq!(arg0.name, "0");

        let scripts = replace_template_placeholders_with_args(
            &task,
            &spec,
            &scripts,
            &["abc".to_string(), "def".to_string()],
        )
        .unwrap();
        assert_eq!(scripts, vec!["echo abc def"]);
    }

    #[test]
    fn test_task_parse_flag() {
        let task = Task::default();
        let parser = TaskScriptParser::new(None);
        let scripts = vec!["echo {{ flag(name='foo') }}".to_string()];
        let (scripts, spec) = parser.parse_run_scripts(&None, &scripts).unwrap();
        assert_eq!(scripts, vec!["echo MISE_TASK_ARG:foo:MISE_TASK_ARG"]);
        let flag = spec.cmd.flags.iter().find(|f| &f.name == "foo").unwrap();
        assert_eq!(&flag.name, "foo");

        let scripts =
            replace_template_placeholders_with_args(&task, &spec, &scripts, &["--foo".to_string()])
                .unwrap();
        assert_eq!(scripts, vec!["echo true"]);
    }

    #[test]
    fn test_task_parse_option() {
        let task = Task::default();
        let parser = TaskScriptParser::new(None);
        let scripts = vec!["echo {{ option(name='foo') }}".to_string()];
        let (scripts, spec) = parser.parse_run_scripts(&None, &scripts).unwrap();
        assert_eq!(scripts, vec!["echo MISE_TASK_ARG:foo:MISE_TASK_ARG"]);
        let option = spec.cmd.flags.iter().find(|f| &f.name == "foo").unwrap();
        assert_eq!(&option.name, "foo");

        let scripts = replace_template_placeholders_with_args(
            &task,
            &spec,
            &scripts,
            &["--foo".to_string(), "abc".to_string()],
        )
        .unwrap();
        assert_eq!(scripts, vec!["echo abc"]);
    }
}
