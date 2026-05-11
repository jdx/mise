use crate::config::Settings;
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
/// The host and provider are passed through `MISE_CREDENTIAL_HOST` and
/// `MISE_CREDENTIAL_PROVIDER`. Results are cached per provider+host.
pub fn get_credential_command_token(provider: &str, cmd: &str, host: &str) -> Option<String> {
    if credential_command_uses_legacy_host_arg(cmd) {
        deprecated_at!(
            "2026.11.0",
            "2027.11.0",
            "credential-command-shell-arg",
            "Use MISE_CREDENTIAL_HOST instead of $1/${{1}} in {provider} credential_command"
        );
    }

    let cache_key = format!("{provider}:{host}");
    let mut cache = CREDENTIAL_COMMAND_CACHE
        .lock()
        .expect("CREDENTIAL_COMMAND_CACHE mutex poisoned");
    if let Some(token) = cache.get(&cache_key) {
        return token.clone();
    }

    let path_without_shims = path_env_without_shims();
    let (program, args) = match credential_command_shell(cmd, host) {
        Some(command) => command,
        None => {
            debug!("{provider} credential_command skipped: default inline shell is empty");
            cache.insert(cache_key, None);
            return None;
        }
    };
    let result = std::process::Command::new(program)
        .args(args)
        .env("PATH", &path_without_shims)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("MISE_CREDENTIAL_HOST", host)
        .env("MISE_CREDENTIAL_PROVIDER", provider)
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

fn credential_command_shell(cmd: &str, host: &str) -> Option<(String, Vec<String>)> {
    let shell = match Settings::get().default_inline_shell() {
        Ok(shell) => shell,
        Err(e) => {
            debug!("failed to parse default inline shell for credential_command: {e}");
            return None;
        }
    };
    credential_command_shell_from(&shell, cmd, host)
}

fn credential_command_shell_from(
    shell: &[String],
    cmd: &str,
    host: &str,
) -> Option<(String, Vec<String>)> {
    let (program, shell_args) = shell.split_first()?;
    let mut args = shell_args.to_vec();
    args.push(cmd.to_string());
    if shell_supports_posix_c_arg_passing(program) {
        args.push("mise-credential-helper".to_string()); // $0
        args.push(host.to_string()); // deprecated $1 compatibility
    }
    Some((program.to_string(), args))
}

fn shell_supports_posix_c_arg_passing(program: &str) -> bool {
    const SHELLS: &[&str] = &["ash", "bash", "dash", "ksh", "sh", "zsh"];
    let basename = program.rsplit(['/', '\\']).next().unwrap_or(program);
    let stem = match basename.rsplit_once('.') {
        Some((stem, ext)) if ext.eq_ignore_ascii_case("exe") => stem,
        _ => basename,
    };
    SHELLS.iter().any(|shell| stem.eq_ignore_ascii_case(shell))
}

fn credential_command_uses_legacy_host_arg(cmd: &str) -> bool {
    cmd.contains("$1") || cmd.contains("${1}")
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
    let len = token.chars().count();
    if len <= 4 {
        "*".repeat(len)
    } else if len <= 8 {
        let prefix: String = token.chars().take(4).collect();
        format!("{prefix}…")
    } else {
        let prefix: String = token.chars().take(4).collect();
        let suffix: String = token.chars().skip(len - 4).collect();
        format!("{prefix}…{suffix}")
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

    #[test]
    fn test_credential_command_shell_preserves_sh_host_arg() {
        let shell = shell_words::split("sh -c -o errexit").unwrap();
        let (program, args) =
            credential_command_shell_from(&shell, "echo token-for-$1", "ghe.example.com").unwrap();

        assert_eq!(program, "sh");
        assert_eq!(
            args,
            vec![
                "-c",
                "-o",
                "errexit",
                "echo token-for-$1",
                "mise-credential-helper",
                "ghe.example.com"
            ]
        );
    }

    #[test]
    fn test_credential_command_shell_does_not_append_args_for_cmd() {
        let shell = shell_words::split("cmd /c").unwrap();
        let (program, args) =
            credential_command_shell_from(&shell, "echo %MISE_CREDENTIAL_HOST%", "github.com")
                .unwrap();

        assert_eq!(program, "cmd");
        assert_eq!(args, vec!["/c", "echo %MISE_CREDENTIAL_HOST%"]);
    }

    #[test]
    fn test_shell_supports_posix_c_arg_passing_matches_windows_bash_path() {
        assert!(shell_supports_posix_c_arg_passing("ash"));
        assert!(shell_supports_posix_c_arg_passing(
            r"C:\Program Files\Git\bin\BASH.EXE"
        ));
        assert!(!shell_supports_posix_c_arg_passing("cmd.exe"));
    }

    #[test]
    fn test_credential_command_uses_legacy_host_arg() {
        assert!(credential_command_uses_legacy_host_arg("echo token-for-$1"));
        assert!(credential_command_uses_legacy_host_arg(
            "echo token-for-${1}"
        ));
        assert!(!credential_command_uses_legacy_host_arg(
            "echo $MISE_CREDENTIAL_HOST"
        ));
    }
}
