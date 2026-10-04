//! Where does a tool version live under the identity layout?
//!
//! This is the bridge between a [`ToolVersion`] and the catalog. It computes the
//! identity a tool version answers, finds the installation that satisfies it
//! (without writing anything), allocates one when installing, and keeps the
//! compatibility links, receipt and unlocked selection in step.
//!
//! The layout is gated behind `experimental`. With it off nothing here is
//! consulted and every path is the legacy `installs/<short>/<version>`.
//! Legacy installations stay where they are and keep working: a version that
//! exists in the legacy location and has no identity-layout counterpart is used
//! in place, never moved.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use dashmap::DashMap;
use eyre::Result;

use super::catalog::{Catalog, read_receipt, write_receipt};
use super::identity::{Digest, InstallIdentity, Mode};
use super::record::{IdentityRecord, RECEIPT_FILE, Receipt};
use crate::file;
use crate::toolset::{ToolRequest, ToolVersion};
use crate::{dirs, env};

/// Whether the identity layout is on.
///
/// `experimental` turns it on. The test harness forces `experimental` for every
/// unit test, so unit tests opt in explicitly with `MISE_TEST_INSTALL_LAYOUT`.
pub(crate) fn enabled() -> bool {
    if mise_util::testing::in_tests() && std::env::var_os("MISE_TEST_INSTALL_LAYOUT").is_none() {
        return false;
    }
    crate::config::Settings::try_get().is_ok_and(|s| s.experimental)
}

/// Backends whose installs are not a plain directory mise owns, so a receipt and
/// a hashed directory name make no sense for them. They keep the legacy layout.
///
/// * `http` installs are links into a content-addressed extraction cache that
///   several tools may share.
/// * `core:rust` links the install to the user's rustup/cargo `bin` and
///   non-isolated `dotnet` links to the shared `DOTNET_ROOT`.
fn is_exempt(full: &str) -> bool {
    full.starts_with("http:")
        || matches!(
            full,
            "rust" | "core:rust" | "dotnet" | "core:dotnet" | "dotnet-core"
        )
}

/// Whether the identity layout governs where `tv` is installed.
pub(crate) fn applies_to(tv: &ToolVersion) -> bool {
    enabled()
        && tv.install_path.is_none()
        && !tv.install_path_is_explicit
        && !tv.install_path_is_exact
        && matches!(
            tv.request,
            ToolRequest::Version { .. }
                | ToolRequest::Prefix { .. }
                | ToolRequest::Sub { .. }
                | ToolRequest::Ref { .. }
        )
        && !is_exempt(&tv.ba().full_without_opts())
}

/// The installs roots to search, primary first. Only the primary root is ever
/// written to by an install.
fn roots() -> Vec<PathBuf> {
    let mut roots = vec![dirs::INSTALLS.to_path_buf()];
    roots.extend(env::shared_install_dirs());
    roots
}

fn digest_of_string(s: &str) -> String {
    Digest::of(s.as_bytes()).to_base32()
}

