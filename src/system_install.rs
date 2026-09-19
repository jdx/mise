//! Publish relocatable tool installations without running a backend as root.
use std::fs;
use std::io::{BufRead, BufReader, Read, Seek, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt, symlink};
use std::path::{Component, Path, PathBuf};

use eyre::{Result, bail, ensure};
use serde::{Deserialize, Serialize};

use crate::backend::Backend;
use crate::backend::backend_type::BackendType;
use crate::install_context::InstallContext;
use crate::system::sudo;
use crate::toolset::{ToolVersion, install_state};

const HELPER: &str = "__publish-system-install";

/// The privileged half of a system installation, run as root by
/// `mise __publish-system-install`. Reads one request and its archive from
/// stdin; never loads configuration or backend code.
pub(crate) fn apply_from_stdin() -> Result<()> {
    ensure!(
        sudo::is_root(),
        "the system installation helper requires root"
    );
    nix::sys::stat::umask(nix::sys::stat::Mode::from_bits_truncate(0o022));
    apply(BufReader::new(std::io::stdin().lock()))
}

/// True when mise itself was started through sudo. Plain root (containers, CI)
/// has no `SUDO_UID`.
pub(crate) fn under_sudo() -> bool {
    sudo::is_root() && std::env::var_os("SUDO_UID").is_some()
}

/// Check the closest existing parent without creating a probe file.
pub(crate) fn needs_elevation(path: &Path) -> bool {
    if sudo::is_root() {
        return false;
    }
    path.ancestors()
        .find(|p| p.exists())
        .is_some_and(|p| nix::unistd::access(p, nix::unistd::AccessFlags::W_OK).is_err())
}

#[derive(Serialize, Deserialize)]
enum Request {
    Install {
        directory: PathBuf,
        tool: String,
        version: String,
        manifest: String,
        replace: bool,
    },
    Links {
        directory: PathBuf,
        links: Vec<(String, PathBuf)>,
        remove: Vec<String>,
    },
}

pub(crate) async fn install<B: Backend + ?Sized>(
    backend: &B,
    ctx: InstallContext,
    tv: ToolVersion,
) -> Result<ToolVersion> {
    ensure!(
        matches!(
            backend.get_type(),
            BackendType::Aqua
                | BackendType::Github
                | BackendType::Gitlab
                | BackendType::Forgejo
                | BackendType::Http
                | BackendType::S3
        ),
        "automatic system installation currently supports binary-download backends only; install {} as your user instead",
        tv.short()
    );
    ensure!(
        tv.request.options().get("postinstall").is_none(),
        "automatic system installation cannot relocate a tool with a postinstall hook; install {} as your user instead",
        tv.short()
    );
    sudo::ensure_elevation_available("mise install --system")?;
    let destination = tv.install_path();
    let directory = destination
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| eyre::eyre!("invalid system install destination"))?
        .to_path_buf();
    // Validate before downloading, but the privileged helper checks again.
    validate_directory(&directory, 0)?;
    let stage = tempfile::tempdir()?;
    let mut staged = tv.clone();
    staged.install_path = Some(
        stage
            .path()
            .join(tv.ba().tool_dir_name())
            .join(tv.tv_pathname()),
    );
    let replace = ctx.force;
    let mut installed = backend.install_version(ctx, staged).await?;
    let manifest_path = stage.path().join(".mise-installs.toml");
    install_state::write_backend_meta_to(tv.ba(), &manifest_path)?;
    let request = Request::Install {
        directory,
        tool: tv.ba().tool_dir_name(),
        version: tv.tv_pathname(),
        manifest: fs::read_to_string(manifest_path)?,
        replace,
    };
    let mut input = tempfile::tempfile()?;
    serde_json::to_writer(&mut input, &request)?;
    input.write_all(b"\n")?;
    archive(&installed.install_path(), &mut input)?;
    input.rewind()?;
    publish(input)?;
    installed.install_path = Some(destination.clone());
    install_state::add_tool_version(installed.ba(), &destination, &installed.tv_pathname());
    Ok(installed)
}

