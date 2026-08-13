use crate::backend::options::BackendOptions;
use crate::backend::platform_target::PlatformTarget;
use crate::backend::static_helpers::fetch_checksum_from_shasums;
use crate::backend::{Backend, VersionCacheManager, VersionInfo};
use crate::build_time::built_info;
use crate::cache::{CacheManager, CacheManagerBuilder};
use crate::cli::args::BackendArg;
use crate::cmd::CmdLineRunner;
use crate::config::{Config, Settings};
use crate::file::{ExtractOptions, ExtractionFormat, display_path};
use crate::git::{CloneOptions, Git};
use crate::http::{HTTP, HTTP_FETCH};
use crate::install_context::InstallContext;
use crate::lockfile::{PlatformInfo, ProvenanceType};
use crate::platform::Platform;
use crate::toolset::{ToolRequest, ToolVersion, ToolVersionOptions, Toolset};
use crate::ui::progress_report::SingleReport;
use crate::{Result, lock_file::LockFile};
use crate::{dirs, file, plugins, sysconfig};
use async_trait::async_trait;
use eyre::{bail, eyre};
use flate2::read::GzDecoder;
use itertools::Itertools;
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::LazyLock as Lazy;
use std::sync::{Arc, OnceLock};
use tokio::sync::Mutex;
use versions::Versioning;
use xx::regex;

const ATTESTATION_HELP: &str = "To disable attestation verification, set MISE_PYTHON_GITHUB_ATTESTATIONS=false\n\
    or add `python.github_attestations = false` under [settings] in mise.toml";
const PBS_RELEASE_DOWNLOAD_URL: &str =
    "https://github.com/astral-sh/python-build-standalone/releases/download/";
/// PyPy's own release index. python-build-standalone ships CPython only, so this is where the
/// precompiled path has to look for a `pypy*` version.
const PYPY_VERSIONS_URL: &str = "https://downloads.python.org/pypy/versions.json";

/// One entry of PyPy's `versions.json`. Only the fields mise uses are modeled.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct PypyRelease {
    pypy_version: String,
    python_version: String,
    #[serde(default)]
    date: Option<String>,
    files: Vec<PypyFile>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct PypyFile {
    filename: String,
    arch: String,
    platform: String,
    download_url: String,
}

#[derive(Debug)]
pub struct PythonPlugin {
    ba: Arc<BackendArg>,
}

#[derive(Debug, Clone, Copy)]
struct PythonOptions<'a> {
    values: BackendOptions<'a>,
}

impl<'a> PythonOptions<'a> {
    fn new(raw: &'a ToolVersionOptions) -> Self {
        Self {
            values: BackendOptions::new(raw),
        }
    }

    fn patch_sysconfig(&self) -> bool {
        self.values.bool_with_default("patch_sysconfig", true)
    }

    fn virtualenv(&self) -> Option<&'a str> {
        self.values.str("virtualenv")
    }

    fn lockfile_options(&self) -> BTreeMap<String, String> {
        let mut opts = BTreeMap::new();
        if !self.patch_sysconfig() {
            opts.insert("patch_sysconfig".into(), "false".into());
        }
        opts
    }
}

pub fn python_path(tv: &ToolVersion) -> PathBuf {
    if cfg!(windows) {
        tv.install_path().join("python.exe")
    } else {
        tv.install_path().join("bin/python")
    }
}

/// Create the conventional `python3` entry next to `python.exe` on Windows.
///
/// The python-build-standalone Windows archives only ship `python.exe`, while
/// cross-platform scripts commonly invoke `python3`. Keeping the alias inside
/// the install directory lets normal PATH and shim discovery handle it just
/// like an upstream executable.
#[cfg(windows)]
fn install_python3_windows(tv: &ToolVersion) -> Result<()> {
    let python_exe = tv.install_path().join("python.exe");
    let python3_exe = tv.install_path().join("python3.exe");

    file::remove_all(&python3_exe)?;
    match std::fs::hard_link(&python_exe, &python3_exe) {
        Ok(()) => Ok(()),
        Err(e) => {
            debug!(
                "python: hardlink {python_exe} as {python3_exe} failed ({e}); copying executable",
                python_exe = python_exe.display(),
                python3_exe = python3_exe.display(),
            );
            std::fs::copy(&python_exe, &python3_exe)?;
            Ok(())
        }
    }
}

/// Create `pip.cmd`/`pip3.cmd` wrappers next to `python.exe` on Windows.
///
/// python-build-standalone Windows archives ship pip only as a site-packages
/// module — there is no `Scripts\pip.exe` launcher (upstream quirk), and
/// `python -m pip install --upgrade pip` is a no-op while pip is current, so
/// the launcher never appears on its own. Like the synthesized `python3.exe`
/// above, keeping the wrappers inside the install root lets normal PATH and
/// shim discovery handle them. Delegating to `python -m pip` means the
/// wrappers always dispatch to the pip currently in site-packages, so they
/// never go stale even if the user later reinstalls pip itself.
#[cfg(windows)]
fn install_pip_wrappers_windows(tv: &ToolVersion) -> Result<()> {
    // if a real pip launcher ever ships, prefer it over synthesizing wrappers
    if tv.install_path().join("Scripts").join("pip.exe").exists() {
        return Ok(());
    }
    // CRLF endings per batch-file convention; no trailing `exit /b` needed —
    // cmd returns the errorlevel of the script's last command.
    const WRAPPER: &str = "@echo off\r\n\"%~dp0python.exe\" -m pip %*\r\n";
    for name in ["pip.cmd", "pip3.cmd"] {
        file::write(tv.install_path().join(name), WRAPPER)?;
    }
    Ok(())
}

/// Sort key for Python versions that handles miniconda's two versioning schemes correctly.
///
/// Miniconda has two formats:
/// - Old format: `miniconda3-{conda_version}` (e.g., `miniconda3-3.16.0`, `miniconda3-4.7.12`)
/// - New format: `miniconda3-{python_version}-{conda_version}` (e.g., `miniconda3-3.7-4.8.2`)
///
/// Returns a tuple for sorting: (distro_priority, prefix_order, is_not_latest, conda_version, python_version)
/// distro_priority: 0 = other distros, 1 = miniconda, 2 = CPython (bare version numbers)
fn python_version_sort_key(
    version: &str,
) -> (u8, u8, bool, Option<Versioning>, Option<Versioning>) {
    // Check if this is a miniconda version and get prefix order
    let (prefix_order, version_part) = if let Some(v) = version.strip_prefix("miniconda3-") {
        (2u8, v)
    } else if let Some(v) = version.strip_prefix("miniconda2-") {
        (1u8, v)
    } else if let Some(v) = version.strip_prefix("miniconda-") {
        (0u8, v)
    } else {
        // Not miniconda - put other distros first (0), CPython (digit-starting) last (2)
        let starts_with_digit = regex!(r"^\d").is_match(version);
        return (if starts_with_digit { 2 } else { 0 }, 0, false, None, None);
    };

    // Handle "latest" specially - put first in each miniconda group
    if version_part == "latest" {
        return (1, prefix_order, false, None, None);
    }

    // Parse miniconda version: old format vs new format
    // Old format has no dash in version part: "3.16.0"
    // New format has dash separating python and conda: "3.7-4.8.2"
    let (conda_version, python_version) = if let Some(dash_pos) = version_part.find('-') {
        // New format: "3.7-4.8.2" -> python=3.7, conda=4.8.2
        let python = &version_part[..dash_pos];
        let conda = &version_part[dash_pos + 1..];
        (Versioning::new(conda), Versioning::new(python))
    } else {
        // Old format: "3.16.0" -> conda=3.16.0, no python version
        (Versioning::new(version_part), None)
    };

    (1, prefix_order, true, conda_version, python_version)
}

impl PythonPlugin {
    pub fn new() -> Self {
        let ba = Arc::new(plugins::core::new_backend_arg("python"));
        Self { ba }
    }

    fn python_build_path(&self) -> PathBuf {
        self.ba.cache_path.join("pyenv")
    }
    fn python_build_bin(&self) -> PathBuf {
        self.python_build_path()
            .join("plugins/python-build/bin/python-build")
    }
    fn lock_pyenv(&self) -> Result<fslock::LockFile> {
        LockFile::new(&self.python_build_path())
            .with_callback(|l| {
                trace!("install_or_update_pyenv {}", l.display());
            })
            .lock()
    }
    fn install_or_update_python_build(&self, ctx: Option<&InstallContext>) -> eyre::Result<()> {
        ensure_not_windows()?;
        let _lock = self.lock_pyenv();
        if self.python_build_bin().exists() {
            self.update_python_build()
        } else {
            self.install_python_build(ctx)
        }
    }
    fn install_python_build(&self, ctx: Option<&InstallContext>) -> eyre::Result<()> {
        if self.python_build_bin().exists() {
            return Ok(());
        }
        let python_build_path = self.python_build_path();
        debug!("Installing python-build to {}", python_build_path.display());
        file::remove_all(&python_build_path)?;
        file::create_dir_all(self.python_build_path().parent().unwrap())?;
        let git = Git::new(self.python_build_path());
        let pr = ctx.map(|ctx| ctx.pr.as_ref());
        let mut clone_options = CloneOptions::default();
        if let Some(pr) = pr {
            clone_options = clone_options.pr(pr);
        }
        git.clone(&Settings::get().python.pyenv_repo, clone_options)?;
        Ok(())
    }
    fn update_python_build(&self) -> eyre::Result<()> {
        // TODO: do not update if recently updated
        debug!(
            "Updating python-build in {}",
            self.python_build_path().display()
        );
        let pyenv_path = self.python_build_path();
        let git = Git::new(pyenv_path.clone());
        match plugins::core::run_fetch_task_with_timeout(move || git.update(None)) {
            Ok(_) => Ok(()),
            Err(err) => {
                warn!(
                    "failed to update python-build repo ({}), attempting self-repair by recloning",
                    err
                );
                // The cached pyenv repo can get corrupted (e.g. unable to read sha1 file).
                // Repair by removing the cache and performing a fresh clone.
                file::remove_all(&pyenv_path)?;
                // Safe to reinstall without a context; progress reporting is optional here.
                self.install_python_build(None)
            }
        }
    }
    fn python_build_definition_created_at(&self) -> eyre::Result<BTreeMap<String, String>> {
        let output = crate::cmd!(
            "git",
            "-C",
            self.python_build_path(),
            "-c",
            format!("safe.directory={}", self.python_build_path().display()),
            "log",
            "--format=%cI",
            "--diff-filter=A",
            "--name-only",
            "--",
            "plugins/python-build/share/python-build",
        )
        .read()?;
        Ok(parse_python_build_definition_created_at(&output))
    }

