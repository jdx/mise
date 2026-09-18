//! Stable hostnames for project daemons, so an HTTP service keeps one URL
//! across worktrees instead of moving with its port.
//!
//! Pitchfork's reverse proxy routes `<daemon>.<worktree>.<project>.<tld>` to
//! whichever port the daemon actually bound. mise derives the same hostname at
//! configuration time and exports it, so a sibling service can be pointed at
//! `{{ env.API_URL }}` and never has to learn the port. Databases keep
//! `port = "auto"`: the proxy speaks HTTP, and a Postgres client does not.
use super::DaemonSettings;
use eyre::{Result, bail};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

/// Pitchfork release that understands `worktree_label` and per-daemon `proxy`.
/// Named in errors and docs so an older supervisor's rejection is explicable.
pub(crate) const REQUIRED_PITCHFORK: &str = "2.26.0";

/// The hostname components a project root contributes, shared by every daemon
/// declared there.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct RootLabels {
    /// Identifies the project itself, derived from its pitchfork namespace.
    pub project: String,
    /// Separates concurrent checkouts of that project.
    pub worktree: String,
}

/// What `proxy` asks for on one daemon.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Proxy {
    /// `proxy = false`: no hostname, and so no URL export.
    Disabled,
    /// `proxy = "label"`, or the daemon's own name when the key is absent.
    Label(String),
}

/// The pitchfork proxy settings that decide what a hostname's URL looks like.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProxySettings {
    pub https: bool,
    pub port: u16,
    pub tld: String,
}

impl Default for ProxySettings {
    /// Pitchfork's own defaults. A machine with no proxy configuration still
    /// gets the URL it would have once the proxy is switched on, rather than no
    /// export at all, because `mise env` runs long before any supervisor does.
    fn default() -> Self {
        Self {
            https: true,
            port: 443,
            tld: "localhost".into(),
        }
    }
}

impl ProxySettings {
    /// The URL for a hostname. Mirrors pitchfork's own derivation: the scheme
    /// follows `proxy.https`, and the port is written out only when it is not
    /// the standard one for that scheme, so the common setup yields a bare
    /// `https://api.main.shop.localhost`.
    pub(crate) fn url(&self, host: &str) -> String {
        let scheme = if self.https { "https" } else { "http" };
        let standard = if self.https { 443 } else { 80 };
        if self.port == standard {
            format!("{scheme}://{host}")
        } else {
            format!("{scheme}://{host}:{}", self.port)
        }
    }

    /// Where the stack's own pages live, for `mise daemons urls`.
    pub(crate) fn stack_url(&self, labels: &RootLabels) -> String {
        self.url(&format!(
            "{}.{}.{}",
            labels.worktree, labels.project, self.tld
        ))
    }

    pub(crate) fn project_url(&self, labels: &RootLabels) -> String {
        self.url(&format!("{}.{}", labels.project, self.tld))
    }
}

/// The user's effective proxy settings.
///
/// Read once per process: `mise env` resolves a URL for every daemon in the
/// project, and none of them can disagree about the scheme. Pitchfork layers
/// `/etc/pitchfork/config.toml`, the user config, project configs and finally
/// the environment; mise reads the two machine-wide files and the environment,
/// which are the layers that apply wherever the daemon is started from. A
/// project-level `[settings.proxy]` in a `pitchfork.toml` is deliberately not
/// consulted: it would make one project's URL depend on which directory the
/// supervisor happened to start in.
pub(crate) fn proxy_settings() -> &'static ProxySettings {
    static SETTINGS: LazyLock<ProxySettings> = LazyLock::new(read_proxy_settings);
    &SETTINGS
}

/// Where pitchfork keeps the user's own configuration. Deliberately not
/// XDG-aware: pitchfork resolves this as `PITCHFORK_CONFIG_DIR` or
/// `~/.config/pitchfork` and ignores `XDG_CONFIG_HOME`, so honouring it here
/// would read a different file from the one serving the URL.
fn user_config_dir() -> PathBuf {
    crate::env::var_path("PITCHFORK_CONFIG_DIR")
        .unwrap_or_else(|| crate::dirs::HOME.join(".config").join("pitchfork"))
}

