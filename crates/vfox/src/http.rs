use netrc_rs::Netrc;
use reqwest::header::{HeaderMap, HeaderValue};
use reqwest::{Client, ClientBuilder};
use std::path::PathBuf;
use std::sync::LazyLock;

pub static CLIENT: LazyLock<Client> = LazyLock::new(|| {
    ClientBuilder::new()
        .user_agent(format!("vfox.rs/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .expect("Failed to create reqwest client")
});

/// Cached parsed netrc file
static NETRC: LazyLock<Option<Netrc>> = LazyLock::new(|| {
    // Check if netrc is disabled via environment variable
    if std::env::var("MISE_NETRC").is_ok_and(|v| v == "0" || v == "false") {
        return None;
    }

    let path = netrc_path();
    if !path.exists() {
        return None;
    }

    match std::fs::read_to_string(&path) {
        Ok(content) => match Netrc::parse(content, false) {
            Ok(netrc) => {
                log::debug!("Loaded netrc from {}", path.display());
                Some(netrc)
            }
            Err(e) => {
                log::warn!("Failed to parse netrc file {}: {}", path.display(), e);
                None
            }
        },
        Err(e) => {
            log::warn!("Failed to read netrc file {}: {}", path.display(), e);
            None
        }
    }
});

/// Get the path to the netrc file
///
/// Checks in order:
/// 1. Custom path from MISE_NETRC_FILE environment variable
/// 2. %USERPROFILE%\_netrc on Windows (Windows convention)
/// 3. ~/.netrc (Unix default, also Windows fallback)
fn netrc_path() -> PathBuf {
    // Check for custom path from environment
    if let Ok(path) = std::env::var("MISE_NETRC_FILE") {
        return PathBuf::from(path);
    }

    let home = homedir::my_home().ok().flatten().unwrap_or_default();

    #[cfg(windows)]
    {
        // On Windows, try _netrc first (Windows convention)
        let windows_netrc = home.join("_netrc");
        if windows_netrc.exists() {
            return windows_netrc;
        }
    }

    home.join(".netrc")
}

/// Look up credentials for a given host from the netrc file
pub fn get_credentials(host: &str) -> Option<(String, String)> {
    let netrc = NETRC.as_ref()?;

    // First try exact host match
    if let Some(machine) = netrc.machines.iter().find(|m| {
        m.name
            .as_ref()
            .is_some_and(|name| name.eq_ignore_ascii_case(host))
    }) && let (Some(login), Some(password)) = (&machine.login, &machine.password)
    {
        log::trace!("Found netrc credentials for host: {}", host);
        return Some((login.clone(), password.clone()));
    }

    // Fall back to default machine if no exact match
    if let Some(machine) = netrc.machines.iter().find(|m| m.name.is_none())
        && let (Some(login), Some(password)) = (&machine.login, &machine.password)
    {
        log::trace!("Using default netrc credentials for host: {}", host);
        return Some((login.clone(), password.clone()));
    }

    None
}

/// Get HTTP headers with netrc credentials for the given URL
pub fn netrc_headers(url: &str) -> HeaderMap {
    use base64::Engine;
    use base64::prelude::BASE64_STANDARD;

    let mut headers = HeaderMap::new();
    if let Ok(parsed) = url::Url::parse(url)
        && let Some(host) = parsed.host_str()
        && let Some((login, password)) = get_credentials(host)
    {
        let credentials = BASE64_STANDARD.encode(format!("{login}:{password}"));
        if let Ok(value) = HeaderValue::from_str(&format!("Basic {credentials}")) {
            headers.insert("authorization", value);
        }
    }
    headers
}