    async fn fetch_precompiled_remote_versions(
        &self,
    ) -> eyre::Result<&Vec<(String, String, String)>> {
        static PRECOMPILED_CACHE: Lazy<CacheManager<Vec<(String, String, String)>>> =
            Lazy::new(|| {
                CacheManagerBuilder::new(dirs::CACHE.join("python").join("precompiled.msgpack.z"))
                    .with_fresh_duration(Settings::get().fetch_remote_versions_cache())
                    .with_cache_key(python_precompiled_platform())
                    .build()
            });
        PRECOMPILED_CACHE
            .get_or_try_init_async(async || {
                let settings = Settings::get();
                let url_path = python_precompiled_url_path(&settings);
                let rsp = HTTP_FETCH
                    .get_bytes(format!("https://mise-versions.jdx.dev/tools/{url_path}"))
                    .await?;
                let mut decoder = GzDecoder::new(rsp.as_ref());
                let mut raw = String::new();
                decoder.read_to_string(&mut raw)?;
                let platform = python_precompiled_platform();
                let flavor = settings.python.precompiled_flavor.clone();
                // order by version, whether it is a release candidate, date, and in the preferred order of install types
                let rank = |v: &str, date: &str, name: &str| {
                    let rc = if regex!(r"rc\d+$").is_match(v) { 0 } else { 1 };
                    let v = Versioning::new(v);
                    let date = date.parse::<i64>().unwrap_or_default();
                    let install_type = if let Some(ref flavor) = flavor {
                        // When flavor is set, prefer exact match
                        let name_without_ext = name.trim_end_matches(".tar.gz");
                        if name_without_ext.ends_with(flavor.as_str()) {
                            0
                        } else {
                            1
                        }
                    } else if name.contains("install_only_stripped") {
                        0
                    } else if name.contains("install_only") {
                        1
                    } else {
                        2
                    };
                    (v, rc, -date, install_type)
                };
                let versions = raw
                    .lines()
                    .filter(|v| v.contains(&platform))
                    .filter(|v| filter_freethreaded(v, &flavor))
                    .flat_map(|v| {
                        // cpython-3.9.5+20210525 or cpython-3.9.5rc3+20210525
                        regex!(r"^cpython-(\d+\.\d+\.[\da-z]+)\+(\d+).*")
                            .captures(v)
                            .map(|caps| {
                                (
                                    caps[1].to_string(),
                                    caps[2].to_string(),
                                    caps[0].to_string(),
                                )
                            })
                    })
                    // multiple dates can have the same version, so sort by date and remove duplicates by unique
                    .sorted_by_cached_key(|(v, date, name)| rank(v, date, name))
                    .unique_by(|(v, _, _)| v.to_string())
                    .collect_vec();
                Ok(versions)
            })
            .await
    }

    async fn fetch_pypy_releases(&self) -> eyre::Result<&Vec<PypyRelease>> {
        static PYPY_CACHE: Lazy<CacheManager<Vec<PypyRelease>>> = Lazy::new(|| {
            CacheManagerBuilder::new(dirs::CACHE.join("python").join("pypy.msgpack.z"))
                .with_fresh_duration(Settings::get().fetch_remote_versions_cache())
                .build()
        });
        PYPY_CACHE
            .get_or_try_init_async(async || Ok(HTTP_FETCH.json(PYPY_VERSIONS_URL).await?))
            .await
    }

    /// Resolve the archive a `pypy*` version would install on `target`, for `mise lock`.
    ///
    /// The URL is always recorded when one exists; the checksum stays unset because PyPy publishes
    /// none in machine-readable form, and `verify_checksum` fills it in on first install. An
    /// unresolvable target (no build published, or a version outside the index) records nothing
    /// rather than failing the lock of the other platforms.
    async fn resolve_pypy_lock_info(
        &self,
        version: &str,
        target: &PlatformTarget,
    ) -> Result<PlatformInfo> {
        // Same split as fetch_precompiled_for_target: settings win for the platform we are on,
        // the target's own values describe every other one.
        let settings = Settings::get();
        let (os, arch) = if target.is_current() {
            // settings.os(), not std::env::consts::OS: `is_current` compares against
            // Platform::current(), which is built from settings.os(). Reading the host OS here
            // would let `MISE_OS=linux` on a windows host take this branch and then record a
            // win64 archive under the linux platform key.
            (settings.os(), settings.arch())
        } else {
            (target.os_name(), target.arch_name())
        };
        let Some((platform, arch)) = pypy_platform_arch(os, arch) else {
            return Ok(PlatformInfo::default());
        };
        let releases = self.fetch_pypy_releases().await?;
        let url = releases
            .iter()
            .find(|r| pypy_version_str(r).as_deref() == Some(version))
            .and_then(|r| pypy_file_for(r, platform, arch))
            .map(|f| f.download_url.clone());
        Ok(PlatformInfo {
            url,
            ..Default::default()
        })
    }

    /// Install a `pypy*` version from PyPy's own downloads.
    ///
    /// python-build-standalone publishes CPython only, so the precompiled path cannot serve these.
    /// PyPy publishes no checksums next to its archives — they live on an HTML page — so integrity
    /// rests on `verify_checksum`, which locks the digest on first install and enforces it after,
    /// the same treatment every backend gives a source without upstream checksums. There is no
    /// provenance step for the same reason: PyPy publishes no attestations.
    async fn install_pypy(&self, ctx: &InstallContext, tv: &mut ToolVersion) -> eyre::Result<()> {
        let platform_key = self.get_platform_key();
        let url = if let Some(url) = tv
            .lock_platforms
            .get(&platform_key)
            .and_then(|pi| pi.url.clone())
        {
            debug!("using lockfile URL for platform {platform_key}: {url}");
            url
        } else {
            let settings = Settings::get();
            let os = settings.os();
            let (platform, arch) = pypy_platform_arch(os, settings.arch())
                .ok_or_else(|| eyre!("pypy publishes no build for {os}-{}", settings.arch()))?;
            let releases = self.fetch_pypy_releases().await?;
            let release = releases
                .iter()
                .find(|r| pypy_version_str(r).as_deref() == Some(tv.version.as_str()))
                .ok_or_else(|| eyre!("no pypy release found for {tv}"))?;
            let file = pypy_file_for(release, platform, arch).ok_or_else(|| {
                eyre!(
                    "pypy {} publishes no {platform}/{arch} build",
                    release.pypy_version
                )
            })?;
            file.download_url.clone()
        };
        let filename = url.split('/').next_back().unwrap();

        let tarball_path = tv.download_path().join(filename);
        ctx.pr.set_message(format!("download {filename}"));
        HTTP.download_file(&url, &tarball_path, Some(ctx.pr.as_ref()))
            .await?;
        tv.lock_platforms.entry(platform_key).or_default().url = Some(url.clone());
        self.verify_checksum(ctx, tv, &tarball_path)?;

        let install = tv.install_path();
        file::remove_all(&install)?;
        file::extract_archive(
            &tarball_path,
            &install,
            ExtractionFormat::from_file_name(filename),
            &ExtractOptions {
                strip_components: 1,
                pr: Some(ctx.pr.as_ref()),
                ..Default::default()
            },
        )?;

        // The archives already carry the interpreter entrypoints — `bin/python` on unix,
        // `python.exe`/`python3.exe` at the root on windows — but no pip. python-build ran
        // `ensurepip` on its pypy definitions for exactly that reason; the wheel ships inside the
        // archive, so this needs no network. It lands `bin/pip*` on unix and `Scripts\pip.exe` on
        // windows, both of which `list_bin_paths` already exposes.
        ctx.pr.set_message("ensurepip".into());
        CmdLineRunner::new(python_path(tv))
            .with_pr(ctx.pr.as_ref())
            .args(["-m", "ensurepip", "--upgrade", "--default-pip"])
            .env("PIP_REQUIRE_VIRTUALENV", "false")
            .execute()?;

        // Belt and braces: current releases ship `bin/python` as a symlink already.
        if !install.join("bin").join("python").exists() {
            #[cfg(unix)]
            file::make_symlink(&install.join("bin/python3"), &install.join("bin/python"))?;
        }
        Ok(())
    }

