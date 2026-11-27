use crate::cmd::{RunningPidGuard, prepare_noninteractive_child};
use crate::env;
use crate::yaml::{self, Value};
use eyre::{Result, eyre};
use mise_settings::Settings;
use reqwest::header::HeaderValue;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::LazyLock as Lazy;

use crate::file::path_env_without_shims;

/// Cache for tokens obtained from `credential_command`.
/// Key format is `{provider}:{host}` to avoid cross-provider collisions and
/// preserve host-aware lookups (so different GitHub Enterprise instances
/// still spawn their own helper). Callers are responsible for not walking
/// equivalent hosts (e.g. `github.com` and `api.github.com`) on this path.
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
            .filter_map(|(host, entry)| {
                // Match the env-var path (`env::scoped_var`), which trims and
                // drops empties: a token with a stray trailing newline is not
                // an HTTP header value.
                let token = entry.token?.trim().to_string();
                if token.is_empty() {
                    return None;
                }
                Some((host, token))
            })
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
/// `MISE_CREDENTIAL_PROVIDER`. Results are cached per `{provider}:{host}`
/// for the lifetime of the process — repeated calls for the same host
/// reuse the cached value (positive or negative).
pub fn get_credential_command_token(provider: &str, cmd: &str, host: &str) -> Option<String> {
    // The host is often that of a URL chosen by a project's config or a
    // server's response, and a URL host may contain `&`, `;`, `"`, `$`, `(`
    // and more. The helper is a shell command line: `cmd` expands
    // `%MISE_CREDENTIAL_HOST%` before it parses the line, and a helper that
    // hands the host to another shell parses it again.
    if !is_plain_host(host) {
        debug!("{provider} credential_command skipped: {host:?} is not a plain host name");
        return None;
    }

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

    let Some(mut command) = credential_command_process(provider, cmd, host) else {
        cache.insert(cache_key, None);
        return None;
    };
    let result = command
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

/// Whether `host` is spelled like a DNS name or an IP address, optionally with
/// a port: no character a shell reads as an operator, a quote or a
/// substitution, and no leading `-` for a program to read as an option.
fn is_plain_host(host: &str) -> bool {
    host.starts_with(|c: char| c.is_ascii_alphanumeric() || c == '[')
        && host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".-_:[]".contains(&b))
}

/// Build the process that runs a provider `credential_command` (or another
/// user-supplied shell command) with the default inline shell, with the
/// credential environment set. Returns `None` when no shell is configured.
pub(crate) fn credential_command_process(
    provider: &str,
    cmd: &str,
    host: &str,
) -> Option<std::process::Command> {
    credential_command_process_with_path(provider, cmd, host, &path_env_without_shims())
}

/// [`credential_command_process`] with the `PATH` the command runs with, which
/// also decides whether it can run without the shell.
fn credential_command_process_with_path(
    provider: &str,
    cmd: &str,
    host: &str,
    path_without_shims: &std::ffi::OsStr,
) -> Option<std::process::Command> {
    let (program, args) = match credential_command_shell(cmd, host) {
        Some(command) => command,
        None => {
            debug!("{provider} credential_command skipped: default inline shell is empty");
            return None;
        }
    };
    let mut command = std::process::Command::new(&program);
    command.args(&args);
    // On Windows, route a `cmd /c <credential-command>` through the verbatim
    // builder so inner double quotes survive (#9355). Other shells and Unix keep
    // the plain Command::new(program).args(args). The command body is the last
    // element of args (see credential_command_shell_from).
    #[cfg(windows)]
    if let Some((body, flags)) = args.split_last()
        && let Some(c) = crate::path::cmd_verbatim_command(&program, flags, body)
    {
        command = c;
    }
    command
        .env("PATH", path_without_shims)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("MISE_CREDENTIAL_HOST", host)
        .env("MISE_CREDENTIAL_PROVIDER", provider);
    // The command is the user's own, but the directory mise runs in may be a
    // project they have not trusted, and programs load code and config from
    // their working directory (`python3 -m`, git, `cmd` looking up `gh`). Run
    // it from the home directory, as `git credential fill` is run below. This
    // comes before `direct_command`, which copies the directory and environment.
    crate::git::detach_git_command(&mut command);
    // The helper's $0/$1 belong to sh, not to a directly executed program.
    if let Some(direct) = crate::inline_command::direct_command(
        &command,
        true,
        cmd,
        &[],
        crate::shells::implicit_inline_shell(),
    ) {
        command = direct;
    }
    Some(command)
}

