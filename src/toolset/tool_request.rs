use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::{
    fmt::{Display, Formatter},
    sync::Arc,
};

use eyre::{Result, bail};
use versions::{Chunk, Version};
use xx::file;

use crate::backend::platform_target::PlatformTarget;
use crate::cli::args::BackendArg;
use crate::config::config_file::config_root;
use crate::dirs;
use crate::env;
use crate::install_before::resolve_before_date;
use crate::lockfile::LockfileTool;
use crate::path::PathExt;
use crate::runtime_symlinks::is_runtime_symlink;
use crate::toolset::tool_version::ResolveOptions;
use crate::toolset::{ToolSource, ToolVersion, ToolVersionOptions};
use crate::{backend, lockfile};
use crate::{backend::ABackend, config::Config};

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum ToolRequest {
    Version {
        backend: Arc<BackendArg>,
        version: String,
        options: ToolVersionOptions,
        source: ToolSource,
    },
    Prefix {
        backend: Arc<BackendArg>,
        prefix: String,
        options: ToolVersionOptions,
        source: ToolSource,
    },
    Ref {
        backend: Arc<BackendArg>,
        ref_: String,
        ref_type: String,
        options: ToolVersionOptions,
        source: ToolSource,
    },
    Sub {
        backend: Arc<BackendArg>,
        sub: String,
        orig_version: String,
        options: ToolVersionOptions,
        source: ToolSource,
    },
    Path {
        backend: Arc<BackendArg>,
        path: PathBuf,
        options: ToolVersionOptions,
        source: ToolSource,
    },
    System {
        backend: Arc<BackendArg>,
        source: ToolSource,
        options: ToolVersionOptions,
    },
}

