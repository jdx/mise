//! Minimal npm registry HTTP client used for version metadata queries.
//!
//! Replaces shelling out to `npm view`, so listing versions for `npm:` tools
//! does not require node/npm to be installed. Config semantics (`.npmrc`
//! parsing, `${VAR}` expansion, scoped registries, nerf-dart auth lookup,
//! `NPM_CONFIG_*` env translation) are ported from the `aube-registry` crate
//! (<https://github.com/jdx/aube>), trimmed to what read-only packument
//! fetches need.
//!
//! Scope notes, matching what `npm view` did when invoked with a neutral
//! `--prefix` (the previous behavior):
//! - user-level `~/.npmrc` (or `NPM_CONFIG_USERCONFIG`) and `NPM_CONFIG_*` /
//!   `npm_config_*` env vars apply; project `.npmrc` files do not
//! - proxies come from the standard `HTTP(S)_PROXY` env vars via mise's HTTP
//!   client; npm-only TLS knobs (`cafile`, client certs) and token helpers are
//!   not supported — set `npm.use_npm_view = true` to keep using the npm CLI
//!   for metadata in those environments

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::LazyLock as Lazy;

use base64::Engine;
use eyre::{Result, eyre};
use indexmap::IndexMap;
use reqwest::header::{HeaderMap, HeaderValue};
use serde::Deserialize;

use crate::env;

pub static NPM_REGISTRY_CONFIG: Lazy<NpmRegistryConfig> = Lazy::new(NpmRegistryConfig::load);

const DEFAULT_REGISTRY: &str = "https://registry.npmjs.org/";
/// Accept header for the full packument route. The full document (not the
/// abbreviated `vnd.npm.install-v1+json` corgi) is required because only it
/// carries the `time` map used for release dates / `minimum_release_age`.
const PACKUMENT_ACCEPT: &str = "application/json; q=1.0, */*";

#[derive(Debug, Default)]
pub struct NpmRegistryConfig {
    /// Default registry URL, normalized with a trailing slash.
    registry: String,
    /// Scoped registry overrides: "@scope" -> registry URL.
    scoped_registries: BTreeMap<String, String>,
    /// Auth entries keyed by normalized nerf-dart URI ("//host/path/").
    auth_by_uri: BTreeMap<String, NpmAuth>,
}

#[derive(Debug, Default, Clone)]
struct NpmAuth {
    /// `_authToken` — sent as `Authorization: Bearer <token>`.
    auth_token: Option<String>,
    /// `_auth` — pre-encoded base64("user:pass"), sent as `Basic`.
    auth: Option<String>,
    username: Option<String>,
    /// `_password` — npm stores this base64-encoded.
    password: Option<String>,
}

impl NpmAuth {
    fn authorization_header(&self) -> Option<String> {
        if let Some(token) = &self.auth_token {
            return Some(format!("Bearer {token}"));
        }
        if let Some(auth) = &self.auth {
            return Some(format!("Basic {auth}"));
        }
        let username = self.username.as_ref()?;
        let password = base64::engine::general_purpose::STANDARD
            .decode(self.password.as_ref()?)
            .ok()?;
        let mut raw = Vec::with_capacity(username.len() + 1 + password.len());
        raw.extend_from_slice(username.as_bytes());
        raw.push(b':');
        raw.extend_from_slice(&password);
        Some(format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode(raw)
        ))
    }
}

impl NpmRegistryConfig {
    pub fn load() -> Self {
        let env_vars: Vec<(String, String)> = env::vars_safe().collect();
        Self::load_with_env(user_npmrc_path(&env_vars), &env_vars)
    }

    fn load_with_env(user_npmrc: Option<PathBuf>, env_vars: &[(String, String)]) -> Self {
        let mut config = Self {
            registry: DEFAULT_REGISTRY.to_string(),
            ..Default::default()
        };
        let mut entries = Vec::new();
        if let Some(path) = user_npmrc
            && path.is_file()
        {
            match std::fs::read_to_string(&path) {
                Ok(content) => entries.extend(parse_npmrc(&content, Some(env_vars))),
                Err(err) => debug!("failed to read {}: {err}", path.display()),
            }
        }
        // Env entries apply after .npmrc entries so last-write-wins gives env
        // the higher precedence npm documents.
        entries.extend(npm_config_env_entries(env_vars));
        config.apply(entries);
        config
    }

