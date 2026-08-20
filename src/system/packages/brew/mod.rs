//! Homebrew formulae without Homebrew.
//!
//! mise installs homebrew/core bottles directly into the canonical prefix
//! (/opt/homebrew on arm64 macOS, /home/linuxbrew/.linuxbrew on Linux) —
//! fetching metadata from formulae.brew.sh, downloading bottles from
//! ghcr.io, and doing the same relocation/codesigning work `brew` does at
//! pour time. mise never shells out to brew to pour a bottle. For supported,
//! fully finalized lifecycle plans, it writes Homebrew-compatible receipts so
//! real Homebrew can adopt mise-poured kegs. Unsupported lifecycle vocabulary
//! fails before mutation.
//!
//! On Linux, formulae without a usable bottle can be built from source, still
//! without Homebrew: mise provisions a mise-managed ruby and evaluates the
//! formula with its own Formula-DSL shim (see source.rs and shim.rb). macOS
//! source builds fail closed because its sandbox cannot prove that detached
//! descendants have stopped mutating the keg; compatible bottles remain
//! supported there.
//!
//! Scope: formulae only. Casks are implemented by the sibling `brew-cask`
//! manager. Services are not implemented. homebrew/core formulae use mise's
//! direct pour path; fully-qualified third-party tap formulae use direct tap
//! metadata when the tap publishes it. mise never shells out to `brew`.

use async_trait::async_trait;
use eyre::{WrapErr, bail};

use super::{InstallOpts, PackageRequest, PackageState, PackageStatus, SystemPackageManager};
use crate::result::Result;
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::ui::progress_report::{ProgressIcon, SingleReport};

mod api;
mod cask;
mod elf;
mod fetch;
mod lifecycle;
mod macho;
mod maintenance;
mod pour;
mod prefix;
mod relocate;
mod resolve;
mod source;
mod tag;

pub struct BrewManager {}
pub use cask::{BrewCaskManager, apply_cask_prune_plan, cask_prune_plan};
pub use maintenance::{apply_prune_plan, default_tap_url, linked_formulae, prune_plan};

