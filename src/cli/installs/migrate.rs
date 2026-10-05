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
        // directory this one has moved aside and is still reinstalling.
        let _migrating = if self.dry_run {
            None
        } else {
            Some(
                crate::lock_file::LockFile::at(
                    &dirs::INSTALLS
                        .join(".mise")
                        .join("locks")
                        .join("migrate.lock"),
                )
                .lock()?,
            )
        };
        recover_interrupted(self.dry_run)?;
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

/// The name a legacy directory is moved aside to while it is migrated. Scans
/// skip dot-prefixed entries, so it is invisible until put back or removed.
fn aside_path(legacy: &Path) -> PathBuf {
    let name = legacy
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    legacy.with_file_name(format!(".{name}{ASIDE_SUFFIX}"))
}

const ASIDE_SUFFIX: &str = ".mise-migrating";

/// Finish or undo migrations an earlier run did not complete (it was
/// interrupted): a directory still moved aside is put back when nothing took
/// its place, and removed when its version link leads to a complete
/// installation.
fn recover_interrupted(dry_run: bool) -> Result<()> {
    let root: &Path = &dirs::INSTALLS;
    for tool in file::dir_subdirs(root).unwrap_or_default() {
        let tool_dir = root.join(&tool);
        for entry in file::ls(&tool_dir).unwrap_or_default() {
            let Some(version) = entry
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(|n| n.strip_prefix('.'))
                .and_then(|n| n.strip_suffix(ASIDE_SUFFIX))
                .map(str::to_string)
            else {
                continue;
            };
            let legacy = tool_dir.join(&version);
            let finished = resolver::link_target(&legacy).is_some();
            let free = std::fs::symlink_metadata(&legacy).is_err();
            if dry_run {
                miseprintln!(
                    "would {} {}, left by an interrupted migration",
                    if finished { "remove" } else { "restore" },
                    display_path(&entry)
                );
            } else if finished {
                file::remove_all(&entry)?;
                info!(
                    "removed {}, left by an interrupted migration",
                    display_path(&entry)
                );
            } else if free {
                file::rename(&entry, &legacy)?;
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
    let aside = aside_path(&legacy);
    if aside.exists() {
        eyre::bail!(
            "{} is left over from an earlier migration; put it back at {} or remove it first",
            display_path(&aside),
            display_path(&legacy)
        );
    }
    // Nothing else installs or removes this version while its directory moves.
    // (The install below takes the same lock itself.)
    {
        let _lock = install_state::lock_tool_version(tv.ba(), &tv.tv_pathname())?;
        file::rename(&legacy, &aside)?;
    }
    let mut guard = Aside {
        legacy: &legacy,
        aside: &aside,
        armed: true,
    };
    // The old path must lead to the new installation before the old directory
    // goes: where the version link could not be made (Windows without junction
    // support), the legacy directory is put back.
    let installed = reinstall(tv).await.and_then(|dir| {
        // An installation that already existed (another spelling made it, or it
        // is in a shared root) was reused without a version link here.
        resolver::ensure_version_link(tv, &dir)?;
        let linked = resolver::link_target(&legacy)
            .is_some_and(|target| target.canonicalize().ok() == dir.canonicalize().ok());
        if linked {
            Ok(dir)
        } else {
            Err(eyre::eyre!(
                "installed {}, but could not link {} to it",
                display_path(&dir),
                display_path(&legacy)
            ))
        }
    });
    guard.armed = false;
    let _lock = install_state::lock_tool_version(tv.ba(), &tv.tv_pathname())?;
    match installed {
        Ok(dir) => {
            if let Err(err) = file::remove_all(&aside) {
                warn!(
                    "migrated {}, but could not remove the old directory {}: {err:#}",
                    tv.style(),
                    display_path(&aside)
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
    ts.install_all_versions(&mut config, vec![request.clone()], &opts)
        .await?;
    let installed = ToolVersion::new(request, tv.version.clone());
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
