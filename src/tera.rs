use std::collections::HashMap;
use std::path::{Path, PathBuf};

use once_cell::sync::Lazy;
use tera::{Context, Tera, Value};

use crate::cmd::cmd;
use crate::env;
use crate::hash::hash_to_str;

pub static BASE_CONTEXT: Lazy<Context> = Lazy::new(|| {
    let mut context = Context::new();
    context.insert("env", &*env::PRISTINE_ENV);
    if let Ok(dir) = env::current_dir() {
        context.insert("cwd", &dir);
    }
    context
});

pub fn get_tera(dir: Option<&Path>) -> Tera {
    let mut tera = Tera::default();
    let dir = dir.map(PathBuf::from);
    tera.register_function(
        "exec",
        move |args: &HashMap<String, Value>| -> tera::Result<Value> {
            match args.get("command") {
                Some(Value::String(command)) => {
                    let mut cmd = cmd("bash", ["-c", command]).full_env(&*env::PRISTINE_ENV);
                    if let Some(dir) = &dir {
                        cmd = cmd.dir(dir);
                    }
                    let result = cmd.read()?;
                    Ok(Value::String(result))
                }
                _ => Err("exec command must be a string".into()),
            }
        },
    );
    tera.register_filter(
        "hash",
        move |input: &Value, _args: &HashMap<String, Value>| match input {
            Value::String(s) => Ok(Value::String(hash_to_str(s))),
            _ => Err("hash input must be a string".into()),
        },
    );
    tera.register_filter(
        "canonicalize",
        move |input: &Value, _args: &HashMap<String, Value>| match input {
            Value::String(s) => {
                let p = Path::new(s).canonicalize()?;
                Ok(Value::String(p.to_string_lossy().to_string()))
            }
            _ => Err("hash input must be a string".into()),
        },
    );
    tera.register_filter(
        "last_modified",
        move |input: &Value, _args: &HashMap<String, Value>| match input {
            Value::String(s) => {
                let p = Path::new(s);
                let metadata = p.metadata()?;
                let modified = metadata.modified()?;
                let modified = modified.duration_since(std::time::UNIX_EPOCH).unwrap();
                Ok(Value::Number(modified.as_secs().into()))
            }
            _ => Err("hash input must be a string".into()),
        },
    );
    tera.register_filter(
        "join_path",
        move |input: &Value, _args: &HashMap<String, Value>| match input {
            Value::Array(arr) => arr
                .iter()
                .map(Value::as_str)
                .collect::<Option<PathBuf>>()
                .ok_or("join_path input must be an array of strings".into())
                .map(|p| Value::String(p.to_string_lossy().to_string())),
            _ => Err("join_path input must be an array of strings".into()),
        },
    );
    tera.register_tester(
        "file_exists",
        move |input: Option<&Value>, _args: &[Value]| match input {
            Some(Value::String(s)) => Ok(Path::new(s).exists()),
            _ => Err("file_exists input must be a string".into()),
        },
    );
    tera
}