/// Publish `links` into `directory` and delete the symlinks named in `remove`.
/// Entries that already match are dropped before deciding whether to elevate.
pub(crate) fn links(
    directory: &Path,
    links: Vec<(String, PathBuf)>,
    remove: Vec<String>,
) -> Result<()> {
    let links = links
        .into_iter()
        .filter(|(name, target)| {
            let path = directory.join(name);
            match fs::symlink_metadata(&path) {
                Ok(metadata) if !metadata.file_type().is_symlink() => {
                    // A concrete install or an unmanaged file occupies the name;
                    // the helper would refuse it, so leave it alone here.
                    warn!("not replacing non-symlink {}", path.display());
                    false
                }
                Ok(_) => fs::read_link(&path).ok().as_ref() != Some(target),
                Err(_) => true,
            }
        })
        .collect::<Vec<_>>();
    let remove = remove
        .into_iter()
        .filter(|name| directory.join(name).is_symlink())
        .collect::<Vec<_>>();
    if links.is_empty() && remove.is_empty() {
        return Ok(());
    }
    let mut input = serde_json::to_vec(&Request::Links {
        directory: directory.to_path_buf(),
        links,
        remove,
    })?;
    input.push(b'\n');
    publish(&input[..])
}

fn publish(input: impl Read) -> Result<()> {
    let executable = std::env::current_exe()?;
    sudo::run_with_reader(
        &executable.to_string_lossy(),
        &[
            "--no-config".to_string(),
            "--no-env".to_string(),
            "--no-hooks".to_string(),
            HELPER.to_string(),
        ],
        input,
    )
}

/// Serialize as the user: root never opens a user-controlled source path.
fn archive(source: &Path, output: impl Write) -> Result<()> {
    let source = &fs::canonicalize(source)?;
    let mut archive = jdx_tar::Builder::new(output);
    for entry in walkdir::WalkDir::new(source)
        .min_depth(1)
        .follow_links(false)
    {
        let entry = entry?;
        let path = entry.path();
        let relative = path.strip_prefix(source)?;
        if entry.file_type().is_symlink() {
            let mut target = fs::read_link(path)?;
            if target.is_absolute() {
                target = fs::canonicalize(&target).unwrap_or(target);
                let within = target.strip_prefix(source).map_err(|_| {
                    eyre::eyre!("cannot relocate external symlink {}", path.display())
                })?;
                target = relative
                    .parent()
                    .unwrap()
                    .components()
                    .map(|_| "..")
                    .collect::<PathBuf>()
                    .join(within);
            }
            safe_link(relative, &target)?;
            let mut header = jdx_tar::Header::new_gnu(jdx_tar::EntryType::Symlink);
            header.set_size(0);
            header.set_mode(0o777);
            archive.append_link(&mut header, relative, &target)?;
        } else if entry.file_type().is_file() {
            let mut file = fs::OpenOptions::new()
                .read(true)
                .custom_flags(nix::libc::O_NOFOLLOW)
                .open(path)?;
            let metadata = file.metadata()?;
            ensure!(metadata.is_file(), "not a regular file: {}", path.display());
            let mut header = jdx_tar::Header::new_gnu(jdx_tar::EntryType::File);
            header.set_size(metadata.len());
            header.set_mode(metadata.mode() & 0o755);
            archive.append_data(&mut header, relative, &mut file)?;
        } else {
            ensure!(
                entry.file_type().is_dir(),
                "unsupported installation entry: {}",
                path.display()
            );
            let mut header = jdx_tar::Header::new_gnu(jdx_tar::EntryType::Directory);
            header.set_mode(0o755);
            archive.append_data(&mut header, relative, std::io::empty())?;
        }
    }
    archive.finish()?;
    Ok(())
}

