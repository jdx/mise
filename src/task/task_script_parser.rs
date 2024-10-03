use crate::tera::{get_tera, BASE_CONTEXT};
use eyre::Result;
use itertools::Itertools;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub struct TaskScriptParser {
    dir: Option<PathBuf>,
    ctx: tera::Context,
}

impl TaskScriptParser {
    pub fn new(dir: Option<PathBuf>) -> Self {
        TaskScriptParser {
            dir,
            ctx: BASE_CONTEXT.clone(),
        }
    }

    fn get_tera(&self) -> tera::Tera {
        get_tera(self.dir.as_deref())
    }

    pub fn parse_run_scripts(&self, scripts: &[String]) -> Result<(Vec<String>, usage::Spec)> {
        let mut tera = self.get_tera();
        let input_args = Arc::new(Mutex::new(vec![]));
        let template_key = |name| format!("MISE_TASK_ARG:{name}:MISE_TASK_ARG");
        tera.register_function("arg", {
            {
                let input_args = input_args.clone();
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
                    input_args.lock().unwrap().push((i, arg));
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
        let scripts = scripts
            .iter()
            .map(|s| tera.render_str(s, &self.ctx).unwrap())
            .collect();
        let mut cmd = usage::SpecCommand::default();
        // TODO: ensure no gaps in args, e.g.: 1,2,3,4,5
        cmd.args = input_args
            .lock()
            .unwrap()
            .iter()
            .cloned()
            .sorted_by_key(|(i, _)| *i)
            .map(|(_, arg)| arg)
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
    spec: &usage::Spec,
    scripts: &[String],
    args: &[String],
) -> Vec<String> {
    let mut cmd = clap::Command::new("mise-task");
    for arg in &spec.cmd.args {
        cmd = cmd.arg(
            clap::Arg::new(arg.name.clone())
                .required(arg.required)
                .action(if arg.var {
                    clap::ArgAction::Append
                } else {
                    clap::ArgAction::Set
                }),
        );
    }
    let mut flags = HashSet::new();
    for flag in &spec.cmd.flags {
        if flag.arg.is_some() {
            cmd = cmd.arg(
                clap::Arg::new(flag.name.clone())
                    .long(flag.name.clone())
                    .action(if flag.var {
                        clap::ArgAction::Append
                    } else {
                        clap::ArgAction::Set
                    }),
            );
        } else {
            flags.insert(flag.name.as_str());
            cmd = cmd.arg(
                clap::Arg::new(flag.name.clone())
                    .long(flag.name.clone())
                    .action(clap::ArgAction::SetTrue),
            );
        }
    }
    let matches = cmd.get_matches_from(["mise-task".to_string()].iter().chain(args.iter()));
    let mut out = vec![];
    for script in scripts {
        let mut script = script.clone();
        for id in matches.ids() {
            let value = if flags.contains(id.as_str()) {
                matches.get_one::<bool>(id.as_str()).unwrap().to_string()
            } else {
                matches.get_many::<String>(id.as_str()).unwrap().join(" ")
            };
            script = script.replace(&format!("MISE_TASK_ARG:{id}:MISE_TASK_ARG"), &value);
        }
        out.push(script);
    }
    out
}

pub fn has_any_args_defined(spec: &usage::Spec) -> bool {
    !spec.cmd.args.is_empty() || !spec.cmd.flags.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test::reset;

    #[test]
    fn test_task_parse_arg() {
        reset();
        let parser = TaskScriptParser::new(None);
        let scripts = vec!["echo {{ arg(i=0, name='foo') }}".to_string()];
        let (scripts, spec) = parser.parse_run_scripts(&scripts).unwrap();
        assert_eq!(scripts, vec!["echo MISE_TASK_ARG:foo:MISE_TASK_ARG"]);
        let arg0 = spec.cmd.args.first().unwrap();
        assert_eq!(arg0.name, "foo");

        let scripts =
            replace_template_placeholders_with_args(&spec, &scripts, &["abc".to_string()]);
        assert_eq!(scripts, vec!["echo abc"]);
    }

    #[test]
    fn test_task_parse_arg_var() {
        reset();
        let parser = TaskScriptParser::new(None);
        let scripts = vec!["echo {{ arg(var=true) }}".to_string()];
        let (scripts, spec) = parser.parse_run_scripts(&scripts).unwrap();
        assert_eq!(scripts, vec!["echo MISE_TASK_ARG:0:MISE_TASK_ARG"]);
        let arg0 = spec.cmd.args.first().unwrap();
        assert_eq!(arg0.name, "0");

        let scripts = replace_template_placeholders_with_args(
            &spec,
            &scripts,
            &["abc".to_string(), "def".to_string()],
        );
        assert_eq!(scripts, vec!["echo abc def"]);
    }

    #[test]
    fn test_task_parse_flag() {
        reset();
        let parser = TaskScriptParser::new(None);
        let scripts = vec!["echo {{ flag(name='foo') }}".to_string()];
        let (scripts, spec) = parser.parse_run_scripts(&scripts).unwrap();
        assert_eq!(scripts, vec!["echo MISE_TASK_ARG:foo:MISE_TASK_ARG"]);
        let flag = spec.cmd.flags.iter().find(|f| &f.name == "foo").unwrap();
        assert_eq!(&flag.name, "foo");

        let scripts =
            replace_template_placeholders_with_args(&spec, &scripts, &["--foo".to_string()]);
        assert_eq!(scripts, vec!["echo true"]);
    }

    #[test]
    fn test_task_parse_option() {
        reset();
        let parser = TaskScriptParser::new(None);
        let scripts = vec!["echo {{ option(name='foo') }}".to_string()];
        let (scripts, spec) = parser.parse_run_scripts(&scripts).unwrap();
        assert_eq!(scripts, vec!["echo MISE_TASK_ARG:foo:MISE_TASK_ARG"]);
        let option = spec.cmd.flags.iter().find(|f| &f.name == "foo").unwrap();
        assert_eq!(&option.name, "foo");

        let scripts = replace_template_placeholders_with_args(
            &spec,
            &scripts,
            &["--foo".to_string(), "abc".to_string()],
        );
        assert_eq!(scripts, vec!["echo abc"]);
    }
}
