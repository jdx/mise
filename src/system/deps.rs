//! Plugin-declared system dependencies with capability detection.
//!
//! Source-compiling plugins (php, erlang, postgres, ...) need system
//! prerequisites — build tools, libraries, headers. Historically these lived
//! as README prose, so users only discovered them when a build failed halfway
//! through. A plugin can instead declare them as structured *capability
//! checks* (see [`SystemDepCheck`]) plus per-package-manager remediation hints.
//!
//! Detection is the source of truth: a check that passes is satisfied,
//! full stop — mise never asks *how* the capability got there, so nix,
//! MacPorts, and from-source installs all pass without ceremony. The package
//! hints are consulted only to *offer* installing the missing subset via the
//! existing [`crate::system::packages`] engine.

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};

use indexmap::IndexMap;
use once_cell::sync::Lazy;
use versions::Versioning;

use crate::config::Settings;
use crate::system::packages::{self, PackageRequest, SystemPackageManager};

/// A single capability a plugin requires before it can install.
#[derive(Debug, Clone)]
pub struct SystemDep {
    pub check: SystemDepCheck,
    /// Optional version constraint (only meaningful for `Bin`/`PkgConfig`).
    pub version: Option<VersionConstraint>,
    /// If set, this dependency is optional and this string is the reason it
    /// might be wanted (e.g. "observer GUI"). Missing optional deps never
    /// prompt or fail — they surface as a single informational line.
    pub optional: Option<String>,
    /// manager name -> package name, used only for remediation hints.
    pub packages: IndexMap<String, String>,
}

/// How to detect whether a [`SystemDep`] is satisfied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SystemDepCheck {
    /// executable resolvable on `PATH`
    Bin(String),
    /// `pkg-config --exists <name>` (a `.pc` module)
    PkgConfig(String),
    /// dynamic linker can resolve a soname, e.g. `libaio.so.1` (Linux only)
    SharedLib(String),
    /// escape hatch: a shell command whose exit status 0 means satisfied
    Command(String),
}

impl SystemDepCheck {
    fn kind(&self) -> &'static str {
        match self {
            SystemDepCheck::Bin(_) => "bin",
            SystemDepCheck::PkgConfig(_) => "pkgconfig",
            SystemDepCheck::SharedLib(_) => "sharedlib",
            SystemDepCheck::Command(_) => "command",
        }
    }

    fn value(&self) -> &str {
        match self {
            SystemDepCheck::Bin(s)
            | SystemDepCheck::PkgConfig(s)
            | SystemDepCheck::SharedLib(s)
            | SystemDepCheck::Command(s) => s,
        }
    }
}

/// A version comparison operator for [`VersionConstraint`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionOp {
    AtLeast,
    Greater,
    AtMost,
    Less,
    Exact,
}

impl VersionOp {
    fn symbol(self) -> &'static str {
        match self {
            VersionOp::AtLeast => ">=",
            VersionOp::Greater => ">",
            VersionOp::AtMost => "<=",
            VersionOp::Less => "<",
            VersionOp::Exact => "=",
        }
    }
}

/// A parsed version constraint like `>=3.0`. A bare version (`3.0`) means
/// `>=3.0` — the common "at least this" case.
///
/// Note: this compares versions of *system binaries/libraries* declared by a
/// plugin author against what is installed on the machine. It is NOT used to
/// resolve or order mise tool versions (see the semver rules in CLAUDE.md);
/// it is the same class of comparison as [`crate::config::config_file::min_version`].
#[derive(Debug, Clone)]
pub struct VersionConstraint {
    pub op: VersionOp,
    pub version: Versioning,
}

impl VersionConstraint {
    pub fn parse(s: &str) -> eyre::Result<Self> {
        let s = s.trim();
        let (op, rest) = if let Some(r) = s.strip_prefix(">=") {
            (VersionOp::AtLeast, r)
        } else if let Some(r) = s.strip_prefix("<=") {
            (VersionOp::AtMost, r)
        } else if let Some(r) = s.strip_prefix('>') {
            (VersionOp::Greater, r)
        } else if let Some(r) = s.strip_prefix('<') {
            (VersionOp::Less, r)
        } else if let Some(r) = s.strip_prefix('=') {
            (VersionOp::Exact, r)
        } else {
            (VersionOp::AtLeast, s)
        };
        let version = Versioning::new(rest.trim())
            .ok_or_else(|| eyre::eyre!("invalid version constraint '{s}'"))?;
        Ok(Self { op, version })
    }

