//! Native source builds for formulae without a usable bottle.
//!
//! Building a formula means running its Ruby `install` method. mise does
//! this without Homebrew: it provisions a mise-managed ruby (precompiled,
//! via the normal tool machinery), downloads the formula's .rb from
//! homebrew/core (sha256-verified against the API metadata), stages the
//! sha256-verified source archive, and evaluates the formula with the
//! Formula-DSL shim in shim.rb. Build dependencies are poured as bottles
//! beforehand by the regular closure machinery (see resolve.rs), so the
//! build environment points at real kegs in the canonical prefix.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use eyre::{WrapErr, bail};

use super::api::Formula;
use super::pour;
use super::prefix;
use super::resolve::ResolvedFormula;
use super::tag;
use crate::cmd::CmdLineRunner;
use crate::config::{Config, Settings};
use crate::file::{ExtractOptions, ExtractionFormat};
use crate::http::HTTP_FETCH;
use crate::result::Result;
use crate::toolset::{InstallOptions, ToolsetBuilder};
use crate::ui::progress_report::SingleReport;

const SHIM_RB: &str = include_str!("shim.rb");
const HOMEBREW_CORE_RAW: &str = "https://raw.githubusercontent.com/Homebrew/homebrew-core";

/// does this formula have a bottle that can be poured on this machine?
pub fn has_bottle(formula: &Formula) -> bool {
    // undocumented override for testing the source-build pipeline with
    // formulae that do have bottles (comma-separated names)
    if let Ok(force) = crate::env::var("MISE_SYSTEM_BREW_FORCE_SOURCE")
        && force.split(',').any(|f| f.trim() == formula.name)
    {
        return false;
    }
    formula
        .bottle_files()
        .and_then(|files| tag::select(files))
        .is_some()
}

/// why `has_bottle` is false, for log/dry-run output
pub fn missing_bottle_reason(formula: &Formula) -> String {
    match formula.bottle_files() {
        Some(files) if !files.is_empty() => {
            let mut tags: Vec<String> = files.keys().cloned().collect();
            tags.sort();
            format!("bottles exist only for: {}", tags.join(", "))
        }
        _ => "source-only formula, no bottles".to_string(),
    }
}

/// Reject early what the source builder cannot handle, with the reason —
/// checked before any work happens so dry-run and real runs fail alike.
pub fn check_buildable(formula: &Formula) -> Result<()> {
    let Some(src) = formula.stable_url() else {
        bail!("{}: formula has no stable source URL", formula.name);
    };
    if let Some(using) = &src.using {
        bail!(
            "{}: source uses the {using:?} download strategy, which mise cannot build from \
             (and no bottle exists for this machine)",
            formula.name,
        );
    }
    if src.checksum.is_none() {
        bail!("{}: source archive has no sha256 in the API", formula.name);
    }
    // the formula .rb must be pinned to the API snapshot's commit and
    // verifiable — evaluating a newer/unverified formula against older
    // source metadata would build the wrong thing
    if formula.ruby_source_path.is_none() {
        bail!("{}: API metadata has no ruby_source_path", formula.name);
    }
    if formula.tap_git_head.is_none() {
        bail!("{}: API metadata has no tap_git_head", formula.name);
    }
    if formula
        .ruby_source_checksum
        .as_ref()
        .and_then(|c| c.sha256.as_deref())
        .is_none()
    {
        bail!("{}: API metadata has no formula checksum", formula.name);
    }
    Ok(())
}

