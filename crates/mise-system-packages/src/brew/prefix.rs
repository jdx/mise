//! The Homebrew prefix: /opt/homebrew (arm64 macOS) or
//! /home/linuxbrew/.linuxbrew (Linux) — detection, ownership, bootstrap.

use std::path::{Path, PathBuf};

use eyre::bail;

use eyre::Result;
use mise_ui::sudo;

/// subdirectories brew's install.sh creates, mirrored here
const SUBDIRS: &[&str] = &[
    "bin",
    "etc",
    "include",
    "lib",
    "sbin",
    "share",
    "var",
    "opt",
    "Cellar",
    "Caskroom",
    "Frameworks",
    "etc/bash_completion.d",
    "share/zsh",
    "share/zsh/site-functions",
    "share/doc",
    "share/man",
    "share/man/man1",
    "var/homebrew",
    "var/homebrew/linked",
];

pub(super) fn prefix() -> PathBuf {
    mise_util::env::system_brew_prefix()
}

pub(super) fn cellar() -> PathBuf {
    prefix().join("Cellar")
}

/// Homebrew's active linked-keg record for a non-keg-only formula.
pub(super) fn linked_keg_record(name: &str) -> PathBuf {
    prefix().join("var/homebrew/linked").join(name)
}

/// where brew would keep its own repository — referenced by the
/// @@HOMEBREW_REPOSITORY@@ placeholder (== prefix on arm64 macOS, a
/// subdirectory on Linux)
pub(super) fn repository() -> PathBuf {
    if cfg!(target_os = "macos") {
        prefix()
    } else {
        prefix().join("Homebrew")
    }
}

/// Linux bottles are built with their ELF interpreter set to
/// `<prefix>/lib/ld.so`; brew points that symlink at a brewed glibc when one
/// is installed, otherwise at the host's dynamic linker. Mirror that here.
/// Called before and after pours so a glibc poured in the current run
/// repoints the symlink.
pub(super) fn setup_linux_runtime() -> Result<()> {
    if !cfg!(target_os = "linux") {
        return Ok(());
    }
    let lib = prefix().join("lib");
    mise_util::file::create_dir_all(&lib)?;
    let ld = lib.join("ld.so");
    // a brewed glibc keg takes precedence (hosts older than the bottles'
    // build glibc); otherwise the host loader
    let loader_names = ["ld-linux-x86-64.so.2", "ld-linux-aarch64.so.1"];
    let brewed_glibc = super::pour::installed_versions("glibc")
        .into_iter()
        .filter_map(|version| {
            let keg_lib = cellar().join("glibc").join(version).join("lib");
            loader_names
                .iter()
                .map(|name| keg_lib.join(name))
                .find(|p| p.exists())
        })
        .next();
    if let Some(target) = brewed_glibc {
        // repoint at the brewed glibc even if ld.so already exists
        if std::fs::read_link(&ld).ok().as_deref() != Some(target.as_path()) {
            if ld.symlink_metadata().is_ok() {
                mise_util::file::remove_file(&ld)?;
            }
            mise_util::file::make_symlink(&target, &ld)?;
        }
        return Ok(());
    }
    if ld.exists() {
        return Ok(()); // valid symlink or file already in place
    }
    if ld.symlink_metadata().is_ok() {
        mise_util::file::remove_file(&ld)?; // dangling symlink
    }
    let host_loader = [
        "/lib64/ld-linux-x86-64.so.2",
        "/lib/ld-linux-aarch64.so.1",
        "/lib64/ld-linux-aarch64.so.1",
    ]
    .iter()
    .map(Path::new)
    .find(|p| p.exists())
    .map(Path::to_path_buf);
    let Some(target) = host_loader else {
        bail!(
            "no dynamic linker found for {} — Homebrew bottles require glibc (musl-based \
             distros are unsupported)",
            ld.display()
        );
    };
    mise_util::file::make_symlink(&target, &ld)?;
    Ok(())
}

fn writable(path: &Path) -> bool {
    // root can write regardless of owner — and `sudo mise` must never treat
    // a user-owned prefix as broken and chown it to root
    if nix::unistd::geteuid().is_root() {
        return path.exists();
    }
    !matches!(
        path.metadata()
            .map(|m| std::os::unix::fs::MetadataExt::uid(&m)),
        Ok(uid) if uid != nix::unistd::geteuid().as_raw()
    ) && path.exists()
}

/// The owner of an existing, unwritable prefix when it is a non-root user other
/// than the current one. Root-owned prefixes are the normal "needs chown" case.
fn foreign_owner(prefix: &Path) -> Option<String> {
    if writable(prefix) {
        return None;
    }
    let uid = std::os::unix::fs::MetadataExt::uid(&prefix.metadata().ok()?);
    let uid = foreign_uid(nix::unistd::geteuid().as_raw(), uid)?;
    Some(
        nix::unistd::User::from_uid(nix::unistd::Uid::from_raw(uid))
            .ok()
            .flatten()
            .map_or_else(|| format!("uid {uid}"), |u| u.name),
    )
}

/// The prefix owner's uid when it is neither root nor the current user.
fn foreign_uid(euid: u32, owner: u32) -> Option<u32> {
    (euid != 0 && owner != 0 && owner != euid).then_some(owner)
}

