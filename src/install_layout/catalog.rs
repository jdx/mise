//! The installation catalog under `<installs>/.mise/`.
//!
//! ```text
//! .mise/
//!   alloc.lock               held while a directory name is allocated
//!   identities/<bucket>/<digest>.toml   IdentityRecord, bucketed by backend
//!   names/<dir>              reservation: the digest a directory name belongs to
//!   selections/<digest>.toml Selection: unlocked request -> chosen installation
//! ```
//!
//! This is durable installation metadata, not a cache. It lives outside the
//! tool directories so that pruning a payload keeps its assigned path, its
//! reservation and its recovery provenance, and so that a restored identity
//! lands where it used to.
//!
//! Directory names are `<label>-<first 8 digest chars>`. When that name is
//! taken by another identity (or by anything else already on disk) the new
//! identity extends its suffix by two characters at a time. An existing
//! installation is never renamed to make room.

use std::path::{Path, PathBuf};

use eyre::{Result, WrapErr};

use super::identity::{DIGEST_CHARS, Digest, InstallIdentity, SHORT_CHARS};
use super::label::label_for;
use super::record::{
    IdentityRecord, Provenance, RECEIPT_FILE, Receipt, Selection, is_plain_dir_name,
};
use crate::file;
use crate::lock_file::LockFile;

/// Characters of a backend hash used to bucket records so that listing one
/// tool's installations is a single directory read.
const BUCKET_CHARS: usize = 13;

#[derive(Clone, Debug)]
pub(crate) struct Catalog {
    root: PathBuf,
    /// Where the installations it assigns live; usually `root` itself.
    store: PathBuf,
}

