//! Registry credential resolution for `mise oci push` (and private pulls).
//!
//! Reads the same credential sources as docker / podman / skopeo so that
//! `docker login` / `podman login` "just work" with the native push client:
//!
//! 1. `$REGISTRY_AUTH_FILE` (podman/skopeo convention)
//! 2. `$XDG_RUNTIME_DIR/containers/auth.json` (podman default)
//! 3. `$XDG_CONFIG_HOME/containers/auth.json`
//! 4. `$DOCKER_CONFIG/config.json`, falling back to `~/.docker/config.json`
//!
//! Within a file, credentials come from (in order) `credHelpers.<host>`,
//! inline `auths` entries, then the global `credsStore`. Credential helpers
//! are the `docker-credential-<name>` executables (osxkeychain, desktop,
//! ecr-login, gcloud, …) that docker itself shells out to.

use std::path::PathBuf;
use std::process::{Command, Stdio};

use base64::Engine;
use base64::prelude::BASE64_STANDARD;
use eyre::{Context, Result, bail};
use serde::Deserialize;

use crate::env;

/// A username/secret pair for registry auth. `username == "<token>"` is the
/// docker credential-helper convention for an identity token; the OCI token
/// endpoint treats it the same as a password grant, so we don't special-case
/// it beyond passing it through.
#[derive(Debug, Clone)]
pub struct Credential {
    pub username: String,
    pub secret: String,
}

impl Credential {
    pub fn basic_auth_header(&self) -> String {
        let raw = format!("{}:{}", self.username, self.secret);
        format!("Basic {}", BASE64_STANDARD.encode(raw))
    }
}

