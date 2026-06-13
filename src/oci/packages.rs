//! Native `[bootstrap.packages]` support for OCI builds.
//!
//! This intentionally does not use a container engine. For apt-based base
//! images, mise unpacks the pulled base image into a temporary rootfs, asks
//! host `apt-get`/`dpkg` to install into that rootfs, then emits the filesystem
//! changes as one OCI layer.

use std::collections::BTreeMap;
use std::fs;
use std::io::{BufRead, BufReader, Read};
#[cfg(unix)]
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use eyre::{Context, Result, bail};
use flate2::read::GzDecoder;
use tempfile::TempDir;
use walkdir::WalkDir;

use crate::file;
use crate::oci::layer::{self, LayerBlob};
use crate::oci::layout::ImageLayout;
use crate::oci::manifest::Descriptor;
use crate::system::ManagerPackages;
use crate::system::packages::PackageRequest;

#[derive(Debug, Clone)]
struct FsEntry {
    kind: FsEntryKind,
    mode: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum FsEntryKind {
    Dir,
    File { size: u64, hash: [u8; 32] },
    Symlink { target: PathBuf },
    Other,
}

pub fn build_system_packages_layer(
    layout: &ImageLayout,
    base_layers: &[Descriptor],
    managers: &[ManagerPackages],
    architecture: &str,
) -> Result<Option<LayerBlob>> {
    let apt_requests = collect_apt_requests(managers)?;
    if apt_requests.is_empty() {
        return Ok(None);
    }
    if base_layers.is_empty() {
        bail!(
            "mise oci requires an apt-based base image when [bootstrap.packages] is configured; \
             `scratch` has no apt metadata"
        );
    }
    if file::which("apt-get").is_none() {
        bail!("mise oci needs `apt-get` on PATH to install apt system packages into the image");
    }
    if file::which("dpkg").is_none() {
        bail!("mise oci needs `dpkg` on PATH to install apt system packages into the image");
    }

    let td = TempDir::with_prefix("mise-oci-apt-rootfs-")
        .wrap_err("creating temp rootfs for apt system packages")?;
    let rootfs = td.path().join("rootfs");
    file::create_dir_all(&rootfs)?;
    unpack_base_layers(layout, base_layers, &rootfs)?;
    if !rootfs.join("etc/apt").is_dir() {
        bail!(
            "mise oci found apt packages in [bootstrap.packages], but the base image does not \
             contain /etc/apt. Use a Debian/Ubuntu base image or remove the apt entries."
        );
    }
    prepare_apt_rootfs(&rootfs)?;

    let before = snapshot(&rootfs)?;
    apt_install_into_rootfs(&rootfs, &apt_requests, architecture)?;
    clean_apt_transients(&rootfs)?;
    let diff_dir = td.path().join("diff");
    materialize_diff(&rootfs, &before, &diff_dir)?;
    if WalkDir::new(&diff_dir).into_iter().count() <= 1 {
        info!("oci: apt system packages produced no filesystem changes");
        return Ok(None);
    }
    info!(
        "oci: adding apt system packages: {}",
        apt_requests
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    );
    layer::build_layer_from_dir_preserve_metadata(&diff_dir, "").map(Some)
}

fn collect_apt_requests(managers: &[ManagerPackages]) -> Result<Vec<PackageRequest>> {
    let mut out = vec![];
    let mut unsupported = vec![];
    for mgr in managers {
        if mgr.disabled || mgr.requests.is_empty() {
            continue;
        }
        match mgr.manager.name() {
            "apt" => out.extend(mgr.requests.clone()),
            other => unsupported.push(other.to_string()),
        }
    }
    if !unsupported.is_empty() {
        unsupported.sort();
        unsupported.dedup();
        bail!(
            "mise oci currently supports only apt entries in [bootstrap.packages]; unsupported \
             manager(s): {}",
            unsupported.join(", ")
        );
    }
    Ok(out)
}

fn unpack_base_layers(layout: &ImageLayout, layers: &[Descriptor], rootfs: &Path) -> Result<()> {
    for layer in layers {
        let path = layout.blob_path(&layer.digest);
        let reader = open_layer_reader(layer, &path)?;
        let mut archive = tar::Archive::new(reader);
        for entry in archive
            .entries()
            .wrap_err_with(|| format!("reading base layer {}", layer.digest))?
        {
            let mut entry =
                entry.wrap_err_with(|| format!("reading base layer entry {}", layer.digest))?;
            let rel = clean_layer_path(&entry.path()?)
                .wrap_err_with(|| format!("reading base layer entry path {}", layer.digest))?;
            if rel.as_os_str().is_empty() || apply_oci_whiteout(rootfs, &rel)? {
                continue;
            }
            entry
                .unpack_in(rootfs)
                .wrap_err_with(|| format!("unpacking base layer {}", layer.digest))?;
        }
    }
    Ok(())
}

fn open_layer_reader(layer: &Descriptor, path: &Path) -> Result<Box<dyn Read>> {
    let file =
        fs::File::open(path).wrap_err_with(|| format!("opening base layer {}", path.display()))?;
    let mut reader = BufReader::new(file);
    Ok(match layer_compression(layer, &mut reader)? {
        LayerCompression::Gzip => Box::new(GzDecoder::new(reader)),
        LayerCompression::Zstd => Box::new(zstd::stream::read::Decoder::new(reader)?),
        LayerCompression::Tar => Box::new(reader),
    })
}

enum LayerCompression {
    Gzip,
    Zstd,
    Tar,
}

fn layer_compression<R: BufRead>(layer: &Descriptor, reader: &mut R) -> Result<LayerCompression> {
    if layer.media_type.ends_with("+gzip")
        || layer.media_type.ends_with(".gzip")
        || layer.media_type.ends_with(".tar.gzip")
    {
        return Ok(LayerCompression::Gzip);
    }
    if layer.media_type.ends_with("+zstd") || layer.media_type.ends_with(".zstd") {
        return Ok(LayerCompression::Zstd);
    }
    if layer.media_type.ends_with(".tar") || layer.media_type.ends_with("+tar") {
        return Ok(LayerCompression::Tar);
    }

    let magic = reader.fill_buf()?;
    if magic.starts_with(&[0x1f, 0x8b]) {
        Ok(LayerCompression::Gzip)
    } else if magic.starts_with(&[0x28, 0xb5, 0x2f, 0xfd]) {
        Ok(LayerCompression::Zstd)
    } else {
        Ok(LayerCompression::Tar)
    }
}

fn clean_layer_path(path: &Path) -> Result<PathBuf> {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir | Component::RootDir => {}
            Component::Normal(part) => out.push(part),
            Component::ParentDir | Component::Prefix(_) => {
                bail!("invalid OCI layer path {}", path.display())
            }
        }
    }
    Ok(out)
}