impl Catalog {
    /// A catalog for the installs root `root` (for example `~/.local/share/mise/installs`).
    pub(crate) fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        let store = super::resolver::store_of(&root);
        Self { root, store }
    }

    /// A catalog for `root` whose installations live in `store`.
    #[cfg(test)]
    pub(crate) fn with_store(root: impl Into<PathBuf>, store: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            store: store.into(),
        }
    }

    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    /// The directory holding this catalog's installations.
    pub(crate) fn store(&self) -> &Path {
        &self.store
    }

    pub(crate) fn meta_dir(&self) -> PathBuf {
        self.root.join(".mise")
    }

    fn identities_dir(&self) -> PathBuf {
        self.meta_dir().join("identities")
    }

    fn bucket_dir(&self, backend: &str) -> PathBuf {
        self.identities_dir().join(bucket(backend))
    }

    fn record_path(&self, identity: &InstallIdentity, digest: &str) -> PathBuf {
        self.bucket_dir(&identity.backend)
            .join(format!("{digest}.toml"))
    }

    fn name_path(&self, dir: &str) -> PathBuf {
        self.meta_dir().join("names").join(dir)
    }

    fn selection_path(&self, key: &InstallIdentity) -> PathBuf {
        self.meta_dir()
            .join("selections")
            .join(format!("{}.toml", key.digest().to_base32()))
    }

    /// The directory of an installation this catalog assigned.
    pub(crate) fn install_dir(&self, record: &IdentityRecord) -> PathBuf {
        self.store.join(&record.dir)
    }

    /// The record for `identity`, if one was ever allocated.
    pub(crate) fn lookup(&self, identity: &InstallIdentity) -> Option<IdentityRecord> {
        let digest = identity.digest().to_base32();
        let record = read_record(&self.record_path(identity, &digest))?;
        (record.digest == digest).then_some(record)
    }

    /// The record whose full digest is `digest`, for a backend.
    pub(crate) fn record_by_digest(&self, backend: &str, digest: &str) -> Option<IdentityRecord> {
        let record = read_record(&self.bucket_dir(backend).join(format!("{digest}.toml")))?;
        (record.digest == digest && record.identity.backend == backend).then_some(record)
    }

    /// The installation an unlocked request's selection points at, when it is
    /// one of this catalog's. A selection naming another root's installation is
    /// not answered from here, even if this catalog has the same identity.
    pub(crate) fn selected_record(&self, key: &InstallIdentity) -> Option<IdentityRecord> {
        let selection = self.selection(key)?;
        if selection.root.is_some() {
            return None;
        }
        self.record_by_digest(&key.backend, &selection.selected)
    }

    /// The directory an identity would first be offered, without reserving it.
    /// Used to report where an install that does not exist yet would go.
    pub(crate) fn tentative_dir(&self, identity: &InstallIdentity) -> PathBuf {
        let digest = identity.digest().to_base32();
        self.store.join(format!(
            "{}-{}",
            label_for(&identity.backend),
            &digest[..SHORT_CHARS]
        ))
    }

    /// Every record for one backend: all the installations (live or pruned) of
    /// every version of a tool.
    pub(crate) fn records_for_backend(&self, backend: &str) -> Vec<IdentityRecord> {
        let mut records: Vec<_> = read_dir_records(&self.bucket_dir(backend))
            .into_iter()
            .filter(|r| r.identity.backend == backend)
            .collect();
        records.sort_by(|a, b| a.dir.cmp(&b.dir));
        records
    }

    /// The record for `identity`, allocating a directory name first if this is
    /// a new identity. Concurrent callers for one identity get one directory;
    /// colliding identities get separate directories.
    pub(crate) fn allocate(&self, identity: &InstallIdentity) -> Result<IdentityRecord> {
        if let Some(record) = self.lookup(identity)
            && self.reservation_holds(&record)
        {
            return Ok(record);
        }
        let _lock = LockFile::at(&self.meta_dir().join("alloc.lock"))
            .lock()
            .wrap_err("failed to lock the installs catalog")?;
        self.allocate_locked(identity)
    }

    fn allocate_locked(&self, identity: &InstallIdentity) -> Result<IdentityRecord> {
        let digest = identity.digest();
        let digest_text = digest.to_base32();
        // Another process may have allocated it while this one waited.
        if let Some(record) = self.lookup(identity) {
            if self.reservation_holds(&record) {
                return Ok(record);
            }
            // The record survived without its reservation. Re-reserve its name
            // unless something else took it in the meantime; the assigned path
            // must not change while the name is free.
            if self.name_available_for(&record.dir, &digest_text) {
                self.reserve(&record.dir, &digest_text)?;
                return Ok(record);
            }
        }
        let label = label_for(&identity.backend);
        let mut len = SHORT_CHARS;
        while len <= DIGEST_CHARS {
            let dir = format!("{label}-{}", &digest_text[..len]);
            if self.name_available_for(&dir, &digest_text) {
                self.reserve(&dir, &digest_text)?;
                let record = IdentityRecord::new(identity.clone(), dir);
                write_record(&self.record_path(identity, &digest_text), &record)?;
                return Ok(record);
            }
            len += 2;
        }
        eyre::bail!(
            "could not allocate an install directory for {}",
            identity.backend
        )
    }

    /// Whether `dir` may be assigned to the identity with `digest`: it is
    /// reserved for that identity already, or nothing reserves or occupies it.
    ///
    /// An occupied path with missing or unreadable identity metadata is not
    /// permission to overwrite it, so it counts as taken.
    fn name_available_for(&self, dir: &str, digest: &str) -> bool {
        match std::fs::read_to_string(self.name_path(dir)) {
            Ok(owner) => return owner.trim() == digest,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            // Unreadable reservation: do not guess.
            Err(_) => return false,
        }
        match std::fs::symlink_metadata(self.store.join(dir)) {
            Err(_) => true,
            // Occupied: only ours if its receipt says so (a catalog that was
            // lost but whose installations survived).
            Ok(_) => read_receipt(&self.store.join(dir))
                .is_some_and(|receipt| receipt.record.digest == digest),
        }
    }

    /// Whether the directory name `dir` is reserved for some identity.
    pub(crate) fn is_reserved(&self, dir: &str) -> bool {
        self.name_path(dir).exists()
    }

    fn reservation_holds(&self, record: &IdentityRecord) -> bool {
        std::fs::read_to_string(self.name_path(&record.dir))
            .is_ok_and(|owner| owner.trim() == record.digest)
    }

    fn reserve(&self, dir: &str, digest: &str) -> Result<()> {
        write_file(&self.name_path(dir), digest)
            .wrap_err_with(|| format!("failed to reserve install directory {dir}"))
    }

    /// Merge `provenance` into an identity's record, under the catalog lock and
    /// against the record as it is on disk, so concurrent updates from
    /// several processes add up instead of overwriting each other: artifact
    /// digests are set (a newer digest for the same algorithm wins), lockfile
    /// pins are added, and the refresh generation only grows.
    pub(crate) fn update_provenance(
        &self,
        identity: &InstallIdentity,
        provenance: Provenance,
    ) -> Result<()> {
        let _lock = self.lock()?;
        let Some(mut record) = self.lookup(identity) else {
            return Ok(());
        };
        let merged = &mut record.provenance;
        merged.artifacts.extend(provenance.artifacts);
        for pin in provenance.pinned_by {
            if !merged.pinned_by.contains(&pin) {
                merged.pinned_by.push(pin);
            }
        }
        merged.generation = merged.generation.max(provenance.generation);
        write_record(&self.record_path(identity, &record.digest), &record)
    }

    /// Adopt installations that exist on disk with a receipt but are missing
    /// from the catalog (it was lost or never written), so the catalog can be
    /// rebuilt from receipts alone. Returns the records it restored.
    /// The installation in this catalog's store whose receipt records `digest`:
    /// what is left to go on when the catalog lost its record and cannot be
    /// rebuilt, as in a read-only shared root. It reads every receipt.
    pub(crate) fn find_by_receipt(&self, digest: &str) -> Option<IdentityRecord> {
        self.receipt_records()
            .find(|record| record.digest == digest)
    }

    /// [`Catalog::records_for_backend`] for a catalog that cannot be rebuilt (a
    /// read-only shared root): when it has lost its records entirely, the
    /// installations' receipts stand in for them. Reads every receipt in that
    /// case only.
    pub(crate) fn records_or_receipts_for_backend(&self, backend: &str) -> Vec<IdentityRecord> {
        if self.meta_dir().join("identities").is_dir() {
            return self.records_for_backend(backend);
        }
        let mut records: Vec<_> = self
            .receipt_records()
            .filter(|record| record.identity.backend == backend)
            .collect();
        records.sort_by(|a, b| a.dir.cmp(&b.dir));
        records
    }

    /// The records the receipts in the store carry, for directories they name.
    fn receipt_records(&self) -> impl Iterator<Item = IdentityRecord> + '_ {
        file::dir_subdirs(&self.store)
            .unwrap_or_default()
            .into_iter()
            .filter(|dir| !dir.starts_with('.'))
            .filter_map(|dir| read_receipt(&self.store.join(&dir)).map(|r| (dir, r.record)))
            .filter(|(dir, record)| record.dir == *dir && record.is_consistent())
            .map(|(_, record)| record)
    }

    pub(crate) fn rebuild_from_receipts(&self) -> Result<Vec<IdentityRecord>> {
        let mut restored = vec![];
        let _lock = self.lock()?;
        for dir in file::dir_subdirs(&self.store).unwrap_or_default() {
            if dir.starts_with('.') {
                continue;
            }
            let Some(receipt) = read_receipt(&self.store.join(&dir)) else {
                continue;
            };
            let record = receipt.record;
            if record.dir != dir || !record.is_consistent() {
                continue;
            }
            let on_disk = self.lookup(&record.identity);
            if on_disk.as_ref().is_some_and(|r| r.dir == dir) && self.reservation_holds(&record) {
                continue;
            }
            // A different directory already holds this identity: leave that
            // assignment alone rather than guess which one is right.
            if on_disk.is_some_and(|r| r.dir != dir) {
                continue;
            }
            if !self.name_available_for(&dir, &record.digest) {
                continue;
            }
            self.reserve(&dir, &record.digest)?;
            write_record(&self.record_path(&record.identity, &record.digest), &record)?;
            restored.push(record);
        }
        Ok(restored)
    }

    /// The full digest the directory name `dir` is reserved for.
    pub(crate) fn owner_of(&self, dir: &str) -> Option<String> {
        std::fs::read_to_string(self.name_path(dir))
            .ok()
            .map(|owner| owner.trim().to_string())
    }

    /// Forget the selections that name the installation with `digest` in this
    /// catalog's own root. Returns how many were removed.
    pub(crate) fn remove_selections_of(&self, digest: &str) -> Result<usize> {
        // Under the lock selections are written with, so a choice made meanwhile
        // is never the one removed.
        let _lock = self.lock()?;
        let dir = self.meta_dir().join("selections");
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return Ok(0);
        };
        let mut removed = 0;
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            let Some(selection) = std::fs::read_to_string(&path)
                .ok()
                .and_then(|body| toml::from_str::<Selection>(&body).ok())
            else {
                continue;
            };
            if selection.selected == digest && selection.root.is_none() {
                file::remove_file(&path)?;
                removed += 1;
            }
        }
        Ok(removed)
    }

    /// Where an unlocked request's choice is remembered, if it was.
    pub(crate) fn selection(&self, key: &InstallIdentity) -> Option<Selection> {
        let body = std::fs::read_to_string(self.selection_path(key)).ok()?;
        let selection: Selection = toml::from_str(&body).ok()?;
        (selection.request.digest() == key.digest()).then_some(selection)
    }

    /// Remember that `selected` satisfies the unlocked request `key`.
    ///
    /// Only an unlocked install or an explicit refresh calls this. A locked
    /// install reuses an installation without touching the selection.
    pub(crate) fn select(
        &self,
        key: &InstallIdentity,
        selected: &IdentityRecord,
        root: Option<&Path>,
    ) -> Result<()> {
        let _lock = self.lock()?;
        self.write_selection(key, selected, root)
    }

    /// [`Catalog::select`], unless `key` already has a selection. Returns whether
    /// it wrote one.
    pub(crate) fn select_if_unset(
        &self,
        key: &InstallIdentity,
        selected: &IdentityRecord,
        root: Option<&Path>,
    ) -> Result<bool> {
        let _lock = self.lock()?;
        if self.selection(key).is_some() {
            return Ok(false);
        }
        self.write_selection(key, selected, root)?;
        Ok(true)
    }

    /// Hold the catalog lock, under which allocations and selections are written,
    /// for a change that has to stay in step with a selection (its version links).
    pub(crate) fn lock(&self) -> Result<fslock::LockFile> {
        LockFile::at(&self.meta_dir().join("alloc.lock")).lock()
    }

    /// [`Catalog::select`] for a caller already holding [`Catalog::lock`].
    pub(crate) fn write_selection(
        &self,
        key: &InstallIdentity,
        selected: &IdentityRecord,
        root: Option<&Path>,
    ) -> Result<()> {
        let selection = Selection {
            request: key.clone(),
            selected: selected.digest.clone(),
            root: root.map(|r| r.to_string_lossy().to_string()),
        };
        write_file(
            &self.selection_path(key),
            toml::to_string_pretty(&selection)?,
        )
    }
}

