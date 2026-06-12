//! The Homebrew prefix: /opt/homebrew (arm64 macOS) or
//! /home/linuxbrew/.linuxbrew (Linux) — detection, ownership, bootstrap.

use std::path::{Path, PathBuf};

use eyre::bail;

use crate::result::Result;
use crate::system::sudo;

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

pub fn prefix() -> PathBuf {
    // undocumented override for testing the pour pipeline without touching
    // the real prefix
    match crate::env::var("MISE_SYSTEM_BREW_PREFIX") {
        Ok(p) if !p.is_empty() => PathBuf::from(p),
        _ if cfg!(target_os = "macos") => PathBuf::from("/opt/homebrew"),
        _ => PathBuf::from("/home/linuxbrew/.linuxbrew"),
    }
}

pub fn cellar() -> PathBuf {
    prefix().join("Cellar")
}

/// where brew would keep its own repository — referenced by the
/// @@HOMEBREW_REPOSITORY@@ placeholder (== prefix on arm64 macOS, a
/// subdirectory on Linux)
pub fn repository() -> PathBuf {
    if cfg!(target_os = "macos") {
        prefix()
    } else {
        prefix().join("Homebrew")
    }
}

/// Linux bottles are built with their ELF interpreter set to
/// `<prefix>/lib/ld.so`; brew points that symlink at a brewed glibc when one
/// is installed, otherwise at the host's dynamic linker. Mirror that here.
pub fn setup_linux_runtime() -> Result<()> {
    if !cfg!(target_os = "linux") {
        return Ok(());
    }
    let lib = prefix().join("lib");
    crate::file::create_dir_all(&lib)?;
    let ld = lib.join("ld.so");
    if ld.exists() {
        return Ok(()); // valid symlink or file already in place
    }
    if ld.symlink_metadata().is_ok() {
        crate::file::remove_file(&ld)?; // dangling symlink
    }
    // a brewed glibc keg takes precedence (hosts older than the bottles'
    // build glibc); otherwise the host loader
    let brewed_glibc = crate::file::ls(&cellar().join("glibc"))
        .unwrap_or_default()
        .into_iter()
        .filter_map(|keg| {
            let candidate = keg.join("lib/ld-linux-x86_64.so.2");
            candidate.exists().then_some(candidate)
        })
        .next_back();
    let host_loader = [
        "/lib64/ld-linux-x86-64.so.2",
        "/lib/ld-linux-aarch64.so.1",
        "/lib64/ld-linux-aarch64.so.1",
    ]
    .iter()
    .map(Path::new)
    .find(|p| p.exists())
    .map(Path::to_path_buf);
    let Some(target) = brewed_glibc.or(host_loader) else {
        bail!(
            "no dynamic linker found for {} — Homebrew bottles require glibc (musl-based \
             distros are unsupported)",
            ld.display()
        );
    };
    crate::file::make_symlink(&target, &ld)?;
    Ok(())
}

fn writable(path: &Path) -> bool {
    !matches!(
        path.metadata()
            .map(|m| std::os::unix::fs::MetadataExt::uid(&m)),
        Ok(uid) if uid != nix::unistd::geteuid().as_raw()
    ) && path.exists()
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

pub fn bootstrap(dry_run: bool) -> Result<()> {
    let prefix = prefix();
    let dirs = bootstrap_dirs(&prefix);
    let needs_create = !prefix.exists();
    let needs_chown = !needs_create && !writable(&prefix);
    let missing_subdirs: Vec<PathBuf> = dirs.iter().filter(|p| !p.exists()).cloned().collect();
    if !needs_create && !needs_chown && missing_subdirs.is_empty() {
        return Ok(());
    }
    // try without elevation first — covers prefixes under user-writable
    // parents; the real prefixes need sudo to create
    if needs_create && !dry_run && dirs.iter().try_for_each(std::fs::create_dir_all).is_ok() {
        return Ok(());
    }
    if needs_create || needs_chown {
        // derive the username from the effective uid — $USER can be unset or
        // stale (sudo -u, minimal containers)
        let Some(user) = nix::unistd::User::from_uid(nix::unistd::geteuid())
            .ok()
            .flatten()
            .map(|u| u.name)
        else {
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
        let mut dirs: Vec<String> = vec![prefix.to_string_lossy().to_string()];
        dirs.extend(SUBDIRS.iter().map(|d| prefix.join(d).display().to_string()));
        let mkdir_args: Vec<String> = ["-p".to_string()].into_iter().chain(dirs.clone()).collect();
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
    } else if !dry_run {
        // prefix is ours, just fill in missing subdirs
        for dir in missing_subdirs {
            crate::file::create_dir_all(&dir)?;
        }
    }
    Ok(())
}