/// Read a boolean the way pitchfork does. Its settings resolver accepts several
/// spellings and compares them without regard to case, so `HTTPS=FALSE` has to
/// mean here what it means there; treating it as true would export a URL whose
/// scheme the proxy never serves.
fn env_flag(value: &str) -> Option<bool> {
    let value = value.trim();
    match value {
        "1" => Some(true),
        "0" | "" => Some(false),
        _ if ["true", "yes", "y", "on"]
            .iter()
            .any(|known| value.eq_ignore_ascii_case(known)) =>
        {
            Some(true)
        }
        _ if ["false", "no", "n", "off"]
            .iter()
            .any(|known| value.eq_ignore_ascii_case(known)) =>
        {
            Some(false)
        }
        _ => None,
    }
}

fn read_proxy_settings() -> ProxySettings {
    let mut settings = ProxySettings::default();
    for path in [
        Path::new("/etc/pitchfork/config.toml").to_path_buf(),
        user_config_dir().join("config.toml"),
    ] {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(doc) = toml::from_str::<toml::Value>(&text) else {
            debug!("ignoring unparseable {}", crate::file::display_path(&path));
            continue;
        };
        let Some(proxy) = doc.get("settings").and_then(|s| s.get("proxy")) else {
            continue;
        };
        if let Some(https) = proxy.get("https").and_then(toml::Value::as_bool) {
            settings.https = https;
        }
        if let Some(port) = proxy
            .get("port")
            .and_then(toml::Value::as_integer)
            .and_then(|p| u16::try_from(p).ok())
            .filter(|p| *p > 0)
        {
            settings.port = port;
        }
        if let Some(tld) = proxy.get("tld").and_then(toml::Value::as_str) {
            settings.tld = tld.to_string();
        }
        // LAN mode forces mDNS, which only resolves under `.local`.
        if proxy.get("lan").and_then(toml::Value::as_bool) == Some(true)
            || proxy
                .get("lan_ip")
                .and_then(toml::Value::as_str)
                .is_some_and(|ip| !ip.is_empty())
        {
            settings.tld = "local".into();
        }
    }
    apply_proxy_env(&mut settings, |key| crate::env::var(key).ok());
    settings
}

/// Environment variables win over both files, exactly as pitchfork resolves
/// them. Taken as a closure so the precedence can be tested without touching
/// the process environment.
fn apply_proxy_env(settings: &mut ProxySettings, var: impl Fn(&str) -> Option<String>) {
    if let Some(https) = var("PITCHFORK_PROXY_HTTPS").as_deref().and_then(env_flag) {
        settings.https = https;
    }
    if let Some(port) = var("PITCHFORK_PROXY_PORT")
        .and_then(|p| p.trim().parse::<u16>().ok())
        .filter(|p| *p > 0)
    {
        settings.port = port;
    }
    if let Some(tld) = var("PITCHFORK_PROXY_TLD").filter(|t| !t.trim().is_empty()) {
        settings.tld = tld.trim().to_string();
    }
    if var("PITCHFORK_PROXY_LAN")
        .as_deref()
        .and_then(env_flag)
        .unwrap_or(false)
        || var("PITCHFORK_PROXY_LAN_IP").is_some_and(|ip| !ip.trim().is_empty())
    {
        settings.tld = "local".into();
    }
}

/// Reject a label that cannot survive as a DNS name. Pitchfork puts the label
/// straight into a hostname and, with `sync_hosts`, into `/etc/hosts`, so an
/// underscore or a trailing dash produces a name no browser will resolve.
pub(crate) fn validate_label(kind: &str, value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 63
        || value.starts_with('-')
        || value.ends_with('-')
        || !value
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        bail!(
            "invalid {kind} {value:?}; use lowercase letters, numbers and '-', \
             up to 63 characters, not starting or ending with '-'"
        );
    }
    Ok(())
}

