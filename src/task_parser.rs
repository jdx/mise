use crate::tera::{get_tera, BASE_CONTEXT};
use clap::Arg;
use eyre::Result;
use itertools::Itertools;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[derive(Debug, Default, Clone)]
pub struct TaskParseArg {
    i: usize,
    name: String,
    required: bool,
    var: bool,
    // default: Option<String>,
    // var_min: Option<usize>,
    // var_max: Option<usize>,
    // choices: Vec<String>,
}

#[derive(Debug, Default)]
pub struct TaskParseResults {
    scripts: Vec<String>,
    args: Vec<TaskParseArg>,
    flags: HashMap<String, TaskParseArg>,
    options: HashMap<String, TaskParseArg>,
}

impl TaskParseResults {
    pub fn render(&self, args: &[String]) -> Vec<String> {
        let mut cmd = clap::Command::new("mise-task");
        for arg in &self.args {
            cmd = cmd.arg(
                Arg::new(arg.name.clone())
                    .required(arg.required)
                    .action(if arg.var {
                        clap::ArgAction::Append
                    } else {
                        clap::ArgAction::Set
                    }),
            );
        }
        for flag in self.flags.values() {
            cmd = cmd.arg(
                Arg::new(flag.name.clone())
                    .long(flag.name.clone())
                    .action(clap::ArgAction::SetTrue),
            );
        }
        for option in self.options.values() {
            cmd = cmd.arg(
                Arg::new(option.name.clone())
                    .long(option.name.clone())
                    .action(if option.var {
                        clap::ArgAction::Append
                    } else {
                        clap::ArgAction::Set
                    }),
            );
        }
        let matches = cmd.get_matches_from(["mise-task".to_string()].iter().chain(args.iter()));
        let mut out = vec![];
        for script in &self.scripts {
            let mut script = script.clone();
            for id in matches.ids() {
                let value = if self.flags.contains_key(id.as_str()) {
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

    pub fn has_any_args_defined(&self) -> bool {
        !self.args.is_empty() || !self.flags.is_empty() || !self.options.is_empty()
    }
}

pub struct TaskParser {
    dir: Option<PathBuf>,
    ctx: tera::Context,
}

impl TaskParser {
    pub fn new(dir: Option<PathBuf>) -> Self {
        TaskParser {
            dir,
            ctx: BASE_CONTEXT.clone(),
        }
    }

    fn get_tera(&self) -> tera::Tera {
        get_tera(self.dir.as_deref())
    }

    pub fn parse_run_scripts(&self, scripts: &[String]) -> Result<TaskParseResults> {
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
                    // let default = args.get("default").map(|d| d.as_str().unwrap().to_string());
                    let arg = TaskParseArg {
                        i,
                        name: name.clone(),
                        required,
                        var,
                        // default,
                    };
                    input_args.lock().unwrap().push(arg);
                    Ok(tera::Value::String(template_key(name)))
                }
            }
        });
        let input_options = Arc::new(Mutex::new(HashMap::new()));
        tera.register_function("option", {
            {
                let input_options = input_options.clone();
                move |args: &HashMap<String, tera::Value>| -> tera::Result<tera::Value> {
                    let name = args
                        .get("name")
                        .map(|n| n.as_str().unwrap().to_string())
                        .unwrap();
                    let var = args
                        .get("var")
                        .map(|r| r.as_bool().unwrap())
                        .unwrap_or(false);
                    // let default = args.get("default").map(|d| d.as_str().unwrap().to_string());
                    let flag = TaskParseArg {
                        name: name.clone(),
                        var,
                        // default,
                        ..Default::default()
                    };
                    input_options.lock().unwrap().insert(name.clone(), flag);
                    Ok(tera::Value::String(template_key(name)))
                }
            }
        });
        let input_flags = Arc::new(Mutex::new(HashMap::new()));
        tera.register_function("flag", {
            {
                let input_flags = input_flags.clone();
                move |args: &HashMap<String, tera::Value>| -> tera::Result<tera::Value> {
                    let name = args
                        .get("name")
                        .map(|n| n.as_str().unwrap().to_string())
                        .unwrap();
                    // let default = args.get("default").map(|d| d.as_str().unwrap().to_string());
                    let flag = TaskParseArg {
                        name: name.clone(),
                        // default,
                        ..Default::default()
                    };
                    input_flags.lock().unwrap().insert(name.clone(), flag);
                    Ok(tera::Value::String(template_key(name)))
                }
            }
        });
        let out = TaskParseResults {
            scripts: scripts
                .iter()
                .map(|s| tera.render_str(s, &self.ctx).unwrap())
                .collect(),
            args: input_args
                .lock()
                .unwrap()
                .iter()
                .cloned()
                .sorted_by_key(|a| a.i)
                .collect(),
            flags: input_flags.lock().unwrap().clone(),
            options: input_options.lock().unwrap().clone(),
        };
        // TODO: ensure no gaps in args, e.g.: 1,2,3,4,5

        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_parse_arg() {
        let parser = TaskParser::new(None);
        let scripts = vec!["echo {{ arg(i=0, name='foo') }}".to_string()];
        let results = parser.parse_run_scripts(&scripts).unwrap();
        assert_eq!(
            results.scripts,
            vec!["echo MISE_TASK_ARG:foo:MISE_TASK_ARG"]
        );
        let arg0 = results.args.first().unwrap();
        assert_eq!(arg0.name, "foo");

        let scripts = results.render(&["abc".to_string()]);
        assert_eq!(scripts, vec!["echo abc"]);
    }

    #[test]
    fn test_task_parse_arg_var() {
        let parser = TaskParser::new(None);
        let scripts = vec!["echo {{ arg(var=true) }}".to_string()];
        let results = parser.parse_run_scripts(&scripts).unwrap();
        assert_eq!(results.scripts, vec!["echo MISE_TASK_ARG:0:MISE_TASK_ARG"]);
        let arg0 = results.args.first().unwrap();
        assert_eq!(arg0.name, "0");

        let scripts = results.render(&["abc".to_string(), "def".to_string()]);
        assert_eq!(scripts, vec!["echo abc def"]);
    }

    #[test]
    fn test_task_parse_flag() {
        let parser = TaskParser::new(None);
        let scripts = vec!["echo {{ flag(name='foo') }}".to_string()];
        let results = parser.parse_run_scripts(&scripts).unwrap();
        assert_eq!(
            results.scripts,
            vec!["echo MISE_TASK_ARG:foo:MISE_TASK_ARG"]
        );
        let flag = results.flags.get("foo").unwrap();
        assert_eq!(flag.name, "foo");

        let scripts = results.render(&["--foo".to_string()]);
        assert_eq!(scripts, vec!["echo true"]);
    }

    #[test]
    fn test_task_parse_option() {
        let parser = TaskParser::new(None);
        let scripts = vec!["echo {{ option(name='foo') }}".to_string()];
        let results = parser.parse_run_scripts(&scripts).unwrap();
        assert_eq!(
            results.scripts,
            vec!["echo MISE_TASK_ARG:foo:MISE_TASK_ARG"]
        );
        let option = results.options.get("foo").unwrap();
        assert_eq!(option.name, "foo");

        let scripts = results.render(&["--foo".to_string(), "abc".to_string()]);
        assert_eq!(scripts, vec!["echo abc"]);
    }
}