    fn apply(&mut self, entries: Vec<(String, String)>) {
        // Bare `_authToken` / `_auth` (no `//uri` prefix) apply to whatever
        // default registry ends up configured, so collect them and resolve
        // after all entries are seen.
        let mut bare_auth_token = None;
        let mut bare_auth = None;
        for (key, value) in entries {
            if value.is_empty() {
                continue;
            }
            if key == "registry" {
                self.registry = normalize_registry_url(&value);
            } else if let Some(rest) = key.strip_prefix('@') {
                if let Some((scope, tail)) = rest.split_once(':')
                    && tail.eq_ignore_ascii_case("registry")
                {
                    self.scoped_registries.insert(
                        format!("@{}", scope.to_ascii_lowercase()),
                        normalize_registry_url(&value),
                    );
                }
            } else if key.starts_with("//") {
                let Some((uri, suffix)) = key.rsplit_once(':') else {
                    continue;
                };
                // Scoped per-URI auth (`//host/:@scope:_authToken`) is not
                // supported; skip rather than send a token for the wrong
                // packages.
                if uri
                    .rsplit_once(':')
                    .is_some_and(|(_, s)| s.starts_with('@'))
                {
                    debug!("ignoring scoped npm auth key: {key}");
                    continue;
                }
                let uri_key = normalize_npmrc_uri_key(uri);
                let entry = self.auth_by_uri.entry(uri_key).or_default();
                match suffix {
                    "_authToken" => entry.auth_token = Some(value),
                    "_auth" => entry.auth = Some(value),
                    "username" => entry.username = Some(value),
                    "_password" => entry.password = Some(value),
                    _ => {}
                }
            } else if key == "_authToken" {
                bare_auth_token = Some(value);
            } else if key == "_auth" {
                bare_auth = Some(value);
            }
        }
        if bare_auth_token.is_some() || bare_auth.is_some() {
            let uri_key = registry_uri_key(&self.registry);
            let entry = self.auth_by_uri.entry(uri_key).or_default();
            if entry.auth_token.is_none() {
                entry.auth_token = bare_auth_token;
            }
            if entry.auth.is_none() {
                entry.auth = bare_auth;
            }
        }
    }

    /// Registry URL for a package, honoring `@scope:registry` overrides.
    pub fn registry_for(&self, name: &str) -> &str {
        package_scope(name)
            .and_then(|scope| self.scoped_registries.get(scope))
            .unwrap_or(&self.registry)
    }

    fn authorization_for(&self, registry_url: &str) -> Option<String> {
        let uri_key = registry_uri_key(registry_url);
        lookup_by_uri_prefix(&self.auth_by_uri, &uri_key)?.authorization_header()
    }

    /// Fetch the full packument for a package from its configured registry.
    pub async fn fetch_packument(&self, name: &str) -> Result<Packument> {
        let registry = self.registry_for(name);
        let url = format!("{registry}{}", encode_package_name(name));
        let mut headers = HeaderMap::new();
        headers.insert("Accept", HeaderValue::from_static(PACKUMENT_ACCEPT));
        if let Some(auth) = self.authorization_for(registry) {
            let mut value = HeaderValue::from_str(&auth)
                .map_err(|err| eyre!("invalid npm auth for {registry}: {err}"))?;
            value.set_sensitive(true);
            headers.insert("Authorization", value);
        }
        let packument: Packument = crate::http::HTTP_FETCH
            .json_with_headers(&url, &headers)
            .await
            .map_err(|err| eyre!("failed to fetch npm metadata for {name} from {url}: {err}"))?;
        Ok(packument)
    }
}