    pub fn satisfied_by(&self, current: &Versioning) -> bool {
        match self.op {
            VersionOp::AtLeast => current >= &self.version,
            VersionOp::Greater => current > &self.version,
            VersionOp::AtMost => current <= &self.version,
            VersionOp::Less => current < &self.version,
            VersionOp::Exact => current == &self.version,
        }
    }
}

impl fmt::Display for VersionConstraint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.op.symbol(), self.version)
    }
}

impl SystemDep {
    /// Human-readable capability label, e.g. `bison >=3.0`, `pkg-config libxml-2.0`.
    pub fn label(&self) -> String {
        let base = match &self.check {
            SystemDepCheck::Bin(name) => name.clone(),
            SystemDepCheck::PkgConfig(name) => format!("pkg-config {name}"),
            SystemDepCheck::SharedLib(name) => format!("shared library {name}"),
            SystemDepCheck::Command(cmd) => format!("`{cmd}`"),
        };
        match &self.version {
            Some(v) => format!("{base} {v}"),
            None => base,
        }
    }

    /// Stable fingerprint used to memoize detection across tools that declare
    /// the same dependency (php + postgres both needing bison probe once).
    fn fingerprint(&self) -> String {
        match &self.version {
            Some(v) => format!("{}:{}:{v}", self.check.kind(), self.check.value()),
            None => format!("{}:{}", self.check.kind(), self.check.value()),
        }
    }
}

impl fmt::Display for SystemDep {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.label())
    }
}

/// The result of probing one [`SystemDep`] on the host.
#[derive(Debug, Clone)]
pub struct DepStatus {
    pub dep: SystemDep,
    /// detected version, if a version was extracted
    pub found: Option<String>,
    pub satisfied: bool,
    /// why it is unsatisfied (or a note when satisfied)
    pub reason: Option<String>,
}

impl TryFrom<vfox::SystemDependency> for SystemDep {
    type Error = eyre::Error;

    fn try_from(d: vfox::SystemDependency) -> eyre::Result<Self> {
        let check = match (&d.bin, &d.pkgconfig, &d.sharedlib, &d.command) {
            (Some(b), None, None, None) => SystemDepCheck::Bin(b.clone()),
            (None, Some(p), None, None) => SystemDepCheck::PkgConfig(p.clone()),
            (None, None, Some(s), None) => SystemDepCheck::SharedLib(s.clone()),
            (None, None, None, Some(c)) => SystemDepCheck::Command(c.clone()),
            _ => eyre::bail!(
                "systemDependencies entry must set exactly one of bin/pkgconfig/sharedlib/command"
            ),
        };
        // A version constraint is only meaningful for bin/pkgconfig (we probe
        // `--version` / `--modversion`). If declared on sharedlib/command,
        // keep the check but drop the version with a warning rather than
        // silently honoring a constraint that is never enforced.
        let version = match (&check, &d.version) {
            (_, None) => None,
            (SystemDepCheck::Bin(_) | SystemDepCheck::PkgConfig(_), Some(v)) => {
                Some(VersionConstraint::parse(v)?)
            }
            (check, Some(_)) => {
                warn!(
                    "systemDependencies: `version` is only supported for bin/pkgconfig checks, ignoring it for the {} check",
                    check.kind()
                );
                None
            }
        };
        Ok(SystemDep {
            check,
            version,
            optional: d.optional.clone(),
            packages: d.packages.into_iter().collect(),
        })
    }
}

/// Host-only detection result: `(satisfied, found_version, reason)`.
type DetectOutcome = (bool, Option<String>, Option<String>);

static CACHE: Lazy<Mutex<HashMap<String, DetectOutcome>>> = Lazy::new(|| Mutex::new(HashMap::new()));

/// Detect all `deps`, memoized. Concurrency-safe; two calls with the same
/// fingerprint reuse the first result.
pub async fn detect(deps: &[SystemDep]) -> Vec<DepStatus> {
    detect_inner(deps, true).await
}

/// Detect all `deps`, bypassing (and refreshing) the memo cache. Used to
/// re-verify after remediation, since a package we just installed changes the
/// answer.
pub async fn detect_fresh(deps: &[SystemDep]) -> Vec<DepStatus> {
    detect_inner(deps, false).await
}

