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

/// The name a legacy directory is moved aside to while it is migrated
/// ([`ASIDE_SUFFIX`]), or once its migration has finished ([`MIGRATED_SUFFIX`]).
/// Scans skip dot-prefixed entries, so it is invisible until put back or removed.
fn aside_path(legacy: &Path, suffix: &str) -> PathBuf {
    let name = legacy
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    legacy.with_file_name(format!(".{name}{suffix}"))
}

const ASIDE_SUFFIX: &str = ".mise-migrating";
const MIGRATED_SUFFIX: &str = ".mise-migrated";

/// Finish or undo migrations an earlier run did not complete (it was
/// interrupted). A directory marked migrated is removed: its replacement was
/// installed, checked and linked. One still moved aside is put back unless a real
/// directory took its place. The replacement's receipt and version link are not
/// proof it finished, because an install writes them before its postinstall runs:
/// the replacement is withdrawn and made again when the version is migrated.
fn recover_interrupted(dry_run: bool) -> Result<()> {
    let root: &Path = &dirs::INSTALLS;
    for tool in file::dir_subdirs(root).unwrap_or_default() {
        let tool_dir = root.join(&tool);
        for entry in file::ls(&tool_dir).unwrap_or_default() {
            let Some(name) = entry
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(|n| n.strip_prefix('.'))
            else {
                continue;
            };
            let (version, finished) = if let Some(v) = name.strip_suffix(MIGRATED_SUFFIX) {
                (v, true)
            } else if let Some(v) = name.strip_suffix(ASIDE_SUFFIX) {
                (v, false)
            } else {
                continue;
            };
            let legacy = tool_dir.join(version);
            // Free, or holding only the version link the interrupted install made.
            let free = std::fs::symlink_metadata(&legacy).is_err()
                || file::is_symlink_or_junction(&legacy);
            if dry_run {
                let action = if finished {
                    format!(
                        "would remove {}, left by an interrupted migration",
                        display_path(&entry)
                    )
                } else if free {
                    format!(
                        "would restore {} from an interrupted migration, then migrate it",
                        display_path(&legacy)
                    )
                } else {
                    format!(
                        "would keep {}, left by an interrupted migration: {} is in use",
                        display_path(&entry),
                        display_path(&legacy)
                    )
                };
                miseprintln!("{action}");
            } else if finished {
                file::remove_all(&entry)?;
                info!(
                    "removed {}, left by an interrupted migration",
                    display_path(&entry)
                );
            } else if free {
                if let Some(dir) = resolver::link_target(&legacy)
                    && resolver::is_primary_install(&dir)
                {
                    resolver::unpublish(&dir);
                }
                restore(&legacy, &entry)?;
                info!(
                    "restored {} from an interrupted migration",
                    display_path(&legacy)
                );
            } else {
                warn!(
                    "{} was left by an interrupted migration; {} is in use, so it is kept",
                    display_path(&entry),
                    display_path(&legacy)
                );
            }
        }
    }
    Ok(())
}

/// Puts a moved-aside legacy directory back if the migration stops before it
/// is resolved (a panic, or the task being dropped).
struct Aside<'a> {
    legacy: &'a Path,
    aside: &'a Path,
    armed: bool,
}

impl Drop for Aside<'_> {
    fn drop(&mut self) {
        if self.armed
            && let Err(err) = restore(self.legacy, self.aside)
        {
            warn!(
                "could not put {} back at {}: {err:#}",
                display_path(self.aside),
                display_path(self.legacy)
            );
        }
    }
}

/// Reinstall one legacy installation into the identity layout. The old
/// directory is moved aside first, so the install cannot find and reuse it,
/// and is put back if the new installation does not complete.
async fn migrate(tv: &ToolVersion) -> Result<()> {
    let legacy = tv.install_path();
    let aside = aside_path(&legacy, ASIDE_SUFFIX);
    if aside.exists() {
        eyre::bail!(
            "{} is left over from an earlier migration; put it back at {} or remove it first",
            display_path(&aside),
            display_path(&legacy)
        );
    }
    // Nothing else installs, removes or links this version until the migration
    // is resolved: an install with the legacy layout would put a directory back
    // where the old one has to return. The reinstall below is locked by its own
    // installation directory, not by this version, so it does not wait on this.
    let _lock = install_state::lock_tool_version(tv.ba(), &tv.tv_pathname())?;
    file::rename(&legacy, &aside)?;
    let mut guard = Aside {
        legacy: &legacy,
        aside: &aside,
        armed: true,
    };
    // The old path must lead to the new installation before the old directory
    // goes: where the version link could not be made (Windows without junction
    // support), the legacy directory is put back and the new installation is
    // withdrawn, so the version resolves to the directory that was kept. (Its
    // directory stays reserved, and installing the version again reuses it.)
    let installed = reinstall(tv).await.and_then(|dir| {
        // An installation that already existed (another spelling made it, or it
        // is in a shared root) was reused without a version link here.
        let linked = resolver::ensure_version_link(tv, &dir).and_then(|()| {
            let target = resolver::link_target(&legacy);
            if target.is_some_and(|t| t.canonicalize().ok() == dir.canonicalize().ok()) {
                Ok(())
            } else {
                Err(eyre::eyre!("the version link names another installation"))
            }
        });
        match linked {
            Ok(()) => Ok(dir),
            Err(err) => {
                if resolver::is_primary_install(&dir) {
                    resolver::unpublish(&dir);
                }
                Err(err.wrap_err(format!(
                    "installed {}, but could not link {} to it",
                    display_path(&dir),
                    display_path(&legacy)
                )))
            }
        }
    });
    guard.armed = false;
    match installed {
        Ok(dir) => {
            // Marked finished before it is removed, so an interruption from here
            // on is completed by the next run rather than undone.
            let migrated = aside_path(&legacy, MIGRATED_SUFFIX);
            let old = match file::rename(&aside, &migrated) {
                Ok(()) => migrated,
                Err(err) => {
                    debug!("could not mark {} migrated: {err:#}", display_path(&aside));
                    aside.clone()
                }
            };
            if let Err(err) = file::remove_all(&old) {
                warn!(
                    "migrated {}, but could not remove the old directory {}: {err:#}",
                    tv.style(),
                    display_path(&old)
                );
            }
            miseprintln!("migrated {} to {}", tv.style(), display_path(&dir));
            Ok(())
        }
        Err(err) => {
            restore(&legacy, &aside)?;
            drop(_lock);
            // The failed install rebuilt the tool's runtime aliases without the
            // version; rebuild them with it back in place.
            let config = Config::reset().await?;
            let ts = config.get_toolset().await?;
            crate::runtime_symlinks::rebuild_for_toolset(&config, ts).await?;
            Err(err)
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
    let opts = InstallOptions {
        reason: "installs migrate".to_string(),
        ..Default::default()
    };
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
