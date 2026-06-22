//! macOS user defaults (preferences) for the `[bootstrap.macos.defaults]` config section.
//!
//! Entries are written with `defaults write <domain> <key> <-type> <value>`
//! and checked with `defaults read-type`/`defaults read`. Like
//! `[bootstrap.packages]` they are machine-global, declarative, and only ever
//! applied when explicitly requested with `mise bootstrap macos-defaults apply`
//! or `mise bootstrap`.

use std::process::Stdio;

use indexmap::IndexMap;

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

/// Supported plist types for `defaults export`/`defaults import`.
/// Unsupported plist types (date, data) fail at the `TryFrom` boundary.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum DefaultsValue {
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    Array(Vec<DefaultsValue>),
    Dict(IndexMap<String, DefaultsValue>),
}

impl TryFrom<&toml::Value> for DefaultsValue {
    type Error = plist::Error;

    fn try_from(value: &toml::Value) -> std::result::Result<Self, Self::Error> {
        let pv = plist::to_value(value)?;
        plist::from_value(&pv)
    }
}

impl DefaultsValue {
    pub fn write_args(&self) -> Vec<String> {
        todo!("replaced by defaults import")
    }

    fn matches(&self, _read_type: &str, _raw: &str) -> bool {
        todo!("replaced by defaults export")
    }
}

impl std::fmt::Display for DefaultsValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Bool(b) => write!(f, "{b}"),
            Self::Int(i) => write!(f, "{i}"),
            Self::Float(v) => write!(f, "{v}"),
            Self::Str(s) => write!(f, "{s}"),
            Self::Array(arr) => {
                write!(f, "[")?;
                for (i, v) in arr.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{v}")?;
                }
                write!(f, "]")
            }
            Self::Dict(dict) => {
                write!(f, "{{")?;
                for (i, (k, v)) in dict.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{k} = {v}")?;
                }
                write!(f, "}}")
            }
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
    fn test_try_from_toml() {
        assert_eq!(
            DefaultsValue::try_from(&val("true")).unwrap(),
            DefaultsValue::Bool(true)
        );
        assert_eq!(
            DefaultsValue::try_from(&val("48")).unwrap(),
            DefaultsValue::Int(48)
        );
        assert_eq!(
            DefaultsValue::try_from(&val("1.5")).unwrap(),
            DefaultsValue::Float(1.5)
        );
        assert_eq!(
            DefaultsValue::try_from(&val(r#""right""#)).unwrap(),
            DefaultsValue::Str("right".into())
        );
        assert_eq!(
            DefaultsValue::try_from(&val("[1, 2]")).unwrap(),
            DefaultsValue::Array(vec![DefaultsValue::Int(1), DefaultsValue::Int(2)])
        );
        assert_eq!(
            DefaultsValue::try_from(&val("{ a = 1 }")).unwrap(),
            DefaultsValue::Dict(IndexMap::from([("a".into(), DefaultsValue::Int(1))]))
        );
    }
}
