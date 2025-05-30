use crate::backend::{Backend, VersionCacheManager};
use crate::build_time::built_info;
use crate::cache::{CacheManager, CacheManagerBuilder};
use crate::cli::args::BackendArg;
use crate::cmd::CmdLineRunner;
use crate::config::{Config, Settings};
use crate::file::{TarOptions, display_path};
use crate::git::{CloneOptions, Git};
use crate::http::{HTTP, HTTP_FETCH};
use crate::install_context::InstallContext;
use crate::toolset::{ToolRequest, ToolVersion, Toolset};
use crate::ui::progress_report::SingleReport;
use crate::{Result, lock_file::LockFile};
use crate::{cmd, dirs, file, plugins, sysconfig};
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

#[derive(Debug)]
pub struct PythonPlugin {
    ba: Arc<BackendArg>,
}

pub fn python_path(tv: &ToolVersion) -> PathBuf {
    if cfg!(windows) {
        tv.install_path().join("python.exe")
    } else {
        tv.install_path().join("bin/python")
    }
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
        let pr = ctx.map(|ctx| &ctx.pr);
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
        let git = Git::new(self.python_build_path());
        plugins::core::run_fetch_task_with_timeout(move || git.update(None))?;
        Ok(())
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
                let rsp = match settings.paranoid {
                    true => {
                        HTTP_FETCH
                            .get_bytes(format!("https://mise-versions.jdx.dev/{url_path}"))
                            .await
                    }
                    // using http is not a security concern and enabling tls makes mise significantly slower
                    false => {
                        HTTP_FETCH
                            .get_bytes(format!("http://mise-versions.jdx.dev/{url_path}"))
                            .await
                    }
                }?;
                let mut decoder = GzDecoder::new(rsp.as_ref());
                let mut raw = String::new();
                decoder.read_to_string(&mut raw)?;
                let platform = python_precompiled_platform();
                // order by version, whether it is a release candidate, date, and in the preferred order of install types
                let rank = |v: &str, date: &str, name: &str| {
                    let rc = if regex!(r"rc\d+$").is_match(v) { 0 } else { 1 };
                    let v = Versioning::new(v);
                    let date = date.parse::<i64>().unwrap_or_default();
                    let install_type = if name.contains("install_only_stripped") {
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

    async fn install_precompiled(
        &self,
        ctx: &InstallContext,
        tv: &ToolVersion,
    ) -> eyre::Result<()> {
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
                        "no precompiled python found for {}, force mise to use a precompiled version with `mise settings set python.compile false`",
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

        let url = format!(
            "https://github.com/astral-sh/python-build-standalone/releases/download/{tag}/{filename}"
        );
        let filename = url.split('/').next_back().unwrap();
        let install = tv.install_path();
        let download = tv.download_path();
        let tarball_path = download.join(filename);

        ctx.pr.set_message(format!("download {filename}"));
        HTTP.download_file(&url, &tarball_path, Some(&ctx.pr))
            .await?;

        file::remove_all(&install)?;
        file::untar(
            &tarball_path,
            &install,
            &TarOptions {
                strip_components: 1,
                pr: Some(&ctx.pr),
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
                if tv.request.options().get("patch_sysconfig") != Some(&"false".to_string()) {
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

        Ok(())
    }

    async fn install_compiled(&self, ctx: &InstallContext, tv: &ToolVersion) -> eyre::Result<()> {
        self.install_or_update_python_build(Some(ctx))?;
        if matches!(&tv.request, ToolRequest::Ref { .. }) {
            return Err(eyre!("Ref versions not supported for python"));
        }
        ctx.pr.set_message("python-build".into());
        let mut cmd = CmdLineRunner::new(self.python_build_bin())
            .with_pr(&ctx.pr)
            .arg(tv.version.as_str())
            .arg(tv.install_path())
            .env("PIP_REQUIRE_VIRTUALENV", "false")
            .envs(ctx.config.env().await?);
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
            let patch_file = patches_dir.join(format!("{}.patch", &tv.version));
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
        pr: &Box<dyn SingleReport>,
    ) -> eyre::Result<()> {
        if !packages_file.exists() {
            return Ok(());
        }
        pr.set_message("install default packages".into());
        CmdLineRunner::new(tv.install_path().join("bin/python"))
            .with_pr(pr)
            .arg("-m")
            .arg("pip")
            .arg("install")
            .arg("--upgrade")
            .arg("-r")
            .arg(packages_file)
            .env("PIP_REQUIRE_VIRTUALENV", "false")
            .envs(config.env().await?)
            .execute()
    }

    async fn get_virtualenv(
        &self,
        config: &Arc<Config>,
        tv: &ToolVersion,
        pr: Option<&Box<dyn SingleReport>>,
    ) -> eyre::Result<Option<PathBuf>> {
        if let Some(virtualenv) = tv.request.options().get("virtualenv") {
            if !Settings::get().experimental {
                warn!(
                    "please enable experimental mode with `mise settings experimental=true` \
                    to use python virtualenv activation"
                );
            }
            let mut virtualenv: PathBuf = file::replace_path(Path::new(virtualenv));
            if !virtualenv.is_absolute() {
                // TODO: use the path of the config file that specified python, not the top one like this
                if let Some(project_root) = &config.project_root {
                    virtualenv = project_root.join(virtualenv);
                }
            }
            if !virtualenv.exists() {
                if Settings::get().python.venv_auto_create {
                    info!("setting up virtualenv at: {}", virtualenv.display());
                    let mut cmd = CmdLineRunner::new(python_path(tv))
                        .arg("-m")
                        .arg("venv")
                        .arg(&virtualenv)
                        .envs(config.env().await?);
                    if let Some(pr) = pr {
                        cmd = cmd.with_pr(pr);
                    }
                    cmd.execute()?;
                } else {
                    warn!(
                        "no venv found at: {p}\n\n\
                        To create a virtualenv manually, run:\n\
                        python -m venv {p}",
                        p = display_path(&virtualenv)
                    );
                    return Ok(None);
                }
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
        pr: &Box<dyn SingleReport>,
    ) -> eyre::Result<()> {
        pr.set_message("python --version".into());
        CmdLineRunner::new(python_path(tv))
            .with_pr(pr)
            .arg("--version")
            .envs(config.env().await?)
            .execute()
    }
}

#[async_trait]
impl Backend for PythonPlugin {
    fn ba(&self) -> &Arc<BackendArg> {
        &self.ba
    }

    async fn _list_remote_versions(&self, _config: &Arc<Config>) -> eyre::Result<Vec<String>> {
        if cfg!(windows) || Settings::get().python.compile == Some(false) {
            Ok(self
                .fetch_precompiled_remote_versions()
                .await?
                .iter()
                .map(|(v, _, _)| v.clone())
                .collect())
        } else {
            self.install_or_update_python_build(None)?;
            let python_build_bin = self.python_build_bin();
            plugins::core::run_fetch_task_with_timeout(move || {
                let output = cmd!(python_build_bin, "--definitions").read()?;
                let versions = output
                    .split('\n')
                    // remove free-threaded pythons like 3.13t and 3.14t-dev
                    .filter(|s| !regex!(r"\dt(-dev)?$").is_match(s))
                    .map(|s| s.to_string())
                    .sorted_by_cached_key(|v| regex!(r"^\d+").is_match(v))
                    .collect();
                Ok(versions)
            })
        }
    }

    fn idiomatic_filenames(&self) -> eyre::Result<Vec<String>> {
        Ok(vec![
            ".python-version".to_string(),
            ".python-versions".to_string(),
        ])
    }

    async fn install_version_(&self, ctx: &InstallContext, tv: ToolVersion) -> Result<ToolVersion> {
        if cfg!(windows) || Settings::get().python.compile != Some(true) {
            self.install_precompiled(ctx, &tv).await?;
        } else {
            self.install_compiled(ctx, &tv).await?;
        }
        self.test_python(&ctx.config, &tv, &ctx.pr).await?;
        if let Err(e) = self.get_virtualenv(&ctx.config, &tv, Some(&ctx.pr)).await {
            warn!("failed to get virtualenv: {e:#}");
        }
        if let Some(default_file) = &Settings::get().python.default_packages_file {
            let default_file = file::replace_path(default_file);
            if let Err(err) = self
                .install_default_packages(&ctx.config, &default_file, &tv, &ctx.pr)
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
        Ok(vec![tv.install_path()])
    }

    async fn exec_env(
        &self,
        config: &Arc<Config>,
        _ts: &Toolset,
        tv: &ToolVersion,
    ) -> eyre::Result<BTreeMap<String, String>> {
        let mut hm = BTreeMap::new();
        match self.get_virtualenv(config, tv, None).await {
            Err(e) => warn!("failed to get virtualenv: {e}"),
            Ok(Some(virtualenv)) => {
                let bin = virtualenv.join("bin");
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

fn python_os(settings: &Settings) -> String {
    if let Some(os) = &settings.python.precompiled_os {
        return os.clone();
    }
    if cfg!(windows) {
        "pc-windows-msvc-shared".into()
    } else if cfg!(target_os = "macos") {
        "apple-darwin".into()
    } else {
        ["unknown", built_info::CFG_OS, built_info::CFG_ENV]
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
    if cfg!(windows) {
        "x86_64"
    } else if cfg!(linux) && arch == "x86_64" {
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

fn ensure_not_windows() -> eyre::Result<()> {
    if cfg!(windows) {
        bail!(
            "python can not currently be compiled on windows with core:python, use vfox:python instead"
        );
    }
    Ok(())
}
