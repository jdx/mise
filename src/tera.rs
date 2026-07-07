use std::collections::{HashMap, HashSet};
use std::iter::once;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, Utc};
use heck::{
    ToKebabCase, ToLowerCamelCase, ToShoutyKebabCase, ToShoutySnakeCase, ToSnakeCase,
    ToUpperCamelCase,
};
use path_absolutize::Absolutize;
use rand::prelude::*;
use regex::Regex;
use serde_json::{Map as JsonMap, Value as JsonValue, json};
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

pub enum TeraEngine {
    V2(Box<Tera>),
    V1(Box<tera1::Tera>),
}

impl TeraEngine {
    fn render_str(&mut self, input: &str, context: &Context) -> TeraResult<String> {
        match self {
            Self::V2(tera) => tera.render_str(input, context, false),
            Self::V1(tera) => {
                let context = tera1_context(input, context)?;
                tera.render_str(input, &context)
                    .map_err(|e| tera_err(e.to_string()))
            }
        }
    }
}

pub fn render_str(tera: &mut TeraEngine, input: &str, context: &Context) -> TeraResult<String> {
    tera.render_str(input, context)
}

pub fn render_str_v2(tera: &mut Tera, input: &str, context: &Context) -> TeraResult<String> {
    tera.render_str(input, context, false)
}

fn tera_err(message: impl ToString) -> tera::Error {
    tera::Error::message(message)
}

fn use_tera_v1() -> bool {
    Settings::try_get().is_ok_and(|settings| settings.tera_v1)
}

fn tera1_err(message: impl ToString) -> tera1::Error {
    tera1::Error::msg(message.to_string())
}

fn json_arg<'a>(args: &'a HashMap<String, JsonValue>, name: &str) -> tera1::Result<&'a JsonValue> {
    args.get(name)
        .ok_or_else(|| tera1_err(format!("missing required argument: {name}")))
}

fn json_str_arg<'a>(args: &'a HashMap<String, JsonValue>, name: &str) -> tera1::Result<&'a str> {
    json_arg(args, name)?
        .as_str()
        .ok_or_else(|| tera1_err(format!("argument `{name}` must be a string")))
}

fn json_path(value: &JsonValue) -> tera1::Result<&str> {
    value
        .as_str()
        .ok_or_else(|| tera1_err("filter input must be a string"))
}

fn tera1_context(input: &str, context: &Context) -> TeraResult<tera1::Context> {
    static IDENT_RE: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"[A-Za-z_][A-Za-z0-9_]*").expect("valid ident regex"));
    static RESERVED: Lazy<HashSet<&'static str>> = Lazy::new(|| {
        HashSet::from([
            "and",
            "as",
            "block",
            "break",
            "continue",
            "elif",
            "else",
            "endblock",
            "endfilter",
            "endif",
            "endfor",
            "endmacro",
            "endraw",
            "endset",
            "extends",
            "false",
            "filter",
            "for",
            "if",
            "import",
            "in",
            "include",
            "is",
            "macro",
            "not",
            "or",
            "raw",
            "set",
            "set_global",
            "true",
        ])
    });

    let mut data = JsonMap::new();
    for m in IDENT_RE.find_iter(input) {
        let key = m.as_str();
        if RESERVED.contains(key) || data.contains_key(key) {
            continue;
        }
        if let Some(value) = context.get(key) {
            data.insert(
                key.to_string(),
                serde_json::to_value(value).map_err(|e| tera_err(e.to_string()))?,
            );
        }
    }
    tera1::Context::from_value(JsonValue::Object(data)).map_err(|e| tera_err(e.to_string()))
}

fn warn_tera_v1_filter(id: &'static str, name: &str, replacement: &str) {
    deprecated_at!(
        "2026.10.0",
        "2027.4.0",
        id,
        "Tera v1 template helper `{name}` is deprecated in mise templates. {replacement}"
    );
}

fn tera_v1_slice_index(index: i64, len: usize) -> usize {
    if index < 0 {
        len.saturating_sub(index.unsigned_abs() as usize)
    } else {
        (index as usize).min(len)
    }
}

fn escape_html(s: &str) -> String {
    let mut output = String::with_capacity(s.len() * 2);
    for c in s.chars() {
        match c {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '"' => output.push_str("&quot;"),
            '\'' => output.push_str("&#x27;"),
            _ => output.push(c),
        }
    }
    output
}

fn slugify(s: &str) -> String {
    let mut out = String::new();
    let mut previous_dash = false;
    for c in s.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            out.push(c);
            previous_dash = false;
        } else if !previous_dash && !out.is_empty() {
            out.push('-');
            previous_dash = true;
        }
    }
    if previous_dash {
        out.pop();
    }
    out
}

fn tera_v1_truthy(value: &Value) -> bool {
    if value.is_none() {
        false
    } else if let Some(b) = value.as_bool() {
        b
    } else if let Some(s) = value.as_str() {
        !s.is_empty()
    } else if let Some(arr) = value.as_array() {
        !arr.is_empty()
    } else if let Some(map) = value.as_map() {
        !map.is_empty()
    } else if let Some(n) = value.as_number() {
        n.as_integer().is_some_and(|n| n != 0) || value.as_f64().is_some_and(|n| n != 0.0)
    } else {
        true
    }
}

