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

/// The invoking user's uid (SUDO_UID) under `sudo mise`, None otherwise.
fn sudo_invoking_uid() -> Option<u32> {
    sudo_invoking_user()?;
    mise_util::env::var("SUDO_UID")
        .ok()?
        .parse()
        .ok()
        .filter(|uid| *uid != 0)
}

/// Refuse to work as root in a prefix another account controls. Root can
/// write anywhere, and mise stages, copies, links and removes by path, so the
/// owner of the prefix, of a directory above it or of one of its standard
/// directories (or of a directory a standard directory links to) could swap
/// a directory for a symlink between a check and a write and have root
/// create files wherever they like. Each of those must be owned by root or
/// by the user who ran `sudo mise`, and none of them may be writable by every
/// user without the sticky bit (see [`world_writable_component`]).
///
/// A prefix spelled with `..` is refused for every user, before anything is
/// checked or created: see [`refuse_parent_steps`].
///
/// The sticky bit protects only entries that exist: in `/tmp`, another user
/// can still create a missing `/tmp/brew` as a symlink after this check and
/// before root's `mkdir -p`, which accepts a link to a directory. So
/// [`bootstrap`] runs the same checks again once it has created what was
/// missing, before anything is written into the prefix. What passes then is
/// owned by root (or the `sudo mise` user) in directories nobody else can
/// rename entries in, so it stays put.
pub(super) fn refuse_foreign_as_root() -> Result<()> {
    let prefix = prefix();
    refuse_parent_steps(&prefix)?;
    if !nix::unistd::geteuid().is_root() {
        return Ok(());
    }
    refuse_redirectable(&prefix, &bootstrap_dirs(&prefix), sudo_invoking_uid())
}

/// The checks of [`refuse_foreign_as_root`] for `prefix` and `dirs`, with
/// `trusted_uid` trusted as much as root.
fn refuse_redirectable(prefix: &Path, dirs: &[PathBuf], trusted_uid: Option<u32>) -> Result<()> {
    if let Some((path, uid)) = foreign_component(prefix, dirs, trusted_uid) {
        let owner = nix::unistd::User::from_uid(nix::unistd::Uid::from_raw(uid))
            .ok()
            .flatten()
            .map_or_else(|| format!("uid {uid}"), |u| u.name);
        bail!(
            "{} is owned by {owner}, so mise will not use root (as root or through sudo) to \
             install into or change the Homebrew prefix {}: {owner} could redirect root's \
             writes. Run mise as {owner}, or make root own {}",
            path.display(),
            prefix.display(),
            path.display(),
        );
    }
    if let Some((path, mode)) = world_writable_component(prefix, dirs) {
        bail!(
            "{} is writable by every user (mode {:04o}) and has no sticky bit, so mise will not \
             use root (as root or through sudo) to install into or change the Homebrew prefix \
             {}: any user could rename what is in it and redirect root's writes. Remove write \
             access for others (`chmod o-w {}`) or set the sticky bit",
            path.display(),
            mode & 0o7777,
            prefix.display(),
            path.display(),
        );
    }
    Ok(())
}

/// The first existing path of `guarded_paths` that every user can write to
/// without the sticky bit, and its mode. Anyone could then replace an entry
/// in it with a symlink. With the sticky bit (`/tmp`), only an entry's owner
/// can rename or remove it. A group-writable directory passes: Homebrew's
/// usual layout gives the admin group write access, so the group is trusted
/// as much as the owner. A symlink's own mode means nothing, so links are
/// judged by the directory that holds them, which is also listed.
fn world_writable_component(prefix: &Path, dirs: &[PathBuf]) -> Option<(PathBuf, u32)> {
    use std::os::unix::fs::MetadataExt;
    guarded_paths(prefix, dirs).into_iter().find_map(|path| {
        let meta = path.symlink_metadata().ok()?;
        let mode = meta.mode();
        (!meta.is_symlink() && mode & 0o002 != 0 && mode & 0o1000 == 0).then_some((path, mode))
    })
}

