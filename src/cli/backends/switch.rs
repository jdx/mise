use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;

use eyre::{Result, bail};

use crate::cli::args::ToolArg;
use crate::cli::lock::Lock;
use crate::config::Config;
use crate::file::display_path;
use crate::lockfile::{self, Lockfile};
use crate::toolset::{InstallOptions, ToolVersion};

/// Switch tools to the backend the registry now installs them from
///
/// When the registry moves a tool to another backend, mise keeps installing it
/// from the backend recorded in mise.lock and warns that the registry has a
/// newer one. This moves those lock entries to the registry's backend at the
/// same versions, relocks them to record the new backend's checksums and URLs,
/// and reinstalls installed versions so they come from the new backend.
///
/// It covers the lockfiles `mise lock` writes: the current project's, or the
/// global config's with `--global`.
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
    /// has replaced. A version (`hk@1.58.1`) limits the switch to that version.
    #[usage(value_name = "TOOL@VERSION", verbatim_doc_comment)]
    tool: Vec<ToolArg>,

    /// Show what would switch without changing the lockfile or installs
    #[usage(long, short = 'n')]
    dry_run: bool,

    /// Switch tools in the global config's lockfile instead of the project's
    #[usage(long, short)]
    global: bool,
}

/// One tool's move in one lockfile.
struct Switch {
    short: String,
    from: String,
    to: String,
    lockfile: PathBuf,
    versions: BTreeSet<String>,
}

impl BackendsSwitch {
    pub(super) async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        let switches = self.find_switches(&config).await?;
        if switches.is_empty() {
            let scope = if self.global { "global" } else { "project" };
            if self.tool.is_empty() {
                miseprintln!(
                    "every tool in the {scope} lockfile already uses the registry's backend"
                );
                return Ok(());
            }
            let tools = self.tool.iter().map(|t| t.to_string()).collect::<Vec<_>>();
            bail!(
                "{} not locked to a backend the registry has replaced in the {scope} lockfile",
                tools.join(", ")
            );
        }

        let mut by_lockfile: BTreeMap<&PathBuf, Vec<&Switch>> = BTreeMap::new();
        for switch in &switches {
            by_lockfile
                .entry(&switch.lockfile)
                .or_default()
                .push(switch);
        }
        let mut switched: BTreeSet<(String, String)> = BTreeSet::new();
        // Read before the rewrite clears the switched entries' platforms, so the
        // relock targets the platforms these lockfiles already cover.
        let mut platforms: BTreeSet<String> = BTreeSet::new();
        for (path, switches) in &by_lockfile {
            platforms.extend(
                lockfile::determine_existing_platforms(path)?
                    .iter()
                    .map(|p| p.to_key()),
            );
            let mut lockfile = Lockfile::read(path)?;
            for switch in switches {
                let registry = crate::registry::REGISTRY.get(switch.short.as_str());
                let versions = lockfile.switch_backend(
                    &switch.short,
                    &switch.from,
                    &switch.versions,
                    |version| {
                        // Each version moves to the backend the registry picks
                        // for it, which only differs from `to` for
                        // version-dependent backends.
                        registry
                            .and_then(|tool| {
                                tool.backends_for_version(Some(version)).first().copied()
                            })
                            .map(str::to_string)
                            .filter(|backend| backend != &switch.from)
                    },
                );
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
                switched.extend(versions.into_iter().map(|v| (switch.short.clone(), v)));
            }
            if !self.dry_run {
                lockfile.write(path)?;
            }
        }
        if self.dry_run || switched.is_empty() {
            return Ok(());
        }

        // The rewritten entries carry no artifact data yet; relocking the same
        // scope records the new backend's checksums and URLs at those versions.
        lockfile::invalidate_caches();
        let shorts: BTreeSet<&str> = switched.iter().map(|(short, _)| short.as_str()).collect();
        let tool = shorts
            .iter()
            .map(|short| ToolArg::from_str(short))
            .collect::<Result<Vec<_>>>()?;
        Lock {
            platform: platforms.into_iter().collect(),
            ..self.lock(tool)
        }
        .run()
        .await?;

        self.reinstall(&switched).await
    }

    /// The `mise lock` run whose scope this command covers.
    fn lock(&self, tool: Vec<ToolArg>) -> Lock {
        Lock {
            tool,
            global: self.global,
            jobs: None,
            dry_run: false,
            platform: vec![],
            bump: false,
            json: false,
            local: false,
            minimum_release_age: None,
            upgrade: false,
        }
    }

    fn selected(&self, tv: &ToolVersion, locked: &str) -> bool {
        self.tool.is_empty()
            || self.tool.iter().any(|t| {
                (t.ba.short == tv.short() || t.ba.short == locked)
                    && t.version.as_ref().is_none_or(|v| v == &tv.version)
            })
    }

    /// Every configured tool, or each named one, that a lock entry in this
    /// command's scope keeps on a backend the registry has replaced.
    async fn find_switches(&self, config: &Arc<Config>) -> Result<Vec<Switch>> {
        let targets = self.lock(vec![]).lockfile_targets(config);
        let ts = config.get_toolset().await?;
        let mut switches: Vec<Switch> = vec![];
        for (_, tv) in ts.list_current_versions() {
            let Some((from, to)) = tv.ba().superseded_locked_backend(&tv.version) else {
                continue;
            };
            if !self.selected(&tv, &from) {
                continue;
            }
            let Some((lockfile, _)) =
                lockfile::lockfile_path_for_tool_source(config, tv.request.source())
            else {
                continue;
            };
            if !targets.contains(&lockfile) {
                continue;
            }
            let short = tv.short().to_string();
            match switches
                .iter_mut()
                .find(|s| s.short == short && s.lockfile == lockfile && s.from == from)
            {
                Some(switch) => {
                    switch.versions.insert(tv.version.clone());
                }
                None => switches.push(Switch {
                    short,
                    from,
                    to,
                    lockfile,
                    versions: BTreeSet::from([tv.version.clone()]),
                }),
            }
        }
        Ok(switches)
    }

    /// Reinstall the switched versions that are installed, from the new
    /// backend. Installs are keyed by tool and version, not backend, so an
    /// install from the old backend would otherwise keep satisfying the new
    /// lock entry.
    async fn reinstall(&self, switched: &BTreeSet<(String, String)>) -> Result<()> {
        let mut config = Config::reset().await?;
        let mut ts = config.get_toolset().await?.clone();
        let requests = ts
            .list_current_versions()
            .into_iter()
            .filter(|(backend, tv)| {
                switched.contains(&(tv.short().to_string(), tv.version.clone()))
                    && backend.is_version_installed(&config, tv, false)
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