/// The identity `tv` answers.
///
/// * `backend` is the canonical full identifier, so `age` and
///   `aqua:FiloSottile/age` agree.
/// * `options` are the options that change what gets installed, as the backend
///   classifies them, plus digests of `install_env` and a one-shot `postinstall`
///   (which can change installed contents; a secret in `install_env` is hashed,
///   never recorded).
/// * A request restored from a lockfile that pins an artifact checksum is
///   [`Mode::Resolved`] and carries that checksum as an input.
///
/// `None` when the backend cannot be created, in which case the legacy layout
/// is used.
pub(crate) fn identity_of(tv: &ToolVersion) -> Option<InstallIdentity> {
    let backend = tv.backend().ok()?;
    let mut options = backend.install_identity_options(tv);
    let core = tv.request.options();
    if !core.install_env.is_empty() {
        let env: BTreeMap<_, _> = core.install_env.iter().collect();
        options.insert(
            "install_env".into(),
            digest_of_string(&serde_json::to_string(&env).ok()?),
        );
    }
    if let Some((script, false)) = core.postinstall() {
        options.insert("postinstall".into(), digest_of_string(script));
    }
    let mut inputs = BTreeMap::new();
    if let Some(graph) = tv.aube_install_identity() {
        inputs.insert("aube".to_string(), graph);
    }
    if let Some(graph) = tv.uv_install_identity() {
        inputs.insert("uv".to_string(), graph);
    }
    let platform = backend.get_platform_key();
    let mut mode = Mode::Fallback;
    if tv.resolved_from_lockfile()
        && let Some(checksum) = tv
            .lock_platforms
            .get(&platform)
            .and_then(|p| p.checksum.clone())
    {
        inputs.insert("artifact.checksum".to_string(), checksum);
        mode = Mode::Resolved;
    }
    Some(InstallIdentity {
        mode,
        backend: crate::backend::canonical_backend_full(&tv.ba().full_without_opts()).into_owned(),
        version: tv.logical_pathname(),
        platform,
        options,
        inputs,
    })
}

/// The pinned artifact checksum of a [`Mode::Resolved`] identity.
fn pin_of(identity: &InstallIdentity) -> Option<&str> {
    identity.inputs.get("artifact.checksum").map(String::as_str)
}

/// Where an installation of an identity is, or would be.
#[derive(Clone, Debug)]
pub(crate) struct Located {
    /// The installation directory.
    pub(crate) dir: PathBuf,
    /// The installs root that holds (or would hold) it.
    pub(crate) root: PathBuf,
    /// The catalog record, when the identity was ever allocated.
    pub(crate) record: Option<IdentityRecord>,
    /// Whether a complete installation is there.
    pub(crate) installed: bool,
}

static LOCATE_CACHE: LazyLock<DashMap<(PathBuf, String), Located>> = LazyLock::new(DashMap::new);

/// Forget cached lookups. Called when install state is reset.
pub(crate) fn reset_cache() {
    LOCATE_CACHE.clear();
}

/// A complete installation: the directory exists and its receipt is intact.
/// The receipt is written last, so its absence means the install was
/// interrupted or has not finished.
pub(crate) fn is_complete(dir: &Path) -> bool {
    dir.is_dir() && read_receipt(dir).is_some()
}

/// Find the installation that satisfies `tv`, without writing anything.
///
/// Order: the catalog record for the identity (or, for an unlocked request, the
/// installation its selection points at), then a legacy installation that is
/// compatible with the request, then the directory the identity would be
/// allocated. `None` means the identity layout does not govern `tv`.
pub(crate) fn locate(tv: &ToolVersion) -> Option<Located> {
    if !applies_to(tv) {
        return None;
    }
    let identity = identity_of(tv)?;
    let digest = identity.digest().to_base32();
    for root in roots() {
        let cache_key = (root.clone(), digest.clone());
        if let Some(hit) = LOCATE_CACHE.get(&cache_key) {
            return Some(hit.clone());
        }
        let catalog = Catalog::new(&root);
        for record in candidates(&catalog, &identity) {
            let dir = catalog.install_dir(&record);
            let installed = is_complete(&dir);
            let located = Located {
                dir,
                root: root.clone(),
                record: Some(record),
                installed,
            };
            if installed {
                LOCATE_CACHE.insert(cache_key, located.clone());
                return Some(located);
            }
        }
    }
    // An install made before the identity layout, in place.
    if let Some(legacy) = legacy_dir(tv, &identity) {
        return Some(Located {
            root: legacy.parent().map(Path::to_path_buf).unwrap_or_default(),
            dir: legacy,
            record: None,
            installed: true,
        });
    }
    // Not installed. Report where a record says it was (a pruned payload
    // keeps its path), or where it would be allocated.
    let primary = Catalog::new(dirs::INSTALLS.to_path_buf());
    let record = candidates(&primary, &identity).into_iter().next();
    let dir = match &record {
        Some(record) => primary.install_dir(record),
        None => primary.tentative_dir(&identity),
    };
    Some(Located {
        dir,
        root: primary.root().to_path_buf(),
        record,
        installed: false,
    })
}

