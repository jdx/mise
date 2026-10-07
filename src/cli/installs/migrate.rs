use std::path::{Path, PathBuf};

use eyre::Result;

use crate::args::ToolArg;
use crate::config::Config;
use crate::dirs;
use crate::file::{self, display_path};
use crate::install_layout::resolver;
use crate::toolset::{InstallOptions, ToolRequest, ToolVersion, install_state};

/// Reinstall legacy installations into the identity install layout
///
/// Installations made before `install_layout = "identity"` was turned on stay in
/// `installs/<tool>/<version>` and keep working; mise never moves them on its own,
/// because tools record their own path in shebangs, virtual environments and
/// package-manager files. This reinstalls each one into its own `<label>-<hash>`
/// directory, then removes the old directory and puts the version link in its
/// place, so a path that pointed into the old directory still resolves.
///
/// Each version is reinstalled from its backend, not copied, so the files are
/// written for their new path. Run it while nothing is using the tools: the old
/// directory is moved aside while its replacement installs, and put back if that
/// fails. Versions whose recorded backend is not the one their tool resolves to
/// now are left alone, as are tools that keep the legacy layout (`http:`,
/// `rust`, `dotnet`).
///
/// A version that cannot be reinstalled (its release was withdrawn or is signed
/// by someone else now, or the network is unavailable) is not an error: its
/// directory is moved as it is into the identity layout, behind the same version
/// link. Only a version that cannot be moved either keeps its old directory,
/// reported as kept in the legacy layout. The command ends with a count of what
/// was migrated, relocated, kept and failed, and fails only when a migration
/// itself broke.
#[derive(Debug, usage_rs::Args)]
#[usage(
    example(
        r###"mise installs migrate --dry-run"###,
        help = r###"List the legacy installations that would move"###
    ),
    example(
        r###"mise installs migrate node python@3.12.1"###,
        help = r###"Move every installed node, and one python"###
    ),
    verbatim_doc_comment
)]
pub(super) struct InstallsMigrate {
    /// Only migrate these tools, or these versions of them
    ///
    /// Defaults to every legacy installation.
    #[usage(value_name = "TOOL@VERSION", verbatim_doc_comment)]
    tool: Vec<ToolArg>,

    /// Show what would move without changing anything
    #[usage(long, short = 'n')]
    dry_run: bool,
}

impl InstallsMigrate {
    pub(super) async fn run(self) -> Result<()> {
        // One migration at a time: another run must not finish or undo a
        // directory this one has moved aside and is still reinstalling. A dry
        // run does not wait for one, and does not report what it has moved aside
        // as interrupted.
        let lock = crate::lock_file::LockFile::at(
            &dirs::INSTALLS
                .join(".mise")
                .join("locks")
                .join("migrate.lock"),
        );
        let migrating = if self.dry_run {
            lock.try_lock()?
        } else {
            Some(lock.lock()?)
        };
        if migrating.is_some() {
            recover_interrupted(self.dry_run)?;
        } else {
            miseprintln!(
                "another `mise installs migrate` is running; what it has moved aside is not listed"
            );
        }
        let config = Config::get().await?;
        let ts = config.get_toolset().await?.clone();
        let mut plan = vec![];
        for (_, tv) in ts.list_installed_versions(&config).await? {
            let named = self.named(&tv);
            if !named.unwrap_or(self.tool.is_empty())
                || !resolver::is_legacy_install(&tv.install_path())
            {
                continue;
            }
            match resolver::migration_target(&logical(&tv)) {
                Ok(target) => plan.push((tv, target)),
                Err(why) if named == Some(true) => warn!("not migrating {}: {why}", tv.style()),
                Err(why) => debug!("not migrating {}: {why}", tv.style()),
            }
        }
        if plan.is_empty() {
            miseprintln!("no legacy installations to migrate");
            return Ok(());
        }
        if self.dry_run {
            for (tv, target) in &plan {
                miseprintln!(
                    "would migrate {} from {} to {}",
                    tv.style(),
                    display_path(tv.install_path()),
                    display_path(target)
                );
            }
            return Ok(());
        }
        let mut migrated = 0;
        let mut relocated = 0;
        let mut kept = 0;
        let mut failed = vec![];
        for (tv, _) in plan {
            match migrate(&tv).await {
                Ok(Outcome::Migrated) => migrated += 1,
                Ok(Outcome::Relocated(why)) => {
                    relocated += 1;
                    miseprintln!(
                        "  {} could not be reinstalled ({why}); moved as it is",
                        tv.style()
                    );
                }
                Ok(Outcome::Kept(why)) => {
                    kept += 1;
                    miseprintln!("skipped {} (kept legacy layout): {why}", tv.style());
                }
                Err(err) => {
                    error!("could not migrate {}: {err:#}", tv.style());
                    failed.push(tv.style());
                }
            }
        }
        miseprintln!(
            "{migrated} migrated, {relocated} relocated, {kept} kept legacy, {} failed",
            failed.len()
        );
        if !failed.is_empty() {
            eyre::bail!(
                "{} could not be migrated and kept the legacy layout",
                failed.join(", ")
            );
        }
        Ok(())
    }