fn safe_link(path: &Path, target: &Path) -> Result<()> {
    let mut depth = path.parent().unwrap_or(Path::new("")).components().count();
    for component in target.components() {
        match component {
            Component::Normal(_) => depth += 1,
            Component::CurDir => {}
            Component::ParentDir if depth > 0 => depth -= 1,
            _ => bail!(
                "symlink escapes installation: {} -> {}",
                path.display(),
                target.display()
            ),
        }
    }
    Ok(())
}

/// Reject writable/user-owned ancestors, including resolved parent symlinks.
/// This is an accident guard, not a sandbox against another root process.
fn validate_directory(path: &Path, owner: u32) -> Result<()> {
    ensure!(
        path.is_absolute() && path.parent().is_some(),
        "system destination must be an absolute non-root directory"
    );
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(link_metadata) => {
                ensure!(
                    link_metadata.uid() == owner || link_metadata.uid() == 0,
                    "refusing system installation through a user-owned path: {}",
                    ancestor.display()
                );
                let metadata = fs::metadata(ancestor)?;
                ensure!(
                    metadata.is_dir()
                        && (metadata.uid() == owner || metadata.uid() == 0)
                        && (metadata.mode() & 0o022 == 0
                            || metadata.uid() == 0 && metadata.mode() & 0o1000 != 0),
                    "refusing system installation through a non-root-owned or writable directory: {}",
                    ancestor.display()
                );
                let resolved = fs::canonicalize(ancestor)?;
                if resolved != ancestor {
                    for parent in resolved.ancestors() {
                        let metadata = fs::metadata(parent)?;
                        ensure!(
                            (metadata.uid() == owner || metadata.uid() == 0)
                                && (metadata.mode() & 0o022 == 0
                                    || metadata.uid() == 0 && metadata.mode() & 0o1000 != 0),
                            "unsafe system directory: {}",
                            parent.display()
                        );
                    }
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(err.into()),
        }
    }
    Ok(())
}

fn create_directory(path: &Path, owner: u32) -> Result<()> {
    validate_directory(path, owner)?;
    let missing = path
        .ancestors()
        .take_while(|p| !p.exists())
        .collect::<Vec<_>>();
    for directory in missing.into_iter().rev() {
        match fs::create_dir(directory) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(err) => return Err(err.into()),
        }
        validate_directory(directory, owner)?;
    }
    Ok(())
}

fn apply(input: impl BufRead) -> Result<()> {
    apply_for_owner(input, 0)
}

