use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;

use crate::args::{BackendArg, ToolArg};
use crate::config::tracking::Tracker;
use crate::config::{Config, Settings};
use crate::file::display_path;
use crate::runtime_symlinks;
use crate::toolset::{
    NeededVersions, PrunableTools, RunningProcess, ToolVersion, prunable_tools_with_sources,
};
use crate::ui::install_progress::removal_progress;
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::ui::prompt::{self, Confirmation};
use crate::{backend::Backend, config, exit};
use console::style;
use eyre::Result;

use super::trust::Trust;

/// Delete tool versions that nothing uses
///
/// mise records each config file it loads (in ~/.local/state/mise/tracked-configs)
/// and each tool stub it runs (in ~/.local/state/mise/tracked-stubs). `mise prune`
/// deletes installed versions that none of them selects. A config that requests
/// `node = "20"` keeps only the 20.x version it resolves to. Versions installed
/// only for `mise exec <TOOL>@<VERSION>` or through `MISE_<TOOL>_VERSION` are
/// deleted. Versions that a running process was started from are kept, so a
/// long-running program keeps its files.
///
/// It also forgets tracked, trusted, and ignored config files that no longer
/// exist. Pass `--tools` or `--configs` to do only one of the two. List the
/// versions it would delete with `mise ls --prunable`.
#[derive(Debug, usage_rs::Args)]
#[usage(
    verbatim_doc_comment,
    example(
        r###"mise prune --dry-run
mise node@20.0.0 is prunable: node is required at 20.19.5 by ~/src/app/mise.toml
mise node@20.0.0 [dryrun]  remove ~/.local/share/mise/installs/node/20.0.0, ~/.cache/mise/node/20.0.0"###,
        help = "Preview what would be deleted and why"
    ),
    example("mise prune --tools node", help = "Delete unused node versions only")
)]
pub(crate) struct Prune {
    /// Prune only these tools
    #[usage()]
    pub installed_tool: Option<Vec<ToolArg>>,

    /// Show what would change without changing anything
    #[usage(long, short = 'n')]
    pub dry_run: bool,

    /// Only forget tracked, trusted, and ignored config files that no longer exist
    #[usage(long)]
    pub configs: bool,

    /// Like --dry-run, but exit with code 1 if there are tools to prune
    ///
    /// Use it in scripts that check whether tools need pruning.
    #[usage(long)]
    pub dry_run_code: bool,

    /// Placeholder for future monorepo pruning; `mise prune --monorepo` is not implemented yet.
    #[usage(long, hide = true, verbatim_doc_comment)]
    pub monorepo: bool,

    /// Only delete unused tool versions
    #[usage(long)]
    pub tools: bool,
}

impl Prune {
    pub(super) fn is_dry_run(&self) -> bool {
        self.dry_run || self.dry_run_code
    }

    pub(crate) async fn run(self) -> Result<()> {
        if self.monorepo {
            unimplemented!("mise prune --monorepo is not implemented yet");
        }
        // Prune inspects the project it runs in from whatever environment it was
        // started in, including when it rebuilds shims afterwards; none of that is
        // a use of the project, so it must not replace the project's snapshots.
        let _suspended = crate::install_layout::snapshots::suspend();
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
            let PrunableTools {
                to_delete,
                needed,
                running,
            } = prunable_tools_with_sources(&config, tools).await?;
            explain_running(&running);
            let has_work = !to_delete.is_empty();
            let explain = self.is_dry_run().then_some(&needed);
            delete(
                &config,
                self.is_dry_run(),
                to_delete,
                explain,
                UnavailableConfirmation::Error,
            )
            .await?;
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
            crate::install_layout::snapshots::clean()?;
            info!("pruned configuration links");
        }
        Ok(())
    }
}

pub(super) async fn prune(
    config: &Arc<Config>,
    tools: Vec<&BackendArg>,
    dry_run: bool,
) -> Result<()> {
    let _suspended = crate::install_layout::snapshots::suspend();
    let PrunableTools {
        to_delete, running, ..
    } = prunable_tools_with_sources(config, tools).await?;
    explain_running(&running);
    delete(
        config,
        dry_run,
        to_delete,
        None,
        UnavailableConfirmation::Decline,
    )
    .await
}

/// How a caller wants to handle a confirmation prompt that nobody can answer.
#[derive(Clone, Copy, PartialEq, Eq)]
enum UnavailableConfirmation {
    Error,
    Decline,
}