impl ToolRequest {
    pub fn new(backend: Arc<BackendArg>, s: &str, source: ToolSource) -> eyre::Result<Self> {
        let s = match s.split_once('-') {
            Some((ref_type @ ("ref" | "tag" | "branch" | "rev"), r)) => format!("{ref_type}:{r}"),
            _ => s.to_string(),
        };
        Ok(match s.split_once(':') {
            Some((ref_type @ ("ref" | "tag" | "branch" | "rev"), r)) => {
                validate_ref_string(r)?;
                Self::Ref {
                    ref_: r.to_string(),
                    ref_type: ref_type.to_string(),
                    options: backend.opts(),
                    backend,
                    source,
                }
            }
            Some(("prefix", p)) => {
                validate_version_string(p)?;
                Self::Prefix {
                    prefix: p.to_string(),
                    options: backend.opts(),
                    backend,
                    source,
                }
            }
            Some(("path", p)) => {
                validate_path_string(p)?;
                let path = resolve_path(p, &source);
                Self::Path {
                    path,
                    options: backend.opts(),
                    backend,
                    source,
                }
            }
            Some((p, v)) if p.starts_with("sub-") => {
                let sub = p.split_once('-').unwrap().1;
                validate_version_string(sub)?;
                validate_version_string(v)?;
                Self::Sub {
                    sub: sub.to_string(),
                    options: backend.opts(),
                    orig_version: v.to_string(),
                    backend,
                    source,
                }
            }
            None => {
                if s == "system" {
                    Self::System {
                        options: backend.opts(),
                        backend,
                        source,
                    }
                } else {
                    validate_version_string(&s)?;
                    Self::Version {
                        version: s,
                        options: backend.opts(),
                        backend,
                        source,
                    }
                }
            }
            _ => bail!("invalid tool version request: {s}"),
        })
    }
    pub fn new_opts(
        backend: Arc<BackendArg>,
        s: &str,
        options: ToolVersionOptions,
        source: ToolSource,
    ) -> eyre::Result<Self> {
        let mut tvr = Self::new(backend, s, source)?;
        match &mut tvr {
            Self::Version { options: o, .. }
            | Self::Prefix { options: o, .. }
            | Self::Ref { options: o, .. } => *o = options,
            _ => Default::default(),
        }
        Ok(tvr)
    }
    pub fn set_source(&mut self, source: ToolSource) -> Self {
        match self {
            Self::Version { source: s, .. }
            | Self::Prefix { source: s, .. }
            | Self::Ref { source: s, .. }
            | Self::Path { source: s, .. }
            | Self::Sub { source: s, .. }
            | Self::System { source: s, .. } => *s = source,
        }
        self.clone()
    }
    pub fn ba(&self) -> &Arc<BackendArg> {
        match self {
            Self::Version { backend, .. }
            | Self::Prefix { backend, .. }
            | Self::Ref { backend, .. }
            | Self::Path { backend, .. }
            | Self::Sub { backend, .. }
            | Self::System { backend, .. } => backend,
        }
    }
    pub fn backend(&self) -> Result<ABackend> {
        self.ba().backend()
    }
    pub fn source(&self) -> &ToolSource {
        match self {
            Self::Version { source, .. }
            | Self::Prefix { source, .. }
            | Self::Ref { source, .. }
            | Self::Path { source, .. }
            | Self::Sub { source, .. }
            | Self::System { source, .. } => source,
        }
    }
    pub fn os(&self) -> &Option<Vec<String>> {
        match self {
            Self::Version { options, .. }
            | Self::Prefix { options, .. }
            | Self::Ref { options, .. }
            | Self::Path { options, .. }
            | Self::Sub { options, .. }
            | Self::System { options, .. } => &options.os,
        }
    }
    pub fn set_options(&mut self, options: ToolVersionOptions) -> &mut Self {
        match self {
            Self::Version { options: o, .. }
            | Self::Prefix { options: o, .. }
            | Self::Ref { options: o, .. }
            | Self::Sub { options: o, .. }
            | Self::Path { options: o, .. }
            | Self::System { options: o, .. } => *o = options,
        }
        self
    }
    pub fn version(&self) -> String {
        match self {
            Self::Version { version: v, .. } => v.clone(),
            Self::Prefix { prefix: p, .. } => format!("prefix:{p}"),
            Self::Ref {
                ref_: r, ref_type, ..
            } => format!("{ref_type}:{r}"),
            Self::Path { path: p, .. } => format!("path:{}", p.display_user()),
            Self::Sub {
                sub, orig_version, ..
            } => format!("sub-{sub}:{orig_version}"),
            Self::System { .. } => "system".to_string(),
        }
    }

    pub fn options(&self) -> ToolVersionOptions {
        match self {
            Self::Version { options: o, .. }
            | Self::Prefix { options: o, .. }
            | Self::Ref { options: o, .. }
            | Self::Sub { options: o, .. }
            | Self::Path { options: o, .. }
            | Self::System { options: o, .. } => o.clone(),
        }
    }

    pub async fn is_installed(&self, config: &Arc<Config>) -> bool {
        if let Some(backend) = backend::get(self.ba()) {
            match self.resolve(config, &Default::default()).await {
                Ok(tv) => backend.is_version_installed(config, &tv, false),
                Err(e) => {
                    debug!("ToolRequest.is_installed: {e:#}");
                    false
                }
            }
        } else {
            false
        }
    }

