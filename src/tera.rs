use std::iter::once;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use heck::{
    ToKebabCase, ToLowerCamelCase, ToShoutyKebabCase, ToShoutySnakeCase, ToSnakeCase,
    ToUpperCamelCase,
};
use path_absolutize::Absolutize;
use rand::prelude::*;
use std::sync::LazyLock as Lazy;
use tera::{Context, Kwargs, State, Tera, TeraResult, Value};
use versions::{Requirement, Versioning};

use crate::cache::CacheManagerBuilder;
use crate::cmd::cmd;
use crate::config::Settings;
use crate::env_diff::EnvMap;
use crate::file::strip_shims_from_path;
use crate::{dirs, duration, env, hash};

/// Global tracker for files accessed during tera template rendering.
/// Functions like `read_file`, `hash_file`, `file_size`, and `last_modified`
/// push paths here so that hook-env can watch them for changes.
static TERA_ACCESSED_FILES: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

fn track_tera_file(path: &Path) {
    if let Ok(mut files) = TERA_ACCESSED_FILES.lock() {
        files.push(path.to_path_buf());
    }
}

/// Take all tracked files, clearing the global list.
pub fn take_tera_accessed_files() -> Vec<PathBuf> {
    let mut files = TERA_ACCESSED_FILES
        .lock()
        .map(|mut f| std::mem::take(&mut *f))
        .unwrap_or_default();
    files.sort();
    files.dedup();
    files
}

/// Fast marker check for Tera 1.x syntax.
///
/// Tera 1.20.1's grammar starts every variable, tag, and comment block with
/// `{{`, `{%`, or `{#` respectively, including whitespace-trimmed forms like
/// `{{-`, `{%-`, and `{#-`.
pub fn contains_template_syntax(input: &str) -> bool {
    input.contains("{{") || input.contains("{%") || input.contains("{#")
}

pub fn render_str(tera: &mut Tera, input: &str, context: &Context) -> TeraResult<String> {
    tera.render_str(input, context, false)
}

fn tera_err(message: impl ToString) -> tera::Error {
    tera::Error::message(message)
}

