use std::path::Path;

use eyre::Result;

use crate::args::ToolArg;
use crate::config::Config;
use crate::file::{self, display_path};
use crate::install_layout::resolver;
use crate::toolset::{InstallOptions, ToolVersion};

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

/// Reinstall one legacy installation into the identity layout. The old
/// directory is moved aside first, so the install cannot find and reuse it,
/// and is put back if the new installation does not complete.
async fn migrate(tv: &ToolVersion) -> Result<()> {
    let legacy = tv.install_path();
    let name = legacy
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let aside = legacy.with_file_name(format!(".{name}.mise-migrating"));
    if aside.exists() {
        eyre::bail!(
            "{} is left over from an earlier migration; put it back at {} or remove it first",
            display_path(&aside),
            display_path(&legacy)
        );
    }
    file::rename(&legacy, &aside)?;
    // `reinstall` reloads the configuration and install state, which then see
    // the version as not installed.
    let installed = reinstall(tv).await;
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
            Config::reset().await?;
            Err(err)
        }
    }
}

async fn reinstall(tv: &ToolVersion) -> Result<std::path::PathBuf> {
    let mut config = Config::reset().await?;
    let mut ts = config.get_toolset().await?.clone();
    let mut request = tv.clone();
    request.install_path = None;
    let opts = InstallOptions {
        reason: "installs migrate".to_string(),
        ..Default::default()
    };
    ts.install_all_versions(&mut config, vec![request.request.clone()], &opts)
        .await?;
    resolver::installation_of(&request).ok_or_else(|| {
        eyre::eyre!(
            "{} did not install into the identity layout",
            request.style()
        )
    })
}

/// Put a legacy directory back where it was, replacing the version link an
/// interrupted install may have made there.
fn restore(legacy: &Path, aside: &Path) -> Result<()> {
    if file::is_symlink_or_junction(legacy) {
        file::remove_dir_link(legacy)?;
    }
    file::rename(aside, legacy)?;
    Ok(())
}