async fn detect_inner(deps: &[SystemDep], use_cache: bool) -> Vec<DepStatus> {
    let mut out = Vec::with_capacity(deps.len());
    for dep in deps {
        // The cache keys on the check + version only (the fingerprint), so it
        // must store just the detection *outcome* — not the whole DepStatus.
        // The `dep` (its `optional`/`packages`) belongs to the current caller;
        // reusing the first caller's dep would misclassify a required dep as
        // optional when two tools share the same check.
        let fp = dep.fingerprint();
        let (satisfied, found, reason) = if use_cache
            && let Some(cached) = CACHE.lock().unwrap().get(&fp).cloned()
        {
            cached
        } else {
            let outcome = detect_outcome(dep).await;
            CACHE.lock().unwrap().insert(fp, outcome.clone());
            outcome
        };
        out.push(DepStatus {
            dep: dep.clone(),
            found,
            satisfied,
            reason,
        });
    }
    out
}

/// Probe a single dep, returning `(satisfied, found_version, reason)` — the
/// part of the result that depends only on the host, not on which tool
/// declared the dep.
async fn detect_outcome(dep: &SystemDep) -> DetectOutcome {
    match &dep.check {
        SystemDepCheck::Bin(name) => check_bin(name, dep.version.as_ref()).await,
        SystemDepCheck::PkgConfig(name) => check_pkgconfig(name, dep.version.as_ref()).await,
        SystemDepCheck::SharedLib(name) => check_sharedlib(name).await,
        SystemDepCheck::Command(cmd) => check_command(cmd).await,
    }
}

async fn check_bin(
    name: &str,
    constraint: Option<&VersionConstraint>,
) -> (bool, Option<String>, Option<String>) {
    let Some(path) = crate::file::which(name) else {
        return (false, None, Some(format!("`{name}` not found on PATH")));
    };
    let Some(constraint) = constraint else {
        return (true, None, None);
    };
    match run_capture(&path.to_string_lossy(), &["--version"]).await {
        Some((_, output)) => match extract_version(&output) {
            Some(v) => {
                let versioning = Versioning::new(&v);
                match versioning {
                    Some(versioning) => {
                        let ok = constraint.satisfied_by(&versioning);
                        (ok, Some(v), None)
                    }
                    // unparseable — do not block; presence is enough
                    None => {
                        debug!(
                            "system dep: `{name}` version '{v}' unparseable, treating as satisfied"
                        );
                        (true, Some(v), None)
                    }
                }
            }
            None => {
                debug!(
                    "system dep: could not extract version from `{name} --version`, treating as satisfied"
                );
                (true, None, None)
            }
        },
        None => {
            debug!("system dep: `{name} --version` failed to run, treating as satisfied");
            (true, None, None)
        }
    }
}

async fn check_pkgconfig(
    name: &str,
    constraint: Option<&VersionConstraint>,
) -> (bool, Option<String>, Option<String>) {
    if crate::file::which("pkg-config").is_none() {
        return (
            false,
            None,
            Some("pkg-config is not installed (needed to detect this library)".to_string()),
        );
    }
    match run_capture("pkg-config", &["--exists", name]).await {
        Some((true, _)) => {}
        _ => {
            return (
                false,
                None,
                Some(format!("pkg-config module `{name}` not found")),
            );
        }
    }
    let Some(constraint) = constraint else {
        return (true, None, None);
    };
    match run_capture("pkg-config", &["--modversion", name]).await {
        Some((true, output)) => {
            let v = output.trim().to_string();
            match Versioning::new(&v) {
                Some(versioning) => (constraint.satisfied_by(&versioning), Some(v), None),
                None => {
                    debug!(
                        "system dep: pkg-config modversion '{v}' unparseable, treating as satisfied"
                    );
                    (true, Some(v), None)
                }
            }
        }
        _ => (true, None, None),
    }
}

#[cfg(target_os = "linux")]
async fn check_sharedlib(soname: &str) -> (bool, Option<String>, Option<String>) {
    // Primary: ldconfig's cache lists every soname the dynamic linker knows.
    // Each entry looks like `	libaio.so.1 (libc6,x86-64) => /path/libaio.so.1`,
    // so match the advertised soname as the whole first token — a substring
    // check would let `libfoo.so.10` satisfy a request for `libfoo.so.1`.
    for ldconfig in ["ldconfig", "/sbin/ldconfig"] {
        if let Some((true, output)) = run_capture(ldconfig, &["-p"]).await {
            if output
                .lines()
                .any(|l| l.split_whitespace().next() == Some(soname))
            {
                return (true, None, None);
            }
            // ldconfig ran and did not list it; still check LD_LIBRARY_PATH below
            break;
        }
    }
    // Fallback: honor LD_LIBRARY_PATH dirs the cache doesn't cover.
    if let Ok(paths) = std::env::var("LD_LIBRARY_PATH") {
        for dir in std::env::split_paths(&paths) {
            if dir.join(soname).exists() {
                return (true, None, None);
            }
        }
    }
    (
        false,
        None,
        Some(format!(
            "shared library `{soname}` not found by the dynamic linker"
        )),
    )
}