fn tera_v1_date(value: Value, args: Kwargs) -> TeraResult<Value> {
    let format = args.get::<&str>("format")?.unwrap_or("%Y-%m-%d");
    let formatted = if let Some(n) = value.as_number() {
        let ts = n
            .as_integer()
            .ok_or_else(|| tera_err(format!("Filter `date` was invoked on a float: {value}")))?;
        let ts = i64::try_from(ts)
            .map_err(|_| tera_err(format!("Filter `date` timestamp is out of range: {value}")))?;
        DateTime::<Utc>::from_timestamp(ts, 0)
            .ok_or_else(|| tera_err(format!("Filter `date` timestamp is out of range: {ts}")))?
            .format(format)
            .to_string()
    } else if let Some(s) = value.as_str() {
        if s.contains('T') {
            match DateTime::<FixedOffset>::parse_from_rfc3339(s) {
                Ok(dt) => dt.format(format).to_string(),
                Err(_) => NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S")
                    .map(|dt| dt.format(format).to_string())
                    .map_err(|_| tera_err(format!("Error parsing `{s:?}` as a date")))?,
            }
        } else {
            NaiveDate::parse_from_str(s, "%Y-%m-%d")
                .map(|dt| dt.format(format).to_string())
                .map_err(|_| tera_err(format!("Error parsing `{s:?}` as YYYY-MM-DD date")))?
        }
    } else {
        return Err(tera_err(format!(
            "Filter `date` expected an integer timestamp or string, got {}",
            value.name()
        )));
    };

    Ok(Value::from(formatted))
}

fn tera_v1_int(value: Value, args: Kwargs) -> TeraResult<Value> {
    let default = args.get::<i64>("default")?;
    let base = args.get::<u32>("base")?.unwrap_or(10);
    if !(2..=36).contains(&base) {
        return Err(tera_err(format!(
            "int filter `base` must be between 2 and 36, got {base}"
        )));
    }

    let fallback = || {
        warn_tera_v1_filter(
            "tera-v1-int-default",
            "int(default=...)",
            "Invalid integer conversions should be handled explicitly.",
        );
        Ok(Value::from(default.unwrap_or_default()))
    };

    if let Some(s) = value.as_str() {
        let s = s.trim();
        let s = match base {
            2 => s.trim_start_matches("0b"),
            8 => s.trim_start_matches("0o"),
            16 => s.trim_start_matches("0x"),
            _ => s,
        };
        match i128::from_str_radix(s, base) {
            Ok(v) => Ok(Value::from(v)),
            Err(_) if s.contains('.') => match s.parse::<f64>() {
                Ok(f) => Ok(Value::from(f as i128)),
                Err(_) => fallback(),
            },
            Err(_) => fallback(),
        }
    } else {
        value
            .as_number()
            .and_then(|n| n.as_integer())
            .map(Value::from)
            .ok_or_else(|| tera_err("Filter `int` received an unexpected type"))
    }
}

