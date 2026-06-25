//! macOS user defaults (preferences) for the `[bootstrap.macos.defaults]` config section.
//!
//! Entries are written with `defaults write <domain> <key> <-type> <value>`
//! and checked with `defaults read-type`/`defaults read`. Like
//! `[bootstrap.packages]` they are machine-global, declarative, and only ever
//! applied when explicitly requested with `mise bootstrap macos defaults apply`
//! or `mise bootstrap`.

use std::process::Stdio;

use crate::result::Result;

/// A single `[bootstrap.macos.defaults.<domain>]` entry: `key = value`
#[derive(Debug, Clone, PartialEq)]
pub struct DefaultsRequest {
    /// preferences domain, e.g. "com.apple.dock" or "NSGlobalDomain"
    pub domain: String,
    pub key: String,
    pub value: DefaultsValue,
}

impl std::fmt::Display for DefaultsRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {} = {}", self.domain, self.key, self.value)
    }
}

/// The value types `defaults write` can set and mise can verify. Other plist
/// types (arrays, dicts, dates, data) are not supported — config entries with
/// those TOML types warn and are skipped.
#[derive(Debug, Clone, PartialEq)]
pub enum DefaultsValue {
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
}

impl DefaultsValue {
    pub fn from_toml(value: &toml::Value) -> Option<Self> {
        match value {
            toml::Value::Boolean(b) => Some(Self::Bool(*b)),
            toml::Value::Integer(i) => Some(Self::Int(*i)),
            toml::Value::Float(f) => Some(Self::Float(*f)),
            toml::Value::String(s) => Some(Self::Str(s.clone())),
            _ => None,
        }
    }

    /// type+value arguments for `defaults write <domain> <key> ...`
    pub fn write_args(&self) -> Vec<String> {
        match self {
            Self::Bool(b) => vec!["-bool".into(), b.to_string()],
            Self::Int(i) => vec!["-int".into(), i.to_string()],
            Self::Float(f) => vec!["-float".into(), f.to_string()],
            Self::Str(s) => vec!["-string".into(), s.clone()],
        }
    }

    pub fn to_json(&self) -> serde_json::Value {
        match self {
            Self::Bool(b) => (*b).into(),
            Self::Int(i) => (*i).into(),
            Self::Float(f) => (*f).into(),
            Self::Str(s) => s.clone().into(),
        }
    }

    /// Does the pair from `defaults read-type` ("boolean", "integer", ...)
    /// and `defaults read` (raw value; booleans print as 1/0) match this
    /// value? Types are compared strictly: an integer 1 does not satisfy a
    /// configured `true` — `mise bootstrap macos defaults apply` converges it to the typed
    /// value.
    fn matches(&self, read_type: &str, raw: &str) -> bool {
        match self {
            Self::Bool(b) => read_type == "boolean" && raw == if *b { "1" } else { "0" },
            Self::Int(i) => read_type == "integer" && raw.parse::<i64>() == Ok(*i),
            Self::Float(f) => {
                read_type == "float" && raw.parse::<f64>().is_ok_and(|v| (v - f).abs() < 1e-9)
            }
            Self::Str(s) => read_type == "string" && raw == s,
        }
    }
}

impl std::fmt::Display for DefaultsValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Bool(b) => write!(f, "{b}"),
            Self::Int(i) => write!(f, "{i}"),
            Self::Float(v) => write!(f, "{v}"),
            Self::Str(s) => write!(f, "{s}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum DefaultsState {
    /// current value matches the config
    Set,
    /// a value exists but differs from the config (in value or type)
    Differs { current: String },
    /// the key is not set in this domain
    Unset,
}

#[derive(Debug, Clone)]
pub struct DefaultsStatus {
    pub request: DefaultsRequest,
    pub state: DefaultsState,
}

pub fn is_available() -> bool {
    cfg!(target_os = "macos") && crate::file::which("defaults").is_some()
}

pub fn unavailable_reason() -> String {
    if cfg!(target_os = "macos") {
        "`defaults` not found".to_string()
    } else {
        "only available on macos".to_string()
    }
}

/// Query the current state of each entry. Side-effect free.
pub async fn status(requests: &[DefaultsRequest]) -> Result<Vec<DefaultsStatus>> {
    let mut out = vec![];
    for req in requests {
        let state = match read(&req.domain, &req.key).await? {
            Some((read_type, raw)) => {
                if req.value.matches(&read_type, &raw) {
                    DefaultsState::Set
                } else {
                    // call out a type mismatch when the raw value alone
                    // would look identical to the configured one
                    let current = if raw == req.value.to_string() {
                        format!("{raw} ({read_type})")
                    } else {
                        raw
                    };
                    DefaultsState::Differs { current }
                }
            }
            None => DefaultsState::Unset,
        };
        out.push(DefaultsStatus {
            request: req.clone(),
            state,
        });
    }
    Ok(out)
}