/// Opaque directory name for a backend's records.
fn bucket(backend: &str) -> String {
    Digest::of(backend.as_bytes()).to_base32()[..BUCKET_CHARS].to_string()
}

fn read_record(path: &Path) -> Option<IdentityRecord> {
    let body = std::fs::read_to_string(path).ok()?;
    match toml::from_str::<IdentityRecord>(&body) {
        Ok(record) if record.is_consistent() => Some(record),
        Ok(_) => {
            debug!("ignoring inconsistent install record {}", path.display());
            None
        }
        Err(err) => {
            debug!(
                "ignoring unreadable install record {}: {err}",
                path.display()
            );
            None
        }
    }
}

fn read_dir_records(dir: &Path) -> Vec<IdentityRecord> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return vec![];
    };
    entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "toml"))
        .filter_map(|p| read_record(&p))
        .collect()
}

fn write_record(path: &Path, record: &IdentityRecord) -> Result<()> {
    write_file(path, toml::to_string_pretty(record)?)
}

/// Atomically write `body` to `path`, creating its parent directories.
fn write_file(path: &Path, body: impl AsRef<[u8]>) -> Result<()> {
    if let Some(parent) = path.parent() {
        file::create_dir_all(parent)?;
    }
    file::write_atomic(path, body)
}