    pub fn install_path(&self, config: &Config) -> Option<PathBuf> {
        match self {
            Self::Version {
                backend, version, ..
            } => {
                let path = backend.installs_path.join(version);
                Some(env::find_in_shared_installs(
                    path,
                    &backend.tool_dir_name(),
                    version,
                ))
            }
            Self::Ref {
                backend,
                ref_,
                ref_type,
                ..
            } => {
                let pathname = format!("{ref_type}-{ref_}");
                let path = backend.installs_path.join(&pathname);
                Some(env::find_in_shared_installs(
                    path,
                    &backend.tool_dir_name(),
                    &pathname,
                ))
            }
            Self::Sub {
                backend,
                sub,
                orig_version,
                ..
            } => self
                .local_resolve(config, orig_version)
                .inspect_err(|e| warn!("ToolRequest.local_resolve: {e:#}"))
                .unwrap_or_default()
                .map(|v| {
                    let pathname = version_sub(&v, sub.as_str());
                    let path = backend.installs_path.join(&pathname);
                    env::find_in_shared_installs(path, &backend.tool_dir_name(), &pathname)
                }),
            Self::Prefix {
                backend, prefix, ..
            } => {
                // Check primary install path first
                let found = match file::ls(&backend.installs_path) {
                    Ok(installs) => installs
                        .iter()
                        .find(|p| {
                            !is_runtime_symlink(p)
                                && p.file_name().unwrap().to_string_lossy().starts_with(prefix)
                        })
                        .cloned(),
                    Err(_) => None,
                };
                // Fall back to shared install directories
                found.or_else(|| {
                    let tool_dir_name = backend.tool_dir_name();
                    for shared_dir in env::shared_install_dirs().iter() {
                        let shared_tool_dir = shared_dir.join(&tool_dir_name);
                        if let Ok(installs) = file::ls(&shared_tool_dir)
                            && let Some(p) = installs.iter().find(|p| {
                                !is_runtime_symlink(p)
                                    && p.file_name().unwrap().to_string_lossy().starts_with(prefix)
                            })
                        {
                            return Some(p.clone());
                        }
                    }
                    None
                })
            }
            Self::Path { path, .. } => Some(path.clone()),
            Self::System { .. } => None,
        }
    }

    pub fn lockfile_resolve(&self, config: &Config) -> Result<Option<LockfileTool>> {
        self.lockfile_resolve_with_prefix(config, &self.version())
    }

    /// Like lockfile_resolve but uses a custom prefix instead of self.version().
    /// This is used after alias resolution (e.g., "lts" → "24") so the lockfile
    /// prefix match can find entries like "24.13.0".starts_with("24").
    pub fn lockfile_resolve_with_prefix(
        &self,
        config: &Config,
        prefix: &str,
    ) -> Result<Option<LockfileTool>> {
        let request_options = if let Ok(backend) = self.backend() {
            let target = PlatformTarget::from_current();
            backend.resolve_lockfile_options(self, &target)
        } else {
            BTreeMap::new()
        };
        let path = match self.source() {
            ToolSource::MiseToml(path) => Some(path),
            _ => None,
        };
        lockfile::get_locked_version(
            config,
            path.map(|p| p.as_path()),
            &self.ba().short,
            prefix,
            &request_options,
        )
    }

    pub fn local_resolve(&self, config: &Config, v: &str) -> eyre::Result<Option<String>> {
        if let Some(lt) = self.lockfile_resolve(config)? {
            return Ok(Some(lt.version));
        }
        if let Some(backend) = backend::get(self.ba()) {
            let matches = backend.list_installed_versions_matching(v);
            if matches.iter().any(|m| m == v) {
                return Ok(Some(v.to_string()));
            }
            if let Some(v) = matches.last() {
                return Ok(Some(v.to_string()));
            }
        }
        Ok(None)
    }

    pub async fn resolve(
        &self,
        config: &Arc<Config>,
        opts: &ResolveOptions,
    ) -> Result<ToolVersion> {
        ToolVersion::resolve(config, self.clone(), opts).await
    }

    pub fn resolve_options(&self, opts: &ResolveOptions) -> Result<ResolveOptions> {
        let minimum_release_age = self.options().minimum_release_age().map(str::to_string);
        let mut opts = opts.clone();
        opts.before_date = resolve_before_date(opts.before_date, minimum_release_age.as_deref())?;
        Ok(opts)
    }