/// Write the given entries (already filtered to unset/differing ones)
pub async fn apply(requests: &[DefaultsRequest], dry_run: bool) -> Result<()> {
    for req in requests {
        let mut args = vec!["write".to_string(), req.domain.clone(), req.key.clone()];
        args.extend(req.value.write_args());
        // shell-quoted so the printed command is copy-pasteable even when a
        // string value contains spaces
        let display = shell_words::join(&args);
        if dry_run {
            miseprintln!("defaults {display}");
            continue;
        }
        debug!("$ defaults {display}");
        let output = tokio::process::Command::new("defaults")
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await?;
        if !output.status.success() {
            eyre::bail!(
                "`defaults {display}` failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
    }
    Ok(())
}

/// `defaults read-type` + `defaults read` for one key. Returns
/// `(type, raw value)`, or None when the key (or domain) does not exist —
/// both commands exit non-zero for that, which is not an error here.
async fn read(domain: &str, key: &str) -> Result<Option<(String, String)>> {
    let Some(read_type) = defaults_cmd(&["read-type", domain, key]).await? else {
        return Ok(None);
    };
    // "Type is boolean" -> "boolean"
    let read_type = read_type
        .strip_prefix("Type is ")
        .unwrap_or(&read_type)
        .to_string();
    let Some(raw) = defaults_cmd(&["read", domain, key]).await? else {
        return Ok(None);
    };
    Ok(Some((read_type, raw)))
}

async fn defaults_cmd(args: &[&str]) -> Result<Option<String>> {
    debug!("$ defaults {}", shell_words::join(args));
    let output = tokio::process::Command::new("defaults")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await?;
    if !output.status.success() {
        // "does not exist" is the expected missing-key/-domain answer; any
        // other failure (cfprefsd unavailable, managed domain, ...) must not
        // masquerade as Unset
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("does not exist") {
            return Ok(None);
        }
        eyre::bail!(
            "`defaults {}` failed: {}",
            shell_words::join(args),
            stderr.trim()
        );
    }
    // strip only the trailing newline — leading/trailing spaces can be
    // significant in string values
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(Some(stdout.trim_end_matches(['\r', '\n']).to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn val(s: &str) -> toml::Value {
        s.parse().unwrap()
    }

    #[test]
    fn test_from_toml() {
        assert_eq!(
            DefaultsValue::from_toml(&val("true")),
            Some(DefaultsValue::Bool(true))
        );
        assert_eq!(
            DefaultsValue::from_toml(&val("48")),
            Some(DefaultsValue::Int(48))
        );
        assert_eq!(
            DefaultsValue::from_toml(&val("1.5")),
            Some(DefaultsValue::Float(1.5))
        );
        assert_eq!(
            DefaultsValue::from_toml(&val(r#""right""#)),
            Some(DefaultsValue::Str("right".into()))
        );

        // unsupported plist shapes are None -> warned + skipped by the caller
        assert_eq!(DefaultsValue::from_toml(&val("[1, 2]")), None);
        assert_eq!(DefaultsValue::from_toml(&val("{ a = 1 }")), None);
    }

    #[test]
    fn test_write_args() {
        assert_eq!(DefaultsValue::Bool(true).write_args(), ["-bool", "true"]);
        assert_eq!(DefaultsValue::Bool(false).write_args(), ["-bool", "false"]);
        assert_eq!(DefaultsValue::Int(2).write_args(), ["-int", "2"]);
        assert_eq!(DefaultsValue::Float(0.5).write_args(), ["-float", "0.5"]);
        assert_eq!(
            DefaultsValue::Str("left".into()).write_args(),
            ["-string", "left"]
        );
    }

    #[test]
    fn test_matches() {
        // booleans read back as 1/0
        assert!(DefaultsValue::Bool(true).matches("boolean", "1"));
        assert!(DefaultsValue::Bool(false).matches("boolean", "0"));
        assert!(!DefaultsValue::Bool(true).matches("boolean", "0"));
        // strict typing: integer 1 does not satisfy `true`
        assert!(!DefaultsValue::Bool(true).matches("integer", "1"));

        assert!(DefaultsValue::Int(2).matches("integer", "2"));
        assert!(!DefaultsValue::Int(2).matches("integer", "3"));
        assert!(!DefaultsValue::Int(2).matches("float", "2"));

        // `defaults read` may print floats without a fraction
        assert!(DefaultsValue::Float(48.0).matches("float", "48"));
        assert!(DefaultsValue::Float(0.5).matches("float", "0.5"));
        assert!(!DefaultsValue::Float(0.5).matches("float", "0.6"));

        assert!(DefaultsValue::Str("left".into()).matches("string", "left"));
        assert!(!DefaultsValue::Str("left".into()).matches("string", "right"));
    }
}