    async fn install_precompiled(
        &self,
        ctx: &InstallContext,
        tv: &mut ToolVersion,
    ) -> eyre::Result<()> {
        // Only where the precompiled list is actually the source of truth. With `compile` unset on
        // unix this function is still entered, but a `pypy*` version falls through to python-build
        // below and installs fine today — no reason to change that here.
        if tv.version.starts_with("pypy") {
            if cfg!(windows) || Settings::get().python.compile == Some(false) {
                return self.install_pypy(ctx, tv).await;
            }
            // With `compile` unset on unix this function is still entered, but python-build owns
            // pypy there and installs it fine today. Hand over directly rather than falling
            // through: the CPython path below would pick up a pypy URL recorded in the lockfile
            // and then verify it against python-build-standalone's attestations, which it is not
            // from. It would also warn about a missing precompiled build that was never expected.
            return self.install_compiled(ctx, tv).await;
        }
        let platform_key = self.get_platform_key();
        let url = if let Some(url) = tv
            .lock_platforms
            .get(&platform_key)
            .and_then(|pi| pi.url.clone())
        {
            debug!("using lockfile URL for platform {platform_key}: {url}");
            url
        } else {
            let precompiled_versions = self.fetch_precompiled_remote_versions().await?;
            let precompile_info = precompiled_versions
                .iter()
                .rev()
                .find(|(v, _, _)| &tv.version == v);
            let (tag, filename) = match precompile_info {
                Some((_, tag, filename)) => (tag, filename),
                None => {
                    if cfg!(windows) || Settings::get().python.compile == Some(false) {
                        if !cfg!(windows) {
                            hint!(
                                "python_compile",
                                "To compile python from source, run",
                                "mise settings python.compile=1"
                            );
                        }
                        let platform = python_precompiled_platform();
                        bail!("no precompiled python found for {tv} on {platform}");
                    }
                    let available = precompiled_versions.iter().map(|(v, _, _)| v).collect_vec();
                    if available.is_empty() {
                        debug!("no precompiled python found for {}", tv.version);
                    } else {
                        warn!(
                            "no precompiled python found for {}, force mise to use a precompiled version with `mise settings set python.compile=false`",
                            tv.version
                        );
                    }
                    trace!(
                        "available precompiled versions: {}",
                        available.into_iter().join(", ")
                    );
                    return self.install_compiled(ctx, tv).await;
                }
            };

            if cfg!(unix) {
                hint!(
                    "python_precompiled",
                    "installing precompiled python from astral-sh/python-build-standalone\n\
                    if you experience issues with this python (e.g.: running poetry), switch to python-build by running",
                    "mise settings python.compile=1"
                );
            }

            format!(
                "https://github.com/astral-sh/python-build-standalone/releases/download/{tag}/{filename}"
            )
        };
        let filename = url.split('/').next_back().unwrap();
        let install = tv.install_path();
        let download = tv.download_path();
        let tarball_path = download.join(filename);

        ctx.pr.set_message(format!("download {filename}"));
        HTTP.download_file(&url, &tarball_path, Some(ctx.pr.as_ref()))
            .await?;

        // Record the URL in lock_platforms so verify_checksum can find it
        tv.lock_platforms
            .entry(platform_key.clone())
            .or_default()
            .url = Some(url.to_string());

        // Check before verify_checksum, which may generate a new checksum from the
        // downloaded file. We only skip provenance when the lockfile already had
        // integrity data before this install.
        let has_lockfile_integrity = Self::has_precompiled_lockfile_integrity(tv, &platform_key);

        self.verify_checksum(ctx, tv, &tarball_path)?;

        let settings = Settings::get();
        if has_lockfile_integrity && !settings.force_provenance_verify() {
            Self::ensure_precompiled_provenance_setting_enabled(tv, &platform_key)?;
        } else {
            self.verify_precompiled_provenance(ctx, tv, &platform_key, &tarball_path)
                .await?;
        }

        file::remove_all(&install)?;
        file::extract_archive(
            &tarball_path,
            &install,
            ExtractionFormat::from_file_name(filename),
            &ExtractOptions {
                strip_components: 1,
                pr: Some(ctx.pr.as_ref()),
                ..Default::default()
            },
        )?;
        if !install.join("bin").exists() {
            // debug builds of indygreg binaries have a different structure
            for entry in file::ls(&install.join("install"))? {
                let filename = entry.file_name().unwrap();
                file::remove_all(install.join(filename))?;
                file::rename(&entry, install.join(filename))?;
            }
        }

        let re_digits = regex!(r"\d+");
        let version_parts = tv.version.split('.').collect_vec();
        let major = re_digits
            .find(version_parts[0])
            .and_then(|m| m.as_str().parse().ok());
        let minor = re_digits
            .find(version_parts[1])
            .and_then(|m| m.as_str().parse().ok());
        let suffix = version_parts
            .get(2)
            .map(|s| re_digits.replace(s, "").to_string());
        if cfg!(unix) {
            if let (Some(major), Some(minor), Some(suffix)) = (major, minor, suffix) {
                let raw_opts = tv.request.options();
                let opts = PythonOptions::new(&raw_opts);
                if opts.patch_sysconfig() {
                    sysconfig::update_sysconfig(&install, major, minor, &suffix)?;
                }
            } else {
                debug!("failed to update sysconfig with version {}", tv.version);
            }
        }

        if !install.join("bin").join("python").exists() {
            #[cfg(unix)]
            file::make_symlink(&install.join("bin/python3"), &install.join("bin/python"))?;
        }

        #[cfg(windows)]
        {
            install_python3_windows(tv)?;
            install_pip_wrappers_windows(tv)?;
        }

        Ok(())
    }

    async fn install_compiled(&self, ctx: &InstallContext, tv: &ToolVersion) -> eyre::Result<()> {
        self.install_or_update_python_build(Some(ctx))?;
        if matches!(&tv.request, ToolRequest::Ref { .. }) {
            return Err(eyre!("Ref versions not supported for python"));
        }
        ctx.pr.set_message("python-build".into());
        let mut cmd = CmdLineRunner::new(self.python_build_bin())
            .with_pr(ctx.pr.as_ref())
            .arg(tv.version.as_str())
            .arg(tv.install_path())
            .envs(ctx.config.env().await?)
            .env_values(tv.install_env())
            .env("PIP_REQUIRE_VIRTUALENV", "false");
        if Settings::get().verbose {
            cmd = cmd.arg("--verbose");
        }
        if let Some(patch_url) = &Settings::get().python.patch_url {
            ctx.pr
                .set_message(format!("with patch file from: {patch_url}"));
            let patch = HTTP.get_text(patch_url).await?;
            cmd = cmd.arg("--patch").stdin_string(patch)
        }
        if let Some(patches_dir) = &Settings::get().python.patches_directory {
            let patch_file = patches_dir.join(format!("{}.patch", tv.version));
            if patch_file.exists() {
                ctx.pr
                    .set_message(format!("with patch file: {}", patch_file.display()));
                let contents = file::read_to_string(&patch_file)?;
                cmd = cmd.arg("--patch").stdin_string(contents);
            } else {
                warn!("patch file not found: {}", patch_file.display());
            }
        }
        cmd.execute()?;
        Ok(())
    }

    async fn install_default_packages(
        &self,
        config: &Arc<Config>,
        packages_file: &Path,
        tv: &ToolVersion,
        pr: &dyn SingleReport,
    ) -> eyre::Result<()> {
        if !packages_file.exists() {
            return Ok(());
        }
        if file::read_to_string(packages_file)
            .unwrap_or_default()
            .lines()
            .any(|package| Settings::parse_default_package_line(package).is_some())
        {
            Settings::warn_default_package_file_deprecated(
                "python.default_packages_file",
                "python package",
            );
        }
        pr.set_message("install default packages".into());
        CmdLineRunner::new(python_path(tv))
            .with_pr(pr)
            .arg("-m")
            .arg("pip")
            .arg("install")
            .arg("--upgrade")
            .arg("-r")
            .arg(packages_file)
            .envs(config.env().await?)
            .env_values(tv.install_env())
            .env("PIP_REQUIRE_VIRTUALENV", "false")
            .execute()
    }

    async fn get_virtualenv(
        &self,
        config: &Arc<Config>,
        tv: &ToolVersion,
    ) -> eyre::Result<Option<PathBuf>> {
        let raw_opts = tv.request.options();
        let opts = PythonOptions::new(&raw_opts);
        if let Some(virtualenv) = opts.virtualenv() {
            deprecated_at!(
                "2026.7.0",
                "2027.7.0",
                "python.virtualenv",
                "the python `virtualenv` tool option is deprecated. Use `_.python.venv` in the `[env]` section instead: https://mise.jdx.dev/lang/python.html#automatic-virtualenv-activation"
            );
            let mut virtualenv: PathBuf = file::replace_path(Path::new(virtualenv));
            if !virtualenv.is_absolute()
                && let Some(project_root) = &config.project_root
            {
                virtualenv = project_root.join(virtualenv);
            }
            if !virtualenv.exists() {
                warn!(
                    "no venv found at: {p}\n\n\
                    To create a virtualenv manually, run:\n\
                    python -m venv {p}",
                    p = display_path(&virtualenv)
                );
                return Ok(None);
            }
            // TODO: enable when it is more reliable
            // self.check_venv_python(&virtualenv, tv)?;
            Ok(Some(virtualenv))
        } else {
            Ok(None)
        }
    }

