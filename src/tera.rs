use std::collections::HashMap;
use std::path::{Path, PathBuf};

use eyre::{eyre, Result};
use heck::{
    ToKebabCase, ToLowerCamelCase, ToShoutyKebabCase, ToShoutySnakeCase, ToSnakeCase,
    ToUpperCamelCase,
};
use once_cell::sync::Lazy;
use tera::{Context, Tera, Value};

use crate::cmd::cmd;
use crate::{env, hash};

pub static BASE_CONTEXT: Lazy<Context> = Lazy::new(|| {
    let mut context = Context::new();
    context.insert("env", &*env::PRISTINE_ENV);
    if let Ok(dir) = env::current_dir() {
        context.insert("cwd", &dir);
    }
    context
});

const DEFAULT_HASH_DIGEST_FUNCTION: &str = "sha256";

fn file_digest(path: &Path, function: &str) -> Result<String> {
    match function {
        "sha256" => hash::file_hash_sha256(path),
        "blake3" => hash::file_hash_blake3(path),
        _ => Err(eyre!("hash function {} is not supported", function)),
    }
}

fn digest(s: &str, function: &str) -> Result<String> {
    match function {
        "sha256" => Ok(hash::sha256_digest(s)),
        "blake3" => Ok(hash::blake3_digest(s)),
        _ => Err(eyre!("hash function {} is not supported", function)),
    }
}

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
            let arch = if cfg!(target_arch = "x86_64") {
                "x64"
            } else if cfg!(target_arch = "aarch64") {
                "arm64"
            } else {
                env::consts::ARCH
            };
            Ok(Value::String(arch.to_string()))
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
            Ok(Value::String(env::consts::OS.to_string()))
        },
    );
    tera.register_function(
        "os_family",
        move |_args: &HashMap<String, Value>| -> tera::Result<Value> {
            Ok(Value::String(env::consts::FAMILY.to_string()))
        },
    );
    tera.register_filter(
        "hash_file",
        move |input: &Value, args: &HashMap<String, Value>| match input {
            Value::String(s) => {
                let path = Path::new(s);
                let function = args
                    .get("function")
                    .and_then(Value::as_str)
                    .unwrap_or(DEFAULT_HASH_DIGEST_FUNCTION);
                let mut hash = file_digest(path, function).unwrap();
                if let Some(len) = args.get("len").and_then(Value::as_u64) {
                    hash = hash.chars().take(len as usize).collect();
                }

                Ok(Value::String(hash))
            }
            _ => Err("hash input must be a string".into()),
        },
    );
    tera.register_filter(
        "hash",
        move |input: &Value, args: &HashMap<String, Value>| match input {
            Value::String(s) => {
                let function = args
                    .get("function")
                    .and_then(Value::as_str)
                    .unwrap_or(DEFAULT_HASH_DIGEST_FUNCTION);
                let mut hash = digest(s, function).unwrap();
                if let Some(len) = args.get("len").and_then(Value::as_u64) {
                    hash = hash.chars().take(len as usize).collect();
                }
                Ok(Value::String(hash))
            }
            _ => Err("hash input must be a string".into()),
        },
    );
    tera.register_function(
        "uuid",
        move |_args: &HashMap<String, Value>| -> tera::Result<Value> {
            let result = uuid::Uuid::new_v4().to_string();
            Ok(Value::String(result))
        },
    );
    tera.register_filter(
        "canonicalize",
        move |input: &Value, _args: &HashMap<String, Value>| match input {
            Value::String(s) => {
                let p = Path::new(s).canonicalize()?;
                Ok(Value::String(p.to_string_lossy().to_string()))
            }
            _ => Err("canonicalize input must be a string".into()),
        },
    );
    tera.register_filter(
        "dirname",
        move |input: &Value, _args: &HashMap<String, Value>| match input {
            Value::String(s) => {
                let p = Path::new(s).parent().unwrap();
                Ok(Value::String(p.to_string_lossy().to_string()))
            }
            _ => Err("dirname input must be a string".into()),
        },
    );
    tera.register_filter(
        "basename",
        move |input: &Value, _args: &HashMap<String, Value>| match input {
            Value::String(s) => {
                let p = Path::new(s).file_name().unwrap();
                Ok(Value::String(p.to_string_lossy().to_string()))
            }
            _ => Err("basename input must be a string".into()),
        },
    );
    tera.register_filter(
        "extname",
        move |input: &Value, _args: &HashMap<String, Value>| match input {
            Value::String(s) => {
                let p = Path::new(s).extension().unwrap();
                Ok(Value::String(p.to_string_lossy().to_string()))
            }
            _ => Err("extname input must be a string".into()),
        },
    );
    tera.register_filter(
        "file_stem",
        move |input: &Value, _args: &HashMap<String, Value>| match input {
            Value::String(s) => {
                let p = Path::new(s).file_stem().unwrap();
                Ok(Value::String(p.to_string_lossy().to_string()))
            }
            _ => Err("filename input must be a string".into()),
        },
    );
    tera.register_filter(
        "file_size",
        move |input: &Value, _args: &HashMap<String, Value>| match input {
            Value::String(s) => {
                let p = Path::new(s);
                let metadata = p.metadata()?;
                let size = metadata.len();
                Ok(Value::Number(size.into()))
            }
            _ => Err("file_size input must be a string".into()),
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
            _ => Err("last_modified input must be a string".into()),
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
        "dir",
        move |input: Option<&Value>, _args: &[Value]| match input {
            Some(Value::String(s)) => Ok(Path::new(s).is_dir()),
            _ => Err("is_dir input must be a string".into()),
        },
    );
    tera.register_tester(
        "file",
        move |input: Option<&Value>, _args: &[Value]| match input {
            Some(Value::String(s)) => Ok(Path::new(s).is_file()),
            _ => Err("is_file input must be a string".into()),
        },
    );
    tera.register_tester(
        "exists",
        move |input: Option<&Value>, _args: &[Value]| match input {
            Some(Value::String(s)) => Ok(Path::new(s).exists()),
            _ => Err("exists input must be a string".into()),
        },
    );

    tera
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test::reset;
    use insta::assert_snapshot;

    #[test]
    fn test_render_with_custom_function_arch() {
        reset();
        if cfg!(target_arch = "x86_64") {
            assert_eq!(render("{{arch()}}"), "x64");
        } else if cfg!(target_arch = "aarch64") {
            assert_eq!(render("{{arch()}}"), "arm64");
        } else {
            assert_eq!(render("{{arch()}}"), env::consts::ARCH);
        }
    }

    #[test]
    fn test_render_with_custom_function_num_cpus() {
        reset();
        let mut tera = get_tera(Option::default());

        let result = tera
            .render_str("{{ num_cpus() }}", &Context::default())
            .unwrap();

        let num = result.parse::<u32>().unwrap();
        assert!(num > 0);
    }

    #[test]
    fn test_render_with_custom_function_os() {
        reset();
        if cfg!(target_os = "linux") {
            assert_eq!(render("{{os()}}"), "linux");
        } else if cfg!(target_os = "macos") {
            assert_eq!(render("{{os()}}"), "macos");
        } else if cfg!(target_os = "windows") {
            assert_eq!(render("{{os()}}"), "windows");
        }
    }

    #[test]
    fn test_render_with_custom_function_os_family() {
        reset();
        if cfg!(target_family = "unix") {
            assert_eq!(render("{{os_family()}}"), "unix");
        } else if cfg!(target_os = "windows") {
            assert_eq!(render("{{os_family()}}"), "windows");
        }
    }

    #[test]
    fn test_render_with_custom_filter_quote() {
        reset();
        let mut tera = get_tera(Option::default());

        let result = tera
            .render_str("{{ \"quoted'str\" | quote }}", &Context::default())
            .unwrap();

        assert_eq!("'quoted\\'str'", result);
    }

    #[test]
    fn test_render_with_custom_filter_kebabcase() {
        reset();
        let mut tera = get_tera(Option::default());

        let result = tera
            .render_str("{{ \"thisFilter\" | kebabcase }}", &Context::default())
            .unwrap();

        assert_eq!("this-filter", result);
    }

    #[test]
    fn test_render_with_custom_filter_lowercamelcase() {
        reset();
        let mut tera = get_tera(Option::default());

        let result = tera
            .render_str("{{ \"Camel-case\" | lowercamelcase }}", &Context::default())
            .unwrap();

        assert_eq!("camelCase", result);
    }

    #[test]
    fn test_render_with_custom_filter_shoutykebabcase() {
        reset();
        let mut tera = get_tera(Option::default());

        let result = tera
            .render_str("{{ \"kebabCase\" | shoutykebabcase }}", &Context::default())
            .unwrap();

        assert_eq!("KEBAB-CASE", result);
    }

    #[test]
    fn test_render_with_custom_filter_shoutysnakecase() {
        reset();
        let mut tera = get_tera(Option::default());

        let result = tera
            .render_str("{{ \"snakeCase\" | shoutysnakecase }}", &Context::default())
            .unwrap();

        assert_eq!("SNAKE_CASE", result);
    }

    #[test]
    fn test_render_with_custom_filter_snakecase() {
        reset();
        let mut tera = get_tera(Option::default());

        let result = tera
            .render_str("{{ \"snakeCase\" | snakecase }}", &Context::default())
            .unwrap();

        assert_eq!("snake_case", result);
    }

    #[test]
    fn test_render_with_custom_filter_uppercamelcase() {
        reset();
        let mut tera = get_tera(Option::default());

        let result = tera
            .render_str("{{ \"CamelCase\" | uppercamelcase }}", &Context::default())
            .unwrap();

        assert_eq!("CamelCase", result);
    }

    #[test]
    fn test_hash() {
        reset();
        let s = render("{{ \"foo\" | hash(len=8) }}");
        assert_eq!(s, "2c26b46b");
    }

    #[test]
    fn test_hash_blake3() {
        reset();
        let s = render("{{ \"foo\" | hash(function=\"blake3\", len=8) }}");
        assert_eq!(s, "04e0bb39");
    }

    #[test]
    fn test_hash_file() {
        reset();
        let s = render("{{ \"../fixtures/shorthands.toml\" | hash_file(len=64) }}");
        assert_snapshot!(s, @"518349c5734814ff9a21ab8d00ed2da6464b1699910246e763a4e6d5feb139fa");
    }

    #[test]
    fn test_hash_file_blake3() {
        reset();
        let s = render(
            "{{ \"../fixtures/shorthands.toml\" | hash_file(function=\"blake3\", len=64) }}",
        );
        assert_snapshot!(s, @"ce17f44735ea2083038e61c4b291ed31593e6cf4d93f5dc147e97e62962ac4e6");
    }

    #[test]
    fn test_uuid() {
        reset();
        let s = render("{{ uuid() }}");
        assert_eq!(s.len(), 36);
    }

    #[test]
    fn test_dirname() {
        reset();
        let s = render(r#"{{ "a/b/c" | dirname }}"#);
        assert_eq!(s, "a/b");
    }

    #[test]
    fn test_basename() {
        reset();
        let s = render(r#"{{ "a/b/c" | basename }}"#);
        assert_eq!(s, "c");
    }

    #[test]
    fn test_extname() {
        reset();
        let s = render(r#"{{ "a/b/c.txt" | extname }}"#);
        assert_eq!(s, "txt");
    }

    #[test]
    fn test_file_stem() {
        reset();
        let s = render(r#"{{ "a/b/c.txt" | file_stem }}"#);
        assert_eq!(s, "c");
    }

    #[test]
    fn test_file_size() {
        reset();
        let s = render(r#"{{ "../fixtures/shorthands.toml" | file_size }}"#);
        assert_eq!(s, "48");
    }

    #[test]
    fn test_is_dir() {
        reset();
        let s = render(r#"{% set p = ".mise" %}{% if p is dir %} ok {% endif %}"#);
        assert_eq!(s.trim(), "ok");
    }

    #[test]
    fn test_is_file() {
        reset();
        let s = render(r#"{% set p = ".test-tool-versions" %}{% if p is file %} ok {% endif %}"#);
        assert_eq!(s.trim(), "ok");
    }

    #[test]
    fn test_exists() {
        reset();
        let s = render(r#"{% set p = ".test-tool-versions" %}{% if p is exists %} ok {% endif %}"#);
        assert_eq!(s.trim(), "ok");
    }

    fn render(s: &str) -> String {
        let mut tera = get_tera(Option::default());

        tera.render_str(s, &Context::default()).unwrap()
    }
}