    pub fn is_os_supported(&self) -> bool {
        if let Some(os_list) = self.os() {
            let current_os = &crate::cli::version::OS;
            let current_arch = &crate::cli::version::ARCH;
            let matched = os_list.iter().any(|entry| {
                if let Some((os, arch)) = entry.split_once('/') {
                    normalize_os(os) == current_os.as_str()
                        && normalize_arch(arch) == current_arch.as_str()
                } else {
                    normalize_os(entry) == current_os.as_str()
                }
            });
            if !matched {
                return false;
            }
        }
        self.ba().is_os_supported()
    }
}

/// Reject version strings that contain shell-quote-breaking characters,
/// control characters, or path-traversal sequences. Version strings flow into
/// install path names and (for vfox plugins) into `ctx.version` / `ctx.rootPath`
/// values that downstream Lua hooks often interpolate into shell commands.
///
/// The deny list is the minimum set of characters that can break out of either
/// a single- or double-quoted shell string, or that trigger expansion *inside*
/// double quotes: quotes themselves, backslash, backtick, and `$`. Plus control
/// characters (newlines split shell tokens) and `..` (filesystem traversal).
/// Everything else is allowed so legitimate version vocabulary (npm-style
/// semver ranges like `>=20 <21 || >=22` or `^1.0.0`, dates, channel names,
/// `lts/hydrogen`, etc.) continues to work — those characters are only
/// dangerous in *unquoted* shell context, which cannot occur without one of
/// the rejected expansion characters appearing first.
fn validate_version_string(s: &str) -> Result<()> {
    if s.is_empty() {
        return Ok(());
    }
    if s.contains("..") {
        bail!("invalid tool version {s:?}: contains path-traversal sequence");
    }
    if let Some(c) = s.chars().find(|c| is_forbidden_version_char(*c)) {
        bail!("invalid tool version {s:?}: contains forbidden character {c:?}");
    }
    Ok(())
}

/// Validate `ref:`/`branch:`/`tag:`/`rev:` values. Same character rules as
/// version strings: branch/tag names already use the same broad vocabulary
/// (`/`, `+`, `-`, etc.), and only the shell-quote-breaking characters need
/// rejection. Kept as a separate function for distinct error messages.
fn validate_ref_string(s: &str) -> Result<()> {
    if s.is_empty() {
        return Ok(());
    }
    if s.contains("..") {
        bail!("invalid tool ref {s:?}: contains path-traversal sequence");
    }
    if let Some(c) = s.chars().find(|c| is_forbidden_version_char(*c)) {
        bail!("invalid tool ref {s:?}: contains forbidden character {c:?}");
    }
    Ok(())
}

/// Validate `path:` values. Filesystem paths legitimately contain `/`, spaces,
/// and many other characters, but the resolved path becomes `ctx.rootPath` /
/// `installPath` for path-mode tools and is interpolated into shell commands
/// by some plugin hooks. Reject the same shell-quote-breaking characters as
/// version strings — `$`, backtick, quotes, and `\` — so a hostile `path:`
/// entry in a project config cannot inject shell syntax. Path traversal is
/// intentionally not rejected here because `path:../tools/foo` is a normal
/// relative-path use case.
fn validate_path_string(s: &str) -> Result<()> {
    if s.is_empty() {
        return Ok(());
    }
    if let Some(c) = s.chars().find(|c| {
        // Allow newlines/tabs/etc. in paths is still bad — keep control-char
        // and quote/expansion rejection, but allow `/` since paths need it.
        is_forbidden_version_char(*c)
    }) {
        bail!("invalid tool path {s:?}: contains forbidden character {c:?}");
    }
    Ok(())
}

fn is_forbidden_version_char(c: char) -> bool {
    if (c as u32) < 0x20 || c == '\x7f' {
        return true;
    }
    matches!(c, '"' | '\'' | '`' | '\\' | '$')
}