fn apply_oci_whiteout(rootfs: &Path, rel: &Path) -> Result<bool> {
    let Some(name) = rel.file_name().and_then(|n| n.to_str()) else {
        return Ok(false);
    };
    let parent = rel.parent().unwrap_or_else(|| Path::new(""));
    if name == ".wh..wh..opq" {
        let dir = rootfs.join(parent);
        if dir.is_dir() {
            for entry in fs::read_dir(&dir)? {
                remove_path(&entry?.path())?;
            }
        }
        return Ok(true);
    }
    let Some(target) = name.strip_prefix(".wh.") else {
        return Ok(false);
    };
    remove_path(&rootfs.join(parent).join(target))?;
    Ok(true)
}

fn remove_path(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path).map(|m| m.file_type()) {
        Ok(ft) if ft.is_dir() && !ft.is_symlink() => fs::remove_dir_all(path)
            .wrap_err_with(|| format!("removing directory {}", path.display()))?,
        Ok(_) => fs::remove_file(path).wrap_err_with(|| format!("removing {}", path.display()))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => {
            return Err(e).wrap_err_with(|| format!("reading metadata for {}", path.display()));
        }
    }
    Ok(())
}

fn apt_install_into_rootfs(
    rootfs: &Path,
    requests: &[PackageRequest],
    architecture: &str,
) -> Result<()> {
    let status = rootfs.join("var/lib/dpkg/status");
    if let Some(parent) = status.parent() {
        file::create_dir_all(parent)?;
    }
    if !status.exists() {
        file::write(&status, "")?;
    }
    file::create_dir_all(rootfs.join("var/cache/apt/archives/partial"))?;
    file::create_dir_all(rootfs.join("var/lib/apt/lists/partial"))?;

    let update_args = apt_root_args(rootfs, &status, architecture)?
        .into_iter()
        .chain(["update".to_string()])
        .collect();
    run_apt_get(update_args)?;

    let install_args = vec![
        "install".to_string(),
        "-y".to_string(),
        "--no-install-recommends".to_string(),
        "--".to_string(),
    ];
    let package_args = requests.iter().map(|p| match &p.version {
        Some(v) => format!("{}={v}", p.name),
        None => p.name.clone(),
    });
    let args = apt_root_args(rootfs, &status, architecture)?
        .into_iter()
        .chain(install_args)
        .chain(package_args)
        .collect();
    run_apt_get(args)
}

