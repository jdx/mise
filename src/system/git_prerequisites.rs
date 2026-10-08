//! Offer to install `git` (and an SSH client) before cloning a repository.
//!
//! `mise bootstrap --from` and `--adopt` shell out to `git`, and an SSH URL
//! also needs `ssh`. A fresh machine may have neither, and the repository's
//! own `[bootstrap.packages]` cannot install them because it has not been
//! cloned yet.
use std::sync::Arc;

use eyre::{Result, bail};

use super::packages::{
    InstallOpts, PackageDesiredState, PackageRequest, SystemPackageManager, builtin_managers,
};
use crate::ui::prompt::{self, Confirmation};

/// A host binary the clone needs, and what each package manager calls it.
struct Prerequisite {
    binary: &'static str,
    packages: &'static [(&'static str, &'static str)],
}

const GIT: Prerequisite = Prerequisite {
    binary: "git",
    packages: &[
        ("apk", "git"),
        ("apt", "git"),
        ("brew", "git"),
        ("dnf", "git"),
        ("pacman", "git"),
        ("scoop", "git"),
        ("winget", "Git.Git"),
        ("zypper", "git"),
    ],
};

// brew, scoop and winget are absent: macOS and Windows ship an SSH client
const SSH: Prerequisite = Prerequisite {
    binary: "ssh",
    packages: &[
        ("apk", "openssh-client"),
        ("apt", "openssh-client"),
        ("dnf", "openssh-clients"),
        ("pacman", "openssh"),
        ("zypper", "openssh-clients"),
    ],
};

/// Whether cloning `origin` goes through `ssh`: `ssh://` URLs and scp-style
/// `user@host:path` remotes.
fn uses_ssh(origin: &str) -> bool {
    if let Some((scheme, _)) = origin.split_once("://") {
        return scheme.eq_ignore_ascii_case("ssh") || scheme.eq_ignore_ascii_case("git+ssh");
    }
    let explicit_local = std::path::Path::new(origin).is_absolute()
        || origin.starts_with("./")
        || origin.starts_with("../");
    if explicit_local {
        return false;
    }
    // scp-style: a colon before the first slash
    let host_part = origin.split('/').next().unwrap_or(origin);
    host_part.contains(':')
}

/// Whether `binary` is on PATH and usable.
///
/// Looked up fresh each time, since `ensure` checks again after installing and
/// `file::which` would serve the earlier miss from its cache. On macOS,
/// `/usr/bin/git` is a stub that opens the Xcode Command Line Tools installer
/// until they are installed, so it does not count.
fn is_installed(binary: &str) -> bool {
    let Some(path) = crate::file::which_spawnable(binary) else {
        return false;
    };
    if cfg!(target_os = "macos") && binary == "git" && path == std::path::Path::new("/usr/bin/git")
    {
        // `xcode-select -p` only proves a developer directory is selected, and
        // running the stub without one opens the installer, so ask it second
        let succeeds = |program: &str, arg: &str| {
            std::process::Command::new(program)
                .arg(arg)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .is_ok_and(|status| status.success())
        };
        return succeeds("xcode-select", "-p") && succeeds("/usr/bin/git", "--version");
    }
    true
}

fn missing_for(origin: &str) -> Vec<&'static Prerequisite> {
    let mut needed = vec![&GIT];
    if uses_ssh(origin) {
        needed.push(&SSH);
    }
    needed
        .into_iter()
        .filter(|p| !is_installed(p.binary))
        .collect()
}

/// Managers in the order to try them. A distribution's own manager comes
/// before Homebrew, which also runs on Linux, and before the Windows ones.
const MANAGER_ORDER: &[&str] = &[
    "apt", "dnf", "pacman", "zypper", "apk", "brew", "winget", "scoop",
];

