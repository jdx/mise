use super::SandboxConfig;

/// System paths that are always readable on macOS.
const SYSTEM_READ_PATHS: &[&str] = &[
    "/System",
    "/Library",
    "/usr",
    "/bin",
    "/sbin",
    "/dev",
    "/etc",
    "/var/run",
    "/tmp",
    "/private",
    "/opt/homebrew",
    "/nix",
];

/// Generate a Seatbelt (SBPL) profile string from sandbox config.
pub fn generate_seatbelt_profile(config: &SandboxConfig) -> String {
    let mut rules = Vec::new();
    rules.push("(version 1)".to_string());
    rules.push("(allow default)".to_string());

    // Filesystem write restrictions
    if config.effective_deny_write() {
        rules.push("(deny file-write*)".to_string());
        // Always allow writes to /tmp and /private/tmp
        rules.push("(allow file-write* (subpath \"/tmp\"))".to_string());
        rules.push("(allow file-write* (subpath \"/private/tmp\"))".to_string());
        // Allow writes to /dev (needed for /dev/null, /dev/tty, etc.)
        rules.push("(allow file-write* (subpath \"/dev\"))".to_string());
        for path in &config.allow_write {
            let path_str = path.to_string_lossy();
            rules.push(format!("(allow file-write* (subpath \"{path_str}\"))"));
            // Writable paths are implicitly readable
            if config.effective_deny_read() {
                rules.push(format!("(allow file-read* (subpath \"{path_str}\"))"));
            }
        }
    }

    // Filesystem read restrictions
    if config.effective_deny_read() {
        rules.push("(deny file-read*)".to_string());
        // System paths always readable
        for path in SYSTEM_READ_PATHS {
            rules.push(format!("(allow file-read* (subpath \"{path}\"))"));
        }
        // Mise tool install dirs
        if let Some(home) = dirs::home_dir() {
            let installs = home.join(".local/share/mise/installs");
            let installs_str = installs.to_string_lossy();
            rules.push(format!("(allow file-read* (subpath \"{installs_str}\"))"));
            // Also allow reading mise shims and other data
            let data = home.join(".local/share/mise");
            let data_str = data.to_string_lossy();
            rules.push(format!("(allow file-read* (subpath \"{data_str}\"))"));
        }
        for path in &config.allow_read {
            let path_str = path.to_string_lossy();
            rules.push(format!("(allow file-read* (subpath \"{path_str}\"))"));
        }
        // allow_write paths are implicitly readable (handled above)
    }

    // Network restrictions
    if config.effective_deny_net() {
        rules.push("(deny network*)".to_string());
        // Always allow local/unix sockets
        rules.push("(allow network* (local unix))".to_string());
        for host in &config.allow_net {
            rules.push(format!("(allow network* (remote ip \"{host}:*\"))"));
            // Also allow DNS lookups
            rules.push(format!(
                "(allow network* (remote unix-socket (path-literal \"/var/run/mDNSResponder\")))"
            ));
        }
    }

    rules.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_deny_write_profile() {
        let config = SandboxConfig {
            deny_write: true,
            ..Default::default()
        };
        let profile = generate_seatbelt_profile(&config);
        assert!(profile.contains("(deny file-write*)"));
        assert!(profile.contains("(allow file-write* (subpath \"/tmp\"))"));
        assert!(!profile.contains("(deny file-read*)"));
        assert!(!profile.contains("(deny network*)"));
    }

    #[test]
    fn test_deny_net_profile() {
        let config = SandboxConfig {
            deny_net: true,
            ..Default::default()
        };
        let profile = generate_seatbelt_profile(&config);
        assert!(profile.contains("(deny network*)"));
        assert!(!profile.contains("(deny file-write*)"));
    }

    #[test]
    fn test_allow_write_implies_deny() {
        let config = SandboxConfig {
            allow_write: vec![PathBuf::from("/tmp/mydir")],
            ..Default::default()
        };
        let profile = generate_seatbelt_profile(&config);
        assert!(profile.contains("(deny file-write*)"));
        assert!(profile.contains("(allow file-write* (subpath \"/tmp/mydir\"))"));
    }

    #[test]
    fn test_allow_net_per_host() {
        let config = SandboxConfig {
            allow_net: vec!["registry.npmjs.org".to_string()],
            ..Default::default()
        };
        let profile = generate_seatbelt_profile(&config);
        assert!(profile.contains("(deny network*)"));
        assert!(profile.contains("(allow network* (remote ip \"registry.npmjs.org:*\"))"));
    }

    #[test]
    fn test_deny_read_includes_system_paths() {
        let config = SandboxConfig {
            deny_read: true,
            ..Default::default()
        };
        let profile = generate_seatbelt_profile(&config);
        assert!(profile.contains("(deny file-read*)"));
        assert!(profile.contains("(allow file-read* (subpath \"/usr\"))"));
        assert!(profile.contains("(allow file-read* (subpath \"/System\"))"));
    }

    #[test]
    fn test_deny_all() {
        let config = SandboxConfig {
            deny_read: true,
            deny_write: true,
            deny_net: true,
            deny_env: true,
            ..Default::default()
        };
        let profile = generate_seatbelt_profile(&config);
        assert!(profile.contains("(deny file-read*)"));
        assert!(profile.contains("(deny file-write*)"));
        assert!(profile.contains("(deny network*)"));
    }
}