/// Read the receipt inside an installation directory. A receipt whose digest
/// does not match its own inputs, or whose assigned directory is not the one
/// it sits in, is not trusted.
pub(crate) fn read_receipt(install_dir: &Path) -> Option<Receipt> {
    let body = std::fs::read_to_string(install_dir.join(RECEIPT_FILE)).ok()?;
    let receipt: Receipt = toml::from_str(&body).ok()?;
    let name = install_dir.file_name()?.to_str()?;
    (receipt.record.is_consistent() && is_plain_dir_name(name) && receipt.record.dir == name)
        .then_some(receipt)
}

/// Write the receipt for a finished (or in-progress) installation.
pub(crate) fn write_receipt(install_dir: &Path, receipt: &Receipt) -> Result<()> {
    write_file(
        &install_dir.join(RECEIPT_FILE),
        toml::to_string_pretty(receipt)?,
    )
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use super::*;
    use crate::install_layout::identity::Mode;

    fn identity(backend: &str, version: &str) -> InstallIdentity {
        InstallIdentity {
            mode: Mode::Fallback,
            backend: backend.into(),
            version: version.into(),
            platform: "linux-x64".into(),
            options: BTreeMap::new(),
            inputs: BTreeMap::new(),
        }
    }

    fn catalog() -> (tempfile::TempDir, Catalog) {
        let tmp = tempfile::tempdir().unwrap();
        let catalog = Catalog::new(tmp.path().join("installs"));
        std::fs::create_dir_all(catalog.root()).unwrap();
        (tmp, catalog)
    }

    #[test]
    fn allocates_label_dash_eight_chars() {
        let (_tmp, catalog) = catalog();
        let id = identity("aqua:FiloSottile/age", "1.2.1");
        let record = catalog.allocate(&id).unwrap();
        let digest = id.digest().to_base32();
        assert_eq!(record.dir, format!("age-{}", &digest[..8]));
        assert!(record.is_consistent());
        assert_eq!(catalog.lookup(&id), Some(record));
    }

    #[test]
    fn same_identity_reuses_its_directory() {
        let (_tmp, catalog) = catalog();
        let id = identity("aqua:FiloSottile/age", "1.2.1");
        let a = catalog.allocate(&id).unwrap();
        let b = catalog.allocate(&id).unwrap();
        assert_eq!(a, b);
        assert_eq!(catalog.records_for_backend("aqua:FiloSottile/age").len(), 1);
    }

    #[test]
    fn different_identities_get_different_directories() {
        let (_tmp, catalog) = catalog();
        let a = catalog
            .allocate(&identity("aqua:FiloSottile/age", "1.2.1"))
            .unwrap();
        let b = catalog
            .allocate(&identity("aqua:FiloSottile/age", "1.2.2"))
            .unwrap();
        assert_ne!(a.dir, b.dir);
        assert_eq!(catalog.records_for_backend("aqua:FiloSottile/age").len(), 2);
    }

    #[test]
    fn a_taken_name_extends_the_suffix_and_never_moves_the_old_install() {
        let (_tmp, catalog) = catalog();
        let first = identity("core:node", "20.0.0");
        let second = identity("core:node", "20.0.1");
        let digest = second.digest().to_base32();
        // Reserve the second identity's first-choice name for a stranger.
        let wanted = format!("node-{}", &digest[..8]);
        write_file(
            &catalog.name_path(&wanted),
            "someotheridentitysomeotheridentitysomeotheridentity",
        )
        .unwrap();
        let record = catalog.allocate(&second).unwrap();
        assert_eq!(record.dir, format!("node-{}", &digest[..10]));
        // The stranger keeps its name; a third request does not take it.
        let first_record = catalog.allocate(&first).unwrap();
        assert_ne!(first_record.dir, wanted);
        assert_eq!(catalog.allocate(&second).unwrap().dir, record.dir);
    }

    #[test]
    fn an_occupied_path_without_metadata_is_never_overwritten() {
        let (_tmp, catalog) = catalog();
        let id = identity("core:node", "20.0.0");
        let digest = id.digest().to_base32();
        // A directory that mise knows nothing about sits on the first choice.
        let squatter = catalog.root().join(format!("node-{}", &digest[..8]));
        std::fs::create_dir_all(&squatter).unwrap();
        std::fs::write(squatter.join("keep"), "x").unwrap();
        let record = catalog.allocate(&id).unwrap();
        assert_eq!(record.dir, format!("node-{}", &digest[..10]));
        assert!(squatter.join("keep").exists());
    }

    #[test]
    fn a_lost_catalog_is_rebuilt_from_receipts_and_paths_are_restored() {
        let (_tmp, catalog) = catalog();
        let id = identity("aqua:FiloSottile/age", "1.2.1");
        let record = catalog.allocate(&id).unwrap();
        let dir = catalog.install_dir(&record);
        std::fs::create_dir_all(&dir).unwrap();
        write_receipt(
            &dir,
            &Receipt {
                record: record.clone(),
                requested_as: None,
                mise_version: None,
            },
        )
        .unwrap();
        std::fs::remove_dir_all(catalog.meta_dir()).unwrap();
        assert_eq!(catalog.lookup(&id), None);

        let restored = catalog.rebuild_from_receipts().unwrap();
        assert_eq!(restored.len(), 1);
        assert_eq!(catalog.lookup(&id).unwrap().dir, record.dir);
        // Allocation finds the surviving installation instead of extending.
        assert_eq!(catalog.allocate(&id).unwrap().dir, record.dir);
    }

    #[test]
    fn provenance_updates_merge_instead_of_overwriting() {
        let (_tmp, catalog) = catalog();
        let id = identity("aqua:jqlang/jq", "1.7.1");
        catalog.allocate(&id).unwrap();
        // Two processes each read the record before either wrote, and each adds a pin.
        let first = Provenance {
            pinned_by: vec!["sha256:a".into()],
            ..Default::default()
        };
        let second = Provenance {
            artifacts: [("checksum".to_string(), "sha256:a".to_string())].into(),
            pinned_by: vec!["sha256:b".into()],
            generation: 0,
        };
        catalog.update_provenance(&id, first).unwrap();
        catalog.update_provenance(&id, second).unwrap();
        let mut stale = Provenance {
            generation: 2,
            ..Default::default()
        };
        catalog.update_provenance(&id, stale.clone()).unwrap();
        stale.generation = 1;
        catalog.update_provenance(&id, stale).unwrap();

        let provenance = catalog.lookup(&id).unwrap().provenance;
        assert_eq!(provenance.pinned_by, ["sha256:a", "sha256:b"]);
        assert_eq!(
            provenance.artifacts.get("checksum").map(String::as_str),
            Some("sha256:a")
        );
        assert_eq!(provenance.generation, 2);
    }

    #[test]
    fn a_separate_store_holds_the_installations_and_the_root_keeps_the_catalog() {
        let tmp = tempfile::tempdir().unwrap();
        let catalog = Catalog::with_store(tmp.path().join("installs"), tmp.path().join("i"));
        let id = identity("core:node", "20.0.0");
        let digest = id.digest().to_base32();
        // A squatter in the store blocks the first choice; one in the root does not.
        let first_choice = format!("node-{}", &digest[..8]);
        std::fs::create_dir_all(catalog.store().join(&first_choice)).unwrap();
        std::fs::create_dir_all(catalog.root().join(format!("node-{}", &digest[..10]))).unwrap();
        let record = catalog.allocate(&id).unwrap();
        assert_eq!(record.dir, format!("node-{}", &digest[..10]));
        let dir = catalog.install_dir(&record);
        assert_eq!(dir, tmp.path().join("i").join(&record.dir));
        assert!(catalog.meta_dir().starts_with(catalog.root()));
        assert_eq!(
            catalog
                .tentative_dir(&identity("core:node", "21.0.0"))
                .parent(),
            Some(catalog.store())
        );

        // Receipts are found in the store when the catalog is rebuilt.
        std::fs::create_dir_all(&dir).unwrap();
        write_receipt(
            &dir,
            &Receipt {
                record: record.clone(),
                requested_as: None,
                mise_version: None,
            },
        )
        .unwrap();
        std::fs::remove_dir_all(catalog.meta_dir()).unwrap();
        assert_eq!(catalog.rebuild_from_receipts().unwrap().len(), 1);
        assert_eq!(catalog.lookup(&id).unwrap().dir, record.dir);
        assert!(catalog.is_reserved(&record.dir));
    }

    #[test]
    fn a_pruned_payload_keeps_its_assigned_path() {
        let (_tmp, catalog) = catalog();
        let id = identity("aqua:FiloSottile/age", "1.2.1");
        let record = catalog.allocate(&id).unwrap();
        let dir = catalog.install_dir(&record);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        // Restoring the same identity lands on the same path.
        assert_eq!(catalog.allocate(&id).unwrap().dir, record.dir);
        // And the pruned name is not handed to anyone else.
        let digest = id.digest().to_base32();
        write_file(
            &catalog.name_path(&format!("age-{}", &digest[..8])),
            &digest,
        )
        .unwrap();
        assert_eq!(catalog.allocate(&id).unwrap().dir, record.dir);
    }

    #[test]
    fn concurrent_allocators_of_one_identity_agree() {
        let (_tmp, catalog) = catalog();
        let id = identity("aqua:FiloSottile/age", "1.2.1");
        let dirs: Vec<_> = (0..8)
            .map(|_| {
                let catalog = catalog.clone();
                let id = id.clone();
                std::thread::spawn(move || catalog.allocate(&id).unwrap().dir)
            })
            .collect::<Vec<_>>()
            .into_iter()
            .map(|h| h.join().unwrap())
            .collect();
        assert!(dirs.windows(2).all(|w| w[0] == w[1]), "{dirs:?}");
    }

    #[test]
    fn concurrent_allocators_of_colliding_identities_stay_separate() {
        let (_tmp, catalog) = catalog();
        let handles: Vec<_> = (0..8)
            .map(|i| {
                let id = identity("core:node", &format!("20.0.{i}"));
                let catalog = catalog.clone();
                std::thread::spawn(move || catalog.allocate(&id).unwrap().dir)
            })
            .collect();
        let dirs: BTreeSet<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        assert_eq!(dirs.len(), 8);
    }

    #[test]
    fn inconsistent_records_are_ignored() {
        let (_tmp, catalog) = catalog();
        let id = identity("core:node", "20.0.0");
        let record = catalog.allocate(&id).unwrap();
        let path = catalog.record_path(&id, &record.digest);
        std::fs::write(&path, "not toml {{").unwrap();
        assert_eq!(catalog.lookup(&id), None);
        // A record naming an escaping directory is rejected.
        let mut bad = IdentityRecord::new(id.clone(), "../escape".into());
        bad.dir = "../escape".into();
        std::fs::write(&path, toml::to_string_pretty(&bad).unwrap()).unwrap();
        assert_eq!(catalog.lookup(&id), None);
    }

    #[test]
    fn selections_are_shared_by_equivalent_requests() {
        let (_tmp, catalog) = catalog();
        let key = identity("aqua:FiloSottile/age", "1.2.1");
        let record = catalog.allocate(&key).unwrap();
        assert_eq!(catalog.selection(&key), None);
        catalog.select(&key, &record, None).unwrap();
        let selection = catalog.selection(&key).unwrap();
        assert_eq!(selection.selected, record.digest);
        // A different version is a different request.
        assert_eq!(
            catalog.selection(&identity("aqua:FiloSottile/age", "1.2.2")),
            None
        );
    }

    #[test]
    fn a_receipt_in_the_wrong_directory_is_not_trusted() {
        let (_tmp, catalog) = catalog();
        let id = identity("core:node", "20.0.0");
        let record = catalog.allocate(&id).unwrap();
        let elsewhere = catalog.root().join("not-the-assigned-dir");
        std::fs::create_dir_all(&elsewhere).unwrap();
        write_receipt(
            &elsewhere,
            &Receipt {
                record,
                requested_as: None,
                mise_version: None,
            },
        )
        .unwrap();
        assert_eq!(read_receipt(&elsewhere), None);
    }
}
