//! Homebrew formulae without Homebrew.
//!
//! On arm64 macOS, mise installs homebrew/core bottles directly into
//! /opt/homebrew — fetching metadata from formulae.brew.sh, downloading
//! bottles from ghcr.io, and doing the same relocation/codesigning work
//! `brew` does at pour time. If a real Homebrew installation is detected,
//! mise delegates to `brew` instead of touching the prefix itself; the
//! receipts we write are brew-compatible, so a later-installed brew adopts
//! mise-poured kegs seamlessly.
//!
//! Scope: formulae only. Taps require evaluating Ruby and are unsupported;
//! casks and services are not implemented.

use async_trait::async_trait;
use eyre::bail;

use super::{InstallOpts, PackageRequest, PackageState, PackageStatus, SystemPackageManager};
use crate::cmd::CmdLineRunner;
use crate::result::Result;

mod api;
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

    async fn install_via_brew(&self, pkgs: &[PackageRequest], opts: &InstallOpts) -> Result<()> {
        let mut args = vec!["install".to_string()];
        args.extend(pkgs.iter().map(|p| p.raw.clone()));
        if opts.dry_run {
            miseprintln!("brew {}", args.join(" "));
            return Ok(());
        }
        info!("$ brew {}", args.join(" "));
        let mut cmd = CmdLineRunner::new("brew");
        for arg in &args {
            cmd = cmd.arg(arg);
        }
        cmd.raw(true).execute()
    }

    async fn install_via_pour(&self, pkgs: &[PackageRequest], opts: &InstallOpts) -> Result<()> {
        let roots: Vec<String> = pkgs.iter().map(|p| p.name.clone()).collect();
        let closure = resolve::resolve_closure(&roots).await?;
        for rf in &closure {
            if rf.on_request && !roots.contains(&rf.formula.name) {
                let alias = roots
                    .iter()
                    .find(|r| rf.formula.aliases.contains(r))
                    .map(|s| s.as_str())
                    .unwrap_or("?");
                warn!(
                    "'{alias}' is an alias of '{}' — use the canonical name in [system.packages] \
                     so `mise system status` can track it",
                    rf.formula.name
                );
            }
        }
        let to_pour: Vec<_> = closure
            .iter()
            .filter(|rf| {
                rf.formula
                    .pkg_version()
                    .map(|v| !pour::keg_installed(&rf.formula.name, &v))
                    .unwrap_or(false)
            })
            .collect();
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
        prefix::bootstrap(false)?;
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
    }

    fn unavailable_reason(&self) -> String {
        "only available on arm64 macos".to_string()
    }

    async fn installed(&self, pkgs: &[PackageRequest]) -> Result<Vec<PackageStatus>> {
        // the Cellar is the source of truth whether kegs were poured by mise
        // or by a real brew
        Ok(pkgs
            .iter()
            .map(|req| {
                let versions = pour::installed_versions(&req.name);
                let state = match versions.first() {
                    Some(version) => PackageState::Installed {
                        version: version.clone(),
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
        if prefix::real_brew_installed() {
            // never share a poured prefix with a real brew — delegate
            debug!("real Homebrew detected, delegating to brew");
            self.install_via_brew(pkgs, opts).await
        } else {
            self.install_via_pour(pkgs, opts).await
        }
    }
}