/// Refuse a prefix with a `..` in it. Where `a/missing/../b` leads depends on
/// what `missing` is once it is created, and the checks read it lexically up
/// to the missing part: `mkdir -p` would make `a/missing` and then write in
/// `a/b`, which nothing checked. (`Path::components` drops an inner `.`, and a
/// leading one is the working directory, which is checked.)
fn refuse_parent_steps(prefix: &Path) -> Result<()> {
    if prefix
        .components()
        .any(|part| part == std::path::Component::ParentDir)
    {
        bail!(
            "the Homebrew prefix {} contains `..`; set MISE_SYSTEM_BREW_PREFIX to a path \
             without it",
            prefix.display()
        );
    }
    Ok(())
}

/// The first existing path of `guarded_paths` whose owner is neither root
/// nor `sudo_uid`. Read without following symlinks, so a link counts by its
/// own owner, who can repoint it.
fn foreign_component(
    prefix: &Path,
    dirs: &[PathBuf],
    sudo_uid: Option<u32>,
) -> Option<(PathBuf, u32)> {
    guarded_paths(prefix, dirs).into_iter().find_map(|path| {
        let uid = std::os::unix::fs::MetadataExt::uid(&path.symlink_metadata().ok()?);
        (uid != 0 && Some(uid) != sudo_uid).then_some((path, uid))
    })
}