    // fn check_venv_python(&self, virtualenv: &Path, tv: &ToolVersion) -> eyre::Result<()> {
    //     let symlink = virtualenv.join("bin/python");
    //     let target = python_path(tv);
    //     let symlink_target = symlink.read_link().unwrap_or_default();
    //     ensure!(
    //         symlink_target == target,
    //         "expected venv {} to point to {}.\nTry deleting the venv at {}.",
    //         display_path(&symlink),
    //         display_path(&target),
    //         display_path(virtualenv)
    //     );
    //     Ok(())
    // }

    async fn test_python(
        &self,
        config: &Arc<Config>,
        tv: &ToolVersion,
        pr: &dyn SingleReport,
    ) -> eyre::Result<()> {
        pr.set_message("python --version".into());
        CmdLineRunner::new(python_path(tv))
            .with_pr(pr)
            .arg("--version")
            .envs(config.env().await?)
            .env_values(tv.install_env())
            .execute()
    }

    /// Fetch the best precompiled release for a specific version and platform target.
    /// Unlike `fetch_precompiled_remote_versions` which uses compile-time cfg!() macros,
    /// this takes a PlatformTarget to support cross-platform lockfile generation.
    /// Respects precompiled_arch, precompiled_os, and precompiled_flavor settings
    /// when the target matches the current platform.
    async fn fetch_precompiled_for_target(
        &self,
        version: &str,
        target: &PlatformTarget,
        locked_filename: Option<&str>,
    ) -> eyre::Result<Option<(String, String)>> {
        let settings = Settings::get();

        // Use settings-aware arch/os for the current platform,
        // target-based defaults for other platforms
        let (arch, os) = if target.is_current() {
            (python_arch(&settings).to_string(), python_os(&settings))
        } else {
            (
                python_arch_for_target(target).to_string(),
                python_os_for_target(target).to_string(),
            )
        };

        let platform = format!("{arch}-{os}");
        let url_path = format!("python-precompiled-{arch}-{os}.gz");
        let rsp = HTTP_FETCH
            .get_bytes(format!("https://mise-versions.jdx.dev/tools/{url_path}"))
            .await?;
        let mut decoder = GzDecoder::new(rsp.as_ref());
        let mut raw = String::new();
        decoder.read_to_string(&mut raw)?;

        let flavor = settings.python.precompiled_flavor.clone();

        // Prefer the PBS artifact already recorded in the lockfile so a plain
        // `mise lock` refreshes the same artifact. `mise lock --bump` resolves
        // without the existing lockfile, so locked_filename is None and the
        // newest build wins.
        let result =
            select_python_precompiled(&raw, version, &platform, flavor.as_deref(), locked_filename);
        if let Some(locked_filename) = locked_filename
            && result
                .as_ref()
                .is_some_and(|(_, filename)| filename != locked_filename)
        {
            debug!(
                "locked python-build-standalone artifact {locked_filename} not found for python {version} on {platform}, finding latest"
            );
        }
        Ok(result)
    }

    fn github_attestations_enabled() -> bool {
        let settings = Settings::get();
        settings
            .python
            .github_attestations
            .unwrap_or(settings.github_attestations)
    }

    fn detect_precompiled_provenance(&self) -> Option<ProvenanceType> {
        // Provenance only applies to precompiled binaries, not compiled-from-source.
        // On Windows, precompiled is always used regardless of compile setting.
        let uses_precompiled = cfg!(windows) || Settings::get().python.compile != Some(true);
        if !uses_precompiled || !Self::github_attestations_enabled() {
            return None;
        }
        Some(ProvenanceType::GithubAttestations)
    }

    fn has_precompiled_lockfile_integrity(tv: &ToolVersion, platform_key: &str) -> bool {
        tv.lock_platforms
            .get(platform_key)
            .is_some_and(|pi| pi.checksum.is_some() && pi.provenance.is_some())
    }

    fn ensure_precompiled_provenance_setting_enabled(
        tv: &ToolVersion,
        platform_key: &str,
    ) -> Result<()> {
        crate::backend::ensure_provenance_setting_enabled(tv, platform_key, |provenance| {
            match provenance {
                ProvenanceType::GithubAttestations => Ok(!Self::github_attestations_enabled()),
                _ => Err(eyre!(
                    "Lockfile has unexpected provenance type {provenance} for python tool {tv}. \
                     Update the lockfile to remove the stale provenance entry."
                )),
            }
        })
    }

    async fn verify_precompiled_provenance(
        &self,
        ctx: &InstallContext,
        tv: &mut ToolVersion,
        platform_key: &str,
        tarball_path: &std::path::Path,
    ) -> Result<()> {
        // Check lockfile provenance expectation before verification
        let locked_provenance = tv
            .lock_platforms
            .get_mut(platform_key)
            .and_then(|pi| pi.provenance.take());

        // Verify GitHub artifact attestations for precompiled binaries
        // Returns Ok(true) if verified, Ok(false) if skipped, Err if failed
        let verified = self
            .verify_github_artifact_attestations(ctx, tarball_path, &tv.version)
            .await?;

        // Record provenance only if verification actually succeeded (not skipped)
        if verified {
            let pi = tv
                .lock_platforms
                .entry(platform_key.to_string())
                .or_default();
            pi.provenance = Some(ProvenanceType::GithubAttestations);
        }

        // Enforce lockfile provenance
        if let Some(ref expected) = locked_provenance {
            let got = tv
                .lock_platforms
                .get(platform_key)
                .and_then(|pi| pi.provenance.as_ref());
            if got.is_none_or(|g| g != expected) {
                let got_str = got
                    .map(|g| g.to_string())
                    .unwrap_or_else(|| "no verification".to_string());
                return Err(eyre!(
                    "Lockfile requires {expected} provenance for {tv} but {got_str} was used. \
                     This may indicate a downgrade attack. Enable the corresponding verification setting \
                     or update the lockfile."
                ));
            }
        }

        Ok(())
    }

    async fn verify_github_artifact_attestations(
        &self,
        ctx: &InstallContext,
        tarball_path: &std::path::Path,
        version: &str,
    ) -> Result<bool> {
        if !Self::github_attestations_enabled() {
            debug!("GitHub artifact attestations verification disabled for Python");
            return Ok(false);
        }

        ctx.pr
            .set_message("verify GitHub artifact attestations".to_string());

        match crate::github::sigstore::verify_attestation(
            tarball_path,
            "astral-sh",
            "python-build-standalone",
            None, // Accept any workflow from repo
            None,
            true,
        )
        .await
        {
            Ok(true) => {
                ctx.pr
                    .set_message("✓ GitHub artifact attestations verified".to_string());
                debug!(
                    "GitHub artifact attestations verified successfully for python@{}",
                    version
                );
                Ok(true)
            }
            Ok(false) => Err(eyre!(
                "GitHub artifact attestations verification failed for python@{version}\n{ATTESTATION_HELP}"
            )),
            Err(crate::github::sigstore::AttestationError::NoAttestations) => Err(eyre!(
                "No GitHub artifact attestations found for python@{version}\n{ATTESTATION_HELP}"
            )),
            Err(e) => Err(eyre!(
                "GitHub artifact attestations verification failed for python@{version}: {e}\n{ATTESTATION_HELP}"
            )),
        }
    }
}

#[async_trait]
impl Backend for PythonPlugin {
    fn ba(&self) -> &Arc<BackendArg> {
        &self.ba
    }

    async fn _list_remote_versions(&self, _config: &Arc<Config>) -> eyre::Result<Vec<VersionInfo>> {
        if cfg!(windows) || Settings::get().python.compile == Some(false) {
            // python-build-standalone is CPython only, so pypy comes from its own index — and only
            // the releases that publish an archive for this platform, the way the CPython list is
            // already filtered. Offering the rest would list versions that cannot install: macOS
            // arm64 is the sharp case, with builds for 43 of the 94 indexed releases.
            let mut versions = vec![];
            let settings = Settings::get();
            if let Some((platform, arch)) = pypy_platform_arch(settings.os(), settings.arch()) {
                // A failure here must not take the CPython list down with it.
                match self.fetch_pypy_releases().await {
                    Ok(releases) => versions = pypy_version_infos(releases, platform, arch),
                    // warn, not debug: this list gets cached for the whole
                    // fetch_remote_versions_cache window, so a user would see `ls-remote` lose
                    // every pypy entry with nothing to explain it. The cpython branch below uses
                    // `?`, so only this half can cache a partial answer.
                    Err(err) => {
                        warn!("failed to fetch pypy versions, listing cpython only: {err:#}")
                    }
                }
            }
            let cpython = self
                .fetch_precompiled_remote_versions()
                .await?
                .iter()
                .map(|(v, date, _)| VersionInfo {
                    version: v.clone(),
                    created_at: python_precompiled_created_at(date),
                    ..Default::default()
                })
                .collect();
            Ok(merge_pypy_and_cpython(versions, cpython))
        } else {
            self.install_or_update_python_build(None)?;
            let python_build_bin = self.python_build_bin();
            let python_build_str = python_build_bin.to_string_lossy().to_string();
            let definition_created_at = self
                .python_build_definition_created_at()
                .inspect_err(|err| {
                    debug!("failed to get python-build definition timestamps: {err:#}")
                })
                .unwrap_or_default();
            plugins::core::run_fetch_task_with_timeout_async(async move || {
                let output = crate::cmd::cmd_read_async_inherited_env(
                    &python_build_str,
                    &["--definitions"],
                    std::iter::empty::<(&str, &std::ffi::OsStr)>(),
                )
                .await?;
                let versions = output
                    .split('\n')
                    // remove free-threaded pythons like 3.13t and 3.14t-dev
                    .filter(|s| !regex!(r"\dt(-dev)?$").is_match(s))
                    .map(|s| VersionInfo {
                        version: s.to_string(),
                        created_at: definition_created_at.get(s).cloned(),
                        ..Default::default()
                    })
                    .sorted_by_cached_key(|v| python_version_sort_key(&v.version))
                    .collect();
                Ok(versions)
            })
            .await
        }
    }