/// Build a formula from source into its keg and link it.
pub async fn build(
    rf: &ResolvedFormula,
    closure: &[ResolvedFormula],
    pr: &dyn SingleReport,
) -> Result<()> {
    let formula = &rf.formula;
    let name = &formula.name;
    let pkg_version = formula.pkg_version()?;
    check_buildable(formula)?;

    pr.set_message("resolve ruby".to_string());
    let ruby = ruby_bin().await?;
    let formula_rb = fetch_formula_rb(rf, pr).await?;
    let archive = fetch_source(formula, pr).await?;

    let build_root = crate::dirs::CACHE
        .join("system-brew")
        .join("build")
        .join(format!("{name}-{pkg_version}"));
    if build_root.exists() {
        crate::file::remove_all(&build_root)?;
    }
    crate::file::create_dir_all(&build_root)?;
    pr.set_message("extract source".to_string());
    let buildpath = stage_source(&archive, &build_root, &source_basename(formula))?;
    let shim_path = build_root.join("mise-brew-shim.rb");
    crate::file::write(&shim_path, SHIM_RB)?;

    // formulae bake the final keg path into binaries, so the build installs
    // straight into the Cellar (same as brew); a failed build removes the keg
    let keg = pour::keg_path(name, &pkg_version);
    if keg.exists() {
        crate::file::remove_all(&keg)?;
    }

    pr.set_message("build from source".to_string());
    let cmd = CmdLineRunner::new(&ruby)
        .arg(&shim_path)
        .current_dir(&buildpath)
        .envs(build_env(
            rf,
            closure,
            &pkg_version,
            &buildpath,
            &formula_rb,
        ))
        .with_pr(pr);
    let built = cmd.execute_async().await;
    if let Err(err) = built {
        let _ = crate::file::remove_all(&keg);
        return Err(err.wrap_err(format!("failed to build {name} {pkg_version} from source")));
    }
    if !keg.is_dir() {
        bail!(
            "build of {name} finished but produced no keg at {}",
            keg.display()
        );
    }

    let host_tag = tag::host_tag();
    let receipt = pour::write_receipt(
        rf,
        &host_tag,
        &keg,
        &Default::default(),
        closure,
        /* poured_from_bottle */ false,
    );
    let linked = receipt.and_then(|()| pour::link_keg(name, &pkg_version, formula.keg_only));
    if let Err(err) = linked {
        if let Err(rm_err) = crate::file::remove_all(&keg) {
            warn!(
                "failed to remove {} after link failure: {rm_err}\n\
                 remove it manually, then re-run `mise bootstrap packages apply`",
                keg.display(),
            );
        }
        return Err(err);
    }
    crate::file::remove_all(&build_root)?;
    Ok(())
}

/// Ensure a mise-managed ruby is installed (precompiled by default) and
/// return the path to its `ruby` executable.
async fn ruby_bin() -> Result<PathBuf> {
    let mut config = Config::get().await?;
    let tool: crate::cli::args::ToolArg = "ruby".parse()?;
    let mut ts = ToolsetBuilder::new()
        .with_args(&[tool])
        .with_default_to_latest(true)
        .build(&config)
        .await?;
    ts.install_missing_versions(
        &mut config,
        &InstallOptions {
            // only ruby — never drag the rest of the config's toolset in
            missing_args_only: true,
            reason: "brew source build".to_string(),
            ..Default::default()
        },
    )
    .await?;
    for (backend, tv) in ts.list_current_versions() {
        if tv.ba().short != "ruby" {
            continue;
        }
        for bin_dir in backend.list_bin_paths(&config, &tv).await? {
            let ruby = bin_dir.join("ruby");
            if ruby.is_file() {
                return Ok(ruby);
            }
        }
    }
    bail!("failed to provision ruby for building from source (try `mise install ruby`)");
}

/// Download the formula's .rb from homebrew/core, pinned to the commit the
/// API metadata was generated from and verified against its sha256.
async fn fetch_formula_rb(rf: &ResolvedFormula, pr: &dyn SingleReport) -> Result<PathBuf> {
    let formula = &rf.formula;
    // all guaranteed present by check_buildable
    let rb_path = formula.ruby_source_path.as_ref().unwrap();
    let sha256 = formula
        .ruby_source_checksum
        .as_ref()
        .and_then(|c| c.sha256.as_deref())
        .unwrap();
    let commit = formula.tap_git_head.as_deref().unwrap();
    let cache_dir = crate::dirs::CACHE.join("system-brew").join("formula");
    let dest = cache_dir.join(format!("{}-{}.rb", formula.name, &sha256[..12]));
    if dest.exists() && crate::hash::ensure_checksum(&dest, sha256, None, "sha256").is_ok() {
        return Ok(dest);
    }
    let raw_base = rf
        .tap_raw_base
        .as_deref()
        .map(|base| base.trim_end_matches("/HEAD"))
        .unwrap_or(HOMEBREW_CORE_RAW);
    let url = format!("{raw_base}/{commit}/{rb_path}");
    pr.set_message(format!("download {rb_path}"));
    HTTP_FETCH.download_file(&url, &dest, Some(pr)).await?;
    crate::hash::ensure_checksum(&dest, sha256, Some(pr), "sha256")?;
    Ok(dest)
}

/// Download the stable source archive, verified against the API's sha256.
/// the source archive's upstream file name
fn source_basename(formula: &Formula) -> String {
    formula
        .stable_url()
        .map(|src| src.url.as_str())
        .and_then(|url| url.rsplit('/').next())
        .filter(|b| !b.is_empty())
        .unwrap_or("source")
        .to_string()
}

async fn fetch_source(formula: &Formula, pr: &dyn SingleReport) -> Result<PathBuf> {
    let src = formula.stable_url().unwrap(); // check_buildable
    let sha256 = src.checksum.as_deref().unwrap(); // check_buildable
    let basename = source_basename(formula);
    let cache_dir = crate::dirs::CACHE.join("system-brew").join("sources");
    let dest = cache_dir.join(format!("{}-{basename}", &sha256[..12]));
    if dest.exists() && crate::hash::ensure_checksum(&dest, sha256, None, "sha256").is_ok() {
        debug!("source cache hit: {}", dest.display());
        return Ok(dest);
    }
    pr.set_message(format!("download {basename}"));
    HTTP_FETCH.download_file(&src.url, &dest, Some(pr)).await?;
    crate::hash::ensure_checksum(&dest, sha256, Some(pr), "sha256")?;
    Ok(dest)
}