fn prepare_apt_rootfs(rootfs: &Path) -> Result<()> {
    let apt_dir = rootfs.join("etc/apt");
    for entry in WalkDir::new(&apt_dir)
        .follow_links(false)
        .sort_by_file_name()
    {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let bytes = file::read(path)?;
        let Ok(mut s) = String::from_utf8(bytes) else {
            continue;
        };
        let original = s.clone();
        let root = rootfs.to_string_lossy();
        for p in [
            "/usr/share/keyrings/",
            "/etc/apt/",
            "/var/cache/apt",
            "/var/lib/apt",
            "/var/log/apt",
        ] {
            s = s.replace(p, &format!("{root}{p}"));
        }
        if s != original {
            file::write(path, s)?;
        }
    }
    Ok(())
}

fn apt_root_args(rootfs: &Path, status: &Path, architecture: &str) -> Result<Vec<String>> {
    Ok(vec![
        "-o".to_string(),
        format!("Dir={}", rootfs.display()),
        "-o".to_string(),
        "Dir::Etc=etc/apt".to_string(),
        "-o".to_string(),
        "Dir::Etc::sourcelist=sources.list".to_string(),
        "-o".to_string(),
        "Dir::Etc::sourceparts=sources.list.d".to_string(),
        "-o".to_string(),
        "Dir::Etc::trusted=trusted.gpg".to_string(),
        "-o".to_string(),
        "Dir::Etc::trustedparts=trusted.gpg.d".to_string(),
        "-o".to_string(),
        "Dir::State=var/lib/apt".to_string(),
        "-o".to_string(),
        format!("Dir::State::status={}", status.display()),
        "-o".to_string(),
        "Dir::Cache=var/cache/apt".to_string(),
        "-o".to_string(),
        "Dir::Log=var/log/apt".to_string(),
        "-o".to_string(),
        "APT::Sandbox::User=root".to_string(),
        "-o".to_string(),
        format!("APT::Architecture={}", apt_architecture(architecture)?),
        "-o".to_string(),
        format!("DPkg::Options::=--root={}", rootfs.display()),
        "-o".to_string(),
        "DPkg::Options::=--force-not-root".to_string(),
    ])
}