    /// Python versions follow PEP 440, so `3.15.0a8`-style separator-less
    /// alpha suffixes are pre-releases that the shared filter wouldn't catch
    /// on its own. See `fuzzy_match_versions_pep440`.
    fn fuzzy_match_filter(
        &self,
        versions: Vec<String>,
        query: &str,
        filter_prereleases: bool,
    ) -> Vec<String> {
        crate::backend::fuzzy_match_versions_pep440(versions, query, filter_prereleases)
    }

    async fn security_info(&self) -> Vec<crate::backend::SecurityFeature> {
        use crate::backend::SecurityFeature;

        let mut features = vec![SecurityFeature::Checksum {
            algorithm: Some("sha256".to_string()),
        }];

        if self.detect_precompiled_provenance().is_some() {
            features.push(SecurityFeature::GithubAttestations {
                signer_workflow: None,
            });
        }

        features
    }

    async fn install_version_(
        &self,
        ctx: &InstallContext,
        mut tv: ToolVersion,
    ) -> Result<ToolVersion> {
        let settings = Settings::get();
        if cfg!(windows) || settings.python.compile != Some(true) {
            validate_python_precompiled_settings(&settings)?;
            self.install_precompiled(ctx, &mut tv).await?;
        } else {
            settings.warn_nixos_python_compile_default();
            self.install_compiled(ctx, &tv).await?;
        }
        self.test_python(&ctx.config, &tv, ctx.pr.as_ref()).await?;
        if let Err(e) = self.get_virtualenv(&ctx.config, &tv).await {
            warn!("failed to get virtualenv: {e:#}");
        }
        if let Some(default_file) = &Settings::get().python.default_packages_file {
            let default_file = file::replace_path(default_file);
            if let Err(err) = self
                .install_default_packages(&ctx.config, &default_file, &tv, ctx.pr.as_ref())
                .await
            {
                warn!("failed to install default python packages: {err:#}");
            }
        }
        Ok(tv)
    }

    #[cfg(windows)]
    async fn list_bin_paths(
        &self,
        _config: &Arc<Config>,
        tv: &ToolVersion,
    ) -> eyre::Result<Vec<PathBuf>> {
        // The install root holds python.exe/python3.exe and the synthesized
        // pip wrappers; Scripts is where pip installs console-script
        // launchers (black.exe, ...). Root stays first so interpreter/pip
        // resolution is stable. Scripts is returned unconditionally per the
        // trait contract (candidates, not existing dirs — see
        // Backend::list_bin_paths docs); it may not exist until the first
        // `pip install`.
        Ok(vec![tv.install_path(), tv.install_path().join("Scripts")])
    }

    async fn exec_env(
        &self,
        config: &Arc<Config>,
        _ts: &Toolset,
        tv: &ToolVersion,
    ) -> eyre::Result<BTreeMap<String, String>> {
        let mut hm = BTreeMap::new();
        match self.get_virtualenv(config, tv).await {
            Err(e) => warn!("failed to get virtualenv: {e}"),
            Ok(Some(virtualenv)) => {
                // Windows venvs place executables in Scripts, not bin (same
                // handling as the `_.python.venv` env directive)
                let bin = virtualenv.join(if cfg!(windows) { "Scripts" } else { "bin" });
                hm.insert("VIRTUAL_ENV".into(), virtualenv.to_string_lossy().into());
                hm.insert("MISE_ADD_PATH".into(), bin.to_string_lossy().into());
            }
            Ok(None) => {}
        };
        Ok(hm)
    }

    fn get_remote_version_cache(&self) -> Arc<Mutex<VersionCacheManager>> {
        static CACHE: OnceLock<Arc<Mutex<VersionCacheManager>>> = OnceLock::new();
        CACHE
            .get_or_init(|| {
                Arc::new(Mutex::new(
                    CacheManagerBuilder::new(
                        self.ba().cache_path.join("remote_versions.msgpack.z"),
                    )
                    .with_fresh_duration(Settings::get().fetch_remote_versions_cache())
                    .with_cache_key((Settings::get().python.compile == Some(false)).to_string())
                    .build(),
                ))
            })
            .clone()
    }

    fn resolve_lockfile_options(
        &self,
        request: &ToolRequest,
        target: &PlatformTarget,
    ) -> Result<BTreeMap<String, String>> {
        let mut opts = BTreeMap::new();
        let settings = Settings::get();
        let is_current_platform = target.is_current();

        // Only include compile option if true (non-default)
        let compile = if is_current_platform {
            settings.python.compile.unwrap_or(false)
        } else {
            false
        };
        if compile {
            opts.insert("compile".to_string(), "true".to_string());
        }

        // Include precompiled options for all platforms to avoid splitting
        // lockfile entries between host and non-host platforms (#8390)
        if !compile {
            if let Some(arch) = settings.python.precompiled_arch.clone() {
                opts.insert("precompiled_arch".to_string(), arch);
            }
            if let Some(os) = settings.python.precompiled_os.clone() {
                opts.insert("precompiled_os".to_string(), os);
            }
            if let Some(flavor) = settings.python.precompiled_flavor.clone() {
                opts.insert("precompiled_flavor".to_string(), flavor);
            }
        }

        let raw_opts = request.options();
        opts.extend(PythonOptions::new(&raw_opts).lockfile_options());
        Ok(opts)
    }

    async fn resolve_lock_info(
        &self,
        tv: &ToolVersion,
        target: &PlatformTarget,
    ) -> Result<PlatformInfo> {
        let version = &tv.version;
        if version.starts_with("pypy") {
            return self.resolve_pypy_lock_info(version, target).await;
        }
        let locked_filename = tv
            .lock_platforms
            .get(&target.to_key())
            .and_then(|info| info.url.as_deref())
            .and_then(|url| python_precompiled_filename_from_url(url, version));

        // Look up the precompiled release for this version and target platform
        let Some((tag, filename)) = self
            .fetch_precompiled_for_target(version, target, locked_filename)
            .await?
        else {
            return Ok(PlatformInfo::default());
        };

        let url = format!(
            "https://github.com/astral-sh/python-build-standalone/releases/download/{tag}/{filename}"
        );

        // Fetch SHA256SUMS from the release to get the checksum
        let shasums_url = format!(
            "https://github.com/astral-sh/python-build-standalone/releases/download/{tag}/SHA256SUMS"
        );
        let checksum = fetch_checksum_from_shasums(&shasums_url, &filename).await;

        // Detect provenance for precompiled binaries
        let provenance = self.detect_precompiled_provenance();

        Ok(PlatformInfo {
            url: Some(url),
            checksum,
            provenance,
            ..Default::default()
        })
    }
}

fn parse_python_build_definition_created_at(output: &str) -> BTreeMap<String, String> {
    let mut created_at = BTreeMap::new();
    let mut current_timestamp = None;
    for line in output.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if !line.starts_with("plugins/") && crate::duration::parse_into_timestamp(line).is_ok() {
            current_timestamp = Some(line.to_string());
            continue;
        }
        if let Some(version) = line.strip_prefix("plugins/python-build/share/python-build/")
            && !version.contains('/')
            && let Some(timestamp) = &current_timestamp
        {
            created_at
                .entry(version.to_string())
                .or_insert_with(|| timestamp.clone());
        }
    }
    created_at
}

fn python_precompiled_created_at(date: &str) -> Option<String> {
    if date.len() != 8 || !date.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(format!(
        "{}-{}-{}T00:00:00Z",
        &date[..4],
        &date[4..6],
        &date[6..]
    ))
}

fn python_precompiled_filename_from_url<'a>(url: &'a str, version: &str) -> Option<&'a str> {
    let (release, filename) = url
        .strip_prefix(PBS_RELEASE_DOWNLOAD_URL)?
        .split_once('/')?;
    if release.len() != 8 || !release.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    filename
        .starts_with(&format!("cpython-{version}+{release}-"))
        .then_some(filename)
}

