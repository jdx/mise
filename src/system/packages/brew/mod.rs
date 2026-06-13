//! Homebrew formulae without Homebrew.
//!
//! mise installs homebrew/core bottles directly into the canonical prefix
//! (/opt/homebrew on arm64 macOS, /home/linuxbrew/.linuxbrew on Linux) —
//! fetching metadata from formulae.brew.sh, downloading bottles from
//! ghcr.io, and doing the same relocation/codesigning work `brew` does at
//! pour time. mise never shells out to brew to pour a bottle; the receipts
//! it writes are brew-compatible, so a real Homebrew sees mise-poured kegs
//! as its own.
//!
//! Formulae without a usable bottle are built from source, still without
//! Homebrew: mise provisions a mise-managed ruby and evaluates the formula
//! with its own Formula-DSL shim (see source.rs and shim.rb).
//!
//! Scope: formulae only. Casks and services are not implemented; taps are
//! unsupported (only homebrew/core formulae are served by the API).

use async_trait::async_trait;
use eyre::bail;

use super::{InstallOpts, PackageRequest, PackageState, PackageStatus, SystemPackageManager};
use crate::result::Result;
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::ui::progress_report::{ProgressIcon, SingleReport};

mod api;
mod elf;
mod fetch;
mod macho;
mod pour;
mod prefix;
mod relocate;
mod resolve;
mod source;
mod state;
mod tag;

pub struct BrewManager {}

impl BrewManager {
    pub fn new() -> Self {
        Self {}
    }

    async fn install_via_pour(&self, pkgs: &[PackageRequest], opts: &InstallOpts) -> Result<()> {
        // bottles only exist for a formula's current version — versioning is
        // expressed in the formula name itself (postgresql@17); the CLI
        // filters pinned requests out before calling
        if let Some(p) = pkgs.iter().find(|p| p.version.is_some()) {
            bail!(
                "brew bottles are only published for a formula's current version ('{p}'): \
                 pin via the formula name instead (e.g. \"brew:postgresql@17\")"
            );
        }
        let roots: Vec<String> = pkgs.iter().map(|p| p.name.clone()).collect();
        let closure = resolve::resolve_closure(&roots).await?;
        for rf in &closure {
            if rf.on_request
                && !roots.contains(&rf.formula.name)
                && let Some(alias) = roots.iter().find(|r| rf.formula.aliases.contains(r))
            {
                warn!(
                    "'{alias}' is an alias of '{}' — use the canonical name in [system.packages] \
                     so `mise system status` can track it",
                    rf.formula.name
                );
            }
        }
        let mut to_pour: Vec<_> = vec![];
        for rf in &closure {
            // a malformed version is an error, not "already poured"
            let pkg_version = rf.formula.pkg_version()?;
            if !pour::keg_installed(&rf.formula.name, &pkg_version) {
                to_pour.push(rf);
            }
        }
        if to_pour.is_empty() {
            info!("brew: all formulae already poured");
            return Ok(());
        }
        // formulae without a usable bottle are built from source by
        // evaluating their Ruby with mise's formula shim; reject the ones
        // the builder can't handle before any work happens
        let source_builds: Vec<_> = to_pour
            .iter()
            .filter(|rf| !source::has_bottle(&rf.formula))
            .collect();
        for rf in &source_builds {
            source::check_buildable(&rf.formula)?;
        }
        if opts.dry_run {
            prefix::bootstrap(true)?;
            for rf in &to_pour {
                let origin = if rf.on_request {
                    "requested"
                } else {
                    "dependency"
                };
                if source::has_bottle(&rf.formula) {
                    miseprintln!(
                        "pour {}/{} ({origin})",
                        rf.formula.name,
                        rf.formula.pkg_version()?,
                    );
                } else {
                    miseprintln!(
                        "build {}/{} from source ({origin}, {})",
                        rf.formula.name,
                        rf.formula.pkg_version()?,
                        source::missing_bottle_reason(&rf.formula),
                    );
                }
            }
            return Ok(());
        }
        if prefix::sudo_invoking_user().is_some() {
            warn!(
                "running under sudo — poured files will be owned by root; run \
                 `mise system install` without sudo instead (mise elevates itself \
                 for the one-time prefix setup)"
            );
        }
        prefix::bootstrap(false)?;
        prefix::setup_linux_runtime()?;
        if !source_builds.is_empty() {
            info!(
                "brew: building from source (no bottle for this machine): {}",
                source_builds
                    .iter()
                    .map(|rf| rf.formula.name.clone())
                    .collect::<Vec<_>>()
                    .join(", "),
            );
        }
        let mpr = MultiProgressReport::get();
        // overall [cur/total] header above the per-formula clx jobs, same as
        // tool installs (no-op when only one formula is being installed)
        mpr.init_footer(false, "install", to_pour.len());
        let mut ledger = state::Ledger::load();
        for rf in &to_pour {
            let name = &rf.formula.name;
            let pkg_version = rf.formula.pkg_version()?;
            let pr: Box<dyn SingleReport> = mpr.add(&format!("brew:{name}"));
            // branch on the same predicate the upfront classification used
            let bottle = if source::has_bottle(&rf.formula) {
                rf.formula.bottle_files().and_then(tag::select)
            } else {
                None
            };
            let installed = match bottle {
                Some((tag, bottle)) => {
                    async {
                        let tarball =
                            fetch::fetch_bottle(name, &pkg_version, bottle, Some(&*pr)).await?;
                        pour::pour(rf, &tag, bottle, &tarball, &closure, &*pr).await?;
                        Ok(pkg_version.clone())
                    }
                    .await
                }
                None => source::build(rf, &closure, &*pr)
                    .await
                    .map(|()| pkg_version.clone()),
            };
            let version = match installed {
                Ok(version) => version,
                Err(err) => {
                    pr.finish_with_icon("failed".to_string(), ProgressIcon::Error);
                    // render the final progress state so the error that
                    // propagates from here isn't masked by live jobs
                    mpr.footer_finish();
                    return Err(err);
                }
            };
            ledger.record(name, &version, rf.on_request);
            ledger.save()?;
            pr.finish_with_message(version);
            mpr.footer_inc(1);
        }
        mpr.footer_finish();
        // a glibc poured in this run repoints <prefix>/lib/ld.so at it
        prefix::setup_linux_runtime()?;
        Ok(())
    }
}