fn run_apt_get(args: Vec<String>) -> Result<()> {
    info!("apt-get {}", args.join(" "));
    let output = Command::new("apt-get")
        .args(&args)
        .env("DEBIAN_FRONTEND", "noninteractive")
        .output()
        .wrap_err("running apt-get for OCI system packages")?;
    if !output.status.success() {
        bail!(
            "apt-get failed while installing OCI system packages: {}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

fn clean_apt_transients(rootfs: &Path) -> Result<()> {
    remove_dir_children(&rootfs.join("var/cache/apt/archives"))?;
    remove_path(&rootfs.join("var/cache/apt/pkgcache.bin"))?;
    remove_path(&rootfs.join("var/cache/apt/srcpkgcache.bin"))?;

    remove_dir_children(&rootfs.join("var/lib/apt/lists"))?;

    remove_dir_children(&rootfs.join("var/log/apt"))?;
    Ok(())
}

fn remove_dir_children(path: &Path) -> Result<()> {
    match fs::read_dir(path) {
        Ok(entries) => {
            for entry in entries {
                remove_path(&entry?.path())?;
            }
            Ok(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e).wrap_err_with(|| format!("reading directory {}", path.display())),
    }
}

fn apt_architecture(oci_arch: &str) -> Result<&'static str> {
    match oci_arch {
        "amd64" | "x86_64" => Ok("amd64"),
        "arm64" | "aarch64" => Ok("arm64"),
        other => bail!("apt system packages are not supported for OCI architecture {other:?}"),
    }
}

fn snapshot(root: &Path) -> Result<BTreeMap<PathBuf, FsEntry>> {
    let mut out = BTreeMap::new();
    for entry in WalkDir::new(root).follow_links(false).sort_by_file_name() {
        let entry = entry?;
        let rel = entry.path().strip_prefix(root)?;
        if rel.as_os_str().is_empty() {
            continue;
        }
        out.insert(rel.to_path_buf(), fs_entry(entry.path())?);
    }
    Ok(out)
}

fn fs_entry(path: &Path) -> Result<FsEntry> {
    let md = fs::symlink_metadata(path)?;
    let mode = mode(&md);
    let ft = md.file_type();
    let kind = if ft.is_dir() {
        FsEntryKind::Dir
    } else if ft.is_symlink() {
        FsEntryKind::Symlink {
            target: fs::read_link(path)?,
        }
    } else if ft.is_file() {
        FsEntryKind::File {
            size: md.len(),
            hash: file_hash(path)?,
        }
    } else {
        FsEntryKind::Other
    };
    Ok(FsEntry { kind, mode })
}

#[cfg(unix)]
fn mode(md: &fs::Metadata) -> u32 {
    md.permissions().mode() & 0o7777
}

#[cfg(not(unix))]
fn mode(_md: &fs::Metadata) -> u32 {
    0o644
}

fn file_hash(path: &Path) -> Result<[u8; 32]> {
    use sha2::{Digest, Sha256};
    let mut f = fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().into())
}

fn materialize_diff(
    rootfs: &Path,
    before: &BTreeMap<PathBuf, FsEntry>,
    diff_dir: &Path,
) -> Result<()> {
    file::create_dir_all(diff_dir)?;
    let after = snapshot(rootfs)?;

    let mut whiteouted_dirs: Vec<PathBuf> = vec![];
    for (rel, old) in before {
        if after.contains_key(rel) {
            continue;
        }
        if whiteouted_dirs.iter().any(|dir| rel.starts_with(dir)) {
            continue;
        }
        write_whiteout(diff_dir, rel)?;
        if matches!(old.kind, FsEntryKind::Dir) {
            whiteouted_dirs.push(rel.clone());
        }
    }

    for (rel, entry) in after {
        if let Some(old) = before.get(&rel) {
            if same_entry(old, &entry) {
                continue;
            }
            if !same_entry_type(old, &entry) {
                write_whiteout(diff_dir, &rel)?;
            }
        }
        copy_entry(rootfs, diff_dir, &rel, &entry)?;
    }
    Ok(())
}

fn same_entry(a: &FsEntry, b: &FsEntry) -> bool {
    a.mode == b.mode && a.kind == b.kind
}

fn same_entry_type(a: &FsEntry, b: &FsEntry) -> bool {
    matches!(
        (&a.kind, &b.kind),
        (FsEntryKind::Dir, FsEntryKind::Dir)
            | (FsEntryKind::File { .. }, FsEntryKind::File { .. })
            | (FsEntryKind::Symlink { .. }, FsEntryKind::Symlink { .. })
            | (FsEntryKind::Other, FsEntryKind::Other)
    )
}

fn write_whiteout(diff_dir: &Path, rel: &Path) -> Result<()> {
    let Some(name) = rel.file_name() else {
        return Ok(());
    };
    let parent = rel.parent().unwrap_or_else(|| Path::new(""));
    let whiteout = diff_dir
        .join(parent)
        .join(format!(".wh.{}", name.to_string_lossy()));
    if let Some(parent) = whiteout.parent() {
        file::create_dir_all(parent)?;
    }
    file::write(whiteout, "")?;
    Ok(())
}

fn copy_entry(rootfs: &Path, diff_dir: &Path, rel: &Path, entry: &FsEntry) -> Result<()> {
    let src = rootfs.join(rel);
    let dst = diff_dir.join(rel);
    if let Some(parent) = dst.parent() {
        file::create_dir_all(parent)?;
    }
    match &entry.kind {
        FsEntryKind::Dir => {
            file::create_dir_all(&dst)?;
            set_mode(&dst, entry.mode)?;
        }
        FsEntryKind::File { .. } => {
            file::copy(&src, &dst)?;
            set_mode(&dst, entry.mode)?;
        }
        FsEntryKind::Symlink { target } => {
            #[cfg(unix)]
            symlink(target, &dst)?;
            #[cfg(not(unix))]
            {
                let _ = target;
                bail!("OCI apt package layers with symlinks require a unix host");
            }
        }
        FsEntryKind::Other => {
            warn!(
                "oci: skipping unsupported filesystem entry {}",
                src.display()
            );
        }
    }
    Ok(())
}

fn set_mode(path: &Path, mode: u32) -> Result<()> {
    #[cfg(unix)]
    fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
    #[cfg(not(unix))]
    {
        let _ = (path, mode);
    }
    Ok(())
}