/// The catalog records that could satisfy `identity`, best first.
///
/// An unlocked request is satisfied by the installation its selection points
/// at, else the one allocated for its own key. A request pinning an artifact is
/// satisfied by the installation allocated for exactly that pin, or by an
/// unlocked installation whose acquired artifact is the pinned one: adopting it
/// does not reinstall, and does not hash the installed files.
fn candidates(catalog: &Catalog, identity: &InstallIdentity) -> Vec<IdentityRecord> {
    let key = identity.request_key();
    let mut found: Vec<IdentityRecord> = vec![];
    let mut push = |record: Option<IdentityRecord>| {
        if let Some(record) = record
            && !found.iter().any(|r| r.digest == record.digest)
        {
            found.push(record);
        }
    };
    match pin_of(identity) {
        Some(pin) => {
            push(catalog.lookup(identity));
            let adoptable = |r: &IdentityRecord| {
                r.provenance.artifacts.get("checksum").map(String::as_str) == Some(pin)
            };
            push(catalog.selected_record(&key).filter(adoptable));
            push(catalog.lookup(&key).filter(adoptable));
        }
        None => {
            push(catalog.selected_record(&key));
            push(catalog.lookup(&key));
        }
    }
    found
}

/// A legacy `installs/<short>/<version>` directory that this request may use.
///
/// Legacy metadata records one backend per tool directory. It is only trusted
/// when that backend is the one the request resolves to; ambiguous or missing
/// metadata is not evidence of compatibility, so the version is then installed
/// afresh in the new layout instead of reinterpreting another backend's payload.
fn legacy_dir(tv: &ToolVersion, identity: &InstallIdentity) -> Option<PathBuf> {
    let name = tv.tv_pathname();
    for root in roots() {
        let tool_dir = root.join(crate::backend::tool_directory_name(&tv.ba().short));
        let path = tool_dir.join(&name);
        // A link is either a compatibility link into the identity layout or a
        // runtime alias; only a real directory is a legacy installation.
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if !meta.is_dir() || meta.file_type().is_symlink() {
            continue;
        }
        if crate::toolset::install_state::legacy_backend_matches(
            &root,
            &tv.ba().short,
            &identity.backend,
        ) && !tv.is_incomplete_at(&path)
        {
            return Some(path);
        }
    }
    None
}

/// The canonical backends whose installations count as installs of `ba`: the one
/// it resolves to now, and every backend the registry routes this tool to, so a
/// tool that moved between backends (or splits versions across them) still lists
/// the versions installed from each.
fn backends_of(ba: &crate::args::BackendArg) -> Vec<String> {
    let mut out =
        vec![crate::backend::canonical_backend_full(&ba.full_without_opts()).into_owned()];
    if let Some(tool) = ba.registry_tool() {
        for backend in tool.backends {
            let name = crate::args::split_bracketed_opts(backend.full)
                .map_or(backend.full, |(name, _)| name);
            let name = crate::backend::canonical_backend_full(name).into_owned();
            if !out.contains(&name) {
                out.push(name);
            }
        }
    }
    out
}

/// The name a record's version is listed under: the logical version, plus the
/// private dependency-graph suffix of an embedded aube or uv install, so it
/// matches the name of its compatibility link.
fn listing_name(record: &IdentityRecord) -> String {
    let identity = &record.identity;
    for (key, tag) in [("uv", "uv"), ("aube", "aube")] {
        if let Some(graph) = identity.inputs.get(key)
            && graph.len() >= 16
        {
            return format!("{}~{tag}~{}", identity.version, &graph[..16]);
        }
    }
    identity.version.clone()
}