fn tera_v1_float(value: Value, args: Kwargs) -> TeraResult<Value> {
    let default = args.get::<f64>("default")?;
    if let Some(s) = value.as_str() {
        match s.trim().parse::<f64>() {
            Ok(f) => Ok(Value::from(f)),
            Err(_) => {
                warn_tera_v1_filter(
                    "tera-v1-float-default",
                    "float(default=...)",
                    "Invalid float conversions should be handled explicitly.",
                );
                Ok(Value::from(default.unwrap_or_default()))
            }
        }
    } else {
        value
            .as_f64()
            .map(Value::from)
            .ok_or_else(|| tera_err("Filter `float` received an unexpected type"))
    }
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
            if alphabet.is_empty() {
                return Err(tera_err("choice alphabet must not be empty"));
            }
            let mut rng = rand::rng();
            let result: String = (0..n)
                .map(|_| *alphabet.choose(&mut rng).expect("alphabet non-empty"))
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
        warn_tera_v1_filter("tera-v1-as-str", "as_str", "Use `str` instead.");
        value.to_string()
    });
    tera.register_filter("escape", move |s: &str, _: Kwargs, _: &State| {
        warn_tera_v1_filter("tera-v1-escape", "escape", "Use `escape_html` instead.");
        Value::from(escape_html(s))
    });
    tera.register_filter("linebreaksbr", move |s: &str, _: Kwargs, _: &State| {
        warn_tera_v1_filter(
            "tera-v1-linebreaksbr",
            "linebreaksbr",
            "Use `newlines_to_br` instead.",
        );
        Value::from(s.replace("\r\n", "<br>").replace(['\n', '\r'], "<br>"))
    });
    tera.register_filter("addslashes", move |s: &str, _: Kwargs, _: &State| {
        warn_tera_v1_filter(
            "tera-v1-addslashes",
            "addslashes",
            "Use an explicit string replacement instead.",
        );
        Value::from(
            s.replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('\'', "\\'"),
        )
    });
    tera.register_filter("slugify", move |s: &str, _: Kwargs, _: &State| {
        warn_tera_v1_filter(
            "tera-v1-slugify",
            "slugify",
            "Use an explicit slug value or custom template logic instead.",
        );
        Value::from(slugify(s))
    });
    tera.register_filter("urlencode", move |s: &str, _: Kwargs, _: &State| {
        warn_tera_v1_filter(
            "tera-v1-urlencode",
            "urlencode",
            "Pre-encode URL components before rendering.",
        );
        Value::from(urlencoding::encode(s).replace("%2F", "/"))
    });
    tera.register_filter("urlencode_strict", move |s: &str, _: Kwargs, _: &State| {
        warn_tera_v1_filter(
            "tera-v1-urlencode-strict",
            "urlencode_strict",
            "Pre-encode URL components before rendering.",
        );
        Value::from(urlencoding::encode(s).into_owned())
    });
    tera.register_filter("striptags", move |s: &str, _: Kwargs, _: &State| {
        static STRIPTAGS_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"<[^>]*>").unwrap());
        warn_tera_v1_filter(
            "tera-v1-striptags",
            "striptags",
            "Strip tags before rendering or use custom template logic instead.",
        );
        Value::from(STRIPTAGS_RE.replace_all(s, "").to_string())
    });
    tera.register_filter("spaceless", move |s: &str, _: Kwargs, _: &State| {
        static SPACELESS_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r">\s+<").unwrap());
        warn_tera_v1_filter(
            "tera-v1-spaceless",
            "spaceless",
            "Minify whitespace before rendering or use custom template logic instead.",
        );
        Value::from(SPACELESS_RE.replace_all(s, "><").to_string())
    });
    tera.register_filter(
        "truncate",
        move |s: &str, args: Kwargs, _: &State| -> TeraResult<Value> {
            let length = match args.get::<usize>("length")? {
                Some(length) => length,
                None => {
                    warn_tera_v1_filter(
                        "tera-v1-truncate-default-length",
                        "truncate without length",
                        "Pass `length=...` explicitly.",
                    );
                    255
                }
            };
            let end = args.get::<&str>("end")?.unwrap_or("…");
            match s.char_indices().nth(length) {
                Some((byte_idx, _)) => Ok(Value::from(s[..byte_idx].to_string() + end)),
                None => Ok(Value::from(s)),
            }
        },
    );
    tera.register_filter(
        "indent",
        move |s: &str, args: Kwargs, _: &State| -> TeraResult<Value> {
            let prefix = match args.get::<&str>("prefix")? {
                Some(prefix) => {
                    warn_tera_v1_filter(
                        "tera-v1-indent-prefix",
                        "indent(prefix=...)",
                        "Use `indent(width=...)` instead.",
                    );
                    prefix.to_string()
                }
                None => " ".repeat(args.get::<usize>("width")?.unwrap_or(4).min(1000)),
            };
            let first = args.get::<bool>("first")?.unwrap_or(false);
            let blank = args.get::<bool>("blank")?.unwrap_or(false);
            let mut out = String::with_capacity(s.len() + prefix.len() * 2);
            let mut first_line = true;
            for line in s.lines() {
                if first_line {
                    if first {
                        out.push_str(&prefix);
                    }
                    first_line = false;
                } else {
                    out.push('\n');
                    if blank || !line.is_empty() {
                        out.push_str(&prefix);
                    }
                }
                out.push_str(line);
            }
            if s.ends_with('\n') {
                out.push('\n');
            }
            Ok(Value::from(out))
        },
    );
    tera.register_filter(
        "int",
        move |value: Value, args: Kwargs, _: &State| -> TeraResult<Value> {
            tera_v1_int(value, args)
        },
    );
    tera.register_filter(
        "float",
        move |value: Value, args: Kwargs, _: &State| -> TeraResult<Value> {
            tera_v1_float(value, args)
        },
    );
    tera.register_filter("first", move |value: Value, _: Kwargs, _: &State| {
        if let Some(arr) = value.as_array() {
            if arr.is_empty() {
                warn_tera_v1_filter(
                    "tera-v1-first-empty-string",
                    "first on an empty array",
                    "Tera 2 returns null for empty arrays.",
                );
                Value::from("")
            } else {
                arr[0].clone()
            }
        } else if let Some(s) = value.as_str() {
            Value::from(s.chars().next().map(String::from).unwrap_or_default())
        } else {
            Value::from("")
        }
    });
    tera.register_filter("last", move |value: Value, _: Kwargs, _: &State| {
        if let Some(arr) = value.as_array() {
            if arr.is_empty() {
                warn_tera_v1_filter(
                    "tera-v1-last-empty-string",
                    "last on an empty array",
                    "Tera 2 returns null for empty arrays.",
                );
                Value::from("")
            } else {
                arr[arr.len() - 1].clone()
            }
        } else if let Some(s) = value.as_str() {
            Value::from(s.chars().next_back().map(String::from).unwrap_or_default())
        } else {
            Value::from("")
        }
    });
    tera.register_filter(
        "nth",
        move |arr: &[Value], args: Kwargs, _: &State| -> TeraResult<Value> {
            let n = args.must_get::<usize>("n")?;
            if let Some(value) = arr.get(n) {
                Ok(value.clone())
            } else {
                warn_tera_v1_filter(
                    "tera-v1-nth-empty-string",
                    "nth outside array bounds",
                    "Tera 2 returns null for missing array elements.",
                );
                Ok(Value::from(""))
            }
        },
    );
    tera.register_filter(
        "unique",
        move |arr: &[Value], args: Kwargs, _: &State| -> TeraResult<Value> {
            let case_sensitive = args.get::<bool>("case_sensitive")?.unwrap_or(false);
            if args.get::<bool>("case_sensitive")?.is_some() {
                warn_tera_v1_filter(
                    "tera-v1-unique-case-sensitive",
                    "unique(case_sensitive=...)",
                    "Tera 2 `unique` no longer accepts arguments.",
                );
            }
            let mut out = Vec::new();
            for value in arr {
                let duplicate = out.iter().any(|existing: &Value| {
                    if case_sensitive {
                        existing == value
                    } else {
                        existing.to_string().to_lowercase() == value.to_string().to_lowercase()
                    }
                });
                if !duplicate {
                    out.push(value.clone());
                }
            }
            Ok(Value::from(out))
        },
    );
    tera.register_filter(
        "json_encode",
        move |value: Value, args: Kwargs, _: &State| -> TeraResult<Value> {
            warn_tera_v1_filter(
                "tera-v1-json-encode",
                "json_encode",
                "Use Tera 2 JSON helpers when available or pre-encode JSON before rendering.",
            );
            let pretty = args.get::<bool>("pretty")?.unwrap_or(false);
            let encoded = if pretty {
                serde_json::to_string_pretty(&value)
            } else {
                serde_json::to_string(&value)
            }
            .map_err(|e| tera_err(e.to_string()))?;
            Ok(Value::from(encoded))
        },
    );
    tera.register_filter(
        "date",
        move |value: Value, args: Kwargs, _: &State| -> TeraResult<Value> {
            warn_tera_v1_filter("tera-v1-date", "date", "Format dates before rendering.");
            tera_v1_date(value, args)
        },
    );
    tera.register_filter(
        "filesizeformat",
        move |value: Value, _: Kwargs, _: &State| -> TeraResult<Value> {
            warn_tera_v1_filter(
                "tera-v1-filesizeformat",
                "filesizeformat",
                "Format file sizes before rendering.",
            );
            let size = value
                .as_number()
                .and_then(|n| n.as_integer())
                .and_then(|n| u64::try_from(n).ok())
                .ok_or_else(|| tera_err("filesizeformat filter expects a non-negative integer"))?;
            Ok(Value::from(bytesize::ByteSize(size).to_string()))
        },
    );
    tera.register_filter(
        "trim_start_matches",
        move |s: &str, args: Kwargs, _: &State| -> TeraResult<Value> {
            warn_tera_v1_filter(
                "tera-v1-trim-start-matches",
                "trim_start_matches",
                "Use `trim_start(pat=...)` instead.",
            );
            let pat = args.must_get::<&str>("pat")?;
            Ok(Value::from(s.trim_start_matches(pat)))
        },
    );
    tera.register_filter(
        "trim_end_matches",
        move |s: &str, args: Kwargs, _: &State| -> TeraResult<Value> {
            warn_tera_v1_filter(
                "tera-v1-trim-end-matches",
                "trim_end_matches",
                "Use `trim_end(pat=...)` instead.",
            );
            let pat = args.must_get::<&str>("pat")?;
            Ok(Value::from(s.trim_end_matches(pat)))
        },
    );
    tera.register_filter(
        "slice",
        move |arr: &[Value], args: Kwargs, _: &State| -> TeraResult<Value> {
            warn_tera_v1_filter(
                "tera-v1-slice",
                "slice",
                "Use Tera 2 slice syntax like `array[start:end]` instead.",
            );
            let len = arr.len();
            let start = args
                .get::<i64>("start")?
                .map(|start| tera_v1_slice_index(start, len))
                .unwrap_or_default();
            let end = args
                .get::<i64>("end")?
                .map(|end| tera_v1_slice_index(end, len))
                .unwrap_or(len);

            if start >= end {
                Ok(Value::from(Vec::<Value>::new()))
            } else {
                Ok(Value::from(arr[start..end].to_vec()))
            }
        },
    );
    tera.register_filter(
        "concat",
        move |arr: &[Value], args: Kwargs, _: &State| -> TeraResult<Value> {
            warn_tera_v1_filter(
                "tera-v1-concat",
                "concat",
                "Use Tera 2 spread syntax instead.",
            );
            let mut out = arr.to_vec();
            let value = args.must_get::<Value>("with")?;
            if let Some(values) = value.as_array() {
                out.extend_from_slice(values);
            } else {
                out.push(value);
            }
            Ok(Value::from(out))
        },
    );
    tera.register_filter(
        "map",
        move |arr: &[Value], args: Kwargs, _: &State| -> TeraResult<Value> {
            warn_tera_v1_filter(
                "tera-v1-map",
                "map",
                "Use Tera 2 list comprehension instead.",
            );
            let attribute = args.must_get::<&str>("attribute")?;
            Ok(Value::from(
                arr.iter()
                    .filter_map(|v| v.get_from_path(attribute))
                    .filter(|v| !v.is_none())
                    .cloned()
                    .collect::<Vec<_>>(),
            ))
        },
    );
    tera.register_filter(
        "filter",
        move |arr: &[Value], args: Kwargs, _: &State| -> TeraResult<Value> {
            warn_tera_v1_filter(
                "tera-v1-filter",
                "filter",
                "Use Tera 2 list comprehension instead.",
            );
            let attribute = args.must_get::<&str>("attribute")?;
            let expected = args.get::<Value>("value")?;
            Ok(Value::from(
                arr.iter()
                    .filter(|v| {
                        let Some(actual) = v.get_from_path(attribute) else {
                            return false;
                        };
                        match &expected {
                            Some(expected) => actual == expected,
                            None => tera_v1_truthy(actual),
                        }
                    })
                    .cloned()
                    .collect::<Vec<_>>(),
            ))
        },
    );
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
    tera.register_test("object", move |value: &Value, _: Kwargs, _: &State| {
        warn_tera_v1_filter("tera-v1-object-test", "object test", "Use `map` instead.");
        value.is_map()
    });
    tera.register_test(
        "divisibleby",
        move |value: Value, args: Kwargs, _: &State| -> TeraResult<bool> {
            warn_tera_v1_filter(
                "tera-v1-divisibleby-test",
                "divisibleby test",
                "Use `divisible_by` instead.",
            );
            let divisor = args.must_get::<i128>("divisor")?;
            if divisor == 0 {
                return Ok(false);
            }
            let value = value
                .as_number()
                .and_then(|n| n.as_integer())
                .ok_or_else(|| tera_err("divisibleby test expects an integer"))?;
            Ok(value.checked_rem_euclid(divisor).is_some_and(|r| r == 0))
        },
    );
    tera.register_test(
        "semver_matching",
        move |version: &str, args: Kwargs, _: &State| -> TeraResult<bool> {
            let requirement = args.must_get::<&str>("requirement")?;
            let requirement = Requirement::new(requirement).ok_or_else(|| {
                tera_err(format!(
                    "semver_matching requirement is invalid: {requirement}"
                ))
            })?;
            let version = Versioning::new(version).ok_or_else(|| {
                tera_err(format!("semver_matching version is invalid: {version}"))
            })?;
            let result = requirement.matches(&version);
            Ok(result)
        },
    );

    tera
});