pub static BASE_CONTEXT: Lazy<Context> = Lazy::new(|| {
    let mut context = Context::new();
    context.insert("env", &*env::PRISTINE_ENV);
    context.insert("mise_bin", &*env::MISE_BIN);
    context.insert("mise_pid", &*env::MISE_PID);
    if !(*env::MISE_ENV).is_empty() {
        context.insert("mise_env", &*env::MISE_ENV);
    }
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
        move |args: Kwargs, _: &State| -> TeraResult<Value> {
            let arch = if cfg!(target_arch = "x86_64") {
                "x64"
            } else if cfg!(target_arch = "aarch64") {
                "arm64"
            } else {
                env::consts::ARCH
            };
            // Check if there's a remap for this arch
            if let Some(remapped) = args.get::<&str>(arch)? {
                return Ok(Value::from(remapped));
            }
            Ok(Value::from(arch))
        },
    );
    tera.register_function(
        "num_cpus",
        move |_: Kwargs, _: &State| -> TeraResult<Value> {
            let num = num_cpus::get();
            Ok(Value::from(num.to_string()))
        },
    );
    tera.register_function("os", move |args: Kwargs, _: &State| -> TeraResult<Value> {
        let os = env::consts::OS;
        // Check if there's a remap for this OS
        if let Some(remapped) = args.get::<&str>(os)? {
            return Ok(Value::from(remapped));
        }
        Ok(Value::from(os))
    });
    tera.register_function(
        "os_family",
        move |_: Kwargs, _: &State| -> TeraResult<Value> { Ok(Value::from(env::consts::FAMILY)) },
    );
    tera.register_function(
        "choice",
        move |args: Kwargs, _: &State| -> TeraResult<Value> {
            let n = args.must_get::<u64>("n")?;
            let alphabet = args.must_get::<&str>("alphabet")?;
            let alphabet = alphabet.chars().collect::<Vec<char>>();
            let mut rng = rand::rng();
            let result: String = (0..n)
                .map(|_| *alphabet.choose(&mut rng).unwrap())
                .collect();
            Ok(Value::from(result))
        },
    );
    tera.register_function(
        "haiku",
        move |args: Kwargs, _: &State| -> TeraResult<Value> {
            let words = args.get::<u64>("words")?.unwrap_or(2).max(1) as usize;
            let separator = args.get::<&str>("separator")?.unwrap_or("-");
            let digits = args.get::<u64>("digits")?.unwrap_or(2) as usize;

            let result = xx::rand::haiku(&xx::rand::HaikuOptions {
                words,
                separator,
                digits,
            });

            Ok(Value::from(result))
        },
    );
    tera.register_filter(
        "hash_file",
        move |s: &str, args: Kwargs, _: &State| -> TeraResult<Value> {
            let path = Path::new(s);
            track_tera_file(path);
            let mut hash = hash::file_hash_blake3(path, None).unwrap();
            if let Some(len) = args.get::<u64>("len")? {
                hash = hash.chars().take(len as usize).collect();
            }
            Ok(Value::from(hash))
        },
    );
    tera.register_filter(
        "hash",
        move |s: &str, args: Kwargs, _: &State| -> TeraResult<Value> {
            // Get the algorithm, default to sha256
            let algorithm = args.get::<&str>("algorithm")?.unwrap_or("sha256");

            let mut hash = match algorithm {
                "sha256" => hash::hash_sha256_to_str(s),
                "blake3" => hash::hash_blake3_to_str(s),
                _ => return Err(tera_err(format!("unknown hash algorithm: {algorithm}"))),
            };

            if let Some(len) = args.get::<u64>("len")? {
                hash = hash.chars().take(len as usize).collect();
            }
            Ok(Value::from(hash))
        },
    );
    tera.register_filter(
        "absolute",
        move |s: &str, _: Kwargs, _: &State| -> TeraResult<Value> {
            let p = Path::new(s).absolutize()?;
            Ok(Value::from(p.to_string_lossy().to_string()))
        },
    );
    tera.register_filter(
        "canonicalize",
        move |s: &str, _: Kwargs, _: &State| -> TeraResult<Value> {
            let p = Path::new(s).canonicalize()?;
            Ok(Value::from(p.to_string_lossy().to_string()))
        },
    );
    // Helper to create path filters that handle empty strings gracefully
    fn path_filter<F>(input: &str, _name: &'static str, f: F) -> TeraResult<Value>
    where
        F: FnOnce(&Path) -> Option<String>,
    {
        if input.is_empty() {
            Ok(Value::from(String::new()))
        } else {
            Ok(Value::from(f(Path::new(input)).unwrap_or_default()))
        }
    }
    tera.register_filter("dirname", move |input: &str, _: Kwargs, _: &State| {
        path_filter(input, "dirname", |p| {
            p.parent().map(|p| p.to_string_lossy().to_string())
        })
    });
    tera.register_filter("basename", move |input: &str, _: Kwargs, _: &State| {
        path_filter(input, "basename", |p| {
            p.file_name().map(|p| p.to_string_lossy().to_string())
        })
    });
    tera.register_filter("extname", move |input: &str, _: Kwargs, _: &State| {
        path_filter(input, "extname", |p| {
            p.extension().map(|p| p.to_string_lossy().to_string())
        })
    });
    tera.register_filter("file_stem", move |input: &str, _: Kwargs, _: &State| {
        path_filter(input, "file_stem", |p| {
            p.file_stem().map(|p| p.to_string_lossy().to_string())
        })
    });
    tera.register_filter(
        "file_size",
        move |s: &str, _: Kwargs, _: &State| -> TeraResult<Value> {
            let p = Path::new(s);
            track_tera_file(p);
            let metadata = p.metadata()?;
            let size = metadata.len();
            Ok(Value::from(size))
        },
    );
    tera.register_filter(
        "last_modified",
        move |s: &str, _: Kwargs, _: &State| -> TeraResult<Value> {
            let p = Path::new(s);
            track_tera_file(p);
            let metadata = p.metadata()?;
            let modified = metadata.modified()?;
            let modified = modified.duration_since(std::time::UNIX_EPOCH).unwrap();
            Ok(Value::from(modified.as_secs()))
        },
    );
    tera.register_filter(
        "join_path",
        move |arr: &[Value], _: Kwargs, _: &State| -> TeraResult<Value> {
            arr.iter()
                .map(Value::as_str)
                .collect::<Option<PathBuf>>()
                .ok_or_else(|| tera_err("join_path input must be an array of strings"))
                .map(|p| Value::from(p.to_string_lossy().to_string()))
        },
    );
    tera.register_filter(
        "quote",
        move |s: &str, _: Kwargs, _: &State| -> TeraResult<Value> {
            let result = format!("'{}'", s.replace("'", "\\'"));

            Ok(Value::from(result))
        },
    );
    tera.register_filter("as_str", move |value: Value, _: Kwargs, _: &State| {
        value.to_string()
    });
    tera.register_filter("kebabcase", move |s: &str, _: Kwargs, _: &State| {
        Value::from(s.to_kebab_case())
    });
    tera.register_filter("lowercamelcase", move |s: &str, _: Kwargs, _: &State| {
        Value::from(s.to_lower_camel_case())
    });
    tera.register_filter("shoutykebabcase", move |s: &str, _: Kwargs, _: &State| {
        Value::from(s.to_shouty_kebab_case())
    });
    tera.register_filter("shoutysnakecase", move |s: &str, _: Kwargs, _: &State| {
        Value::from(s.to_shouty_snake_case())
    });
    tera.register_filter("snakecase", move |s: &str, _: Kwargs, _: &State| {
        Value::from(s.to_snake_case())
    });
    tera.register_filter("uppercamelcase", move |s: &str, _: Kwargs, _: &State| {
        Value::from(s.to_upper_camel_case())
    });
    tera.register_test("dir", move |s: &str, _: Kwargs, _: &State| {
        Path::new(s).is_dir()
    });
    tera.register_test("file", move |s: &str, _: Kwargs, _: &State| {
        Path::new(s).is_file()
    });
    tera.register_test("exists", move |s: &str, _: Kwargs, _: &State| {
        Path::new(s).exists()
    });
    tera.register_test(
        "semver_matching",
        move |version: &str, args: Kwargs, _: &State| -> TeraResult<bool> {
            let requirement = args.must_get::<&str>("requirement")?;
            let result = Requirement::new(requirement)
                .unwrap()
                .matches(&Versioning::new(version).unwrap());
            Ok(result)
        },
    );

    tera
});

