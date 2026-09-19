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

/// Maximum length of one DNS label (RFC 1035).
const MAX_LABEL_LEN: usize = 63;

/// Maximum length of a whole hostname (RFC 1035), which the labels share with
/// the configured TLD.
const MAX_HOSTNAME_LEN: usize = 253;

/// The hostname components a project root contributes, shared by every daemon
/// declared there.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct RootLabels {
    /// Identifies the project itself across all of its checkouts.
    pub project: Option<String>,
    /// Separates this checkout from the project's others. Absent in the
    /// primary checkout, whose daemons sit directly under the project label.
    pub worktree: Option<String>,
}

impl RootLabels {
    /// The hostname suffix these labels contribute, under `tld`. `None` when
    /// the project could not be named, which leaves nothing to build on.
    fn suffix(&self, tld: &str) -> Option<String> {
        let project = self.project.as_deref()?;
        Some(match self.worktree.as_deref() {
            Some(worktree) => format!("{worktree}.{project}.{tld}"),
            None => format!("{project}.{tld}"),
        })
    }
}

/// What `proxy` asks for on one daemon.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Proxy {
    /// `proxy = false`: no hostname, and so no URL export.
    Disabled,
    /// `proxy = "label"`: the declaration names the hostname component.
    Label(String),
    /// Absent, or `proxy = true`: the label comes from the daemon's name.
    Derived,
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

    /// Where this checkout's own page lives, for `mise daemons urls`. The
    /// primary checkout has no page of its own; its stack is the project.
    pub(crate) fn stack_url(&self, labels: &RootLabels) -> Option<String> {
        let project = labels.project.as_deref()?;
        let worktree = labels.worktree.as_deref()?;
        self.page_url(&format!("{worktree}.{project}.{}", self.tld))
    }

    pub(crate) fn project_url(&self, labels: &RootLabels) -> Option<String> {
        let project = labels.project.as_deref()?;
        self.page_url(&format!("{project}.{}", self.tld))
    }

    /// A page's URL, held to the same length a daemon's hostname is. Printing
    /// one DNS will not carry would offer a link that cannot be followed.
    fn page_url(&self, host: &str) -> Option<String> {
        hostname_fits(host).then(|| self.url(host))
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
    // Accumulated across the layers and settled once at the end; see below.
    let mut lan = Lan::default();
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
        if let Some(value) = proxy.get("lan").and_then(toml::Value::as_bool) {
            lan.enabled = value;
        }
        // Pinning an address is how pitchfork documents turning LAN mode on, so
        // it is carried as itself and only implies LAN once the layers settle.
        if let Some(ip) = proxy.get("lan_ip").and_then(toml::Value::as_str) {
            lan.ip = ip.to_string();
        }
    }
    let lan = apply_proxy_env(&mut settings, lan, |key| crate::env::var(key).ok());
    // Applied once, after every layer. Doing it inside the loop would let a
    // later file's `tld` undo an earlier layer's LAN mode, which pitchfork
    // resolves the other way round: it settles the settings first, then the
    // proxy forces the mDNS TLD.
    if lan.on() {
        settings.tld = "local".into();
    }
    settings
}

/// `lan` and `lan_ip` as the layers have left them. They settle independently
/// and only then decide the TLD, because `lan_ip` implies LAN mode: folding the
/// address into a running flag would let a later `lan = false` discard an
/// earlier address, and would leave a later empty address unable to clear one.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct Lan {
    enabled: bool,
    ip: String,
}

impl Lan {
    /// Pitchfork forces the mDNS TLD when either says so.
    fn on(&self) -> bool {
        self.enabled || !self.ip.is_empty()
    }
}

