use std::collections::HashMap;
use std::path::{Path, PathBuf};

use heck::{
    ToKebabCase, ToLowerCamelCase, ToShoutyKebabCase, ToShoutySnakeCase, ToSnakeCase,
    ToUpperCamelCase,
};
use once_cell::sync::Lazy;
use rand::seq::SliceRandom;
use rand::thread_rng;
use tera::{Context, Tera, Value};
use versions::{Requirement, Versioning};

use crate::cmd::cmd;
use crate::{env, hash};

pub static BASE_CONTEXT: Lazy<Context> = Lazy::new(|| {
    let mut context = Context::new();
    context.insert("env", &*env::PRISTINE_ENV);
    context.insert("mise_bin", &*env::MISE_BIN);
    context.insert("mise_pid", &*env::MISE_PID);
    if let Ok(dir) = env::current_dir() {
        context.insert("cwd", &dir);
    }
    context.insert("xdg_cache_home", &*env::XDG_CACHE_HOME);
    context.insert("xdg_config_home", &*env::XDG_CONFIG_HOME);
    context.insert("xdg_data_home", &*env::XDG_DATA_HOME);
    context.insert("xdg_state_home", &*env::XDG_STATE_HOME);
    context
});

static TERA: Lazy<Tera> = Lazy::new(|| {
    let mut tera = Tera::default();
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
    tera.register_function(
        "choice",
        move |args: &HashMap<String, Value>| -> tera::Result<Value> {
            match args.get("n") {
                Some(Value::Number(n)) => {
                    let n = n.as_u64().unwrap();
                    match args.get("alphabet") {
                        Some(Value::String(alphabet)) => {
                            let alphabet = alphabet.chars().collect::<Vec<char>>();
                            let mut rng = thread_rng();
                            let result =
                                (0..n).map(|_| alphabet.choose(&mut rng).unwrap()).collect();
                            Ok(Value::String(result))
                        }
                        _ => Err("choice alphabet must be an string".into()),
                    }
                }
                _ => Err("choice n must be an integer".into()),
            }
        },
    );
    tera.register_filter(
        "hash_file",
        move |input: &Value, args: &HashMap<String, Value>| match input {
            Value::String(s) => {
                let path = Path::new(s);
                let mut hash = hash::file_hash_sha256(path).unwrap();
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
                let mut hash = hash::hash_sha256_to_str(s);
                if let Some(len) = args.get("len").and_then(Value::as_u64) {
                    hash = hash.chars().take(len as usize).collect();
                }
                Ok(Value::String(hash))
            }
            _ => Err("hash input must be a string".into()),
        },
    );
    // TODO: add `absolute` feature.
    // wait until #![feature(absolute_path)] hits Rust stable release channel
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
    tera.register_tester(
        "semver_matching",
        move |input: Option<&Value>, args: &[Value]| match input {
            Some(Value::String(version)) => match args.first() {
                Some(Value::String(requirement)) => {
                    println!("{}", requirement);
                    let result = Requirement::new(requirement)
                        .unwrap()
                        .matches(&Versioning::new(version).unwrap());
                    Ok(result)
                }
                _ => Err("semver_matching argument must be a string".into()),
            },
            _ => Err("semver_matching input must be a string".into()),
        },
    );

    tera
});

pub fn get_tera(dir: Option<&Path>) -> Tera {
    let mut tera = TERA.clone();
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

    tera
}

#[cfg(test)]
mod tests {
    use super::*;
    use insta::assert_snapshot;
    use pretty_assertions::assert_str_eq;

    #[test]
    fn test_config_root() {
        assert_eq!(render("{{config_root}}"), "/");
    }

    #[test]
    fn test_cwd() {
        assert_eq!(render("{{cwd}}"), "/");
    }

    #[test]
    fn test_mise_bin() {
        assert_eq!(
            render("{{mise_bin}}"),
            env::current_exe()
                .unwrap()
                .into_os_string()
                .into_string()
                .unwrap()
        );
    }

    #[test]
    fn test_mise_pid() {
        let s = render("{{mise_pid}}");
        let pid = s.trim().parse::<u32>().unwrap();
        assert!(pid > 0);
    }

    #[test]
    fn test_xdg_cache_home() {
        let s = render("{{xdg_cache_home}}");
        assert_str_eq!(s, env::XDG_CACHE_HOME.to_string_lossy());
    }

    #[test]
    #[cfg(unix)]
    fn test_xdg_config_home() {
        let s = render("{{xdg_config_home}}");
        assert!(s.ends_with("/.config")); // test dir is not deterministic
    }

    #[test]
    #[cfg(unix)]
    fn test_xdg_data_home() {
        let s = render("{{xdg_data_home}}");
        assert!(s.ends_with("/.local/share")); // test dir is not deterministic
    }

    #[test]
    #[cfg(unix)]
    fn test_xdg_state_home() {
        let s = render("{{xdg_state_home}}");
        assert!(s.ends_with("/.local/state")); // test dir is not deterministic
    }

    #[test]
    fn test_arch() {
        if cfg!(target_arch = "x86_64") {
            assert_eq!(render("{{arch()}}"), "x64");
        } else if cfg!(target_arch = "aarch64") {
            assert_eq!(render("{{arch()}}"), "arm64");
        } else {
            assert_eq!(render("{{arch()}}"), env::consts::ARCH);
        }
    }

    #[test]
    fn test_num_cpus() {
        let s = render("{{ num_cpus() }}");
        let num = s.parse::<u32>().unwrap();
        assert!(num > 0);
    }