/// Unpack the source archive the way brew stages it: when the archive holds
/// a single top-level directory, that directory is the buildpath.
fn stage_source(archive: &Path, build_root: &Path, basename: &str) -> Result<PathBuf> {
    let stage = build_root.join("src");
    crate::file::create_dir_all(&stage)?;
    // `basename` is the upstream file name — the cache entry's own name
    // carries a checksum prefix that must not leak into the build tree
    let format = ExtractionFormat::from_file_name(basename);
    if format.is_archive() {
        crate::file::extract_archive(archive, &stage, format, &ExtractOptions::default())
            .wrap_err_with(|| format!("failed to extract {}", archive.display()))?;
    } else {
        // a bare file (script, single binary): stage it as-is
        crate::file::copy(archive, stage.join(basename))?;
    }
    let entries: Vec<PathBuf> = crate::file::ls(&stage)?.into_iter().collect();
    match entries.as_slice() {
        [single] if single.is_dir() => Ok(single.clone()),
        _ => Ok(stage),
    }
}

/// The environment the formula builds in: dependency kegs first on PATH,
/// pkg-config/include/lib flags pointing into the prefix, and the shim's
/// own variables. Mirrors the spirit of brew's superenv without the
/// compiler shims.
fn build_env(
    rf: &ResolvedFormula,
    closure: &[ResolvedFormula],
    pkg_version: &str,
    buildpath: &Path,
    formula_rb: &Path,
) -> HashMap<String, String> {
    let prefix = prefix::prefix();
    let opt = prefix.join("opt");
    // only this formula's transitive dependencies — unrelated formulae from
    // the same install batch must not leak into the build environment
    let by_name: HashMap<&str, &ResolvedFormula> = closure
        .iter()
        .flat_map(|other| {
            std::iter::once((other.formula.name.as_str(), other)).chain(
                other
                    .formula
                    .aliases
                    .iter()
                    .map(move |a| (a.as_str(), other)),
            )
        })
        .collect();
    // walk each formula's deps under the same variations tag the closure
    // resolution used (the dep's selected bottle tag, not the host's)
    let host_tag = tag::host_tag();
    let rf_tag = super::resolve::dep_tag(&rf.formula, &host_tag);
    let mut deps: Vec<&ResolvedFormula> = vec![];
    let mut seen: std::collections::HashSet<&str> =
        std::iter::once(rf.formula.name.as_str()).collect();
    let mut queue: Vec<&String> = rf
        .formula
        .dependencies_for(&rf_tag)
        .iter()
        .chain(rf.formula.build_dependencies_for(&rf_tag))
        .collect();
    while let Some(dep) = queue.pop() {
        let Some(other) = by_name.get(dep.as_str()) else {
            continue;
        };
        if !seen.insert(other.formula.name.as_str()) {
            continue;
        }
        deps.push(other);
        let other_tag = super::resolve::dep_tag(&other.formula, &host_tag);
        queue.extend(other.formula.dependencies_for(&other_tag));
    }
    let dep_opts: Vec<PathBuf> = deps
        .iter()
        .map(|other| opt.join(&other.formula.name))
        .filter(|p| p.is_dir())
        .collect();

    let mut path: Vec<String> = dep_opts
        .iter()
        .map(|p| p.join("bin"))
        .filter(|p| p.is_dir())
        .map(|p| p.display().to_string())
        .collect();
    path.push(prefix.join("bin").display().to_string());
    for dir in ["/usr/local/bin", "/usr/bin", "/bin", "/usr/sbin", "/sbin"] {
        path.push(dir.to_string());
    }

    let pkg_config_path: Vec<String> = dep_opts
        .iter()
        .flat_map(|p| [p.join("lib/pkgconfig"), p.join("share/pkgconfig")])
        .chain([prefix.join("lib/pkgconfig"), prefix.join("share/pkgconfig")])
        .filter(|p| p.is_dir())
        .map(|p| p.display().to_string())
        .collect();

    let mut cppflags: Vec<String> = vec![];
    let mut ldflags: Vec<String> = vec![];
    for dir in dep_opts.iter().chain([&prefix]) {
        let include = dir.join("include");
        if include.is_dir() {
            cppflags.push(format!("-I{}", include.display()));
        }
        let lib = dir.join("lib");
        if lib.is_dir() {
            ldflags.push(format!("-L{}", lib.display()));
        }
    }
    if cfg!(target_os = "linux") {
        // binaries must find brewed libraries at runtime without ldconfig
        ldflags.push(format!("-Wl,-rpath,{}", prefix.join("lib").display()));
    }

    let jobs = Settings::get().jobs.max(1);
    let stable_version = rf.formula.versions.stable.clone().unwrap_or_default();
    let mut env = HashMap::from(
        [
            ("MISE_BREW_PREFIX", prefix.display().to_string()),
            ("MISE_BREW_CELLAR", prefix::cellar().display().to_string()),
            ("MISE_BREW_FORMULA_FILE", formula_rb.display().to_string()),
            ("MISE_BREW_NAME", rf.formula.name.clone()),
            ("MISE_BREW_VERSION", stable_version),
            ("MISE_BREW_PKG_VERSION", pkg_version.to_string()),
            ("MISE_BREW_BUILDPATH", buildpath.display().to_string()),
            (
                "MISE_BREW_CACHE",
                crate::dirs::CACHE
                    .join("system-brew")
                    .join("downloads")
                    .display()
                    .to_string(),
            ),
            ("MISE_BREW_MAKE_JOBS", jobs.to_string()),
            ("PATH", path.join(":")),
            ("MAKEFLAGS", format!("-j{jobs}")),
            ("HOMEBREW_PREFIX", prefix.display().to_string()),
            ("HOMEBREW_CELLAR", prefix::cellar().display().to_string()),
            (
                "CMAKE_PREFIX_PATH",
                std::iter::once(prefix.clone())
                    .chain(dep_opts.iter().cloned())
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join(":"),
            ),
        ]
        .map(|(k, v)| (k.to_string(), v)),
    );
    if !pkg_config_path.is_empty() {
        env.insert("PKG_CONFIG_PATH".into(), pkg_config_path.join(":"));
    }
    if !cppflags.is_empty() {
        env.insert("CPPFLAGS".into(), cppflags.join(" "));
        env.insert("CFLAGS".into(), cppflags.join(" "));
        env.insert("CXXFLAGS".into(), cppflags.join(" "));
    }
    if !ldflags.is_empty() {
        env.insert("LDFLAGS".into(), ldflags.join(" "));
    }
    env
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::super::api::{BottleFile, BottleSpec, RubySourceChecksum, SourceUrl, Versions};
    use super::*;

    fn formula(tags: &[&str]) -> Formula {
        let files: HashMap<String, BottleFile> = tags
            .iter()
            .map(|tag| {
                (
                    tag.to_string(),
                    BottleFile {
                        cellar: ":any".to_string(),
                        url: "https://example.com/bottle.tar.gz".to_string(),
                        sha256: "0".repeat(64),
                    },
                )
            })
            .collect();
        let mut bottle = HashMap::new();
        if !tags.is_empty() {
            bottle.insert("stable".to_string(), BottleSpec { files });
        }
        Formula {
            name: "test".to_string(),
            tap: None,
            aliases: vec![],
            versions: Versions {
                stable: Some("1.0.0".to_string()),
            },
            revision: 0,
            keg_only: false,
            dependencies: vec![],
            build_dependencies: vec![],
            bottle,
            variations: HashMap::new(),
            urls: HashMap::from([(
                "stable".to_string(),
                SourceUrl {
                    url: "https://example.com/test-1.0.0.tar.gz".to_string(),
                    checksum: Some("0".repeat(64)),
                    using: None,
                },
            )]),
            ruby_source_path: Some("Formula/t/test.rb".to_string()),
            ruby_source_checksum: Some(RubySourceChecksum {
                sha256: Some("1".repeat(64)),
            }),
            tap_git_head: Some("abc123".to_string()),
        }
    }

    #[test]
    fn test_has_bottle() {
        // the version-independent "all" tag matches every machine
        assert!(has_bottle(&formula(&["all"])));
        assert!(!has_bottle(&formula(&[])));
    }

    #[test]
    fn test_missing_bottle_reason() {
        assert_eq!(
            missing_bottle_reason(&formula(&[])),
            "source-only formula, no bottles"
        );
        assert_eq!(
            missing_bottle_reason(&formula(&["x86_64_linux", "arm64_sonoma"])),
            "bottles exist only for: arm64_sonoma, x86_64_linux"
        );
    }

    #[test]
    fn test_check_buildable() {
        assert!(check_buildable(&formula(&[])).is_ok());

        let mut git_source = formula(&[]);
        git_source.urls.get_mut("stable").unwrap().using = Some("git".to_string());
        assert!(check_buildable(&git_source).is_err());

        let mut no_checksum = formula(&[]);
        no_checksum.urls.get_mut("stable").unwrap().checksum = None;
        assert!(check_buildable(&no_checksum).is_err());

        let mut no_url = formula(&[]);
        no_url.urls.clear();
        assert!(check_buildable(&no_url).is_err());
    }
}
