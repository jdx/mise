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
use std::sync::OnceLock;

use dashmap::DashMap;
use eyre::Result;

use super::catalog::{Catalog, read_receipt, write_receipt};
use super::identity::{Digest, InstallIdentity, Mode};
use super::record::{IdentityRecord, RECEIPT_FILE, Receipt};
use crate::file;
use crate::toolset::{ToolRequest, ToolVersion};
use crate::{dirs, env};

/// Whether the identity layout is on: `install_layout = "identity"`, which is
/// opt-in even with `experimental` on and requires it. (Later, `experimental` will
/// include it.)
pub fn enabled() -> bool {
    let Ok(settings) = crate::config::Settings::try_get() else {
        return false;
    };
    if settings.install_layout.as_deref() != Some("identity") {
        return false;
    }
    if !settings.experimental {
        warn_once!(
            "[experimental] install_layout = \"identity\" requires experimental = true; \
             installing into the legacy layout"
        );
        return false;
    }
    true
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

/// Whether two paths name the same place, compared as written (no symlink
/// resolution). On Windows case, separators and the `\\?\` verbatim prefix are
/// ignored, which canonicalizing would add and the configured roots do not carry.
pub(crate) fn same_path(a: &Path, b: &Path) -> bool {
    // A relative data directory (`MISE_DATA_DIR=data`) makes the configured roots
    // relative while paths derived from a link are absolute: compare them
    // from the same working directory.
    use path_absolutize::Absolutize;
    let absolute = |p: &Path| {
        p.absolutize()
            .map_or_else(|_| p.to_path_buf(), |p| p.into_owned())
    };
    let (a, b) = (absolute(a), absolute(b));
    let (a, b) = (a.as_path(), b.as_path());
    if cfg!(windows) {
        windows_comparable(a) == windows_comparable(b)
    } else {
        a == b
    }
}

/// A Windows path in the form two spellings of one place share: one kind of
/// separator, case folded, no trailing separator, and the verbatim prefix undone
/// (`\\?\C:\x` is `C:\x`, `\\?\UNC\server\share` is `\\server\share`).
fn windows_comparable(path: &Path) -> String {
    let text = path.to_string_lossy().replace('/', "\\").to_lowercase();
    let text = if let Some(rest) = text.strip_prefix(r"\\?\unc\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = text.strip_prefix(r"\\?\") {
        rest.to_string()
    } else {
        text
    };
    text.trim_end_matches('\\').to_string()
}

/// Whether `path` is the primary installs root.
pub(crate) fn is_primary_root(path: &Path) -> bool {
    same_path(path, &dirs::INSTALLS)
}

/// The directory that holds the installations of the installs root `root`: the
/// install store ([`dirs::INSTALL_STORE`]) for the primary root, the root itself
/// for a shared one. Version links, runtime aliases and the catalog always stay in
/// the root. The store is the root itself unless it was moved, which Windows does
/// by default to shorten the real paths of installations.
pub(crate) fn store_of(root: &Path) -> PathBuf {
    if is_primary_root(root) {
        dirs::INSTALL_STORE.to_path_buf()
    } else {
        root.to_path_buf()
    }
}

/// Whether `path` is the directory that holds the primary root's installations.
pub(crate) fn is_primary_store(path: &Path) -> bool {
    same_path(path, &dirs::INSTALL_STORE)
}

/// Whether `path` is an installation in the primary root's store, the only place
/// mise installs into.
pub(crate) fn is_primary_install(path: &Path) -> bool {
    path.parent().is_some_and(is_primary_store)
}

/// The installs root whose installations live in `store`.
fn root_of_store(store: &Path) -> Option<PathBuf> {
    roots()
        .into_iter()
        .find(|root| same_path(&store_of(root), store))
}

/// The directories that hold installations, primary first.
fn stores() -> Vec<PathBuf> {
    roots().iter().map(|root| store_of(root)).collect()
}

/// Whether the identity layout governs `tv`, either because it will choose where
/// `tv` installs or because an install already chose (the install path is set to a
/// directory of the layout).
pub(crate) fn governs(tv: &ToolVersion) -> bool {
    applies_to(tv)
        || (enabled()
            && tv
                .install_path
                .as_deref()
                .is_some_and(|path| dir_name_of(path).is_some()))
}

/// The installs roots to search, primary first. Only the primary root is ever
/// written to by an install.
fn roots() -> Vec<PathBuf> {
    let mut roots = vec![dirs::INSTALLS.to_path_buf()];
    // A shared directory can be configured as the user's own installs directory
    // (or twice); each root is searched once.
    for dir in env::shared_install_dirs() {
        if !roots.iter().any(|root| same_path(root, &dir)) {
            roots.push(dir);
        }
    }
    roots
}

fn digest_of_string(s: &str) -> String {
    Digest::of(s.as_bytes()).to_base32()
}

/// The canonical full backend identifier for the layout: pipx folded into pypi, no
/// options, and no credentials (a plugin repository URL can carry `user:token@`).
pub(crate) fn canonical_backend(full: &str) -> String {
    let name = crate::args::split_bracketed_opts(full).map_or(full, |(name, _)| name);
    redact_credentials(&crate::backend::canonical_backend_full(name))
}

/// The identity `tv` answers.
///
/// * `backend` is the canonical full identifier, so `age` and
///   `aqua:FiloSottile/age` agree.
/// * `options` are the options that change what gets installed, as the backend
///   classifies them, plus a digest of `install_env` (it can change what a build
///   produces; a secret in it is hashed, never recorded). `postinstall` is not an
///   input: it runs after the install, and editing it has never reinstalled a tool.
/// * A request restored from a lockfile that pins an artifact checksum is
///   [`Mode::Resolved`] and carries that checksum as an input.
///
/// `None` when the backend cannot be created, in which case the legacy layout
/// is used.
pub(crate) fn identity_of(tv: &ToolVersion) -> Option<InstallIdentity> {
    let backend = tv.backend().ok()?;
    // Credentials in an option (a registry URL, a signed download URL) must not
    // reach the catalog or a receipt, which are plain files: URL userinfo is
    // dropped, and a query string, which can carry a token but can also choose
    // what is downloaded, takes part in the identity only as a digest.
    let mut options: BTreeMap<String, String> = backend
        .install_identity_options(tv)
        .into_iter()
        .map(|(key, value)| (key, redact_credentials(&value)))
        .collect();
    let core = tv.request.options();
    if !core.install_env.is_empty() {
        let env: BTreeMap<_, _> = core.install_env.iter().collect();
        options.insert(
            "install_env".into(),
            digest_of_string(&serde_json::to_string(&env).ok()?),
        );
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
        backend: canonical_backend(&tv.ba().full()),
        version: tv.logical_pathname(),
        platform,
        options,
        inputs,
    })
}

/// `value` with the parts of URLs in it that can carry credentials kept out of
/// plain text: userinfo (`user:token@`) is removed, and the query string, where
/// signed URLs and API keys (`?token=`) put them but which can also choose what
/// is downloaded (`?id=2`), is replaced by a digest of itself, so two queries
/// still make two identities. Applied to whole values and to values that merely
/// contain a URL.
pub(crate) fn redact_credentials(value: &str) -> String {
    let ends_at = |s: &str, stop: &dyn Fn(char) -> bool| s.find(stop).unwrap_or(s.len());
    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(i) = rest.find("://") {
        let (head, tail) = rest.split_at(i + 3);
        out.push_str(head);
        // The authority ends at the first `/`, `?`, `#` or whitespace; userinfo is
        // whatever precedes the last `@` inside it.
        let end = ends_at(tail, &|c| matches!(c, '/' | '?' | '#') || c.is_whitespace());
        let authority = &tail[..end];
        let host = authority
            .rsplit_once('@')
            .map_or(authority, |(_, host)| host);
        out.push_str(host);
        // The path runs to the query; the query to the fragment or whitespace.
        let tail = &tail[end..];
        let path = ends_at(tail, &|c| matches!(c, '?' | '#') || c.is_whitespace());
        out.push_str(&tail[..path]);
        let mut tail = &tail[path..];
        if let Some(query) = tail.strip_prefix('?') {
            let end = ends_at(query, &|c| c == '#' || c.is_whitespace());
            out.push('?');
            out.push_str(&digest_of_string(&query[..end])[..16]);
            tail = &query[end..];
        }
        rest = tail;
    }
    out.push_str(rest);
    out
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

static LOCATE_CACHE: OnceLock<DashMap<(PathBuf, String), Located>> = OnceLock::new();
/// The installations of a set of backends (see [`installs_of`]), kept for the
/// life of the process like the rest of install state and cleared with it.
static INSTALLS_OF_CACHE: OnceLock<DashMap<String, Vec<(String, PathBuf)>>> = OnceLock::new();

fn locate_cache() -> &'static DashMap<(PathBuf, String), Located> {
    LOCATE_CACHE.get_or_init(DashMap::new)
}

fn installs_of_cache() -> &'static DashMap<String, Vec<(String, PathBuf)>> {
    INSTALLS_OF_CACHE.get_or_init(DashMap::new)
}

/// Forget cached lookups. Called when install state is reset.
pub(crate) fn reset_cache() {
    // Install state is reset on every run, usually before anything here was
    // looked up: building a cache only to empty it is not free.
    if let Some(cache) = LOCATE_CACHE.get() {
        cache.clear();
    }
    if let Some(cache) = INSTALLS_OF_CACHE.get() {
        cache.clear();
    }
}

