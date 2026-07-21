use std::sync::Arc;

use indexmap::IndexSet;

use crate::backend::Backend;
use crate::config::settings::SystemDepsMode;
use crate::config::{Config, Settings};
use crate::system::deps::{self, DepStatus, SystemDep};
use crate::toolset::install_options::InstallOptions;
use crate::toolset::tool_request::ToolRequest;
use crate::toolset::tool_version::ToolVersion;

pub(super) type TVTuple = (Arc<dyn Backend>, ToolVersion);

pub(super) fn show_python_install_hint(versions: &[ToolRequest]) {
    let num_python = versions
        .iter()
        .filter(|tr| tr.ba().tool_name == "python")
        .count();
    if num_python != 1 {
        return;
    }
    hint!(
        "python_multi",
        "use multiple versions simultaneously with",
        "mise use python@3.12 python@3.11"
    );
}

/// Check plugin-declared system prerequisites for the tools about to install,
/// and — depending on the `system_deps` setting — report, prompt to install,
/// or auto-install the missing subset. Never fails the install: any error is
/// downgraded to a warning. Runs once for the whole batch, on the main task
/// (before parallel install tasks spawn), because the system-packages driver
/// futures are not `Send`.
pub(super) async fn preflight_system_deps(
    config: &Arc<Config>,
    versions: &[ToolRequest],
    opts: &InstallOptions,
) {
    let mode = Settings::get().system_deps;
    if mode == SystemDepsMode::Ignore {
        return;
    }
    if let Err(err) = preflight_system_deps_inner(config, versions, opts, mode).await {
        warn!("system dependency check failed: {err:#}");
    }
}

async fn preflight_system_deps_inner(
    config: &Arc<Config>,
    versions: &[ToolRequest],
    opts: &InstallOptions,
    mode: SystemDepsMode,
) -> eyre::Result<()> {
    // Collect declared deps per tool, deduping backends (multiple versions of
    // one tool share a backend and its declarations).
    let mut seen_backends = IndexSet::new();
    let mut per_tool: Vec<(String, Vec<SystemDep>)> = vec![];
    for tr in versions {
        // Skip requests not applicable to this OS, matching doctor's
        // analyze_system_deps and bootstrap's collect_plugin_deps.
        if !tr.is_os_supported() {
            continue;
        }
        let Ok(backend) = tr.backend() else { continue };
        if !seen_backends.insert(backend.ba().short.clone()) {
            continue;
        }
        let deps = backend.system_dependencies();
        if !deps.is_empty() {
            per_tool.push((backend.ba().tool_name.clone(), deps));
        }
    }
    if per_tool.is_empty() {
        return Ok(());
    }

    // Detect (memoized) and partition into missing required / optional.
    let mut missing_required: Vec<(String, DepStatus)> = vec![];
    let mut missing_optional: Vec<(String, DepStatus)> = vec![];
    for (tool, deps) in &per_tool {
        for status in deps::detect(deps).await {
            if status.satisfied {
                continue;
            }
            if status.dep.optional.is_some() {
                missing_optional.push((tool.clone(), status));
            } else {
                missing_required.push((tool.clone(), status));
            }
        }
    }

    for (tool, status) in &missing_optional {
        let reason = status.dep.optional.as_deref().unwrap_or_default();
        info!(
            "{tool}: optional system dependency {} is missing ({reason})",
            status.dep.label()
        );
    }

    if missing_required.is_empty() {
        return Ok(());
    }

    // Effective mode: prompt degrades to warn only when we can neither ask
    // (non-interactive) nor auto-confirm (`--yes`/MISE_YES). With `opts.yes`
    // set, Prompt stays Prompt and the remediation below runs non-interactively
    // (the driver uses `yes` to skip its own confirmation).
    let effective = match mode {
        SystemDepsMode::Prompt if !console::user_attended_stderr() && !opts.yes => {
            SystemDepsMode::Warn
        }
        other => other,
    };

    report_missing(&missing_required);

    let missing_deps: Vec<SystemDep> = missing_required
        .iter()
        .map(|(_, s)| s.dep.clone())
        .collect();
    let refs: Vec<&SystemDep> = missing_deps.iter().collect();
    let (mut by_mgr, unremediable) = deps::build_requests(&refs).await;
    // Attach any `[bootstrap.brew.taps]` git URLs so a tapped formula hint can
    // be installed the same way `mise bootstrap packages use` handles it.
    crate::system::attach_brew_tap_urls(config, &mut by_mgr);

    // Deps no available package manager can install are surfaced in every mode
    // (warn and prompt/auto) — otherwise they'd only appear in the generic
    // missing list with no actionable message.
    for dep in &unremediable {
        warn!(
            "no available package manager can install {} — install it manually",
            dep.label()
        );
    }

    if effective == SystemDepsMode::Warn {
        for line in deps::hint_commands(&refs).await {
            warn!("install with: {line}");
        }
        return Ok(());
    }

    // Prompt / Auto: remediate via the system-packages driver.
    if by_mgr.is_empty() {
        return Ok(());
    }

    let mgrs = crate::system::packages_from_requests(by_mgr)?;
    let driver_opts = crate::cli::system::driver::DriverOpts {
        manager: None,
        explicit: false,
        dry_run: opts.dry_run,
        update: false,
        yes: matches!(mode, SystemDepsMode::Auto) || opts.yes,
    };
    if let Err(err) = crate::cli::system::driver::run(
        mgrs,
        crate::cli::system::driver::Action::Install,
        &driver_opts,
    )
    .await
    {
        warn!("failed to install system dependencies: {err:#}");
        return Ok(());
    }

    if opts.dry_run {
        return Ok(());
    }

    // Re-detect only the deps we actually tried to install (those with a
    // package-manager hint), bypassing the memo cache. Deps with no hint, or
    // that the user declined at the driver prompt, are excluded — warning that
    // they "were installed but not detected" would be wrong.
    let mut remediated = vec![];
    for dep in &missing_deps {
        if deps::pick_manager(dep).await.is_some() {
            remediated.push(dep.clone());
        }
    }
    for status in deps::detect_fresh(&remediated).await {
        if !status.satisfied {
            warn!(
                "{} is still not detected after attempting to install it — the package install \
                 may have been declined or skipped, or it needs to be linked or added to PATH \
                 (e.g. `brew link --force`)",
                status.dep.label()
            );
        }
    }

    Ok(())
}

fn report_missing(missing: &[(String, DepStatus)]) {
    warn!("missing system dependencies for tools about to install:");
    for (tool, status) in missing {
        match (&status.found, &status.reason) {
            (Some(found), _) => warn!("  {tool}: {} required, found {found}", status.dep.label()),
            (None, Some(reason)) => warn!("  {tool}: {} — {reason}", status.dep.label()),
            (None, None) => warn!("  {tool}: {}", status.dep.label()),
        }
    }
}
