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
            match resolver::migration_target(&tv) {
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
        let mut failed = vec![];
        for (tv, _) in plan {
            if let Err(err) = migrate(&tv).await {
                error!("could not migrate {}: {err:#}", tv.style());
                failed.push(tv.style());
            }
        }
        if !failed.is_empty() {
            eyre::bail!(
                "{} could not be migrated and kept the legacy layout",
                failed.join(", ")
            );
        }
        Ok(())
    }

    /// Whether `tv` is one of the tools asked for: `None` when none were named.
    fn named(&self, tv: &ToolVersion) -> Option<bool> {
        if self.tool.is_empty() {
            return None;
        }
        Some(self.tool.iter().any(|ta| {
            ta.ba.short == tv.ba().short
                && ta
                    .version
                    .as_deref()
                    .is_none_or(|v| v == tv.version || v == tv.tv_pathname())
        }))
    }
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
/// linked. Until then, undoing it puts the old directory back and withdraws every
/// installation of the version that the migration made, which is any not listed
/// in `existing`. One that was complete before the migration started is never
/// touched.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct Journal {
    /// The tool's short name, for the version's lock.
    tool: String,
    /// The legacy directory, `installs/<tool>/<version>`.
    legacy: PathBuf,
    /// The canonical backend and version of the replacement.
    backend: String,
    version: String,
    /// The complete installations of that backend and version before it started.
    existing: Vec<String>,
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
        let (backend, version) = resolver::identity_scope(tv)
            .ok_or_else(|| eyre::eyre!("the backend of {} cannot be loaded", tv.style()))?;
        let existing = resolver::installations()
            .into_iter()
            .filter(|i| i.backend == backend && i.version == version)
            .map(|i| i.name)
            .collect();
        let journal = Self {
            tool: tv.ba().short.clone(),
            legacy: tv.install_path(),
            backend,
            version,
            existing,
            finished: false,
        };
        journal.write()?;
        Ok(journal)
    }

    fn write(&self) -> Result<()> {
        file::create_dir_all(Self::dir())?;
        file::write(Self::path_for(&self.legacy), toml::to_string(self)?)
    }

    fn remove(&self) {
        if let Err(err) = file::remove_file(Self::path_for(&self.legacy)) {
            debug!("could not remove the migration journal: {err:#}");
        }
    }

    fn all() -> Vec<Self> {
        file::ls(&Self::dir())
            .unwrap_or_default()
            .into_iter()
            .filter_map(|path| {
                let body = std::fs::read_to_string(&path).ok()?;
                toml::from_str(&body)
                    .inspect_err(|err| debug!("ignoring {}: {err}", display_path(&path)))
                    .ok()
            })
            .collect()
    }

    /// Withdraw the installations of the version the migration made, each under
    /// its install lock. Without `wait` a lock that is held (by an install this
    /// process is dropping) is not waited for; returns whether nothing was left.
    fn withdraw(&self, wait: bool) -> bool {
        let mut done = true;
        let made = resolver::installations().into_iter().filter(|i| {
            !i.shared
                && i.backend == self.backend
                && i.version == self.version
                && !self.existing.contains(&i.name)
        });
        for installation in made {
            let lock = if wait {
                resolver::lock_install(&installation.dir, &|_| {})
            } else {
                resolver::try_lock_install(&installation.dir)
            };
            match lock {
                Ok(Some(_lock)) => resolver::unpublish(&installation.dir),
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
    for journal in Journal::all() {
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
        if !free && aside.exists() {
            warn!(
                "{} was left by an interrupted migration; {} is in use, so it is kept",
                display_path(&aside),
                display_path(legacy)
            );
            continue;
        }
        let version = legacy
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let ba = crate::args::BackendArg::from(journal.tool.as_str());
        let _lock = install_state::lock_tool_version(&ba, &version)?;
        journal.undo(true)?;
        info!(
            "restored {} from an interrupted migration",
            display_path(legacy)
        );
    }
    Ok(())
}

/// Undoes a migration that stops before it is resolved (a panic, or the task
/// being dropped).
struct Undo<'a> {
    journal: &'a Journal,
    armed: bool,
}

impl Drop for Undo<'_> {
    fn drop(&mut self) {
        if self.armed
            && let Err(err) = self.journal.undo(false)
        {
            warn!(
                "could not put {} back: {err:#}",
                display_path(&self.journal.legacy)
            );
        }
    }
}

/// Reinstall one legacy installation into the identity layout. The old
/// directory is moved aside first, so the install cannot find and reuse it,
/// and is put back if the new installation does not complete.
async fn migrate(tv: &ToolVersion) -> Result<()> {
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
    let journal = Journal::start(tv)?;
    if let Err(err) = file::rename(&legacy, &aside) {
        journal.remove();
        return Err(err);
    }
    let mut guard = Undo {
        journal: &journal,
        armed: true,
    };
    // The old path must lead to the new installation before the old directory
    // goes: where the version link could not be made (Windows without junction
    // support), the migration is undone, so the version resolves to the old
    // directory again.
    let installed = reinstall(tv).await.and_then(|dir| {
        // An installation that already existed (another spelling made it, or it
        // is in a shared root) was reused without a version link here.
        resolver::ensure_version_link(tv, &dir)
            .and_then(|()| {
                let target = resolver::link_target(&legacy);
                if target.is_some_and(|t| t.canonicalize().ok() == dir.canonicalize().ok()) {
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
            Ok(())
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
                Ok(()) => Err(err),
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
    // version it pins.
    opts.resolve_options.use_locked_version = false;
    let installed = ts
        .install_all_versions(&mut config, vec![request.clone()], &opts)
        .await?;
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