static TERA1: Lazy<tera1::Tera> = Lazy::new(|| {
    let mut tera = tera1::Tera::default();
    tera.register_function("arch", tera1_host_fn(host_arch_name));
    tera.register_function("os", tera1_host_fn(|| env::consts::OS));
    tera.register_function("os_family", move |_: &HashMap<String, JsonValue>| {
        Ok(json!(env::consts::FAMILY))
    });
    tera.register_function("num_cpus", move |_: &HashMap<String, JsonValue>| {
        Ok(json!(num_cpus::get().to_string()))
    });
    tera.register_function("choice", move |args: &HashMap<String, JsonValue>| {
        let n = json_arg(args, "n")?
            .as_u64()
            .ok_or_else(|| tera1_err("choice n must be an integer"))?;
        let alphabet = json_str_arg(args, "alphabet")?
            .chars()
            .collect::<Vec<char>>();
        if alphabet.is_empty() {
            return Err(tera1_err("choice alphabet must not be empty"));
        }
        let mut rng = rand::rng();
        let result: String = (0..n)
            .map(|_| *alphabet.choose(&mut rng).expect("alphabet non-empty"))
            .collect();
        Ok(json!(result))
    });
    tera.register_function("haiku", move |args: &HashMap<String, JsonValue>| {
        let words = args
            .get("words")
            .and_then(JsonValue::as_u64)
            .unwrap_or(2)
            .max(1) as usize;
        let separator = args
            .get("separator")
            .and_then(JsonValue::as_str)
            .unwrap_or("-");
        let digits = args.get("digits").and_then(JsonValue::as_u64).unwrap_or(2) as usize;
        Ok(json!(xx::rand::haiku(&xx::rand::HaikuOptions {
            words,
            separator,
            digits,
        })))
    });

    tera.register_filter(
        "hash_file",
        move |value: &JsonValue, args: &HashMap<String, JsonValue>| {
            let path = Path::new(json_path(value)?);
            track_tera_file(path);
            let mut hash =
                hash::file_hash_blake3(path, None).map_err(|e| tera1_err(e.to_string()))?;
            if let Some(len) = args.get("len").and_then(JsonValue::as_u64) {
                hash = hash.chars().take(len as usize).collect();
            }
            Ok(json!(hash))
        },
    );
    tera.register_filter(
        "hash",
        move |value: &JsonValue, args: &HashMap<String, JsonValue>| {
            let s = json_path(value)?;
            let algorithm = args
                .get("algorithm")
                .and_then(JsonValue::as_str)
                .unwrap_or("sha256");
            let mut hash = match algorithm {
                "sha256" => hash::hash_sha256_to_str(s),
                "blake3" => hash::hash_blake3_to_str(s),
                _ => return Err(tera1_err(format!("unknown hash algorithm: {algorithm}"))),
            };
            if let Some(len) = args.get("len").and_then(JsonValue::as_u64) {
                hash = hash.chars().take(len as usize).collect();
            }
            Ok(json!(hash))
        },
    );
    tera.register_filter(
        "absolute",
        tera1_path_filter(|p| Ok(p.absolutize()?.to_path_buf())),
    );
    tera.register_filter("canonicalize", tera1_path_filter(|p| p.canonicalize()));
    tera.register_filter(
        "dirname",
        move |value: &JsonValue, _: &HashMap<String, JsonValue>| {
            let p = Path::new(json_path(value)?);
            Ok(json!(
                p.parent()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default()
            ))
        },
    );
    tera.register_filter(
        "basename",
        move |value: &JsonValue, _: &HashMap<String, JsonValue>| {
            let p = Path::new(json_path(value)?);
            Ok(json!(
                p.file_name()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default()
            ))
        },
    );
    tera.register_filter(
        "extname",
        move |value: &JsonValue, _: &HashMap<String, JsonValue>| {
            let p = Path::new(json_path(value)?);
            Ok(json!(
                p.extension()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default()
            ))
        },
    );
    tera.register_filter(
        "file_stem",
        move |value: &JsonValue, _: &HashMap<String, JsonValue>| {
            let p = Path::new(json_path(value)?);
            Ok(json!(
                p.file_stem()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default()
            ))
        },
    );
    tera.register_filter(
        "file_size",
        move |value: &JsonValue, _: &HashMap<String, JsonValue>| {
            let path = Path::new(json_path(value)?);
            track_tera_file(path);
            Ok(json!(
                path.metadata().map_err(|e| tera1_err(e.to_string()))?.len()
            ))
        },
    );
    tera.register_filter(
        "last_modified",
        move |value: &JsonValue, _: &HashMap<String, JsonValue>| {
            let path = Path::new(json_path(value)?);
            track_tera_file(path);
            Ok(json!(
                path.metadata()
                    .map_err(|e| tera1_err(e.to_string()))?
                    .modified()
                    .map_err(|e| tera1_err(e.to_string()))?
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_err(|e| tera1_err(e.to_string()))?
                    .as_secs()
            ))
        },
    );
    tera.register_filter(
        "join_path",
        move |value: &JsonValue, _: &HashMap<String, JsonValue>| {
            let arr = value
                .as_array()
                .ok_or_else(|| tera1_err("join_path input must be an array of strings"))?;
            let mut path = PathBuf::new();
            for value in arr {
                path.push(
                    value
                        .as_str()
                        .ok_or_else(|| tera1_err("join_path input must be an array of strings"))?,
                );
            }
            Ok(json!(path.to_string_lossy().to_string()))
        },
    );
    tera.register_filter(
        "quote",
        move |value: &JsonValue, _: &HashMap<String, JsonValue>| {
            Ok(json!(format!(
                "'{}'",
                json_path(value)?.replace("'", "\\'")
            )))
        },
    );
    tera.register_filter("kebabcase", tera1_string_filter(|s| s.to_kebab_case()));
    tera.register_filter(
        "lowercamelcase",
        tera1_string_filter(|s| s.to_lower_camel_case()),
    );
    tera.register_filter(
        "shoutykebabcase",
        tera1_string_filter(|s| s.to_shouty_kebab_case()),
    );
    tera.register_filter(
        "shoutysnakecase",
        tera1_string_filter(|s| s.to_shouty_snake_case()),
    );
    tera.register_filter("snakecase", tera1_string_filter(|s| s.to_snake_case()));
    tera.register_filter(
        "uppercamelcase",
        tera1_string_filter(|s| s.to_upper_camel_case()),
    );

    tera.register_tester("dir", move |value: Option<&JsonValue>, _: &[JsonValue]| {
        Ok(value
            .and_then(JsonValue::as_str)
            .is_some_and(|s| Path::new(s).is_dir()))
    });
    tera.register_tester("file", move |value: Option<&JsonValue>, _: &[JsonValue]| {
        Ok(value
            .and_then(JsonValue::as_str)
            .is_some_and(|s| Path::new(s).is_file()))
    });
    tera.register_tester(
        "exists",
        move |value: Option<&JsonValue>, _: &[JsonValue]| {
            Ok(value
                .and_then(JsonValue::as_str)
                .is_some_and(|s| Path::new(s).exists()))
        },
    );
    tera.register_tester(
        "semver_matching",
        move |value: Option<&JsonValue>, args: &[JsonValue]| {
            let version = value
                .and_then(JsonValue::as_str)
                .ok_or_else(|| tera1_err("semver_matching value must be a string"))?;
            let requirement = args
                .first()
                .and_then(JsonValue::as_str)
                .ok_or_else(|| tera1_err("semver_matching requirement must be a string"))?;
            let requirement = Requirement::new(requirement).ok_or_else(|| {
                tera1_err(format!(
                    "semver_matching requirement is invalid: {requirement}"
                ))
            })?;
            let version = Versioning::new(version).ok_or_else(|| {
                tera1_err(format!("semver_matching version is invalid: {version}"))
            })?;
            Ok(requirement.matches(&version))
        },
    );

    tera
});