#[cfg(not(target_os = "linux"))]
async fn check_sharedlib(soname: &str) -> (bool, Option<String>, Option<String>) {
    debug!("system dep: sharedlib check for `{soname}` is Linux-only, treating as satisfied");
    (true, None, None)
}

async fn check_command(cmd: &str) -> (bool, Option<String>, Option<String>) {
    let (program, args): (&str, Vec<&str>) = if cfg!(windows) {
        ("cmd", vec!["/C", cmd])
    } else {
        ("sh", vec!["-c", cmd])
    };
    match run_capture(program, &args).await {
        Some((true, _)) => (true, None, None),
        _ => (
            false,
            None,
            Some(format!("command `{cmd}` did not succeed")),
        ),
    }
}

/// Run a short command, capturing combined stdout+stderr. Returns
/// `(success, output)` or `None` if it could not be spawned or timed out.
/// Silent and side-effect free — never elevates. A 5s wall-clock timeout keeps
/// a hanging binary (e.g. one that blocks on `--version`) from stalling the
/// whole install.
async fn run_capture(program: &str, args: &[&str]) -> Option<(bool, String)> {
    let fut = tokio::process::Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .output();
    let output = match tokio::time::timeout(std::time::Duration::from_secs(5), fut).await {
        Ok(Ok(output)) => output,
        Ok(Err(_)) => return None,
        Err(_) => {
            debug!("system dep: `{program}` timed out, treating check as inconclusive");
            return None;
        }
    };
    let mut combined = String::from_utf8_lossy(&output.stdout).into_owned();
    combined.push_str(&String::from_utf8_lossy(&output.stderr));
    Some((output.status.success(), combined))
}

/// Extract the first version-like token (`3`, `3.0`, `3.12.1`) from text such
/// as `bison (GNU Bison) 3.8.2`.
fn extract_version(text: &str) -> Option<String> {
    static RE: Lazy<regex::Regex> = Lazy::new(|| regex::Regex::new(r"\d+(?:\.\d+)+").unwrap());
    RE.find(text).map(|m| m.as_str().to_string())
}

/// Pick the first available, settings-enabled package manager that has a hint
/// for `dep`. Mirrors the manager selection [`crate::system`] applies so we
/// never propose a manager the driver would reject.
pub fn pick_manager(dep: &SystemDep) -> Option<Arc<dyn SystemPackageManager>> {
    let enabled = Settings::get().system_packages.managers.clone();
    for m in packages::all_managers() {
        let name = m.name();
        if enabled
            .as_ref()
            .is_some_and(|e| !e.iter().any(|x| x == name))
        {
            continue;
        }
        if !m.is_available() {
            continue;
        }
        if dep.packages.contains_key(name) {
            return Some(m);
        }
    }
    None
}

/// Group missing deps into per-manager [`PackageRequest`]s for remediation.
/// Returns `(by_manager, unremediable)` where `unremediable` are deps with no
/// available manager hint.
pub fn build_requests(
    missing: &[&SystemDep],
) -> (IndexMap<String, Vec<PackageRequest>>, Vec<SystemDep>) {
    let mut by_mgr: IndexMap<String, Vec<PackageRequest>> = IndexMap::new();
    let mut unremediable = vec![];
    for dep in missing {
        match pick_manager(dep) {
            Some(m) => {
                let pkg = dep.packages.get(m.name()).cloned().unwrap_or_default();
                let requests = by_mgr.entry(m.name().to_string()).or_default();
                if !requests.iter().any(|r| r.name == pkg) {
                    requests.push(PackageRequest {
                        name: pkg,
                        version: None,
                        tap_url: None,
                    });
                }
            }
            None => unremediable.push((*dep).clone()),
        }
    }
    (by_mgr, unremediable)
}