    /// Whether `tv` is one of the tools asked for: `None` when none were named.
    /// A version names the installed version it is, or else, as for
    /// `mise uninstall node@20`, those it begins up to a `.`, `-` or `+` (`20`
    /// names `20.11.1`, not `200.1`); `latest` names the newest installed one.
    fn named(&self, tv: &ToolVersion) -> Option<bool> {
        if self.tool.is_empty() {
            return None;
        }
        Some(self.tool.iter().any(|ta| {
            ta.ba.short == tv.ba().short
                && ta.version.as_deref().is_none_or(|v| {
                    let Ok(backend) = tv.backend() else {
                        return false;
                    };
                    if v == "latest" {
                        // An installed version's name on disk (`ref-main`), or its
                        // version (`ref:main`).
                        let latest = backend.latest_installed_version(None).ok().flatten();
                        return latest.is_some_and(|l| l == tv.version || l == tv.tv_pathname());
                    }
                    if v == tv.version || v == tv.tv_pathname() {
                        return true;
                    }
                    let exact = backend.list_installed_versions().iter().any(|i| i == v);
                    !exact
                        && tv
                            .version
                            .strip_prefix(v)
                            .is_some_and(|rest| rest.starts_with(['.', '-', '+']))
                })
        }))
    }
}

/// How one version's migration ended, when nothing broke.
enum Outcome {
    Migrated,
    /// It could not be reinstalled, so the existing directory was moved into
    /// the identity layout; the reason is why it was not reinstalled.
    Relocated(String),
    /// It could not be reinstalled, so the old directory is as it was.
    Kept(String),
}

/// A reinstall that could not happen: the backend failed to install the
/// version. Nothing was changed, so the legacy directory is kept as it is.
#[derive(Debug)]
struct CannotReinstall(String);

impl std::fmt::Display for CannotReinstall {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for CannotReinstall {}

/// `tv` with the version it stands for. A directory found by scanning can carry
/// a private suffix (`2026.9.3~aube~<digest>`, `~uv~`) that is not part of any
/// version a backend can install; its identity, reinstall and journal use the
/// version without it, while the old path and version link keep the name on disk.
fn logical(tv: &ToolVersion) -> ToolVersion {
    let mut logical = tv.clone();
    logical.strip_install_path_identity();
    logical
}

/// Where a legacy directory waits while it is migrated: beside the version link
/// that replaces it, under a dot name that scans skip.
fn aside_path(legacy: &Path) -> PathBuf {
    let name = legacy
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    legacy.with_file_name(format!(".{name}.mise-migrating"))
}

/// What a migration records before it touches anything, so that an interrupted
/// one is finished or undone exactly.
///
/// The old directory is deleted only once the journal says the migration
/// finished: its replacement installed (postinstall included), checked and
/// linked. Until then, undoing it puts the old directory back and withdraws the
/// installations the migration's install wrote into, which the journal names
/// before anything is written to them. Nothing else is touched.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct Journal {
    /// The tool's short name, for the version's lock.
    tool: String,
    /// The legacy directory, `installs/<tool>/<version>`.
    legacy: PathBuf,
    /// The canonical backend and version of the replacement.
    backend: String,
    version: String,
    /// The installation directories of that backend and version the migration's
    /// install was about to write into.
    made: Vec<PathBuf>,
    /// The old directory is being moved into the identity layout rather than
    /// reinstalled: it is then in the one installation directory in `made`, not
    /// aside, and undoing it moves it back.
    #[serde(default)]
    relocating: bool,
    finished: bool,
}

impl Journal {
    fn dir() -> PathBuf {
        dirs::INSTALLS.join(".mise").join("migrations")
    }