/// Full packument, trimmed to the fields version listing needs.
#[derive(Debug, Deserialize)]
pub struct Packument {
    #[serde(default)]
    versions: IndexMap<String, serde::de::IgnoredAny>,
    #[serde(default, rename = "dist-tags")]
    dist_tags: serde_json::Value,
    #[serde(default)]
    time: serde_json::Value,
}

impl Packument {
    /// Versions sorted ascending by semver with their publish timestamps.
    ///
    /// The packument document is in publish order, which differs from semver
    /// order when maintainers backport patches to old release lines. `npm
    /// view versions` sorted semver-ascending, and prefix resolution (e.g.
    /// `npm:@angular/cli@19`) picks the last match, so the sort is load-bearing.
    /// npm registry versions are strictly semver (the registry enforces it);
    /// anything unparseable keeps document order at the end.
    pub fn versions_with_time(&self) -> Vec<(&str, Option<&str>)> {
        let time = self.time.as_object();
        let mut versions: Vec<&str> = self.versions.keys().map(|v| v.as_str()).collect();
        versions.sort_by_cached_key(|v| {
            let parsed = versions::SemVer::new(v);
            (parsed.is_none(), parsed)
        });
        versions
            .into_iter()
            .map(|version| {
                let created_at = time
                    .and_then(|time| time.get(version))
                    .and_then(|v| v.as_str());
                (version, created_at)
            })
            .collect()
    }

    pub fn latest_dist_tag(&self) -> Option<String> {
        Some(self.dist_tags.get("latest")?.as_str()?.to_string())
    }
}

/// The user-level npmrc path: `NPM_CONFIG_USERCONFIG` or `~/.npmrc`.
fn user_npmrc_path(env_vars: &[(String, String)]) -> Option<PathBuf> {
    for (key, value) in env_vars {
        if (key.eq_ignore_ascii_case("npm_config_userconfig")) && !value.is_empty() {
            return Some(PathBuf::from(value));
        }
    }
    Some(env::HOME.join(".npmrc"))
}

/// Parse .npmrc content into key=value pairs. Supports comments (`#`/`;`),
/// backslash line continuation, matched-quote stripping, and `${VAR}`
/// expansion (when `expand_env` is provided), mirroring npm's `ini` parser.
fn parse_npmrc(content: &str, expand_env: Option<&[(String, String)]>) -> Vec<(String, String)> {
    let content = content.strip_prefix('\u{feff}').unwrap_or(content);
    let mut entries = Vec::new();

    // Fold backslash-continuation before line iteration: a trailing `\` joins
    // the next physical line, used for long auth tokens.
    let mut logical: Vec<String> = Vec::new();
    let mut acc = String::new();
    for raw in content.lines() {
        if let Some(stripped) = raw.strip_suffix('\\') {
            acc.push_str(stripped);
            continue;
        }
        acc.push_str(raw);
        logical.push(std::mem::take(&mut acc));
    }
    if !acc.is_empty() {
        logical.push(acc);
    }

    for line in &logical {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            let key = maybe_substitute_env(key.trim(), expand_env);
            let value = maybe_substitute_env(strip_matched_quotes(value.trim()), expand_env);
            entries.push((key, value));
        }
    }
    entries
}

fn maybe_substitute_env(value: &str, expand_env: Option<&[(String, String)]>) -> String {
    match expand_env {
        Some(env_vars) => substitute_env(value, env_vars),
        None => value.to_string(),
    }
}

/// Strip a single layer of matched surrounding `"` or `'`, mirroring npm's
/// `ini` parser (`_auth="abc=="` keeps its `=` padding).
fn strip_matched_quotes(value: &str) -> &str {
    let bytes = value.as_bytes();
    if bytes.len() >= 2
        && (bytes[0] == b'"' || bytes[0] == b'\'')
        && bytes[bytes.len() - 1] == bytes[0]
    {
        &value[1..value.len() - 1]
    } else {
        value
    }
}