/// Returns a Tera instance for use during early initialization (miserc loading).
/// This is a plain clone of the global `TERA` static. `exec` and `read_file` are absent
/// because they are only registered in [`get_tera`], not in `TERA` itself — so they
/// cannot accidentally become available here if `TERA` changes in the future.
pub fn get_miserc_tera() -> Tera {
    TERA.clone()
}

pub fn get_tera(dir: Option<&Path>) -> Tera {
    let mut tera = TERA.clone();
    let dir = dir.map(PathBuf::from);
    tera.register_function("exec", tera_exec(dir.clone(), env::PRISTINE_ENV.clone()));
    tera.register_function("read_file", tera_read_file(dir));

    tera
}

/// Like [`get_tera`] but with `os()` and `arch()` bound to an explicit target
/// platform instead of the current host. Used by cross-platform `mise lock` to
/// render URL/checksum templates for platforms other than the one mise runs on.
///
/// `os` should be a platform os name (e.g. "macos", "linux", "windows") and
/// `arch` a platform arch name (e.g. "x64", "arm64"), matching the values
/// returned by the host-bound functions. Remap arguments such as
/// `os(macos="darwin")` and `arch(x64="amd64")` keep the same semantics.
pub fn get_tera_for_target(dir: Option<&Path>, os: &str, arch: &str) -> Tera {
    let mut tera = get_tera(dir);

    // os_family() must follow the target too, not the host.
    let family = if os == "windows" { "windows" } else { "unix" };

    let os = os.to_string();
    tera.register_function("os", move |args: Kwargs, _: &State| -> TeraResult<Value> {
        if let Some(remapped) = args.get::<&str>(&os)? {
            return Ok(Value::from(remapped));
        }
        Ok(Value::from(os.clone()))
    });

    let arch = arch.to_string();
    tera.register_function(
        "arch",
        move |args: Kwargs, _: &State| -> TeraResult<Value> {
            if let Some(remapped) = args.get::<&str>(&arch)? {
                return Ok(Value::from(remapped));
            }
            Ok(Value::from(arch.clone()))
        },
    );

    tera.register_function(
        "os_family",
        move |_: Kwargs, _: &State| -> TeraResult<Value> { Ok(Value::from(family)) },
    );

    tera
}

/// Like [`get_tera`] but with `os()` and `arch()` rewritten to re-emit
/// themselves as template fragments (e.g. `os(macos="darwin")` renders back to
/// the literal `{{ os(macos="darwin") }}`).
///
/// Used when rendering tool option templates at config-load time: env/vars are
/// resolved, but `os()`/`arch()` are deferred so the backend can re-render them
/// for the host at install time or for an arbitrary target during cross-platform
/// `mise lock`. Mirrors how `{{ version }}` is preserved via a placeholder.
pub fn get_tera_preserving_os_arch(dir: Option<&Path>) -> Tera {
    let mut tera = get_tera(dir);
    tera.register_function("os", reemit_template_fn("os"));
    tera.register_function("arch", reemit_template_fn("arch"));
    // os_family() must be deferred too: it derives from the target OS, so
    // resolving it against the host here would bake e.g. "unix" into a template
    // that is later rendered for a windows target.
    tera.register_function("os_family", reemit_template_fn("os_family"));
    tera
}