    fn path_for(legacy: &Path) -> PathBuf {
        let part = |p: Option<&std::ffi::OsStr>| {
            p.map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default()
        };
        let tool = part(legacy.parent().and_then(Path::file_name));
        let version = part(legacy.file_name());
        Self::dir().join(format!("{tool}@{version}.toml"))
    }

    fn start(tv: &ToolVersion, logical: &ToolVersion, relocating: bool) -> Result<Self> {
        let (backend, version) = resolver::identity_scope(logical)
            .ok_or_else(|| eyre::eyre!("the backend of {} cannot be loaded", tv.style()))?;
        let journal = Self {
            tool: tv.ba().short.clone(),
            legacy: tv.install_path(),
            backend,
            version,
            made: vec![],
            relocating,
            finished: false,
        };
        journal.write()?;
        Ok(journal)
    }

    fn write(&self) -> Result<()> {
        file::create_dir_all(Self::dir())?;
        file::write_atomic(Self::path_for(&self.legacy), toml::to_string(self)?)
    }

    fn remove(&self) {
        if let Err(err) = file::remove_file(Self::path_for(&self.legacy)) {
            debug!("could not remove the migration journal: {err:#}");
        }
    }

    /// Every journal, and the files in their place that cannot be read as one
    /// (with what an interrupted atomic write left behind).
    fn all() -> (Vec<Self>, Vec<PathBuf>) {
        let mut journals = vec![];
        let mut unreadable = vec![];
        for path in file::ls(&Self::dir()).unwrap_or_default() {
            let name = path.file_name().map(|n| n.to_string_lossy().to_string());
            if name.is_none_or(|n| n.starts_with('.') || !n.ends_with(".toml")) {
                unreadable.push(path);
                continue;
            }
            match std::fs::read_to_string(&path)
                .map_err(eyre::Report::from)
                .and_then(|body| Ok(toml::from_str::<Self>(&body)?))
            {
                Ok(journal) => journals.push(journal),
                Err(err) => {
                    debug!("cannot read {}: {err:#}", display_path(&path));
                    unreadable.push(path);
                }
            }
        }
        (journals, unreadable)
    }

    /// Withdraw the installations of the version the migration made, each under
    /// its install lock. Without `wait` a lock that is held (by an install this
    /// process is dropping) is not waited for; returns whether nothing was left.
    fn withdraw(&self, wait: bool) -> bool {
        let mut done = true;
        for dir in self.made.iter().filter(|dir| dir.exists()) {
            let lock = if wait {
                resolver::lock_install(dir, &|_| {})
            } else {
                resolver::try_lock_install(dir)
            };
            match lock {
                // A receipt that could not be removed still marks it complete.
                Ok(Some(_lock)) => {
                    resolver::unpublish(dir);
                    if resolver::is_complete(dir) {
                        done = false;
                    }
                }
                Ok(None) | Err(_) => done = false,
            }
        }
        done
    }

    /// Put the old directory back and withdraw what the migration made. The
    /// journal stays until both are done, so the next run finishes the job.
    fn undo(&self, wait: bool) -> Result<()> {
        let complete = self.withdraw(wait);
        let aside = aside_path(&self.legacy);
        // A relocated directory has no copy aside: it goes back from where it
        // was moved to (its receipt and links are withdrawn above).
        if self.relocating
            && !aside.exists()
            && let [dir] = self.made.as_slice()
            && std::fs::symlink_metadata(dir).is_ok()
        {
            if resolver::is_complete(dir) {
                // Still published, and the only copy: it cannot go back, so
                // this is a failure, and the journal stays for the next run.
                eyre::bail!(
                    "{} is still published and could not be withdrawn; run \
                     `mise installs migrate` again to put {} back",
                    display_path(dir),
                    display_path(&self.legacy)
                );
            } else {
                // The version link would be in the way of putting it back.
                if file::is_symlink_or_junction(&self.legacy) {
                    file::remove_dir_link(&self.legacy)?;
                }
                file::rename(dir, &aside)?;
            }
        }
        if aside.exists() {
            restore(&self.legacy, &aside)?;
        }
        if complete {
            self.remove();
        }
        Ok(())
    }
}