/// Substitute `${VAR}` references from the given env snapshot.
fn substitute_env(value: &str, env_vars: &[(String, String)]) -> String {
    let mut result = String::with_capacity(value.len());
    let mut chars = value.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '$' && chars.peek() == Some(&'{') {
            chars.next(); // consume '{'
            let mut var_name = String::new();
            for c in chars.by_ref() {
                if c == '}' {
                    break;
                }
                var_name.push(c);
            }
            if let Some((_, val)) = env_vars.iter().find(|(k, _)| *k == var_name) {
                result.push_str(val);
            }
        } else {
            result.push(c);
        }
    }
    result
}

/// Synthesize `.npmrc`-style entries from `npm_config_*` / `NPM_CONFIG_*` env
/// vars. Only registry/scoped-registry/auth keys are translated; everything
/// else is owned by other subsystems (proxies come from the standard env vars
/// via mise's HTTP client).
fn npm_config_env_entries(env_vars: &[(String, String)]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut uri_scoped = Vec::new();
    for (name, value) in env_vars {
        if value.is_empty() {
            continue;
        }
        let Some(suffix) = strip_npm_config_prefix(name) else {
            continue;
        };
        // Per-URI auth keys carry `.npmrc` syntax in the env-var name
        // (e.g. `npm_config_//registry.example.com/:_authToken`).
        if suffix.starts_with("//") && is_url_scoped_env_auth_key(suffix) {
            uri_scoped.push((suffix.to_string(), value.clone()));
            continue;
        }
        // Scoped-registry keys: `@myorg:registry` (scope lowercased; npm
        // scopes are case-insensitive on the registry side).
        if let Some(rest) = suffix.strip_prefix('@')
            && let Some((scope, tail)) = rest.split_once(':')
            && tail.eq_ignore_ascii_case("registry")
        {
            out.push((
                format!("@{}:registry", scope.to_ascii_lowercase()),
                value.clone(),
            ));
            continue;
        }
        if suffix.eq_ignore_ascii_case("registry") {
            out.push(("registry".to_string(), value.clone()));
        }
    }
    out.extend(uri_scoped);
    out
}

fn strip_npm_config_prefix(name: &str) -> Option<&str> {
    let prefix = "npm_config_";
    let candidate = name.get(..prefix.len())?;
    if candidate.eq_ignore_ascii_case(prefix) {
        Some(&name[prefix.len()..])
    } else {
        None
    }
}

fn is_url_scoped_env_auth_key(key: &str) -> bool {
    key.rsplit_once(':').is_some_and(|(_, suffix)| {
        matches!(suffix, "_authToken" | "_auth" | "username" | "_password")
    })
}

/// Extract the scope from a package name ("@myorg/pkg" -> "@myorg").
fn package_scope(name: &str) -> Option<&str> {
    if name.starts_with('@') {
        name.find('/').map(|idx| &name[..idx])
    } else {
        None
    }
}

/// Scoped package names URL-encode the separating slash.
fn encode_package_name(name: &str) -> String {
    if name.starts_with('@') {
        name.replacen('/', "%2F", 1)
    } else {
        name.to_string()
    }
}

/// Ensure a registry URL ends with a trailing slash.
fn normalize_registry_url(url: &str) -> String {
    let url = url.trim();
    if url.ends_with('/') {
        url.to_string()
    } else {
        format!("{url}/")
    }
}

/// Convert a registry URL to the nerf-dart URI key used in .npmrc auth
/// lookups: "https://registry.example.com/" -> "//registry.example.com/".
/// Strips only the scheme's own default port (`:443` for https, `:80` for
/// http), matching npm's behavior.
fn registry_uri_key(url: &str) -> String {
    let (rest, default_port) = if let Some(rest) = url.strip_prefix("https:") {
        (rest, ":443")
    } else if let Some(rest) = url.strip_prefix("http:") {
        (rest, ":80")
    } else {
        return url.to_string();
    };
    strip_authority_port_suffix(rest, default_port)
}