fn host_arch_name() -> &'static str {
    if cfg!(target_arch = "x86_64") {
        "x64"
    } else if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        env::consts::ARCH
    }
}

fn tera1_host_fn(
    default: fn() -> &'static str,
) -> impl Fn(&HashMap<String, JsonValue>) -> tera1::Result<JsonValue> {
    move |args| {
        let value = default();
        Ok(args
            .get(value)
            .and_then(JsonValue::as_str)
            .map_or_else(|| json!(value), |remapped| json!(remapped)))
    }
}

fn tera1_string_filter(
    f: fn(&str) -> String,
) -> impl Fn(&JsonValue, &HashMap<String, JsonValue>) -> tera1::Result<JsonValue> {
    move |value, _| Ok(json!(f(json_path(value)?)))
}

fn tera1_path_filter(
    f: fn(&Path) -> std::io::Result<PathBuf>,
) -> impl Fn(&JsonValue, &HashMap<String, JsonValue>) -> tera1::Result<JsonValue> {
    move |value, _| {
        let p = f(Path::new(json_path(value)?)).map_err(|e| tera1_err(e.to_string()))?;
        Ok(json!(p.to_string_lossy().to_string()))
    }
}

pub(crate) fn get_tera_v2(dir: Option<&Path>) -> Tera {
    let mut tera = TERA.clone();
    let dir = dir.map(PathBuf::from);
    tera.register_function("exec", tera_exec(dir.clone(), env::PRISTINE_ENV.clone()));
    tera.register_function("read_file", tera_read_file(dir));
    tera
}

