use crate::env;
use serde::Deserialize;
use serde_yaml::Value;
use std::collections::HashMap;
use std::sync::LazyLock as Lazy;

use crate::file::path_env_without_shims;

/// Cache for tokens obtained from `credential_command`.
/// Key format is `{provider}:{host}` to avoid cross-provider collisions.
static CREDENTIAL_COMMAND_CACHE: Lazy<std::sync::Mutex<HashMap<String, Option<String>>>> =
    Lazy::new(Default::default);

/// Cache for tokens obtained from `git credential fill`.
/// Key format is `{provider}:{host}` to avoid cross-provider collisions.
static GIT_CREDENTIAL_CACHE: Lazy<std::sync::Mutex<HashMap<String, Option<String>>>> =
    Lazy::new(Default::default);

#[derive(Deserialize)]
struct HostTokensFile {
    tokens: Option<HashMap<String, HostTokenEntry>>,
}

#[derive(Deserialize)]
struct HostTokenEntry {
    token: Option<String>,
}

pub fn parse_tokens_toml(contents: &str) -> Option<HashMap<String, String>> {
    let file: HostTokensFile = toml::from_str(contents).ok()?;
    Some(
        file.tokens?
            .into_iter()
            .filter_map(|(host, entry)| entry.token.map(|token| (host, token)))
            .collect(),
    )
}

pub fn read_tokens_toml(filename: &str, label: &str) -> Option<HashMap<String, String>> {
    let path = env::MISE_CONFIG_DIR.join(filename);
    let contents = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            trace!("{filename} not readable at {}: {e}", path.display());
            return None;
        }
    };
    match parse_tokens_toml(&contents) {
        Some(tokens) => Some(tokens),
        None => {
            debug!("failed to parse {label} at {}", path.display());
            None
        }
    }
}

/// Get a token by running a provider-specific `credential_command`.
///
/// The host is passed as `$1` to the command. Results are cached per provider+host.
pub fn get_credential_command_token(provider: &str, cmd: &str, host: &str) -> Option<String> {
    let cache_key = format!("{provider}:{host}");
    let mut cache = CREDENTIAL_COMMAND_CACHE
        .lock()
        .expect("CREDENTIAL_COMMAND_CACHE mutex poisoned");
    if let Some(token) = cache.get(&cache_key) {
        return token.clone();
    }

    let path_without_shims = path_env_without_shims();
    let result = std::process::Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .arg("mise-credential-helper") // $0
        .arg(host) // $1
        .env("PATH", &path_without_shims)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .ok()
        .and_then(|output| {
            if !output.status.success() {
                if let Ok(err) = String::from_utf8(output.stderr)
                    && !err.trim().is_empty()
                {
                    debug!("{provider} credential_command stderr: {}", err.trim());
                }
                return None;
            }
            String::from_utf8(output.stdout)
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        });

    trace!(
        "{provider} credential_command for {host}: {}",
        if result.is_some() {
            "found"
        } else {
            "not found"
        }
    );
    cache.insert(cache_key, result.clone());
    result
}

/// Get a token by running `git credential fill`.
///
/// Results are cached per provider+host so the subprocess is only spawned once.
pub fn get_git_credential_token(provider: &str, host: &str) -> Option<String> {
    let cache_key = format!("{provider}:{host}");
    let mut cache = GIT_CREDENTIAL_CACHE
        .lock()
        .expect("GIT_CREDENTIAL_CACHE mutex poisoned");
    if let Some(token) = cache.get(&cache_key) {
        return token.clone();
    }

    let path_without_shims = path_env_without_shims();
    let input = format!("protocol=https\nhost={host}\n\n");
    let result = std::process::Command::new("git")
        .args(["credential", "fill"])
        .env("PATH", &path_without_shims)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()
        .and_then(|mut child| {
            use std::io::Write;
            child.stdin.take()?.write_all(input.as_bytes()).ok()?;
            let output = child.wait_with_output().ok()?;
            if !output.status.success() {
                return None;
            }
            String::from_utf8(output.stdout)
                .ok()?
                .lines()
                .find_map(|line| line.strip_prefix("password="))
                .map(|p| p.to_string())
                .filter(|s| !s.is_empty())
        });

    trace!(
        "{provider} git credential fill for {host}: {}",
        if result.is_some() {
            "found"
        } else {
            "not found"
        }
    );
    cache.insert(cache_key, result.clone());
    result
}