/// The complete identity-layout installations of a tool: `(listing name, directory)`.
pub(crate) fn installs_of(ba: &crate::args::BackendArg) -> Vec<(String, PathBuf)> {
    if !enabled() {
        return vec![];
    }
    let mut out: Vec<(String, PathBuf)> = vec![];
    for root in roots() {
        let catalog = Catalog::new(&root);
        for backend in backends_of(ba) {
            for record in catalog.records_for_backend(&backend) {
                let dir = catalog.install_dir(&record);
                if is_complete(&dir) && !out.iter().any(|(_, d)| *d == dir) {
                    out.push((listing_name(&record), dir));
                }
            }
        }
    }
    out
}

/// The installation a listed version name stands for: the one its link names,
/// else the first complete installation with that listing name.
pub fn physical_dir(ba: &crate::args::BackendArg, name: &str) -> Option<PathBuf> {
    if !enabled() {
        return None;
    }
    for root in roots() {
        let slot = root
            .join(crate::backend::tool_directory_name(&ba.short))
            .join(name);
        if let Some(dir) = link_target(&slot) {
            return Some(dir);
        }
    }
    installs_of(ba)
        .into_iter()
        .find(|(n, _)| n == name)
        .map(|(_, dir)| dir)
}

/// The version of another installation of the same tool as `tv`, named by its
/// directory in the installs root: another variant or version, if its receipt says
/// it is an installation of `tv`'s backend. Used to explain what keeps a prune.
pub fn sibling_version(tv: &ToolVersion, dir_name: &str) -> Option<String> {
    let dir = tv.install_path().parent()?.join(dir_name);
    let receipt = read_receipt(&dir)?;
    let backend = crate::backend::canonical_backend_full(&tv.ba().full_without_opts()).into_owned();
    (receipt.record.identity.backend == backend).then_some(receipt.record.identity.version)
}

/// Refuse to remove an identity-layout path that mise did not create: a direct
/// child of an installs root that has neither a receipt nor a reserved name.
/// Anything else (a legacy `<tool>/<version>` dir, an explicit path) is not this
/// function's business.
pub(crate) fn guard_removal(path: &Path) -> Result<()> {
    let Some(name) = dir_name_of(path) else {
        return Ok(());
    };
    let root = path.parent().unwrap_or(path);
    if read_receipt(path).is_some() || root.join(".mise").join("names").join(&name).exists() {
        return Ok(());
    }
    eyre::bail!(
        "refusing to remove {}: it is not an installation mise created",
        path.display()
    )
}

/// Remove every version link in the installs root that names the installation
/// `dir`, from whichever tool directory holds it (`age` and
/// `aqua:FiloSottile/age` each have their own). Best effort: a link left behind
/// dangles and is collected by the next rebuild.
pub(crate) fn unlink_installation(dir: &Path) {
    let (Some(name), Some(root)) = (dir.file_name(), dir.parent()) else {
        return;
    };
    for tool in file::dir_subdirs(root).unwrap_or_default() {
        if is_reserved_dir(root, &tool) {
            continue;
        }
        let tool_dir = root.join(&tool);
        for entry in file::ls(&tool_dir).unwrap_or_default() {
            if !is_dir_link(&entry) || !is_compat_link_shape(&entry) {
                continue;
            }
            let names_it = file::resolve_symlink(&entry)
                .ok()
                .flatten()
                .is_some_and(|target| target.file_name() == Some(name));
            if names_it && let Err(err) = file::remove_dir_link(&entry) {
                debug!("could not remove version link {}: {err:#}", entry.display());
            }
        }
    }
}

/// Remove every identity-layout installation of a tool's backends, for
/// `mise plugins uninstall --purge`. The catalog keeps its records.
pub(crate) fn purge_installs(ba: &crate::args::BackendArg) -> Result<()> {
    if !enabled() {
        return Ok(());
    }
    for (_, dir) in installs_of(ba) {
        if dir.parent() == Some(&**dirs::INSTALLS) {
            file::remove_all(&dir)?;
        }
    }
    Ok(())
}