fn reemit_template_fn(name: &'static str) -> impl Fn(Kwargs, &State) -> TeraResult<Value> {
    move |args: Kwargs, _: &State| {
        let args = args.deserialize::<std::collections::BTreeMap<String, serde_json::Value>>()?;
        let mut parts: Vec<String> = args
            .iter()
            .filter_map(|(k, v)| reemit_arg_literal(v).map(|lit| format!("{k}={lit}")))
            .collect();
        let rendered = if parts.is_empty() {
            format!("{{{{ {name}() }}}}")
        } else {
            parts.sort();
            format!("{{{{ {name}({}) }}}}", parts.join(", "))
        };
        Ok(Value::from(rendered))
    }
}

/// Render a Tera function argument value back into its template literal form so
/// it round-trips through re-emission. Tera string literals are literal (no
/// escape sequences), so strings are simply re-quoted; numbers/bools render
/// natively; other types are dropped (os()/arch() ignore non-string remaps).
fn reemit_arg_literal(v: &serde_json::Value) -> Option<String> {
    match v {
        serde_json::Value::String(s) => Some(format!("\"{s}\"")),
        serde_json::Value::Number(n) => Some(n.to_string()),
        serde_json::Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

pub fn tera_exec(
    dir: Option<PathBuf>,
    env: EnvMap,
) -> impl Fn(Kwargs, &State) -> TeraResult<Value> {
    move |args: Kwargs, _: &State| -> TeraResult<Value> {
        let cache = args.get::<String>("cache_key")?;
        let cache_duration = match args.get::<String>("cache_duration")? {
            Some(duration) => match duration::parse_duration(&duration) {
                Ok(duration) => Some(duration),
                Err(e) => return Err(tera_err(format!("exec cache_duration: {e}"))),
            },
            None => None,
        };
        match args.get::<String>("command")? {
            Some(command) => {
                let shell = Settings::get()
                    .default_inline_shell()
                    .map_err(|e| tera_err(e.to_string()))?;
                let args = shell
                    .iter()
                    .skip(1)
                    .chain(once(&command))
                    .collect::<Vec<&String>>();
                // Strip mise shims from PATH to prevent infinite recursion
                // when the command (e.g. `gh auth token`) is a mise-managed
                // tool. Without this, the shim re-enters mise, which may
                // evaluate the same template again indefinitely.
                let mut env_no_shims = env.clone();
                if let Some(path_val) = env_no_shims.get(&*env::PATH_KEY).cloned() {
                    env_no_shims
                        .insert(env::PATH_KEY.to_string(), strip_shims_from_path(&path_val));
                }
                // Run the command capturing stdout (duct `.read()` trims one
                // trailing newline). On Windows a `cmd /c` command is passed
                // verbatim so inner double quotes survive (#9355); non-cmd shells
                // and Unix keep the duct path. Returns eyre::Result so it can feed
                // the cache's get_or_try_init directly.
                let run_once = || -> eyre::Result<String> {
                    #[cfg(windows)]
                    {
                        if let Some(mut c) =
                            crate::path::cmd_verbatim_command(&shell[0], &shell[1..], &command)
                        {
                            c.env_clear();
                            c.envs(env_no_shims.iter());
                            if let Some(dir) = &dir {
                                c.current_dir(dir);
                            }
                            let out = c.output()?;
                            if !out.status.success() {
                                eyre::bail!(
                                    "exec command failed: {}",
                                    String::from_utf8_lossy(&out.stderr)
                                );
                            }
                            // Strict UTF-8 like duct's `.read()` (which uses
                            // read_to_string and errors on invalid UTF-8 rather
                            // than substituting U+FFFD replacement characters).
                            let mut s = String::from_utf8(out.stdout)?;
                            // Match duct's `.read()`: strip *all* trailing \r and
                            // \n, so the cmd path returns byte-identical output to
                            // the non-cmd (duct) path on the same machine.
                            while s.ends_with('\n') || s.ends_with('\r') {
                                s.pop();
                            }
                            return Ok(s);
                        }
                    }
                    let mut expr: duct::Expression = cmd(&shell[0], args).full_env(&env_no_shims);
                    if let Some(dir) = &dir {
                        expr = expr.dir(dir);
                    }
                    Ok(expr.read()?)
                };
                let result = if cache.is_some() || cache_duration.is_some() {
                    let cachehash = hash::hash_blake3_to_str(
                        &(dir
                            .as_ref()
                            .map(|d| d.to_string_lossy().to_string())
                            .unwrap_or_default()
                            + &command),
                    )[..8]
                        .to_string();
                    let mut cacheman =
                        CacheManagerBuilder::new(dirs::CACHE.join("exec").join(cachehash));
                    if let Some(cache) = cache {
                        cacheman = cacheman.with_cache_key(cache);
                    }
                    if let Some(cache_duration) = cache_duration {
                        cacheman = cacheman.with_fresh_duration(Some(cache_duration));
                    }
                    let cache = cacheman.build();
                    match cache.get_or_try_init(run_once) {
                        Ok(result) => result.clone(),
                        Err(e) => return Err(tera_err(format!("exec command: {e}"))),
                    }
                } else {
                    run_once().map_err(|e| tera_err(format!("exec command: {e}")))?
                };
                Ok(Value::from(result))
            }
            _ => Err(tera_err("exec command must be a string")),
        }
    }
}

pub fn tera_read_file(dir: Option<PathBuf>) -> impl Fn(Kwargs, &State) -> TeraResult<Value> {
    move |args: Kwargs, _: &State| -> TeraResult<Value> {
        match args.get::<String>("path")? {
            Some(path_str) => {
                let path = if let Some(ref base_dir) = dir {
                    // Resolve relative to config directory
                    base_dir.join(&path_str)
                } else {
                    // Use path as-is if no directory context
                    PathBuf::from(&path_str)
                };

                track_tera_file(&path);
                match std::fs::read_to_string(&path) {
                    Ok(contents) => Ok(Value::from(contents)),
                    Err(e) => Err(tera_err(format!(
                        "Failed to read file '{}': {}",
                        path.display(),
                        e
                    ))),
                }
            }
            _ => Err(tera_err("read_file path must be a string")),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::config::Config;

    use super::*;
    use pretty_assertions::assert_str_eq;

    #[tokio::test]
    async fn test_config_root() {
        let _config = Config::get().await.unwrap();
        assert_eq!(render("{{config_root}}"), "/");
    }

    #[tokio::test]
    async fn test_mise_env() {
        let _config = Config::get().await.unwrap();
        assert_eq!(render("{% if mise_env %}{{mise_env}}{% endif %}"), "");
    }

    #[tokio::test]
    async fn test_cwd() {
        let _config = Config::get().await.unwrap();
        assert_eq!(render("{{cwd}}"), "/");
    }

    #[tokio::test]
    async fn test_mise_bin() {
        let _config = Config::get().await.unwrap();
        assert_eq!(
            render("{{mise_bin}}"),
            env::current_exe()
                .unwrap()
                .into_os_string()
                .into_string()
                .unwrap()
        );
    }

    #[tokio::test]
    async fn test_mise_pid() {
        let _config = Config::get().await.unwrap();
        let s = render("{{mise_pid}}");
        let pid = s.trim().parse::<u32>().unwrap();
        assert!(pid > 0);
    }

    #[tokio::test]
    async fn test_xdg_cache_home() {
        let _config = Config::get().await.unwrap();
        let s = render("{{xdg_cache_home}}");
        assert_str_eq!(s, env::XDG_CACHE_HOME.to_string_lossy());
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn test_xdg_config_home() {
        let _config = Config::get().await.unwrap();
        let s = render("{{xdg_config_home}}");
        assert!(s.ends_with("/.config")); // test dir is not deterministic
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn test_xdg_data_home() {
        let _config = Config::get().await.unwrap();
        let s = render("{{xdg_data_home}}");
        assert!(s.ends_with("/.local/share")); // test dir is not deterministic
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn test_xdg_state_home() {
        let _config = Config::get().await.unwrap();
        let s = render("{{xdg_state_home}}");
        assert!(s.ends_with("/.local/state")); // test dir is not deterministic
    }

    #[tokio::test]
    async fn test_arch() {
        let _config = Config::get().await.unwrap();
        if cfg!(target_arch = "x86_64") {
            assert_eq!(render("{{arch()}}"), "x64");
        } else if cfg!(target_arch = "aarch64") {
            assert_eq!(render("{{arch()}}"), "arm64");
        } else {
            assert_eq!(render("{{arch()}}"), env::consts::ARCH);
        }
    }

    #[tokio::test]
    async fn test_num_cpus() {
        let _config = Config::get().await.unwrap();
        let s = render("{{ num_cpus() }}");
        let num = s.parse::<u32>().unwrap();
        assert!(num > 0);
    }

    #[tokio::test]
    async fn test_os() {
        let _config = Config::get().await.unwrap();
        if cfg!(target_os = "linux") {
            assert_eq!(render("{{os()}}"), "linux");
        } else if cfg!(target_os = "macos") {
            assert_eq!(render("{{os()}}"), "macos");
        } else if cfg!(target_os = "windows") {
            assert_eq!(render("{{os()}}"), "windows");
        }
    }

    #[tokio::test]
    async fn test_os_family() {
        let _config = Config::get().await.unwrap();
        if cfg!(target_family = "unix") {
            assert_eq!(render("{{os_family()}}"), "unix");
        } else if cfg!(target_os = "windows") {
            assert_eq!(render("{{os_family()}}"), "windows");
        }
    }

    #[tokio::test]
    async fn test_choice() {
        let _config = Config::get().await.unwrap();
        let result = render("{{choice(n=8, alphabet=\"abcdefgh\")}}");
        assert_eq!(result.trim().len(), 8);
    }

    #[tokio::test]
    async fn test_haiku() {
        let _config = Config::get().await.unwrap();
        // Default: 2 words + number
        let result = render("{{haiku()}}");
        let parts: Vec<&str> = result.split('-').collect();
        assert_eq!(parts.len(), 3);
        assert!(!parts[0].is_empty());
        assert!(!parts[1].is_empty());
        assert!(parts[2].parse::<u32>().is_ok());

        // Custom: 3 words, no digits, underscore separator
        let result = render("{{haiku(words=3, digits=0, separator=\"_\")}}");
        let parts: Vec<&str> = result.split('_').collect();
        assert_eq!(parts.len(), 3);
        assert!(parts.iter().all(|p| p.parse::<u32>().is_err())); // no numbers
    }

    #[tokio::test]
    async fn test_quote() {
        let _config = Config::get().await.unwrap();
        let s = render("{{ \"quoted'str\" | quote }}");
        assert_eq!(s, "'quoted\\'str'");
    }

    #[tokio::test]
    async fn test_as_str() {
        let _config = Config::get().await.unwrap();
        assert_eq!(render("{{ true | as_str }}"), "true");
        assert_eq!(render("{{ \"hello\" | as_str }}"), "hello");
    }

    #[tokio::test]
    async fn test_kebabcase() {
        let _config = Config::get().await.unwrap();
        let s = render("{{ \"thisFilter\" | kebabcase }}");
        assert_eq!(s, "this-filter");
    }

    #[tokio::test]
    async fn test_lowercamelcase() {
        let _config = Config::get().await.unwrap();
        let s = render("{{ \"Camel-case\" | lowercamelcase }}");
        assert_eq!(s, "camelCase");
    }

    #[tokio::test]
    async fn test_shoutykebabcase() {
        let _config = Config::get().await.unwrap();
        let s = render("{{ \"kebabCase\" | shoutykebabcase }}");
        assert_eq!(s, "KEBAB-CASE");
    }

    #[tokio::test]
    async fn test_shoutysnakecase() {
        let _config = Config::get().await.unwrap();
        let s = render("{{ \"snakeCase\" | shoutysnakecase }}");
        assert_eq!(s, "SNAKE_CASE");
    }

    #[tokio::test]
    async fn test_snakecase() {
        let _config = Config::get().await.unwrap();
        let s = render("{{ \"snakeCase\" | snakecase }}");
        assert_eq!(s, "snake_case");
    }

    #[tokio::test]
    async fn test_uppercamelcase() {
        let _config = Config::get().await.unwrap();
        let s = render("{{ \"CamelCase\" | uppercamelcase }}");
        assert_eq!(s, "CamelCase");
    }

    #[tokio::test]
    async fn test_hash() {
        let _config = Config::get().await.unwrap();
        // SHA256 of "foo" is 2c26b46b68ffc68ff99b453c1d30413413422d706483bfa0f98a5e886266e7ae
        let s = render("{{ \"foo\" | hash(len=8) }}");
        assert_eq!(s, "2c26b46b");
        // Test explicit sha256
        let s = render("{{ \"foo\" | hash(algorithm=\"sha256\", len=8) }}");
        assert_eq!(s, "2c26b46b");
        // Test blake3 - BLAKE3 of "foo" starts with 04e0bb39
        let s = render("{{ \"foo\" | hash(algorithm=\"blake3\", len=8) }}");
        assert_eq!(s, "04e0bb39");
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn test_hash_file() {
        let _config = Config::get().await.unwrap();
        let s = render("{{ \"../fixtures/shorthands.toml\" | hash_file(len=64) }}");
        insta::assert_snapshot!(s, @"ce17f44735ea2083038e61c4b291ed31593e6cf4d93f5dc147e97e62962ac4e6");
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn test_absolute() {
        let _config = Config::get().await.unwrap();
        let s = render("{{ \"/a/b/../c\" | absolute }}");
        assert_eq!(s, "/a/c");
        // relative path
        let s = render("{{ \"a/b/../c\" | absolute }}");
        assert!(s.ends_with("/a/c"));
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn test_canonicalize() {
        let _config = Config::get().await.unwrap();
        let s = render("{{ \"../fixtures/shorthands.toml\" | canonicalize }}");
        assert!(s.ends_with("/fixtures/shorthands.toml")); // test dir is not deterministic
    }

    #[tokio::test]
    async fn test_dirname() {
        let _config = Config::get().await.unwrap();
        let s = render(r#"{{ "a/b/c" | dirname }}"#);
        assert_eq!(s, "a/b");
    }

    #[tokio::test]
    async fn test_basename() {
        let _config = Config::get().await.unwrap();
        let s = render(r#"{{ "a/b/c" | basename }}"#);
        assert_eq!(s, "c");
    }

    #[tokio::test]
    async fn test_extname() {
        let _config = Config::get().await.unwrap();
        let s = render(r#"{{ "a/b/c.txt" | extname }}"#);
        assert_eq!(s, "txt");
    }

    #[tokio::test]
    async fn test_file_stem() {
        let _config = Config::get().await.unwrap();
        let s = render(r#"{{ "a/b/c.txt" | file_stem }}"#);
        assert_eq!(s, "c");
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn test_file_size() {
        let _config = Config::get().await.unwrap();
        let s = render(r#"{{ "../fixtures/shorthands.toml" | file_size }}"#);
        assert_eq!(s, "48");
    }

    #[tokio::test]
    async fn test_last_modified() {
        let _config = Config::get().await.unwrap();
        let s = render(r#"{{ "../fixtures/shorthands.toml" | last_modified }}"#);
        let timestamp = s.parse::<u64>().unwrap();
        assert!((1725000000..=2725000000).contains(&timestamp));
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn test_join_path() {
        let _config = Config::get().await.unwrap();
        let s = render(r#"{{ ["..", "fixtures", "shorthands.toml"] | join_path }}"#);
        assert_eq!(s, "../fixtures/shorthands.toml");
    }

    #[tokio::test]
    async fn test_is_dir() {
        let _config = Config::get().await.unwrap();
        let s = render(r#"{% set p = ".mise" %}{% if p is dir %} ok {% endif %}"#);
        assert_eq!(s.trim(), "ok");
    }

    #[tokio::test]
    async fn test_is_file() {
        let _config = Config::get().await.unwrap();
        let s = render(r#"{% set p = ".test-tool-versions" %}{% if p is file %} ok {% endif %}"#);
        assert_eq!(s.trim(), "ok");
    }

    #[tokio::test]
    async fn test_exists() {
        let _config = Config::get().await.unwrap();
        let s = render(r#"{% set p = ".test-tool-versions" %}{% if p is exists %} ok {% endif %}"#);
        assert_eq!(s.trim(), "ok");
    }

    #[tokio::test]
    async fn test_semver_matching() {
        let _config = Config::get().await.unwrap();
        let s = render(
            r#"{% set p = "1.10.2" %}{% if p is semver_matching(requirement="^1.10.0") %} ok {% endif %}"#,
        );
        assert_eq!(s.trim(), "ok");
    }

    #[test]
    fn test_contains_template_syntax() {
        assert!(contains_template_syntax("{{ foo }}"));
        assert!(contains_template_syntax("{{- foo -}}"));
        assert!(contains_template_syntax("{% if foo %}bar{% endif %}"));
        assert!(contains_template_syntax("{%- if foo -%}bar{%- endif -%}"));
        assert!(contains_template_syntax("{# comment #}"));
        assert!(contains_template_syntax("{#- comment -#}"));
        assert!(!contains_template_syntax("plain text"));
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn test_read_file() {
        use std::fs;
        use tempfile::TempDir;

        let _config = Config::get().await.unwrap();

        // Create a temp directory and test file
        let temp_dir = TempDir::new().unwrap();
        let test_file_path = temp_dir.path().join("test.txt");
        fs::write(&test_file_path, "test content\nwith multiple lines").unwrap();

        // Test with the temp file
        let mut tera_ctx = BASE_CONTEXT.clone();
        tera_ctx.insert("config_root", &temp_dir.path().to_str().unwrap());
        tera_ctx.insert("cwd", temp_dir.path().to_str().unwrap());
        let mut tera = get_tera(Some(temp_dir.path()));

        let s = render_str(&mut tera, r#"{{ read_file(path="test.txt") }}"#, &tera_ctx).unwrap();
        assert_eq!(s, "test content\nwith multiple lines");

        // Test with trim filter
        let s = render_str(
            &mut tera,
            r#"{{ read_file(path="test.txt") | trim }}"#,
            &tera_ctx,
        )
        .unwrap();
        assert_eq!(s, "test content\nwith multiple lines");
    }

    fn render(s: &str) -> String {
        let config_root = Path::new("/");
        let mut tera_ctx = BASE_CONTEXT.clone();
        tera_ctx.insert("config_root", &config_root);
        tera_ctx.insert("cwd", "/");
        let mut tera = get_tera(Option::from(config_root));
        render_str(&mut tera, s, &tera_ctx).unwrap()
    }

    fn render_for_target(s: &str, os: &str, arch: &str) -> String {
        let mut tera_ctx = BASE_CONTEXT.clone();
        tera_ctx.insert("cwd", "/");
        let mut tera = get_tera_for_target(None, os, arch);
        render_str(&mut tera, s, &tera_ctx).unwrap()
    }

    #[tokio::test]
    async fn test_os_arch_for_target() {
        let _config = Config::get().await.unwrap();
        // os()/arch() resolve to the requested target, not the host.
        assert_eq!(
            render_for_target("{{os()}}-{{arch()}}", "windows", "arm64"),
            "windows-arm64"
        );
        assert_eq!(render_for_target("{{os()}}", "macos", "x64"), "macos");
    }

    #[tokio::test]
    async fn test_os_arch_remap_for_target() {
        let _config = Config::get().await.unwrap();
        // Remap arguments keep host semantics but apply to the target value.
        assert_eq!(
            render_for_target(
                r#"{{os(macos="darwin")}}_{{arch(x64="amd64")}}"#,
                "macos",
                "x64"
            ),
            "darwin_amd64"
        );
        // A remap that does not match the target value is ignored.
        assert_eq!(
            render_for_target(r#"{{arch(x64="amd64")}}"#, "linux", "arm64"),
            "arm64"
        );
    }

    #[tokio::test]
    async fn test_os_family_for_target() {
        let _config = Config::get().await.unwrap();
        // os_family() follows the target, not the host.
        assert_eq!(
            render_for_target("{{os_family()}}", "windows", "x64"),
            "windows"
        );
        assert_eq!(render_for_target("{{os_family()}}", "linux", "x64"), "unix");
        assert_eq!(
            render_for_target("{{os_family()}}", "macos", "arm64"),
            "unix"
        );
    }

    #[tokio::test]
    async fn test_preserving_os_arch_round_trips_through_target() {
        let _config = Config::get().await.unwrap();
        // A deferred os(...) remap survives config-load preservation and then
        // re-renders correctly for a target platform.
        let mut ctx = BASE_CONTEXT.clone();
        ctx.insert("cwd", "/");
        let mut deferred = get_tera_preserving_os_arch(None);
        let preserved = render_str(&mut deferred, r#"{{ os(macos="darwin") }}"#, &ctx).unwrap();
        assert_eq!(preserved, r#"{{ os(macos="darwin") }}"#);
        let mut tera = get_tera_for_target(None, "macos", "arm64");
        assert_eq!(render_str(&mut tera, &preserved, &ctx).unwrap(), "darwin");
    }

    #[tokio::test]
    async fn test_preserving_os_family_round_trips_through_target() {
        let _config = Config::get().await.unwrap();
        // os_family() must be deferred at config-load time and resolve against
        // the lock target, not the host — otherwise a windows target locked from
        // a unix host would get "unix" baked in.
        let mut ctx = BASE_CONTEXT.clone();
        ctx.insert("cwd", "/");
        let mut deferred = get_tera_preserving_os_arch(None);
        let preserved = render_str(&mut deferred, r#"{{ os_family() }}"#, &ctx).unwrap();
        assert_eq!(preserved, r#"{{ os_family() }}"#);
        let mut tera = get_tera_for_target(None, "windows", "x64");
        assert_eq!(render_str(&mut tera, &preserved, &ctx).unwrap(), "windows");
    }
}
