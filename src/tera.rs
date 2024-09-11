use std::collections::HashMap;
use std::path::{Path, PathBuf};

use heck::{
    ToKebabCase, ToLowerCamelCase, ToShoutyKebabCase, ToShoutySnakeCase, ToSnakeCase,
    ToUpperCamelCase,
};
use once_cell::sync::Lazy;
use platform_info::{PlatformInfo, PlatformInfoAPI, UNameAPI};
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
    tera.register_function(
        "arch",
        move |_args: &HashMap<String, Value>| -> tera::Result<Value> {
            let info = PlatformInfo::new().expect("unable to determine platform info");
            let result = String::from(info.machine().to_string_lossy()); // ignore potential UTF-8 convension error
            Ok(Value::String(result))
        },
    );
    tera.register_function(
        "num_cpus",
        move |_args: &HashMap<String, Value>| -> tera::Result<Value> {
            let num = num_cpus::get();
            Ok(Value::String(num.to_string()))
        },
    );
    tera.register_function(
        "os",
        move |_args: &HashMap<String, Value>| -> tera::Result<Value> {
            let info = PlatformInfo::new().expect("unable to determine platform info");
            let result = String::from(info.osname().to_string_lossy()); // ignore potential UTF-8 convension error
            Ok(Value::String(result))
        },
    );
    tera.register_function(
        "os_family",
        move |_args: &HashMap<String, Value>| -> tera::Result<Value> {
            let info = PlatformInfo::new().expect("unable to determine platform info");
            let result = String::from(info.sysname().to_string_lossy()); // ignore potential UTF-8 convension error
            Ok(Value::String(result))
        },
    );
    tera.register_function(
        "invocation_directory",
        move |_args: &HashMap<String, Value>| -> tera::Result<Value> {
            let path = env::current_dir().unwrap_or_default();

            let result = String::from(path.to_string_lossy());

            Ok(Value::String(result))
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
    tera.register_filter(
        "quote",
        move |input: &Value, _args: &HashMap<String, Value>| match input {
            Value::String(s) => {
                let result = format!("'{}'", s.replace("'", "\\'"));

                Ok(Value::String(result))
            }
            _ => Err("quote input must be a string".into()),
        },
    );
    tera.register_filter(
        "kebabcase",
        move |input: &Value, _args: &HashMap<String, Value>| match input {
            Value::String(s) => Ok(Value::String(s.to_kebab_case())),
            _ => Err("kebabcase input must be a string".into()),
        },
    );
    tera.register_filter(
        "lowercamelcase",
        move |input: &Value, _args: &HashMap<String, Value>| match input {
            Value::String(s) => Ok(Value::String(s.to_lower_camel_case())),
            _ => Err("lowercamelcase input must be a string".into()),
        },
    );
    tera.register_filter(
        "shoutykebabcase",
        move |input: &Value, _args: &HashMap<String, Value>| match input {
            Value::String(s) => Ok(Value::String(s.to_shouty_kebab_case())),
            _ => Err("shoutykebabcase input must be a string".into()),
        },
    );
    tera.register_filter(
        "shoutysnakecase",
        move |input: &Value, _args: &HashMap<String, Value>| match input {
            Value::String(s) => Ok(Value::String(s.to_shouty_snake_case())),
            _ => Err("shoutysnakecase input must be a string".into()),
        },
    );
    tera.register_filter(
        "snakecase",
        move |input: &Value, _args: &HashMap<String, Value>| match input {
            Value::String(s) => Ok(Value::String(s.to_snake_case())),
            _ => Err("snakecase input must be a string".into()),
        },
    );
    tera.register_filter(
        "uppercamelcase",
        move |input: &Value, _args: &HashMap<String, Value>| match input {
            Value::String(s) => Ok(Value::String(s.to_upper_camel_case())),
            _ => Err("uppercamelcase input must be a string".into()),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(target_arch = "x86_64")]
    fn test_render_with_custom_function_arch_x86_64() {
        let mut tera = get_tera(Option::default());

        let result = tera
            .render_str("{{ arch() }}", &Context::default())
            .unwrap();

        assert_eq!("x86_64", result);
    }

    #[test]
    #[cfg(target_arch = "aarch64")]
    fn test_render_with_custom_function_arch_arm64() {
        let mut tera = get_tera(Option::default());

        let result = tera
            .render_str("{{ arch() }}", &Context::default())
            .unwrap();

        assert_eq!("aarch64", result);
    }

    #[test]
    fn test_render_with_custom_function_num_cpus() {
        let mut tera = get_tera(Option::default());

        let result = tera
            .render_str("{{ num_cpus() }}", &Context::default())
            .unwrap();

        let num = result.parse::<u32>().unwrap();
        assert!(num > 0);
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn test_render_with_custom_function_os_linux() {
        let mut tera = get_tera(Option::default());

        let result = tera.render_str("{{ os() }}", &Context::default()).unwrap();

        assert_eq!("GNU/Linux", result);
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn test_render_with_custom_function_os_windows() {
        let mut tera = get_tera(Option::default());

        let result = tera.render_str("{{ os() }}", &Context::default()).unwrap();

        assert_eq!("Windows", result);
    }

    #[test]
    #[cfg(target_family = "unix")]
    fn test_render_with_custom_function_os_family_unix() {
        let mut tera = get_tera(Option::default());

        let result = tera
            .render_str("{{ os_family() }}", &Context::default())
            .unwrap();

        assert_eq!("Linux", result);
    }

    #[test]
    #[cfg(target_family = "windows")]
    fn test_render_with_custom_function_os_windows() {
        let mut tera = get_tera(Option::default());

        let result = tera
            .render_str("{{ os_family() }}", &Context::default())
            .unwrap();

        assert_eq!("Windows", result);
    }

    #[test]
    #[cfg(target_family = "unix")]
    fn test_render_with_custom_function_invocation_directory() {
        let a = env::set_current_dir("/tmp").is_ok();
        let mut tera = get_tera(Option::default());
        assert!(a);
        println!("{:?}", env::current_dir().unwrap());

        let result = tera
            .render_str("{{ invocation_directory() }}", &Context::default())
            .unwrap();

        assert_eq!("/tmp", result);
    }

    #[test]
    #[cfg(target_family = "windows")]
    fn test_render_with_custom_function_invocation_directory() {
        let a = env::set_current_dir("C:\\").is_ok();
        let mut tera = get_tera(Option::default());
        assert!(a);

        let result = tera
            .render_str("{{ invocation_directory() }}", &Context::default())
            .unwrap();

        assert_eq!("C:\\", result);
    }

    #[test]
    fn test_render_with_custom_filter_quote() {
        let mut tera = get_tera(Option::default());

        let result = tera
            .render_str("{{ \"quoted'str\" | quote }}", &Context::default())
            .unwrap();

        assert_eq!("'quoted\'str'", result);
    }

    #[test]
    fn test_render_with_custom_filter_kebabcase() {
        let mut tera = get_tera(Option::default());

        let result = tera
            .render_str("{{ \"thisFilter\" | kebabcase }}", &Context::default())
            .unwrap();

        assert_eq!("this-filter", result);
    }

    #[test]
    fn test_render_with_custom_filter_lowercamelcase() {
        let mut tera = get_tera(Option::default());

        let result = tera
            .render_str("{{ \"Camel-case\" | lowercamelcase }}", &Context::default())
            .unwrap();

        assert_eq!("camelCase", result);
    }

    #[test]
    fn test_render_with_custom_filter_shoutykebabcase() {
        let mut tera = get_tera(Option::default());

        let result = tera
            .render_str("{{ \"kebabCase\" | shoutykebabcase }}", &Context::default())
            .unwrap();

        assert_eq!("KEBAB-CASE", result);
    }

    #[test]
    fn test_render_with_custom_filter_shoutysnakecase() {
        let mut tera = get_tera(Option::default());

        let result = tera
            .render_str("{{ \"snakeCase\" | shoutysnakecase }}", &Context::default())
            .unwrap();

        assert_eq!("SNAKE_CASE", result);
    }

    #[test]
    fn test_render_with_custom_filter_snakecase() {
        let mut tera = get_tera(Option::default());

        let result = tera
            .render_str("{{ \"snakeCase\" | snakecase }}", &Context::default())
            .unwrap();

        assert_eq!("snake_case", result);
    }

    #[test]
    fn test_render_with_custom_filter_uppercamelcase() {
        let mut tera = get_tera(Option::default());

        let result = tera
            .render_str("{{ \"CamelCase\" | uppercamelcase }}", &Context::default())
            .unwrap();

        assert_eq!("CamelCase", result);
    }
}