/// Fold a daemon name, namespace or directory name into something a hostname
/// can carry. These are not typed as labels by their owners, so they are
/// repaired rather than rejected: `my_api.v2` becomes `my-api-v2`.
pub(crate) fn sanitize_label(value: &str) -> String {
    let mapped: String = value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let trimmed = mapped
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    let trimmed = if trimmed.is_empty() {
        "mise".to_string()
    } else {
        trimmed
    };
    // Cutting at 63 can land just after a separator, which would leave a label
    // ending in `-`; that is not a name DNS accepts.
    trimmed
        .chars()
        .take(63)
        .collect::<String>()
        .trim_end_matches('-')
        .to_string()
}

/// The checkout a project root belongs to, which is what distinguishes two
/// copies of one project. A project root nested in a monorepo
/// (`/repo/packages/api`) takes the checkout's name, not its own, so every
/// project in one worktree shares a worktree label.
pub(crate) fn default_worktree_label(root: &Path) -> String {
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let dir = crate::git::checkout_root(&root).unwrap_or(root);
    sanitize_label(&dir.file_name().unwrap_or_default().to_string_lossy())
}

/// The hostname components for a project root.
///
/// The project label comes from the pitchfork namespace before any
/// per-worktree suffix: the suffix exists to keep two checkouts' daemon IDs
/// apart, and the worktree label already does that here. A project without an
/// explicit `[daemons_settings] namespace` therefore carries the hash mise
/// generates for it, which is stable but not memorable; naming the namespace is
/// what buys a readable hostname.
pub(crate) fn labels(root: &Path, settings: &DaemonSettings) -> Result<RootLabels> {
    let project = match settings.namespace.as_deref() {
        Some(explicit) => sanitize_label(explicit),
        // The hashed default namespace is derived from the root path, so every
        // checkout would produce a different project label and the worktree
        // component would be saying it twice. Hashing the main checkout's
        // equivalent path instead keeps one project label across checkouts.
        None => {
            let stable =
                crate::git::main_checkout_equivalent(root).unwrap_or_else(|| root.to_path_buf());
            sanitize_label(&super::runtime::namespace(&stable)?)
        }
    };
    let worktree = match settings.worktree_label.as_deref() {
        Some(label) => {
            validate_label("[daemons_settings] worktree_label", label)?;
            label.to_string()
        }
        None => default_worktree_label(root),
    };
    Ok(RootLabels { project, worktree })
}