/// Finish or undo migrations an earlier run did not complete (it was
/// interrupted). A finished one only had its old directory left to remove. An
/// unfinished one is undone, unless a real directory took the old one's place:
/// a replacement's receipt and version link are no proof it finished, since an
/// install writes them before its postinstall runs.
fn recover_interrupted(dry_run: bool) -> Result<()> {
    let (journals, unreadable) = Journal::all();
    // A journal that cannot be read says nothing about what its run made: only
    // the old directory it moved aside can still be put back (below).
    for path in unreadable {
        if dry_run {
            miseprintln!("would remove {}, which cannot be read", display_path(&path));
        } else {
            warn!(
                "removing {}, a migration journal that cannot be read",
                display_path(&path)
            );
            file::remove_file(&path)?;
        }
    }
    for journal in journals {
        let legacy = &journal.legacy;
        let aside = aside_path(legacy);
        // Free, or holding only the version link the interrupted install made.
        let free =
            std::fs::symlink_metadata(legacy).is_err() || file::is_symlink_or_junction(legacy);
        if dry_run {
            let action = if journal.finished {
                format!(
                    "would remove {}, left by an interrupted migration",
                    display_path(&aside)
                )
            } else if free {
                format!(
                    "would restore {} from an interrupted migration, then migrate it",
                    display_path(legacy)
                )
            } else {
                format!(
                    "would keep {}, left by an interrupted migration: {} is in use",
                    display_path(&aside),
                    display_path(legacy)
                )
            };
            miseprintln!("{action}");
            continue;
        }
        if journal.finished {
            if aside.exists() {
                file::remove_all(&aside)?;
                info!(
                    "removed {}, left by an interrupted migration",
                    display_path(&aside)
                );
            }
            journal.remove();
            continue;
        }
        let version = legacy
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let ba = crate::args::BackendArg::from(journal.tool.as_str());
        let _lock = install_state::lock_tool_version(&ba, &version)?;
        if !free && aside.exists() {
            // Something else put a directory where the old one has to return (an
            // install with the legacy layout). It stays; the old one is kept
            // under another name, so nothing blocks migrating the version again.
            let withdrawn = journal.withdraw(true);
            let kept = keep_aside(&aside)?;
            // What could not be withdrawn is tried again on the next run.
            if withdrawn {
                journal.remove();
            }
            warn!(
                "{} took the place of the directory an interrupted migration moved aside; \
                 that one is kept at {}: remove it if {} works",
                display_path(legacy),
                display_path(&kept),
                version
            );
            continue;
        }
        journal.undo(true)?;
        info!(
            "restored {} from an interrupted migration",
            display_path(legacy)
        );
    }
    // Old directories moved aside by a run whose journal is gone (or could not
    // be read). Without it nothing says whether that run finished, so one is put
    // back only where nothing took its place; a version link there may be the
    // finished migration's, and both are kept for the user to decide.
    let root: &Path = &dirs::INSTALLS;
    for tool in file::dir_subdirs(root).unwrap_or_default() {
        let tool_dir = root.join(&tool);
        for entry in file::ls(&tool_dir).unwrap_or_default() {
            let Some(version) = entry
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(|n| n.strip_prefix('.'))
                .and_then(|n| n.strip_suffix(".mise-migrating"))
            else {
                continue;
            };
            let legacy = tool_dir.join(version);
            if Journal::path_for(&legacy).exists() {
                continue;
            }
            let free = std::fs::symlink_metadata(&legacy).is_err();
            if dry_run {
                if free {
                    miseprintln!(
                        "would restore {} from an interrupted migration, then migrate it",
                        display_path(&legacy)
                    );
                } else {
                    miseprintln!(
                        "would keep {}, left by a migration whose record is gone, under \
                         another name",
                        display_path(&entry)
                    );
                }
            } else if free {
                // Under the version's lock, as a migration moves it.
                let ba = crate::args::BackendArg::from(tool.as_str());
                let _lock = install_state::lock_tool_version(&ba, version)?;
                if std::fs::symlink_metadata(&legacy).is_ok() {
                    continue;
                }
                restore(&legacy, &entry)?;
                info!(
                    "restored {} from an interrupted migration",
                    display_path(&legacy)
                );
            } else {
                let kept = keep_aside(&entry)?;
                warn!(
                    "a migration whose record is gone left an old directory beside {}; it is \
                     kept at {}: remove it if {} works",
                    display_path(&legacy),
                    display_path(&kept),
                    version
                );
            }
        }
    }
    Ok(())
}

