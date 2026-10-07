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
/// by someone else now, or the network is unavailable) is not an error: its old
/// directory stays exactly as it was, and the run reports it as kept in the
/// legacy layout. It still works, and a later run tries it again. The command
/// ends with a count of what moved, what was kept and what failed, and fails
/// only when a migration itself broke.
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
        let mut kept = 0;
        let mut failed = vec![];
        for (tv, _) in plan {
            match migrate(&tv).await {
                Ok(Outcome::Migrated) => migrated += 1,
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
            "{migrated} migrated, {kept} kept legacy, {} failed",
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

    fn start(tv: &ToolVersion) -> Result<Self> {
        let (backend, version) = resolver::identity_scope(&logical(tv))
            .ok_or_else(|| eyre::eyre!("the backend of {} cannot be loaded", tv.style()))?;
        let journal = Self {
            tool: tv.ba().short.clone(),
            legacy: tv.install_path(),
            backend,
            version,
            made: vec![],
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
    let legacy = tv.install_path();
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
    let journal: Shared = std::sync::Arc::new(std::sync::Mutex::new(Journal::start(tv)?));
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
    let installed = reinstall(&logical(tv)).await.and_then(|dir| {
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
            miseprintln!("migrated {} to {}", tv.style(), display_path(&dir));
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
            if let Err(rebuild_err) = rebuilt.await {
                warn!(
                    "could not rebuild the runtime links of {}: {rebuild_err:#}",
                    tv.style()
                );
            }
            match undone {
                // The install could not happen and the old directory is back:
                // nothing broke.
                Ok(()) => match err.downcast_ref::<CannotReinstall>() {
                    Some(cannot) => Ok(Outcome::Kept(cannot.0.clone())),
                    None => Err(err),
                },
                // The next run finishes undoing it, from the journal.
                Err(undo_err) => Err(err.wrap_err(format!(
                    "the old directory could not be put back yet: {undo_err:#}"
                ))),
            }
        }
    }
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
}
