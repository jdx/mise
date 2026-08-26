use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;

use crate::cli::args::{BackendArg, ToolArg};
use crate::config::tracking::Tracker;
use crate::config::{Config, Settings};
use crate::file::display_path;
use crate::runtime_symlinks;
use crate::toolset::{
    NeededVersions, ToolVersion, ToolsetBuilder, get_versions_needed_by_tracked_configs,
    get_versions_needed_by_tracked_stubs,
};
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::ui::prompt;
use crate::{backend::Backend, config, env, exit};
use console::style;
use eyre::Result;

use super::trust::Trust;

/// Delete unused versions of tools
///
/// mise tracks which config files have been used in ~/.local/state/mise/tracked-configs
/// Versions which are no longer the latest specified in any of those configs are deleted.
/// Versions installed only with environment variables `MISE_<TOOL>_VERSION` will be deleted,
/// as will versions only referenced on the command line `mise exec <TOOL>@<VERSION>`.
///
/// Tool stubs that have been executed are tracked in ~/.local/state/mise/tracked-stubs.
/// Versions still referenced by a tracked stub are not deleted.
///
/// You can list prunable tools with `mise ls --prunable`
#[derive(Debug, usage_rs::Args)]
#[usage(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub(crate) struct Prune {
    /// Prune only these tools
    #[usage()]
    pub installed_tool: Option<Vec<ToolArg>>,

    /// Do not actually delete anything
    #[usage(long, short = 'n')]
    pub dry_run: bool,

    /// Prune only tracked and trusted configuration links that point to nonexistent configurations
    #[usage(long)]
    pub configs: bool,

    /// Like --dry-run but exits with code 1 if there are tools to prune
    ///
    /// This is useful for scripts to check if tools need to be pruned.
    #[usage(long, verbatim_doc_comment)]
    pub dry_run_code: bool,

    /// Placeholder for future monorepo pruning; `mise prune --monorepo` is not implemented yet.
    #[usage(long, verbatim_doc_comment)]
    pub monorepo: bool,

    /// Prune only unused versions of tools
    #[usage(long)]
    pub tools: bool,
}

impl Prune {
    fn is_dry_run(&self) -> bool {
        self.dry_run || self.dry_run_code
    }

    pub(crate) async fn run(self) -> Result<()> {
        if self.monorepo {
            unimplemented!("mise prune --monorepo is not implemented yet");
        }
        let mut config = Config::get().await?;
        if self.configs || !self.tools {
            self.prune_configs()?;
        }
        if self.tools || !self.configs {
            let backends = self
                .installed_tool
                .as_ref()
                .map(|it| it.iter().map(|ta| ta.ba.as_ref()).collect());
            let tools = backends.unwrap_or_default();
            let (to_delete, needed) = prunable_tools_with_sources(&config, tools).await?;
            let has_work = !to_delete.is_empty();
            let explain = self.is_dry_run().then_some(&needed);
            delete(&config, self.is_dry_run(), to_delete, explain).await?;
            if self.dry_run_code && has_work {
                return Err(exit::request(1));
            }
            if self.is_dry_run() {
                return Ok(());
            }
            config = Config::reset().await?;
            let ts = config.get_toolset().await?;
            config::rebuild_shims_and_runtime_symlinks(
                &config,
                ts,
                &[],
                crate::lockfile::LockfileUpdateMode::Normal,
            )
            .await?;
        }
        Ok(())
    }

    fn prune_configs(&self) -> Result<()> {
        if self.is_dry_run() {
            info!("pruned configuration links {}", style("[dryrun]").bold());
        } else {
            Tracker::clean()?;
            Trust::clean()?;
            info!("pruned configuration links");
        }
        Ok(())
    }
}

pub(super) async fn prunable_tools(
    config: &Arc<Config>,
    tools: Vec<&BackendArg>,
) -> Result<Vec<(Arc<dyn Backend>, ToolVersion)>> {
    Ok(prunable_tools_with_sources(config, tools).await?.0)
}

