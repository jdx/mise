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
//! Scope: formulae only. Casks and services are not implemented. homebrew/core
//! formulae use mise's direct pour path; fully-qualified third-party tap
//! formulae (`owner/tap/formula`) use a real Homebrew installation.

use async_trait::async_trait;
use eyre::{WrapErr, bail, eyre};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

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

const BREW_TIMEOUT: Duration = Duration::from_secs(30 * 60);

pub struct BrewManager {}

impl BrewManager {
    pub fn new() -> Self {
        Self {}
    }

    fn split_tapped<'a>(
        &self,
        pkgs: &'a [PackageRequest],
    ) -> (Vec<&'a PackageRequest>, Vec<&'a PackageRequest>) {
        pkgs.iter().partition(|p| is_tapped_formula(&p.name))
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

    async fn install_via_brew(&self, pkgs: &[&PackageRequest], opts: &InstallOpts) -> Result<()> {
        if pkgs.is_empty() {
            return Ok(());
        }
        let brew = brew_bin_for_tapped(pkgs, opts.dry_run)?;
        ensure_taps(&brew, pkgs, opts.dry_run).await?;
        run_brew(&brew, &["update-if-needed".to_string()], opts.dry_run).await?;
        let mut args = vec!["install".to_string()];
        args.extend(pkgs.iter().map(|p| p.name.clone()));
        run_brew(&brew, &args, opts.dry_run).await
    }

    async fn upgrade_via_brew(&self, pkgs: &[&PackageRequest], opts: &InstallOpts) -> Result<()> {
        if pkgs.is_empty() {
            return Ok(());
        }
        let brew = brew_bin_for_tapped(pkgs, opts.dry_run)?;
        ensure_taps(&brew, pkgs, opts.dry_run).await?;
        run_brew(&brew, &["update-if-needed".to_string()], opts.dry_run).await?;
        let mut args = vec!["upgrade".to_string()];
        args.extend(pkgs.iter().map(|p| p.name.clone()));
        run_brew(&brew, &args, opts.dry_run).await
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
        let brew = brew_bin();
        let mut statuses = Vec::with_capacity(pkgs.len());
        for req in pkgs {
            let linked_name = if is_tapped_formula(&req.name) {
                tapped_formula_name(&req.name)
            } else {
                core_formula_name(&req.name)
            };
            let version = if is_tapped_formula(&req.name) {
                match &brew {
                    Some(brew) => brew_list_version(brew, &req.name)
                        .await?
                        .or_else(|| pour::linked_version(linked_name)),
                    None => pour::linked_version(linked_name),
                }
            } else {
                pour::linked_version(linked_name)
            };
            let state = match version {
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
            statuses.push(PackageStatus {
                request: req.clone(),
                state,
            });
        }
        Ok(statuses)
    }

    async fn install(&self, pkgs: &[PackageRequest], opts: &InstallOpts) -> Result<()> {
        let (tapped, core) = self.split_tapped(pkgs);
        if !tapped.is_empty() {
            brew_bin_for_tapped(&tapped, opts.dry_run)?;
        }
        if !core.is_empty() {
            let core = core
                .into_iter()
                .map(normalize_core_request)
                .collect::<Vec<_>>();
            self.install_via_pour(&core, opts).await?;
        }
        self.install_via_brew(&tapped, opts).await
    }

    async fn upgrade(&self, pkgs: &[PackageRequest], opts: &InstallOpts) -> Result<()> {
        let (tapped, core) = self.split_tapped(pkgs);
        if !tapped.is_empty() {
            brew_bin_for_tapped(&tapped, opts.dry_run)?;
        }
        if !core.is_empty() {
            let core = core
                .into_iter()
                .map(normalize_core_request)
                .collect::<Vec<_>>();
            self.install_via_pour(&core, opts).await?;
        }
        self.upgrade_via_brew(&tapped, opts).await
    }
}

fn is_tapped_formula(name: &str) -> bool {
    crate::system::brew_tap_name(name).is_some()
}

fn tapped_formula_name(name: &str) -> &str {
    name.rsplit('/').next().unwrap_or(name)
}

fn core_formula_name(name: &str) -> &str {
    match split_formula_name(name) {
        Some(("homebrew", "core", formula)) => formula,
        _ => name,
    }
}

fn normalize_core_request(req: &PackageRequest) -> PackageRequest {
    let mut req = req.clone();
    req.name = core_formula_name(&req.name).to_string();
    req
}

fn split_formula_name(name: &str) -> Option<(&str, &str, &str)> {
    let mut parts = name.split('/');
    let owner = parts.next()?;
    let tap = parts.next()?;
    let formula = parts.next()?;
    if parts.next().is_some() || owner.is_empty() || tap.is_empty() || formula.is_empty() {
        None
    } else {
        Some((owner, tap, formula))
    }
}

fn brew_bin() -> Option<PathBuf> {
    crate::file::which("brew").or_else(|| {
        let brew = prefix::prefix().join("bin").join("brew");
        brew.exists().then_some(brew)
    })
}

fn brew_bin_for_run(dry_run: bool) -> Option<PathBuf> {
    brew_bin().or_else(|| dry_run.then(|| PathBuf::from("brew")))
}

fn brew_bin_for_tapped(pkgs: &[&PackageRequest], dry_run: bool) -> Result<PathBuf> {
    brew_bin_for_run(dry_run).ok_or_else(|| {
        eyre!(
            "brew: custom tap formulae require Homebrew to be installed \
             (needed for {})",
            pkgs.iter()
                .map(|p| p.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )
    })
}

fn display_cmd(program: &Path, args: &[String]) -> String {
    std::iter::once(program.to_string_lossy().to_string())
        .chain(args.iter().cloned())
        .map(|s| shell_escape::escape(s.into()).to_string())
        .collect::<Vec<_>>()
        .join(" ")
}

async fn run_brew(brew: &PathBuf, args: &[String], dry_run: bool) -> Result<()> {
    if dry_run {
        miseprintln!("{}", display_cmd(brew, args));
        return Ok(());
    }
    let display = display_cmd(brew, args);
    debug!("$ {display}");
    let mut cmd = tokio::process::Command::new(brew);
    cmd.args(args).stdin(Stdio::null()).kill_on_drop(true);
    let status = tokio::time::timeout(BREW_TIMEOUT, cmd.status())
        .await
        .map_err(|_| eyre!("brew timed out after {:?} running {display}", BREW_TIMEOUT))?
        .wrap_err_with(|| format!("failed to run {display}"))?;
    if !status.success() {
        bail!("{display} failed with {status}");
    }
    Ok(())
}

pub(crate) async fn tap(tap: &str, url: Option<&str>, dry_run: bool) -> Result<()> {
    let brew = brew_bin_for_run(dry_run)
        .ok_or_else(|| eyre::eyre!("brew: Homebrew must be installed to tap {tap}"))?;
    let mut args = vec!["tap".to_string(), tap.to_string()];
    if let Some(url) = url {
        args.push(url.to_string());
    }
    run_brew(&brew, &args, dry_run).await
}

pub(crate) async fn untap(taps: &[String], dry_run: bool) -> Result<()> {
    let brew = brew_bin_for_run(dry_run).ok_or_else(|| {
        eyre::eyre!(
            "brew: Homebrew must be installed to untap {}",
            taps.join(", ")
        )
    })?;
    let mut args = vec!["untap".to_string()];
    args.extend(taps.iter().cloned());
    run_brew(&brew, &args, dry_run).await
}

async fn ensure_taps(brew: &PathBuf, pkgs: &[&PackageRequest], dry_run: bool) -> Result<()> {
    let mut taps = indexmap::IndexMap::<String, Option<String>>::new();
    for pkg in pkgs {
        if let Some(tap) = crate::system::brew_tap_name(&pkg.name) {
            taps.entry(tap.to_string()).or_insert(pkg.tap_url.clone());
        }
    }
    for (tap, url) in taps {
        let mut args = vec!["tap".to_string(), tap];
        if let Some(url) = url {
            args.push(url);
        }
        run_brew(brew, &args, dry_run).await?;
    }
    Ok(())
}

async fn brew_list_version(brew: &PathBuf, name: &str) -> Result<Option<String>> {
    let args = vec![
        "list".to_string(),
        "--versions".to_string(),
        name.to_string(),
    ];
    let display = display_cmd(brew, &args);
    debug!("$ {display}");
    let mut cmd = tokio::process::Command::new(brew);
    cmd.args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let output = tokio::time::timeout(BREW_TIMEOUT, cmd.output())
        .await
        .map_err(|_| eyre!("brew timed out after {:?} running {display}", BREW_TIMEOUT))?
        .wrap_err_with(|| format!("failed to run {display}"))?;
    if !output.status.success() {
        return Ok(None);
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout.lines().find_map(|line| {
        let mut tokens = line.split_whitespace();
        tokens.next();
        tokens.next().map(str::to_string)
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tapped_formula_detection() {
        assert!(!is_tapped_formula("jq"));
        assert!(!is_tapped_formula("postgresql@17"));
        assert!(!is_tapped_formula("homebrew/core/jq"));
        assert!(is_tapped_formula("railwaycat/emacsmacport/emacs-mac"));
        assert_eq!(core_formula_name("homebrew/core/jq"), "jq");
        assert_eq!(core_formula_name("jq"), "jq");
        assert_eq!(
            tapped_formula_name("railwaycat/emacsmacport/emacs-mac"),
            "emacs-mac"
        );
    }
}