/// Rename an old directory that cannot be put back to a name no later
/// migration reads as its own, and return it.
fn keep_aside(aside: &Path) -> Result<PathBuf> {
    let name = aside
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let base = name.strip_suffix(".mise-migrating").unwrap_or(&name);
    let mut kept = aside.with_file_name(format!("{base}.mise-kept"));
    let mut n = 1;
    while std::fs::symlink_metadata(&kept).is_ok() {
        n += 1;
        kept = aside.with_file_name(format!("{base}.mise-kept-{n}"));
    }
    file::rename(aside, &kept)?;
    Ok(kept)
}

/// The journal of the migration in progress, which its install's allocations
/// are added to.
type Shared = std::sync::Arc<std::sync::Mutex<Journal>>;

fn snapshot(journal: &Shared) -> Journal {
    journal.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Stops recording allocations, and undoes a migration that stops before it is
/// resolved (a panic, or the task being dropped).
struct Undo {
    journal: Shared,
    armed: bool,
}

impl Drop for Undo {
    fn drop(&mut self) {
        resolver::observe_writes(None);
        let journal = snapshot(&self.journal);
        if self.armed
            && let Err(err) = journal.undo(false)
        {
            warn!(
                "could not put {} back: {err:#}",
                display_path(&journal.legacy)
            );
        }
    }
}

/// Reinstall one legacy installation into the identity layout. The old
/// directory is moved aside first, so the install cannot find and reuse it,
/// and is put back if the new installation does not complete.
async fn migrate(tv: &ToolVersion) -> Result<Outcome> {
    match attempt(tv, false).await? {
        Outcome::Kept(why) => match attempt(tv, true).await {
            Ok(Outcome::Migrated) => Ok(Outcome::Relocated(why)),
            Ok(Outcome::Kept(not_moved)) => Ok(Outcome::Kept(format!(
                "{why}; not moved either: {not_moved}"
            ))),
            Ok(relocated) => Ok(relocated),
            Err(err) => Err(err),
        },
        outcome => Ok(outcome),
    }
}

/// One try at migrating `tv`: reinstalling it, or with `relocate` moving the
/// existing directory into the identity layout.
async fn attempt(tv: &ToolVersion, relocate: bool) -> Result<Outcome> {
    let legacy = tv.install_path();
    // Before the directory moves: an older suffix (`-aube-<digest>`) is checked
    // against the lockfile inside it.
    let logical_tv = logical(tv);
    // The installer links the version without its suffix, which is not this
    // version's name: that link goes, unless something else already had the name.
    let extra_link = extra_link(tv, &logical_tv).filter(|p| std::fs::symlink_metadata(p).is_err());
    let aside = aside_path(&legacy);
    if aside.exists() || Journal::path_for(&legacy).exists() {
        eyre::bail!(
            "an earlier migration of {} did not finish; run `mise installs migrate` again \
             to finish or undo it",
            display_path(&legacy)
        );
    }
    // Nothing else installs, removes or links this version until the migration
    // is resolved: an install with the legacy layout would put a directory back
    // where the old one has to return. The reinstall below is locked by its own
    // installation directory, not by this version, so it does not wait on this.
    let _lock = install_state::lock_tool_version(tv.ba(), &tv.tv_pathname())?;
    // The plan was made before the lock: something (`mise link`, an install) may
    // have taken the version's place since.
    if !resolver::is_legacy_install(&legacy) {
        eyre::bail!(
            "{} is no longer a legacy installation; it was left alone",
            display_path(&legacy)
        );
    }
    let journal: Shared = std::sync::Arc::new(std::sync::Mutex::new(Journal::start(
        tv,
        &logical_tv,
        relocate,
    )?));
    if let Err(err) = file::rename(&legacy, &aside) {
        snapshot(&journal).remove();
        return Err(err);
    }
    // Every directory the install is about to write into is in the journal
    // before anything is written there; one it cannot record is not written.
    let recorder = journal.clone();
    resolver::observe_writes(Some(Box::new(move |dir, backend, version| {
        let mut journal = recorder.lock().unwrap_or_else(|e| e.into_inner());
        if backend == journal.backend
            && version == journal.version
            && !journal.made.iter().any(|d| d == dir)
        {
            journal.made.push(dir.to_path_buf());
            journal.write()?;
        }
        Ok(())
    })));
    let mut guard = Undo {
        journal: journal.clone(),
        armed: true,
    };
    // The old path must lead to the new installation before the old directory
    // goes: where the version link could not be made (Windows without junction
    // support), the migration is undone, so the version resolves to the old
    // directory again.
    let step = if relocate {
        relocate_into_layout(&logical_tv, &aside)
    } else {
        reinstall(&logical_tv).await
    };
    if let Some(extra) = &extra_link {
        let mut ours = snapshot(&journal).made;
        ours.extend(step.as_ref().ok().cloned());
        drop_extra_link(extra, &ours);
    }
    let installed = step.and_then(|dir| {
        // An installation that already existed (another spelling made it, or it
        // is in a shared root) was reused without a version link here.
        resolver::ensure_version_link(tv, &dir)
            .and_then(|()| {
                let leads_to_dir = resolver::link_target(&legacy).is_some_and(
                    |t| matches!((t.canonicalize(), dir.canonicalize()), (Ok(a), Ok(b)) if a == b),
                );
                if leads_to_dir {
                    Ok(dir.clone())
                } else {
                    Err(eyre::eyre!("the version link names another installation"))
                }
            })
            .map_err(|err| {
                err.wrap_err(format!(
                    "installed {}, but could not link {} to it",
                    display_path(&dir),
                    display_path(&legacy)
                ))
            })
    });
    guard.armed = false;
    drop(guard);
    let journal = snapshot(&journal);
    match installed {
        Ok(dir) => {
            // From here the old directory goes, whatever stops this run.
            let mut finished = journal.clone();
            finished.finished = true;
            if let Err(err) = finished.write() {
                journal.undo(true)?;
                return Err(err.wrap_err("could not record the migration as finished"));
            }
            match file::remove_all(&aside) {
                Ok(()) => finished.remove(),
                Err(err) => warn!(
                    "migrated {}, but could not remove the old directory {}: {err:#}",
                    tv.style(),
                    display_path(&aside)
                ),
            }
            if relocate {
                miseprintln!("relocated {} to {}", tv.style(), display_path(&dir));
            } else {
                miseprintln!("migrated {} to {}", tv.style(), display_path(&dir));
            }
            Ok(Outcome::Migrated)
        }
        Err(err) => {
            let undone = journal.undo(true);
            drop(_lock);
            // The failed install rebuilt the tool's runtime aliases without the
            // version; rebuild them with it back in place.
            let rebuilt = async {
                let config = Config::reset().await?;
                let ts = config.get_toolset().await?;
                crate::runtime_symlinks::rebuild_for_toolset(&config, ts).await
            };
            settle(err, undone, rebuilt.await)
        }
    }
}

/// How a migration that did not install ends, once its old directory was put
/// back (`undone`) and the tool's runtime links rebuilt (`rebuilt`). A reinstall
/// that could not happen leaves nothing broken, so long as both worked; a path
/// that may not resolve is a failure.
fn settle(err: eyre::Report, undone: Result<()>, rebuilt: Result<()>) -> Result<Outcome> {
    // The next run finishes undoing it, from the journal.
    if let Err(undo_err) = undone {
        return Err(err.wrap_err(format!(
            "the old directory could not be put back yet: {undo_err:#}"
        )));
    }
    if let Err(rebuild_err) = rebuilt {
        return Err(err.wrap_err(format!(
            "the old directory is back, but its runtime links (such as `latest`) could not be \
             rebuilt: {rebuild_err:#}"
        )));
    }
    match err.downcast_ref::<CannotReinstall>() {
        Some(cannot) => Ok(Outcome::Kept(cannot.0.clone())),
        None => Err(err),
    }
}

/// The version link the installer makes for the logical version of an install
/// whose directory carries a private suffix, when that is not the old path.
fn extra_link(tv: &ToolVersion, logical: &ToolVersion) -> Option<PathBuf> {
    let extra = tv.ba().installs_path().join(logical.tv_pathname());
    (extra != tv.install_path()).then_some(extra)
}

/// Remove `extra` if this migration's install made it: a link whose full target
/// is one of `ours`, the installation directories the migration wrote into or
/// made. A link to anything else (a `mise link`, another install, a directory of
/// the same name in another root) is not ours. The target is compared as a path,
/// so it need not exist (a failed install's directory may be gone).
fn drop_extra_link(extra: &Path, ours: &[PathBuf]) {
    if !file::is_symlink_or_junction(extra) {
        return;
    }
    let Ok(target) = std::fs::read_link(extra) else {
        return;
    };
    let target = match (target.is_relative(), extra.parent()) {
        (true, Some(parent)) => parent.join(target),
        _ => target,
    };
    let same = |dir: &PathBuf| {
        use path_absolutize::Absolutize;
        let clean = |p: &Path| p.absolutize().map(|p| p.into_owned()).ok();
        if let (Ok(a), Ok(b)) = (target.canonicalize(), dir.canonicalize()) {
            return a == b;
        }
        matches!((clean(&target), clean(dir)), (Some(a), Some(b)) if a == b)
    };
    if ours.iter().any(same)
        && let Err(err) = file::remove_dir_link(extra)
    {
        debug!("could not remove {}: {err:#}", display_path(extra));
    }
}

/// Move the old directory (already aside) into the identity layout as the
/// installation the version's identity names, with the receipt a normal install
/// writes. Only what the old directory can say is in the identity: the backend
/// and version, the platform, and the options the request carries. A digest
/// that comes from a lockfile (an artifact checksum, an embedded aube or uv
/// dependency graph) is not reconstructed.
///
/// Nothing is rewritten inside it: paths it recorded for itself keep resolving
/// through the version link that replaces the old directory. A failure leaves it
/// where it can be put back, so it is reported like a failed reinstall.
fn relocate_into_layout(tv: &ToolVersion, aside: &Path) -> Result<PathBuf> {
    let cannot = |err: eyre::Report| eyre::Report::new(CannotReinstall(format!("{err:#}")));
    resolver::relocate(tv, aside).map_err(cannot)
}

/// Install exactly the legacy installation's version (not what its request,
/// such as `latest`, resolves to today) and return where it went.
async fn reinstall(tv: &ToolVersion) -> Result<PathBuf> {
    let mut config = Config::reset().await?;
    let mut ts = config.get_toolset().await?.clone();
    let request = ToolRequest::new_with_options(
        tv.request.ba().clone(),
        &tv.version,
        tv.request.options(),
        tv.request.source().clone(),
    )?;
    let mut opts = InstallOptions {
        reason: "installs migrate".to_string(),
        ..Default::default()
    };
    // Exactly this version: a lockfile entry for the tool must not swap in the
    // version it pins. Locked mode, which requires a lockfile URL for every
    // install, does not apply either: this is a version already installed,
    // moved, not one the lockfile chose.
    opts.resolve_options.use_locked_version = false;
    opts.locked = false;
    // A project's `tool_config.locked` policy too, while its config file stays
    // the request's source for hooks and templates.
    opts.ignore_tool_config_locked = true;
    let installed = ts
        .install_all_versions(&mut config, vec![request.clone()], &opts)
        .await
        .map_err(|err| {
            let why = format!("{err:#}");
            eyre::Report::new(CannotReinstall(
                why.split_whitespace().collect::<Vec<_>>().join(" "),
            ))
        })?;
    // The install may have given the request the options the configuration sets
    // for the tool; what it installed is the version it reports back.
    let installed = installed
        .into_iter()
        .find(|t| t.ba().short == tv.ba().short && t.version == tv.version)
        .unwrap_or_else(|| ToolVersion::new(request, tv.version.clone()));
    resolver::installation_of(&installed).ok_or_else(|| {
        eyre::eyre!(
            "{} did not install into the identity layout",
            installed.style()
        )
    })
}

/// Put a legacy directory back where it was, replacing the version link an
/// interrupted install may have made there.
fn restore(legacy: &Path, aside: &Path) -> Result<()> {
    if file::is_symlink_or_junction(legacy) {
        file::remove_dir_link(legacy)?;
    }
    // Something else installed a real directory there meanwhile (a mise using
    // the legacy layout): keep both rather than overwrite either.
    file::rename(aside, legacy).map_err(|err| {
        eyre::eyre!(
            "{err:#}; the old installation is kept at {}",
            display_path(aside)
        )
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::args::BackendArg;
    use crate::toolset::ToolSource;

    #[test]
    fn logical_drops_the_private_suffix_of_an_embedded_aube_install() {
        let dir = "2026.9.3~aube~f375ab1a2e9919a1";
        let npm = Arc::new(BackendArg::from("npm:openclaw"));
        let request = ToolRequest::new(npm, "2026.9.3", ToolSource::Argument).unwrap();
        let tv = ToolVersion::new(request, dir.into());
        let logical = logical(&tv);
        // what is reinstalled is the npm version, but the old path is the one on disk
        assert_eq!(logical.version, "2026.9.3");
        assert_eq!(logical.install_path(), tv.install_path());
        assert!(logical.install_path().ends_with(dir));
        // the version link that replaces the directory keeps its name
        assert_eq!(tv.tv_pathname(), dir);

        let github = Arc::new(BackendArg::from("github:owner/tool"));
        let request = ToolRequest::new(github, "latest", ToolSource::Argument).unwrap();
        let tv = ToolVersion::new(request, dir.into());
        assert_eq!(super::logical(&tv).version, dir);
    }

    #[test]
    fn an_aube_directory_has_an_extra_unsuffixed_link_to_drop() {
        let dir = "2026.9.3~aube~f375ab1a2e9919a1";
        let npm = Arc::new(BackendArg::from("npm:openclaw"));
        let request = ToolRequest::new(npm.clone(), "2026.9.3", ToolSource::Argument).unwrap();
        let tv = ToolVersion::new(request, dir.into());
        let extra = extra_link(&tv, &logical(&tv)).unwrap();
        assert!(extra.ends_with("2026.9.3"));
        // a plain version has no other name
        let request = ToolRequest::new(npm, "2026.9.3", ToolSource::Argument).unwrap();
        let plain = ToolVersion::new(request, "2026.9.3".into());
        assert!(extra_link(&plain, &logical(&plain)).is_none());
    }

    #[cfg(unix)]
    #[test]
    fn drop_extra_link_removes_only_a_link_to_what_the_migration_made() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("installs");
        let elsewhere = tmp.path().join("shared");
        let tools = root.join("tool");
        std::fs::create_dir_all(&tools).unwrap();
        let ours = root.join("tool-aaaaaaaa");
        let same_name = elsewhere.join("tool-aaaaaaaa");
        let other = root.join("tool-bbbbbbbb");
        for dir in [&ours, &same_name, &other] {
            std::fs::create_dir_all(dir).unwrap();
        }
        let link = tools.join("1.0.0");
        let ours_list = std::slice::from_ref(&ours);
        // another install, and a same-named directory in another root
        for target in [&other, &same_name] {
            std::os::unix::fs::symlink(target, &link).unwrap();
            drop_extra_link(&link, ours_list);
            assert!(link.is_symlink());
            std::fs::remove_file(&link).unwrap();
        }
        drop_extra_link(&link, ours_list);
        // a relative link to ours, as the installer writes it
        std::os::unix::fs::symlink("../tool-aaaaaaaa", &link).unwrap();
        drop_extra_link(&link, ours_list);
        assert!(!link.is_symlink());
        // dangling, its directory gone again
        std::os::unix::fs::symlink(&ours, &link).unwrap();
        std::fs::remove_dir(&ours).unwrap();
        drop_extra_link(&link, ours_list);
        assert!(!link.is_symlink() && other.is_dir() && same_name.is_dir());
    }

    #[cfg(unix)]
    #[test]
    fn undoing_a_relocation_that_is_still_published_fails_and_keeps_the_journal() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("tool-abcdefgh");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join(".mise-install.toml"), "").unwrap();
        // the receipt cannot be removed
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();
        if std::fs::remove_file(dir.join(".mise-install.toml")).is_ok() {
            return; // permissions are not enforced (root)
        }
        let journal = Journal {
            tool: "tool".into(),
            legacy: tmp.path().join("tool").join("1.0.0"),
            backend: "b".into(),
            version: "1.0.0".into(),
            made: vec![dir.clone()],
            relocating: true,
            finished: false,
        };
        let result = journal.undo(false);
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(dir.exists());
        assert!(result.is_err());
    }

    #[test]
    fn settle_keeps_only_when_everything_was_put_back() {
        let cannot = || eyre::Report::new(CannotReinstall("gone".into()));
        assert!(matches!(
            settle(cannot(), Ok(()), Ok(())),
            Ok(Outcome::Kept(why)) if why == "gone"
        ));
        // the links that were removed while the install failed are still missing
        let err = settle(cannot(), Ok(()), Err(eyre::eyre!("boom")))
            .err()
            .unwrap();
        assert!(format!("{err:#}").contains("runtime links"));
        let err = settle(cannot(), Err(eyre::eyre!("boom")), Ok(()))
            .err()
            .unwrap();
        assert!(format!("{err:#}").contains("could not be put back"));
        // any other failure stays one
        assert!(settle(eyre::eyre!("link"), Ok(()), Ok(())).is_err());
    }
}
