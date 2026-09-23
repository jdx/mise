use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::str::FromStr;

use eyre::{Result, bail};

use crate::cli::args::ToolArg;
use crate::cli::lock::Lock;
use crate::config::Config;
use crate::file::display_path;
use crate::lockfile::{self, Lockfile};
use crate::toolset::InstallOptions;

/// Switch tools to the backend the registry now installs them from
///
/// When the registry moves a tool to another backend, mise keeps installing it
/// from the backend recorded in mise.lock and warns that the registry has a
/// newer one. This moves those lock entries to the registry's backend at the
/// same versions, relocks them to record the new backend's checksums and URLs,
/// and reinstalls installed versions so they come from the new backend.
#[derive(Debug, usage_rs::Args)]
#[usage(
    example(
        r###"mise backends switch communique"###,
        help = r###"Move communique to the registry's current backend"###
    ),
    example(
        r###"mise backends switch --dry-run"###,
        help = r###"List every locked tool the registry now installs from another backend"###
    ),
    verbatim_doc_comment
)]
pub(super) struct BackendsSwitch {
    /// Tools to switch
    ///
    /// Defaults to every configured tool whose locked backend the registry
    /// has replaced.
    #[usage(value_name = "TOOL", verbatim_doc_comment)]
    tool: Vec<String>,

    /// Show what would switch without changing the lockfile or installs
    #[usage(long, short = 'n')]
    dry_run: bool,
}

/// One tool's move in one lockfile.
struct Switch {
    short: String,
    from: String,
    to: String,
    lockfile: PathBuf,
}

impl BackendsSwitch {
    pub(super) async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        let switches = self.find_switches(&config).await?;
        if switches.is_empty() {
            if self.tool.is_empty() {
                miseprintln!("every locked tool already uses the registry's backend");
                return Ok(());
            }
            bail!(
                "{} not locked to a backend the registry has replaced",
                self.tool.join(", ")
            );
        }

        let mut by_lockfile: BTreeMap<&PathBuf, Vec<&Switch>> = BTreeMap::new();
        for switch in &switches {
            by_lockfile
                .entry(&switch.lockfile)
                .or_default()
                .push(switch);
        }
        for (path, switches) in &by_lockfile {
            let mut lockfile = Lockfile::read(path)?;
            for switch in switches {
                let registry = crate::registry::REGISTRY.get(switch.short.as_str());
                let versions = lockfile.switch_backend(&switch.short, &switch.from, |version| {
                    // Each version moves to the backend the registry picks for
                    // it, which only differs from `to` for version-dependent
                    // backends.
                    registry
                        .and_then(|tool| tool.backends_for_version(Some(version)).first().copied())
                        .map(str::to_string)
                        .filter(|backend| backend != &switch.from)
                });
                if versions.is_empty() {
                    continue;
                }
                let prefix = if self.dry_run {
                    "would switch"
                } else {
                    "switching"
                };
                info!(
                    "{prefix} {}@{} from {} to {} in {}",
                    switch.short,
                    versions.join(", "),
                    switch.from,
                    switch.to,
                    display_path(path)
                );
            }
            if !self.dry_run {
                lockfile.write(path)?;
            }
        }
        if self.dry_run {
            return Ok(());
        }

        // The rewritten entries carry no artifact data yet; relocking records
        // the new backend's checksums and URLs at the locked versions.
        lockfile::invalidate_caches();
        let shorts: BTreeSet<&str> = switches.iter().map(|s| s.short.as_str()).collect();
        let tool = shorts
            .iter()
            .map(|short| ToolArg::from_str(short))
            .collect::<Result<Vec<_>>>()?;
        Lock {
            tool,
            global: false,
            jobs: None,
            dry_run: false,
            platform: vec![],
            bump: false,
            json: false,
            local: false,
            minimum_release_age: None,
            upgrade: false,
        }
        .run()
        .await?;

        self.reinstall(&shorts).await
    }

    /// Every configured tool, or each named one, that a lock entry keeps on a
    /// backend the registry has replaced.
    async fn find_switches(&self, config: &std::sync::Arc<Config>) -> Result<Vec<Switch>> {
        let ts = config.get_toolset().await?;
        let mut switches = vec![];
        let mut seen = BTreeSet::new();
        for (_, tv) in ts.list_current_versions() {
            let short = tv.short().to_string();
            if !self.tool.is_empty() && !self.tool.contains(&short) {
                continue;
            }
            let Some((from, to)) = tv.ba().superseded_locked_backend(&tv.version) else {
                continue;
            };
            let Some((lockfile, _)) =
                lockfile::lockfile_path_for_tool_source(config, tv.request.source())
            else {
                continue;
            };
            if seen.insert((short.clone(), lockfile.clone())) {
                switches.push(Switch {
                    short,
                    from,
                    to,
                    lockfile,
                });
            }
        }
        Ok(switches)
    }

    /// Reinstall the switched tools' installed versions from the new backend.
    /// Installs are keyed by tool and version, not backend, so an install from
    /// the old backend would otherwise keep satisfying the new lock entry.
    async fn reinstall(&self, shorts: &BTreeSet<&str>) -> Result<()> {
        let mut config = Config::reset().await?;
        let mut ts = config.get_toolset().await?.clone();
        let requests = ts
            .list_current_versions()
            .into_iter()
            .filter(|(backend, tv)| {
                shorts.contains(tv.short()) && backend.is_version_installed(&config, tv, false)
            })
            .map(|(_, tv)| tv.request)
            .collect::<Vec<_>>();
        if requests.is_empty() {
            return Ok(());
        }
        let opts = InstallOptions {
            reason: "backends switch".to_string(),
            force: true,
            ..Default::default()
        };
        ts.install_all_versions(&mut config, requests, &opts)
            .await?;
        Ok(())
    }
}
