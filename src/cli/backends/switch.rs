use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;

use eyre::{Result, bail};

use crate::args::ToolArg;
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

        let missing = self.switch(switches).await?;
        if !missing.is_empty() {
            bail!(
                "did not switch {}: the entry is not in that lockfile yet (a legacy monorepo lockfile, for example); run `mise lock` first",
                missing.join(", ")
            );
        }
        Ok(())
    }

    /// Rewrite, relock, and reinstall the switches. Returns the switches whose
    /// entries were not found in their lockfile.
    async fn switch(&self, switches: Vec<Switch>) -> Result<Vec<String>> {
        let mut by_lockfile: BTreeMap<&PathBuf, Vec<&Switch>> = BTreeMap::new();
        for switch in &switches {
            by_lockfile
                .entry(&switch.lockfile)
                .or_default()
                .push(switch);
        }
        let mut switched: BTreeSet<(String, String)> = BTreeSet::new();
        let mut missing = vec![];
        // Each lockfile's switched tools and the platforms it covered before the
        // rewrite cleared the switched entries' artifacts.
        let mut relocks: Vec<(&PathBuf, BTreeSet<String>, Vec<String>)> = vec![];
        // Entries that had artifact data must get the new backend's back.
        let mut needs_platforms: Vec<(&PathBuf, String, String)> = vec![];
        // Restored if writing or relocking fails, so a failed switch never
        // leaves entries on the new backend without artifact data.
        let mut originals: Vec<(&PathBuf, Option<String>)> = vec![];
        let mut rewritten: Vec<(&PathBuf, Lockfile)> = vec![];
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
                let not_moved = switch
                    .versions
                    .iter()
                    .filter(|v| !moved.iter().any(|(moved, _, _)| moved == *v))
                    .cloned()
                    .collect::<Vec<_>>();
                if !not_moved.is_empty() {
                    missing.push(format!(
                        "{}@{} in {}",
                        switch.short,
                        not_moved.join(", "),
                        display_path(path)
                    ));
                }
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
                if !moved.is_empty() {
                    tools.insert(switch.short.clone());
                }
                switched.extend(moved.into_iter().map(|(v, _, _)| (switch.short.clone(), v)));
            }
            // A lockfile where nothing moved is left alone: not rewritten,
            // relocked, or snapshotted.
            if tools.is_empty() {
                continue;
            }
            relocks.push((path, tools, platforms));
            if !self.dry_run {
                originals.push((path, crate::file::read_to_string(path).ok()));
                rewritten.push((path, lockfile));
            }
        }
        if self.dry_run || switched.is_empty() {
            return Ok(missing);
        }

        // Snapshot each lockfile and its graph sidecar directory: a relock can
        // replace or prune sidecars, so restoring the lockfile text alone could
        // leave it pointing at files that are gone.
        let mut snapshots = originals
            .iter()
            .map(|(path, content)| Snapshot::take(path, content.clone()))
            .collect::<Result<Vec<_>>>()?;
        snapshots.sort_by(|a, b| a.sidecars.cmp(&b.sidecars));
        let result = self
            .write_and_relock(rewritten, relocks, &needs_platforms)
            .await;
        if let Err(err) = result {
            // Attempt every restore and keep the original error. Parents
            // restore before the nested sidecar roots of local lockfiles.
            let failed = snapshots
                .into_iter()
                .filter_map(|snapshot| {
                    let path = display_path(&snapshot.lockfile);
                    snapshot.restore().err().map(|e| format!("{path}: {e}"))
                })
                .collect::<Vec<_>>();
            lockfile::invalidate_caches();
            if failed.is_empty() {
                return Err(err.wrap_err(
                    "could not switch to the new backend; restored the previous lockfiles",
                ));
            }
            return Err(err.wrap_err(format!(
                "could not switch to the new backend, and could not restore {}",
                failed.join(", ")
            )));
        }

        // The lockfiles are switched and complete at this point; a failed
        // reinstall only leaves installs from the old backend in place.
        if let Err(err) = self.reinstall(&switched).await {
            let tools = switched
                .iter()
                .map(|(short, version)| format!("{short}@{version}"))
                .collect::<Vec<_>>()
                .join(" ");
            return Err(err.wrap_err(format!(
                "switched the lockfiles, but reinstalling from the new backend failed; run `mise install --force {tools}` to finish"
            )));
        }
        Ok(missing)
    }

    /// Write the rewritten lockfiles and relock them under the new backend,
    /// failing if an entry that had artifact data did not get the new one's.
    async fn write_and_relock(
        &self,
        rewritten: Vec<(&PathBuf, Lockfile)>,
        relocks: Vec<(&PathBuf, BTreeSet<String>, Vec<String>)>,
        needs_platforms: &[(&PathBuf, String, String)],
    ) -> Result<()> {
        for (path, lf) in rewritten {
            lf.write(path)?;
        }

        // The rewritten entries carry no artifact data yet. Relock each
        // lockfile on its own, for the platforms it already covered, to record
        // the new backend's checksums and URLs at those versions.
        lockfile::invalidate_caches();
        for (path, tools, platforms) in relocks {
            let tool = tools
                .iter()
                .map(|short| ToolArg::from_str(short))
                .collect::<Result<Vec<_>>>()?;
            Lock {
                platform: platforms,
                lockfiles: Some(BTreeSet::from([path.clone()])),
                ..self.lock(tool)
            }
            .run()
            .await?;
        }
        let missing = needs_platforms
            .iter()
            .filter(|(path, short, version)| {
                !Lockfile::read(path).is_ok_and(|lf| lf.has_platforms(short, version))
            })
            .map(|(_, short, version)| format!("{short}@{version}"))
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            bail!(
                "the new backend recorded no artifacts for {}",
                missing.join(", ")
            );
        }
        Ok(())
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
    /// every config source (mise.toml, idiomatic version files, .tool-versions)
    /// on its own, so a project tool cannot hide a global one with the same
    /// name. Each tool maps to its lockfile the way `mise lock` maps it.
    async fn scoped_versions(&self, config: &Arc<Config>) -> Result<Vec<(PathBuf, ToolVersion)>> {
        let targets: BTreeSet<PathBuf> = self
            .lock(vec![])
            .lockfile_targets(config)
            .into_keys()
            .collect();
        let mut versions = vec![];
        for cf in config.config_files.values() {
            let mut ts: Toolset = cf.to_tool_request_set()?.into();
            ts.resolve_with_opts(config, &ResolveOptions::default())
                .await?;
            for (_, tv) in ts.list_current_versions() {
                if let Some((lockfile, _)) =
                    lockfile::lockfile_path_for_tool_source(config, tv.request.source())
                    && targets.contains(&lockfile)
                {
                    versions.push((lockfile, tv));
                }
            }
        }
        Ok(versions)
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

/// A lockfile and its graph sidecar directory as they were before the switch.
struct Snapshot {
    lockfile: PathBuf,
    content: Option<String>,
    sidecars: PathBuf,
    /// A copy of `sidecars`, or `None` when the directory did not exist.
    sidecar_copy: Option<tempfile::TempDir>,
}

impl Snapshot {
    fn take(lockfile: &Path, content: Option<String>) -> Result<Self> {
        // A symlinked lockfile keeps its sidecars beside the target, where
        // reads and writes resolve it.
        let target = if lockfile.is_symlink() {
            std::fs::canonicalize(lockfile).unwrap_or_else(|_| lockfile.to_path_buf())
        } else {
            lockfile.to_path_buf()
        };
        let sidecars = lockfile::sidecar_root(&target);
        let sidecar_copy = if sidecars.is_dir() {
            let copy = tempfile::tempdir()?;
            crate::file::copy_dir_all_preserve_symlinks(&sidecars, &copy.path().join("sidecars"))?;
            Some(copy)
        } else {
            None
        };
        Ok(Self {
            lockfile: lockfile.to_path_buf(),
            content,
            sidecars,
            sidecar_copy,
        })
    }

    fn restore(self) -> Result<()> {
        // Restore the sidecars even when the lockfile text cannot be, and keep
        // the backup whenever either step fails.
        let lockfile_restored = match &self.content {
            Some(content) => crate::file::write(&self.lockfile, content),
            None => Ok(()),
        };
        let sidecars_restored = (|| -> Result<()> {
            if self.sidecars.exists() {
                crate::file::remove_all(&self.sidecars)?;
            }
            if let Some(copy) = &self.sidecar_copy {
                crate::file::copy_dir_all_preserve_symlinks(
                    &copy.path().join("sidecars"),
                    &self.sidecars,
                )?;
            }
            Ok(())
        })();
        let err = match (lockfile_restored, sidecars_restored) {
            (Ok(()), Ok(())) => return Ok(()),
            (Err(err), Ok(())) | (Ok(()), Err(err)) => err,
            (Err(lockfile), Err(sidecars)) => lockfile.wrap_err(format!("{sidecars:#}")),
        };
        match self.sidecar_copy {
            // Keep the backup instead of letting the temp dir delete it.
            Some(copy) => Err(err.wrap_err(format!(
                "the previous sidecars are kept in {}",
                display_path(copy.keep().join("sidecars"))
            ))),
            None => Err(err),
        }
    }
}
