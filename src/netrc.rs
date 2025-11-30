use std::path::PathBuf;
use std::sync::LazyLock;

use netrc_rs::Netrc;

use crate::config::Settings;
use crate::dirs;

/// Cached parsed netrc file
static NETRC: LazyLock<Option<Netrc>> = LazyLock::new(|| {
    let settings = Settings::get();
    if !settings.netrc {
        return None;
    }

    let path = netrc_path();
    if !path.exists() {
        return None;
    }

    match std::fs::read_to_string(&path) {
        Ok(content) => match Netrc::parse(content, false) {
            Ok(netrc) => {
                debug!("Loaded netrc from {}", path.display());
                Some(netrc)
            }
            Err(e) => {
                warn!("Failed to parse netrc file {}: {}", path.display(), e);
                None
            }
        },
        Err(e) => {
            warn!("Failed to read netrc file {}: {}", path.display(), e);
            None
        }
    }
});

/// Get the path to the netrc file
///
/// Checks in order:
/// 1. Custom path from settings (netrc_file)
/// 2. %USERPROFILE%\_netrc on Windows (Windows convention)
/// 3. ~/.netrc (Unix default, also Windows fallback)
fn netrc_path() -> PathBuf {
    let settings = Settings::get();
    if let Some(path) = &settings.netrc_file {
        return path.clone();
    }

    #[cfg(windows)]
    {
        // On Windows, try _netrc first (Windows convention)
        let windows_netrc = dirs::HOME.join("_netrc");
        if windows_netrc.exists() {
            return windows_netrc;
        }
    }

    dirs::HOME.join(".netrc")
}

/// Look up credentials for a given host from the netrc file
///
/// Returns `Some((login, password))` if credentials are found, `None` otherwise
pub fn get_credentials(host: &str) -> Option<(String, String)> {
    let netrc = NETRC.as_ref()?;

    // First try exact host match
    if let Some(machine) = netrc.machines.iter().find(|m| {
        m.name
            .as_ref()
            .is_some_and(|name| name.eq_ignore_ascii_case(host))
    }) && let (Some(login), Some(password)) = (&machine.login, &machine.password)
    {
        trace!("Found netrc credentials for host: {}", host);
        return Some((login.clone(), password.clone()));
    }

    // Fall back to default machine if no exact match
    if let Some(machine) = netrc.machines.iter().find(|m| m.name.is_none())
        && let (Some(login), Some(password)) = (&machine.login, &machine.password)
    {
        trace!("Using default netrc credentials for host: {}", host);
        return Some((login.clone(), password.clone()));
    }

    None
}