/// The invoking user when mise itself was run under sudo (root euid with
/// SUDO_USER set). None for plain root (e.g. a container) and non-root runs.
pub(super) fn sudo_invoking_user() -> Option<String> {
    if nix::unistd::geteuid().is_root()
        && let Ok(sudo_user) = mise_util::env::var("SUDO_USER")
        && !sudo_user.is_empty()
        && sudo_user != "root"
    {
        Some(sudo_user)
    } else {
        None
    }
}

/// Who should own the prefix. Under `sudo mise`, the invoking user
/// (SUDO_USER), not root — mirrors brew's install.sh. Plain root (e.g. a
/// container) owns it as root.
pub(super) fn prefix_owner() -> Option<String> {
    if let Some(user) = sudo_invoking_user() {
        return Some(user);
    }
    // derive the username from the effective uid — $USER can be unset or
    // stale (sudo -u, minimal containers)
    nix::unistd::User::from_uid(nix::unistd::geteuid())
        .ok()
        .flatten()
        .map(|u| u.name)
}

/// Make sure the prefix exists, has the standard layout, and is writable by
/// the current user. May elevate once with sudo (mirrors what brew's own
/// install.sh does). No-op if everything is already in place.
fn bootstrap_dirs(prefix: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = SUBDIRS.iter().map(|d| prefix.join(d)).collect();
    // on Linux the repository (@@HOMEBREW_REPOSITORY@@) lives in a
    // subdirectory rather than being the prefix itself
    let repository = repository();
    if repository != *prefix {
        dirs.push(repository.join("Library"));
    }
    dirs
}

pub(super) fn bootstrap(dry_run: bool) -> Result<()> {
    let prefix = prefix();
    if let Some(owner) = foreign_owner(&prefix) {
        // mise pours bottles itself, so it needs a prefix it can write. An
        // existing prefix owned by another real user (e.g. a dedicated
        // `linuxbrew` user behind a proxy) is that user's boundary — never
        // mkdir/chown over it
        bail!(
            "{} is owned by {owner} and not writable by {}; mise will not change the \
             ownership of another user's Homebrew prefix. Install the missing formulae with \
             that prefix's own brew, or remove them from [bootstrap.packages]",
            prefix.display(),
            prefix_owner().unwrap_or_else(|| "the current user".to_string()),
        );
    }
    let dirs = bootstrap_dirs(&prefix);
    let needs_create = !prefix.exists();
    let needs_chown = !needs_create && !writable(&prefix);
    let missing_subdirs: Vec<PathBuf> = dirs.iter().filter(|p| !p.exists()).cloned().collect();
    if !needs_create && !needs_chown && missing_subdirs.is_empty() {
        return Ok(());
    }
    // try without elevation first — covers prefixes under user-writable
    // parents; the real prefixes need sudo to create. Skipped under `sudo
    // mise`: root could create the dirs, but they must be chowned to the
    // invoking user afterwards
    if needs_create
        && !dry_run
        && sudo_invoking_user().is_none()
        && dirs.iter().try_for_each(std::fs::create_dir_all).is_ok()
    {
        return Ok(());
    }
    if needs_create || needs_chown {
        let Some(user) = prefix_owner() else {
            // never chown to a guessed owner — that can lock the user out
            bail!(
                "cannot determine the current user to own {}",
                prefix.display()
            );
        };
        // brew's install.sh chowns to user:admin on macOS, just the user on
        // Linux (the admin group doesn't exist there)
        let owner = if cfg!(target_os = "macos") {
            format!("{user}:admin")
        } else {
            user
        };
        let mut mkdir_dirs: Vec<String> = vec![prefix.to_string_lossy().to_string()];
        mkdir_dirs.extend(dirs.iter().map(|d| d.display().to_string()));
        let mkdir_args: Vec<String> = ["-p".to_string()].into_iter().chain(mkdir_dirs).collect();
        let chown_args: Vec<String> = vec!["-R".to_string(), owner, prefix.display().to_string()];
        if dry_run {
            miseprintln!("{}", sudo::argv("mkdir", &mkdir_args).join(" "));
            miseprintln!("{}", sudo::argv("chown", &chown_args).join(" "));
            return Ok(());
        }
        info!("creating {} (requires sudo once)", prefix.display());
        sudo::run("mkdir", &mkdir_args, &[])?;
        sudo::run("chown", &chown_args, &[])?;
        if !writable(&prefix) {
            bail!("{} is still not writable after bootstrap", prefix.display());
        }
    } else if dry_run {
        // prefix is ours but subdirs are missing — show what a real run
        // would create (no sudo needed)
        for dir in &missing_subdirs {
            miseprintln!("mkdir -p {}", dir.display());
        }
    } else {
        // prefix is ours, just fill in missing subdirs
        for dir in missing_subdirs {
            mise_util::file::create_dir_all(&dir)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn foreign_owner_ignores_own_and_missing_prefixes() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(foreign_owner(dir.path()), None);
        assert_eq!(foreign_owner(&dir.path().join("missing")), None);
    }

    #[test]
    fn foreign_uid_flags_only_other_non_root_owners() {
        assert_eq!(foreign_uid(1000, 1001), Some(1001));
        assert_eq!(foreign_uid(1000, 0), None);
        assert_eq!(foreign_uid(1000, 1000), None);
        assert_eq!(foreign_uid(0, 1001), None);
    }
}
