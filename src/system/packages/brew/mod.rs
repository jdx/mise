//! Homebrew formulae without Homebrew.
//!
//! mise installs homebrew/core bottles directly into the canonical prefix
//! (/opt/homebrew on arm64 macOS, /home/linuxbrew/.linuxbrew on Linux) —
//! fetching metadata from formulae.brew.sh, downloading bottles from
//! ghcr.io, and doing the same relocation/codesigning work `brew` does at
//! pour time. mise never shells out to brew; the receipts it writes are
//! brew-compatible, so a real Homebrew sees mise-poured kegs as its own.
//!
//! Scope: formulae only. Taps require evaluating Ruby and are unsupported;
//! casks and services are not implemented.

use async_trait::async_trait;
use eyre::bail;

use super::{InstallOpts, PackageRequest, PackageState, PackageStatus, SystemPackageManager};
use crate::result::Result;

mod api;
mod elf;
mod fetch;
mod macho;
mod pour;
mod prefix;
mod relocate;
mod resolve;
mod state;
mod tag;

pub struct BrewManager {}

impl BrewManager {
    pub fn new() -> Self {
        Self {}
    }

    async fn install_via_pour(&self, pkgs: &[PackageRequest], opts: &InstallOpts) -> Result<()> {
        // bottles only exist for a formula's current version — versioning is
        // expressed in the formula name itself (postgresql@17)
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
        if opts.dry_run {
            prefix::bootstrap(true)?;
            for rf in &to_pour {
                miseprintln!(
                    "pour {}/{} ({})",
                    rf.formula.name,
                    rf.formula.pkg_version()?,
                    if rf.on_request {
                        "requested"
                    } else {
                        "dependency"
                    },
                );
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
        let mut ledger = state::Ledger::load();
        for rf in &to_pour {
            let name = &rf.formula.name;
            let pkg_version = rf.formula.pkg_version()?;
            let Some(files) = rf.formula.bottle_files() else {
                bail!("{name} has no bottles (source-only formulae are unsupported)");
            };
            let Some((tag, bottle)) = tag::select(files) else {
                bail!(
                    "{name} has no bottle for this machine (available: {})",
                    files.keys().cloned().collect::<Vec<_>>().join(", "),
                );
            };
            info!("brew: pouring {name} {pkg_version}");
            let tarball = fetch::fetch_bottle(name, &pkg_version, bottle).await?;
            pour::pour(rf, &tag, bottle, &tarball, &closure).await?;
            ledger.record(name, &pkg_version, rf.on_request);
            ledger.save()?;
        }
        // a glibc poured in this run repoints <prefix>/lib/ld.so at it
        prefix::setup_linux_runtime()?;
        Ok(())
    }
}

#[async_trait]
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