fn get_tera_v1(dir: Option<&Path>) -> tera1::Tera {
    let mut tera = TERA1.clone();
    let dir = dir.map(PathBuf::from);
    tera.register_function("exec", tera1_exec(dir.clone(), env::PRISTINE_ENV.clone()));
    tera.register_function("read_file", tera1_read_file(dir));
    tera
}

/// Returns a Tera instance for use during early initialization (miserc loading).
/// This is a plain clone of the global `TERA` static. `exec` and `read_file` are absent
/// because they are only registered in [`get_tera`], not in `TERA` itself — so they
/// cannot accidentally become available here if `TERA` changes in the future. This also
/// intentionally ignores `tera_v1`, since Settings are not fully loaded at this stage.
pub fn get_miserc_tera() -> TeraEngine {
    TeraEngine::V2(Box::new(TERA.clone()))
}

pub fn get_tera(dir: Option<&Path>) -> TeraEngine {
    if use_tera_v1() {
        TeraEngine::V1(Box::new(get_tera_v1(dir)))
    } else {
        TeraEngine::V2(Box::new(get_tera_v2(dir)))
    }
}

/// Like [`get_tera`] but with `os()` and `arch()` bound to an explicit target
/// platform instead of the current host. Used by cross-platform `mise lock` to
/// render URL/checksum templates for platforms other than the one mise runs on.
///
/// `os` should be a platform os name (e.g. "macos", "linux", "windows") and
/// `arch` a platform arch name (e.g. "x64", "arm64"), matching the values
/// returned by the host-bound functions. Remap arguments such as
/// `os(macos="darwin")` and `arch(x64="amd64")` keep the same semantics.
pub fn get_tera_for_target(dir: Option<&Path>, os: &str, arch: &str) -> TeraEngine {
    // os_family() must follow the target too, not the host.
    let family = if os == "windows" { "windows" } else { "unix" };
    if use_tera_v1() {
        let mut tera = get_tera_v1(dir);
        let os = os.to_string();
        tera.register_function("os", move |args: &HashMap<String, JsonValue>| {
            Ok(args
                .get(&os)
                .and_then(JsonValue::as_str)
                .map_or_else(|| json!(os), |remapped| json!(remapped)))
        });
        let arch = arch.to_string();
        tera.register_function("arch", move |args: &HashMap<String, JsonValue>| {
            Ok(args
                .get(&arch)
                .and_then(JsonValue::as_str)
                .map_or_else(|| json!(arch), |remapped| json!(remapped)))
        });
        tera.register_function("os_family", move |_: &HashMap<String, JsonValue>| {
            Ok(json!(family))
        });
        TeraEngine::V1(Box::new(tera))
    } else {
        let mut tera = get_tera_v2(dir);
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

        TeraEngine::V2(Box::new(tera))
    }
}