fn select_python_precompiled(
    manifest: &str,
    version: &str,
    platform: &str,
    flavor: Option<&str>,
    locked_filename: Option<&str>,
) -> Option<(String, String)> {
    let flavor = flavor.map(str::to_string);
    let candidates = manifest
        .lines()
        .filter(|line| line.contains(platform))
        .flat_map(|line| {
            regex!(r"^cpython-(\d+\.\d+\.[\da-z]+)\+(\d+).*")
                .captures(line)
                .map(|captures| {
                    (
                        captures[1].to_string(),
                        captures[2].to_string(),
                        captures[0].to_string(),
                    )
                })
        })
        .filter(|(candidate, _, _)| candidate == version)
        .collect_vec();
    let select = |filename: Option<&str>| {
        candidates
            .iter()
            .filter(|(_, _, candidate)| {
                filename.map_or_else(
                    || filter_freethreaded(candidate, &flavor),
                    |filename| candidate == filename,
                )
            })
            .min_by_key(|(_, date, name)| {
                let install_type = if let Some(flavor) = flavor.as_deref() {
                    let name_without_ext = name.trim_end_matches(".tar.gz");
                    usize::from(!name_without_ext.ends_with(flavor))
                } else if name.contains("install_only_stripped") {
                    0
                } else if name.contains("install_only") {
                    1
                } else {
                    2
                };
                let date = date.parse::<i64>().unwrap_or_default();
                (install_type, -date)
            })
            .map(|(_, release, filename)| (release.clone(), filename.clone()))
    };
    select(locked_filename).or_else(|| select(None))
}

fn python_precompiled_url_path(settings: &Settings) -> String {
    if cfg!(windows) || cfg!(linux) || cfg!(macos) {
        format!(
            "python-precompiled-{}-{}.gz",
            python_arch(settings),
            python_os(settings)
        )
    } else {
        "python-precompiled.gz".into()
    }
}

fn validate_python_precompiled_settings(settings: &Settings) -> Result<()> {
    if let Some(arch) = &settings.python.precompiled_arch
        && let Some((precompiled_arch, precompiled_os)) = split_python_precompiled_triple(arch)
    {
        bail!(
            "invalid python.precompiled_arch={arch:?}: this looks like a target triple. \
             Set python.precompiled_arch={precompiled_arch:?} and \
             python.precompiled_os={precompiled_os:?} instead."
        );
    }
    if let Some(arch) = &settings.python.precompiled_arch
        && looks_like_python_precompiled_os_value(arch)
    {
        bail!(
            "invalid python.precompiled_arch={arch:?}: this looks like an OS value. \
             Use python.precompiled_os={arch:?} instead, and set python.precompiled_arch \
             to an architecture such as \"x86_64\" or \"aarch64\"."
        );
    }
    Ok(())
}

fn split_python_precompiled_triple(value: &str) -> Option<(&str, &str)> {
    ["-unknown-linux", "-apple-darwin", "-pc-windows"]
        .iter()
        .find_map(|os_marker| {
            let index = value.find(os_marker)?;
            (index > 0).then(|| (&value[..index], &value[index + 1..]))
        })
}

fn looks_like_python_precompiled_os_value(value: &str) -> bool {
    value.contains("unknown-linux")
        || value.contains("apple-darwin")
        || value.contains("pc-windows")
}

fn python_os(settings: &Settings) -> String {
    if let Some(os) = &settings.python.precompiled_os {
        return os.clone();
    }
    if cfg!(windows) {
        "pc-windows-msvc".into()
    } else if cfg!(target_os = "macos") {
        "apple-darwin".into()
    } else {
        let current = Platform::current();
        let libc = current.libc().unwrap_or("gnu");
        ["unknown", built_info::CFG_OS, libc]
            .iter()
            .filter(|s| !s.is_empty())
            .join("-")
    }
}

fn python_arch(settings: &Settings) -> &str {
    if let Some(arch) = &settings.python.precompiled_arch {
        return arch.as_str();
    }
    let arch = settings.arch();
    resolve_python_arch(std::env::consts::OS, arch)
}

fn resolve_python_arch<'a>(os: &str, arch: &'a str) -> &'a str {
    let arch = match arch {
        "x64" => "x86_64",
        "arm64" => "aarch64",
        other => other,
    };
    if os == "windows" && arch != "aarch64" {
        "x86_64"
    } else if os == "linux" && arch == "x86_64" {
        if cfg!(target_feature = "avx512f") {
            "x86_64_v4"
        } else if cfg!(target_feature = "avx2") {
            "x86_64_v3"
        } else if cfg!(target_feature = "sse4.1") {
            "x86_64_v2"
        } else {
            "x86_64"
        }
    } else {
        arch
    }
}

fn python_precompiled_platform() -> String {
    let settings = Settings::get();
    let os = python_os(&settings);
    let arch = python_arch(&settings);
    if let Some(flavor) = &settings.python.precompiled_flavor {
        format!("{arch}-{os}-{flavor}")
    } else {
        format!("{arch}-{os}")
    }
}

/// Map a PlatformTarget OS to the python-build-standalone OS string.
fn python_os_for_target(target: &PlatformTarget) -> String {
    match target.os_name() {
        "macos" => "apple-darwin".to_string(),
        "windows" => "pc-windows-msvc".to_string(),
        _ => format!("unknown-linux-{}", target.libc().unwrap_or("gnu")),
    }
}

/// Map a PlatformTarget arch to the python-build-standalone arch string.
fn python_arch_for_target(target: &PlatformTarget) -> &'static str {
    match target.arch_name() {
        "arm64" => "aarch64",
        _ => "x86_64",
    }
}

/// Spell a PyPy release the way python-build's definitions do, e.g. `pypy3.10-7.3.17`, so the
/// same version string resolves whichever install path is taken.
///
/// `None` for the `nightly` entries: they point at rolling `pypy-c-jit-latest-*` artifacts on
/// buildbot, so the same version string would name different bytes on every install and the
/// recorded checksum would go stale immediately. python-build ships no nightly definitions either.
fn pypy_version_str(release: &PypyRelease) -> Option<String> {
    if !release
        .pypy_version
        .starts_with(|c: char| c.is_ascii_digit())
    {
        return None;
    }
    let mut parts = release.python_version.split('.');
    let major = parts.next()?;
    let minor = parts.next()?;
    if major.is_empty() || minor.is_empty() {
        return None;
    }
    Some(format!("pypy{major}.{minor}-{}", release.pypy_version))
}

/// Put the two halves of the python list together: pypy first, CPython after.
///
/// `python@latest` takes the tail, so CPython has to end up there. Concatenation rather than a
/// sort: each half keeps the order its own source published, and nothing here compares a pypy
/// version string against a CPython one — the two sources make no promise those are comparable.
fn merge_pypy_and_cpython(pypy: Vec<VersionInfo>, cpython: Vec<VersionInfo>) -> Vec<VersionInfo> {
    pypy.into_iter().chain(cpython).collect()
}

/// The `(platform, arch)` pair `versions.json` uses for the host, or `None` where PyPy publishes
/// no build — notably Windows on arm64.
fn pypy_platform_arch(os: &str, arch: &str) -> Option<(&'static str, &'static str)> {
    match (os, arch) {
        ("linux", "x64" | "x86_64") => Some(("linux", "x64")),
        ("linux", "arm64" | "aarch64") => Some(("linux", "aarch64")),
        ("macos", "x64" | "x86_64") => Some(("darwin", "x64")),
        ("macos", "arm64" | "aarch64") => Some(("darwin", "arm64")),
        ("windows", "x64" | "x86_64") => Some(("win64", "x64")),
        _ => None,
    }
}

/// Turn the PyPy index into version entries for `ls-remote` on `platform`/`arch`.
///
/// Releases with no archive for that pair are dropped: PyPy's platform coverage is uneven — 3.6 and
/// 3.7 have no macOS arm64 build at all — and listing one would let a bare `pypy3.7` resolve to a
/// version that then fails at download.
///
/// The order is PyPy's own: `versions.json` is newest-first, so reversing it gives oldest-first,
/// and a bare `pypy3.10` — which takes the last prefix match — lands on whatever upstream lists
/// as its most recent 3.10 build. Nothing here parses or compares the version strings; PyPy's
/// `7.3.4rc2` and friends are exactly the shapes a semver comparator gets wrong.
fn pypy_version_infos(releases: &[PypyRelease], platform: &str, arch: &str) -> Vec<VersionInfo> {
    releases
        .iter()
        .rev()
        .filter(|r| pypy_file_for(r, platform, arch).is_some())
        .filter_map(|r| {
            Some(VersionInfo {
                version: pypy_version_str(r)?,
                created_at: pypy_created_at(r.date.as_deref()),
                ..Default::default()
            })
        })
        // versions.json repeats a few releases verbatim
        .unique_by(|v| v.version.clone())
        .collect()
}

/// `versions.json` dates are plain `YYYY-MM-DD`; `created_at` wants a timestamp.
fn pypy_created_at(date: Option<&str>) -> Option<String> {
    let date = date?;
    if date.len() != 10 {
        return None;
    }
    Some(format!("{date}T00:00:00Z"))
}

fn pypy_file_for<'a>(release: &'a PypyRelease, platform: &str, arch: &str) -> Option<&'a PypyFile> {
    release
        .files
        .iter()
        .find(|f| f.platform == platform && f.arch == arch)
}

fn ensure_not_windows() -> eyre::Result<()> {
    if cfg!(windows) {
        bail!(
            "python cannot currently be compiled on windows with core:python, use vfox:python instead"
        );
    }
    Ok(())
}