/// Read `proxy` and `proxy_tls` from a daemon table, validate them, write the
/// normalized values back for pitchfork, and return the daemon's hostname.
///
/// `None` means the daemon opted out with `proxy = false` and gets no URL.
pub(crate) fn apply(
    name: &str,
    table: &mut toml::Table,
    labels: &RootLabels,
    tld: &str,
) -> Result<Option<String>> {
    let tls = match table.get("proxy_tls") {
        None => None,
        Some(toml::Value::String(mode)) if matches!(mode.as_str(), "terminate" | "passthrough") => {
            Some(mode.clone())
        }
        Some(other) => bail!(
            "[daemons.{name}].proxy_tls must be \"terminate\" or \"passthrough\"; got {other}"
        ),
    };
    let proxy = match table.get("proxy") {
        None => Proxy::Label(sanitize_label(name)),
        Some(toml::Value::Boolean(false)) => Proxy::Disabled,
        Some(toml::Value::Boolean(true)) => bail!(
            "[daemons.{name}].proxy must be false or a hostname label; \
             omit it to use the daemon's name"
        ),
        Some(toml::Value::String(label)) => {
            validate_label(&format!("[daemons.{name}].proxy label"), label)?;
            Proxy::Label(label.clone())
        }
        Some(other) => {
            bail!("[daemons.{name}].proxy must be false or a hostname label; got {other}")
        }
    };
    if matches!(proxy, Proxy::Disabled) && tls.is_some() {
        bail!("[daemons.{name}] sets proxy_tls but proxy = false, so nothing is proxied");
    }
    match &proxy {
        Proxy::Disabled => {
            table.insert("proxy".into(), toml::Value::Boolean(false));
            Ok(None)
        }
        Proxy::Label(label) => {
            table.insert("proxy".into(), toml::Value::String(label.clone()));
            Ok(Some(format!(
                "{label}.{}.{}.{tld}",
                labels.worktree, labels.project
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(namespace: Option<&str>, worktree_label: Option<&str>) -> DaemonSettings {
        DaemonSettings {
            namespace: namespace.map(str::to_string),
            namespace_per_worktree: None,
            worktree_label: worktree_label.map(str::to_string),
        }
    }

    #[test]
    fn a_label_is_repaired_or_rejected_depending_on_who_wrote_it() {
        assert_eq!(sanitize_label("My_Api.v2"), "my-api-v2");
        assert_eq!(sanitize_label("--"), "mise");
        assert_eq!(sanitize_label(&"a".repeat(80)).len(), 63);
        // Truncation must not leave a trailing separator behind.
        let cut = sanitize_label(&format!("{}-{}", "a".repeat(62), "b".repeat(5)));
        assert_eq!(cut, "a".repeat(62));
        validate_label("label", &cut).unwrap();
        assert!(validate_label("label", "api-2").is_ok());
        for invalid in ["", "-api", "api-", "API", "my_api", &"a".repeat(64)] {
            assert!(validate_label("label", invalid).is_err(), "{invalid:?}");
        }
    }

    #[test]
    fn a_url_omits_the_port_only_on_the_standard_one() {
        let host = "api.main.shop.localhost";
        assert_eq!(
            ProxySettings::default().url(host),
            "https://api.main.shop.localhost"
        );
        let plain = ProxySettings {
            https: false,
            port: 80,
            tld: "localhost".into(),
        };
        assert_eq!(plain.url(host), "http://api.main.shop.localhost");
        let custom = ProxySettings {
            https: true,
            port: 8443,
            tld: "localhost".into(),
        };
        assert_eq!(custom.url(host), "https://api.main.shop.localhost:8443");
        let http_custom = ProxySettings {
            https: false,
            port: 8088,
            tld: "test".into(),
        };
        assert_eq!(http_custom.url(host), "http://api.main.shop.localhost:8088");
    }

    #[test]
    fn the_environment_wins_over_the_files_pitchfork_reads() {
        let mut settings = ProxySettings::default();
        let env = |key: &str| match key {
            "PITCHFORK_PROXY_HTTPS" => Some("false".to_string()),
            "PITCHFORK_PROXY_PORT" => Some("8088".to_string()),
            "PITCHFORK_PROXY_TLD" => Some("test".to_string()),
            _ => None,
        };
        apply_proxy_env(&mut settings, env);
        assert_eq!(
            settings,
            ProxySettings {
                https: false,
                port: 8088,
                tld: "test".into()
            }
        );
        // LAN mode forces the mDNS TLD whatever the files or `tld` asked for.
        let mut lan = ProxySettings::default();
        apply_proxy_env(&mut lan, |key| {
            matches!(key, "PITCHFORK_PROXY_LAN").then(|| "1".to_string())
        });
        assert_eq!(lan.tld, "local");

        // Pitchfork compares these spellings without regard to case, so mise
        // must read `FALSE` and `Off` as it does rather than as "not empty".
        for spelling in ["FALSE", "False", "no", "N", "Off", "0", ""] {
            let mut cased = ProxySettings::default();
            apply_proxy_env(&mut cased, |key| {
                matches!(key, "PITCHFORK_PROXY_HTTPS").then(|| spelling.to_string())
            });
            assert!(!cased.https, "{spelling:?} must turn HTTPS off");
        }
        for spelling in ["TRUE", "Yes", "y", "On", "1"] {
            let mut cased = ProxySettings {
                https: false,
                ..ProxySettings::default()
            };
            apply_proxy_env(&mut cased, |key| {
                matches!(key, "PITCHFORK_PROXY_HTTPS").then(|| spelling.to_string())
            });
            assert!(cased.https, "{spelling:?} must turn HTTPS on");
        }
        // A spelling neither side recognises leaves the setting alone rather
        // than guessing at it.
        let mut unknown = ProxySettings::default();
        apply_proxy_env(&mut unknown, |key| {
            matches!(key, "PITCHFORK_PROXY_HTTPS").then(|| "maybe".to_string())
        });
        assert!(unknown.https);
    }

    #[test]
    fn proxy_declarations_are_validated_and_normalized() {
        let labels = RootLabels {
            project: "shop".into(),
            worktree: "main".into(),
        };
        let parse = |text: &str| -> Result<(Option<String>, toml::Table)> {
            let mut table: toml::Table = toml::from_str(text)?;
            let host = apply("api", &mut table, &labels, "localhost")?;
            Ok((host, table))
        };
        let (host, table) = parse("").unwrap();
        assert_eq!(host.unwrap(), "api.main.shop.localhost");
        assert_eq!(table["proxy"].as_str().unwrap(), "api");

        let (host, table) = parse("proxy = 'web'").unwrap();
        assert_eq!(host.unwrap(), "web.main.shop.localhost");
        assert_eq!(table["proxy"].as_str().unwrap(), "web");

        let (host, table) = parse("proxy = false").unwrap();
        assert!(host.is_none(), "an opted-out daemon has no hostname");
        assert_eq!(table["proxy"].as_bool(), Some(false));

        for mode in ["terminate", "passthrough"] {
            let (_, table) = parse(&format!("proxy_tls = '{mode}'")).unwrap();
            assert_eq!(table["proxy_tls"].as_str().unwrap(), mode);
        }
        for invalid in [
            "proxy = true",
            "proxy = 3000",
            "proxy = 'Web'",
            "proxy = 'my_web'",
            "proxy_tls = 'reencrypt'",
            "proxy_tls = true",
            "proxy = false\nproxy_tls = 'terminate'",
        ] {
            assert!(parse(invalid).is_err(), "{invalid:?}");
        }
    }

    /// A daemon name that is not a shell identifier still runs; only the
    /// convenience variables are skipped, and punctuation is folded rather
    /// than rejected.
    #[test]
    fn endpoint_variables_share_one_stem() {
        assert_eq!(super::super::env_var_base("api").unwrap(), "API");
        assert_eq!(
            super::super::env_var_base("my-api.v2").unwrap(),
            "MY_API_V2"
        );
        assert!(super::super::env_var_base("9api").is_none());
    }

    #[test]
    fn labels_name_the_project_and_the_checkout() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("shop");
        std::fs::create_dir_all(root.join(".git")).unwrap();

        // An explicit namespace is what makes the hostname readable.
        let resolved = labels(&root, &settings(Some("shop"), None)).unwrap();
        assert_eq!(resolved.project, "shop");
        assert_eq!(resolved.worktree, "shop");

        // An explicit worktree_label replaces the directory name.
        let resolved = labels(&root, &settings(Some("shop"), Some("pr-42"))).unwrap();
        assert_eq!(resolved.worktree, "pr-42");
        assert!(labels(&root, &settings(Some("shop"), Some("PR 42"))).is_err());

        // Without a namespace the hashed default is sanitized into a label,
        // which stays stable and unique but is not memorable.
        let resolved = labels(&root, &settings(None, None)).unwrap();
        assert!(resolved.project.starts_with("shop-"), "{resolved:?}");
        validate_label("project", &resolved.project).unwrap();
    }
}