/// Confirms and removes the supplied versions according to the caller's prompt policy.
async fn delete(
    config: &Arc<Config>,
    dry_run: bool,
    to_delete: Vec<(Arc<dyn Backend>, ToolVersion)>,
    explain: Option<&NeededVersions>,
    unavailable: UnavailableConfirmation,
) -> Result<()> {
    let mpr = MultiProgressReport::get();
    if dry_run {
        for (p, tv) in to_delete {
            if let Some(needed) = explain {
                explain_removal(&tv, needed);
            }
            let prefix = format!("{} {} ", tv.style(), style("[dryrun]").bold());
            let pr = mpr.add(&prefix);
            p.uninstall_version(config, &tv, pr.as_ref(), true).await?;
            pr.finish();
        }
        return Ok(());
    }

    // Ask about everything first, then remove. A prompt in the middle of a
    // live region has to pause it, and the answers are the same either way.
    let mut confirmed = Vec::with_capacity(to_delete.len());
    for (p, tv) in to_delete {
        if let Some(needed) = explain {
            explain_removal(&tv, needed);
        }
        if Settings::get().yes {
            confirmed.push((p, tv));
            continue;
        }
        match prompt::confirm_with_all(format!("remove {} ?", tv))? {
            Confirmation::Yes => confirmed.push((p, tv)),
            Confirmation::No | Confirmation::Unanswered => {}
            Confirmation::Unavailable if unavailable == UnavailableConfirmation::Decline => {}
            Confirmation::Unavailable => eyre::bail!(
                "mise prune requires confirmation but there was nobody to ask; pass --yes to prune non-interactively"
            ),
        }
    }

    let mut progress = removal_progress(
        &mpr,
        confirmed
            .iter()
            .map(|(_, tv)| (removal_key(tv), tv.style())),
    );
    for (p, tv) in confirmed {
        let tool = progress
            .as_ref()
            .and_then(|progress| progress.start_tool(&removal_key(&tv)));
        let pr = match &tool {
            Some(tool) => tool.reporter(),
            None => mpr.add(&tv.style()),
        };
        let result = p.uninstall_version(config, &tv, pr.as_ref(), false).await;
        if let Some(tool) = &tool {
            tool.complete(result.as_ref().err().map(|e| e.to_string()).as_deref());
        }
        result?;
        if let Err(err) = crate::tool_purgatory::forget_path(&tv.install_path()) {
            warn!("failed to clear tool purgatory entry: {err:#}");
        }
        runtime_symlinks::remove_missing_symlinks(p)?;
        if tool.is_none() {
            pr.finish();
        }
    }
    if let Some(progress) = progress.as_mut() {
        progress.finish(vec![]);
    }
    Ok(())
}

/// The session key for a version being removed: the same `short@version`
/// shape the install scheduler uses.
pub(crate) fn removal_key(tv: &ToolVersion) -> String {
    match crate::install_layout::resolver::dir_name_of(&tv.install_path()) {
        // Variants of one version are separate rows.
        Some(dir) => format!("{}@{}#{dir}", tv.ba().short, tv.version),
        None => format!("{}@{}", tv.ba().short, tv.version),
    }
}

/// Say which versions are kept because processes are still running from them.
/// Nothing tracked needs these, so without this a version left behind after a
/// prune would look like a mistake.
fn explain_running(running: &[(ToolVersion, Vec<RunningProcess>)]) {
    const SHOWN: usize = 3;
    for (tv, processes) in running {
        let mut held_by = processes
            .iter()
            .take(SHOWN)
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ");
        if processes.len() > SHOWN {
            held_by.push_str(&format!(" and {} more", processes.len() - SHOWN));
        }
        info!("{} is kept: still running as {held_by}", tv.style());
    }
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
    let layout_dir = crate::install_layout::resolver::dir_name_of(&tv.install_path());
    let kept: BTreeMap<String, &BTreeSet<PathBuf>> = needed
        .iter()
        .filter_map(|((s, name), sources)| {
            if s.is_empty() {
                // An identity-layout key: the same tool when its receipt says so.
                if layout_dir.as_deref() == Some(name) {
                    return None;
                }
                crate::install_layout::resolver::sibling_version(tv, name)
                    .map(|version| (version, sources))
            } else {
                (s == short).then(|| (name.clone(), sources))
            }
        })
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
