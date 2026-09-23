use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;

use eyre::{Result, bail, eyre};

use crate::cli::args::ToolArg;
use crate::cli::lock::Lock;
use crate::config::Config;
use crate::file::display_path;
use crate::lockfile::{self, Lockfile};
use crate::toolset::{InstallOptions, ResolveOptions, ToolVersion, Toolset};

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
    lockfile: PathBuf,
    versions: BTreeSet<String>,
}

impl BackendsSwitch {
    pub(super) async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        let switches = self.find_switches(&config).await?;
        self.ensure_not_shadowed(&config, &switches).await?;
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
        // Each lockfile's switched tools and the platforms it covered before the
        // rewrite cleared the switched entries' artifacts.
        let mut relocks: Vec<(&PathBuf, BTreeSet<String>, Vec<String>)> = vec![];
        // Entries that had artifact data must get the new backend's back.
        let mut needs_platforms: Vec<(&PathBuf, String, String)> = vec![];
        // Restored if relocking fails, so a failed switch never leaves entries on
        // the new backend without artifact data.
        let mut originals: Vec<(&PathBuf, Option<String>)> = vec![];
        for (path, switches) in &by_lockfile {
            let platforms = lockfile::determine_existing_platforms(path)?
                .iter()
                .map(|p| p.to_key())
                .collect::<Vec<_>>();
            let mut tools = BTreeSet::new();
            let mut lockfile = Lockfile::read(path)?;
            for switch in switches {
                let registry = crate::registry::REGISTRY.get(switch.short.as_str());
                let moved = lockfile.switch_backend(
                    &switch.short,
                    &switch.from,
                    &switch.versions,
                    |version| {
                        // Each version moves to the backend the registry picks
                        // for it; version-dependent backends can differ.
                        registry
                            .and_then(|tool| {
                                tool.backends_for_version(Some(version)).first().copied()
                            })
                            .map(str::to_string)
                            .filter(|backend| backend != &switch.from)
                    },
                );
                let prefix = if self.dry_run {
                    "would switch"
                } else {
                    "switching"
                };
                let mut by_backend: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
                for (version, backend, had_platforms) in &moved {
                    by_backend.entry(backend).or_default().push(version);
                    if *had_platforms {
                        needs_platforms.push((path, switch.short.clone(), version.clone()));
                    }
                }
                for (backend, versions) in by_backend {
                    info!(
                        "{prefix} {}@{} from {} to {backend} in {}",
                        switch.short,
                        versions.join(", "),
                        switch.from,
                        display_path(path)
                    );
                }
                tools.insert(switch.short.clone());
                switched.extend(moved.into_iter().map(|(v, _, _)| (switch.short.clone(), v)));
            }
            if !tools.is_empty() {
                relocks.push((path, tools, platforms));
            }
            if !self.dry_run {
                originals.push((path, crate::file::read_to_string(path).ok()));
                lockfile.write(path)?;
            }
        }
        if self.dry_run || switched.is_empty() {
            return Ok(());
        }