fn filter_freethreaded(v: &str, flavor: &Option<String>) -> bool {
    flavor.as_ref().is_some_and(|f| f.contains("freethreaded")) || !v.contains("freethreaded")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts_with(key: &str, value: &str) -> ToolVersionOptions {
        opts_with_value(key, toml::Value::String(value.to_string()))
    }

    fn opts_with_value(key: &str, value: toml::Value) -> ToolVersionOptions {
        let mut opts = ToolVersionOptions::default();
        opts.opts.insert(key.to_string(), value);
        opts
    }

    /// Two entries in the shape `https://downloads.python.org/pypy/versions.json` publishes.
    const PYPY_VERSIONS_FIXTURE: &str = r#"[
      {
        "pypy_version": "7.3.17",
        "python_version": "3.10.14",
        "stable": true,
        "date": "2024-08-28",
        "files": [
          {"filename": "pypy3.10-v7.3.17-linux64.tar.bz2", "arch": "x64", "platform": "linux",
           "download_url": "https://downloads.python.org/pypy/pypy3.10-v7.3.17-linux64.tar.bz2"},
          {"filename": "pypy3.10-v7.3.17-aarch64.tar.bz2", "arch": "aarch64", "platform": "linux",
           "download_url": "https://downloads.python.org/pypy/pypy3.10-v7.3.17-aarch64.tar.bz2"},
          {"filename": "pypy3.10-v7.3.17-macos_arm64.tar.bz2", "arch": "arm64", "platform": "darwin",
           "download_url": "https://downloads.python.org/pypy/pypy3.10-v7.3.17-macos_arm64.tar.bz2"},
          {"filename": "pypy3.10-v7.3.17-win64.zip", "arch": "x64", "platform": "win64",
           "download_url": "https://downloads.python.org/pypy/pypy3.10-v7.3.17-win64.zip"}
        ]
      },
      {
        "pypy_version": "7.3.23",
        "python_version": "2.7.18",
        "stable": true,
        "date": "2026-05-01",
        "files": [
          {"filename": "pypy2.7-v7.3.23-linux64.tar.bz2", "arch": "x64", "platform": "linux",
           "download_url": "https://downloads.python.org/pypy/pypy2.7-v7.3.23-linux64.tar.bz2"}
        ]
      },
      {
        "pypy_version": "nightly",
        "python_version": "3.10",
        "files": [
          {"filename": "pypy-c-jit-latest-linux64.tar.bz2", "arch": "x64", "platform": "linux",
           "download_url": "https://buildbot.pypy.org/nightly/py3.10/pypy-c-jit-latest-linux64.tar.bz2"}
        ]
      }
    ]"#;

    fn pypy_fixture() -> Vec<PypyRelease> {
        serde_json::from_str(PYPY_VERSIONS_FIXTURE).unwrap()
    }

    /// The nightly entry is dropped: its `download_url` is a rolling `pypy-c-jit-latest-*`
    /// artifact, so a pinned version + recorded checksum could never stay true.
    #[test]
    fn pypy_versions_are_spelled_like_python_build_definitions() {
        let releases = pypy_fixture();
        assert_eq!(releases.len(), 3);
        assert_eq!(
            releases.iter().filter_map(pypy_version_str).collect_vec(),
            vec!["pypy3.10-7.3.17", "pypy2.7-7.3.23"]
        );
    }

    #[test]
    fn pypy_platform_arch_covers_what_pypy_publishes() {
        assert_eq!(pypy_platform_arch("linux", "x64"), Some(("linux", "x64")));
        assert_eq!(
            pypy_platform_arch("linux", "arm64"),
            Some(("linux", "aarch64"))
        );
        assert_eq!(pypy_platform_arch("macos", "x64"), Some(("darwin", "x64")));
        assert_eq!(
            pypy_platform_arch("macos", "arm64"),
            Some(("darwin", "arm64"))
        );
        assert_eq!(pypy_platform_arch("windows", "x64"), Some(("win64", "x64")));
        // pypy ships no windows arm64 build
        assert_eq!(pypy_platform_arch("windows", "arm64"), None);
    }

    #[test]
    fn pypy_file_lookup_picks_the_host_archive() {
        let releases = pypy_fixture();
        let release = &releases[0];
        assert_eq!(
            pypy_file_for(release, "win64", "x64").map(|f| f.filename.as_str()),
            Some("pypy3.10-v7.3.17-win64.zip")
        );
        assert_eq!(
            pypy_file_for(release, "darwin", "arm64").map(|f| f.filename.as_str()),
            Some("pypy3.10-v7.3.17-macos_arm64.tar.bz2")
        );
        assert!(pypy_file_for(release, "win64", "aarch64").is_none());
        // the 2.7 release publishes linux only
        assert!(pypy_file_for(&releases[1], "win64", "x64").is_none());
    }

    #[test]
    fn pypy_created_at_becomes_a_timestamp() {
        assert_eq!(
            pypy_created_at(Some("2024-08-28")).as_deref(),
            Some("2024-08-28T00:00:00Z")
        );
        assert_eq!(pypy_created_at(None), None);
        assert_eq!(pypy_created_at(Some("2024-08")), None);
    }

    /// `versions.json` repeats a handful of releases verbatim (`7.3.6rc1` appears twice for each
    /// of 2.7/3.7/3.8), which would otherwise show up twice in `ls-remote`.
    #[test]
    fn pypy_version_infos_dedupes_and_dates() {
        let mut releases = pypy_fixture();
        let dupe = releases[0].clone();
        releases.push(dupe);
        let infos = pypy_version_infos(&releases, "linux", "x64");
        // the fixture is index order, so the list is that reversed: the 3.10 release is last in
        // the input (as the duplicate) and therefore first here
        assert_eq!(
            infos.iter().map(|v| v.version.as_str()).collect_vec(),
            vec!["pypy3.10-7.3.17", "pypy2.7-7.3.23"]
        );
        assert_eq!(infos[0].created_at.as_deref(), Some("2024-08-28T00:00:00Z"));
    }

    /// The list is upstream's own order reversed, with nothing parsing the version strings.
    /// `versions.json` is newest-first, and a bare `pypy3.10` takes the last prefix match, so the
    /// newest 3.10 has to end up last among the 3.10 entries.
    #[test]
    fn pypy_version_infos_follow_the_index_order_reversed() {
        let json = r#"[
          {"pypy_version": "7.3.19", "python_version": "3.10.16", "date": "2025-02-26",
           "files": [{"filename": "a", "arch": "x64", "platform": "linux", "download_url": "u"}]},
          {"pypy_version": "7.3.17", "python_version": "3.10.14", "date": "2024-08-28",
           "files": [{"filename": "b", "arch": "x64", "platform": "linux", "download_url": "u"}]},
          {"pypy_version": "7.3.4rc2", "python_version": "2.7.18", "date": "2021-04-01",
           "files": [{"filename": "c", "arch": "x64", "platform": "linux", "download_url": "u"}]},
          {"pypy_version": "7.3.4", "python_version": "2.7.18", "date": "2021-04-04",
           "files": [{"filename": "d", "arch": "x64", "platform": "linux", "download_url": "u"}]}
        ]"#;
        let releases: Vec<PypyRelease> = serde_json::from_str(json).unwrap();
        let listed = pypy_version_infos(&releases, "linux", "x64")
            .into_iter()
            .map(|v| v.version)
            .collect_vec();

        // exactly the input read backwards — note 7.3.4 sits *before* 7.3.4rc2 upstream, and that
        // is preserved rather than "corrected" by comparing the strings
        assert_eq!(
            listed,
            vec![
                "pypy2.7-7.3.4",
                "pypy2.7-7.3.4rc2",
                "pypy3.10-7.3.17",
                "pypy3.10-7.3.19"
            ]
        );
        // a bare `pypy3.10` takes the last prefix match
        assert_eq!(
            listed.iter().rfind(|v| v.starts_with("pypy3.10-")).unwrap(),
            "pypy3.10-7.3.19"
        );
    }

    /// PyPy's platform coverage is uneven, so a version listed on one host may have no archive on
    /// another. Listing it there would let a bare `pypy2.7` resolve to something uninstallable.
    #[test]
    fn pypy_version_infos_drops_releases_with_no_archive_for_the_platform() {
        let releases = pypy_fixture();
        let listed = |platform, arch| {
            pypy_version_infos(&releases, platform, arch)
                .into_iter()
                .map(|v| v.version)
                .collect_vec()
        };
        // the 2.7 fixture release publishes linux/x64 only
        assert_eq!(listed("win64", "x64"), vec!["pypy3.10-7.3.17"]);
        assert_eq!(listed("linux", "aarch64"), vec!["pypy3.10-7.3.17"]);
        assert_eq!(
            listed("linux", "x64"),
            vec!["pypy2.7-7.3.23", "pypy3.10-7.3.17"]
        );
        // neither fixture release publishes darwin/x64
        assert!(listed("darwin", "x64").is_empty());
        // and the order is the fixture's, reversed — 2.7 sits after 3.10 in the input
        assert_eq!(
            pypy_fixture()
                .iter()
                .filter_map(pypy_version_str)
                .collect_vec(),
            vec!["pypy3.10-7.3.17", "pypy2.7-7.3.23"]
        );
    }

    /// `python@latest` resolves against the end of the list, so the merged list puts pypy first
    /// and CPython after — by concatenation, without comparing any two version strings.
    #[test]
    fn merged_list_keeps_cpython_last() {
        let vi = |v: &str| VersionInfo {
            version: v.to_string(),
            ..Default::default()
        };
        let pypy = vec![vi("pypy2.7-7.3.23"), vi("pypy3.10-7.3.17")];
        let cpython = vec![vi("3.12.0"), vi("3.13.1")];

        let merged = merge_pypy_and_cpython(pypy.clone(), cpython)
            .into_iter()
            .map(|v| v.version)
            .collect_vec();

        assert_eq!(merged.last().unwrap(), "3.13.1");
        assert!(merged[..pypy.len()].iter().all(|v| v.starts_with("pypy")));
        assert!(merged[pypy.len()..].iter().all(|v| !v.starts_with("pypy")));
    }

    #[test]
    fn python_options_reads_patch_sysconfig() {
        assert!(PythonOptions::new(&ToolVersionOptions::default()).patch_sysconfig());
        assert!(!PythonOptions::new(&opts_with("patch_sysconfig", "false")).patch_sysconfig());
        assert!(!PythonOptions::new(&opts_with("patch_sysconfig", "FALSE")).patch_sysconfig());
        assert!(!PythonOptions::new(&opts_with("patch_sysconfig", "0")).patch_sysconfig());
        assert!(
            !PythonOptions::new(&opts_with_value(
                "patch_sysconfig",
                toml::Value::Boolean(false)
            ))
            .patch_sysconfig()
        );
        assert!(PythonOptions::new(&opts_with("patch_sysconfig", "1")).patch_sysconfig());
        assert!(PythonOptions::new(&opts_with("patch_sysconfig", "00")).patch_sysconfig());
    }

    #[test]
    fn python_options_reads_virtualenv() {
        let opts = opts_with("virtualenv", ".venv");
        assert_eq!(PythonOptions::new(&opts).virtualenv(), Some(".venv"));
    }

    #[test]
    fn python_lockfile_options_include_patch_sysconfig_but_not_virtualenv() {
        let mut opts = opts_with("patch_sysconfig", "false");
        opts.opts.insert(
            "virtualenv".to_string(),
            toml::Value::String(".venv".to_string()),
        );

        assert_eq!(
            PythonOptions::new(&opts).lockfile_options(),
            BTreeMap::from([("patch_sysconfig".to_string(), "false".to_string())])
        );

        let opts = opts_with("patch_sysconfig", "true");
        assert!(PythonOptions::new(&opts).lockfile_options().is_empty());
    }

    #[test]
    fn parses_python_build_definition_created_at() {
        let output = "\
2026-06-11T12:34:56+00:00
plugins/python-build/share/python-build/3.14.6
plugins/python-build/share/python-build/3.13.8

2026-06-01T01:02:03+00:00
plugins/python-build/share/python-build/3.14.5
plugins/python-build/share/python-build/patches/3.14.5/foo.patch
";

        assert_eq!(
            parse_python_build_definition_created_at(output),
            BTreeMap::from([
                (
                    "3.13.8".to_string(),
                    "2026-06-11T12:34:56+00:00".to_string()
                ),
                (
                    "3.14.5".to_string(),
                    "2026-06-01T01:02:03+00:00".to_string()
                ),
                (
                    "3.14.6".to_string(),
                    "2026-06-11T12:34:56+00:00".to_string()
                ),
            ])
        );
    }

    #[test]
    fn parses_python_precompiled_created_at() {
        assert_eq!(
            python_precompiled_created_at("20260611").as_deref(),
            Some("2026-06-11T00:00:00Z")
        );
        assert_eq!(python_precompiled_created_at("2026-06-11"), None);
        assert_eq!(python_precompiled_created_at("notadate"), None);
    }

    #[test]
    fn parses_python_precompiled_filename_from_url() {
        let url = "https://github.com/astral-sh/python-build-standalone/releases/download/20260728/cpython-3.12.13+20260728-x86_64-unknown-linux-gnu-install_only_stripped.tar.gz";
        assert_eq!(
            python_precompiled_filename_from_url(url, "3.12.13"),
            Some("cpython-3.12.13+20260728-x86_64-unknown-linux-gnu-install_only_stripped.tar.gz")
        );
        assert_eq!(python_precompiled_filename_from_url(url, "3.12.12"), None);
        assert_eq!(
            python_precompiled_filename_from_url(
                "https://example.com/releases/download/20260728/cpython-3.12.13+20260728.tar.gz",
                "3.12.13"
            ),
            None
        );
    }

    #[test]
    fn selects_locked_python_precompiled_release() {
        let manifest = "\
cpython-3.12.13+20260728-x86_64-unknown-linux-gnu-install_only.tar.gz
cpython-3.12.13+20260728-x86_64-unknown-linux-gnu-install_only_stripped.tar.gz
cpython-3.12.13+20260728-x86_64-unknown-linux-gnu-install_only_stripped+freethreaded.tar.gz
cpython-3.12.13+20260805-x86_64-unknown-linux-gnu-install_only_stripped.tar.gz
";
        let platform = "x86_64-unknown-linux-gnu";

        assert_eq!(
            select_python_precompiled(
                manifest,
                "3.12.13",
                platform,
                None,
                Some("cpython-3.12.13+20260728-x86_64-unknown-linux-gnu-install_only.tar.gz")
            ),
            Some((
                "20260728".to_string(),
                "cpython-3.12.13+20260728-x86_64-unknown-linux-gnu-install_only.tar.gz".to_string()
            ))
        );
        assert_eq!(
            select_python_precompiled(manifest, "3.12.13", platform, None, None),
            Some((
                "20260805".to_string(),
                "cpython-3.12.13+20260805-x86_64-unknown-linux-gnu-install_only_stripped.tar.gz"
                    .to_string()
            ))
        );
        assert_eq!(
            select_python_precompiled(
                manifest,
                "3.12.13",
                platform,
                None,
                Some(
                    "cpython-3.12.13+20260728-x86_64-unknown-linux-gnu-install_only_stripped+freethreaded.tar.gz"
                )
            ),
            Some((
                "20260728".to_string(),
                "cpython-3.12.13+20260728-x86_64-unknown-linux-gnu-install_only_stripped+freethreaded.tar.gz"
                    .to_string()
            ))
        );
        assert_eq!(
            select_python_precompiled(
                manifest,
                "3.12.13",
                platform,
                None,
                Some(
                    "cpython-3.12.13+20250101-x86_64-unknown-linux-gnu-install_only_stripped.tar.gz"
                )
            ),
            Some((
                "20260805".to_string(),
                "cpython-3.12.13+20260805-x86_64-unknown-linux-gnu-install_only_stripped.tar.gz"
                    .to_string()
            ))
        );
    }

    #[test]
    fn test_validate_python_precompiled_settings_rejects_os_as_arch() {
        let mut settings = Settings::default();
        settings.python.precompiled_arch = Some("unknown-linux-musl".to_string());

        let err = validate_python_precompiled_settings(&settings).unwrap_err();
        assert!(err.to_string().contains("python.precompiled_os"));
    }

    #[test]
    fn test_validate_python_precompiled_settings_rejects_triple_as_arch() {
        let mut settings = Settings::default();
        settings.python.precompiled_arch = Some("x86_64-unknown-linux-musl".to_string());

        let err = validate_python_precompiled_settings(&settings).unwrap_err();
        let err = err.to_string();
        assert!(err.contains("python.precompiled_arch=\"x86_64\""));
        assert!(err.contains("python.precompiled_os=\"unknown-linux-musl\""));
    }

    #[test]
    fn test_validate_python_precompiled_settings_accepts_arch() {
        let mut settings = Settings::default();
        settings.python.precompiled_arch = Some("x86_64".to_string());

        validate_python_precompiled_settings(&settings).unwrap();
    }

    #[test]
    fn test_resolve_python_arch_windows_x64() {
        assert_eq!(resolve_python_arch("windows", "x64"), "x86_64");
        assert_eq!(resolve_python_arch("windows", "x86_64"), "x86_64");
    }

    #[test]
    fn test_resolve_python_arch_windows_arm64() {
        assert_eq!(resolve_python_arch("windows", "arm64"), "aarch64");
        assert_eq!(resolve_python_arch("windows", "aarch64"), "aarch64");
    }

    #[test]
    fn test_resolve_python_arch_linux_x64() {
        // Exact variant depends on CPU features at compile time,
        // but it should always start with "x86_64"
        assert!(resolve_python_arch("linux", "x64").starts_with("x86_64"));
    }

    #[test]
    fn test_resolve_python_arch_linux_arm64() {
        assert_eq!(resolve_python_arch("linux", "arm64"), "aarch64");
    }

    #[test]
    fn test_resolve_python_arch_macos() {
        assert_eq!(resolve_python_arch("macos", "arm64"), "aarch64");
        assert_eq!(resolve_python_arch("macos", "x64"), "x86_64");
    }

    #[test]
    fn test_python_os_for_target_linux_libc() {
        use crate::backend::platform_target::PlatformTarget;
        use crate::platform::Platform;

        let target = PlatformTarget::new(Platform::parse("linux-x64").unwrap());
        assert_eq!(python_os_for_target(&target), "unknown-linux-gnu");

        let target = PlatformTarget::new(Platform::parse("linux-x64-musl").unwrap());
        assert_eq!(python_os_for_target(&target), "unknown-linux-musl");
    }
}
