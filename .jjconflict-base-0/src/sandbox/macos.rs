use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

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

/// Ancestor directories a read-restricted profile has to expose metadata for,
/// each one once and in a stable order.
///
/// `realpath(3)` resolves a path one component at a time, so a denied `lstat`
/// on an intermediate directory fails the whole call even when the target
/// subtree is fully readable — an ordinary `open` of the same file still
/// succeeds, because the kernel's path walk only permission-checks the target
/// vnode. Ruby hits this before running a line of code: the portable builds use
/// `--enable-load-relative` and resolve their own executable at startup. That
/// breaks evaluating a third-party tap definition for every Ruby mise can pick
/// — the provisioned one under `MISE_DATA_DIR`
/// (<https://github.com/jdx/mise/discussions/12969>) and Homebrew's vendored
/// one under `/opt/homebrew` alike; `/private/tmp`
/// (<https://github.com/jdx/mise/discussions/12937>) was the first case found
/// and is covered here by the same derivation rather than by a rule of its own.
///
/// Metadata only, so this does not widen what the sandbox exposes in kind: a
/// directory named here can be `stat`ed but not listed, and its entries stay
/// unreadable. Nothing outside the ancestor chains becomes reachable.
///
/// `/` is left out. Nothing asks for its metadata — a profile carrying only
/// `/opt` lets a Ruby under `/opt/homebrew` start — and
/// `(allow file-read-data (literal "/"))` already covers process startup.
///
/// Lexical, which only matters for a path that does not exist:
/// [`SandboxConfig::resolve_paths`] canonicalizes the ones that do, and a rule
/// naming a missing directory is inert either way.
fn metadata_ancestors<'a>(paths: impl IntoIterator<Item = &'a Path>) -> BTreeSet<PathBuf> {
    let root = Path::new("/");
    paths
        .into_iter()
        .flat_map(|path| path.ancestors().skip(1))
        .filter(|ancestor| *ancestor != root && !ancestor.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .collect()
}

/// [`metadata_ancestors`] plus `path` itself.
///
/// For a spelling no `(subpath …)` rule names — a symlink, since Seatbelt
/// matches the canonical path — the link node has to be `lstat`-able too, or the
/// walk stops on it rather than on one of its parents. Still metadata: the link
/// can be resolved, and what it points at is readable only because the
/// canonical target has its own rule.
fn walk_through(path: &Path) -> BTreeSet<PathBuf> {
    let mut paths = metadata_ancestors(std::iter::once(path));
    paths.insert(path.to_path_buf());
    paths
}

/// [`metadata_ancestors`] for a directory whose rule names `target` while
/// callers reach it as `configured`. The two differ only when `configured` runs
/// through a symlink, and then both spellings have to be walkable.
fn walk_metadata(configured: &Path, target: &Path) -> BTreeSet<PathBuf> {
    let mut paths = metadata_ancestors(std::iter::once(target));
    if configured != target {
        paths.extend(walk_through(configured));
    }
    paths
}

/// Generate a Seatbelt (SBPL) profile string from sandbox config.
pub(crate) async fn generate_seatbelt_profile(
    config: &SandboxConfig,
    initial_program: Option<&std::path::Path>,
) -> String {
    let mut rules = Vec::new();
    rules.push("(version 1)".to_string());
    rules.push("(allow default)".to_string());

    // Filesystem write restrictions
    if config.effective_deny_write() {
        rules.push("(deny file-write*)".to_string());
        if !config.deny_temp_write {
            rules.push("(allow file-write* (subpath \"/tmp\"))".to_string());
            rules.push("(allow file-write* (subpath \"/private/tmp\"))".to_string());
        }
        rules.push("(allow file-write* (subpath \"/dev\"))".to_string());
        for path in &config.allow_write {
            let path_str = sbpl_escape(&path.to_string_lossy());
            rules.push(format!("(allow file-write* (subpath \"{path_str}\"))"));
            rules.push(format!("(allow file-write* (literal \"{path_str}\"))"));
        }
    }

    // Filesystem read restrictions
    if config.effective_deny_read() {
        rules.push("(deny file-read*)".to_string());
        // Seatbelt requires data access to the root vnode for process startup and getcwd.
        // This exposes names directly under `/`, but descendants still obey the read rules.
        rules.push("(allow file-read-data (literal \"/\"))".to_string());
        for path in SYSTEM_READ_PATHS {
            rules.push(format!("(allow file-read* (subpath \"{path}\"))"));
        }
        // Seatbelt matches a rule against the canonical path, so a subpath
        // naming a symlinked data dir grants nothing at all — not even a read.
        // Name what it resolves to; `walk_metadata` keeps the configured
        // spelling walkable. Unlike this one, SYSTEM_READ_PATHS is left alone:
        // it already lists both spellings where they differ (`/tmp` and
        // `/private/tmp`, `/etc` and `/private/etc`, `/var/run` and
        // `/private/var/run`).
        let data_dir = &*crate::env::MISE_DATA_DIR;
        let data_canonical = data_dir.canonicalize();
        let data_target = data_canonical.as_deref().unwrap_or(data_dir.as_path());
        let data_str = sbpl_escape(&data_target.to_string_lossy());
        rules.push(format!("(allow file-read* (subpath \"{data_str}\"))"));
        for path in &config.allow_read {
            let path_str = sbpl_escape(&path.to_string_lossy());
            rules.push(format!("(allow file-read* (subpath \"{path_str}\"))"));
            rules.push(format!("(allow file-read* (literal \"{path_str}\"))"));
        }
        // allow_write paths are implicitly readable — emit AFTER deny-read
        for path in &config.allow_write {
            let path_str = sbpl_escape(&path.to_string_lossy());
            rules.push(format!("(allow file-read* (subpath \"{path_str}\"))"));
            rules.push(format!("(allow file-read* (literal \"{path_str}\"))"));
        }
        // Every path allowed above, so a `realpath` of anything inside one of
        // them can walk down to it. See `metadata_ancestors`.
        let mut metadata = metadata_ancestors(
            SYSTEM_READ_PATHS
                .iter()
                .copied()
                .map(Path::new)
                .chain(config.allow_read.iter().map(PathBuf::as_path))
                .chain(config.allow_write.iter().map(PathBuf::as_path)),
        );
        metadata.extend(walk_metadata(data_dir, data_target));
        // The allow-list entries above are canonical — `resolve_paths` made them
        // so — and the spellings it replaced are the ones a caller will use.
        for path in &config.symlinked_allow_paths {
            metadata.extend(walk_through(path));
        }
        for ancestor in metadata {
            let path_str = sbpl_escape(&ancestor.to_string_lossy());
            rules.push(format!(
                "(allow file-read-metadata (literal \"{path_str}\"))"
            ));
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

    if config.deny_process {
        rules.push("(deny process-fork)".to_string());
        rules.push("(deny process-exec)".to_string());
        if let Some(path) = initial_program {
            let path_str = sbpl_escape(&path.to_string_lossy());
            rules.push(format!("(allow process-exec (literal \"{path_str}\"))"));
            if let Ok(canonical) = path.canonicalize()
                && canonical != path
            {
                let canonical = sbpl_escape(&canonical.to_string_lossy());
                rules.push(format!("(allow process-exec (literal \"{canonical}\"))"));
            }
        }
    }

    rules.join("\n")
}

#[cfg(test)]
mod tests {
    // `Path` and `PathBuf` come from the parent module.
    use super::*;
    use std::{fs, process::Command};

    #[tokio::test]
    async fn test_deny_write_profile() {
        let config = SandboxConfig {
            deny_write: true,
            ..Default::default()
        };
        let profile = generate_seatbelt_profile(&config, None).await;
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
        let profile = generate_seatbelt_profile(&config, None).await;
        assert!(profile.contains("(deny network*)"));
        assert!(!profile.contains("(deny file-write*)"));
    }

    #[tokio::test]
    async fn test_allow_write_implies_deny() {
        let config = SandboxConfig {
            allow_write: vec![PathBuf::from("/tmp/mydir")],
            ..Default::default()
        };
        let profile = generate_seatbelt_profile(&config, None).await;
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
        let profile = generate_seatbelt_profile(&config, None).await;
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
        let profile = generate_seatbelt_profile(&config, None).await;
        assert!(profile.contains("(deny file-read*)"));
        assert!(profile.contains("(allow file-read* (subpath \"/usr\"))"));
        assert!(profile.contains("(allow file-read* (subpath \"/System\"))"));
    }

    #[tokio::test]
    async fn test_allow_read_executes_shell_without_reading_siblings() {
        let root = tempfile::tempdir().unwrap();
        let allowed_dir = root.path().join("allowed");
        fs::create_dir(&allowed_dir).unwrap();
        let allowed_file = allowed_dir.join("allowed.txt");
        let denied_file = root.path().join("denied.txt");
        fs::write(&allowed_file, "allowed").unwrap();
        fs::write(&denied_file, "denied").unwrap();
        let allowed_file = allowed_file.canonicalize().unwrap();
        let denied_file = denied_file.canonicalize().unwrap();

        let mut config = SandboxConfig {
            deny_read: true,
            allow_read: vec![allowed_file.clone()],
            ..Default::default()
        };
        config.resolve_paths();
        let profile = generate_seatbelt_profile(&config, Some(Path::new("/bin/sh"))).await;
        let read = |path: &std::path::Path| {
            Command::new("sandbox-exec")
                .current_dir("/")
                .args(["-p", &profile, "--", "/bin/sh", "-c", "cat \"$1\"", "sh"])
                .arg(path)
                .output()
                .unwrap()
        };

        let allowed = read(&allowed_file);
        assert!(
            allowed.status.success(),
            "sandboxed shell failed: {}",
            String::from_utf8_lossy(&allowed.stderr)
        );
        assert!(!read(&denied_file).status.success());
    }

    #[tokio::test]
    async fn test_deny_read_allows_metadata_on_ancestors_of_system_paths() {
        let config = SandboxConfig {
            deny_read: true,
            ..Default::default()
        };
        let profile = generate_seatbelt_profile(&config, None).await;
        // `/private` is derived now rather than written by hand, so assert it
        // explicitly: it is what made Ruby under `/private/tmp` start again.
        for ancestor in ["/private", "/private/var", "/var", "/opt"] {
            assert!(
                profile.contains(&format!(
                    "(allow file-read-metadata (literal \"{ancestor}\"))"
                )),
                "no metadata rule for {ancestor} in:\n{profile}"
            );
        }
        // Root is already covered by the file-read-data rule and nothing asks
        // for its metadata, so it should not appear.
        assert!(!profile.contains("(allow file-read-metadata (literal \"/\"))"));
    }

    #[tokio::test]
    async fn test_deny_read_allows_metadata_on_ancestors_of_the_data_dir() {
        // Derived from the running configuration rather than hardcoded: the
        // data dir moves with MISE_DATA_DIR, and the Ruby mise provisions for
        // tap metadata lives under it.
        let config = SandboxConfig {
            deny_read: true,
            ..Default::default()
        };
        let profile = generate_seatbelt_profile(&config, None).await;
        let data_dir = &*crate::env::MISE_DATA_DIR;
        let mut checked = 0;
        for ancestor in data_dir.ancestors().skip(1) {
            if ancestor == Path::new("/") || ancestor.as_os_str().is_empty() {
                break;
            }
            assert!(
                profile.contains(&format!(
                    "(allow file-read-metadata (literal \"{}\"))",
                    ancestor.display()
                )),
                "no metadata rule for {} in:\n{profile}",
                ancestor.display()
            );
            checked += 1;
        }
        assert!(
            checked > 0,
            "{} has no ancestor to check",
            data_dir.display()
        );
    }

    #[tokio::test]
    async fn test_allow_read_and_allow_write_ancestors_get_metadata() {
        let root = tempfile::tempdir().unwrap();
        let readable = root.path().join("read").join("deep");
        let writable = root.path().join("write").join("deep");
        fs::create_dir_all(&readable).unwrap();
        fs::create_dir_all(&writable).unwrap();
        let readable = readable.canonicalize().unwrap();
        let writable = writable.canonicalize().unwrap();

        let mut config = SandboxConfig {
            allow_read: vec![readable.clone()],
            allow_write: vec![writable.clone()],
            ..Default::default()
        };
        config.resolve_paths();
        let profile = generate_seatbelt_profile(&config, None).await;

        // A write-allowed path is implicitly readable, so it needs the walk too.
        for path in [&readable, &writable] {
            let parent = path.parent().unwrap();
            assert!(
                profile.contains(&format!(
                    "(allow file-read-metadata (literal \"{}\"))",
                    parent.display()
                )),
                "no metadata rule for {} in:\n{profile}",
                parent.display()
            );
        }
    }

    #[test]
    fn test_walk_metadata_is_just_the_ancestors_when_the_path_is_canonical() {
        let path = Path::new("/a/b/c");
        assert_eq!(
            walk_metadata(path, path),
            [PathBuf::from("/a"), PathBuf::from("/a/b")]
                .into_iter()
                .collect::<BTreeSet<_>>()
        );
    }

    #[test]
    fn test_walk_metadata_covers_both_spellings_of_a_symlinked_path() {
        // Seatbelt matches the canonical path, so the rule names the target —
        // but a caller reaching it through the configured path has to `lstat`
        // the link itself and everything above it as well.
        let configured = Path::new("/Users/me/data");
        let target = Path::new("/Volumes/tools/mise");
        assert_eq!(
            walk_metadata(configured, target),
            [
                "/Users",
                "/Users/me",
                "/Users/me/data",
                "/Volumes",
                "/Volumes/tools",
            ]
            .into_iter()
            .map(PathBuf::from)
            .collect::<BTreeSet<_>>()
        );
    }

    #[tokio::test]
    async fn test_ancestor_metadata_is_emitted_once_per_directory() {
        // `/private/tmp`, `/private/etc` and `/private/var/run` all descend
        // from `/private`; one rule covers them.
        let config = SandboxConfig {
            deny_read: true,
            ..Default::default()
        };
        let profile = generate_seatbelt_profile(&config, None).await;
        assert_eq!(
            profile
                .matches("(allow file-read-metadata (literal \"/private\"))")
                .count(),
            1
        );
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn test_ancestor_metadata_lets_a_nested_path_resolve() {
        // The defect: `realpath(3)` walks component by component, so a file
        // inside a fully allowed subtree could not be resolved while the
        // directories leading to it were denied — while an ordinary read of the
        // same file succeeded, because the kernel's path walk only checks the
        // target. Ruby does this to its own executable at startup, which is what
        // broke evaluating a third-party tap definition.
        let root = tempfile::tempdir().unwrap();
        let nested = root.path().join("allowed").join("deep");
        fs::create_dir_all(&nested).unwrap();
        let file = nested.join("payload.txt");
        fs::write(&file, "payload").unwrap();
        let nested = nested.canonicalize().unwrap();
        let file = file.canonicalize().unwrap();

        let mut config = SandboxConfig {
            deny_read: true,
            allow_read: vec![nested],
            ..Default::default()
        };
        config.resolve_paths();
        let profile = generate_seatbelt_profile(&config, None).await;

        // Assert both halves: the two disagreeing is the signature of the bug,
        // so a read that still works is part of what is being pinned.
        let script = r#"
path = ARGV[0]
puts "realpath #{File.realpath(path)}"
puts "read #{File.read(path)}"
"#;
        let output = Command::new("sandbox-exec")
            .args([
                "-p",
                &profile,
                "--",
                "/usr/bin/ruby",
                "--disable-gems",
                "-e",
                script,
            ])
            .arg(&file)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "sandboxed Ruby failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            format!("realpath {}\nread payload\n", file.display())
        );
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn test_metadata_reaches_an_allow_listed_path_through_its_symlink() {
        // `resolve_paths` canonicalizes the allow-list and Seatbelt matches the
        // canonical path, so the rule names the target. A caller still refers to
        // the directory the way it was allowed, and that walk has to survive —
        // naming the target alone leaves it failing on the link.
        let root_dir = tempfile::tempdir().unwrap();
        let root = root_dir.path().canonicalize().unwrap();
        let target = root.join("target");
        fs::create_dir_all(target.join("deep")).unwrap();
        fs::write(target.join("deep").join("payload.txt"), "payload").unwrap();
        std::os::unix::fs::symlink(&target, root.join("link")).unwrap();
        let configured = root.join("link").join("deep");

        let mut config = SandboxConfig {
            deny_read: true,
            allow_read: vec![configured.clone()],
            ..Default::default()
        };
        config.resolve_paths();
        assert_eq!(config.allow_read, vec![target.join("deep")]);
        assert_eq!(config.symlinked_allow_paths, vec![configured.clone()]);

        let profile = generate_seatbelt_profile(&config, None).await;
        let script = r#"
path = ARGV[0]
puts "realpath #{File.realpath(path)}"
puts "read #{File.read(path)}"
"#;
        let output = Command::new("sandbox-exec")
            .args([
                "-p",
                &profile,
                "--",
                "/usr/bin/ruby",
                "--disable-gems",
                "-e",
                script,
            ])
            .arg(configured.join("payload.txt"))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "sandboxed Ruby failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            format!(
                "realpath {}\nread payload\n",
                target.join("deep").join("payload.txt").display()
            )
        );
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn test_ancestor_metadata_does_not_expose_directory_contents() {
        let config = SandboxConfig {
            deny_read: true,
            ..Default::default()
        };
        let profile = generate_seatbelt_profile(&config, None).await;
        let run = |program: &str, args: &[&str]| {
            Command::new("sandbox-exec")
                .args(["-p", &profile, "--", program])
                .args(args)
                .output()
                .unwrap()
        };

        // Each reachable ancestor answers `stat` and nothing more.
        for dir in ["/private", "/var", "/opt"] {
            let metadata = run("/usr/bin/stat", &["-f", "%N", dir]);
            assert!(
                metadata.status.success(),
                "sandboxed stat {dir} failed: {}",
                String::from_utf8_lossy(&metadata.stderr)
            );
            assert!(
                !run("/bin/ls", &[dir]).status.success(),
                "sandboxed ls {dir} succeeded"
            );
        }
        // A directory that is not an ancestor of anything allowed stays
        // invisible, so what widened is the ancestor chains and not the
        // filesystem.
        assert!(
            !run("/usr/bin/stat", &["-f", "%N", "/Applications"])
                .status
                .success()
        );
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
        let profile = generate_seatbelt_profile(&config, None).await;
        assert!(profile.contains("(deny file-read*)"));
        assert!(profile.contains("(deny file-write*)"));
        assert!(profile.contains("(deny network*)"));
    }

    #[tokio::test]
    async fn test_deny_process_allows_only_initial_executable() {
        let config = SandboxConfig {
            deny_process: true,
            ..Default::default()
        };
        let profile = generate_seatbelt_profile(&config, Some(Path::new("/usr/bin/ruby"))).await;
        assert!(profile.contains("(deny process-fork)"));
        assert!(profile.contains("(deny process-exec)"));
        assert!(profile.contains("(allow process-exec (literal \"/usr/bin/ruby\"))"));
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn test_deny_process_at_runtime() {
        let config = SandboxConfig {
            deny_process: true,
            ..Default::default()
        };
        let profile = generate_seatbelt_profile(&config, Some(Path::new("/usr/bin/ruby"))).await;
        let script = r#"
puts "ruby started"
begin
  fork { exit! }
  abort "fork escaped sandbox"
rescue SystemCallError
end
begin
  exec "/usr/bin/true"
rescue SystemCallError
end
puts "child processes blocked"
"#;
        let output = Command::new("sandbox-exec")
            .args([
                "-p",
                &profile,
                "--",
                "/usr/bin/ruby",
                "--disable-gems",
                "-e",
                script,
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "sandboxed Ruby failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "ruby started\nchild processes blocked\n"
        );
    }
}