/// Like [`get_tera`] but with `os()` and `arch()` rewritten to re-emit
/// themselves as template fragments (e.g. `os(macos="darwin")` renders back to
/// the literal `{{ os(macos="darwin") }}`).
///
/// Used when rendering tool option templates at config-load time: env/vars are
/// resolved, but `os()`/`arch()` are deferred so the backend can re-render them
/// for the host at install time or for an arbitrary target during cross-platform
/// `mise lock`. Mirrors how `{{ version }}` is preserved via a placeholder.
pub fn get_tera_preserving_os_arch(dir: Option<&Path>) -> TeraEngine {
    if use_tera_v1() {
        let mut tera = get_tera_v1(dir);
        tera.register_function("os", reemit_template_fn_v1("os"));
        tera.register_function("arch", reemit_template_fn_v1("arch"));
        tera.register_function("os_family", reemit_template_fn_v1("os_family"));
        TeraEngine::V1(Box::new(tera))
    } else {
        let mut tera = get_tera_v2(dir);
        tera.register_function("os", reemit_template_fn("os"));
        tera.register_function("arch", reemit_template_fn("arch"));
        // os_family() must be deferred too: it derives from the target OS, so
        // resolving it against the host here would bake e.g. "unix" into a template
        // that is later rendered for a windows target.
        tera.register_function("os_family", reemit_template_fn("os_family"));
        TeraEngine::V2(Box::new(tera))
    }
}

fn reemit_template_fn_v1(
    name: &'static str,
) -> impl Fn(&HashMap<String, JsonValue>) -> tera1::Result<JsonValue> {
    move |args| {
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
        Ok(json!(rendered))
    }
}

pub(crate) fn tera1_exec(
    dir: Option<PathBuf>,
    env: EnvMap,
) -> impl Fn(&HashMap<String, JsonValue>) -> tera1::Result<JsonValue> {
    move |args: &HashMap<String, JsonValue>| -> tera1::Result<JsonValue> {
        let command = json_str_arg(args, "command")?;
        let shell = Settings::get()
            .default_inline_shell()
            .map_err(|e| tera1_err(e.to_string()))?;
        let shell_args = shell
            .iter()
            .skip(1)
            .chain(once(&command.to_string()))
            .cloned()
            .collect::<Vec<String>>();
        let mut env_no_shims = env.clone();
        if let Some(path_val) = env_no_shims.get(&*env::PATH_KEY).cloned() {
            env_no_shims.insert(env::PATH_KEY.to_string(), strip_shims_from_path(&path_val));
        }
        let run_once = || -> eyre::Result<String> {
            #[cfg(windows)]
            {
                if let Some(mut c) =
                    crate::path::cmd_verbatim_command(&shell[0], &shell[1..], command)
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
                    let mut s = String::from_utf8(out.stdout)?;
                    while s.ends_with('\n') || s.ends_with('\r') {
                        s.pop();
                    }
                    return Ok(s);
                }
            }
            let mut expr: duct::Expression = cmd(&shell[0], &shell_args).full_env(&env_no_shims);
            if let Some(dir) = &dir {
                expr = expr.dir(dir);
            }
            Ok(expr.read()?)
        };
        Ok(json!(
            run_once().map_err(|e| tera1_err(format!("exec command: {e}")))?
        ))
    }
}