/// Like [`prunable_tools`], but also returns what the tracked configs and stubs
/// still need. Pruning removes what none of them named, so the versions that
/// were kept — and the files that kept them — are the only evidence available
/// for explaining a removal.
async fn prunable_tools_with_sources(
    config: &Arc<Config>,
    tools: Vec<&BackendArg>,
) -> Result<(Vec<(Arc<dyn Backend>, ToolVersion)>, NeededVersions)> {
    let ts = ToolsetBuilder::new().build(config).await?;
    let mut to_delete = ts
        .list_installed_versions(config)
        .await?
        .into_iter()
        // System and shared installs are read-only fallback locations. Prune only
        // manages versions in the user's primary install directory.
        .filter(|(_, tv)| {
            env::install_path_category(&tv.install_path()) == env::InstallPathCategory::Local
        })
        .map(|(p, tv)| ((tv.ba().short.to_string(), tv.tv_pathname()), (p, tv)))
        .collect::<BTreeMap<(String, String), (Arc<dyn Backend>, ToolVersion)>>();

    if !tools.is_empty() {
        to_delete.retain(|_, (_, tv)| tools.contains(&tv.ba()));
    }

    // Remove versions that are still needed by tracked configs
    let mut needed = get_versions_needed_by_tracked_configs(config, true, true).await?;

    // Remove versions that are still needed by tracked tool stubs
    for (key, sources) in get_versions_needed_by_tracked_stubs(config).await? {
        needed.entry(key).or_default().extend(sources);
    }

    for key in needed.keys() {
        to_delete.remove(key);
    }

    Ok((to_delete.into_values().collect(), needed))
}

pub(super) async fn prune(
    config: &Arc<Config>,
    tools: Vec<&BackendArg>,
    dry_run: bool,
) -> Result<()> {
    let to_delete = prunable_tools(config, tools).await?;
    delete(config, dry_run, to_delete, None).await
}

async fn delete(
    config: &Arc<Config>,
    dry_run: bool,
    to_delete: Vec<(Arc<dyn Backend>, ToolVersion)>,
    explain: Option<&NeededVersions>,
) -> Result<()> {
    let mpr = MultiProgressReport::get();
    for (p, tv) in to_delete {
        if let Some(needed) = explain {
            explain_removal(&tv, needed);
        }
        let mut prefix = tv.style();
        if dry_run {
            prefix = format!("{} {} ", prefix, style("[dryrun]").bold());
        }
        if !dry_run
            && !Settings::get().yes
            && !prompt::confirm_with_all(format!("remove {} ?", tv))?.is_yes()
        {
            continue;
        }
        let pr = mpr.add(&prefix);
        p.uninstall_version(config, &tv, pr.as_ref(), dry_run)
            .await?;
        if !dry_run {
            runtime_symlinks::remove_missing_symlinks(p)?;
        }
        pr.finish();
    }
    Ok(())
}

/// Say why `tv` is up for removal.
///
/// Pruning decides by absence — a version goes because nothing among the
/// tracked configs and stubs resolved to it — so there is no file to point at
/// as the cause, and the output leaves the user nothing to check against.
/// Report the other side instead: the versions of the same tool that were kept
/// and the files that kept them. An empty list is itself the answer, and the
/// common one: nothing tracked mentions this tool at all.
fn explain_removal(tv: &ToolVersion, needed: &NeededVersions) {
    let short = &tv.ba().short;
    // `needed` is a HashMap; collect into a BTreeMap so the order is stable.
    let kept: BTreeMap<&String, &BTreeSet<PathBuf>> = needed
        .iter()
        .filter(|((s, _), _)| s == short)
        .map(|((_, version), sources)| (version, sources))
        .collect();
    // Match the short form the progress line below uses, not the fully
    // qualified `backend:name@version` that `Display` renders.
    let style = tv.style();
    if kept.is_empty() {
        info!("{style} is prunable: no tracked config or tool stub requires {short}");
        return;
    }
    let kept = kept
        .into_iter()
        .map(|(version, sources)| {
            let sources = sources.iter().map(display_path).collect::<Vec<_>>();
            format!("{version} by {}", sources.join(", "))
        })
        .collect::<Vec<_>>();
    info!(
        "{style} is prunable: {short} is required at {}",
        kept.join("; ")
    );
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise prune --dry-run</bold>
    rm -rf ~/.local/share/mise/versions/node/20.0.0
    rm -rf ~/.local/share/mise/versions/node/20.0.1
"#
);
