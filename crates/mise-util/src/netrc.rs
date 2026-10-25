use std::path::PathBuf;
use std::sync::LazyLock;

use netrc_rs::Netrc;

use crate::dirs;
use mise_settings::Settings;

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

    // Check file permissions on Unix systems
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(metadata) = std::fs::metadata(&path) {
            let mode = metadata.permissions().mode();
            if mode & 0o077 != 0 {
                warn!(
                    "netrc file {} has insecure permissions (mode: {:o}). Should be 0600 or 0400",
                    path.display(),
                    mode & 0o777
                );
            }
        }
    }

    match std::fs::read_to_string(&path) {
        Ok(content) => match Netrc::parse(strip_comments(&content), false) {
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

/// Remove comment lines (lines that start with `#`) from netrc content
///
/// curl and Python accept these lines, but netrc-rs 0.1.2 does not.
/// Each comment line becomes an empty line, so parse errors keep the
/// correct line numbers. A `#` that is not at the start of a line stays,
/// because it can be part of a password.
fn strip_comments(content: &str) -> String {
    content
        .lines()
        .map(|line| {
            if line.trim_start().starts_with('#') {
                ""
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_netrc_with_comment_lines() {
        let content = "#stepsecurity-package-config-credential-dmg-begin
machine example.com
  # indented comment
  login user
  password pa#ss
#stepsecurity-package-config-credential-dmg-end
";
        let netrc = Netrc::parse(strip_comments(content), false).unwrap();
        assert_eq!(netrc.machines.len(), 1);
        let machine = &netrc.machines[0];
        assert_eq!(machine.name.as_deref(), Some("example.com"));
        assert_eq!(machine.login.as_deref(), Some("user"));
        assert_eq!(machine.password.as_deref(), Some("pa#ss"));
    }
}