/// Copy-pasteable install hint commands for `missing`, grouped by the manager
/// that would satisfy each. Used by warn mode.
pub fn hint_commands(missing: &[&SystemDep]) -> Vec<String> {
    let (by_mgr, _) = build_requests(missing);
    by_mgr
        .into_iter()
        .map(|(mgr, requests)| {
            let pkgs = requests
                .iter()
                .map(|r| r.name.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            format!("{}{pkgs}", install_prefix(&mgr))
        })
        .collect()
}

fn install_prefix(mgr: &str) -> &'static str {
    match mgr {
        "brew" => "brew install ",
        "brew-cask" => "brew install --cask ",
        "apt" => "sudo apt-get install -y ",
        "dnf" => "sudo dnf install -y ",
        "pacman" => "sudo pacman -S ",
        "apk" => "sudo apk add ",
        "mas" => "mas install ",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_constraint_parse_and_compare() {
        let c = VersionConstraint::parse(">=3.0").unwrap();
        assert_eq!(c.op, VersionOp::AtLeast);
        assert!(!c.satisfied_by(&Versioning::new("2.3").unwrap()));
        assert!(c.satisfied_by(&Versioning::new("3.0").unwrap()));
        assert!(c.satisfied_by(&Versioning::new("3.8.2").unwrap()));

        // bare version means >=
        let bare = VersionConstraint::parse("3.0").unwrap();
        assert_eq!(bare.op, VersionOp::AtLeast);
        assert!(bare.satisfied_by(&Versioning::new("3.1").unwrap()));
        assert!(!bare.satisfied_by(&Versioning::new("2.9").unwrap()));

        assert!(
            VersionConstraint::parse(">2")
                .unwrap()
                .satisfied_by(&Versioning::new("3").unwrap())
        );
        assert!(
            !VersionConstraint::parse(">3")
                .unwrap()
                .satisfied_by(&Versioning::new("3").unwrap())
        );
        assert!(
            VersionConstraint::parse("<=1.2")
                .unwrap()
                .satisfied_by(&Versioning::new("1.2").unwrap())
        );
        assert!(
            VersionConstraint::parse("=3.0")
                .unwrap()
                .satisfied_by(&Versioning::new("3.0").unwrap())
        );
        assert!(
            !VersionConstraint::parse("=3.0")
                .unwrap()
                .satisfied_by(&Versioning::new("3.0.1").unwrap())
        );
    }

    #[test]
    fn test_extract_version() {
        assert_eq!(
            extract_version("bison (GNU Bison) 2.3").as_deref(),
            Some("2.3")
        );
        assert_eq!(extract_version("GNU Make 3.81").as_deref(), Some("3.81"));
        assert_eq!(
            extract_version("OpenSSL 3.0.13 30 Jan 2024").as_deref(),
            Some("3.0.13")
        );
        assert_eq!(extract_version("no version here").as_deref(), None);
    }

    #[test]
    fn test_label() {
        let dep = SystemDep {
            check: SystemDepCheck::Bin("bison".into()),
            version: Some(VersionConstraint::parse(">=3.0").unwrap()),
            optional: None,
            packages: Default::default(),
        };
        assert_eq!(dep.label(), "bison >=3.0");

        let dep = SystemDep {
            check: SystemDepCheck::PkgConfig("libxml-2.0".into()),
            version: None,
            optional: None,
            packages: Default::default(),
        };
        assert_eq!(dep.label(), "pkg-config libxml-2.0");
    }

    #[tokio::test]
    async fn test_check_bin_present_and_missing() {
        // `sh` is present on every unix CI runner; on windows use `cmd`.
        let present = if cfg!(windows) { "cmd" } else { "sh" };
        let (ok, _, _) = check_bin(present, None).await;
        assert!(ok);

        let (ok, _, reason) = check_bin("definitely-not-a-real-binary-xyz", None).await;
        assert!(!ok);
        assert!(reason.is_some());
    }

    #[tokio::test]
    async fn test_check_command() {
        let (ok, _, _) = check_command("true").await;
        assert!(ok);
        let (ok, _, _) = check_command("false").await;
        assert!(!ok);
    }

    #[test]
    fn test_version_only_applies_to_bin_and_pkgconfig() {
        // bin + version is honored
        let d = vfox::SystemDependency {
            bin: Some("bison".into()),
            version: Some(">=3.0".into()),
            ..Default::default()
        };
        let dep = SystemDep::try_from(d).unwrap();
        assert!(dep.version.is_some());

        // sharedlib + version: the check is kept, the version is dropped
        let d = vfox::SystemDependency {
            sharedlib: Some("libaio.so.1".into()),
            version: Some(">=1.0".into()),
            ..Default::default()
        };
        let dep = SystemDep::try_from(d).unwrap();
        assert!(matches!(dep.check, SystemDepCheck::SharedLib(_)));
        assert!(dep.version.is_none());
        // and label must not render a phantom version
        assert_eq!(dep.label(), "shared library libaio.so.1");
    }
}