/// The first available package manager that can supply every missing binary.
fn pick_manager(
    missing: &[&'static Prerequisite],
) -> Option<(Arc<dyn SystemPackageManager>, Vec<PackageRequest>)> {
    let mut managers = builtin_managers();
    managers.retain(|manager| MANAGER_ORDER.contains(&manager.name()));
    managers.sort_by_key(|manager| MANAGER_ORDER.iter().position(|n| *n == manager.name()));
    for manager in managers {
        if !manager.is_available() {
            continue;
        }
        let names: Option<Vec<&str>> = missing
            .iter()
            .map(|p| {
                p.packages
                    .iter()
                    .find(|(m, _)| *m == manager.name())
                    .map(|(_, pkg)| *pkg)
            })
            .collect();
        let Some(names) = names else { continue };
        let requests = names
            .into_iter()
            .map(|name| PackageRequest {
                name: name.to_string(),
                version: None,
                tap_url: None,
                desired: PackageDesiredState::Present,
            })
            .collect();
        return Some((manager, requests));
    }
    None
}

/// Make sure the binaries cloning `origin` needs are installed, offering to
/// install them with the host package manager when they are not.
///
/// `yes`, or the global `yes` setting (`MISE_YES`), accepts the offer without
/// asking. A dry run changes nothing, so it
/// reports the missing binaries instead.
pub async fn ensure(origin: &str, yes: bool, dry_run: bool) -> Result<()> {
    let missing = missing_for(origin);
    if missing.is_empty() {
        return Ok(());
    }
    let binaries = missing
        .iter()
        .map(|p| p.binary)
        .collect::<Vec<_>>()
        .join(" and ");
    let Some((manager, requests)) = pick_manager(&missing) else {
        bail!(
            "{binaries} must be installed to clone {origin}, and mise found no package manager to install it with. Install it yourself, then run this command again"
        );
    };
    let packages = requests
        .iter()
        .map(|r| r.name.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    if dry_run {
        bail!(
            "{binaries} must be installed to clone {origin}; a real run would install {packages} with {}",
            manager.name()
        );
    }
    if !yes && !crate::config::Settings::get().yes {
        let answer = prompt::confirm(format!(
            "{binaries} must be installed to clone {origin}. Install {packages} with {}?",
            manager.name()
        ))?;
        match answer {
            Confirmation::Yes => {}
            Confirmation::Unavailable | Confirmation::Unanswered => bail!(
                "{binaries} must be installed to clone {origin}. Run again with --yes to install {packages} with {}, or install it yourself",
                manager.name()
            ),
            Confirmation::No => bail!("{binaries} must be installed to clone {origin}"),
        }
    }
    info!("installing {packages} with {}", manager.name());
    manager
        .install(
            &requests,
            &InstallOpts {
                dry_run: false,
                update: false,
            },
        )
        .await?;
    let still_missing = missing_for(origin);
    if !still_missing.is_empty() {
        bail!(
            "installed {packages} with {}, but {} is still not on PATH",
            manager.name(),
            still_missing
                .iter()
                .map(|p| p.binary)
                .collect::<Vec<_>>()
                .join(" and ")
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_ssh_remotes() {
        for origin in [
            "git@github.com:example/setup.git",
            "ssh://git@github.com/example/setup.git",
            "host:example/setup.git",
        ] {
            assert!(uses_ssh(origin), "{origin}");
        }
    }

    #[test]
    fn https_and_local_remotes_do_not_need_ssh() {
        for origin in [
            "https://github.com/example/setup.git",
            "/srv/git/setup.git",
            "./setup",
            "../setup",
            "/srv/a:b/setup.git",
        ] {
            assert!(!uses_ssh(origin), "{origin}");
        }
    }

    #[test]
    fn manager_must_supply_every_missing_binary() {
        let (manager, requests) =
            pick_manager(&[&GIT]).map_or((None, vec![]), |(m, r)| (Some(m), r));
        if let Some(manager) = manager {
            assert_eq!(requests.len(), 1, "{}", manager.name());
        }
        // winget and scoop have no OpenSSH package, so neither may be picked
        // when ssh is missing
        if let Some((manager, requests)) = pick_manager(&[&GIT, &SSH]) {
            assert_eq!(requests.len(), 2);
            assert!(!["brew", "scoop", "winget"].contains(&manager.name()));
        }
    }
}
