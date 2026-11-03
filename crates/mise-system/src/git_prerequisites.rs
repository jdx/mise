//! Offer to install `git` (and an SSH client) before cloning a repository.
//!
//! `mise bootstrap --from` and `--adopt` shell out to `git`, and an SSH URL
//! also needs `ssh`. A fresh machine may have neither, and the repository's
//! own `[bootstrap.packages]` cannot install them because it has not been
//! cloned yet.
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use eyre::{Result, bail};

use super::packages::{
    InstallOpts, PackageDesiredState, PackageRequest, SystemPackageManager, builtin_managers,
};
use mise_ui::prompt::{self, Confirmation};

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
        ("choco", "git"),
        ("dnf", "git"),
        ("pacman", "git"),
        ("scoop", "git"),
        ("winget", "Git.Git"),
        ("zypper", "git"),
    ],
};

// brew, choco, scoop and winget are absent: macOS and Windows ship an SSH client
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

/// Whether `binary` is on PATH, or in one of `extra_dirs`, and usable.
///
/// Looked up fresh each time, since `ensure` checks again after installing and
/// `file::which` would serve the earlier miss from its cache. The PATH behind
/// the lookup is the one we started with, so a directory the install added
/// has to be named in `extra_dirs` to be seen.
fn is_installed(binary: &str, extra_dirs: &[PathBuf]) -> bool {
    [
        mise_util::file::which_spawnable(binary),
        mise_util::file::which_spawnable_in(binary, extra_dirs),
    ]
    .into_iter()
    .flatten()
    .any(|path| is_usable(binary, &path))
}

/// On macOS, `/usr/bin/git` is a stub that opens the Xcode Command Line Tools
/// installer until they are installed, so it does not count.
fn is_usable(binary: &str, path: &Path) -> bool {
    if !(cfg!(target_os = "macos") && binary == "git" && path == Path::new("/usr/bin/git")) {
        return true;
    }
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
    succeeds("xcode-select", "-p") && succeeds("/usr/bin/git", "--version")
}

fn missing_for(origin: &str, extra_dirs: &[PathBuf]) -> Vec<&'static Prerequisite> {
    let mut needed = vec![&GIT];
    if uses_ssh(origin) {
        needed.push(&SSH);
    }
    needed
        .into_iter()
        .filter(|p| !is_installed(p.binary, extra_dirs))
        .collect()
}

/// Where `manager` installs binaries, when PATH need not list it already.
/// Homebrew is the case that matters: its prefix is not in PATH by default.
fn manager_bin_dir(manager: &str) -> Option<PathBuf> {
    (manager == "brew").then(|| mise_util::env::system_brew_prefix().join("bin"))
}

/// `current` with `dir` first and any later copy of it dropped, so a directory
/// already listed behind a shadowing one still wins. Other entries keep their
/// order.
fn path_with_dir_first(current: &OsStr, dir: &Path) -> Option<OsString> {
    let rest = mise_util::env::split_paths(current).filter(|p| p != dir);
    mise_util::env::join_paths(std::iter::once(dir.to_path_buf()).chain(rest)).ok()
}

/// Put `dir` first on the PATH child processes inherit, so the clone runs the
/// `git` that was just installed rather than one the old PATH shadows it with.
fn prepend_to_path(dir: &Path) {
    let key = &*mise_util::env::PATH_KEY;
    let current = mise_util::env::var_os(key).unwrap_or_default();
    if let Some(joined) = path_with_dir_first(&current, dir) {
        mise_util::env::set_var(key, joined);
    }
}

/// Managers in the order to try them. A distribution's own manager comes
/// before Homebrew, which also runs on Linux, and before the Windows ones.
const MANAGER_ORDER: &[&str] = &[
    "apt", "dnf", "pacman", "zypper", "apk", "brew", "winget", "scoop", "choco",
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
    let missing = missing_for(origin, &[]);
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
    if !yes && !mise_settings::Settings::get().yes {
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
                ask: false,
            },
        )
        .await?;
    // The manager may have installed into a directory PATH does not list,
    // so make it reachable before the check below
    let installed_into: Vec<PathBuf> = manager_bin_dir(manager.name()).into_iter().collect();
    for dir in &installed_into {
        prepend_to_path(dir);
    }
    let still_missing = missing_for(origin, &installed_into);
    if !still_missing.is_empty() {
        bail!(
            "installed {packages} with {}, but {} is still not on PATH. An installer that \
             extends PATH for new shells only, such as winget, needs a new shell before this \
             command can see it",
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
    #[cfg(unix)]
    fn path_moves_an_already_listed_dir_to_the_front() {
        let current = OsString::from("/usr/bin:/opt/homebrew/bin:/bin");
        let joined = path_with_dir_first(&current, Path::new("/opt/homebrew/bin")).unwrap();
        assert_eq!(joined, OsString::from("/opt/homebrew/bin:/usr/bin:/bin"));
    }

    #[test]
    #[cfg(unix)]
    fn finds_a_binary_only_in_an_extra_dir() {
        let dir = tempfile::tempdir().unwrap();
        let name = "mise-git-prerequisite-probe";
        let binary = dir.path().join(name);
        std::fs::write(&binary, "#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        std::fs::set_permissions(&binary, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .unwrap();
        assert!(!is_installed(name, &[]));
        assert!(is_installed(name, &[dir.path().to_path_buf()]));
    }

    #[test]
    fn manager_must_supply_every_missing_binary() {
        let (manager, requests) =
            pick_manager(&[&GIT]).map_or((None, vec![]), |(m, r)| (Some(m), r));
        if let Some(manager) = manager {
            assert_eq!(requests.len(), 1, "{}", manager.name());
        }
        // winget, scoop and choco have no OpenSSH package, so none may be picked
        // when ssh is missing
        if let Some((manager, requests)) = pick_manager(&[&GIT, &SSH]) {
            assert_eq!(requests.len(), 2);
            assert!(!["brew", "choco", "scoop", "winget"].contains(&manager.name()));
        }
    }
}
