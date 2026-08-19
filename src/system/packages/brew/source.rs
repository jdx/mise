//! Native Linux source builds for formulae without a usable bottle.
//!
//! Building a formula means running its Ruby `install` method. mise does
//! this without Homebrew: it provisions a mise-managed ruby (precompiled,
//! via the normal tool machinery), downloads the formula's .rb from
//! homebrew/core (sha256-verified against the API metadata), stages the
//! sha256-verified source archive, and evaluates the formula with the
//! Formula-DSL shim in shim.rb. Build dependencies are poured as bottles
//! beforehand by the regular closure machinery (see resolve.rs), so the
//! build environment points at real kegs in the canonical prefix.
//!
//! macOS remains bottle-only. `sandbox-exec` cannot contain a descendant that
//! double-forks or creates a new session, so a detached formula process could
//! retain a writable keg file descriptor after the Ruby leader exits. Source
//! builds therefore fail before downloads, staging, or Cellar mutation there.

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
use crate::http::{HTTP, HTTP_FETCH};
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
    validate_source_build_platform(&formula.name)?;
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
    let Some(source_checksum) = src.checksum.as_deref() else {
        bail!("{}: source archive has no sha256 in the API", formula.name);
    };
    if !valid_sha256(source_checksum) {
        bail!("{}: source archive has an invalid sha256", formula.name);
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
    let Some(formula_checksum) = formula
        .ruby_source_checksum
        .as_ref()
        .and_then(|c| c.sha256.as_deref())
    else {
        bail!("{}: API metadata has no formula checksum", formula.name);
    };
    if !valid_sha256(formula_checksum) {
        bail!(
            "{}: API metadata has an invalid formula checksum",
            formula.name
        );
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn validate_source_build_platform(name: &str) -> Result<()> {
    bail!(
        "brew:{name}: source builds are unsupported on macOS because sandbox-exec cannot contain detached descendants; install a compatible bottle"
    )
}

#[cfg(not(target_os = "macos"))]
fn validate_source_build_platform(_name: &str) -> Result<()> {
    Ok(())
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Build a formula from source into its keg and link it.
pub async fn build(
    rf: &ResolvedFormula,
    closure: &[ResolvedFormula],
    lifecycle: &super::lifecycle::PreparedFormulaLifecycle,
    pr: &dyn SingleReport,
) -> Result<()> {
    let formula = &rf.formula;
    let name = &formula.name;
    pour::validate_formula_install_policy(formula)?;
    let pkg_version = formula.pkg_version()?;
    check_buildable(formula)?;
    let keg = pour::keg_path(name, &pkg_version);
    pour::prepare_formula_rack(&keg)?;
    if pour::complete_interrupted_finalization(&keg)? {
        return Ok(());
    }
    if pour::resume_source_finalization(&keg, formula.keg_only, lifecycle, pr).await? {
        return Ok(());
    }
    pr.set_message("resolve ruby".to_string());
    let ruby = ruby_bin().await?;
    let formula_rb = fetch_formula_rb(rf, pr).await?;
    let archive = fetch_source(formula, pr).await?;

    let build_root = crate::dirs::CACHE
        .join("system-brew")
        .join("build")
        .join(format!(
            "{name}-{pkg_version}-{}",
            crate::rand::random_string(32)
        ));
    match build_root.symlink_metadata() {
        Ok(_) => bail!(
            "source-build staging path already exists: {}",
            build_root.display()
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    crate::file::create_dir_all(&build_root)?;
    let _build_root_cleanup = OwnedBuildRoot::new(&build_root)?;
    pr.set_message("extract source".to_string());
    let buildpath = stage_source(&archive, &build_root, &source_basename(formula))?;
    let shim_path = build_root.join("mise-brew-shim.rb");
    crate::file::write(&shim_path, SHIM_RB)?;
    let sandbox_home = build_root.join("home");
    let sandbox_tmp = build_root.join("tmp");
    crate::file::create_dir_all(&sandbox_home)?;
    crate::file::create_dir_all(&sandbox_tmp)?;

    let mut env = build_env(rf, closure, &pkg_version, &buildpath, &formula_rb);
    env.insert("HOME".to_string(), sandbox_home.display().to_string());
    env.insert("TMPDIR".to_string(), sandbox_tmp.display().to_string());
    env.insert("TMP".to_string(), sandbox_tmp.display().to_string());
    env.insert("TEMP".to_string(), sandbox_tmp.display().to_string());
    let inspection_sandbox = source_sandbox_config(
        &ruby,
        &formula_rb,
        &build_root,
        &sandbox_home,
        &sandbox_tmp,
        &env,
        None,
    )?;
    let mut inspection_env = env.clone();
    inspection_env.insert("MISE_BREW_INSPECT_ONLY".to_string(), "1".to_string());
    let mut inspection = CmdLineRunner::new(&ruby)
        .arg(&shim_path)
        .current_dir(&buildpath)
        .env_clear()
        .envs(inspection_env)
        .with_pr(pr)
        .with_sandbox(inspection_sandbox)
        .with_process_group_cleanup();
    inspection
        .apply_sandbox()
        .await
        .wrap_err_with(|| format!("failed to confine source formula inspection for {name}"))?;
    inspection.execute_async().await.wrap_err_with(|| {
        format!("brew:{name}: formula uses unsupported or unsafe source-build declarations")
    })?;

    // Formulae bake the final keg path into binaries, so the build installs
    // straight into the Cellar. Authority is durable before Ruby can write it.
    let transaction = pour::begin_source_build_transaction(
        name,
        &pkg_version,
        &keg,
        pour::active_keg(name),
        super::lifecycle::prepared_identity_sha256(lifecycle)?,
    )?;
    let predecessor_keg = transaction.predecessor_keg;
    let existing_backup = transaction.existing_backup;

    let sandbox = source_sandbox_config(
        &ruby,
        &formula_rb,
        &build_root,
        &sandbox_home,
        &sandbox_tmp,
        &env,
        Some(&keg),
    )?;

    pr.set_message("build from source".to_string());
    let mut cmd = CmdLineRunner::new(&ruby)
        .arg(&shim_path)
        .current_dir(&buildpath)
        .env_clear()
        .envs(env)
        .with_pr(pr)
        .with_sandbox(sandbox)
        .with_process_group_cleanup();
    if let Err(error) = cmd.apply_sandbox().await {
        pour::rollback_source_build_transaction(&keg)?;
        return Err(error.wrap_err(format!("failed to confine source build for {name}")));
    }
    let built = cmd.execute_async().await;
    if let Err(err) = built {
        pour::rollback_source_build_transaction(&keg)?;
        return Err(err.wrap_err(format!("failed to build {name} {pkg_version} from source")));
    }
    pour::validate_source_build_transaction(&keg)?;
    match keg.symlink_metadata() {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
        Ok(_) => {
            pour::rollback_source_build_transaction(&keg)?;
            bail!(
                "build of {name} finished but produced no keg at {}",
                keg.display()
            );
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            pour::rollback_source_build_transaction(&keg)?;
            bail!(
                "build of {name} finished but produced no keg at {}",
                keg.display()
            );
        }
        Err(error) => return Err(error.into()),
    }
    pour::prepare_source_build_metadata(&keg)?;

    let formula_snapshot = keg.join(".brew").join(format!("{name}.rb"));
    let provenance = (|| {
        pour::validate_source_build_transaction(&keg)?;
        super::lifecycle::atomic_copy(&formula_rb, &formula_snapshot)?;
        Ok(pour::FormulaInstallProvenance::SourceBuild {
            formula_snapshot,
            compiler: source_compiler()?,
            built_on: native_build_system_info()?,
        })
    })();
    let provenance = match provenance {
        Ok(provenance) => provenance,
        Err(error) => {
            pour::rollback_source_build_transaction(&keg)?;
            return Err(error);
        }
    };
    let host_tag = tag::host_tag();
    let report = Default::default();
    let finalized = pour::finalize_formula(pour::FormulaFinalizer {
        rf,
        tag: &host_tag,
        staged_keg: &keg,
        keg: &keg,
        report: &report,
        closure,
        provenance,
        lifecycle,
        pr,
        existing_backup,
        predecessor_keg,
    })
    .await;
    if finalized.is_ok() {
        crate::file::remove_all(&build_root)?;
    }
    finalized
}

fn source_sandbox_config(
    ruby: &Path,
    formula_rb: &Path,
    build_root: &Path,
    sandbox_home: &Path,
    sandbox_tmp: &Path,
    env: &HashMap<String, String>,
    writable_keg: Option<&Path>,
) -> Result<crate::sandbox::SandboxConfig> {
    let ruby_root = ruby
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| eyre::eyre!("source-build Ruby has no installation root"))?;
    let brew_prefix = prefix::prefix();
    let mut allow_read = vec![
        build_root.to_path_buf(),
        formula_rb.to_path_buf(),
        ruby_root.to_path_buf(),
    ];
    if let Some(paths) = env.get("CMAKE_PREFIX_PATH") {
        allow_read.extend(
            std::env::split_paths(paths)
                .filter(|path| path != &brew_prefix)
                .filter(|path| path.is_dir()),
        );
    }
    allow_read.extend(source_platform_read_paths()?);
    let mut allow_write = if writable_keg.is_some() {
        vec![build_root.to_path_buf()]
    } else {
        vec![sandbox_home.to_path_buf(), sandbox_tmp.to_path_buf()]
    };
    allow_write.extend(writable_keg.map(Path::to_path_buf));
    let mut sandbox = crate::sandbox::SandboxConfig {
        deny_read: true,
        deny_write: true,
        deny_net: true,
        deny_local_sockets: true,
        deny_env: true,
        allow_read,
        allow_write,
        deny_system_temp_write: true,
        deny_mise_data_read: true,
        ..Default::default()
    };
    sandbox.resolve_paths();
    Ok(sandbox)
}

#[cfg(target_os = "macos")]
fn source_platform_read_paths() -> Result<Vec<PathBuf>> {
    let output = std::process::Command::new("/usr/bin/xcode-select")
        .arg("-p")
        .output()
        .wrap_err("could not locate active Xcode developer directory")?;
    if !output.status.success() {
        bail!("could not locate active Xcode developer directory");
    }
    let path = PathBuf::from(String::from_utf8(output.stdout)?.trim());
    let metadata = path.symlink_metadata()?;
    if !path.is_absolute() || !metadata.is_dir() || metadata.file_type().is_symlink() {
        bail!("active Xcode developer directory is not a real absolute directory");
    }
    Ok(vec![path.canonicalize()?])
}

#[cfg(not(target_os = "macos"))]
fn source_platform_read_paths() -> Result<Vec<PathBuf>> {
    Ok(vec![])
}

struct OwnedBuildRoot {
    path: PathBuf,
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
}

impl OwnedBuildRoot {
    fn new(path: &Path) -> Result<Self> {
        let metadata = path.symlink_metadata()?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            bail!("source-build staging root is not a real directory");
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            Ok(Self {
                path: path.to_path_buf(),
                device: metadata.dev(),
                inode: metadata.ino(),
            })
        }
        #[cfg(not(unix))]
        {
            Ok(Self {
                path: path.to_path_buf(),
            })
        }
    }

    #[cfg(unix)]
    fn still_owned(&self) -> bool {
        use std::os::unix::fs::MetadataExt;
        self.path.symlink_metadata().is_ok_and(|metadata| {
            metadata.is_dir()
                && !metadata.file_type().is_symlink()
                && metadata.dev() == self.device
                && metadata.ino() == self.inode
        })
    }

    #[cfg(not(unix))]
    fn still_owned(&self) -> bool {
        false
    }
}

impl Drop for OwnedBuildRoot {
    fn drop(&mut self) {
        if self.still_owned() {
            let _ = crate::file::remove_all(&self.path);
        }
    }
}

fn source_compiler() -> Result<String> {
    let output = std::process::Command::new("cc").arg("--version").output()?;
    if !output.status.success() {
        bail!("cannot determine source-build compiler")
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let version = command_output("cc", &["-dumpfullversion", "-dumpversion"]);
    parse_source_compiler(&text, version.as_deref())
}

fn parse_source_compiler(version_output: &str, dumped_version: Option<&str>) -> Result<String> {
    let text = version_output.to_lowercase();
    if text.contains("clang") {
        return Ok("clang".to_string());
    }
    if text.contains("gcc")
        || text.contains("free software foundation")
        || text.contains("gnu compiler collection")
    {
        let major = dumped_version
            .and_then(|version| version.split('.').next())
            .filter(|major| !major.is_empty() && major.chars().all(|c| c.is_ascii_digit()))
            .ok_or_else(|| eyre::eyre!("cannot determine source-build GCC major version"))?;
        return Ok(format!("gcc-{major}"));
    }
    bail!("unrecognized source-build compiler")
}

fn native_build_system_info() -> Result<serde_json::Value> {
    let os = if cfg!(target_os = "macos") {
        "macOS"
    } else {
        "Linux"
    };
    let os_version = if cfg!(target_os = "macos") {
        command_output("/usr/bin/sw_vers", &["-productVersion"])
    } else {
        std::fs::read_to_string("/etc/os-release")
            .ok()
            .and_then(|contents| {
                contents.lines().find_map(|line| {
                    line.strip_prefix("PRETTY_NAME=")
                        .map(|value| value.trim_matches('"').to_string())
                })
            })
    }
    .ok_or_else(|| eyre::eyre!("cannot determine source-build operating system version"))?;
    let cpu_family = command_output("uname", &["-m"])
        .ok_or_else(|| eyre::eyre!("cannot determine source-build CPU family"))?;
    Ok(serde_json::json!({
        "os": os,
        "os_version": os_version,
        "cpu_family": cpu_family,
    }))
}

fn command_output(program: &str, args: &[&str]) -> Option<String> {
    let output = std::process::Command::new(program)
        .args(args)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|value| !value.is_empty())
}

/// Ensure a mise-managed ruby is installed (precompiled by default) and
/// return the path to its `ruby` executable.
pub(crate) async fn ruby_bin() -> Result<PathBuf> {
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
    HTTP.download_file(&src.url, &dest, Some(pr)).await?;
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
        [single] => {
            let metadata = single.symlink_metadata()?;
            if metadata.file_type().is_symlink() {
                bail!(
                    "source archive top-level entry is a symlink: {}",
                    single.display()
                );
            }
            if metadata.is_dir() {
                let canonical_stage = stage.canonicalize()?;
                let canonical_single = single.canonicalize()?;
                if !canonical_single.starts_with(&canonical_stage) {
                    bail!("source archive escaped staging root: {}", single.display());
                }
                Ok(single.clone())
            } else {
                Ok(stage)
            }
        }
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
        let Some(other) = by_name.get(super::resolve::formula_reference_name(dep)) else {
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
    for dir in ["/usr/local/bin", "/usr/bin", "/bin", "/usr/sbin", "/sbin"] {
        path.push(dir.to_string());
    }

    let pkg_config_path: Vec<String> = dep_opts
        .iter()
        .flat_map(|p| [p.join("lib/pkgconfig"), p.join("share/pkgconfig")])
        .filter(|p| p.is_dir())
        .map(|p| p.display().to_string())
        .collect();

    let mut cppflags: Vec<String> = vec![];
    let mut ldflags: Vec<String> = vec![];
    for dir in &dep_opts {
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
        ldflags.extend(
            dep_opts
                .iter()
                .map(|dependency| format!("-Wl,-rpath,{}", dependency.join("lib").display())),
        );
    }

    let jobs = crate::jobs::normalize(Settings::get().jobs);
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
                dep_opts
                    .iter()
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
    use std::process::Output;

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
            post_install_steps: vec![],
            post_install_defined: false,
            install_policy: Default::default(),
        }
    }

    fn run_shim_formula(
        source: &str,
        inspect_only: bool,
    ) -> Result<Option<(tempfile::TempDir, Output, PathBuf)>> {
        let mut ruby_candidates = crate::file::ls(&crate::dirs::INSTALLS.join("ruby"))
            .unwrap_or_default()
            .into_iter()
            .map(|install| install.join("bin/ruby"))
            .collect::<Vec<_>>();
        ruby_candidates.extend(crate::file::which("ruby"));
        let Some(ruby) = ruby_candidates.into_iter().find(|ruby| {
            std::process::Command::new(ruby)
                .args([
                    "--disable-gems",
                    "-e",
                    "major, minor = RUBY_VERSION.split('.').map(&:to_i); exit((major > 3 || (major == 3 && minor >= 1)) ? 0 : 1)",
                ])
                .status()
                .is_ok_and(|status| status.success())
        }) else {
            return Ok(None);
        };
        let tmp = tempfile::tempdir()?;
        let prefix = tmp.path().join("prefix");
        let build = tmp.path().join("build");
        let cache = tmp.path().join("cache");
        crate::file::create_dir_all(&build)?;
        crate::file::create_dir_all(&cache)?;
        let shim = tmp.path().join("shim.rb");
        let formula = tmp.path().join("test.rb");
        crate::file::write(&shim, SHIM_RB)?;
        crate::file::write(&formula, source)?;
        let mut command = std::process::Command::new(ruby);
        command
            .arg(&shim)
            .env_clear()
            .env("PATH", "/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin")
            .env("MISE_BREW_PREFIX", &prefix)
            .env("MISE_BREW_CELLAR", prefix.join("Cellar"))
            .env("MISE_BREW_FORMULA_FILE", &formula)
            .env("MISE_BREW_NAME", "test")
            .env("MISE_BREW_VERSION", "1.0")
            .env("MISE_BREW_PKG_VERSION", "1.0")
            .env("MISE_BREW_BUILDPATH", &build)
            .env("MISE_BREW_CACHE", &cache)
            .env("MISE_BREW_MAKE_JOBS", "2");
        if inspect_only {
            command.env("MISE_BREW_INSPECT_ONLY", "1");
        }
        let output = command.output()?;
        let keg = prefix.join("Cellar/test/1.0");
        Ok(Some((tmp, output, keg)))
    }

    fn shim_failure_text(output: &Output) -> String {
        format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
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
        let buildable = formula(&[]);
        if cfg!(target_os = "macos") {
            let error = check_buildable(&buildable).unwrap_err().to_string();
            assert!(error.contains("source builds are unsupported on macOS"));
            assert!(error.contains("install a compatible bottle"));
        } else {
            assert!(check_buildable(&buildable).is_ok());
        }

        let mut git_source = formula(&[]);
        git_source.urls.get_mut("stable").unwrap().using = Some("git".to_string());
        assert!(check_buildable(&git_source).is_err());

        let mut no_checksum = formula(&[]);
        no_checksum.urls.get_mut("stable").unwrap().checksum = None;
        assert!(check_buildable(&no_checksum).is_err());

        let mut no_url = formula(&[]);
        no_url.urls.clear();
        assert!(check_buildable(&no_url).is_err());

        let mut short_source_checksum = formula(&[]);
        short_source_checksum
            .urls
            .get_mut("stable")
            .unwrap()
            .checksum = Some("abc".to_string());
        assert!(check_buildable(&short_source_checksum).is_err());

        let mut non_ascii_formula_checksum = formula(&[]);
        non_ascii_formula_checksum
            .ruby_source_checksum
            .as_mut()
            .unwrap()
            .sha256 = Some("é".repeat(32));
        assert!(check_buildable(&non_ascii_formula_checksum).is_err());
    }

    #[test]
    fn source_platform_gate_does_not_disable_bottles() {
        let bottled = formula(&["all"]);
        assert!(has_bottle(&bottled));
        if cfg!(target_os = "macos") {
            assert!(check_buildable(&bottled).is_err());
        } else {
            assert!(check_buildable(&bottled).is_ok());
        }
    }

    #[test]
    fn source_shim_stages_shared_defaults_and_defers_post_install() {
        assert!(SHIM_RB.contains("def etc = prefix + \".bottle/etc\""));
        assert!(SHIM_RB.contains("def var = prefix + \".bottle/var\""));
        assert!(!SHIM_RB.contains("formula.post_install"));
    }

    #[test]
    fn source_compiler_matches_homebrew_receipt_names() {
        assert_eq!(
            parse_source_compiler(
                "cc (Ubuntu 13.3.0-6ubuntu2~24.04) 13.3.0\nCopyright (C) Free Software Foundation, Inc.",
                Some("13.3.0")
            )
            .unwrap(),
            "gcc-13"
        );
        assert_eq!(
            parse_source_compiler("Apple clang version 21.0.0", Some("21.0.0")).unwrap(),
            "clang"
        );
        assert!(parse_source_compiler("Tiny C Compiler", Some("0.9.27")).is_err());
        assert!(parse_source_compiler("gcc", Some("unknown")).is_err());
    }

    #[test]
    fn source_shim_preserves_exact_inreplace_and_append_semantics() -> Result<()> {
        let Some((_tmp, output, keg)) = run_shim_formula(
            r#"
class Test < Formula
  def install
    value = buildpath + "value"
    value.write("x x")
    inreplace(value, "x", "y", global: false)
    (prefix + "result").write(value.read)
  end
end
"#,
            false,
        )?
        else {
            return Ok(());
        };
        assert!(output.status.success(), "{}", shim_failure_text(&output));
        assert_eq!(crate::file::read_to_string(keg.join("result"))?, "y x");

        let Some((_tmp, output, _)) = run_shim_formula(
            r#"
class Test < Formula
  def install
    (buildpath + "missing").append_lines("value")
  end
end
"#,
            false,
        )?
        else {
            return Ok(());
        };
        assert!(!output.status.success());
        assert!(shim_failure_text(&output).contains("Cannot append file that doesn't exist"));
        Ok(())
    }

    #[test]
    fn source_shim_fails_closed_on_ambiguous_formula_behavior() -> Result<()> {
        let cases = [
            (
                r#"class Test < Formula
  mystery_install_policy true
end
"#,
                true,
                "unknown formula DSL",
            ),
            (
                r#"class Test < Formula
  def install
    ENV.mystery_build_environment
  end
end
"#,
                false,
                "exact Homebrew build-environment semantics are not implemented",
            ),
            (
                r#"class Test < Formula
  def install
    deps.each { |dep| puts dep }
  end
end
"#,
                false,
                "typed Dependency objects are not implemented",
            ),
            (
                r#"class Test < Formula
  def install
    Version.new("1.0-alpha") < Version.new("1.0")
  end
end
"#,
                false,
                "opaque version comparison",
            ),
            (
                r#"class Test < Formula
  MacOS::Xcode.installed?
end
"#,
                true,
                "exact Xcode detection is not implemented",
            ),
            (
                r#"class Test < Formula
  disable! because: :unmaintained
end
"#,
                true,
                "disabled formula policy",
            ),
        ];
        for (source, inspect_only, expected) in cases {
            let Some((_tmp, output, _)) = run_shim_formula(source, inspect_only)? else {
                return Ok(());
            };
            assert!(
                !output.status.success(),
                "case unexpectedly succeeded: {source}"
            );
            assert!(
                shim_failure_text(&output).contains(expected),
                "missing {expected:?}: {}",
                shim_failure_text(&output)
            );
        }
        Ok(())
    }

    #[test]
    fn owned_build_root_is_cleaned_after_failure_scope() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let build_root = tmp.path().join("build");
        crate::file::create_dir_all(&build_root)?;
        {
            let _owned = OwnedBuildRoot::new(&build_root)?;
            crate::file::write(build_root.join("partial"), "partial")?;
        }
        assert!(build_root.symlink_metadata().is_err());
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn owned_build_root_never_removes_replacement_directory() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let build_root = tmp.path().join("build");
        crate::file::create_dir_all(&build_root)?;
        let owned = OwnedBuildRoot::new(&build_root)?;
        crate::file::rename(&build_root, tmp.path().join("old-build"))?;
        crate::file::create_dir_all(&build_root)?;
        crate::file::write(build_root.join("foreign"), "foreign")?;
        drop(owned);
        assert_eq!(
            crate::file::read_to_string(build_root.join("foreign"))?,
            "foreign"
        );
        Ok(())
    }
}