/// Resolve a `path:` tool version request value against the config file's directory.
///
/// - `~/` is expanded to `$HOME`
/// - a leading `./` is stripped
/// - remaining relative paths are joined with `config_root(source)` when the
///   source is a file-based config; otherwise they fall back to the current
///   working directory so CLI usage (e.g. `mise use tool@path:./x`) behaves
///   the way users expect.
fn resolve_path(p: &str, source: &ToolSource) -> PathBuf {
    let p = Path::new(p);
    if let Ok(rest) = p.strip_prefix("~/") {
        return dirs::HOME.join(rest);
    }
    if p.is_absolute() {
        return p.to_path_buf();
    }
    let p = p.strip_prefix("./").unwrap_or(p);
    let base = match source.path() {
        Some(src) => config_root::config_root(src),
        None => dirs::CWD
            .as_ref()
            .cloned()
            .unwrap_or_else(|| PathBuf::from(".")),
    };
    base.join(p)
}

/// Normalize OS name aliases to the canonical form used by `std::env::consts::OS`.
fn normalize_os(os: &str) -> &str {
    match os {
        "darwin" | "macos" => "macos",
        "windows" | "win" => "windows",
        other => other,
    }
}

/// Normalize architecture name aliases to the canonical form used by `cli::version::ARCH`.
fn normalize_arch(arch: &str) -> &str {
    match arch {
        "x86_64" | "amd64" | "x64" => "x64",
        "aarch64" | "arm64" => "arm64",
        other => other,
    }
}

/// subtracts sub from orig and removes suffix
/// e.g. version_sub("18.2.3", "2") -> "16"
/// e.g. version_sub("18.2.3", "0.1") -> "18.1"
/// e.g. version_sub("2.79.0", "0.0.1") -> "2.78" (underflow, returns prefix)
pub fn version_sub(orig: &str, sub: &str) -> String {
    let mut orig = Version::new(orig).unwrap();
    let sub = Version::new(sub).unwrap();
    while orig.chunks.0.len() > sub.chunks.0.len() {
        orig.chunks.0.pop();
    }
    for i in 0..orig.chunks.0.len() {
        let m = sub.nth(i).unwrap();
        let orig_val = orig.chunks.0[i].single_digit().unwrap();

        if orig_val < m {
            // Handle underflow with borrowing from higher digits
            for j in (0..i).rev() {
                let prev_val = orig.chunks.0[j].single_digit().unwrap();
                if prev_val > 0 {
                    orig.chunks.0[j] = Chunk::Numeric(prev_val - 1);
                    orig.chunks.0.truncate(j + 1);
                    return orig.to_string();
                }
            }
            return "0".to_string();
        }

        orig.chunks.0[i] = Chunk::Numeric(orig_val - m);
    }
    orig.to_string()
}

impl Display for ToolRequest {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}@{}", &self.ba(), self.version())
    }
}

#[cfg(test)]
mod tests {
    use super::{validate_ref_string, validate_version_string, version_sub};
    use pretty_assertions::assert_str_eq;
    use test_log::test;

    #[test]
    fn test_validate_version_string_accepts_real_versions() {
        for v in [
            // concrete versions seen in the wild
            "1.2.3",
            "1.2.3-beta",
            "1.2.3+build",
            "20240115",
            "lts/hydrogen",
            "lts-iron",
            "latest",
            "3.12.0a1",
            "3.2.0-preview1",
            "tip",
            "HEAD",
            "nightly",
            "1.2.3~rc1",
            "v1.2.3",
            "1.20.0-rc.4-otp-29",
            "29.0-rc3",
            "2.35.0-beta.01",
            "stable",
            "3.16-dev",
            // npm-style semver range queries (from package.json engines)
            ">=20.0.0",
            ">= 25.6.1",
            "^1.0.0",
            "~1.2.3",
            "*",
            "25.x",
            ">=20 <21 || >=22",
            ">=18 <20 || >=22",
        ] {
            assert!(
                validate_version_string(v).is_ok(),
                "expected {v:?} to be accepted"
            );
        }
    }