#[async_trait(?Send)]
impl SystemPackageManager for BrewManager {
    fn name(&self) -> &'static str {
        "brew"
    }

    fn is_available(&self) -> bool {
        cfg!(all(target_os = "macos", target_arch = "aarch64"))
            || cfg!(all(
                target_os = "linux",
                any(target_arch = "x86_64", target_arch = "aarch64")
            ))
    }

    fn unavailable_reason(&self) -> String {
        "only available on arm64 macos and x86_64/arm64 linux".to_string()
    }

    fn supports_version_pins(&self) -> bool {
        false
    }

    async fn installed(&self, pkgs: &[PackageRequest]) -> Result<Vec<PackageStatus>> {
        // the prefix is the source of truth whether kegs were poured by mise
        // or by a real brew; a formula counts as installed only when its opt
        // symlink resolves to a keg — a Cellar directory without one is a
        // remnant of a failed install and must not mask a retry
        Ok(pkgs
            .iter()
            .map(|req| {
                let state = match pour::linked_version(&req.name) {
                    // a pin matches the keg version exactly or up to its
                    // revision suffix ("17.5" matches keg "17.5_1")
                    Some(version) => match &req.version {
                        Some(requested)
                            if version != *requested
                                && !version.starts_with(&format!("{requested}_")) =>
                        {
                            PackageState::VersionMismatch { installed: version }
                        }
                        _ => PackageState::Installed { version },
                    },
                    None => PackageState::Missing,
                };
                PackageStatus {
                    request: req.clone(),
                    state,
                }
            })
            .collect())
    }

    async fn install(&self, pkgs: &[PackageRequest], opts: &InstallOpts) -> Result<()> {
        self.install_via_pour(pkgs, opts).await
    }
}
