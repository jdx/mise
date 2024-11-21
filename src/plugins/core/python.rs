use crate::backend::{Backend, VersionCacheManager};
use crate::build_time::built_info;
use crate::cache::{CacheManager, CacheManagerBuilder};
use crate::cli::args::BackendArg;
use crate::cmd::CmdLineRunner;
use crate::config::{Config, SETTINGS};
use crate::file::{display_path, TarFormat, TarOptions};
use crate::git::Git;
use crate::http::{HTTP, HTTP_FETCH};
use crate::install_context::InstallContext;
use crate::toolset::{ToolRequest, ToolVersion, Toolset};
use crate::ui::progress_report::SingleReport;
use crate::{cmd, file, plugins};
use eyre::{bail, eyre};
use itertools::Itertools;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use versions::Versioning;
use xx::regex;

#[derive(Debug)]
pub struct PythonPlugin {
    ba: BackendArg,
    precompiled_cache: CacheManager<Vec<(String, String, String)>>,
}

impl PythonPlugin {
    pub fn new() -> Self {
        let ba = plugins::core::new_backend_arg("python");
        Self {
            precompiled_cache: CacheManagerBuilder::new(
                ba.cache_path.join("precompiled.msgpack.z"),
            )
            .with_fresh_duration(SETTINGS.fetch_remote_versions_cache())
            .build(),
            ba,
        }
    }