/// Normalize an `//host[:port]/path` key from `.npmrc` so it matches what
/// `registry_uri_key` produces on the lookup side. Ingest can't know the
/// intended scheme (npmrc keys are scheme-less), so both default ports are
/// stripped.
fn normalize_npmrc_uri_key(key: &str) -> String {
    let stripped = strip_authority_port_suffix(key, ":443");
    if stripped != key {
        return stripped;
    }
    strip_authority_port_suffix(key, ":80")
}

fn strip_authority_port_suffix(key: &str, port_suffix: &str) -> String {
    let Some(after) = key.strip_prefix("//") else {
        return key.to_string();
    };
    let (authority, path) = match after.find('/') {
        Some(idx) => (&after[..idx], &after[idx..]),
        None => (after, ""),
    };
    let Some(authority) = authority.strip_suffix(port_suffix) else {
        return key.to_string();
    };
    format!("//{authority}{path}")
}

/// Look up `key`, falling back to longest-prefix matching by trimming path
/// segments from the right (npm/pnpm auth resolution). Stops before falling
/// all the way to the host-less `//` prefix.
fn lookup_by_uri_prefix<'a, V>(map: &'a BTreeMap<String, V>, key: &str) -> Option<&'a V> {
    if let Some(v) = map.get(key) {
        return Some(v);
    }
    let trimmed = key.trim_end_matches('/');
    if !trimmed.is_empty()
        && trimmed != key
        && let Some(v) = map.get(trimmed)
    {
        return Some(v);
    }
    let mut cursor = trimmed;
    while let Some(idx) = cursor.rfind('/') {
        cursor = &cursor[..idx];
        if cursor.len() <= 2 {
            break;
        }
        let with_slash = format!("{cursor}/");
        if let Some(v) = map.get(&with_slash) {
            return Some(v);
        }
        if let Some(v) = map.get(cursor) {
            return Some(v);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(entries: &[(&str, &str)]) -> Vec<(String, String)> {
        entries
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn config_from(npmrc: &str, env_vars: &[(String, String)]) -> NpmRegistryConfig {
        let mut config = NpmRegistryConfig {
            registry: DEFAULT_REGISTRY.to_string(),
            ..Default::default()
        };
        let mut entries = parse_npmrc(npmrc, Some(env_vars));
        entries.extend(npm_config_env_entries(env_vars));
        config.apply(entries);
        config
    }

    #[test]
    fn test_parse_npmrc_basics() {
        let entries = parse_npmrc(
            "# comment\n; also comment\nregistry=https://r.example.com\n_auth=\"abc==\"\n",
            None,
        );
        assert_eq!(
            entries,
            vec![
                ("registry".to_string(), "https://r.example.com".to_string()),
                ("_auth".to_string(), "abc==".to_string()),
            ]
        );
    }

    #[test]
    fn test_parse_npmrc_line_continuation() {
        let entries = parse_npmrc("//h.example.com/:_authToken=abc\\\ndef\n", None);
        assert_eq!(
            entries,
            vec![(
                "//h.example.com/:_authToken".to_string(),
                "abcdef".to_string()
            )]
        );
    }

    #[test]
    fn test_parse_npmrc_env_expansion() {
        let env_vars = env(&[("NPM_TOKEN", "secret")]);
        let entries = parse_npmrc(
            "//registry.npmjs.org/:_authToken=${NPM_TOKEN}\n",
            Some(&env_vars),
        );
        assert_eq!(entries[0].1, "secret");
    }

    #[test]
    fn test_registry_for_scoped() {
        let config = config_from(
            "registry=https://r.example.com\n@myorg:registry=https://scoped.example.com\n",
            &[],
        );
        assert_eq!(config.registry_for("left-pad"), "https://r.example.com/");
        assert_eq!(
            config.registry_for("@myorg/tool"),
            "https://scoped.example.com/"
        );
        assert_eq!(config.registry_for("@other/tool"), "https://r.example.com/");
    }

    #[test]
    fn test_env_overrides_npmrc() {
        let env_vars = env(&[("NPM_CONFIG_REGISTRY", "https://env.example.com")]);
        let config = config_from("registry=https://file.example.com\n", &env_vars);
        assert_eq!(config.registry_for("x"), "https://env.example.com/");
    }

    #[test]
    fn test_env_scoped_registry() {
        let env_vars = env(&[("npm_config_@MyOrg:registry", "https://env.example.com")]);
        let config = config_from("", &env_vars);
        assert_eq!(
            config.registry_for("@myorg/tool"),
            "https://env.example.com/"
        );
    }

    #[test]
    fn test_auth_token_lookup_with_default_port() {
        let config = config_from(
            "registry=https://r.example.com:443/npm/\n//r.example.com/npm/:_authToken=tok\n",
            &[],
        );
        assert_eq!(
            config.authorization_for(config.registry_for("x")),
            Some("Bearer tok".to_string())
        );
    }

    #[test]
    fn test_auth_prefix_fallback() {
        let config = config_from("//r.example.com/:_authToken=tok\n", &[]);
        assert_eq!(
            config.authorization_for("https://r.example.com/deep/path/"),
            Some("Bearer tok".to_string())
        );
        assert_eq!(config.authorization_for("https://other.example.com/"), None);
    }

    #[test]
    fn test_basic_auth_from_username_password() {
        // base64("s3cret") == "czNjcmV0"
        let config = config_from(
            "//r.example.com/:username=user\n//r.example.com/:_password=czNjcmV0\n",
            &[],
        );
        // base64("user:s3cret") == "dXNlcjpzM2NyZXQ="
        assert_eq!(
            config.authorization_for("https://r.example.com/"),
            Some("Basic dXNlcjpzM2NyZXQ=".to_string())
        );
    }

    #[test]
    fn test_bare_auth_applies_to_default_registry() {
        let config = config_from("registry=https://r.example.com\n_authToken=tok\n", &[]);
        assert_eq!(
            config.authorization_for("https://r.example.com/"),
            Some("Bearer tok".to_string())
        );
    }

    #[test]
    fn test_env_uri_scoped_auth() {
        let env_vars = env(&[("npm_config_//r.example.com/:_authToken", "envtok")]);
        let config = config_from("", &env_vars);
        assert_eq!(
            config.authorization_for("https://r.example.com/"),
            Some("Bearer envtok".to_string())
        );
    }

    #[test]
    fn test_scoped_per_uri_auth_ignored() {
        let config = config_from("//r.example.com/:@myorg:_authToken=tok\n", &[]);
        assert_eq!(config.authorization_for("https://r.example.com/"), None);
    }

    #[test]
    fn test_encode_package_name() {
        assert_eq!(encode_package_name("left-pad"), "left-pad");
        assert_eq!(encode_package_name("@myorg/tool"), "@myorg%2Ftool");
    }

    #[test]
    fn test_packument_versions_semver_sorted_with_time() {
        // Document order is publish order (1.2.0 backported after 1.10.0);
        // output must be semver-ascending like `npm view versions` was.
        let packument: Packument = serde_json::from_str(
            r#"{
                "name": "x",
                "dist-tags": {"latest": "2.0.0"},
                "versions": {"1.0.0": {}, "1.10.0": {}, "1.2.0": {}},
                "time": {"created": "2020-01-01T00:00:00Z", "1.0.0": "2020-01-02T00:00:00Z"}
            }"#,
        )
        .unwrap();
        assert_eq!(
            packument.versions_with_time(),
            vec![
                ("1.0.0", Some("2020-01-02T00:00:00Z")),
                ("1.2.0", None),
                ("1.10.0", None),
            ]
        );
        assert_eq!(packument.latest_dist_tag(), Some("2.0.0".to_string()));
    }

    #[test]
    fn test_packument_tolerates_missing_fields() {
        let packument: Packument = serde_json::from_str(r#"{"name": "x"}"#).unwrap();
        assert!(packument.versions_with_time().is_empty());
        assert_eq!(packument.latest_dist_tag(), None);
    }
}