    #[test]
    fn test_os() {
        if cfg!(target_os = "linux") {
            assert_eq!(render("{{os()}}"), "linux");
        } else if cfg!(target_os = "macos") {
            assert_eq!(render("{{os()}}"), "macos");
        } else if cfg!(target_os = "windows") {
            assert_eq!(render("{{os()}}"), "windows");
        }
    }

    #[test]
    fn test_os_family() {
        if cfg!(target_family = "unix") {
            assert_eq!(render("{{os_family()}}"), "unix");
        } else if cfg!(target_os = "windows") {
            assert_eq!(render("{{os_family()}}"), "windows");
        }
    }

    #[test]
    fn test_choice() {
        let result = render("{{choice(n=8, alphabet=\"abcdefgh\")}}");
        assert_eq!(result.trim().len(), 8);
    }

    #[test]
    fn test_quote() {
        let s = render("{{ \"quoted'str\" | quote }}");
        assert_eq!(s, "'quoted\\'str'");
    }

    #[test]
    fn test_kebabcase() {
        let s = render("{{ \"thisFilter\" | kebabcase }}");
        assert_eq!(s, "this-filter");
    }

    #[test]
    fn test_lowercamelcase() {
        let s = render("{{ \"Camel-case\" | lowercamelcase }}");
        assert_eq!(s, "camelCase");
    }

    #[test]
    fn test_shoutykebabcase() {
        let s = render("{{ \"kebabCase\" | shoutykebabcase }}");
        assert_eq!(s, "KEBAB-CASE");
    }

    #[test]
    fn test_shoutysnakecase() {
        let s = render("{{ \"snakeCase\" | shoutysnakecase }}");
        assert_eq!(s, "SNAKE_CASE");
    }

    #[test]
    fn test_snakecase() {
        let s = render("{{ \"snakeCase\" | snakecase }}");
        assert_eq!(s, "snake_case");
    }

    #[test]
    fn test_uppercamelcase() {
        let s = render("{{ \"CamelCase\" | uppercamelcase }}");
        assert_eq!(s, "CamelCase");
    }

    #[test]
    fn test_hash() {
        let s = render("{{ \"foo\" | hash(len=8) }}");
        assert_eq!(s, "2c26b46b");
    }

    #[test]
    #[cfg(unix)]
    fn test_hash_file() {
        let s = render("{{ \"../fixtures/shorthands.toml\" | hash_file(len=64) }}");
        assert_snapshot!(s, @"518349c5734814ff9a21ab8d00ed2da6464b1699910246e763a4e6d5feb139fa");
    }

    #[test]
    #[cfg(unix)]
    fn test_canonicalize() {
        let s = render("{{ \"../fixtures/shorthands.toml\" | canonicalize }}");
        assert!(s.ends_with("/fixtures/shorthands.toml")); // test dir is not deterministic
    }

    #[test]
    fn test_dirname() {
        let s = render(r#"{{ "a/b/c" | dirname }}"#);
        assert_eq!(s, "a/b");
    }

    #[test]
    fn test_basename() {
        let s = render(r#"{{ "a/b/c" | basename }}"#);
        assert_eq!(s, "c");
    }

    #[test]
    fn test_extname() {
        let s = render(r#"{{ "a/b/c.txt" | extname }}"#);
        assert_eq!(s, "txt");
    }

    #[test]
    fn test_file_stem() {
        let s = render(r#"{{ "a/b/c.txt" | file_stem }}"#);
        assert_eq!(s, "c");
    }

    #[test]
    #[cfg(unix)]
    fn test_file_size() {
        let s = render(r#"{{ "../fixtures/shorthands.toml" | file_size }}"#);
        assert_eq!(s, "48");
    }

    #[test]
    fn test_last_modified() {
        let s = render(r#"{{ "../fixtures/shorthands.toml" | last_modified }}"#);
        let timestamp = s.parse::<u64>().unwrap();
        assert!((1725000000..=2725000000).contains(&timestamp));
    }

    #[test]
    #[cfg(unix)]
    fn test_join_path() {
        let s = render(r#"{{ ["..", "fixtures", "shorthands.toml"] | join_path }}"#);
        assert_eq!(s, "../fixtures/shorthands.toml");
    }

    #[test]
    fn test_is_dir() {
        let s = render(r#"{% set p = ".mise" %}{% if p is dir %} ok {% endif %}"#);
        assert_eq!(s.trim(), "ok");
    }

    #[test]
    fn test_is_file() {
        let s = render(r#"{% set p = ".test-tool-versions" %}{% if p is file %} ok {% endif %}"#);
        assert_eq!(s.trim(), "ok");
    }

    #[test]
    fn test_exists() {
        let s = render(r#"{% set p = ".test-tool-versions" %}{% if p is exists %} ok {% endif %}"#);
        assert_eq!(s.trim(), "ok");
    }

    #[test]
    fn test_semver_matching() {
        let s = render(
            r#"{% set p = "1.10.2" %}{% if p is semver_matching("^1.10.0") %} ok {% endif %}"#,
        );
        assert_eq!(s.trim(), "ok");
    }

    fn render(s: &str) -> String {
        let config_root = Path::new("/");
        let mut tera_ctx = BASE_CONTEXT.clone();
        tera_ctx.insert("config_root", &config_root);
        tera_ctx.insert("cwd", "/");
        let mut tera = get_tera(Option::from(config_root));
        tera.render_str(s, &tera_ctx).unwrap()
    }
}