/// A complete installation: the directory exists and its receipt is intact.
/// The receipt is written last, so its absence means the install was
/// interrupted or has not finished.
pub fn is_complete(dir: &Path) -> bool {
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
    let key = identity.request_key();
    let unlocked = pin_of(&identity).is_none();
    let mut ambiguous = false;
    let mut try_variant = false;
    if unlocked {
        let cache_key = (dirs::INSTALLS.to_path_buf(), digest.clone());
        if let Some(hit) = locate_cache().get(&cache_key) {
            return Some(hit.clone());
        }
        match unlocked_choice(&key) {
            // A selection is answered by what it names alone: if that was pruned
            // (installing restores it) or its root is gone, nothing else is chosen
            // in its place without the user saying so. The exception is an install
            // made before this layout at the version's path in the user's own
            // root: that path is what PATH runs, and installing the selection
            // could not put its link there.
            Unlocked::Selected(located) => {
                if located.installed {
                    locate_cache().insert(cache_key, located.clone());
                    return Some(located);
                }
                if is_primary_root(&located.root)
                    && let Some(legacy) = legacy_dir(tv, &identity)
                    && legacy
                        .parent()
                        .and_then(Path::parent)
                        .is_some_and(is_primary_root)
                {
                    return Some(legacy_located(legacy));
                }
                return Some(located);
            }
            Unlocked::Found(located) => {
                locate_cache().insert(cache_key, located.clone());
                return Some(located);
            }
            Unlocked::Ambiguous(_) => ambiguous = true,
            // Nothing of the request's own: a bare version named on the command
            // line may still stand for a variant, after a legacy install (below).
            Unlocked::Nothing => {
                try_variant = matches!(tv.request.source(), crate::toolset::ToolSource::Argument)
                    && is_bare(tv, &identity);
            }
        }
    } else {
        for root in roots() {
            let cache_key = (root.clone(), digest.clone());
            if let Some(hit) = locate_cache().get(&cache_key) {
                return Some(hit.clone());
            }
            let catalog = Catalog::new(&root);
            for record in pinned_candidates(&catalog, &identity) {
                let dir = catalog.install_dir(&record);
                if is_complete(&dir) {
                    let located = Located {
                        dir,
                        root: root.clone(),
                        record: Some(record),
                        installed: true,
                    };
                    locate_cache().insert(cache_key, located.clone());
                    return Some(located);
                }
            }
        }
    }
    // An install made before the identity layout, in place.
    if !ambiguous && let Some(legacy) = legacy_dir(tv, &identity) {
        return Some(legacy_located(legacy));
    }
    // A version named on the command line means whatever is installed of it, as
    // it did before this layout: a lone installation made with the request's
    // options and more that a configuration set (`filter_bins`, `matching`) stands
    // for it (see `extends`). With several, which is meant is not known. It is
    // not cached: the cache is keyed by the request, and the same request from a
    // configuration file does not stand for a variant.
    if try_variant && let [only] = variant_choices(tv.ba(), &identity).as_slice() {
        return Some(only.clone());
    }
    let primary = Catalog::new(dirs::INSTALLS.to_path_buf());
    // Several installations answer and none is chosen: report a path that is
    // never an installation, so nothing (`where`, `exec`, PATH) quietly runs one
    // of them. Installing reports the choice to make.
    if ambiguous {
        return Some(Located {
            dir: primary.meta_dir().join("ambiguous").join(&digest),
            root: primary.root().to_path_buf(),
            record: None,
            installed: false,
        });
    }
    // Not installed. Report where a record says it was (a pruned payload
    // keeps its path), or where it would be allocated.
    let record = if unlocked {
        primary.lookup(&key)
    } else {
        pinned_candidates(&primary, &identity).into_iter().next()
    };
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

/// What an unlocked request resolves to. Its selection lives in the user's own
/// catalog and may name an installation in a read-only shared root.
enum Unlocked {
    /// The installation the selection names, installed or not. Its record is
    /// `None` when the root it names no longer lists it.
    Selected(Located),
    /// Nothing is selected, and exactly one complete installation, in any root,
    /// answers the request.
    Found(Located),
    /// Nothing is selected, and several complete installations answer it:
    /// choosing one would be a guess.
    Ambiguous(Vec<Located>),
    /// Nothing is selected or installed.
    Nothing,
}

fn unlocked_choice(key: &InstallIdentity) -> Unlocked {
    let primary = Catalog::new(dirs::INSTALLS.to_path_buf());
    if let Some(selection) = primary.selection(key) {
        let root = selection
            .root
            .map(PathBuf::from)
            .unwrap_or_else(|| primary.root().to_path_buf());
        let catalog = Catalog::new(&root);
        // A shared root's catalog cannot be rebuilt here; the installation's own
        // receipt still says what it is.
        let record = catalog
            .record_by_digest(&key.backend, &selection.selected)
            .or_else(|| {
                (!is_primary_root(&root))
                    .then(|| catalog.find_by_receipt(&selection.selected))
                    .flatten()
            });
        // A selection in the user's own catalog whose record is gone is treated
        // as no selection; one naming another root is not given up on silently.
        if record.is_some() || !is_primary_root(&root) {
            let dir = match &record {
                Some(record) => catalog.install_dir(record),
                None => catalog.store().join(&selection.selected),
            };
            let installed = record.is_some() && is_complete(&dir);
            return Unlocked::Selected(Located {
                dir,
                root,
                record,
                installed,
            });
        }
    }
    let mut complete: Vec<Located> = vec![];
    for root in roots() {
        let catalog = Catalog::new(&root);
        // A shared root's catalog cannot be rebuilt here; if it is gone, the
        // installations it listed still count, by their receipts.
        for record in records_of(&catalog, &root, &key.backend) {
            let dir = catalog.install_dir(&record);
            if same_request(&record.identity, key) && is_complete(&dir) {
                complete.push(Located {
                    dir,
                    root: root.clone(),
                    record: Some(record),
                    installed: true,
                });
            }
        }
    }
    match complete.len() {
        0 => Unlocked::Nothing,
        1 => Unlocked::Found(complete.remove(0)),
        _ => Unlocked::Ambiguous(complete),
    }
}

/// Whether the request `identity` came from sets no install options of its own:
/// its options are those the same version named with nothing else gets (from the
/// tool's registry entry or backend alias, and from settings).
fn is_bare(tv: &ToolVersion, identity: &InstallIdentity) -> bool {
    // From the tool's name alone: options written inline (`tool[opt=value]`) are
    // the request's own, and the plain request does not have them.
    let Ok(plain) = ToolRequest::new(
        std::sync::Arc::new(crate::args::BackendArg::from(tv.ba().short.as_str())),
        &tv.version,
        crate::toolset::ToolSource::Argument,
    ) else {
        return false;
    };
    identity_of(&ToolVersion::new(plain, tv.version.clone()))
        .is_some_and(|plain| plain.options == identity.options)
}

/// Whether an installation made for `a` can stand for a request for `b`: it was
/// made with every install option `b` has (the request's own, its registry's
/// defaults, the settings that take part), and more besides. A version named
/// without options thus finds an installation a configuration made with some,
/// and never one made with different values for the options it has.
fn extends(a: &InstallIdentity, b: &InstallIdentity) -> bool {
    b.options
        .iter()
        .all(|(key, value)| a.options.get(key) == Some(value))
}

/// Whether `a` was built from the dependency graph (aube, uv) `b` names, or
/// neither has one. A graph is not an option a configuration sets: an
/// installation built from another one is not a variant of the request.
fn same_graph(a: &InstallIdentity, b: Option<&InstallIdentity>) -> bool {
    ["aube", "uv"]
        .iter()
        .all(|k| a.inputs.get(*k) == b.and_then(|b| b.inputs.get(*k)))
}

/// The installations a bare version could stand for, one for each set of options
/// they were made with: copies of one option set (another root, an older refresh
/// generation) are that set's own choice, its selection or its only copy. A set
/// whose selected copy is gone stands for nothing; one whose copies are not
/// settled lists them all, for the user to select.
fn variant_choices(ba: &crate::args::BackendArg, identity: &InstallIdentity) -> Vec<Located> {
    let mut sets: Vec<(InstallIdentity, Vec<Located>)> = vec![];
    for located in variant_installations(ba, identity) {
        let Some(record) = &located.record else {
            continue;
        };
        let key = request_of(&record.identity);
        match sets.iter_mut().find(|(k, _)| *k == key) {
            Some((_, copies)) => copies.push(located),
            None => sets.push((key, vec![located])),
        }
    }
    sets.into_iter()
        .flat_map(|(key, copies)| match unlocked_choice(&key) {
            // The set's own choice, if it is one of this tool's copies.
            Unlocked::Selected(located) | Unlocked::Found(located)
                if located.installed && copies.iter().any(|c| c.dir == located.dir) =>
            {
                vec![located]
            }
            // Its selected copy is gone (pruned): no other copy is taken for it.
            Unlocked::Selected(located) if !located.installed => vec![],
            _ => copies,
        })
        .collect()
}

/// The records of `backend` in the catalog of `root`; for a shared root whose
/// catalog is gone, its installations' receipts.
fn records_of(catalog: &Catalog, root: &Path, backend: &str) -> Vec<IdentityRecord> {
    if is_primary_root(root) {
        catalog.records_for_backend(backend)
    } else {
        catalog.records_or_receipts_for_backend(backend)
    }
}

/// The complete installations, in every root, of `identity`'s backend, version and
/// platform made for another request (other install options) of the tool `ba`;
/// another tool aliasing the same backend (`oxfmt` and `oxlint`) has its own.
fn variant_installations(ba: &crate::args::BackendArg, identity: &InstallIdentity) -> Vec<Located> {
    let own = request_of(identity);
    let mut out = vec![];
    for root in roots() {
        let catalog = Catalog::new(&root);
        for record in records_of(&catalog, &root, &identity.backend) {
            let dir = catalog.install_dir(&record);
            if record.identity.version == identity.version
                && record.identity.platform == identity.platform
                && same_graph(&record.identity, Some(identity))
                && extends(&record.identity, identity)
                && request_of(&record.identity) != own
                && is_complete(&dir)
                && belongs_to(ba, &record, &dir)
            {
                out.push(Located {
                    dir,
                    root: root.clone(),
                    record: Some(record),
                    installed: true,
                });
            }
        }
    }
    out
}

/// The catalog records that could satisfy a request pinning an artifact, best
/// first: the installation allocated for exactly that pin, one the pin already
/// adopted, or an unlocked installation whose acquired artifact is the pinned
/// one. Adopting it does not reinstall, and does not hash the installed files.
fn pinned_candidates(catalog: &Catalog, identity: &InstallIdentity) -> Vec<IdentityRecord> {
    let key = identity.request_key();
    let mut found: Vec<IdentityRecord> = vec![];
    let mut push = |record: Option<IdentityRecord>| {
        if let Some(record) = record
            && !found.iter().any(|r| r.digest == record.digest)
        {
            found.push(record);
        }
    };
    let Some(pin) = pin_of(identity) else {
        return found;
    };
    push(catalog.lookup(identity));
    // An installation this pin already adopted is the one it keeps, even if a
    // later refresh moved the unlocked selection to another copy of the same
    // artifact.
    push(
        catalog
            .records_for_backend(&key.backend)
            .into_iter()
            .find(|r| {
                same_request(&r.identity, &key) && r.provenance.pinned_by.iter().any(|p| p == pin)
            }),
    );
    let adoptable = |r: &IdentityRecord| {
        r.provenance.artifacts.get("checksum").map(String::as_str) == Some(pin)
    };
    push(catalog.selected_record(&key).filter(adoptable));
    push(catalog.lookup(&key).filter(adoptable));
    found
}

/// The unlocked request an installation answers: its identity without a pin
/// or a refresh generation. Its selection is recorded under this key.
fn request_of(identity: &InstallIdentity) -> InstallIdentity {
    let mut key = identity.request_key();
    key.inputs.remove("generation");
    key
}

/// The next refresh generation of the unlocked request `key`.
fn next_generation(catalog: &Catalog, key: &InstallIdentity) -> u32 {
    catalog
        .records_for_backend(&key.backend)
        .iter()
        .filter(|r| same_request(&r.identity, key))
        .map(|r| {
            let named = r
                .identity
                .inputs
                .get("generation")
                .and_then(|g| g.parse().ok())
                .unwrap_or(0);
            r.provenance.generation.max(named)
        })
        .max()
        .unwrap_or(0)
        + 1
}

/// Why an unlocked request with no selection cannot just use an installation.
fn ambiguity_error(tv: &ToolVersion, installations: &[Located]) -> eyre::Report {
    // Full paths: the same identity has the same name in each root that has it.
    let list = installations
        .iter()
        .map(|l| {
            let pinned = if l
                .record
                .as_ref()
                .is_some_and(|r| !r.provenance.pinned_by.is_empty())
            {
                " (a lockfile pins it)"
            } else {
                ""
            };
            format!("  {}{pinned}", file::display_path(&l.dir))
        })
        .collect::<Vec<_>>()
        .join("\n");
    eyre::eyre!(
        "{} matches several installations and none is selected:\n{list}\n\
         Choose one with `mise installs select <dir>`, or install a fresh one with \
         `mise install --force {}@{}`",
        tv.style(),
        tv.ba().short,
        tv.version
    )
}

/// Whether `identity` answers the unlocked request `key`, whichever refresh
/// generation of the request it belongs to.
fn same_request(identity: &InstallIdentity, key: &InstallIdentity) -> bool {
    request_of(identity) == request_of(key)
}

/// What [`locate`] reports for a legacy installation at `legacy`.
fn legacy_located(legacy: PathBuf) -> Located {
    Located {
        root: legacy.parent().map(Path::to_path_buf).unwrap_or_default(),
        dir: legacy,
        record: None,
        installed: true,
    }
}

/// A legacy `installs/<short>/<version>` directory that this request may use.
///
/// Legacy metadata records one backend per tool directory. A recorded backend
/// is only trusted when it is the one the request resolves to; otherwise the
/// version is installed afresh in the new layout instead of reinterpreting
/// another backend's payload. A tool directory with no recorded backend at all
/// (made before mise recorded one) is, as it always was, its short name's own.
fn legacy_dir(tv: &ToolVersion, identity: &InstallIdentity) -> Option<PathBuf> {
    let name = tv.tv_pathname();
    for root in roots() {
        let tool_dir = root.join(crate::backend::tool_directory_name(&tv.ba().short));
        let path = tool_dir.join(&name);
        if !is_foreign_slot(&path) {
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
    let mut out = vec![canonical_backend(&ba.full())];
    if let Some(tool) = ba.registry_tool() {
        for backend in tool.backends {
            let name = canonical_backend(backend.full);
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
pub fn installs_of(ba: &crate::args::BackendArg) -> Vec<(String, PathBuf)> {
    if !enabled() {
        return vec![];
    }
    let backends = backends_of(ba);
    let key = format!("{}|{:?}|{}", ba.short, ba.opts, backends.join("\n"));
    if let Some(hit) = installs_of_cache().get(&key) {
        return hit.clone();
    }
    let mut out: Vec<(String, PathBuf)> = vec![];
    for root in roots() {
        let catalog = Catalog::new(&root);
        for backend in &backends {
            for record in catalog.records_for_backend(backend) {
                let dir = catalog.install_dir(&record);
                // The receipt's presence is what marks an installation complete; its
                // contents were validated when the record was written.
                if dir.join(RECEIPT_FILE).is_file()
                    && !out.iter().any(|(_, d)| *d == dir)
                    && belongs_to(ba, &record, &dir)
                {
                    out.push((listing_name(&record), dir));
                }
            }
        }
    }
    installs_of_cache().insert(key, out.clone());
    out
}

/// Whether an installation is one of `ba`'s own. Tools that share a backend can
/// still be different installs of it (two `[tool_alias]` entries of one backend
/// with different options), and each should see only its own. An installation
/// is `ba`'s when its tool directory links it, when `ba`'s spelling requested it,
/// or when `ba` would make the very same request (the shorthand and the explicit
/// backend share one).
fn belongs_to(ba: &crate::args::BackendArg, record: &IdentityRecord, dir: &Path) -> bool {
    let name = listing_name(record);
    if link_target(&ba.installs_path().join(&name)).is_some_and(|linked| same_path(&linked, dir)) {
        return true;
    }
    if read_receipt(dir)
        .and_then(|receipt| receipt.requested_as)
        .is_some_and(|short| short == ba.short)
    {
        return true;
    }
    let Ok(request) = ToolRequest::new(
        std::sync::Arc::new(ba.clone()),
        &record.identity.version,
        crate::toolset::ToolSource::Unknown,
    ) else {
        return true;
    };
    let tv = ToolVersion::new(request, record.identity.version.clone());
    // `ba`'s plain request carries no dependency graph (that comes from a
    // lockfile), so a graph install is `ba`'s when it was otherwise the same
    // request. Two aliases of one backend still differ in their options.
    let mut recorded = record.identity.clone();
    recorded.inputs.remove("aube");
    recorded.inputs.remove("uv");
    identity_of(&tv).is_none_or(|identity| same_request(&recorded, &identity))
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
        // A legacy installation or a `mise link` stands for itself: another
        // installation that happens to list under the same name must not be mistaken
        // for it.
        if is_foreign_slot(&slot) {
            return Some(slot);
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
    let receipt = stores()
        .into_iter()
        .find_map(|store| read_receipt(&store.join(dir_name)))?;
    let backend = canonical_backend(&tv.ba().full());
    (receipt.record.identity.backend == backend).then_some(receipt.record.identity.version)
}

/// Refuse to remove an identity-layout path that mise did not create: a direct
/// child of an installs root that has neither a receipt nor a reserved name.
/// Anything else (a legacy `<tool>/<version>` dir, an explicit path) is not this
/// function's business.
pub(crate) fn guard_removal(path: &Path) -> Result<()> {
    // Any direct child of an install store, whatever it is named: a tool
    // directory beside the installations is not one either.
    let (Some(name), Some(store)) = (path.file_name(), path.parent()) else {
        return Ok(());
    };
    let Some(root) = root_of_store(store) else {
        return Ok(());
    };
    if read_receipt(path).is_some() || root.join(".mise").join("names").join(name).exists() {
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
    let (Some(name), Some(root)) = (dir.file_name(), dir.parent().and_then(root_of_store)) else {
        return;
    };
    let root = root.as_path();
    for tool in file::dir_subdirs(root).unwrap_or_default() {
        if is_reserved_dir(root, &tool) {
            continue;
        }
        let tool_dir = root.join(&tool);
        for entry in file::ls(&tool_dir).unwrap_or_default() {
            if !is_dir_link(&entry) || !is_compat_link_shape(&entry) {
                continue;
            }
            // The whole target is compared, not its name: a link to the same
            // identity in another root's store has the same name.
            let names_it = file::resolve_symlink(&entry)
                .ok()
                .flatten()
                .is_some_and(|target| {
                    target.file_name() == Some(name) && same_path(&tool_dir.join(&target), dir)
                });
            if names_it && let Err(err) = file::remove_dir_link(&entry) {
                debug!("could not remove version link {}: {err:#}", entry.display());
            }
        }
    }
    reset_cache();
}

/// Remove every identity-layout installation of a tool's backends, for
/// `mise plugins uninstall --purge`. The catalog keeps its records.
pub(crate) fn purge_installs(ba: &crate::args::BackendArg) -> Result<()> {
    if !enabled() {
        return Ok(());
    }
    for (_, dir) in installs_of(ba) {
        if is_primary_install(&dir) {
            file::remove_all(&dir)?;
        }
    }
    reset_cache();
    Ok(())
}

/// The complete installations of canonical `backend` at `version`, and the
/// checksum each was acquired or pinned with. Prune uses it to keep what a
/// lockfile entry names.
///
/// A lockfile writes a version as requested (`ref:main`) and an identity records
/// its path name (`ref-main`), so both are compared the way a version directory
/// is named.
pub(crate) fn installs_matching(backend: &str, version: &str) -> Vec<(String, Option<String>)> {
    let pathname = |v: &str| v.replace([':', '/'], "-");
    let version = pathname(version);
    let mut out = vec![];
    for root in roots() {
        let catalog = Catalog::new(&root);
        for record in catalog.records_for_backend(backend) {
            if pathname(&record.identity.version) != version
                || !is_complete(&catalog.install_dir(&record))
            {
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

/// The installations of `tv`'s tool and version on this platform made with the
/// install options `tv` carries and more (see `extends`). A version named on the
/// command line carries none of its own, so `mise where tool@1.0` has to find a
/// copy made with the options a configuration file sets. Installations of `tv`'s
/// own request are not variants: which of those it uses is its selection's
/// business, never a guess here.
pub fn variants_of(tv: &ToolVersion) -> Vec<PathBuf> {
    if !enabled() {
        return vec![];
    }
    let mut bare = tv.clone();
    bare.install_path = None;
    let own_identity = identity_of(&bare);
    // A request with options of its own (named, or set by a configuration) means
    // exactly those; only a bare version can stand for a variant.
    if own_identity
        .as_ref()
        .is_some_and(|identity| !is_bare(tv, identity))
    {
        return vec![];
    }
    let own_request = own_identity.as_ref().map(request_of);
    // The request has installations of its own, or a selection: which one it uses
    // is for the selection to say (or the user, when that is ambiguous), so a
    // variant made with other options is no stand-in for it.
    if let Some(own) = &own_request
        && !matches!(unlocked_choice(own), Unlocked::Nothing)
    {
        return vec![];
    }
    let Some(own_identity) = own_identity else {
        return vec![];
    };
    variant_choices(tv.ba(), &own_identity)
        .into_iter()
        .map(|located| located.dir)
        .collect()
}

/// The path to put on PATH for an unlocked `tv`: the link users know
/// (`installs/<short>/<alias or version>`) when it resolves to the installation
/// this request selects, else the installation directory itself. A link can point
/// at only one variant of a version, so it is not trusted blindly.
pub(crate) fn runtime_dir(tv: &ToolVersion) -> PathBuf {
    let install = tv.install_path();
    if !is_primary_install(&install) {
        return install;
    }
    let name = tv.runtime_pathname().unwrap_or_else(|| tv.tv_pathname());
    let candidate = tv.ba().installs_path().join(name);
    match (candidate.canonicalize(), install.canonicalize()) {
        (Ok(a), Ok(b)) if a == b => candidate,
        _ => install,
    }
}

/// Serialize installs and removals of one installation across every spelling of
/// its tool. The per-tool install lock is keyed by the tool's own cache directory,
/// which `age` and `aqua:FiloSottile/age` do not share, while the installation
/// they share has exactly one directory.
pub fn lock_install(dir: &Path, on_wait: &dyn Fn(Option<u32>)) -> Result<Option<fslock::LockFile>> {
    let Some(path) = install_lock_path(dir) else {
        return Ok(None);
    };
    Ok(Some(
        crate::lock_file::LockFile::at(&path)
            .with_pid()
            .lock_with_notice(on_wait)?,
    ))
}

/// [`lock_install`] without waiting: `None` when it is held, or `dir` takes no lock.
pub fn try_lock_install(dir: &Path) -> Result<Option<fslock::LockFile>> {
    match install_lock_path(dir) {
        Some(path) => crate::lock_file::LockFile::at(&path).with_pid().try_lock(),
        None => Ok(None),
    }
}

fn install_lock_path(dir: &Path) -> Option<PathBuf> {
    let name = dir_name_of(dir)?;
    // A shared root is read-only: a satisfied install there is used without a lock,
    // and nothing is ever installed into it.
    if !is_primary_install(dir) {
        return None;
    }
    // Kept beside the installation, in its store: installs directories that share
    // a store (MISE_INSTALL_STORE_DIR) then share the lock too. With the store
    // in the installs directory this is the catalog's own lock directory.
    let store = store_of(&dirs::INSTALLS);
    let locks = if same_path(&store, &dirs::INSTALLS) {
        store.join(".mise").join("locks")
    } else {
        store.join(".mise-locks")
    };
    Some(locks.join(format!("{name}.lock")))
}

/// Whether the version slot `path` holds something this layout did not put there:
/// a real directory (a legacy installation) or a link that is neither a
/// compatibility link into the layout nor a runtime alias (`mise link`).
fn is_foreign_slot(path: &Path) -> bool {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return false;
    };
    if meta.is_dir() && !meta.file_type().is_symlink() && !is_dir_link(path) {
        return true;
    }
    (meta.file_type().is_symlink() || is_dir_link(path))
        && !is_compat_link_shape(path)
        && !crate::runtime_symlinks::is_runtime_symlink(path)
}

/// What [`allocate`] decided.
#[derive(Clone, Debug)]
pub(crate) struct Allocated {
    pub(crate) record: IdentityRecord,
    pub(crate) dir: PathBuf,
    /// An existing installation that already satisfies the request: nothing
    /// needs to be installed, only recorded.
    pub(crate) reused: bool,
    /// The installation is in a read-only shared root, or is a variant standing
    /// in for a bare version: it may be used, never written to. Installing means
    /// allocating again, for the request itself in the primary root.
    pub(crate) read_only: bool,
    /// The unlocked request this allocation answers; set when its selection
    /// should be (re)recorded on success. `None` for locked requests, which
    /// never touch a selection.
    pub(crate) selects: Option<InstallIdentity>,
}

/// The installation made before the identity layout, in the user's own installs
/// root, that `tv` uses in place, if there is one. Installing `tv` again
/// refreshes it there, so the install's path must stay on it even after a forced
/// reinstall has removed it.
pub(crate) fn legacy_in_place(tv: &ToolVersion) -> Option<PathBuf> {
    if !applies_to(tv) {
        return None;
    }
    let located = locate(tv)?;
    let in_own_root = located
        .dir
        .parent()
        .and_then(Path::parent)
        .is_some_and(is_primary_root);
    (located.installed && located.record.is_none() && in_own_root).then_some(located.dir)
}

/// What a command watching the installs it runs is told about each one: the
/// directory an install is about to write into, its canonical backend and its
/// version. An error stops that install before it writes anything.
pub type WriteObserver = Box<dyn Fn(&Path, &str, &str) -> Result<()> + Send>;

static WRITE_OBSERVER: std::sync::Mutex<Option<WriteObserver>> = std::sync::Mutex::new(None);

/// Have `observer` told about every installation directory an install in this
/// process is about to write into, once it holds that directory's lock and has
/// found it has to write; `None` stops it. `mise installs migrate` records
/// them, so that undoing an interrupted migration withdraws exactly what it
/// made.
pub fn observe_writes(observer: Option<WriteObserver>) {
    *WRITE_OBSERVER.lock().unwrap_or_else(|e| e.into_inner()) = observer;
}

/// Tell the observer (see [`observe_writes`]) that an install is about to write
/// into `allocated`.
pub(crate) fn note_writing(allocated: &Allocated) -> Result<()> {
    if allocated.read_only {
        return Ok(());
    }
    match WRITE_OBSERVER
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
    {
        Some(observer) => observer(
            &allocated.dir,
            &allocated.record.identity.backend,
            &allocated.record.identity.version,
        ),
        None => Ok(()),
    }
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

    if let Some(located) = locate(tv)
        && located.installed
    {
        match located.record {
            // A legacy installation (or a `mise link`) is used, and refreshed, in
            // place; it is never moved into the identity layout.
            None => return Ok(None),
            // A variant standing in for a bare version (see `locate`) is used where
            // it is, never installed into or selected for that version: installing
            // makes the bare request an installation of its own.
            Some(record) if request_of(&record.identity) != request_of(&identity) => {
                if !refresh {
                    return Ok(Some(Allocated {
                        read_only: true,
                        dir: located.dir,
                        reused: true,
                        selects: None,
                        record,
                    }));
                }
            }
            // An installation that already satisfies the request, possibly in a
            // read-only shared root, is reused as it is.
            Some(record) if !refresh => {
                return Ok(Some(Allocated {
                    read_only: !is_primary_root(&located.root),
                    dir: located.dir,
                    reused: true,
                    selects: (!locked).then_some(key),
                    record,
                }));
            }
            Some(_) => {}
        }
    }

    let existing = if locked {
        pinned_candidates(&catalog, &identity).into_iter().next()
    } else {
        // An installation in another root is never written to: a refresh, or one
        // that is missing, installs into the user's own root.
        let own = |located: Located| {
            if is_primary_root(&located.root) {
                located.record
            } else {
                catalog.lookup(&key)
            }
        };
        match unlocked_choice(&key) {
            // The installation selected in another root is gone: say so rather than
            // quietly installing a replacement and moving every project's selection.
            Unlocked::Selected(located) if !refresh && !is_primary_root(&located.root) => {
                eyre::bail!(
                    "the installation selected for {} is in {}, which no longer has it. \
                     Select another with `mise installs select <dir>`, or install a fresh one \
                     with `mise install --force {}@{}`",
                    tv.style(),
                    located.root.display(),
                    tv.ba().short,
                    tv.version
                );
            }
            Unlocked::Selected(located) | Unlocked::Found(located) => own(located),
            Unlocked::Ambiguous(records) if !refresh => {
                return Err(ambiguity_error(tv, &records));
            }
            // A refresh is the explicit choice: it installs into the request's own
            // directory and selects that.
            Unlocked::Ambiguous(_) | Unlocked::Nothing => catalog.lookup(&key),
        }
    };
    if let Some(existing) = existing {
        let pinned_elsewhere = !existing.provenance.pinned_by.is_empty() && !locked;
        if !(refresh && pinned_elsewhere) {
            return Ok(Some(Allocated {
                read_only: false,
                dir: catalog.install_dir(&existing),
                reused: true,
                selects: (!locked).then_some(key),
                record: existing,
            }));
        }
        // Move the unlocked selection to a new generation.
        let generation = next_generation(&catalog, &key);
        let mut next = key.clone();
        next.inputs
            .insert("generation".into(), generation.to_string());
        let mut record = catalog.allocate(&next)?;
        record.provenance.generation = generation;
        catalog.update_provenance(&next, record.provenance.clone())?;
        return Ok(Some(Allocated {
            read_only: false,
            dir: catalog.install_dir(&record),
            reused: false,
            selects: Some(key),
            record,
        }));
    }
    let record = catalog.allocate(&identity)?;
    Ok(Some(Allocated {
        read_only: false,
        dir: catalog.install_dir(&record),
        reused: false,
        selects: (!locked).then_some(key),
        record,
    }))
}

/// Record a finished (or reused) installation: write its receipt, point the
/// compatibility link for the requesting tool at it, and remember the unlocked
/// selection. The receipt is what marks the installation complete. It goes in
/// before the link, so nothing that finds the link takes it for a stale one; when
/// a later step fails, the caller withdraws the installation with [`unpublish`].
///
/// `adopt_new_artifact` is an explicit refresh (`--force`, a rolling update). Without
/// it, an installation restored under a recorded identity must be made from the
/// artifact that identity recorded: a different one is an error, never a silent
/// replacement of what the selection stood for.
pub(crate) fn finish(
    tv: &ToolVersion,
    allocated: &Allocated,
    adopt_new_artifact: bool,
) -> Result<()> {
    let catalog = Catalog::new(dirs::INSTALLS.to_path_buf());
    let mut record = allocated.record.clone();
    let acquired = tv
        .backend()
        .ok()
        .map(|b| b.get_platform_key())
        .and_then(|platform| tv.lock_platforms.get(&platform))
        .and_then(|p| p.checksum.clone());
    if allocated.reused && !adopt_new_artifact {
        check_restored_artifact(tv, &record, acquired.as_deref())?;
    }
    if let Some(checksum) = acquired {
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
    reset_cache();
    Ok(())
}

/// Fail when the artifact just acquired for a restored installation is not the one
/// its record names. Compares only what both sides know; an installation recorded
/// without a checksum (a plugin that does not report one) has nothing to compare.
fn check_restored_artifact(
    tv: &ToolVersion,
    record: &IdentityRecord,
    acquired: Option<&str>,
) -> Result<()> {
    let (Some(recorded), Some(acquired)) = (
        record
            .provenance
            .artifacts
            .get("checksum")
            .map(String::as_str),
        acquired,
    ) else {
        return Ok(());
    };
    if recorded == acquired {
        return Ok(());
    }
    eyre::bail!(
        "{} was installed from an artifact with checksum {recorded}, but the artifact now \
         available has checksum {acquired}. Run `mise install --force {}@{}` to adopt the new \
         artifact deliberately",
        tv.style(),
        tv.ba().short,
        tv.version
    )
}

/// Withdraw an installation [`finish`] published: remove its receipt (so it is
/// incomplete again) and the version links that name it. The catalog record and
/// the directory name stay, so a retry lands in the same place.
pub fn unpublish(dir: &Path) {
    if let Err(err) = file::remove_file(dir.join(RECEIPT_FILE)) {
        debug!("could not remove the receipt of {}: {err:#}", dir.display());
    }
    unlink_installation(dir);
    reset_cache();
}

/// [`note_reuse`] for a request found already satisfied, where nothing else will
/// visit the install: a failure is only worth a debug line.
pub(crate) fn note_satisfied(tv: &ToolVersion) {
    if let Err(err) = note_reuse(tv) {
        debug!("could not record the use of {}: {err:#}", tv.style());
    }
}

/// Use an installation that already satisfies `tv` (found by [`locate`]): make
/// sure the requesting tool has its compatibility link and, for a pinned
/// request that adopted an unlocked installation, remember the pin so a later
/// refresh does not replace it in place.
pub(crate) fn note_reuse(tv: &ToolVersion) -> Result<()> {
    // `tv` may already carry the path an install chose; what is needed here is
    // the installation its identity resolves to.
    let mut bare = tv.clone();
    bare.install_path = None;
    let Some(located) = locate(&bare) else {
        return Ok(());
    };
    let (true, Some(record)) = (located.installed, located.record) else {
        return Ok(());
    };
    let Some(identity) = identity_of(tv) else {
        return Ok(());
    };
    let catalog = Catalog::new(dirs::INSTALLS.to_path_buf());
    // An unlocked request that found its installation with nothing selected (one
    // a lockfile, another spelling or a shared root made) remembers it, so the
    // choice stays put when more installations of the request appear.
    let key = identity.request_key();
    // A variant standing in for a bare request named on the command line is not
    // its selection.
    if pin_of(&identity).is_none() && same_request(&record.identity, &key) {
        let shared = (!is_primary_root(&located.root)).then_some(located.root.as_path());
        if catalog.select_if_unset(&key, &record, shared)? {
            reset_cache();
        }
    }
    // A pin is recorded in the catalog that lists the installation, and a shared
    // root's is never written to. The version link is the user's own.
    if is_primary_root(&located.root)
        && let Some(pin) = pin_of(&identity)
        && !record.provenance.pinned_by.iter().any(|p| p == pin)
    {
        let mut provenance = record.provenance.clone();
        provenance.pinned_by.push(pin.to_string());
        catalog.update_provenance(&record.identity, provenance)?;
        reset_cache();
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
    let target = link_value(&tool_dir, dir);
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

/// What a version link in `tool_dir` stores to name the installation `dir`:
/// `../<name>` when installations sit beside the tool directories, a path through
/// their common parent when the store is a sibling of the installs root (Windows'
/// `i` beside `installs`), else the installation's absolute path.
fn link_value(tool_dir: &Path, dir: &Path) -> PathBuf {
    use path_absolutize::Absolutize;
    // A relative store (`MISE_INSTALL_STORE_DIR=store`) would be read from the
    // link's own directory, so a target that is not relative to it is absolute.
    let absolute = || {
        dir.absolutize()
            .map_or_else(|_| dir.to_path_buf(), |p| p.into_owned())
    };
    let name = dir.file_name().unwrap_or_default();
    let (Some(root), Some(store)) = (tool_dir.parent(), dir.parent()) else {
        return absolute();
    };
    if same_path(root, store) {
        return Path::new("..").join(name);
    }
    // The sibling form `../../<store>/<name>` has to be right both ways it is
    // read: lexically, by mise, which recognizes a version link by its target's
    // directory being the configured store; and physically, by everything that
    // follows the link, where `..` through a symlinked installs directory leaves
    // the symlink's target. So the configured directories must be siblings, and
    // so must the physical ones, under the same store name. Anything else gets
    // an absolute target.
    let (Ok(physical_root), Ok(physical_store)) = (root.canonicalize(), store.canonicalize())
    else {
        return absolute();
    };
    let siblings = matches!(
        (root.parent(), store.parent()),
        (Some(a), Some(b)) if same_path(a, b)
    ) && physical_root.parent().is_some()
        && physical_root.parent() == physical_store.parent()
        && physical_store.file_name() == store.file_name();
    match store.file_name() {
        Some(store_name) if siblings => Path::new("..").join("..").join(store_name).join(name),
        _ => absolute(),
    }
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

/// Recreate the version links of a tool's installations that have none: the tool
/// directory was removed by hand, or the installation was made through another
/// spelling of the tool. A slot that exists is left alone, because a link can name
/// only one variant. A missing one names the selected installation of its version,
/// in whichever root that is, or else the first of the user's own.
pub(crate) fn heal_links(ba: &crate::args::BackendArg) {
    let tool_dir = ba.installs_path();
    let installs = installs_of(ba);
    if installs
        .iter()
        .all(|(name, _)| std::fs::symlink_metadata(tool_dir.join(name)).is_ok())
    {
        return;
    }
    let primary = Catalog::new(dirs::INSTALLS.to_path_buf());
    // Under the catalog lock `mise installs select` moves links with, so a choice
    // made meanwhile is not overwritten with an older one.
    let _lock = match primary.lock() {
        Ok(lock) => lock,
        Err(err) => {
            debug!("could not lock the installs catalog to repair links: {err:#}");
            return;
        }
    };
    let mut missing: indexmap::IndexMap<String, (PathBuf, bool)> = indexmap::IndexMap::new();
    for (name, dir) in installs {
        if std::fs::symlink_metadata(tool_dir.join(&name)).is_ok() {
            continue;
        }
        let selected = is_selected_install(&primary, &dir);
        if !selected && !is_primary_install(&dir) {
            continue;
        }
        match missing.get(&name) {
            Some((_, true)) => {}
            Some((_, false)) if !selected => {}
            _ => {
                missing.insert(name, (dir, selected));
            }
        }
    }
    for (name, (dir, _)) in missing {
        let slot = tool_dir.join(&name);
        if let Err(err) = file::create_dir_all(tool_dir)
            .and_then(|()| file::make_dir_link(&link_value(tool_dir, &dir), &slot))
        {
            debug!("could not link {}: {err:#}", slot.display());
        }
    }
}

/// Whether the installation at `dir` is the one its request's selection names.
fn is_selected_install(primary: &Catalog, dir: &Path) -> bool {
    let (Some(store), Some(name)) = (dir.parent(), dir.file_name().and_then(|n| n.to_str())) else {
        return false;
    };
    let Some(root) = root_of_store(store) else {
        return false;
    };
    describe(primary, &Catalog::new(&root), &root, name).is_some_and(|i| i.selected)
}

/// The installation directory a compatibility link names, if `slot` is one: a
/// link whose target is a direct child of an install store holding a receipt.
pub fn link_target(slot: &Path) -> Option<PathBuf> {
    let target = file::resolve_symlink(slot).ok().flatten()?;
    let target = if target.is_absolute() {
        target
    } else {
        slot.parent()?.join(target)
    };
    // Lexical, not canonical: a canonical path on Windows carries a verbatim
    // prefix the configured roots do not, so the same installation would no
    // longer compare equal to its own directory.
    use path_absolutize::Absolutize;
    let target = target.absolutize().ok()?.into_owned();
    let parent = target.parent()?;
    let is_store = stores().iter().any(|s| same_path(s, parent));
    (is_store && target.join(RECEIPT_FILE).exists()).then_some(target)
}

/// One installation of the identity layout, as `mise installs` shows it.
#[derive(Clone, Debug, serde::Serialize)]
pub struct Installation {
    /// The directory name, `<label>-<hash>`.
    pub name: String,
    pub dir: PathBuf,
    /// The canonical backend it was installed from.
    pub backend: String,
    pub version: String,
    pub platform: String,
    /// `fallback` (identified by its request) or `resolved` (a pinned artifact).
    pub mode: String,
    /// The install-affecting options it was made with.
    pub options: BTreeMap<String, String>,
    /// The spelling it was first requested as.
    pub requested_as: Option<String>,
    /// The checksum of the artifact it was installed from, when known.
    pub checksum: Option<String>,
    /// A lockfile adopted it: a refresh never replaces it in place.
    pub pinned: bool,
    /// Unlocked requests for its tool, version, platform and options use it.
    pub selected: bool,
    /// It is in a read-only shared installs directory.
    pub shared: bool,
}

/// Every complete installation of the identity layout, in every installs root,
/// ordered by name within each root.
pub fn installations() -> Vec<Installation> {
    let primary = Catalog::new(dirs::INSTALLS.to_path_buf());
    let mut out = vec![];
    for root in roots() {
        let catalog = Catalog::new(&root);
        for name in file::dir_subdirs(catalog.store()).unwrap_or_default() {
            if !has_hash_suffix(&name) {
                continue;
            }
            if let Some(found) = describe(&primary, &catalog, &root, &name) {
                out.push(found);
            }
        }
    }
    out
}

/// Whether the installation `i` is one of `ba`'s, judged from its receipt (so an
/// installation the catalog lost, or one in a shared root, still counts).
pub fn installation_belongs_to(ba: &crate::args::BackendArg, i: &Installation) -> bool {
    if !backends_of(ba).contains(&i.backend) {
        return false;
    }
    read_receipt(&i.dir).is_some_and(|receipt| belongs_to(ba, &receipt.record, &i.dir))
}

/// The installation named `name` in the store of `root`, if there is a complete one.
fn describe(primary: &Catalog, catalog: &Catalog, root: &Path, name: &str) -> Option<Installation> {
    let dir = catalog.store().join(name);
    let receipt = read_receipt(&dir)?;
    // The catalog's record carries provenance added after the receipt was written.
    let record = catalog
        .record_by_digest(&receipt.record.identity.backend, &receipt.record.digest)
        .unwrap_or(receipt.record);
    let shared = !is_primary_root(root);
    let selected = primary
        .selection(&request_of(&record.identity))
        .is_some_and(|s| {
            s.selected == record.digest
                && match &s.root {
                    Some(r) => shared && same_path(Path::new(r), root),
                    None => !shared,
                }
        });
    let identity = &record.identity;
    Some(Installation {
        name: name.to_string(),
        dir,
        backend: identity.backend.clone(),
        version: identity.version.clone(),
        platform: identity.platform.clone(),
        mode: match identity.mode {
            Mode::Resolved => "resolved",
            Mode::Fallback => "fallback",
        }
        .to_string(),
        options: identity.options.clone(),
        requested_as: receipt.requested_as,
        checksum: identity
            .inputs
            .get("artifact.checksum")
            .or_else(|| record.provenance.artifacts.get("checksum"))
            .cloned(),
        pinned: !record.provenance.pinned_by.is_empty(),
        selected,
        shared,
    })
}

/// Make the installation `name` (a directory name, or its path) the one that
/// unlocked requests for its tool, version, platform and options use, in every
/// project on this machine, and point the version links that named another
/// installation of that request at it. Locked requests are not affected.
pub fn select(installation: &str) -> Result<Installation> {
    let path = Path::new(installation);
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(installation)
        .to_string();
    // A path names its installs root; a bare name may be in several.
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty());
    let primary = Catalog::new(dirs::INSTALLS.to_path_buf());
    let mut found: Vec<(PathBuf, Catalog)> = roots()
        .into_iter()
        .map(|root| {
            let catalog = Catalog::new(&root);
            (root, catalog)
        })
        .filter(|(root, catalog)| {
            parent.is_none_or(|p| same_path(p, catalog.store()))
                && describe(&primary, catalog, root, &name).is_some()
        })
        .collect();
    if found.len() > 1 {
        let paths = found
            .iter()
            .map(|(_, c)| format!("  {}", c.store().join(&name).display()))
            .collect::<Vec<_>>()
            .join("\n");
        eyre::bail!("{name} is in more than one installs directory; pass its path:\n{paths}");
    }
    let Some((root, catalog)) = found.pop() else {
        eyre::bail!("{installation} is not an installation; `mise installs ls` lists them");
    };
    let dir = catalog.store().join(&name);
    let receipt = read_receipt(&dir).ok_or_else(|| eyre::eyre!("{name} has no receipt"))?;
    // A selection is resolved through the catalog that lists the installation.
    // The user's own catalog is rebuilt from receipts if it lost the record; a
    // shared root's is read-only, so its installation's receipt stands in for
    // the record, as it does when the selection is used.
    let lookup =
        || catalog.record_by_digest(&receipt.record.identity.backend, &receipt.record.digest);
    let record = match lookup() {
        Some(record) => record,
        None if is_primary_root(&root) => {
            catalog.rebuild_from_receipts()?;
            lookup().ok_or_else(|| eyre::eyre!("{name} could not be added to the catalog"))?
        }
        // The receipt of the very directory being selected.
        None if receipt.record.dir == name && receipt.record.is_consistent() => {
            receipt.record.clone()
        }
        None => eyre::bail!(
            "{} is not listed in the catalog of {}, so it cannot be selected",
            name,
            root.display()
        ),
    };
    let key = request_of(&record.identity);
    let shared = (!is_primary_root(&root)).then_some(root.as_path());
    // The version links follow the selection under the same lock, so two
    // selections made at once cannot leave the links on the other one.
    let lock = primary.lock()?;
    primary.write_selection(&key, &record, shared)?;
    retarget_links(&record, &dir, receipt.requested_as.as_deref());
    drop(lock);
    reset_cache();
    describe(&primary, &catalog, &root, &name)
        .ok_or_else(|| eyre::eyre!("{name} is no longer installed"))
}

/// Forget the unlocked selections that name the installation `dir`, after
/// `mise uninstall` removed it on purpose: the requests it answered choose
/// again (another installation, or a legacy or shared copy). Prune keeps them,
/// so a pruned selection is restored in place.
pub fn forget_selections_of(dir: &Path) {
    if !enabled() || !is_primary_install(dir) {
        return;
    }
    let Some(name) = dir.file_name().and_then(|n| n.to_str()) else {
        return;
    };
    let catalog = Catalog::new(dirs::INSTALLS.to_path_buf());
    let Some(digest) = catalog.owner_of(name) else {
        return;
    };
    if let Err(err) = catalog.remove_selections_of(&digest) {
        debug!(
            "could not forget the selections of {}: {err:#}",
            dir.display()
        );
    }
    reset_cache();
}

/// Point every version link in the primary root that names another installation
/// of `record`'s request at `dir` (which may be in a shared root) instead, and
/// give the tool it was requested as a link if it has none. A link can name only
/// one variant; after a selection it should name the selected one. Nothing is
/// written to a shared root.
fn retarget_links(record: &IdentityRecord, dir: &Path, requested_as: Option<&str>) {
    let root: &Path = &dirs::INSTALLS;
    let key = request_of(&record.identity);
    let name = listing_name(record);
    let requested_dir =
        requested_as.map(|short| root.join(crate::backend::tool_directory_name(short)));
    let mut tool_dirs: Vec<PathBuf> = file::dir_subdirs(root)
        .unwrap_or_default()
        .into_iter()
        .filter(|tool| !is_reserved_dir(root, tool))
        .map(|tool| root.join(tool))
        .collect();
    // The tool it was requested as gets its link even when its directory is gone.
    if let Some(dir) = &requested_dir
        && !tool_dirs.contains(dir)
    {
        tool_dirs.push(dir.clone());
    }
    for tool_dir in tool_dirs {
        let slot = tool_dir.join(&name);
        let requested = requested_dir.as_ref() == Some(&tool_dir);
        let retarget = match link_target(&slot) {
            // In the tool's own directory the link follows the selection from any
            // installation of the version (another variant included); in other
            // tool directories (an alias with other options) only from another
            // installation of the same request.
            Some(current) => {
                !same_path(&current, dir)
                    && (requested
                        || read_receipt(&current)
                            .is_some_and(|r| same_request(&r.record.identity, &key)))
            }
            // An empty slot, or a version link whose installation is gone, in the
            // directory of the tool it was requested as. Anything else there (a
            // legacy installation, a `mise link`) is left alone.
            None => {
                requested
                    && (std::fs::symlink_metadata(&slot).is_err() || is_compat_link_shape(&slot))
            }
        };
        if !retarget {
            continue;
        }
        if let Err(err) = file::create_dir_all(&tool_dir)
            .and_then(|()| file::make_dir_link(&link_value(&tool_dir, dir), &slot))
        {
            warn!(
                "could not link {} to {}: {err:#}",
                slot.display(),
                dir.display()
            );
        }
    }
}

/// Whether `dir` is a legacy installation in the primary installs root: a real
/// directory at `installs/<tool>/<version>`, not a link of any kind.
pub fn is_legacy_install(dir: &Path) -> bool {
    let in_primary_tool_dir = dir
        .parent()
        .and_then(Path::parent)
        .is_some_and(is_primary_root);
    in_primary_tool_dir
        && !is_dir_link(dir)
        && std::fs::symlink_metadata(dir).is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink())
}

/// The canonical backend and version an installation of `tv` records, whatever
/// options it is made with.
pub fn identity_scope(tv: &ToolVersion) -> Option<(String, String)> {
    let mut bare = tv.clone();
    bare.install_path = None;
    let identity = identity_of(&bare)?;
    Some((identity.backend, identity.version))
}

/// Where reinstalling `tv`'s legacy installation into this layout would put it,
/// or why it would not be moved: the layout does not cover it, or the backend
/// recorded for the legacy directory is not the one the request resolves to now
/// (so it is not what the request means, and installing would not replace it).
pub fn migration_target(tv: &ToolVersion) -> std::result::Result<PathBuf, String> {
    let mut bare = tv.clone();
    bare.install_path = None;
    if !applies_to(&bare) {
        return Err("it keeps the legacy layout".into());
    }
    let identity = identity_of(&bare).ok_or("its backend cannot be loaded")?;
    if !crate::toolset::install_state::legacy_backend_matches(
        &dirs::INSTALLS,
        &tv.ba().short,
        &identity.backend,
    ) {
        return Err(format!(
            "it was installed from another backend than {} now resolves to ({}); \
             uninstall it if nothing uses it",
            tv.ba().short,
            identity.backend
        ));
    }
    match locate(&bare) {
        Some(located) if located.record.is_some() => Ok(located.dir),
        _ => {
            let catalog = Catalog::new(dirs::INSTALLS.to_path_buf());
            Ok(match catalog.lookup(&identity) {
                Some(record) => catalog.install_dir(&record),
                None => catalog.tentative_dir(&identity),
            })
        }
    }
}

/// Move `from`, an existing installation of `tv`'s version, into the directory
/// the identity layout allocates for it, give it the receipt and selection a
/// fresh install would, and link `installs/<tool>/<version>` to it. Nothing
/// inside is rewritten; the caller keeps the old path resolving with that link.
///
/// The move is a rename: it fails (and changes nothing) where the directory
/// cannot be renamed, for instance across file systems.
pub fn relocate(tv: &ToolVersion, from: &Path) -> Result<PathBuf> {
    let mut bare = tv.clone();
    bare.install_path = None;
    let allocated = allocate(&bare, false)?
        .filter(|a| !a.read_only)
        .ok_or_else(|| eyre::eyre!("the identity layout does not govern it"))?;
    let _lock = lock_install(&allocated.dir, &|_| {})?;
    // Under the lock: an install that finished there while this waited is
    // someone else's, and is neither written to nor recorded as ours. What a
    // failed reinstall left there is not.
    clear_leftover(&allocated.dir)?;
    // Recorded before anything moves, so an interrupted move is put back.
    note_writing(&allocated)?;
    file::rename(from, &allocated.dir)?;
    finish(&bare, &allocated, false)?;
    Ok(allocated.dir)
}

/// Clear the way for a relocation into `dir`, which the caller holds the
/// install lock of. Nothing is there, or only what an install that failed left
/// (a real directory with no receipt: the lock means nobody is still writing
/// it) and that is removed. A finished installation, or anything that is not a
/// plain directory, is never touched and fails the relocation.
fn clear_leftover(dir: &Path) -> Result<()> {
    let Ok(meta) = std::fs::symlink_metadata(dir) else {
        return Ok(());
    };
    if !meta.is_dir() || meta.file_type().is_symlink() || is_complete(dir) {
        eyre::bail!("{} already exists", dir.display());
    }
    file::remove_all(dir)
}

/// Make sure `installs/<tool>/<version>` links to the installation `dir` (which
/// may be in a shared root), as [`link`] does after an install. An installation
/// that was reused rather than installed may not have made one.
pub fn ensure_version_link(tv: &ToolVersion, dir: &Path) -> Result<()> {
    link(tv, dir)
}

/// The complete installation of this layout that `tv`'s request resolves to,
/// ignoring any path `tv` already carries.
pub fn installation_of(tv: &ToolVersion) -> Option<PathBuf> {
    let mut bare = tv.clone();
    bare.install_path = None;
    reset_cache();
    locate(&bare)
        .filter(|located| located.installed && located.record.is_some())
        .map(|located| located.dir)
}

/// Adopt installations whose receipt is on disk but that the primary catalog does
/// not know (it was lost, or a mise that predates it made them), so the catalog can
/// always be rebuilt from receipts alone. `root_entries` are the names in the
/// primary installs root, already read by the caller; a separate store is read here.
pub(crate) fn adopt_unrecorded_installs(root_entries: &std::collections::BTreeSet<String>) {
    let catalog = Catalog::new(dirs::INSTALLS.to_path_buf());
    let store_entries;
    let entries = if same_path(catalog.store(), catalog.root()) {
        root_entries
    } else {
        store_entries = file::dir_subdirs(catalog.store()).unwrap_or_default();
        &store_entries
    };
    let unrecorded = entries.iter().any(|name| {
        has_hash_suffix(name)
            && catalog.store().join(name).join(RECEIPT_FILE).exists()
            && !catalog.is_reserved(name)
    });
    if unrecorded && let Err(err) = catalog.rebuild_from_receipts() {
        warn!("failed to rebuild the install catalog from receipts: {err:#}");
    }
}

/// Whether `name` is the receipt file an installation directory holds.
pub fn is_receipt_name(name: &std::ffi::OsStr) -> bool {
    name == RECEIPT_FILE
}

/// Whether the entry `name` of an installs `root` is not a tool directory: the
/// catalog (`.mise`, or any hidden entry) or an identity-layout installation,
/// told apart by its reservation or its receipt rather than by how it is named.
pub(crate) fn is_reserved_dir(root: &Path, name: &str) -> bool {
    // Every name the catalog assigns ends in a hash, so a tool directory is told
    // apart without touching the disk.
    name.starts_with('.')
        || (has_hash_suffix(name)
            && (root.join(".mise").join("names").join(name).exists()
                || root.join(name).join(RECEIPT_FILE).exists()))
}

/// Whether `path` is a directory link: a symlink, or a junction on Windows.
pub(crate) fn is_dir_link(path: &Path) -> bool {
    file::is_symlink_or_junction(path)
}

/// Whether `path` is shaped like a compatibility link: a link to a direct child
/// of the store of the installs root that contains it (`../<name>-<hash>`). It
/// says nothing about whether that installation is still there.
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
    let (Some(store), Some(root)) = (target.parent(), tool_dir.parent()) else {
        return false;
    };
    // Its own root's store, or (for a selection of a shared installation) the
    // store of another root.
    (same_path(store, &store_of(root)) || stores().iter().any(|s| same_path(store, s)))
        && target
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(has_hash_suffix)
}

/// Whether `name` ends in `-<base32 digest prefix>` the way an allocated
/// installation directory does (at least 8 lowercase base32 characters).
pub(crate) fn has_hash_suffix(name: &str) -> bool {
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
/// direct child of an install store) rather than a legacy `<tool>/<version>`.
pub fn dir_name_of(path: &Path) -> Option<String> {
    // The name is checked first: it costs nothing, and listing the installs roots
    // reads settings and the disk on every call, which hot paths make often.
    let name = path.file_name()?.to_str()?;
    if !has_hash_suffix(name) {
        return None;
    }
    let parent = path.parent()?;
    stores()
        .iter()
        .any(|r| same_path(r, parent))
        .then(|| name.to_string())
}

#[cfg(test)]
mod tests {
    #[test]
    fn clear_leftover_removes_only_what_a_failed_install_left() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("tool-abcdefgh");
        assert!(super::clear_leftover(&dir).is_ok());
        // a failed reinstall: payload, no receipt
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        std::fs::write(dir.join("bin/x"), "").unwrap();
        assert!(super::clear_leftover(&dir).is_ok());
        assert!(!dir.exists());
        // a finished install is never touched
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join(super::RECEIPT_FILE), "").unwrap();
        if super::is_complete(&dir) {
            assert!(super::clear_leftover(&dir).is_err());
            assert!(dir.exists());
        }
        #[cfg(unix)]
        {
            let link = tmp.path().join("dangling");
            std::os::unix::fs::symlink(tmp.path().join("nowhere"), &link).unwrap();
            assert!(super::clear_leftover(&link).is_err());
        }
    }

    use super::*;

    #[test]
    fn windows_spellings_of_one_root_compare_equal() {
        let a = windows_comparable(Path::new(r"C:\Users\Me\AppData\Local\mise\installs"));
        assert_eq!(
            a,
            windows_comparable(Path::new(r"\\?\c:\users\me\appdata\local\mise\installs\"))
        );
        assert_eq!(
            a,
            windows_comparable(Path::new("C:/Users/Me/AppData/Local/mise/installs"))
        );
        assert_ne!(
            a,
            windows_comparable(Path::new(r"C:\Users\Me\other\installs"))
        );
        // UNC and WSL shares: the verbatim spelling is the same place.
        assert_eq!(
            windows_comparable(Path::new(r"\\?\UNC\server\Share\mise\installs")),
            windows_comparable(Path::new(r"\\server\share\mise\installs"))
        );
        assert_eq!(
            windows_comparable(Path::new(r"\\?\UNC\wsl$\Ubuntu\home\me\installs\")),
            windows_comparable(Path::new(r"\\WSL$\ubuntu\home\me\installs"))
        );
        assert_ne!(
            windows_comparable(Path::new(r"\\server\share")),
            windows_comparable(Path::new(r"C:\server\share"))
        );
    }

    #[test]
    fn credentials_never_reach_identity_values() {
        assert_eq!(
            redact_credentials("https://user:tok@registry.example.com/simple/"),
            "https://registry.example.com/simple/"
        );
        assert_eq!(
            redact_credentials("git+https://github.com/o/r.git#main"),
            "git+https://github.com/o/r.git#main"
        );
        assert_eq!(
            redact_credentials("git+ssh://git@github.com:org/repo.git"),
            "git+ssh://github.com:org/repo.git"
        );
        let percent_encoded = "git+https://user%40example.test:token%2Fvalue@host.example/o/r.git?access_token=query-token";
        let redacted_percent_encoded = redact_credentials(percent_encoded);
        for secret in [
            "user%40example.test",
            "token%2Fvalue",
            "access_token=query-token",
            "query-token",
        ] {
            assert!(
                !redacted_percent_encoded.contains(secret),
                "redacted URL leaked {secret:?}: {redacted_percent_encoded}"
            );
        }
        assert!(redacted_percent_encoded.starts_with("git+https://host.example/o/r.git?"));
        let query = |q: &str| digest_of_string(q)[..16].to_string();
        assert_eq!(
            redact_credentials("https://tok@host.example/a?b=c@d"),
            format!("https://host.example/a?{}", query("b=c@d"))
        );
        // Signed URLs and API keys travel in the query string, which is kept only
        // as a digest...
        assert_eq!(
            redact_credentials("https://bucket.example/t.tgz?X-Amz-Signature=s#frag"),
            format!(
                "https://bucket.example/t.tgz?{}#frag",
                query("X-Amz-Signature=s")
            )
        );
        assert_eq!(
            redact_credentials("https://api.example?token=t other"),
            format!("https://api.example?{} other", query("token=t"))
        );
        // ...because a query can also choose what is downloaded.
        assert_ne!(
            redact_credentials("https://example.com/patch?id=1"),
            redact_credentials("https://example.com/patch?id=2")
        );
        assert_eq!(redact_credentials("plain value?x=1"), "plain value?x=1");
        assert_eq!(redact_credentials("plain value"), "plain value");
        assert_eq!(redact_credentials("https://host/a@b"), "https://host/a@b");
        assert_eq!(
            redact_credentials("--index-url https://u:p@h.example/x --other https://h2/y"),
            "--index-url https://h.example/x --other https://h2/y"
        );
    }

    /// An installation in the real installs root of the test environment with a
    /// unique name, removed on drop, plus a tool directory beside it.
    struct Fixture {
        install: PathBuf,
        tool_dir: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let unique = format!(
                "{}{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            );
            let id = InstallIdentity {
                backend: format!("core:fixture{unique}"),
                version: "1.2.3".into(),
                platform: "linux-x64".into(),
                ..Default::default()
            };
            let catalog = Catalog::new(dirs::INSTALLS.to_path_buf());
            let record = catalog.allocate(&id).unwrap();
            let install = catalog.install_dir(&record);
            std::fs::create_dir_all(install.join("bin")).unwrap();
            write_receipt(
                &install,
                &Receipt {
                    record,
                    requested_as: None,
                    mise_version: None,
                },
            )
            .unwrap();
            let tool_dir = dirs::INSTALLS.join(format!("fixture-tool-{unique}"));
            std::fs::create_dir_all(&tool_dir).unwrap();
            Self { install, tool_dir }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.install);
            let _ = std::fs::remove_dir_all(&self.tool_dir);
        }
    }

    #[test]
    fn a_version_link_names_its_installation_and_an_alias_does_not() {
        let fx = Fixture::new();
        let version = fx.tool_dir.join("1.2.3");
        let alias = fx.tool_dir.join("1");
        file::make_dir_link(&link_value(&fx.tool_dir, &fx.install), &version).unwrap();
        file::make_dir_link(Path::new("./1.2.3"), &alias).unwrap();

        assert!(is_compat_link_shape(&version));
        assert_eq!(link_target(&version), Some(fx.install.clone()));
        assert_eq!(
            dir_name_of(&fx.install).as_deref(),
            fx.install.file_name().unwrap().to_str()
        );
        // A runtime alias points at a sibling inside the tool directory, never at an
        // installation, even though it resolves to one through the version link.
        assert!(!is_compat_link_shape(&alias));
        assert_eq!(link_target(&alias), None);
        // A link into the store that is not a receipt-bearing installation.
        let stranger = dirs::INSTALL_STORE.join(format!(
            "stranger-{}",
            fx.tool_dir.file_name().unwrap().to_string_lossy()
        ));
        std::fs::create_dir_all(&stranger).unwrap();
        let stray = fx.tool_dir.join("stray");
        file::make_dir_link(&link_value(&fx.tool_dir, &stranger), &stray).unwrap();
        assert_eq!(link_target(&stray), None);
        let _ = std::fs::remove_dir_all(&stranger);
    }

    #[test]
    fn unlinking_an_installation_removes_every_link_to_it_and_only_those() {
        let fx = Fixture::new();
        let other = Fixture::new();
        file::make_dir_link(
            &link_value(&fx.tool_dir, &fx.install),
            &fx.tool_dir.join("1.2.3"),
        )
        .unwrap();
        file::make_dir_link(
            &link_value(&other.tool_dir, &fx.install),
            &other.tool_dir.join("1.2.3"),
        )
        .unwrap();
        file::make_dir_link(
            &link_value(&fx.tool_dir, &other.install),
            &fx.tool_dir.join("9.9.9"),
        )
        .unwrap();

        unlink_installation(&fx.install);

        assert!(std::fs::symlink_metadata(fx.tool_dir.join("1.2.3")).is_err());
        assert!(std::fs::symlink_metadata(other.tool_dir.join("1.2.3")).is_err());
        assert!(
            is_dir_link(&fx.tool_dir.join("9.9.9")),
            "a link to another installation stays"
        );
        assert!(
            fx.install.join("bin").is_dir(),
            "the installation itself is not touched"
        );
    }

    #[test]
    fn removal_is_refused_for_a_directory_mise_did_not_create() {
        let fx = Fixture::new();
        assert!(guard_removal(&fx.install).is_ok());
        let stranger = dirs::INSTALL_STORE.join(format!("stranger-{}", std::process::id()));
        std::fs::create_dir_all(&stranger).unwrap();
        let err = guard_removal(&stranger).unwrap_err().to_string();
        assert!(err.contains("not an installation mise created"), "{err}");
        // A legacy `<tool>/<version>` path is not this function's business.
        assert!(guard_removal(&fx.tool_dir.join("1.2.3")).is_ok());
        let _ = std::fs::remove_dir_all(&stranger);
    }

    #[test]
    fn a_restored_install_must_come_from_the_recorded_artifact() {
        let ba = std::sync::Arc::new(crate::args::BackendArg::from("dummy"));
        let request = ToolRequest::new(ba, "1.0.0", crate::toolset::ToolSource::Argument).unwrap();
        let tv = ToolVersion::new(request, "1.0.0".into());
        let mut record = IdentityRecord::new(
            InstallIdentity {
                backend: "asdf:dummy".into(),
                version: "1.0.0".into(),
                platform: "linux-x64".into(),
                ..Default::default()
            },
            "dummy-aaaaaaaa".into(),
        );
        // Nothing recorded, or nothing acquired: nothing to compare.
        assert!(check_restored_artifact(&tv, &record, Some("sha256:b")).is_ok());
        record
            .provenance
            .artifacts
            .insert("checksum".into(), "sha256:a".into());
        assert!(check_restored_artifact(&tv, &record, None).is_ok());
        assert!(check_restored_artifact(&tv, &record, Some("sha256:a")).is_ok());
        let err = check_restored_artifact(&tv, &record, Some("sha256:b"))
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("sha256:a") && err.contains("sha256:b"),
            "{err}"
        );
        assert!(err.contains("mise install --force dummy@1.0.0"), "{err}");
    }

    #[test]
    fn a_refresh_generation_still_answers_its_request() {
        let key = InstallIdentity {
            backend: "aqua:jqlang/jq".into(),
            version: "1.7.1".into(),
            platform: "linux-x64".into(),
            ..Default::default()
        };
        let mut generation = key.clone();
        generation.inputs.insert("generation".into(), "2".into());
        let mut pinned = generation.clone();
        pinned.mode = Mode::Resolved;
        pinned
            .inputs
            .insert("artifact.checksum".into(), "sha256:a".into());
        assert!(same_request(&key, &key));
        assert!(same_request(&generation, &key));
        assert!(same_request(&pinned, &key));
        let mut other = key.clone();
        other.version = "1.7.2".into();
        assert!(!same_request(&other, &key));
    }

    #[test]
    fn a_relative_root_is_the_same_place_as_its_absolute_spelling() {
        let cwd = std::env::current_dir().unwrap();
        assert!(same_path(
            Path::new("reldata/installs"),
            &cwd.join("reldata/installs")
        ));
        assert!(same_path(
            Path::new("./reldata/../reldata/installs"),
            &cwd.join("reldata/installs")
        ));
        assert!(!same_path(
            Path::new("reldata/installs"),
            &cwd.join("other/installs")
        ));
    }

    #[test]
    fn the_backend_of_the_identity_carries_no_credentials_or_options() {
        assert_eq!(
            canonical_backend("asdf:https://user:tok@git.example.com/org/plugin.git"),
            "asdf:https://git.example.com/org/plugin.git"
        );
        assert_eq!(
            canonical_backend("github:owner/repo[matching=x]"),
            "github:owner/repo"
        );
        assert_eq!(canonical_backend("pipx:black"), "pypi:black");
    }

    #[test]
    fn a_version_link_reaches_the_store_relatively_when_it_can() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().canonicalize().unwrap();
        let tool_dir = base.join("data/installs/age");
        for dir in ["data/installs/age", "data/i", "elsewhere/store"] {
            std::fs::create_dir_all(base.join(dir)).unwrap();
        }
        assert_eq!(
            link_value(&tool_dir, &base.join("data/installs/age-p4n6w2ra")),
            Path::new("..").join("age-p4n6w2ra")
        );
        // Windows' default: the store is `i` beside `installs`.
        assert_eq!(
            link_value(&tool_dir, &base.join("data/i/age-p4n6w2ra")),
            Path::new("..").join("..").join("i").join("age-p4n6w2ra")
        );
        // A store somewhere else entirely is named absolutely, even when it was
        // configured as a relative path.
        assert_eq!(
            link_value(&tool_dir, &base.join("elsewhere/store/age-p4n6w2ra")),
            base.join("elsewhere/store/age-p4n6w2ra")
        );
        assert_eq!(
            link_value(&tool_dir, Path::new("store/age-p4n6w2ra")),
            std::env::current_dir().unwrap().join("store/age-p4n6w2ra")
        );
        // Through a symlinked installs directory, `../..` would leave the
        // symlink's target, so the link is absolute.
        #[cfg(unix)]
        {
            std::fs::create_dir_all(base.join("real/installs/age")).unwrap();
            std::fs::create_dir_all(base.join("linked")).unwrap();
            std::os::unix::fs::symlink(base.join("real/installs"), base.join("linked/installs"))
                .unwrap();
            std::fs::create_dir_all(base.join("linked/i")).unwrap();
            assert_eq!(
                link_value(
                    &base.join("linked/installs/age"),
                    &base.join("linked/i/age-p4n6w2ra")
                ),
                base.join("linked/i/age-p4n6w2ra")
            );
            // A store that is a symlink to the real sibling, under another name,
            // is named absolutely: a relative link through the physical name
            // would not be recognized as pointing into the configured store.
            std::fs::create_dir_all(base.join("links")).unwrap();
            std::os::unix::fs::symlink(base.join("data/i"), base.join("links/alias")).unwrap();
            assert_eq!(
                link_value(&tool_dir, &base.join("links/alias/age-p4n6w2ra")),
                base.join("links/alias/age-p4n6w2ra")
            );
            std::os::unix::fs::symlink(base.join("data/i"), base.join("data/alias")).unwrap();
            assert_eq!(
                link_value(&tool_dir, &base.join("data/alias/age-p4n6w2ra")),
                base.join("data/alias/age-p4n6w2ra")
            );
        }
    }

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