/// The complete installations of canonical `backend` at logical `version`, and
/// the checksum each was acquired or pinned with. Prune uses it to keep what a
/// lockfile entry names.
pub(crate) fn installs_matching(backend: &str, version: &str) -> Vec<(String, Option<String>)> {
    let mut out = vec![];
    for root in roots() {
        let catalog = Catalog::new(&root);
        for record in catalog.records_for_backend(backend) {
            if record.identity.version != version || !is_complete(&catalog.install_dir(&record)) {
                continue;
            }
            let checksum = record
                .identity
                .inputs
                .get("artifact.checksum")
                .or_else(|| record.provenance.artifacts.get("checksum"))
                .cloned();
            out.push((record.dir, checksum));
        }
    }
    out
}

/// The path to put on PATH for an unlocked `tv`: the link users know
/// (`installs/<short>/<alias or version>`) when it resolves to the installation
/// this request selects, else the installation directory itself. A link can point
/// at only one variant of a version, so it is not trusted blindly.
pub(crate) fn runtime_dir(tv: &ToolVersion) -> PathBuf {
    let install = tv.install_path();
    if install.parent() != Some(&**dirs::INSTALLS) {
        return install;
    }
    let name = tv.runtime_pathname().unwrap_or_else(|| tv.tv_pathname());
    let candidate = tv.ba().installs_path().join(name);
    match (candidate.canonicalize(), install.canonicalize()) {
        (Ok(a), Ok(b)) if a == b => candidate,
        _ => install,
    }
}

/// What [`allocate`] decided.
#[derive(Clone, Debug)]
pub(crate) struct Allocated {
    pub(crate) record: IdentityRecord,
    pub(crate) dir: PathBuf,
    /// An existing installation that already satisfies the request: nothing
    /// needs to be installed, only recorded.
    pub(crate) reused: bool,
    /// The unlocked request this allocation answers; set when its selection
    /// should be (re)recorded on success. `None` for locked requests, which
    /// never touch a selection.
    pub(crate) selects: Option<InstallIdentity>,
}

/// Choose (and reserve) the directory an install of `tv` goes into.
///
/// `refresh` is an explicit reinstall or update. An unlocked installation that
/// a lockfile adopted is never replaced in place: a refresh moves the shared
/// selection to a new generation instead, so the pinned installation stays as
/// the lockfile expects.
pub(crate) fn allocate(tv: &ToolVersion, refresh: bool) -> Result<Option<Allocated>> {
    if !applies_to(tv) {
        return Ok(None);
    }
    let Some(identity) = identity_of(tv) else {
        return Ok(None);
    };
    let catalog = Catalog::new(dirs::INSTALLS.to_path_buf());
    let key = identity.request_key();
    let locked = pin_of(&identity).is_some();

    if let Some(existing) = candidates(&catalog, &identity).into_iter().next() {
        let pinned_elsewhere = !existing.provenance.pinned_by.is_empty() && !locked;
        if !(refresh && pinned_elsewhere) {
            return Ok(Some(Allocated {
                dir: catalog.install_dir(&existing),
                reused: true,
                selects: (!locked).then_some(key),
                record: existing,
            }));
        }
        // Move the unlocked selection to a new generation.
        let mut next = key.clone();
        next.inputs.insert(
            "generation".into(),
            (existing.provenance.generation + 1).to_string(),
        );
        let mut record = catalog.allocate(&next)?;
        record.provenance.generation = existing.provenance.generation + 1;
        catalog.update_provenance(&next, record.provenance.clone())?;
        return Ok(Some(Allocated {
            dir: catalog.install_dir(&record),
            reused: false,
            selects: Some(key),
            record,
        }));
    }
    let record = catalog.allocate(&identity)?;
    Ok(Some(Allocated {
        dir: catalog.install_dir(&record),
        reused: false,
        selects: (!locked).then_some(key),
        record,
    }))
}