pub(crate) fn tera1_read_file(
    dir: Option<PathBuf>,
) -> impl Fn(&HashMap<String, JsonValue>) -> tera1::Result<JsonValue> {
    move |args: &HashMap<String, JsonValue>| -> tera1::Result<JsonValue> {
        let path_str = json_str_arg(args, "path")?;
        let path = if let Some(ref base_dir) = dir {
            base_dir.join(path_str)
        } else {
            PathBuf::from(path_str)
        };
        track_tera_file(&path);
        Ok(json!(std::fs::read_to_string(&path).map_err(|e| {
            tera1_err(format!("read_file({}): {e}", path.display()))
        })?))
    }
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
    use crate::config::{Config, Settings};

    use super::*;
    use pretty_assertions::assert_str_eq;

    static TEST_SETTINGS_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    struct SettingsGuard {
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    impl SettingsGuard {
        fn tera_v1() -> Self {
            let lock = TEST_SETTINGS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            Settings::override_with(|settings| settings.tera_v1 = Some(true));
            Self { _lock: lock }
        }
    }

    impl Drop for SettingsGuard {
        fn drop(&mut self) {
            Settings::reset(None);
        }
    }

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

    #[test]
    fn test_tera_v1_setting_selects_v1_engine() {
        let _guard = SettingsGuard::tera_v1();
        assert!(matches!(get_tera(None), TeraEngine::V1(_)));
    }

    #[test]
    fn test_miserc_tera_ignores_tera_v1_setting() {
        let _guard = SettingsGuard::tera_v1();
        assert!(matches!(get_miserc_tera(), TeraEngine::V2(_)));
    }

    #[test]
    fn test_tera_v1_engine_renders_v1_macro() {
        let mut tera_ctx = BASE_CONTEXT.clone();
        tera_ctx.insert("name", "mise");
        let mut tera = TeraEngine::V1(Box::new(TERA1.clone()));
        assert_eq!(
            render_str(
                &mut tera,
                "{% macro greet(name) %}hi {{ name }}{% endmacro %}{{ self::greet(name=name) }}",
                &tera_ctx
            )
            .unwrap(),
            "hi mise"
        );

        let mut tera = TeraEngine::V2(Box::new(TERA.clone()));
        assert!(
            render_str(
                &mut tera,
                "{% macro greet(name) %}hi {{ name }}{% endmacro %}{{ self::greet(name=name) }}",
                &tera_ctx
            )
            .is_err()
        );
    }

    #[test]
    fn test_tera_v1_compat_filters() {
        assert_eq!(
            render("v{{ 'v0.80.0' | trim_start_matches(pat='v') }}"),
            "v0.80.0"
        );
        assert_eq!(
            render("{{ 'v0.80.0' | trim_start_matches(pat='v') }}"),
            "0.80.0"
        );
        assert_eq!(
            render("{{ '0.80.0.tar.gz' | trim_end_matches(pat='.tar.gz') }}"),
            "0.80.0"
        );
        assert_eq!(
            render("{{ '1.12.6' | split(pat='.') | slice(start=0, end=2) | join(sep='.') }}"),
            "1.12"
        );
        assert_eq!(
            render("{{ '1.12.6' | split(pat='.') | slice(start=-2) | join(sep='.') }}"),
            "12.6"
        );
        assert_eq!(
            render("{{ ('1.12.6' | split(pat='.'))[0:2] | join(sep='.') }}"),
            "1.12"
        );
        assert_eq!(
            render("{{ ('a/b/c' | split(pat='/'))[0] ~ '/mod.rs' }}"),
            "a/mod.rs"
        );
        assert_eq!(render("{{ ['a'] | concat(with=['b']) | join }}"), "ab");
        assert_eq!(
            render(
                "{{ [{'name': 'alice', 'active': true}, {'name': 'bob', 'active': false}] | map(attribute='name') | join(sep=',') }}"
            ),
            "alice,bob"
        );
        assert_eq!(
            render(
                "{{ [{'name': 'alice', 'active': true}, {'name': 'bob', 'active': false}] | filter(attribute='active', value=true) | length }}"
            ),
            "1"
        );
        assert_eq!(
            render(
                "{{ [{'name': 'alice', 'active': true}, {'name': 'bob', 'active': false}] | filter(attribute='active') | map(attribute='name') | join(sep=',') }}"
            ),
            "alice"
        );
        assert_eq!(render("{{ '<b>x</b>' | striptags }}"), "x");
        assert_eq!(
            render("{{ '<b> x </b> <i>y</i>' | spaceless }}"),
            "<b> x </b><i>y</i>"
        );
        assert_eq!(render(r#"{{ "a'b" | addslashes }}"#), r#"a\'b"#);
        assert_eq!(render("{{ 'Hello, world!' | slugify }}"), "hello-world");
        assert_eq!(render("{{ 'a b' | urlencode }}"), "a%20b");
        assert_eq!(render("{{ 'a/b c' | urlencode }}"), "a/b%20c");
        assert_eq!(render("{{ 'a/b c' | urlencode_strict }}"), "a%2Fb%20c");
        assert_eq!(render("{{ '<br>' | escape }}"), "&lt;br&gt;");
        assert_eq!(render("{{ 'a\nb' | linebreaksbr }}"), "a<br>b");
        assert_eq!(render("{{ {'ok': true} | json_encode }}"), r#"{"ok":true}"#);
        assert_eq!(render("{{ 0 | date(format='%Y-%m-%d') }}"), "1970-01-01");
        assert_eq!(render("{{ 'abc' | truncate }}"), "abc");
        assert_eq!(render("{{ 'a\nb' | indent(prefix='>') }}"), "a\n>b");
        assert_eq!(render("{{ 'nope' | int(default=7) }}"), "7");
        assert_eq!(render("{{ 'nope' | float(default=1.5) }}"), "1.5");
        assert_eq!(render("{{ [] | first }}"), "");
        assert_eq!(render("{{ [] | last }}"), "");
        assert_eq!(render("{{ 'abc' | first }}"), "a");
        assert_eq!(render("{{ 'abc' | last }}"), "c");
        assert_eq!(render("{{ [] | nth(n=0) }}"), "");
        assert_eq!(render("{{ ['a', 'A'] | unique | join }}"), "a");
        assert_eq!(
            render("{{ ['a', 'A'] | unique(case_sensitive=false) | join }}"),
            "a"
        );
        assert_eq!(render("{{ {'ok': true} is object }}"), "true");
        assert_eq!(render("{{ 6 is divisibleby(divisor=3) }}"), "true");
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