fn apply_for_owner(mut input: impl BufRead, owner: u32) -> Result<()> {
    let mut header = String::new();
    input.read_line(&mut header)?;
    let request: Request = serde_json::from_str(&header)?;
    let directory = match &request {
        Request::Install { directory, .. } | Request::Links { directory, .. } => directory,
    };
    create_directory(directory, owner)?;
    let directory = fs::canonicalize(directory)?;
    let lock_path = directory.join(".mise-publish.lock");
    ensure!(
        !lock_path.is_symlink(),
        "system publication lock must not be a symlink"
    );
    let mut lock = fslock::LockFile::open(&lock_path)?;
    lock.lock()?;
    match request {
        Request::Install {
            tool,
            version,
            manifest,
            replace,
            ..
        } => {
            ensure!(
                crate::file::is_plain_file_name(&tool)
                    && crate::file::is_plain_file_name(&version)
                    && !tool.starts_with('.')
                    && !version.starts_with('.'),
                "invalid tool or version directory"
            );
            let tool_dir = directory.join(&tool);
            create_directory(&tool_dir, owner)?;
            let stage = tempfile::tempdir_in(&tool_dir)?;
            let tree = stage.path().join("new");
            fs::create_dir(&tree)?;
            unpack(input, &tree)?;
            let destination = tool_dir.join(version);
            let exists = fs::symlink_metadata(&destination).is_ok();
            ensure!(
                !exists || replace,
                "system installation already exists: {}",
                destination.display()
            );
            let incoming: toml::Table = toml::from_str(&manifest)?;
            ensure!(
                incoming.len() == 1 && incoming.contains_key(&tool),
                "invalid installation manifest"
            );
            let manifest_path = directory.join(".mise-installs.toml");
            ensure!(
                !manifest_path.is_symlink(),
                "system manifest must not be a symlink"
            );
            let mut merged: toml::Table = match fs::read_to_string(&manifest_path) {
                Ok(text) => toml::from_str(&text)?,
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => toml::Table::new(),
                Err(err) => return Err(err.into()),
            };
            merged.extend(incoming.clone());
            let tool_manifest = tool_dir.join(".mise.backend.toml");
            ensure!(
                !tool_manifest.is_symlink(),
                "tool manifest must not be a symlink"
            );
            let tool_metadata = crate::file::prepare_atomic_write(
                &tool_manifest,
                toml::to_string(&incoming[&tool])?,
            )?;
            let metadata =
                crate::file::prepare_atomic_write(&manifest_path, toml::to_string(&merged)?)?;
            // Read before touching the tree so an error here cannot strand a
            // replaced installation. The per-tool manifest takes precedence over
            // the consolidated one, so a failure between the two commits must put
            // the old tool manifest back alongside the old tree.
            let previous_tool_manifest = match fs::read_to_string(&tool_manifest) {
                Ok(text) => Some(text),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
                Err(err) => return Err(err.into()),
            };
            let backup = stage.path().join("old");
            if exists {
                fs::rename(&destination, &backup)?;
            }
            if let Err(err) = fs::rename(&tree, &destination) {
                if exists && let Err(rollback) = fs::rename(&backup, &destination) {
                    let recovery = stage.keep();
                    bail!(
                        "publication failed: {err}; rollback failed: {rollback}; previous installation retained in {}",
                        recovery.display()
                    );
                }
                return Err(err.into());
            }
            if let Err(err) = tool_metadata.commit().and_then(|()| metadata.commit()) {
                // Keep the previous tool usable when metadata publication fails.
                let rollback = fs::rename(&destination, &tree)
                    .and_then(|()| {
                        if exists {
                            fs::rename(&backup, &destination)
                        } else {
                            Ok(())
                        }
                    })
                    .and_then(|()| match &previous_tool_manifest {
                        Some(text) => fs::write(&tool_manifest, text),
                        None => match fs::remove_file(&tool_manifest) {
                            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
                            result => result,
                        },
                    });
                if let Err(rollback) = rollback {
                    let recovery = stage.keep();
                    bail!(
                        "metadata publication failed: {err}; rollback failed: {rollback}; recovery files retained in {}",
                        recovery.display()
                    );
                }
                return Err(err);
            }
        }
        Request::Links { links, remove, .. } => {
            for name in remove {
                ensure!(crate::file::is_plain_file_name(&name), "invalid link name");
                let path = directory.join(name);
                match fs::symlink_metadata(&path) {
                    Ok(metadata) => {
                        ensure!(
                            metadata.file_type().is_symlink(),
                            "refusing to remove non-symlink {}",
                            path.display()
                        );
                        fs::remove_file(&path)?;
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                    Err(err) => return Err(err.into()),
                }
            }
            for (name, target) in links {
                ensure!(crate::file::is_plain_file_name(&name), "invalid link name");
                if target.is_relative() {
                    safe_link(Path::new(&name), &target)?;
                }
                let destination = directory.join(&name);
                if let Ok(metadata) = fs::symlink_metadata(&destination) {
                    ensure!(
                        metadata.file_type().is_symlink(),
                        "refusing to replace {}",
                        destination.display()
                    );
                    let existing = fs::read_link(&destination)?;
                    if existing == target {
                        continue;
                    }
                    // Runtime links retarget between relative versions; shims
                    // retarget between absolute `mise` executables. A link whose
                    // target has a different shape or name was not made by mise.
                    ensure!(
                        existing.is_absolute() == target.is_absolute()
                            && (target.is_relative() || existing.file_name() == target.file_name()),
                        "refusing to replace unrelated link {}",
                        destination.display()
                    );
                }
                let stage = tempfile::tempdir_in(&directory)?;
                let link = stage.path().join("link");
                symlink(target, &link)?;
                fs::rename(link, destination)?;
            }
        }
    }
    Ok(())
}

fn unpack(input: impl Read, directory: &Path) -> Result<()> {
    let mut archive = jdx_tar::Archive::new(input);
    let mut links = Vec::new();
    let mut files = 0;
    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.into_owned();
        ensure!(
            !path.as_os_str().is_empty()
                && path.components().all(|p| matches!(p, Component::Normal(_))),
            "invalid archive path"
        );
        let destination = directory.join(&path);
        fs::create_dir_all(destination.parent().unwrap())?;
        let kind = entry.entry_type();
        if kind == jdx_tar::EntryType::Symlink {
            let target = entry
                .header()
                .link_name()
                .ok_or_else(|| eyre::eyre!("missing link target"))?
                .into_owned();
            safe_link(&path, &target)?;
            links.push((destination, target));
        } else if kind == jdx_tar::EntryType::Directory {
            fs::create_dir_all(&destination)?;
        } else {
            ensure!(
                kind == jdx_tar::EntryType::File,
                "unsupported archive entry"
            );
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&destination)?;
            std::io::copy(&mut entry, &mut file)?;
            files += 1;
            file.set_permissions(fs::Permissions::from_mode(entry.header().mode() & 0o755))?;
        }
    }
    ensure!(files > 0, "empty system installation archive");
    // Install symlinks last, so archive entries can never write through one.
    for (path, target) in links {
        symlink(target, path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archive_boundary() -> Result<()> {
        assert!(safe_link(Path::new("bin/tool"), Path::new("../lib/tool")).is_ok());
        assert!(safe_link(Path::new("tool"), Path::new("../outside")).is_err());
        assert!(safe_link(Path::new("bin/tool"), Path::new("/etc/passwd")).is_err());
        let source = tempfile::tempdir()?;
        fs::write(source.path().join("tool"), "binary")?;
        fs::set_permissions(
            source.path().join("tool"),
            fs::Permissions::from_mode(0o6755),
        )?;
        fs::create_dir(source.path().join("empty"))?;
        symlink(source.path().join("tool"), source.path().join("alias"))?;
        let mut bytes = Vec::new();
        archive(source.path(), &mut bytes)?;
        let output = tempfile::tempdir()?;
        unpack(&bytes[..], output.path())?;
        assert_eq!(fs::read_to_string(output.path().join("alias"))?, "binary");
        assert_eq!(
            fs::read_link(output.path().join("alias"))?,
            Path::new("tool")
        );
        assert!(output.path().join("empty").is_dir());
        assert_eq!(
            fs::metadata(output.path().join("tool"))?.mode() & 0o7777,
            0o755
        );
        let destination = tempfile::tempdir()?;
        let root = destination.path().join("installs");
        let request = Request::Install {
            directory: root.clone(),
            tool: "uv".into(),
            version: "1".into(),
            manifest: "[uv]\nshort = 'uv'\nfull = 'aqua:astral-sh/uv'\n".into(),
            replace: false,
        };
        let mut input = serde_json::to_vec(&request)?;
        input.push(b'\n');
        input.extend(&bytes);
        let owner = nix::unistd::geteuid().as_raw();
        apply_for_owner(&input[..], owner)?;
        assert_eq!(fs::read_to_string(root.join("uv/1/alias"))?, "binary");
        assert!(root.join(".mise-installs.toml").is_file());
        assert!(root.join("uv/.mise.backend.toml").is_file());
        assert!(apply_for_owner(&input[..], owner).is_err());
        assert_eq!(fs::read_to_string(root.join("uv/1/tool"))?, "binary");
        let reserved = Request::Install {
            directory: root.clone(),
            tool: "uv".into(),
            version: ".mise.backend.toml".into(),
            manifest: "[uv]\nshort = 'uv'\n".into(),
            replace: true,
        };
        let mut reserved = serde_json::to_vec(&reserved)?;
        reserved.push(b'\n');
        reserved.extend(&bytes);
        assert!(apply_for_owner(&reserved[..], owner).is_err());
        assert!(root.join("uv/.mise.backend.toml").is_file());

        // A malformed replacement must leave the previous installation intact.
        let request = Request::Install {
            directory: root.clone(),
            tool: "uv".into(),
            version: "1".into(),
            manifest: "[uv]\nshort = 'uv'\n".into(),
            replace: true,
        };
        let mut malicious = serde_json::to_vec(&request)?;
        malicious.push(b'\n');
        let mut archive = jdx_tar::Builder::new(&mut malicious);
        let mut header = jdx_tar::Header::new_gnu(jdx_tar::EntryType::Symlink);
        archive.append_link(&mut header, "escape", "../../outside")?;
        archive.finish()?;
        assert!(apply_for_owner(&malicious[..], owner).is_err());
        assert_eq!(fs::read_to_string(root.join("uv/1/tool"))?, "binary");
        let links_request = |links: Vec<(&str, &str)>, remove: Vec<&str>| -> Result<Vec<u8>> {
            let mut bytes = serde_json::to_vec(&Request::Links {
                directory: root.join("uv"),
                links: links
                    .into_iter()
                    .map(|(name, target)| (name.to_string(), PathBuf::from(target)))
                    .collect(),
                remove: remove.into_iter().map(str::to_string).collect(),
            })?;
            bytes.push(b'\n');
            Ok(bytes)
        };
        let links = links_request(vec![("latest", "./1")], vec![])?;
        apply_for_owner(&links[..], owner)?;
        apply_for_owner(&links[..], owner)?;
        assert_eq!(fs::read_to_string(root.join("uv/latest/tool"))?, "binary");
        // Relative targets may not escape the directory.
        let escape = links_request(vec![("evil", "../../outside")], vec![])?;
        assert!(apply_for_owner(&escape[..], owner).is_err());
        assert!(fs::symlink_metadata(root.join("uv/evil")).is_err());
        // Shims retarget between `mise` executables but not to unrelated links.
        let old_mise = source.path().join("mise");
        let new_mise = source.path().join("bin").join("mise");
        fs::create_dir(source.path().join("bin"))?;
        fs::write(&old_mise, "old")?;
        fs::write(&new_mise, "new")?;
        let shim = links_request(vec![("uv", old_mise.to_str().unwrap())], vec![])?;
        apply_for_owner(&shim[..], owner)?;
        let shim = links_request(vec![("uv", new_mise.to_str().unwrap())], vec![])?;
        apply_for_owner(&shim[..], owner)?;
        assert_eq!(fs::read_link(root.join("uv/uv"))?, new_mise);
        let unrelated = links_request(
            vec![("uv", source.path().join("tool").to_str().unwrap())],
            vec![],
        )?;
        assert!(apply_for_owner(&unrelated[..], owner).is_err());
        assert_eq!(fs::read_link(root.join("uv/uv"))?, new_mise);
        // Removal only deletes symlinks, and tolerates names that are gone.
        let remove = links_request(vec![], vec!["latest", "uv", "missing"])?;
        apply_for_owner(&remove[..], owner)?;
        assert!(fs::symlink_metadata(root.join("uv/latest")).is_err());
        assert!(fs::symlink_metadata(root.join("uv/uv")).is_err());
        let remove_dir = links_request(vec![], vec!["1"])?;
        assert!(apply_for_owner(&remove_dir[..], owner).is_err());
        assert!(root.join("uv/1").is_dir());
        fs::set_permissions(&root, fs::Permissions::from_mode(0o777))?;
        assert!(validate_directory(&root, owner).is_err());
        Ok(())
    }
}
