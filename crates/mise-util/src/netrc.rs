use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use netrc_jdx::Netrc;

use crate::{dirs, file};
use mise_settings::Settings;

/// Cached parsed netrc file
static NETRC: LazyLock<Option<Netrc>> = LazyLock::new(|| {
    let settings = Settings::get();
    if !settings.netrc {
        return None;
    }

    let path = netrc_path()?;
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
        Ok(content) => match Netrc::parse(&content) {
            Ok(netrc) => {
                debug!("Loaded netrc from {}", path.display());
                Some(netrc)
            }
            Err(e) => {
                warn!("{}", parse_error_warning(&path, &e));
                None
            }
        },
        Err(e) => {
            warn!("Failed to read netrc file {}: {}", path.display(), e);
            None
        }
    }
});

/// The warning for a netrc file that does not parse. It names the file and
/// the line, and nothing the file holds: the parser's message quotes the token
/// it stopped at, which can be part of a password (`password "two words"` is
/// split at the space, so `words"` is the unexpected token).
fn parse_error_warning(path: &Path, err: &netrc_jdx::Error) -> String {
    format!(
        "Failed to parse netrc file {} (line {}); fix that line, or set netrc = false",
        path.display(),
        err.line
    )
}

/// Get the path to the netrc file
///
/// Checks in order:
/// 1. Custom path from settings (netrc_file)
/// 2. %USERPROFILE%\_netrc on Windows (Windows convention)
/// 3. ~/.netrc (Unix default, also Windows fallback)
fn netrc_path() -> Option<PathBuf> {
    let settings = Settings::get();
    if let Some(path) = &settings.netrc_file {
        return configured_netrc_path(path);
    }

    #[cfg(windows)]
    {
        // On Windows, try _netrc first (Windows convention)
        let windows_netrc = dirs::HOME.join("_netrc");
        if windows_netrc.exists() {
            return Some(windows_netrc);
        }
    }

    Some(dirs::HOME.join(".netrc"))
}

/// `netrc_file` with `~/` expanded. A path that is still relative is refused:
/// it would name a file in whatever directory mise runs in, which a cloned
/// repository supplies, so the repository would choose the logins mise sends.
fn configured_netrc_path(path: &Path) -> Option<PathBuf> {
    let path = file::replace_path(path);
    if !path.is_absolute() {
        warn!(
            "ignoring netrc_file {}: it is not an absolute path, so mise is not using a netrc file. Set netrc_file to an absolute path or one that starts with ~/",
            path.display()
        );
        return None;
    }
    Some(path)
}

/// Look up credentials for a given host from the netrc file
///
/// Returns `Some((login, password))` if credentials are found, `None` otherwise
pub fn get_credentials(host: &str) -> Option<(String, String)> {
    let netrc = NETRC.as_ref()?;

    let (login, password) = netrc.credentials(host)?;
    trace!("Found netrc credentials for host: {}", host);
    Some((login.to_string(), password.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credentials_skip_comment_lines() {
        let netrc = Netrc::parse(
            "#stepsecurity-begin\nmachine example.com\n  login user\n  password pa#ss\n#stepsecurity-end\n",
        )
        .unwrap();
        assert_eq!(netrc.credentials("example.com"), Some(("user", "pa#ss")));
    }

    #[test]
    fn parse_error_warning_does_not_quote_the_file() {
        let content = "# ci\nmachine example.com login user password \"correct horse\"\n";
        let err = Netrc::parse(content).unwrap_err();
        let warning = parse_error_warning(Path::new("/home/ci/.netrc"), &err);
        assert!(!warning.contains("horse"), "{warning}");
        assert!(
            warning.starts_with("Failed to parse netrc file /home/ci/.netrc (line 2)"),
            "{warning}"
        );
    }

    #[test]
    fn configured_netrc_path_expands_home_and_refuses_relative_paths() {
        assert_eq!(
            configured_netrc_path(Path::new("~/.config/netrc")),
            Some(dirs::HOME.join(".config").join("netrc"))
        );
        let absolute = dirs::HOME.join("netrc");
        assert_eq!(configured_netrc_path(&absolute), Some(absolute));
        assert_eq!(configured_netrc_path(Path::new("~~/netrc")), None);
        assert_eq!(configured_netrc_path(Path::new("x/netrc")), None);
    }
}