fn credential_command_shell(cmd: &str, host: &str) -> Option<(String, Vec<String>)> {
    let shell = match crate::shells::default_inline_shell(&Settings::get()) {
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
    let mut command = std::process::Command::new("git");
    command
        .args(["credential", "fill"])
        .env("PATH", &path_without_shims)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    // A `credential.helper` from the repository mise happens to run in is a
    // shell command chosen by that project; use only the user's own helpers.
    crate::git::detach_git_command(&mut command);
    prepare_noninteractive_child(&mut command);
    let result = command.spawn().ok().and_then(|mut child| {
        let _running_pid = RunningPidGuard::new(Some(child.id()));
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

/// Build an `Authorization: Bearer <token>` header value.
///
/// Tokens reach mise from environment variables, credential helpers, config
/// files and `gh`/`glab` config, so a value containing a character that is
/// illegal in an HTTP header (a newline, say) is a user-configuration problem,
/// not a bug. Returning an error keeps it from aborting the process, and the
/// token never appears in the message.
pub fn bearer_header(provider: &str, token: &str) -> Result<HeaderValue> {
    let mut value = HeaderValue::from_str(format!("Bearer {token}").as_str()).map_err(|_| {
        eyre!(
            "invalid {provider} token: it contains characters that cannot be sent in an HTTP header (token redacted)"
        )
    })?;
    value.set_sensitive(true);
    Ok(value)
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
    let doc: Value = yaml::from_str(contents).ok()?;
    let mut out = HashMap::new();
    if let Some(map) = doc.as_mapping() {
        collect_mapping_tokens(map, &mut out);

        if let Some(hosts_value) = map.get(&Value::from("hosts"))
            && let Some(hosts) = hosts_value.as_mapping()
        {
            collect_mapping_tokens(hosts, &mut out);
        }

        if let Some(logins_value) = map.get(&Value::from("logins")) {
            collect_list_tokens(logins_value, &mut out);
        }
    }
    if out.is_empty() { None } else { Some(out) }
}

fn collect_mapping_tokens(map: &yaml::Mapping, out: &mut HashMap<String, String>) {
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
            .get(&Value::from("name"))
            .and_then(Value::as_str)
            .or_else(|| map.get(&Value::from("host")).and_then(Value::as_str))
            .or_else(|| map.get(&Value::from("url")).and_then(Value::as_str));
        if let (Some(host), Some(token)) = (host, token_from_entry(map)) {
            out.insert(host.to_string(), token);
        }
    }
}

pub(crate) fn token_from_entry(entry: &yaml::Mapping) -> Option<String> {
    ["oauth_token", "token", "access_token", "access-token"]
        .iter()
        .find_map(|k| {
            entry
                .get(&Value::from(*k))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(ToString::to_string)
        })
}

/// First candidate that is a regular file, else `fallback`.
///
/// Forge CLIs (`gh`, `glab`) each keep their config under a platform-dependent directory, and
/// mise probes the plausible ones in priority order. Factored out so that order is one rule with
/// one set of tests instead of a copy per forge — the copies are exactly how the Windows location
/// came to be missing from both.
///
/// `is_file` rather than `exists`: every caller hands the result to `read_to_string`, so a
/// directory that happens to be named `hosts.yml` must not shadow a real config further down the
/// list. Symlinks are followed, so a symlinked config still matches.
pub fn first_existing_file(candidates: Vec<PathBuf>, fallback: PathBuf) -> PathBuf {
    candidates
        .into_iter()
        .find(|p| p.is_file())
        .unwrap_or(fallback)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bearer_header_marks_the_value_sensitive() {
        let value = bearer_header("GitHub", "ghp_abc123").unwrap();
        assert_eq!(value, "Bearer ghp_abc123");
        assert!(
            value.is_sensitive(),
            "the Authorization value must be redacted from reqwest's debug output"
        );
    }

    /// #13471: this used to be `HeaderValue::from_str(..).unwrap()`, which
    /// aborted the process on a token containing a newline.
    #[test]
    fn test_bearer_header_rejects_invalid_characters_without_leaking_the_token() {
        for token in [
            "ghp_a\nsecret_value",
            "ghp_a\rsecret_value",
            "ghp_a\0secret_value",
        ] {
            let err = bearer_header("GitHub", token)
                .expect_err("{token:?} is not a legal header value")
                .to_string();
            assert!(err.contains("invalid GitHub token"), "{err}");
            assert!(
                !err.contains("secret_value"),
                "token leaked into error: {err}"
            );
        }
    }

    #[test]
    fn test_parse_tokens_toml_trims_and_drops_blank_tokens() {
        let tokens = parse_tokens_toml(
            r#"
[tokens."github.com"]
token = "  ghp_abc123\n"

[tokens."blank.example.com"]
token = "   "
"#,
        )
        .unwrap();

        assert_eq!(
            tokens.get("github.com").map(String::as_str),
            Some("ghp_abc123")
        );
        assert!(
            !tokens.contains_key("blank.example.com"),
            "a whitespace-only token is no token at all"
        );
    }

    #[test]
    fn test_first_existing_file_prefers_the_earliest_that_exists() {
        let dir = tempfile::tempdir().unwrap();
        let second = dir.path().join("second");
        let third = dir.path().join("third");
        std::fs::write(&second, "").unwrap();
        std::fs::write(&third, "").unwrap();
        assert_eq!(
            first_existing_file(
                vec![dir.path().join("first"), second.clone(), third],
                dir.path().join("fallback")
            ),
            second,
            "a missing earlier candidate must be skipped, not short-circuit the search"
        );
    }

    #[test]
    fn test_first_existing_file_skips_a_directory_of_the_same_name() {
        // A directory named like the config file must not shadow a real one further down:
        // the result is handed straight to `read_to_string`.
        let dir = tempfile::tempdir().unwrap();
        let decoy = dir.path().join("early").join("config.yml");
        std::fs::create_dir_all(&decoy).unwrap();
        let real = dir.path().join("late-config.yml");
        std::fs::write(&real, "").unwrap();
        assert_eq!(
            first_existing_file(vec![decoy, real.clone()], dir.path().join("fallback")),
            real
        );
    }

    #[test]
    fn test_first_existing_file_falls_back_when_none_exist() {
        let dir = tempfile::tempdir().unwrap();
        let fallback = dir.path().join("fallback");
        assert_eq!(
            first_existing_file(
                vec![dir.path().join("a"), dir.path().join("b")],
                fallback.clone()
            ),
            fallback,
            "the fallback is returned even though it does not exist -- callers use it to name \
             the conventional location in a trace message"
        );
    }

    #[test]
    fn test_first_existing_file_with_no_candidates() {
        let dir = tempfile::tempdir().unwrap();
        let fallback = dir.path().join("fallback");
        assert_eq!(first_existing_file(vec![], fallback.clone()), fallback);
    }

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
        let shell = shell_words::split("sh -o errexit -c").unwrap();
        let (program, args) =
            credential_command_shell_from(&shell, "echo token-for-$1", "ghe.example.com").unwrap();

        assert_eq!(program, "sh");
        assert_eq!(
            args,
            vec![
                "-o",
                "errexit",
                "-c",
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

    #[cfg(unix)]
    #[test]
    fn test_get_credential_command_token_caches_per_host() {
        // Same host on repeat calls must reuse the cached result (1 spawn).
        // Different hosts get their own cache entries — important for
        // multi-instance setups (e.g. github.com vs a GHE instance).
        let dir = tempfile::tempdir().unwrap();
        let counter = dir.path().join("counter");
        let cmd = format!(
            "echo invocation >> {} && echo \"token-for-$MISE_CREDENTIAL_HOST\"",
            counter.display()
        );
        let provider = "test-per-host";

        let a1 = get_credential_command_token(provider, &cmd, "host-a.example.com").unwrap();
        let a2 = get_credential_command_token(provider, &cmd, "host-a.example.com").unwrap();
        let b1 = get_credential_command_token(provider, &cmd, "host-b.example.com").unwrap();

        assert_eq!(a1, "token-for-host-a.example.com");
        assert_eq!(a2, "token-for-host-a.example.com");
        assert_eq!(b1, "token-for-host-b.example.com");

        let invocations = std::fs::read_to_string(&counter).unwrap().lines().count();
        assert_eq!(
            invocations, 2,
            "1 spawn per host: 1 for host-a, 1 for host-b"
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_credential_command_runs_outside_the_current_directory() {
        let cwd = std::env::current_dir().unwrap().canonicalize().unwrap();
        let home = crate::env::HOME.canonicalize().unwrap();
        // `pwd` is a shell builtin, so this runs through the shell.
        let pwd = get_credential_command_token("test-cwd", "pwd -P", "cwd.example.com")
            .expect("pwd prints the directory");
        assert_ne!(std::path::PathBuf::from(&pwd), cwd);
        assert_eq!(std::path::PathBuf::from(&pwd), home);
        // `env` is run without the shell, which must keep the directory.
        for cmd in ["pwd -P", "env"] {
            let command = credential_command_process("test-cwd", cmd, "example.com").unwrap();
            assert_eq!(
                command
                    .get_current_dir()
                    .map(|dir| dir.canonicalize().unwrap()),
                Some(home.clone()),
                "{cmd} would run in the directory mise was called in"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn test_credential_command_does_not_inherit_git_dir() {
        let inherited = "/inherited/git-dir";
        let mut env = crate::testing::EnvVarGuard::new();
        env.set("GIT_DIR", inherited)
            .set("GIT_WORK_TREE", inherited);
        // Shell startup variables and exported functions send even a plain
        // command through the shell, so they are cleared for the direct case.
        let functions: Vec<_> = std::env::vars_os()
            .map(|(key, _)| key)
            .filter(|key| key.to_string_lossy().starts_with("BASH_FUNC_"))
            .collect();
        for key in ["ENV", "BASH_ENV", "SHELLOPTS", "BASHOPTS"]
            .map(std::ffi::OsString::from)
            .into_iter()
            .chain(functions)
        {
            env.remove(key);
        }
        // A relative entry in the caller's PATH would send `env` through the
        // shell too, so the commands get only the absolute entries of the
        // caller's PATH, which keeps tools found outside /usr/bin (Nix).
        let caller_path = std::env::var_os("PATH").unwrap_or_default();
        let dirs: Vec<_> = std::env::split_paths(&caller_path)
            .filter(|dir| dir.is_absolute())
            .collect();
        if !dirs.iter().any(|dir| dir.join("env").is_file()) {
            eprintln!("skipped: no absolute PATH entry holds `env`");
            return;
        }
        let path = std::env::join_paths(dirs).unwrap();
        let path = path.as_os_str();
        // The first runs through the shell, the second without it.
        for (cmd, direct) in [
            ("echo \"[$GIT_DIR][$GIT_WORK_TREE]\"", false),
            ("env", true),
        ] {
            let mut command =
                credential_command_process_with_path("test-git-dir", cmd, "example.com", path)
                    .unwrap();
            assert_eq!(
                std::path::Path::new(command.get_program()).file_name() == Some("env".as_ref()),
                direct,
                "{cmd} took an unexpected path"
            );
            let output = command.output().unwrap();
            // `env` prints the whole environment, which can hold credentials,
            // so a failure names variables and never shows their values.
            assert!(output.status.success(), "{cmd} failed: {}", output.status);
            let stdout = String::from_utf8(output.stdout).unwrap();
            let inherited_in: Vec<&str> = stdout
                .lines()
                .filter(|line| line.contains(inherited))
                .map(|line| line.split_once('=').map_or(line, |(name, _)| name))
                .collect();
            assert!(
                inherited_in.is_empty(),
                "{cmd} inherited it in {inherited_in:?}"
            );
        }
    }

    #[test]
    fn test_is_plain_host() {
        for host in [
            "github.com",
            "acme.ghe.com",
            "GHE.Example.com",
            "xn--bcher-kva.example",
            "my_host.internal",
            "localhost:8080",
            "127.0.0.1",
            "[::1]",
            "[2001:db8::1]:8443",
        ] {
            assert!(is_plain_host(host), "{host}");
        }
        for host in [
            "",
            "a&evil.bat&b.ghe.com",
            "a;{touch,x};b.ghe.com",
            "a\"b.ghe.com",
            "$(id).ghe.com",
            "`id`.ghe.com",
            "a'b.ghe.com",
            "a=b.ghe.com",
            "a b.ghe.com",
            "a%PATH%.ghe.com",
            "a\nb.ghe.com",
            "-h.ghe.com",
            ".ghe.com",
            "bücher.example",
        ] {
            assert!(!is_plain_host(host), "{host}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn test_get_credential_command_token_skips_a_host_a_shell_would_parse() {
        // The helper stands in for `cmd /c "... %MISE_CREDENTIAL_HOST%"`: the
        // host is substituted into text that a shell then parses.
        let dir = tempfile::tempdir().unwrap();
        let ran = dir.path().join("ran");
        let cmd = format!(
            "echo helper >> {} && sh -c \"echo token-for-$MISE_CREDENTIAL_HOST\"",
            ran.display()
        );
        let provider = "test-shell-host";

        assert_eq!(
            get_credential_command_token(provider, &cmd, "a;echo${IFS}injected;b.ghe.com"),
            None
        );
        assert!(!ran.exists(), "the helper must not run for such a host");
        assert_eq!(
            get_credential_command_token(provider, &cmd, "acme.ghe.com").as_deref(),
            Some("token-for-acme.ghe.com")
        );
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