/// The paths whose owners could redirect root's writes in the prefix: the
/// prefix and the directories above it, and each of `dirs`, all spelled as
/// given and every path met while resolving them (see `resolution_paths`). A
/// standard directory that is a link (a Cellar on another disk) is written
/// through, so the links and directories it leads through count as much as
/// the link itself. A path that does not exist yet is resolved as far as it
/// exists, which a link above a prefix still to be created can lead anywhere
/// from. A relative path (`MISE_SYSTEM_BREW_PREFIX=brew`) is taken from the
/// working directory, joined but not resolved, so the working directory and
/// the directories above it are checked too.
fn guarded_paths(prefix: &Path, dirs: &[PathBuf]) -> Vec<PathBuf> {
    std::iter::once(prefix)
        .chain(dirs.iter().map(PathBuf::as_path))
        .flat_map(|path| {
            let path = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
            path.ancestors()
                .map(Path::to_path_buf)
                .chain(resolution_paths(&path))
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Most links followed while resolving one path, as Linux's MAXSYMLINKS.
const MAX_LINK_HOPS: usize = 40;

/// Every path the kernel looks up while resolving `path`, one link at a
/// time: each directory reached, each link, and each path in the link's
/// target. A chain `Cellar -> /a/bridge -> /b/cellar` lists `/a` and
/// `/a/bridge` as well as `/b/cellar`, since whoever controls `/a` can
/// repoint `bridge`. Every path's parent is listed before it, so each owner
/// that could change where `path` leads is checked. Stops at the first
/// component that does not exist.
fn resolution_paths(path: &Path) -> Vec<PathBuf> {
    // components still to resolve, last first; one component per entry
    let mut rest: Vec<PathBuf> = path
        .components()
        .rev()
        .map(|c| PathBuf::from(c.as_os_str()))
        .collect();
    let mut resolved = PathBuf::new();
    let mut visited = Vec::new();
    let mut hops = 0;
    while let Some(next) = rest.pop() {
        match next.components().next() {
            Some(std::path::Component::Normal(name)) => {
                let here = resolved.join(name);
                let Ok(meta) = here.symlink_metadata() else {
                    break;
                };
                visited.push(here.clone());
                if !meta.is_symlink() {
                    resolved = here;
                    continue;
                }
                hops += 1;
                let Ok(target) = std::fs::read_link(&here) else {
                    break;
                };
                if hops > MAX_LINK_HOPS {
                    break;
                }
                // a relative target resolves from `resolved`, the link's
                // directory; an absolute one starts again at its root
                rest.extend(
                    target
                        .components()
                        .rev()
                        .map(|c| PathBuf::from(c.as_os_str())),
                );
            }
            Some(std::path::Component::ParentDir) => {
                resolved.pop();
            }
            Some(std::path::Component::CurDir) | None => {}
            Some(root) => resolved = PathBuf::from(root.as_os_str()),
        }
    }
    visited
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

/// Each of `paths` and the directories above it that do not exist, which is
/// what `mkdir -p` on them would create, parents first. Only a path the
/// filesystem reports as not found counts: one that cannot be read (no
/// permission to search its parent) may well exist, and this list is what a
/// user is told to remove as root.
fn missing_paths<'a>(paths: impl IntoIterator<Item = &'a PathBuf>) -> Vec<PathBuf> {
    let mut missing = std::collections::BTreeSet::new();
    for path in paths {
        let path = std::path::absolute(path).unwrap_or_else(|_| path.clone());
        for ancestor in path.ancestors() {
            match ancestor.symlink_metadata() {
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                    missing.insert(ancestor.to_path_buf());
                }
                _ => break,
            }
        }
    }
    missing.into_iter().collect()
}

pub(super) fn bootstrap(dry_run: bool) -> Result<()> {
    let prefix = prefix();
    refuse_foreign_as_root()?;
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
        // what was missing could have been created as a link meanwhile
        return refuse_foreign_as_root();
    }
    if needs_create || needs_chown {
        // root is about to `mkdir -p` and `chown -R` here. For a non-root user
        // `refuse_foreign_as_root` checked nothing, so the chain is checked
        // now, before root creates anything: a refusal after the mkdir would
        // leave root-owned directories that every later run refuses again
        let euid = nix::unistd::geteuid();
        let trusted = if euid.is_root() {
            sudo_invoking_uid()
        } else {
            Some(euid.as_raw())
        };
        refuse_redirectable(&prefix, &dirs, trusted)?;
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
        let to_create = missing_paths(std::iter::once(&prefix).chain(&dirs));
        sudo::run("mkdir", &mkdir_args, &[])?;
        // root has just run `mkdir -p` on paths that did not all exist, and
        // is about to `chown -R` the prefix: a link created meanwhile in
        // place of a missing one would send both elsewhere, so the chain is
        // checked again before the chown
        if let Err(err) = refuse_redirectable(&prefix, &dirs, trusted) {
            if to_create.is_empty() {
                return Err(err);
            }
            let list: Vec<String> = to_create.iter().map(|p| p.display().to_string()).collect();
            bail!(
                "{err}\nThis was found after `sudo mkdir -p` ran, and mise did not change the \
                 owner of what it created. These paths did not exist before it ran; check \
                 them and remove what is left as root before trying again: {}",
                list.join(", ")
            );
        }
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
        refuse_foreign_as_root()?;
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
    fn root_accepts_only_a_prefix_owned_by_root_or_the_sudo_user() {
        let me = nix::unistd::geteuid().as_raw();
        if me == 0 {
            // root owns everything here; the check below needs another owner
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().canonicalize().unwrap();
        let prefix = base.join("prefix");
        let dirs = [prefix.join("Cellar")];
        std::fs::create_dir_all(&dirs[0]).unwrap();
        // the temp dir is ours, not root's, and nobody ran sudo
        let (path, uid) = foreign_component(&prefix, &dirs, None).unwrap();
        assert_eq!(uid, me);
        assert!(prefix.starts_with(&path));
        // under `sudo mise` run by the owner it is theirs to change
        assert_eq!(foreign_component(&prefix, &dirs, Some(me)), None);
        // a symlink in the path is judged by its own owner, not its target's
        let link = base.join("link");
        std::os::unix::fs::symlink("/", &link).unwrap();
        let (path, _) = foreign_component(&link.join("usr"), &[], Some(me + 1)).unwrap();
        assert_eq!(path, link);
        // a prefix that does not exist yet is judged by the directories above it
        let (path, _) = foreign_component(&base.join("new/prefix"), &[], None).unwrap();
        assert_eq!(path, base);
        // a missing prefix below root-owned directories is fine
        assert_eq!(
            foreign_component(Path::new("/nonexistent-mise-prefix"), &[], None),
            None
        );
    }

    /// a standard directory that is a link is judged by where it leads too,
    /// so a Cellar link that root owns cannot hand root's writes to the
    /// owner of the directory it points at
    #[test]
    fn root_checks_where_the_standard_directories_lead() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().canonicalize().unwrap();
        let prefix = base.join("prefix");
        let disk = base.join("disk/cellar");
        std::fs::create_dir_all(&prefix).unwrap();
        std::fs::create_dir_all(&disk).unwrap();
        std::os::unix::fs::symlink(&disk, prefix.join("Cellar")).unwrap();
        let paths = guarded_paths(&prefix, &[prefix.join("Cellar")]);
        assert!(paths.contains(&prefix.join("Cellar")));
        assert!(paths.contains(&disk));
        assert!(paths.contains(&base.join("disk")));
    }

    /// a prefix still to be created under a link is judged by where the link
    /// leads too, since creating the prefix writes there
    #[test]
    fn root_checks_where_a_link_above_a_missing_prefix_leads() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().canonicalize().unwrap();
        let target = base.join("foreign/base");
        std::fs::create_dir_all(&target).unwrap();
        let alias = base.join("alias");
        std::os::unix::fs::symlink(&target, &alias).unwrap();
        let prefix = alias.join("new/prefix");
        let paths = guarded_paths(&prefix, &[prefix.join("Cellar")]);
        assert!(paths.contains(&alias));
        assert!(paths.contains(&target));
        assert!(paths.contains(&base.join("foreign")));
    }

    /// a link in the middle of a chain is checked, with the directory that
    /// holds it, even though the chain ends somewhere root owns
    #[test]
    fn root_checks_every_link_in_a_chain() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().canonicalize().unwrap();
        let prefix = base.join("prefix");
        let cellar = base.join("safe/cellar");
        std::fs::create_dir_all(&prefix).unwrap();
        std::fs::create_dir_all(&cellar).unwrap();
        std::fs::create_dir_all(base.join("foreign/sub")).unwrap();
        // Cellar -> foreign/bridge -> ../safe/cellar, the last hop relative
        // and through `..`
        std::os::unix::fs::symlink(base.join("foreign/bridge"), prefix.join("Cellar")).unwrap();
        std::os::unix::fs::symlink("sub/../../safe/cellar", base.join("foreign/bridge")).unwrap();
        let paths = guarded_paths(&prefix, &[prefix.join("Cellar")]);
        for path in [
            prefix.join("Cellar"),
            base.join("foreign"),
            base.join("foreign/bridge"),
            base.join("foreign/sub"),
            base.join("safe"),
            cellar.clone(),
        ] {
            assert!(paths.contains(&path), "{} not in {paths:?}", path.display());
        }
        // a loop ends instead of resolving forever
        std::os::unix::fs::symlink(base.join("b"), base.join("a")).unwrap();
        std::os::unix::fs::symlink(base.join("a"), base.join("b")).unwrap();
        let paths = resolution_paths(&base.join("a/prefix"));
        assert!(paths.contains(&base.join("a")));
        assert!(paths.contains(&base.join("b")));
    }

    /// a relative prefix is written to below the working directory, so that
    /// directory and those above it are checked
    #[test]
    fn root_checks_the_working_directory_of_a_relative_prefix() {
        let cwd = std::env::current_dir().unwrap();
        let paths = guarded_paths(Path::new("brew"), &[PathBuf::from("brew/Cellar")]);
        for path in cwd.ancestors() {
            assert!(paths.contains(&path.to_path_buf()), "{}", path.display());
        }
        assert!(paths.contains(&cwd.join("brew")));
        assert!(paths.contains(&cwd.join("brew/Cellar")));
        assert!(paths.iter().all(|path| path.is_absolute()), "{paths:?}");
    }

    /// `..` makes where the prefix lands depend on directories still to be
    /// created, so it is refused up front
    #[test]
    fn a_prefix_with_a_parent_step_is_refused() {
        for prefix in ["/safe/new/../foreign/prefix", "../brew", "brew/.."] {
            let err = refuse_parent_steps(Path::new(prefix)).unwrap_err();
            assert!(err.to_string().contains("contains `..`"), "{prefix}: {err}");
        }
        for prefix in [
            "/home/linuxbrew/.linuxbrew",
            "/opt/./homebrew",
            "./brew",
            "brew",
        ] {
            assert!(refuse_parent_steps(Path::new(prefix)).is_ok(), "{prefix}");
        }
    }

    /// a directory every user can write to lets any of them swap its entries
    /// for links, unless the sticky bit stops them; the group is trusted
    #[test]
    fn root_refuses_a_world_writable_directory_without_the_sticky_bit() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().canonicalize().unwrap();
        for (mode, refused) in [(0o777, true), (0o1777, false), (0o775, false)] {
            let parent = base.join(format!("{mode:o}"));
            let prefix = parent.join("prefix");
            std::fs::create_dir_all(prefix.join("Cellar")).unwrap();
            std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(mode)).unwrap();
            let found = world_writable_component(&prefix, &[prefix.join("Cellar")]);
            if refused {
                let (path, found_mode) = found.unwrap();
                assert_eq!(path, parent);
                assert_eq!(found_mode & 0o7777, mode);
            } else {
                assert_eq!(found, None, "{mode:o}");
            }
        }
        // a standard directory that is world-writable counts as much
        let prefix = base.join("775/prefix");
        std::fs::set_permissions(
            prefix.join("Cellar"),
            std::fs::Permissions::from_mode(0o777),
        )
        .unwrap();
        let (path, _) = world_writable_component(&prefix, &[prefix.join("Cellar")]).unwrap();
        assert_eq!(path, prefix.join("Cellar"));
    }

    /// a prefix still to be created in a sticky directory passes, but a link
    /// that appears in its place before `mkdir -p` (here to a directory every
    /// user can write to) is caught when the chain is checked again
    #[test]
    fn a_link_created_in_place_of_a_missing_prefix_is_caught_by_the_recheck() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().canonicalize().unwrap();
        let sticky = base.join("tmp");
        std::fs::create_dir(&sticky).unwrap();
        std::fs::set_permissions(&sticky, std::fs::Permissions::from_mode(0o1777)).unwrap();
        let prefix = sticky.join("brew");
        let dirs = [prefix.join("Cellar")];
        let me = Some(nix::unistd::geteuid().as_raw());
        refuse_redirectable(&prefix, &dirs, me).unwrap();

        let planted = sticky.join("attacker");
        std::fs::create_dir_all(planted.join("Cellar")).unwrap();
        std::fs::set_permissions(&planted, std::fs::Permissions::from_mode(0o777)).unwrap();
        std::os::unix::fs::symlink(&planted, &prefix).unwrap();
        let err = refuse_redirectable(&prefix, &dirs, me).unwrap_err();
        assert!(
            err.to_string()
                .contains(&format!("{} is writable by every user", planted.display())),
            "{err}"
        );
    }

    /// a non-root user's setup through sudo is checked before `sudo mkdir -p`
    /// with that user trusted, so a prefix to be created where others could
    /// redirect it is refused while nothing exists yet; `missing_paths` lists
    /// what the mkdir would have created, for the message after a late refusal
    #[test]
    fn a_doomed_sudo_setup_is_refused_before_anything_is_created() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().canonicalize().unwrap();
        let open = base.join("open");
        std::fs::create_dir(&open).unwrap();
        std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o777)).unwrap();
        let prefix = open.join("brew");
        let dirs = vec![prefix.join("Cellar"), prefix.join("Homebrew/Library")];
        let me = Some(nix::unistd::geteuid().as_raw());
        let err = refuse_redirectable(&prefix, &dirs, me).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("is writable by every user"), "{msg}");
        assert!(msg.contains("as root or through sudo"), "{msg}");
        assert!(!prefix.exists());

        assert_eq!(
            missing_paths(std::iter::once(&prefix).chain(&dirs)),
            vec![
                prefix.clone(),
                prefix.join("Cellar"),
                prefix.join("Homebrew"),
                prefix.join("Homebrew/Library"),
            ]
        );
        std::fs::create_dir_all(prefix.join("Cellar")).unwrap();
        assert_eq!(
            missing_paths(std::iter::once(&prefix).chain(&dirs)),
            vec![prefix.join("Homebrew"), prefix.join("Homebrew/Library")]
        );
    }

    /// a path that cannot be read for lack of permission may exist, so it is
    /// not listed as something mkdir created
    #[test]
    fn missing_paths_skips_paths_it_cannot_read() {
        use std::os::unix::fs::PermissionsExt;
        if nix::unistd::geteuid().is_root() {
            // root can read past any permission
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().canonicalize().unwrap();
        let locked = base.join("locked");
        std::fs::create_dir_all(locked.join("brew")).unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
        let prefix = locked.join("brew");
        let dirs = vec![prefix.join("Cellar")];
        let missing = missing_paths(std::iter::once(&prefix).chain(&dirs));
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(missing, Vec::<PathBuf>::new());
    }

    #[test]
    fn foreign_uid_flags_only_other_non_root_owners() {
        assert_eq!(foreign_uid(1000, 1001), Some(1001));
        assert_eq!(foreign_uid(1000, 0), None);
        assert_eq!(foreign_uid(1000, 1000), None);
        assert_eq!(foreign_uid(0, 1001), None);
    }
}