/// Record a finished (or reused) installation: write its receipt, point the
/// compatibility link for the requesting tool at it, and remember the unlocked
/// selection. The receipt goes in last; its presence is what marks the
/// installation complete.
pub(crate) fn finish(tv: &ToolVersion, allocated: &Allocated) -> Result<()> {
    let catalog = Catalog::new(dirs::INSTALLS.to_path_buf());
    let mut record = allocated.record.clone();
    if let Some(checksum) = tv
        .backend()
        .ok()
        .map(|b| b.get_platform_key())
        .and_then(|platform| tv.lock_platforms.get(&platform))
        .and_then(|p| p.checksum.clone())
    {
        record
            .provenance
            .artifacts
            .insert("checksum".into(), checksum);
    }
    if let Some(pin) = pin_of(&record.identity)
        && !record.provenance.pinned_by.iter().any(|p| p == pin)
    {
        record.provenance.pinned_by.push(pin.to_string());
    }
    if !allocated.reused || read_receipt(&allocated.dir).is_none() {
        write_receipt(
            &allocated.dir,
            &Receipt {
                record: record.clone(),
                requested_as: Some(tv.ba().short.clone()),
                mise_version: Some(crate::version::VERSION.to_string()),
            },
        )?;
    }
    catalog.update_provenance(&record.identity, record.provenance.clone())?;
    link(tv, &allocated.dir)?;
    if let Some(key) = &allocated.selects {
        catalog.select(key, &record, None)?;
    }
    LOCATE_CACHE.clear();
    Ok(())
}

/// Use an installation that already satisfies `tv` (found by [`locate`]): make
/// sure the requesting tool has its compatibility link and, for a pinned
/// request that adopted an unlocked installation, remember the pin so a later
/// refresh does not replace it in place.
pub(crate) fn note_reuse(tv: &ToolVersion) -> Result<()> {
    let Some(located) = locate(tv) else {
        return Ok(());
    };
    let (true, Some(record)) = (located.installed, located.record) else {
        return Ok(());
    };
    if located.root != *dirs::INSTALLS {
        // A read-only shared installation: never written to.
        return Ok(());
    }
    let Some(identity) = identity_of(tv) else {
        return Ok(());
    };
    let catalog = Catalog::new(dirs::INSTALLS.to_path_buf());
    if let Some(pin) = pin_of(&identity)
        && !record.provenance.pinned_by.iter().any(|p| p == pin)
    {
        let mut provenance = record.provenance.clone();
        provenance.pinned_by.push(pin.to_string());
        catalog.update_provenance(&record.identity, provenance)?;
        LOCATE_CACHE.clear();
    }
    link(tv, &located.dir)
}

/// `installs/<short>/<version> -> ../<name>-<hash>`, for the tool that asked.
///
/// A real legacy directory in that slot is left alone. Where a link cannot be
/// created (Windows without junction support) the install still works through
/// mise; the compatibility path is just unavailable, with a warning.
pub(crate) fn link(tv: &ToolVersion, dir: &Path) -> Result<()> {
    let tool_dir = tv.ba().installs_path().to_path_buf();
    let slot = tool_dir.join(tv.tv_pathname());
    if let Ok(meta) = std::fs::symlink_metadata(&slot)
        && !meta.file_type().is_symlink()
        && meta.is_dir()
    {
        // A junction on Windows reports as a directory, not a symlink.
        if !file::is_symlink_or_junction(&slot) {
            return Ok(());
        }
    }
    file::create_dir_all(&tool_dir)?;
    let target = Path::new("..").join(dir.file_name().unwrap_or_default());
    if file::resolve_symlink(&slot)?.is_some_and(|current| same_link_target(&slot, &current, dir)) {
        return Ok(());
    }
    if let Err(err) = file::make_dir_link(&target, &slot) {
        if cfg!(windows) {
            warn!(
                "could not link {} to {}: {err:#}. {} works through mise; run `mise where` for \
                 its path.",
                slot.display(),
                dir.display(),
                tv.style()
            );
            return Ok(());
        }
        return Err(err);
    }
    Ok(())
}