        // The rewritten entries carry no artifact data yet. Relock each
        // lockfile on its own, for the platforms it already covered, to record
        // the new backend's checksums and URLs at those versions.
        lockfile::invalidate_caches();
        let mut relocked = Ok(());
        for (path, tools, platforms) in relocks {
            let tool = tools
                .iter()
                .map(|short| ToolArg::from_str(short))
                .collect::<Result<Vec<_>>>()?;
            relocked = Lock {
                platform: platforms,
                lockfiles: Some(BTreeSet::from([path.clone()])),
                ..self.lock(tool)
            }
            .run()
            .await;
            if relocked.is_err() {
                break;
            }
        }
        let relocked = relocked.and_then(|()| {
            let missing = needs_platforms
                .iter()
                .filter(|(path, short, version)| {
                    !Lockfile::read(path).is_ok_and(|lf| lf.has_platforms(short, version))
                })
                .map(|(_, short, version)| format!("{short}@{version}"))
                .collect::<Vec<_>>();
            if missing.is_empty() {
                Ok(())
            } else {
                Err(eyre!(
                    "the new backend recorded no artifacts for {}",
                    missing.join(", ")
                ))
            }
        });
        if let Err(err) = relocked {
            for (path, original) in originals {
                if let Some(original) = original {
                    crate::file::write(path, original)?;
                }
            }
            return Err(err.wrap_err(
                "could not relock under the new backend; restored the previous lockfiles",
            ));
        }

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
            lockfiles: None,
        }
    }

    fn selected(&self, tv: &ToolVersion, locked: &str) -> bool {
        self.tool.is_empty()
            || self.tool.iter().any(|t| {
                (t.ba.short == tv.short() || t.ba.short == locked)
                    && t.version.as_ref().is_none_or(|v| v == &tv.version)
            })
    }

    /// The versions each lockfile in this command's scope locks, resolved from
    /// the config files that lockfile serves. Resolving each config on its own
    /// keeps a project tool from hiding a global one with the same name.
    async fn scoped_versions(&self, config: &Arc<Config>) -> Result<Vec<(PathBuf, ToolVersion)>> {
        let mut versions = vec![];
        for (lockfile, config_paths) in self.lock(vec![]).lockfile_targets(config) {
            for path in config_paths {
                let Some(cf) = config.config_files.get(&path) else {
                    continue;
                };
                let mut ts: Toolset = cf.to_tool_request_set()?.into();
                ts.resolve_with_opts(config, &ResolveOptions::default())
                    .await?;
                versions.extend(
                    ts.list_current_versions()
                        .into_iter()
                        .map(|(_, tv)| (lockfile.clone(), tv)),
                );
            }
        }
        Ok(versions)
    }

    /// `mise lock` locks each tool from the config that wins for it, so it
    /// cannot relock a lock entry whose tool another config shadows (a project
    /// tool with the same name as a global one). Refuse before rewriting it.
    async fn ensure_not_shadowed(&self, config: &Arc<Config>, switches: &[Switch]) -> Result<()> {
        let ts = config.get_toolset().await?;
        let shadowed = ts
            .list_current_versions()
            .into_iter()
            .filter_map(|(_, tv)| {
                let (active, _) =
                    lockfile::lockfile_path_for_tool_source(config, tv.request.source())?;
                switches
                    .iter()
                    .find(|s| s.short == tv.short() && s.lockfile != active)
                    .map(|s| {
                        format!(
                            "{} in {} is shadowed by {}",
                            s.short,
                            display_path(&s.lockfile),
                            tv.request.source()
                        )
                    })
            })
            .collect::<BTreeSet<_>>();
        if shadowed.is_empty() {
            return Ok(());
        }
        bail!(
            "{}; run this from a directory whose config does not set it",
            shadowed.into_iter().collect::<Vec<_>>().join(", ")
        )
    }

    /// Every configured tool, or each named one, that a lock entry in this
    /// command's scope keeps on a backend the registry has replaced.
    async fn find_switches(&self, config: &Arc<Config>) -> Result<Vec<Switch>> {
        let mut switches: Vec<Switch> = vec![];
        for (lockfile, tv) in self.scoped_versions(config).await? {
            if !tv.resolved_from_lockfile() {
                continue;
            }
            let Some((from, _)) = tv.ba().superseded_backend(&tv.version) else {
                continue;
            };
            if !self.selected(&tv, &from) {
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
        let mut requests = vec![];
        for (_, tv) in self.scoped_versions(&config).await? {
            if switched.contains(&(tv.short().to_string(), tv.version.clone()))
                && tv.backend()?.is_version_installed(&config, &tv, false)
            {
                requests.push(tv.request);
            }
        }
        if requests.is_empty() {
            return Ok(());
        }
        let opts = InstallOptions {
            reason: "backends switch".to_string(),
            force: true,
            ..Default::default()
        };
        let mut ts = config.get_toolset().await?.clone();
        ts.install_all_versions(&mut config, requests, &opts)
            .await?;
        Ok(())
    }
}
