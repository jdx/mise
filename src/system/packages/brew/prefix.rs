//! The Homebrew prefix (/opt/homebrew): detection, ownership, bootstrap.

use std::path::{Path, PathBuf};

use eyre::bail;

use crate::result::Result;
use crate::system::sudo;

pub const PREFIX: &str = "/opt/homebrew";

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
        _ => PathBuf::from(PREFIX),
    }
}

pub fn cellar() -> PathBuf {
    prefix().join("Cellar")
}

/// Is a real Homebrew installation managing this prefix? (a brew elsewhere
/// on PATH with a different prefix doesn't count — it owns its own prefix)
pub fn real_brew_installed() -> bool {
    let prefix = prefix();
    prefix.join("bin/brew").exists()
        || prefix.join(".git").exists()
        || prefix.join("Homebrew").exists()
}

fn writable(path: &Path) -> bool {
    !matches!(
        path.metadata()
            .map(|m| std::os::unix::fs::MetadataExt::uid(&m)),
        Ok(uid) if uid != nix::unistd::geteuid().as_raw()
    ) && path.exists()
}

/// Make sure /opt/homebrew exists, has the standard layout, and is writable
/// by the current user. May elevate once with sudo (mirrors what brew's own
/// install.sh does). No-op if everything is already in place.
pub fn bootstrap(dry_run: bool) -> Result<()> {
    let prefix = prefix();
    let needs_create = !prefix.exists();
    let needs_chown = !needs_create && !writable(&prefix);
    let missing_subdirs: Vec<PathBuf> = SUBDIRS
        .iter()
        .map(|d| prefix.join(d))
        .filter(|p| !p.exists())
        .collect();
    if !needs_create && !needs_chown && missing_subdirs.is_empty() {
        return Ok(());
    }
    // try without elevation first — covers prefixes under user-writable
    // parents; the real /opt/homebrew needs sudo to create
    if needs_create
        && !dry_run
        && SUBDIRS
            .iter()
            .try_for_each(|d| std::fs::create_dir_all(prefix.join(d)))
            .is_ok()
    {
        return Ok(());
    }
    if needs_create || needs_chown {
        let user = crate::env::var("USER").unwrap_or_else(|_| "root".into());
        let mut dirs: Vec<String> = vec![prefix.to_string_lossy().to_string()];
        dirs.extend(SUBDIRS.iter().map(|d| prefix.join(d).display().to_string()));
        let mkdir_args: Vec<String> = ["-p".to_string()].into_iter().chain(dirs.clone()).collect();
        let chown_args: Vec<String> = vec![
            "-R".to_string(),
            format!("{user}:admin"),
            prefix.display().to_string(),
        ];
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