    fn python_build_path(&self) -> PathBuf {
        self.ba.cache_path.join("pyenv")
    }
    fn python_build_bin(&self) -> PathBuf {
        self.python_build_path()
            .join("plugins/python-build/bin/python-build")
    }
    fn install_or_update_python_build(&self) -> eyre::Result<()> {
        ensure_not_windows()?;
        if self.python_build_bin().exists() {
            self.update_python_build()
        } else {
            self.install_python_build()
        }
    }
    fn install_python_build(&self) -> eyre::Result<()> {
        if self.python_build_bin().exists() {
            return Ok(());
        }
        let python_build_path = self.python_build_path();
        debug!("Installing python-build to {}", python_build_path.display());
        file::remove_all(&python_build_path)?;
        file::create_dir_all(self.python_build_path().parent().unwrap())?;
        let git = Git::new(self.python_build_path());
        git.clone(&SETTINGS.python.pyenv_repo)?;
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

    fn python_path(&self, tv: &ToolVersion) -> PathBuf {
        if cfg!(windows) {
            tv.install_path().join("python.exe")
        } else {
            tv.install_path().join("bin/python")
        }
    }

    fn fetch_precompiled_remote_versions(&self) -> eyre::Result<&Vec<(String, String, String)>> {
        self.precompiled_cache.get_or_try_init(|| {
            let raw = match SETTINGS.paranoid {
                true => HTTP_FETCH.get_text("https://mise-versions.jdx.dev/python-precompiled"),
                // using http is not a security concern and enabling tls makes mise significantly slower
                false => HTTP_FETCH.get_text("http://mise-versions.jdx.dev/python-precompiled"),
            }?;
            let arch = python_arch();
            let os = python_os();
            let platform = format!("{arch}-{os}");
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
    }

    fn install_precompiled(&self, ctx: &InstallContext, tv: &ToolVersion) -> eyre::Result<()> {
        let precompiled_versions = self.fetch_precompiled_remote_versions()?;
        let precompile_info = precompiled_versions
            .iter()
            .rev()
            .find(|(v, _, _)| &tv.version == v);
        let (tag, filename) = match precompile_info {
            Some((_, tag, filename)) => (tag, filename),
            None => {
                if cfg!(windows) || SETTINGS.python.compile == Some(false) {
                    if !cfg!(windows) {
                        hint!(
                            "python_compile",
                            "To compile python from source, run",
                            "mise settings set python.compile 1"
                        );
                    }
                    let arch = python_arch();
                    let os = python_os();
                    bail!(
                        "no precompiled python found for {} on {arch}-{os}",
                        tv.version
                    );
                }
                debug!("no precompiled python found for {}", tv.version);
                let mut available = precompiled_versions.iter().map(|(v, _, _)| v);
                trace!("available precompiled versions: {}", available.join(", "));
                return self.install_compiled(ctx, tv);
            }
        };

        if cfg!(unix) {
            hint!(
                "python_precompiled",
                "installing precompiled python from indygreg/python-build-standalone\n\
                if you experience issues with this python (e.g.: running poetry), switch to python-build by running",
                "mise settings set python.compile 1"
            );
        }

        let url = format!(
            "https://github.com/indygreg/python-build-standalone/releases/download/{tag}/{filename}"
        );
        let filename = url.split('/').last().unwrap();
        let install = tv.install_path();
        let download = tv.download_path();
        let tarball_path = download.join(filename);

        ctx.pr.set_message(format!("download {filename}"));
        HTTP.download_file(&url, &tarball_path, Some(ctx.pr.as_ref()))?;

        file::remove_all(&install)?;
        file::untar(
            &tarball_path,
            &install,
            &TarOptions {
                format: TarFormat::TarGz,
                strip_components: 1,
                pr: Some(ctx.pr.as_ref()),
            },
        )?;
        #[cfg(unix)]
        file::make_symlink(&install.join("bin/python3"), &install.join("bin/python"))?;
        Ok(())
    }

    fn install_compiled(&self, ctx: &InstallContext, tv: &ToolVersion) -> eyre::Result<()> {
        let config = Config::get();
        self.install_or_update_python_build()?;
        if matches!(&tv.request, ToolRequest::Ref { .. }) {
            return Err(eyre!("Ref versions not supported for python"));
        }
        ctx.pr.set_message("python-build".into());
        let mut cmd = CmdLineRunner::new(self.python_build_bin())
            .with_pr(ctx.pr.as_ref())
            .arg(tv.version.as_str())
            .arg(tv.install_path())
            .env("PIP_REQUIRE_VIRTUALENV", "false")
            .envs(config.env()?);
        if SETTINGS.verbose {
            cmd = cmd.arg("--verbose");
        }
        if let Some(patch_url) = &SETTINGS.python.patch_url {
            ctx.pr
                .set_message(format!("with patch file from: {patch_url}"));
            let patch = HTTP.get_text(patch_url)?;
            cmd = cmd.arg("--patch").stdin_string(patch)
        }
        if let Some(patches_dir) = &SETTINGS.python.patches_directory {
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

    fn install_default_packages(
        &self,
        config: &Config,
        packages_file: &Path,
        tv: &ToolVersion,
        pr: &dyn SingleReport,
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
            .envs(config.env()?)
            .execute()
    }

    fn get_virtualenv(
        &self,
        config: &Config,
        tv: &ToolVersion,
        pr: Option<&dyn SingleReport>,
    ) -> eyre::Result<Option<PathBuf>> {
        if let Some(virtualenv) = tv.request.options().get("virtualenv") {
            if !SETTINGS.experimental {
                warn!(
                    "please enable experimental mode with `mise settings set experimental true` \
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
                if SETTINGS.python.venv_auto_create {
                    info!("setting up virtualenv at: {}", virtualenv.display());
                    let mut cmd = CmdLineRunner::new(self.python_path(tv))
                        .arg("-m")
                        .arg("venv")
                        .arg(&virtualenv)
                        .envs(config.env()?);
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
    //     let target = self.python_path(tv);
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

    fn test_python(
        &self,
        config: &Config,
        tv: &ToolVersion,
        pr: &dyn SingleReport,
    ) -> eyre::Result<()> {
        pr.set_message("python --version".into());
        CmdLineRunner::new(self.python_path(tv))
            .with_pr(pr)
            .arg("--version")
            .envs(config.env()?)
            .execute()
    }
}

impl Backend for PythonPlugin {
    fn ba(&self) -> &BackendArg {
        &self.ba
    }

    fn _list_remote_versions(&self) -> eyre::Result<Vec<String>> {
        if cfg!(windows) || SETTINGS.python.compile == Some(false) {
            Ok(self
                .fetch_precompiled_remote_versions()?
                .iter()
                .map(|(v, _, _)| v.clone())
                .collect())
        } else {
            self.install_or_update_python_build()?;
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

    fn legacy_filenames(&self) -> eyre::Result<Vec<String>> {
        Ok(vec![".python-version".to_string()])
    }

    fn install_version_impl(
        &self,
        ctx: &InstallContext,
        tv: ToolVersion,
    ) -> eyre::Result<ToolVersion> {
        let config = Config::get();
        if cfg!(windows) || SETTINGS.python.compile != Some(true) {
            self.install_precompiled(ctx, &tv)?;
        } else {
            self.install_compiled(ctx, &tv)?;
        }
        self.test_python(&config, &tv, ctx.pr.as_ref())?;
        if let Err(e) = self.get_virtualenv(&config, &tv, Some(ctx.pr.as_ref())) {
            warn!("failed to get virtualenv: {e:#}");
        }
        if let Some(default_file) = &SETTINGS.python.default_packages_file {
            if let Err(err) =
                self.install_default_packages(&config, default_file, &tv, ctx.pr.as_ref())
            {
                warn!("failed to install default python packages: {err:#}");
            }
        }
        Ok(tv)
    }

    #[cfg(windows)]
    fn list_bin_paths(&self, tv: &ToolVersion) -> eyre::Result<Vec<PathBuf>> {
        Ok(vec![tv.install_path()])
    }

    fn exec_env(
        &self,
        config: &Config,
        _ts: &Toolset,
        tv: &ToolVersion,
    ) -> eyre::Result<BTreeMap<String, String>> {
        let mut hm = BTreeMap::new();
        match self.get_virtualenv(config, tv, None) {
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

    fn get_remote_version_cache(&self) -> Arc<VersionCacheManager> {
        static CACHE: OnceLock<Arc<VersionCacheManager>> = OnceLock::new();
        CACHE
            .get_or_init(|| {
                CacheManagerBuilder::new(self.ba().cache_path.join("remote_versions.msgpack.z"))
                    .with_fresh_duration(SETTINGS.fetch_remote_versions_cache())
                    .with_cache_key((SETTINGS.python.compile == Some(false)).to_string())
                    .build()
                    .into()
            })
            .clone()
    }
}

fn python_os() -> String {
    if let Some(os) = &SETTINGS.python.precompiled_os {
        return os.clone();
    }
    if cfg!(windows) {
        "pc-windows-msvc-shared".into()
    } else if cfg!(target_os = "macos") {
        "apple-darwin".into()
    } else {
        let os = &built_info::CFG_OS;
        let env = &built_info::CFG_ENV;
        format!("unknown-{os}-{env}")
    }
}

fn python_arch() -> &'static str {
    if let Some(arch) = &SETTINGS.python.precompiled_arch {
        return arch.as_str();
    }
    if cfg!(windows) {
        "x86_64"
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
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
        built_info::CFG_TARGET_ARCH
    }
}

fn ensure_not_windows() -> eyre::Result<()> {
    if cfg!(windows) {
        bail!("python can not currently be compiled on windows with core:python, use vfox:python instead");
    }
    Ok(())
}