pub fn mask_token(token: &str) -> String {
    let len = token.len();
    if len <= 4 {
        "*".repeat(len)
    } else if len <= 8 {
        format!("{}…", &token[..4])
    } else {
        format!("{}…{}", &token[..4], &token[len - 4..])
    }
}

pub fn yaml_hosts_to_tokens(contents: &str) -> Option<HashMap<String, String>> {
    let yaml: Value = serde_yaml::from_str(contents).ok()?;
    let mut out = HashMap::new();
    if let Some(map) = yaml.as_mapping() {
        collect_mapping_tokens(map, &mut out);

        if let Some(hosts_value) = map.get(Value::String("hosts".to_string()))
            && let Some(hosts) = hosts_value.as_mapping()
        {
            collect_mapping_tokens(hosts, &mut out);
        }

        if let Some(logins_value) = map.get(Value::String("logins".to_string())) {
            collect_list_tokens(logins_value, &mut out);
        }
    }
    if out.is_empty() { None } else { Some(out) }
}

fn collect_mapping_tokens(map: &serde_yaml::Mapping, out: &mut HashMap<String, String>) {
    for (k, v) in map {
        let Some(host) = k.as_str() else {
            continue;
        };
        let Some(entry) = v.as_mapping() else {
            continue;
        };

        if let Some(token) = token_from_entry(entry) {
            out.insert(host.to_string(), token);
        }
    }
}

fn collect_list_tokens(v: &Value, out: &mut HashMap<String, String>) {
    let Some(entries) = v.as_sequence() else {
        return;
    };
    for entry in entries {
        let Some(map) = entry.as_mapping() else {
            continue;
        };
        let host = map
            .get(Value::String("name".to_string()))
            .and_then(Value::as_str)
            .or_else(|| {
                map.get(Value::String("host".to_string()))
                    .and_then(Value::as_str)
            })
            .or_else(|| {
                map.get(Value::String("url".to_string()))
                    .and_then(Value::as_str)
            });
        if let (Some(host), Some(token)) = (host, token_from_entry(map)) {
            out.insert(host.to_string(), token);
        }
    }
}

fn token_from_entry(entry: &serde_yaml::Mapping) -> Option<String> {
    ["oauth_token", "token", "access_token", "access-token"]
        .iter()
        .find_map(|k| {
            entry
                .get(Value::String((*k).to_string()))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(ToString::to_string)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_tokens_toml() {
        let toml = r#"
[tokens."example.com"]
token = "abc"
"#;
        let result = parse_tokens_toml(toml).unwrap();
        assert_eq!(result.get("example.com").unwrap(), "abc");
    }

    #[test]
    fn test_yaml_hosts_to_tokens_with_hosts_map() {
        let yaml = r#"
hosts:
  gitlab.com:
    token: glab-token
  codeberg.org:
    oauth_token: tea-token
"#;
        let result = yaml_hosts_to_tokens(yaml).unwrap();
        assert_eq!(result.get("gitlab.com").unwrap(), "glab-token");
        assert_eq!(result.get("codeberg.org").unwrap(), "tea-token");
    }

    #[test]
    fn test_yaml_hosts_to_tokens_with_logins_list() {
        let yaml = r#"
logins:
  - name: codeberg.org
    token: token1
  - host: forgejo.local
    access_token: token2
"#;
        let result = yaml_hosts_to_tokens(yaml).unwrap();
        assert_eq!(result.get("codeberg.org").unwrap(), "token1");
        assert_eq!(result.get("forgejo.local").unwrap(), "token2");
    }
}