    #[test]
    fn test_validate_version_string_rejects_metacharacters() {
        for v in [
            // quote / expansion characters that break shell context
            "1.0$(id)",
            "1.0`id`",
            "1.0$HOME",
            "1.0\"x",
            "1.0'x",
            "1.0\\x",
            // control characters / newline splitting
            "1.0\nrm",
            "1.0\rrm",
            "1.0\tx",
            "1.0\x00x",
            // path traversal
            "../etc/passwd",
            "1.0/../etc",
        ] {
            assert!(
                validate_version_string(v).is_err(),
                "expected {v:?} to be rejected"
            );
        }
    }

    #[test]
    fn test_validate_ref_string_allows_slash() {
        assert!(validate_ref_string("feature/foo").is_ok());
        assert!(validate_ref_string("release/1.2").is_ok());
        assert!(validate_ref_string("main").is_ok());
    }

    #[test]
    fn test_validate_ref_string_rejects_metacharacters() {
        for v in ["a$(id)", "a..b", "a`b`", "a\"b", "a'b", "a\\b"] {
            assert!(
                validate_ref_string(v).is_err(),
                "expected ref {v:?} to be rejected"
            );
        }
    }

    #[test]
    fn test_validate_path_string() {
        use super::validate_path_string;
        // valid paths
        for p in [
            "/home/user/tools/foo",
            "./relative/path",
            "../parent",
            "~/tools/bar",
            "/path with spaces/tool",
            "C:/Users/foo",
        ] {
            assert!(
                validate_path_string(p).is_ok(),
                "expected path {p:?} to be accepted"
            );
        }
        // shell-dangerous paths
        for p in [
            "/tmp/$HOME",
            "/tmp/`id`",
            "/tmp/$(id)",
            "/tmp/'rm",
            "/tmp/\"rm",
            "/tmp/\\rm",
            "/tmp/\nrm",
        ] {
            assert!(
                validate_path_string(p).is_err(),
                "expected path {p:?} to be rejected"
            );
        }
    }

    #[test]
    fn test_version_sub() {
        assert_str_eq!(version_sub("18.2.3", "2"), "16");
        assert_str_eq!(version_sub("18.2.3", "0.1"), "18.1");
        assert_str_eq!(version_sub("18.2.3", "0.0.1"), "18.2.2");
    }

    #[test]
    fn test_version_sub_underflow() {
        // Test cases that would cause underflow return prefix for higher digit
        assert_str_eq!(version_sub("2.0.0", "0.0.1"), "1");
        assert_str_eq!(version_sub("2.79.0", "0.0.1"), "2.78");
        assert_str_eq!(version_sub("1.0.0", "0.1.0"), "0");
        assert_str_eq!(version_sub("0.1.0", "1"), "0");
        assert_str_eq!(version_sub("1.2.3", "0.2.4"), "0");
        assert_str_eq!(version_sub("1.3.3", "0.2.4"), "1.0");
    }

    #[test]
    fn test_normalize_os() {
        use super::normalize_os;
        assert_eq!(normalize_os("macos"), "macos");
        assert_eq!(normalize_os("darwin"), "macos");
        assert_eq!(normalize_os("linux"), "linux");
        assert_eq!(normalize_os("windows"), "windows");
        assert_eq!(normalize_os("win"), "windows");
        assert_eq!(normalize_os("freebsd"), "freebsd");
    }

    #[test]
    fn test_normalize_arch() {
        use super::normalize_arch;
        assert_eq!(normalize_arch("arm64"), "arm64");
        assert_eq!(normalize_arch("aarch64"), "arm64");
        assert_eq!(normalize_arch("x64"), "x64");
        assert_eq!(normalize_arch("x86_64"), "x64");
        assert_eq!(normalize_arch("amd64"), "x64");
        assert_eq!(normalize_arch("riscv64"), "riscv64");
    }
}