/// docker/podman `config.json` / `auth.json` — only the fields we read.
#[derive(Debug, Default, Deserialize)]
struct AuthFile {
    #[serde(default)]
    auths: indexmap::IndexMap<String, AuthEntry>,
    #[serde(default, rename = "credHelpers")]
    cred_helpers: indexmap::IndexMap<String, String>,
    #[serde(default, rename = "credsStore")]
    creds_store: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct AuthEntry {
    #[serde(default)]
    auth: Option<String>,
    #[serde(default)]
    username: Option<String>,
    #[serde(default)]
    password: Option<String>,
    #[serde(default, rename = "identitytoken")]
    identity_token: Option<String>,
}

/// Resolve credentials for `registry` (a bare host like `ghcr.io` or
/// `docker.io`). Returns `None` when no credential source has an entry —
/// the caller should proceed anonymously.
pub fn resolve_credential(registry: &str) -> Result<Option<Credential>> {
    for path in auth_file_paths() {
        if !path.is_file() {
            continue;
        }
        // No single source should hard-fail resolution — a later file (or
        // anonymous access) may still work. Warn and move on for unreadable,
        // malformed, or otherwise erroring files.
        let raw = match crate::file::read_to_string(&path) {
            Ok(raw) => raw,
            Err(e) => {
                warn!("skipping unreadable auth file {}: {e}", path.display());
                continue;
            }
        };
        let file: AuthFile = match serde_json::from_str(&raw) {
            Ok(f) => f,
            Err(e) => {
                warn!("skipping malformed auth file {}: {e}", path.display());
                continue;
            }
        };
        match credential_from_file(&file, registry) {
            Ok(Some(cred)) => {
                debug!("using registry credentials from {}", path.display());
                return Ok(Some(cred));
            }
            Ok(None) => {}
            Err(e) => warn!("skipping auth file {} ({e})", path.display()),
        }
    }
    Ok(None)
}

fn auth_file_paths() -> Vec<PathBuf> {
    let mut paths = vec![];
    if let Some(p) = env::var_os("REGISTRY_AUTH_FILE") {
        paths.push(PathBuf::from(p));
    }
    if let Some(p) = env::var_os("XDG_RUNTIME_DIR") {
        paths.push(PathBuf::from(p).join("containers/auth.json"));
    }
    paths.push(env::XDG_CONFIG_HOME.join("containers/auth.json"));
    if let Some(p) = env::var_os("DOCKER_CONFIG") {
        paths.push(PathBuf::from(p).join("config.json"));
    } else {
        paths.push(env::HOME.join(".docker/config.json"));
    }
    paths
}

fn credential_from_file(file: &AuthFile, registry: &str) -> Result<Option<Credential>> {
    let aliases = registry_aliases(registry);

    // 1. Per-registry credential helper. A helper that exits non-zero when it
    // has no entry for the host (the docker "credentials not found"
    // convention) is a miss, not a failure — fall through to inline auths /
    // credsStore / the next file rather than aborting the whole push.
    for (key, helper) in &file.cred_helpers {
        if key_matches(key, &aliases) {
            match run_credential_helper(helper, registry) {
                Ok(cred) => return Ok(Some(cred)),
                Err(e) => {
                    debug!("credHelpers helper {helper} has no credentials for {registry}: {e}");
                }
            }
        }
    }

    // 2. Inline auths.
    for (key, entry) in &file.auths {
        if !key_matches(key, &aliases) {
            continue;
        }
        if let Some(cred) = credential_from_entry(entry, key)? {
            return Ok(Some(cred));
        }
    }

    // 3. Global credential store. Only consult it if the file mentions one —
    // and treat "helper has no entry for this registry" as a miss rather
    // than an error so resolution can fall through to the next file.
    if let Some(helper) = &file.creds_store {
        match run_credential_helper(helper, registry) {
            Ok(cred) => return Ok(Some(cred)),
            Err(e) => {
                debug!("credsStore helper {helper} has no credentials for {registry}: {e}");
            }
        }
    }

    Ok(None)
}

fn credential_from_entry(entry: &AuthEntry, key: &str) -> Result<Option<Credential>> {
    let (mut username, mut secret) = (entry.username.clone(), entry.password.clone());
    if let Some(auth) = entry.auth.as_deref().filter(|a| !a.is_empty()) {
        let decoded = BASE64_STANDARD
            .decode(auth.trim())
            .wrap_err_with(|| format!("decoding base64 `auth` for {key}"))?;
        let decoded = String::from_utf8(decoded)
            .wrap_err_with(|| format!("`auth` for {key} is not valid UTF-8"))?;
        let Some((u, p)) = decoded.split_once(':') else {
            bail!("`auth` for {key} is not `user:password`");
        };
        username = Some(u.to_string());
        secret = Some(p.to_string());
    }
    // An identity token (docker.io "Docker Desktop" login flow) replaces the
    // password; the username from `auth` is ignored by registries in this
    // mode but `<token>` is the conventional placeholder.
    if let Some(token) = entry.identity_token.as_deref().filter(|t| !t.is_empty()) {
        return Ok(Some(Credential {
            username: "<token>".to_string(),
            secret: token.to_string(),
        }));
    }
    match (username, secret) {
        (Some(u), Some(p)) => Ok(Some(Credential {
            username: u,
            secret: p,
        })),
        _ => Ok(None),
    }
}

/// All keys under which credentials for `registry` may be stored. Docker Hub
/// is the messy one — logins are stored under the legacy v1 endpoint.
fn registry_aliases(registry: &str) -> Vec<String> {
    let mut aliases = vec![registry.to_string()];
    if matches!(
        registry,
        "docker.io" | "index.docker.io" | "registry-1.docker.io"
    ) {
        aliases.extend([
            "docker.io".into(),
            "index.docker.io".into(),
            "registry-1.docker.io".into(),
            "https://index.docker.io/v1/".into(),
        ]);
    }
    aliases
}

/// Match an auth-file key against the alias list. Keys may carry a scheme
/// and/or path (`https://ghcr.io/v2/`); compare on the host portion.
fn key_matches(key: &str, aliases: &[String]) -> bool {
    if aliases.iter().any(|a| a == key) {
        return true;
    }
    let stripped = key
        .trim_start_matches("https://")
        .trim_start_matches("http://");
    let host = stripped.split('/').next().unwrap_or(stripped);
    aliases.iter().any(|a| a == host)
}

/// Shell out to `docker-credential-<helper> get`, passing the registry on
/// stdin, and parse the `{"Username": …, "Secret": …}` response — the same
/// protocol docker itself uses.
fn run_credential_helper(helper: &str, registry: &str) -> Result<Credential> {
    let bin = format!("docker-credential-{helper}");
    // Docker Hub helpers store the credential under the legacy v1 URL.
    let server = if registry == "docker.io" || registry == "registry-1.docker.io" {
        "https://index.docker.io/v1/"
    } else {
        registry
    };
    debug!("running {bin} get for {server}");
    let mut child = Command::new(&bin)
        .arg("get")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .wrap_err_with(|| format!("spawning {bin} (from credHelpers/credsStore)"))?;
    {
        use std::io::Write;
        let mut stdin = child.stdin.take().expect("stdin piped");
        stdin.write_all(server.as_bytes())?;
    }
    let out = child.wait_with_output()?;
    if !out.status.success() {
        bail!(
            "{bin} get failed for {server}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    #[derive(Deserialize)]
    struct HelperResponse {
        #[serde(rename = "Username")]
        username: String,
        #[serde(rename = "Secret")]
        secret: String,
    }
    let resp: HelperResponse = serde_json::from_slice(&out.stdout)
        .wrap_err_with(|| format!("parsing {bin} get output"))?;
    Ok(Credential {
        username: resp.username,
        secret: resp.secret,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> AuthFile {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn inline_auth_base64() {
        let file = parse(r#"{"auths": {"ghcr.io": {"auth": "dXNlcjpwYXNz"}}}"#);
        let cred = credential_from_file(&file, "ghcr.io").unwrap().unwrap();
        assert_eq!(cred.username, "user");
        assert_eq!(cred.secret, "pass");
    }

    #[test]
    fn inline_auth_username_password() {
        let file = parse(r#"{"auths": {"ghcr.io": {"username": "u", "password": "p"}}}"#);
        let cred = credential_from_file(&file, "ghcr.io").unwrap().unwrap();
        assert_eq!(cred.username, "u");
        assert_eq!(cred.secret, "p");
    }

    #[test]
    fn dockerhub_legacy_key_matches() {
        let file = parse(r#"{"auths": {"https://index.docker.io/v1/": {"auth": "dXNlcjpwYXNz"}}}"#);
        let cred = credential_from_file(&file, "docker.io").unwrap().unwrap();
        assert_eq!(cred.username, "user");
    }

    #[test]
    fn scheme_prefixed_key_matches() {
        let file = parse(r#"{"auths": {"https://ghcr.io": {"auth": "dXNlcjpwYXNz"}}}"#);
        assert!(credential_from_file(&file, "ghcr.io").unwrap().is_some());
    }

    #[test]
    fn identity_token_wins() {
        let file =
            parse(r#"{"auths": {"ghcr.io": {"auth": "dXNlcjpwYXNz", "identitytoken": "idtok"}}}"#);
        let cred = credential_from_file(&file, "ghcr.io").unwrap().unwrap();
        assert_eq!(cred.username, "<token>");
        assert_eq!(cred.secret, "idtok");
    }

    #[test]
    fn no_entry_returns_none() {
        let file = parse(r#"{"auths": {"ghcr.io": {"auth": "dXNlcjpwYXNz"}}}"#);
        assert!(credential_from_file(&file, "quay.io").unwrap().is_none());
    }

    #[test]
    fn empty_auth_entry_is_none() {
        // `docker logout` can leave empty entries behind.
        let file = parse(r#"{"auths": {"ghcr.io": {}}}"#);
        assert!(credential_from_file(&file, "ghcr.io").unwrap().is_none());
    }

    #[test]
    fn malformed_base64_errors() {
        let file = parse(r#"{"auths": {"ghcr.io": {"auth": "!!!"}}}"#);
        assert!(credential_from_file(&file, "ghcr.io").is_err());
    }

    #[test]
    fn basic_auth_header_roundtrip() {
        let cred = Credential {
            username: "user".into(),
            secret: "pass".into(),
        };
        assert_eq!(cred.basic_auth_header(), "Basic dXNlcjpwYXNz");
    }
}