struct PreparedBottleInstall {
    tag: String,
    bottle: api::BottleFile,
    archive: fetch::VerifiedArtifact,
    oci_metadata: Option<fetch::OciBottleMetadata>,
}

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
        let closure = resolve::resolve_closure_with_taps(pkgs).await?;
        for rf in &closure {
            if rf.on_request
                && !roots.contains(&rf.formula.name)
                && let Some(alias) = roots.iter().find(|r| rf.formula.aliases.contains(r))
            {
                warn!(
                    "'{alias}' is an alias of '{}' — use the canonical name in [bootstrap.packages] \
                     so `mise bootstrap packages status` can track it",
                    rf.formula.name
                );
            }
        }
        let mut to_pour: Vec<_> = vec![];
        let mut to_repair = vec![];
        for rf in &closure {
            // a malformed version is an error, not "already poured"
            let pkg_version = rf.formula.pkg_version()?;
            let keg = pour::keg_path(&rf.formula.name, &pkg_version);
            if keg.is_dir() {
                let health = pour::installed_formula_health(&rf.formula.name, &pkg_version);
                match health.kind {
                    pour::FormulaHealthKind::Healthy => {}
                    pour::FormulaHealthKind::Repairable => to_repair.push((rf, health)),
                    pour::FormulaHealthKind::ReinstallRequired => to_pour.push(rf),
                }
            } else {
                to_pour.push(rf);
            }
        }
        if to_pour.is_empty() && to_repair.is_empty() {
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
        // Compile every lifecycle that can mutate before the first extraction
        // or source build. Current formulae outside this mutation set are not
        // rejected for lifecycle types mise does not need to execute.
        let mut prepared_lifecycles = prepare_lifecycles(&to_pour)?;
        let repair_formulae = to_repair.iter().map(|(rf, _)| *rf).collect::<Vec<_>>();
        let mut prepared_repairs = prepare_lifecycles(&repair_formulae)?;
        // A bottle embeds its build-time formula snapshot. That snapshot may
        // differ from the current tap source without a version change, so
        // legacy repair validates against the checksum-pinned bottle rather
        // than incorrectly treating the tap source checksum as bottle
        // provenance. All lifecycle plans are compiled before these downloads.
        for ((rf, health), lifecycle) in to_repair.iter().zip(&mut prepared_repairs) {
            if !health.mise_owned || !lifecycle::requires_legacy_snapshot_evidence(lifecycle) {
                continue;
            }
            match health.poured_from_bottle {
                Some(true) => {
                    let pkg_version = rf.formula.pkg_version()?;
                    let Some((_tag, bottle)) = rf.formula.bottle_files().and_then(tag::select)
                    else {
                        bail!(
                            "brew:{} requires reinstall: installed receipt says bottle, but no compatible checksum-pinned bottle is available for legacy repair",
                            rf.formula.name
                        )
                    };
                    let tarball = fetch::fetch_bottle(
                        &rf.formula.name,
                        &pkg_version,
                        bottle,
                        None,
                    )
                    .await
                    .wrap_err_with(|| {
                        format!(
                            "brew:{} cannot verify legacy bottle provenance; retry online or reinstall",
                            rf.formula.name
                        )
                    })?;
                    lifecycle.bind_bottle_formula_snapshot_sha256(
                        pour::bottle_formula_snapshot_sha256(
                            &rf.formula.name,
                            &pkg_version,
                            &tarball,
                        )?,
                    )?;
                }
                Some(false) => {}
                None => bail!(
                    "brew:{} requires reinstall: legacy receipt does not record bottle/source provenance",
                    rf.formula.name
                ),
            }
        }
        for ((_, health), lifecycle) in to_repair.iter().zip(&prepared_repairs) {
            pour::preflight_formula_repair(health, lifecycle)?;
        }
        if opts.dry_run {
            prefix::bootstrap(true)?;
            for ((_, health), lifecycle) in to_repair.iter().zip(&prepared_repairs) {
                pour::repair_formula(health, lifecycle, true).await?;
            }
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
        // Authenticate and retain every bottle before the first prefix,
        // repair, or lifecycle mutation. Snapshot authorization and pour then
        // consume the same anonymous descriptor, so a mutable cache pathname
        // cannot swap bytes between preflight and extraction.
        let mut prepared_bottles = Vec::with_capacity(to_pour.len());
        for (rf, lifecycle) in to_pour.iter().zip(&mut prepared_lifecycles) {
            let prepared = if source::has_bottle(&rf.formula) {
                let pkg_version = rf.formula.pkg_version()?;
                let Some((tag, bottle)) = rf.formula.bottle_files().and_then(tag::select) else {
                    bail!(
                        "brew:{} lost its selected bottle during preflight",
                        rf.formula.name
                    )
                };
                let archive =
                    fetch::fetch_bottle(&rf.formula.name, &pkg_version, bottle, None).await?;
                lifecycle.bind_bottle_formula_snapshot_sha256(
                    pour::bottle_formula_snapshot_sha256(&rf.formula.name, &pkg_version, &archive)?,
                )?;
                let oci_metadata =
                    fetch::fetch_oci_bottle_metadata(&rf.formula.name, &pkg_version, &tag, bottle)
                        .await?;
                Some(PreparedBottleInstall {
                    tag,
                    bottle: bottle.clone(),
                    archive,
                    oci_metadata,
                })
            } else {
                None
            };
            prepared_bottles.push(prepared);
        }
        if prefix::sudo_invoking_user().is_some() {
            warn!(
                "running under sudo — poured files will be owned by root; run \
                 `mise bootstrap packages apply` without sudo instead (mise elevates itself \
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
        mpr.init_footer(false, "install", to_pour.len() + to_repair.len());
        for ((rf, health), lifecycle) in to_repair.iter().zip(&prepared_repairs) {
            let name = &rf.formula.name;
            let pr: Box<dyn SingleReport> = mpr.add(&format!("brew:{name}"));
            pr.set_message("repair lifecycle".to_string());
            if let Err(error) = pour::repair_formula(health, lifecycle, false).await {
                pr.finish_with_icon("failed".to_string(), ProgressIcon::Error);
                mpr.footer_finish();
                return Err(error);
            }
            pr.finish_with_message(health.version.clone());
            mpr.footer_inc(1);
        }
        for ((rf, lifecycle), prepared_bottle) in to_pour
            .iter()
            .zip(&mut prepared_lifecycles)
            .zip(&prepared_bottles)
        {
            let name = &rf.formula.name;
            let pkg_version = rf.formula.pkg_version()?;
            let pr: Box<dyn SingleReport> = mpr.add(&format!("brew:{name}"));
            // branch on the same predicate the upfront classification used
            let installed = match prepared_bottle {
                Some(prepared_bottle) => {
                    async {
                        pour::pour(pour::BottlePour {
                            rf,
                            tag: &prepared_bottle.tag,
                            bottle: &prepared_bottle.bottle,
                            oci_metadata: prepared_bottle.oci_metadata.as_ref(),
                            tarball: &prepared_bottle.archive,
                            closure: &closure,
                            lifecycle,
                            pr: &*pr,
                        })
                        .await?;
                        Ok(pkg_version.clone())
                    }
                    .await
                }
                None => source::build(rf, &closure, lifecycle, &*pr)
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
            pr.finish_with_message(version);
            mpr.footer_inc(1);
        }
        mpr.footer_finish();
        // a glibc poured in this run repoints <prefix>/lib/ld.so at it
        prefix::setup_linux_runtime()?;
        Ok(())
    }
}

fn prepare_lifecycles(
    to_pour: &[&resolve::ResolvedFormula],
) -> Result<Vec<lifecycle::PreparedFormulaLifecycle>> {
    to_pour
        .iter()
        .map(|rf| {
            let version = rf.formula.pkg_version()?;
            lifecycle::prepare(&rf.formula, &pour::keg_path(&rf.formula.name, &version))
                .wrap_err_with(|| {
                    format!(
                        "failed to prepare brew:{} lifecycle; no package state was changed",
                        rf.formula.name
                    )
                })
        })
        .collect()
}

#[async_trait(?Send)]
impl SystemPackageManager for BrewManager {
    fn name(&self) -> &str {
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
        let mut statuses = Vec::with_capacity(pkgs.len());
        for req in pkgs {
            let linked_name = request_formula_name(&req.name);
            let linked = pour::installed_closure_health(linked_name).map(|health| {
                let reason = (health.kind != pour::FormulaHealthKind::Healthy)
                    .then(|| health.reasons.join("; "));
                (health.version, reason)
            });
            let state = linked_package_state(&req.version, linked);
            statuses.push(PackageStatus {
                request: req.clone(),
                state,
            });
        }
        Ok(statuses)
    }

    async fn install(&self, pkgs: &[PackageRequest], opts: &InstallOpts) -> Result<()> {
        let (tapped, core) = self.split_tapped(pkgs);
        if !core.is_empty() {
            let core = core
                .into_iter()
                .map(normalize_core_request)
                .collect::<Vec<_>>();
            self.install_via_pour(&core, opts).await?;
        }
        if !tapped.is_empty() {
            let tapped = tapped.into_iter().cloned().collect::<Vec<_>>();
            self.install_via_pour(&tapped, opts).await?;
        }
        Ok(())
    }

    async fn upgrade(&self, pkgs: &[PackageRequest], opts: &InstallOpts) -> Result<()> {
        let (tapped, core) = self.split_tapped(pkgs);
        if !core.is_empty() {
            let core = core
                .into_iter()
                .map(normalize_core_request)
                .collect::<Vec<_>>();
            self.install_via_pour(&core, opts).await?;
        }
        if !tapped.is_empty() {
            let tapped = tapped.into_iter().cloned().collect::<Vec<_>>();
            self.install_via_pour(&tapped, opts).await?;
        }
        Ok(())
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

/// Return the formula name used by active records for core and tapped requests.
fn request_formula_name(name: &str) -> &str {
    if is_tapped_formula(name) {
        tapped_formula_name(name)
    } else {
        core_formula_name(name)
    }
}

/// Classify the active keg while preserving version mismatch precedence over repair.
fn linked_package_state(
    requested: &Option<String>,
    linked: Option<(String, Option<String>)>,
) -> PackageState {
    match linked {
        // a pin matches the keg version exactly or up to its revision suffix
        // ("17.5" matches keg "17.5_1")
        Some((version, _))
            if requested.as_ref().is_some_and(|requested| {
                version != *requested && !version.starts_with(&format!("{requested}_"))
            }) =>
        {
            PackageState::VersionMismatch { installed: version }
        }
        Some((version, Some(reason))) => PackageState::NeedsRepair {
            installed: version,
            reason,
        },
        Some((version, None)) => PackageState::Installed { version },
        None => PackageState::Missing,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn resolved_formula(name: &str, steps: Vec<serde_json::Value>) -> resolve::ResolvedFormula {
        resolve::ResolvedFormula {
            formula: serde_json::from_value(serde_json::json!({
                "name": name,
                "versions": {"stable": "1"},
                "bottle": {},
                "post_install_steps": steps
            }))
            .unwrap(),
            tap_raw_base: None,
            on_request: false,
        }
    }

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

    #[test]
    fn version_mismatch_takes_precedence_over_record_repair() {
        assert_eq!(
            linked_package_state(
                &Some("2.0".to_string()),
                Some(("1.0".to_string(), Some("missing record".to_string())))
            ),
            PackageState::VersionMismatch {
                installed: "1.0".to_string()
            }
        );
        assert_eq!(
            linked_package_state(
                &Some("1.0".to_string()),
                Some(("1.0_1".to_string(), Some("missing record".to_string())))
            ),
            PackageState::NeedsRepair {
                installed: "1.0_1".to_string(),
                reason: "missing record".to_string()
            }
        );
    }

    #[test]
    fn prepares_only_formulae_in_mutation_set() {
        let current_postgres = resolved_formula(
            "postgresql@17",
            vec![serde_json::json!({
                "type": "init_data_dir",
                "path": {"base": "var", "path": "postgresql@17"},
                "using": "postgresql_initdb"
            })],
        );
        let mutating = resolved_formula("hello", vec![]);
        prepare_lifecycles(&[&mutating]).unwrap();

        let error = prepare_lifecycles(&[&current_postgres]).unwrap_err();
        assert!(error.to_string().contains("no package state was changed"));
    }
}