fn same_link_target(slot: &Path, current: &Path, dir: &Path) -> bool {
    let current = if current.is_absolute() {
        current.to_path_buf()
    } else {
        slot.parent().unwrap_or(slot).join(current)
    };
    match (current.canonicalize(), dir.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// The installation directory a compatibility link names, if `slot` is one: a
/// link whose target is a direct child of an installs root holding a receipt.
pub fn link_target(slot: &Path) -> Option<PathBuf> {
    let target = file::resolve_symlink(slot).ok().flatten()?;
    let target = if target.is_absolute() {
        target
    } else {
        slot.parent()?.join(target)
    };
    let target = target.canonicalize().ok().or(Some(target))?;
    let parent = target.parent()?;
    let is_root = roots()
        .iter()
        .any(|r| r.canonicalize().ok().as_deref() == Some(parent) || r.as_path() == parent);
    (is_root && target.join(RECEIPT_FILE).exists()).then_some(target)
}

/// Whether the entry `name` of an installs `root` is not a tool directory: the
/// catalog (`.mise`, or any hidden entry) or an identity-layout installation,
/// told apart by its reservation or its receipt rather than by how it is named.
pub(crate) fn is_reserved_dir(root: &Path, name: &str) -> bool {
    name.starts_with('.')
        || root.join(".mise").join("names").join(name).exists()
        || root.join(name).join(RECEIPT_FILE).exists()
}

/// Whether `path` is a directory link: a symlink, or a junction on Windows.
pub(crate) fn is_dir_link(path: &Path) -> bool {
    file::is_symlink_or_junction(path)
}

/// Whether `path` is shaped like a compatibility link: a link to a direct child
/// of the installs root that contains it (`../<name>-<hash>`). It says nothing
/// about whether that installation is still there.
pub(crate) fn is_compat_link_shape(path: &Path) -> bool {
    let Some(target) = file::resolve_symlink(path).ok().flatten() else {
        return false;
    };
    let target = if target.is_absolute() {
        target
    } else {
        match path.parent() {
            Some(parent) => parent.join(target),
            None => return false,
        }
    };
    let clean = |p: &Path| {
        use path_absolutize::Absolutize;
        p.absolutize().map(|p| p.into_owned()).ok()
    };
    let (Some(target), Some(tool_dir)) = (clean(&target), path.parent().and_then(clean)) else {
        return false;
    };
    target.parent() == tool_dir.parent()
        && target.parent().is_some()
        && target
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(has_hash_suffix)
}

/// Whether `name` ends in `-<base32 digest prefix>` the way an allocated
/// installation directory does (at least 8 lowercase base32 characters).
fn has_hash_suffix(name: &str) -> bool {
    name.rsplit_once('-').is_some_and(|(label, suffix)| {
        !label.is_empty()
            && suffix.len() >= 8
            && suffix.len() <= 52
            && suffix
                .bytes()
                .all(|b| matches!(b, b'a'..=b'z' | b'2'..=b'7'))
    })
}

/// Whether `path` is an installation directory of the identity layout (a
/// direct child of an installs root) rather than a legacy `<tool>/<version>`.
pub fn dir_name_of(path: &Path) -> Option<String> {
    let parent = path.parent()?;
    roots()
        .iter()
        .any(|r| r.as_path() == parent)
        .then(|| path.file_name().map(|n| n.to_string_lossy().to_string()))
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_suffix_shape() {
        assert!(has_hash_suffix("age-p4n6w2ra"));
        assert!(has_hash_suffix("node-k7m3q2vd6n"));
        assert!(!has_hash_suffix("nowhere"));
        assert!(!has_hash_suffix("age-short"));
        assert!(!has_hash_suffix("-p4n6w2ra"));
        assert!(!has_hash_suffix("age-P4N6W2RA"));
        assert!(!has_hash_suffix("age-p4n6w2r1"));
    }

    #[test]
    fn exempt_backends() {
        assert!(is_exempt("http:my-tool"));
        assert!(is_exempt("core:rust"));
        assert!(is_exempt("dotnet"));
        assert!(!is_exempt("aqua:FiloSottile/age"));
        assert!(!is_exempt("core:node"));
    }
}
