use super::SandboxConfig;

/// Sanitize a string for use in an SBPL profile.
/// Escapes double quotes and backslashes to prevent injection.
fn sbpl_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

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
    "/private/tmp",
    "/private/etc",
    "/private/var/run",
    "/opt/homebrew",
    "/nix",
];

/// Generate a Seatbelt (SBPL) profile string from sandbox config.
pub async fn generate_seatbelt_profile(config: &SandboxConfig) -> String {
    let mut rules = Vec::new();
    rules.push("(version 1)".to_string());
    rules.push("(allow default)".to_string());

    // Filesystem write restrictions
    if config.effective_deny_write() {
        rules.push("(deny file-write*)".to_string());
        rules.push("(allow file-write* (subpath \"/tmp\"))".to_string());
        rules.push("(allow file-write* (subpath \"/private/tmp\"))".to_string());
        rules.push("(allow file-write* (subpath \"/dev\"))".to_string());
        for path in &config.allow_write {
            let path_str = sbpl_escape(&path.to_string_lossy());
            rules.push(format!("(allow file-write* (subpath \"{path_str}\"))"));
        }
    }

    // Filesystem read restrictions
    if config.effective_deny_read() {
        rules.push("(deny file-read*)".to_string());
        for path in SYSTEM_READ_PATHS {
            rules.push(format!("(allow file-read* (subpath \"{path}\"))"));
        }
        let data_dir = &*crate::env::MISE_DATA_DIR;
        let data_str = sbpl_escape(&data_dir.to_string_lossy());
        rules.push(format!("(allow file-read* (subpath \"{data_str}\"))"));
        for path in &config.allow_read {
            let path_str = sbpl_escape(&path.to_string_lossy());
            rules.push(format!("(allow file-read* (subpath \"{path_str}\"))"));
        }
        // allow_write paths are implicitly readable — emit AFTER deny-read
        for path in &config.allow_write {
            let path_str = sbpl_escape(&path.to_string_lossy());
            rules.push(format!("(allow file-read* (subpath \"{path_str}\"))"));
        }
    }

    // Network restrictions
    if config.effective_deny_net() {
        rules.push("(deny network*)".to_string());
        // Always allow local/unix sockets
        rules.push("(allow network* (local unix))".to_string());
        if !config.allow_net.is_empty() {
            // Allow DNS lookups via mDNSResponder (needed for hostname resolution)
            rules.push(
                "(allow network* (remote unix-socket (path-literal \"/var/run/mDNSResponder\")))"
                    .to_string(),
            );
            // Resolve all hostnames to IPs in parallel — Seatbelt's `ip` predicate requires IP literals
            let lookups: Vec<_> = config
                .allow_net
                .iter()
                .map(|host| {
                    let host = host.clone();
                    tokio::spawn(async move {
                        match tokio::net::lookup_host(format!("{host}:0")).await {
                            Ok(addrs) => {
                                let ips: Vec<_> = addrs.map(|a| a.ip()).collect();
                                (host, ips)
                            }
                            Err(_) => (host, vec![]),
                        }
                    })
                })
                .collect();
            for handle in lookups {
                if let Ok((host, ips)) = handle.await {
                    if ips.is_empty() {
                        // Resolution failed — use the value directly (might be an IP already)
                        let host = sbpl_escape(&host);
                        rules.push(format!("(allow network* (remote ip \"{host}:*\"))"));
                    } else {
                        for ip in ips {
                            rules.push(format!("(allow network* (remote ip \"{ip}:*\"))"));
                        }
                    }
                }
            }
        }
    }

    rules.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[tokio::test]
    async fn test_deny_write_profile() {
        let config = SandboxConfig {
            deny_write: true,
            ..Default::default()
        };
        let profile = generate_seatbelt_profile(&config).await;
        assert!(profile.contains("(deny file-write*)"));
        assert!(profile.contains("(allow file-write* (subpath \"/tmp\"))"));
        assert!(!profile.contains("(deny file-read*)"));
        assert!(!profile.contains("(deny network*)"));
    }

    #[tokio::test]
    async fn test_deny_net_profile() {
        let config = SandboxConfig {
            deny_net: true,
            ..Default::default()
        };
        let profile = generate_seatbelt_profile(&config).await;
        assert!(profile.contains("(deny network*)"));
        assert!(!profile.contains("(deny file-write*)"));
    }

    #[tokio::test]
    async fn test_allow_write_implies_deny() {
        let config = SandboxConfig {
            allow_write: vec![PathBuf::from("/tmp/mydir")],
            ..Default::default()
        };
        let profile = generate_seatbelt_profile(&config).await;
        assert!(profile.contains("(deny file-write*)"));
        assert!(profile.contains("(allow file-write* (subpath \"/tmp/mydir\"))"));
    }

    #[tokio::test]
    async fn test_allow_net_per_host() {
        // Test with an IP address directly (no DNS resolution needed)
        let config = SandboxConfig {
            allow_net: vec!["1.2.3.4".to_string()],
            ..Default::default()
        };
        let profile = generate_seatbelt_profile(&config).await;
        assert!(profile.contains("(deny network*)"));
        assert!(profile.contains("(allow network* (remote ip \"1.2.3.4:*\"))"));
        // mDNSResponder rule should appear exactly once
        assert_eq!(
            profile.matches("mDNSResponder").count(),
            1,
            "mDNSResponder rule should appear once"
        );
    }

    #[tokio::test]
    async fn test_deny_read_includes_system_paths() {
        let config = SandboxConfig {
            deny_read: true,
            ..Default::default()
        };
        let profile = generate_seatbelt_profile(&config).await;
        assert!(profile.contains("(deny file-read*)"));
        assert!(profile.contains("(allow file-read* (subpath \"/usr\"))"));
        assert!(profile.contains("(allow file-read* (subpath \"/System\"))"));
    }

    #[tokio::test]
    async fn test_deny_all() {
        let config = SandboxConfig {
            deny_read: true,
            deny_write: true,
            deny_net: true,
            deny_env: true,
            ..Default::default()
        };
        let profile = generate_seatbelt_profile(&config).await;
        assert!(profile.contains("(deny file-read*)"));
        assert!(profile.contains("(deny file-write*)"));
        assert!(profile.contains("(deny network*)"));
    }
}
