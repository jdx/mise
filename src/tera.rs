use std::collections::HashMap;
use std::path::Path;

use once_cell::sync::Lazy;
use tera::{Context, Tera, Value};

use crate::cmd::cmd;
use crate::env;

pub static BASE_CONTEXT: Lazy<Context> = Lazy::new(|| {
    let mut context = Context::new();
    context.insert("env", &*env::PRISTINE_ENV);
    context
});

pub fn get_tera(dir: &Path) -> Tera {
    let mut tera = Tera::default();
    let dir = dir.to_path_buf();
    tera.register_function(
        "exec",
        move |args: &HashMap<String, Value>| -> tera::Result<Value> {
            match args.get("command") {
                Some(Value::String(command)) => {
                    let result = cmd("bash", ["-c", command])
                        .dir(&dir)
                        .full_env(&*env::PRISTINE_ENV)
                        .read()?;
                    Ok(Value::String(result))
                }
                _ => Err("exec command must be a string".into()),
            }
        },
    );
    tera
}