/// Environment variables win over both files, exactly as pitchfork resolves
/// them. LAN is carried through rather than applied, because the caller settles
/// it once every layer has been read. `var` is taken as a closure so the
/// precedence can be tested without touching the process environment.
fn apply_proxy_env(
    settings: &mut ProxySettings,
    mut lan: Lan,
    var: impl Fn(&str) -> Option<String>,
) -> Lan {
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
    if let Some(value) = var("PITCHFORK_PROXY_LAN").as_deref().and_then(env_flag) {
        lan.enabled = value;
    }
    if let Some(ip) = var("PITCHFORK_PROXY_LAN_IP") {
        lan.ip = ip.trim().to_string();
    }
    lan
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
/// can carry, exactly as pitchfork's `sanitize_label` does: letters lowercased,
/// digits kept, everything else a `-`, runs collapsed, ends trimmed, truncated
/// to 63 characters and trimmed again.
///
/// `None` when nothing usable is left, in which case there is no hostname to
/// offer. Repairing rather than rejecting is deliberate: these names were not
/// written as DNS labels by whoever chose them.
pub(crate) fn sanitize_label(value: &str) -> Option<String> {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let trimmed = out.trim_matches('-');
    let trimmed = if trimmed.len() > MAX_LABEL_LEN {
        trimmed[..MAX_LABEL_LEN].trim_end_matches('-')
    } else {
        trimmed
    };
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

/// Whether a hostname still leaves room for the configured TLD. A name nothing
/// can resolve is worse than no name, and pitchfork refuses the same ones.
fn hostname_fits(host: &str) -> bool {
    host.len() <= MAX_HOSTNAME_LEN
}

/// The `worktree_label` a checkout's own pitchfork configuration declares.
///
/// Pitchfork reads this key from the four configuration files in the checkout
/// itself, not from the file mise generates and registers, so mise has to read
/// it from the same place or the two would disagree about the hostname. The
/// highest-precedence file wins, as it does for `namespace`.
fn declared_worktree_label(dir: &Path) -> Option<String> {
    for name in [
        "pitchfork.local.toml",
        "pitchfork.toml",
        ".config/pitchfork.local.toml",
        ".config/pitchfork.toml",
    ] {
        let path = dir.join(name);
        if !path.exists() {
            continue;
        }
        let Ok(doc) = std::fs::read_to_string(&path).map(|t| toml::from_str::<toml::Value>(&t))
        else {
            continue;
        };
        if let Ok(doc) = doc
            && let Some(label) = doc.get("worktree_label").and_then(toml::Value::as_str)
        {
            return Some(label.to_string());
        }
    }
    None
}

/// The label naming one linked worktree: what its own configuration declares,
/// otherwise its directory name.
fn worktree_label(dir: &Path) -> Option<String> {
    match declared_worktree_label(dir) {
        Some(label) => sanitize_label(&label),
        None => sanitize_label(&dir.file_name()?.to_string_lossy()),
    }
}

/// The hostname components for a project root, derived the way pitchfork
/// derives them so the two always name the same host.
///
/// The project label is the explicit `[daemons_settings] namespace` before any
/// per-worktree suffix, otherwise the repository's directory name. The
/// worktree label is present only in a linked worktree; in the primary checkout
/// a daemon sits directly under the project, as `api.shop.localhost`.
///
/// A project root nested in a monorepo, such as `/repo/packages/api`, takes its
/// enclosing checkout's names, so every project in one worktree shares them.
pub(crate) fn labels(root: &Path, settings: &DaemonSettings) -> Result<RootLabels> {
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let checkout = crate::git::checkout_of(&root);
    let project = match settings.namespace.as_deref() {
        Some(explicit) => sanitize_label(explicit),
        // Not the hashed default namespace: that is derived from the root path,
        // so every checkout would produce a different project label and the
        // worktree component would be saying it twice. Pitchfork names the
        // project after the repository's directory, so mise does too: the
        // checkout's own for an ordinary repository, and for a bare one the
        // directory holding it and the worktrees beside it.
        None => {
            let dir = checkout.repository.as_deref().unwrap_or(root.as_path());
            dir.file_name()
                .and_then(|name| sanitize_label(&name.to_string_lossy()))
        }
    };
    let worktree = checkout.worktree.as_deref().and_then(worktree_label);
    // A worktree whose name yields no label cannot be told from the primary
    // checkout by the suffix, which would hand both one hostname for two
    // different ports. Without a name for this copy there is no hostname for
    // it at all.
    if checkout.worktree.is_some() && worktree.is_none() {
        return Ok(RootLabels::default());
    }
    Ok(RootLabels { project, worktree })
}

/// What a daemon's `proxy` declaration resolved to.
pub(crate) struct Applied {
    /// The hostname the proxy routes to it. None when the daemon opted out,
    /// configured no port, or produced a name DNS could not carry.
    pub host: Option<String>,
}

/// Read `proxy` and `proxy_tls` from a daemon table, validate them, write the
/// normalized values back for pitchfork, and return the daemon's hostname.
pub(crate) fn apply(
    name: &str,
    table: &mut toml::Table,
    labels: &RootLabels,
    tld: &str,
) -> Result<Applied> {
    // Pitchfork routes only a daemon that configures a port, so one without a
    // port has no hostname to advertise and gets no URL export either.
    let routable = table.contains_key("port");
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
        // `proxy = true` turns routing back on for a daemon a preset opted out
        // of, using the name-derived label. Pitchfork accepts it, so mise does.
        None | Some(toml::Value::Boolean(true)) => Proxy::Derived,
        Some(toml::Value::Boolean(false)) => Proxy::Disabled,
        Some(toml::Value::String(label)) => {
            validate_label(&format!("[daemons.{name}].proxy label"), label)?;
            Proxy::Label(label.clone())
        }
        Some(other) => {
            bail!("[daemons.{name}].proxy must be a hostname label, true, or false; got {other}")
        }
    };
    if tls.is_some() {
        // `proxy_tls` says what the proxy should do with TLS for this daemon,
        // so it is a mistake on one the proxy will never route. Writing it out
        // beside the `proxy = false` this produces would put the very pair
        // rejected above into the generated configuration.
        if matches!(proxy, Proxy::Disabled) {
            bail!("[daemons.{name}] sets proxy_tls but proxy = false, so nothing is proxied");
        }
        if !routable {
            bail!(
                "[daemons.{name}] sets proxy_tls but configures no port, so pitchfork never routes it; give it a port or drop proxy_tls"
            );
        }
    }
    // A daemon with no port is not routed, so writing a label for it would
    // leave the generated configuration claiming something mise does not
    // believe and its hostname outside the collision bookkeeping below.
    let label = match &proxy {
        Proxy::Disabled => None,
        _ if !routable => None,
        Proxy::Label(label) => Some(label.clone()),
        Proxy::Derived => sanitize_label(name),
    };
    let Some(label) = label else {
        withdraw(table);
        return Ok(Applied { host: None });
    };
    // The label is written out even when mise derived it, so pitchfork routes
    // the name mise exported rather than folding the daemon's name again.
    table.insert("proxy".into(), toml::Value::String(label.clone()));
    let host = labels
        .suffix(tld)
        .map(|suffix| format!("{label}.{suffix}"))
        .filter(|host| hostname_fits(host));
    // A name too long for DNS is no name at all, and pitchfork refuses it too.
    // Nothing the declaration asked for causes this, so it is withdrawn rather
    // than reported.
    if host.is_none() {
        withdraw(table);
    }
    Ok(Applied { host })
}

/// Take a daemon's hostname away after the fact, when another daemon turned out
/// to derive the same one. The proxy must not be asked to route an ambiguous
/// hostname, and the daemon keeps running on its port.
pub(crate) fn withdraw(table: &mut toml::Table) {
    table.insert("proxy".into(), toml::Value::Boolean(false));
    // `proxy_tls` beside `proxy = false` is the pair `apply` refuses when a
    // declaration writes it, so the generated file must not carry it either.
    table.remove("proxy_tls");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(namespace: Option<&str>) -> DaemonSettings {
        DaemonSettings {
            namespace: namespace.map(str::to_string),
            namespace_per_worktree: None,
        }
    }

    /// A primary checkout and a linked worktree of it, as `git worktree add`
    /// leaves them.
    fn checkout_pair(tmp: &Path) -> (PathBuf, PathBuf) {
        let primary = tmp.join("shop");
        let private = primary.join(".git").join("worktrees").join("pr-42");
        std::fs::create_dir_all(&private).unwrap();
        std::fs::write(primary.join(".git").join("HEAD"), "ref: refs/heads/main\n").unwrap();
        std::fs::write(private.join("commondir"), "../..\n").unwrap();
        let linked = tmp.join("shop-pr-42");
        std::fs::create_dir_all(&linked).unwrap();
        std::fs::write(
            linked.join(".git"),
            format!("gitdir: {}\n", private.display()),
        )
        .unwrap();
        (primary, linked)
    }

    #[test]
    fn a_label_is_repaired_or_rejected_depending_on_who_wrote_it() {
        assert_eq!(sanitize_label("My_Api.v2").unwrap(), "my-api-v2");
        // Runs collapse and the ends are trimmed, as pitchfork does it.
        assert_eq!(sanitize_label("--a__b--").unwrap(), "a-b");
        assert!(sanitize_label("--").is_none(), "nothing usable is no label");
        assert_eq!(
            sanitize_label(&"a".repeat(80)).unwrap().len(),
            MAX_LABEL_LEN
        );
        // Truncation must not leave a trailing separator behind.
        let cut = sanitize_label(&format!("{}-{}", "a".repeat(62), "b".repeat(5))).unwrap();
        assert_eq!(cut, "a".repeat(62));
        validate_label("label", &cut).unwrap();
        // A declared label is rejected rather than repaired: the user typed it.
        assert!(validate_label("label", "api-2").is_ok());
        for invalid in ["", "-api", "api-", "API", "my_api", &"a".repeat(64)] {
            assert!(validate_label("label", invalid).is_err(), "{invalid:?}");
        }
    }

    #[test]
    fn a_url_omits_the_port_only_on_the_standard_one() {
        let host = "api.shop.localhost";
        assert_eq!(
            ProxySettings::default().url(host),
            "https://api.shop.localhost"
        );
        let plain = ProxySettings {
            https: false,
            port: 80,
            tld: "localhost".into(),
        };
        assert_eq!(plain.url(host), "http://api.shop.localhost");
        let custom = ProxySettings {
            https: true,
            port: 8443,
            tld: "localhost".into(),
        };
        assert_eq!(custom.url(host), "https://api.shop.localhost:8443");
        let http_custom = ProxySettings {
            https: false,
            port: 8088,
            tld: "test".into(),
        };
        assert_eq!(http_custom.url(host), "http://api.shop.localhost:8088");
    }

    #[test]
    fn a_page_url_is_withheld_when_it_would_not_resolve() {
        let labels = RootLabels {
            project: Some("shop".into()),
            worktree: Some("feature".into()),
        };
        let settings = ProxySettings::default();
        assert_eq!(
            settings.stack_url(&labels).as_deref(),
            Some("https://feature.shop.localhost")
        );
        assert_eq!(
            settings.project_url(&labels).as_deref(),
            Some("https://shop.localhost")
        );

        // Both labels are capped, so only the configured TLD can push a page
        // past what DNS carries. A daemon's own hostname is already withheld
        // there, and offering a link that cannot be followed is worse than
        // offering none.
        let long = ProxySettings {
            tld: "t".repeat(MAX_HOSTNAME_LEN),
            ..ProxySettings::default()
        };
        assert_eq!(long.stack_url(&labels), None);
        assert_eq!(long.project_url(&labels), None);
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
        apply_proxy_env(&mut settings, Lan::default(), env);
        assert_eq!(
            settings,
            ProxySettings {
                https: false,
                port: 8088,
                tld: "test".into()
            }
        );

        // LAN mode forces the mDNS TLD, and it is settled after every layer
        // rather than inside one, so a later `tld` cannot undo it.
        let mut settings = ProxySettings {
            tld: "test".into(),
            ..ProxySettings::default()
        };
        let one = |key: &str, value: &str| {
            let key = key.to_string();
            let value = value.to_string();
            move |asked: &str| (asked == key).then(|| value.clone())
        };
        let lan = apply_proxy_env(
            &mut settings,
            Lan::default(),
            one("PITCHFORK_PROXY_LAN", "1"),
        );
        assert!(lan.on());
        assert_eq!(
            settings.tld, "test",
            "the caller settles the mDNS TLD, not this"
        );

        // `lan` and `lan_ip` settle as themselves. A later `lan = false` must
        // not discard an address an earlier layer pinned, since the address
        // implies LAN mode on its own, and a later empty address must clear one.
        let pinned = Lan {
            enabled: false,
            ip: "192.168.1.42".into(),
        };
        let after = apply_proxy_env(
            &mut settings,
            pinned.clone(),
            one("PITCHFORK_PROXY_LAN", "off"),
        );
        assert!(after.on(), "a pinned address keeps LAN mode on: {after:?}");
        let cleared = apply_proxy_env(&mut settings, pinned, one("PITCHFORK_PROXY_LAN_IP", ""));
        assert!(
            !cleared.on(),
            "an empty address clears the pin: {cleared:?}"
        );
        // And pinning one turns LAN mode on by itself.
        let ip = apply_proxy_env(
            &mut settings,
            Lan::default(),
            one("PITCHFORK_PROXY_LAN_IP", "192.168.1.42"),
        );
        assert!(ip.on());

        // Pitchfork compares these spellings without regard to case, so mise
        // must read `FALSE` and `Off` as it does rather than as "not empty".
        for spelling in ["FALSE", "False", "no", "N", "Off", "0", ""] {
            let mut cased = ProxySettings::default();
            apply_proxy_env(&mut cased, Lan::default(), |key| {
                matches!(key, "PITCHFORK_PROXY_HTTPS").then(|| spelling.to_string())
            });
            assert!(!cased.https, "{spelling:?} must turn HTTPS off");
        }
        for spelling in ["TRUE", "Yes", "y", "On", "1"] {
            let mut cased = ProxySettings {
                https: false,
                ..ProxySettings::default()
            };
            apply_proxy_env(&mut cased, Lan::default(), |key| {
                matches!(key, "PITCHFORK_PROXY_HTTPS").then(|| spelling.to_string())
            });
            assert!(cased.https, "{spelling:?} must turn HTTPS on");
        }
        // A spelling neither side recognises leaves the setting alone.
        let mut unknown = ProxySettings::default();
        apply_proxy_env(&mut unknown, Lan::default(), |key| {
            matches!(key, "PITCHFORK_PROXY_HTTPS").then(|| "maybe".to_string())
        });
        assert!(unknown.https);
    }

    #[test]
    fn proxy_declarations_are_validated_and_normalized() {
        let labels = RootLabels {
            project: Some("shop".into()),
            worktree: None,
        };
        let parse = |text: &str| -> Result<(Option<String>, toml::Table)> {
            let mut table: toml::Table = toml::from_str(text)?;
            let applied = apply("api", &mut table, &labels, "localhost")?;
            Ok((applied.host, table))
        };
        let (host, table) = parse("port = 3000").unwrap();
        assert_eq!(host.unwrap(), "api.shop.localhost");
        // The derived label is written out, so pitchfork routes the name mise
        // exported rather than folding the daemon's name a second time.
        assert_eq!(table["proxy"].as_str().unwrap(), "api");

        let (host, table) = parse("port = 3000\nproxy = 'web'").unwrap();
        assert_eq!(host.unwrap(), "web.shop.localhost");
        assert_eq!(table["proxy"].as_str().unwrap(), "web");

        let (host, table) = parse("port = 3000\nproxy = false").unwrap();
        assert!(host.is_none(), "an opted-out daemon has no hostname");
        assert_eq!(table["proxy"].as_bool(), Some(false));

        // `proxy = true` turns routing back on for a daemon a preset opted out
        // of, using the name-derived label. Pitchfork accepts it, so mise does.
        let (host, table) = parse("port = 3000\nproxy = true").unwrap();
        assert_eq!(host.unwrap(), "api.shop.localhost");
        assert_eq!(table["proxy"].as_str().unwrap(), "api");

        // Pitchfork routes only a daemon that configures a port, and the
        // generated configuration has to say so rather than carrying a label
        // for a daemon nothing will route.
        let (host, table) = parse("").unwrap();
        assert!(host.is_none(), "a portless daemon is never routed");
        assert_eq!(table["proxy"].as_bool(), Some(false));
        // Even when the declaration named a label.
        let (host, table) = parse("proxy = 'web'").unwrap();
        assert!(host.is_none());
        assert_eq!(table["proxy"].as_bool(), Some(false));
        // Declaring how the proxy should handle TLS for a daemon it will never
        // route is the same mistake as declaring it beside proxy = false.
        assert!(parse("proxy_tls = 'terminate'").is_err());
        assert!(parse("proxy = true\nproxy_tls = 'terminate'").is_err());

        for mode in ["terminate", "passthrough"] {
            let (_, table) = parse(&format!("port = 3000\nproxy_tls = '{mode}'")).unwrap();
            assert_eq!(table["proxy_tls"].as_str().unwrap(), mode);
        }
        for invalid in [
            "port = 3000\nproxy = 3000",
            "port = 3000\nproxy = 'Web'",
            "port = 3000\nproxy = 'my_web'",
            "port = 3000\nproxy_tls = 'reencrypt'",
            "port = 3000\nproxy_tls = true",
            "port = 3000\nproxy = false\nproxy_tls = 'terminate'",
        ] {
            assert!(parse(invalid).is_err(), "{invalid:?}");
        }
    }

    /// A hostname nothing can resolve is worse than no hostname, and pitchfork
    /// refuses the same ones.
    #[test]
    fn a_hostname_that_cannot_fit_is_not_offered() {
        let labels = RootLabels {
            project: Some("a".repeat(MAX_LABEL_LEN)),
            worktree: Some("b".repeat(MAX_LABEL_LEN)),
        };
        let mut table: toml::Table = toml::from_str("port = 3000").unwrap();
        let long = "c".repeat(MAX_LABEL_LEN);
        let applied = apply(&long, &mut table, &labels, &"d".repeat(MAX_LABEL_LEN)).unwrap();
        assert!(applied.host.is_none(), "{:?}", applied.host);
        // And the generated configuration agrees, rather than asking the proxy
        // to route a name it will refuse. Nothing the declaration asked for
        // caused this, so a declared TLS mode is withdrawn with it rather than
        // left beside a proxy = false the loader would have rejected.
        let mut table: toml::Table =
            toml::from_str("port = 3000\nproxy_tls = 'passthrough'").unwrap();
        let applied = apply(&long, &mut table, &labels, &"d".repeat(MAX_LABEL_LEN)).unwrap();
        assert!(applied.host.is_none());
        assert_eq!(table["proxy"].as_bool(), Some(false));
        assert!(!table.contains_key("proxy_tls"), "{table:?}");
        // The same labels fit under an ordinary TLD.
        let mut table: toml::Table = toml::from_str("port = 3000").unwrap();
        assert!(
            apply(&long, &mut table, &labels, "localhost")
                .unwrap()
                .host
                .is_some()
        );
    }

    /// A daemon name that is not a shell identifier still runs; only the
    /// convenience variables are skipped, and punctuation is folded.
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
    fn a_primary_checkout_has_no_worktree_label() {
        let tmp = tempfile::tempdir().unwrap();
        let (primary, linked) = checkout_pair(tmp.path());

        // The primary checkout's daemons sit directly under the project, which
        // is what pitchfork routes; adding a worktree component would make
        // every URL a 404.
        let resolved = labels(&primary, &settings(Some("shop"))).unwrap();
        assert_eq!(resolved.project.as_deref(), Some("shop"));
        assert_eq!(resolved.worktree, None);
        assert_eq!(resolved.suffix("localhost").unwrap(), "shop.localhost");

        // A linked worktree adds its own component.
        let resolved = labels(&linked, &settings(Some("shop"))).unwrap();
        assert_eq!(resolved.worktree.as_deref(), Some("shop-pr-42"));
        assert_eq!(
            resolved.suffix("localhost").unwrap(),
            "shop-pr-42.shop.localhost"
        );

        // Without an explicit namespace the project is named after the primary
        // checkout's directory, from either checkout, so the two agree.
        assert_eq!(
            labels(&primary, &settings(None))
                .unwrap()
                .project
                .as_deref(),
            Some("shop")
        );
        assert_eq!(
            labels(&linked, &settings(None)).unwrap().project.as_deref(),
            Some("shop")
        );

        // A project root nested in a monorepo takes its checkout's names.
        let nested = linked.join("packages").join("api");
        std::fs::create_dir_all(&nested).unwrap();
        let resolved = labels(&nested, &settings(None)).unwrap();
        assert_eq!(resolved.project.as_deref(), Some("shop"));
        assert_eq!(resolved.worktree.as_deref(), Some("shop-pr-42"));
    }

    /// Without a name for this copy there is no hostname for it. Leaving the
    /// worktree component off would give it the primary checkout's hostname,
    /// and two checkouts would advertise one host for two different ports.
    #[test]
    fn a_worktree_that_cannot_be_named_gets_no_hostname() {
        let tmp = tempfile::tempdir().unwrap();
        let primary = tmp.path().join("shop");
        let private = primary.join(".git").join("worktrees").join("odd");
        std::fs::create_dir_all(&private).unwrap();
        std::fs::write(primary.join(".git").join("HEAD"), "ref: refs/heads/main\n").unwrap();
        std::fs::write(private.join("commondir"), "../..\n").unwrap();
        // Nothing in this directory name survives being folded into a label.
        let linked = tmp.path().join("---");
        std::fs::create_dir_all(&linked).unwrap();
        std::fs::write(
            linked.join(".git"),
            format!("gitdir: {}\n", private.display()),
        )
        .unwrap();

        let resolved = labels(&linked, &settings(Some("shop"))).unwrap();
        assert_eq!(resolved, RootLabels::default());
        assert!(resolved.suffix("localhost").is_none());
        // The primary checkout keeps its own, which is the one at stake.
        assert_eq!(
            labels(&primary, &settings(Some("shop")))
                .unwrap()
                .suffix("localhost")
                .unwrap(),
            "shop.localhost"
        );
    }

    /// A bare repository with worktrees beside it has no ordinary checkout, so
    /// the repository directory names the project. Falling back to the root's
    /// own directory would repeat the worktree label in the hostname, and would
    /// give sibling projects in one worktree different project labels.
    #[test]
    fn a_bare_repositorys_worktrees_share_one_project_label() {
        let tmp = tempfile::tempdir().unwrap();
        // The usual layout: one directory holding the bare repository and the
        // worktrees checked out beside it. That directory is the project, which
        // is also what pitchfork names: the parent of the common git dir.
        let project = tmp.path().join("shop");
        let bare = project.join("repo.git");
        std::fs::create_dir_all(&bare).unwrap();
        std::fs::write(bare.join("HEAD"), "ref: refs/heads/main\n").unwrap();
        let worktree = |name: &str| {
            let private = bare.join("worktrees").join(name);
            std::fs::create_dir_all(&private).unwrap();
            std::fs::write(private.join("commondir"), "../..\n").unwrap();
            let root = project.join(name);
            std::fs::create_dir_all(&root).unwrap();
            std::fs::write(
                root.join(".git"),
                format!("gitdir: {}\n", private.display()),
            )
            .unwrap();
            root
        };
        let main = worktree("main");
        let feature = worktree("feature");

        // The project is named once, by the repository, and the worktree
        // component stays distinct from it.
        let resolved = labels(&main, &settings(None)).unwrap();
        assert_eq!(resolved.project.as_deref(), Some("shop"));
        assert_eq!(resolved.worktree.as_deref(), Some("main"));
        assert_eq!(resolved.suffix("localhost").unwrap(), "main.shop.localhost");
        assert_eq!(
            labels(&feature, &settings(None))
                .unwrap()
                .project
                .as_deref(),
            Some("shop"),
            "every worktree of one repository names the same project"
        );

        // Sibling projects inside one worktree share both labels.
        let api = main.join("packages").join("api");
        let web = main.join("packages").join("web");
        std::fs::create_dir_all(&api).unwrap();
        std::fs::create_dir_all(&web).unwrap();
        assert_eq!(
            labels(&api, &settings(None)).unwrap(),
            labels(&web, &settings(None)).unwrap()
        );
    }

    /// A submodule is its own working copy, but the copy that distinguishes it
    /// is the worktree containing it: the same submodule checked out under two
    /// worktrees is two copies, and naming both after the submodule's own
    /// directory would give them one hostname.
    #[test]
    fn a_submodule_takes_the_worktree_that_contains_it() {
        let tmp = tempfile::tempdir().unwrap();
        let (primary, linked) = checkout_pair(tmp.path());
        // Git keeps a submodule's git dir under the superproject's, which for a
        // linked worktree is the shared one in the primary checkout.
        let module = primary
            .join(".git")
            .join("modules")
            .join("vendor")
            .join("shared-lib");
        std::fs::create_dir_all(&module).unwrap();
        let submodule = linked.join("vendor").join("shared-lib");
        std::fs::create_dir_all(&submodule).unwrap();
        std::fs::write(
            submodule.join(".git"),
            format!("gitdir: {}\n", module.display()),
        )
        .unwrap();
        let resolved = labels(&submodule, &settings(Some("shop"))).unwrap();
        assert_eq!(resolved.worktree.as_deref(), Some("shop-pr-42"));
        assert_eq!(resolved.project.as_deref(), Some("shop"));
    }

    /// Pitchfork reads `worktree_label` from the checkout's own configuration
    /// rather than from the file mise registers, so mise reads it there too.
    #[test]
    fn a_checkouts_own_config_names_the_worktree() {
        let tmp = tempfile::tempdir().unwrap();
        let (_, linked) = checkout_pair(tmp.path());
        std::fs::write(linked.join("pitchfork.toml"), "worktree_label = 'pr-42'\n").unwrap();
        let resolved = labels(&linked, &settings(Some("shop"))).unwrap();
        assert_eq!(resolved.worktree.as_deref(), Some("pr-42"));
        // The highest-precedence file wins, as it does for `namespace`.
        std::fs::write(
            linked.join("pitchfork.local.toml"),
            "worktree_label = 'mine'\n",
        )
        .unwrap();
        assert_eq!(
            labels(&linked, &settings(Some("shop")))
                .unwrap()
                .worktree
                .as_deref(),
            Some("mine")
        );
        // A label that cannot be one is folded, not rejected: pitchfork does
        // the same, and refusing would break a checkout that already works.
        std::fs::write(
            linked.join("pitchfork.local.toml"),
            "worktree_label = 'PR 42'\n",
        )
        .unwrap();
        assert_eq!(
            labels(&linked, &settings(Some("shop")))
                .unwrap()
                .worktree
                .as_deref(),
            Some("pr-42")
        );
    }
}
