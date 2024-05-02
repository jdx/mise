use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use itertools::Itertools;

use crate::build_time::built_info;
use crate::cache::CacheManager;
use crate::cli::args::ForgeArg;
use crate::cmd::CmdLineRunner;
use crate::config::{Config, Settings};
use crate::file::display_path;
use crate::forge::Forge;
use crate::git::Git;
use crate::http::{HTTP, HTTP_FETCH};
use crate::install_context::InstallContext;
use crate::plugins::core::CorePlugin;
use crate::toolset::{ToolVersion, ToolVersionRequest, Toolset};
use crate::ui::progress_report::SingleReport;
use crate::{cmd, env, file};

#[derive(Debug)]
pub struct PythonPlugin {
    core: CorePlugin,
    precompiled_cache: CacheManager<Vec<(String, String, String)>>,
}

impl PythonPlugin {
    pub fn new() -> Self {
        let core = CorePlugin::new("python");
        Self {
            precompiled_cache: CacheManager::new(core.fa.cache_path.join("precompiled.msgpack.z"))
                .with_fresh_duration(*env::MISE_FETCH_REMOTE_VERSIONS_CACHE),
            core,
        }
    }

    fn python_build_path(&self) -> PathBuf {
        self.core.fa.cache_path.join("pyenv")
    }
    fn python_build_bin(&self) -> PathBuf {
        self.python_build_path()
            .join("plugins/python-build/bin/python-build")
    }
    fn install_or_update_python_build(&self) -> eyre::Result<()> {
        if self.python_build_path().exists() {
            self.update_python_build()
        } else {
            self.install_python_build()
        }
    }
    fn install_python_build(&self) -> eyre::Result<()> {
        if self.python_build_path().exists() {
            return Ok(());
        }
        let settings = Settings::try_get()?;
        let python_build_path = self.python_build_path();
        debug!("Installing python-build to {}", python_build_path.display());
        file::create_dir_all(self.python_build_path().parent().unwrap())?;
        let git = Git::new(self.python_build_path());
        git.clone(&settings.python_pyenv_repo)?;
        Ok(())
    }
    fn update_python_build(&self) -> eyre::Result<()> {
        // TODO: do not update if recently updated
        debug!(
            "Updating python-build in {}",
            self.python_build_path().display()
        );
        let git = Git::new(self.python_build_path());
        CorePlugin::run_fetch_task_with_timeout(move || git.update(None))?;
        Ok(())
    }

    fn fetch_remote_versions(&self) -> eyre::Result<Vec<String>> {
        match self.core.fetch_remote_versions_from_mise() {
            Ok(Some(versions)) => return Ok(versions),
            Ok(None) => {}
            Err(e) => warn!("failed to fetch remote versions: {}", e),
        }
        self.install_or_update_python_build()?;
        let python_build_bin = self.python_build_bin();
        CorePlugin::run_fetch_task_with_timeout(move || {
            let output = cmd!(python_build_bin, "--definitions").read()?;
            let versions = output
                .split('\n')
                .map(|s| s.to_string())
                .sorted_by_cached_key(|v| regex!(r"^\d+").is_match(v))
                .collect();
            Ok(versions)
        })
    }

    fn python_path(&self, tv: &ToolVersion) -> PathBuf {
        tv.install_short_path().join("bin/python")
    }

    fn fetch_precompiled_remote_versions(&self) -> eyre::Result<&Vec<(String, String, String)>> {
        self.precompiled_cache.get_or_try_init(|| {
            let settings = Settings::get();
            let raw = HTTP_FETCH.get_text("http://mise-versions.jdx.dev/python-precompiled")?;
            let platform = format!("{}-{}", python_arch(&settings), python_os(&settings));
            let versions = raw
                .lines()
                .filter(|v| v.contains(&platform))
                .flat_map(|v| {
                    regex!(r"^cpython-(\d+\.\d+\.\d+)\+(\d+).*")
                        .captures(v)
                        .map(|caps| {
                            (
                                caps[1].to_string(),
                                caps[2].to_string(),
                                caps[0].to_string(),
                            )
                        })
                })
                .collect_vec();
            Ok(versions)
        })
    }

    fn install_precompiled(&self, ctx: &InstallContext) -> eyre::Result<()> {
        let precompiled_versions = self.fetch_precompiled_remote_versions()?;
        let precompile_info = precompiled_versions
            .iter()
            .rev()
            .find(|(v, _, _)| &ctx.tv.version == v);
        let (tag, filename) = match precompile_info {
            Some((_, tag, filename)) => (tag, filename),
            None => {
                debug!("no precompiled python found for {}", ctx.tv.version);
                let mut available = precompiled_versions.iter().map(|(v, _, _)| v);
                trace!("available precompiled versions: {}", available.join(", "));
                return self.install_compiled(ctx);
            }
        };

        warn!("installing precompiled python from indygreg/python-build-standalone");
        warn!("if you experience issues with this python (e.g.: running poetry), switch to python-build");
        warn!("by running: mise settings set python_compile 1");

        let url = format!(
            "https://github.com/indygreg/python-build-standalone/releases/download/{tag}/{filename}"
        );
        let filename = url.split('/').last().unwrap();
        let install = ctx.tv.install_path();
        let download = ctx.tv.download_path();
        let tarball_path = download.join(filename);

        ctx.pr.set_message(format!("downloading {filename}"));
        HTTP.download_file(&url, &tarball_path, Some(ctx.pr.as_ref()))?;

        ctx.pr.set_message(format!("installing {filename}"));
        file::untar(&tarball_path, &download)?;
        file::remove_all(&install)?;
        file::rename(download.join("python"), &install)?;
        file::make_symlink(&install.join("bin/python3"), &install.join("bin/python"))?;
        Ok(())
    }

    fn install_compiled(&self, ctx: &InstallContext) -> eyre::Result<()> {
        let config = Config::get();
        let settings = Settings::get();
        self.install_or_update_python_build()?;
        if matches!(&ctx.tv.request, ToolVersionRequest::Ref(..)) {
            return Err(eyre!("Ref versions not supported for python"));
        }
        ctx.pr.set_message("Running python-build".into());
        let mut cmd = CmdLineRunner::new(self.python_build_bin())
            .with_pr(ctx.pr.as_ref())
            .arg(ctx.tv.version.as_str())
            .arg(&ctx.tv.install_path())
            .env("PIP_REQUIRE_VIRTUALENV", "false")
            .envs(config.env()?);
        if settings.verbose {
            cmd = cmd.arg("--verbose");
        }
        if let Some(patch_url) = &settings.python_patch_url {
            ctx.pr
                .set_message(format!("with patch file from: {patch_url}"));
            let patch = HTTP.get_text(patch_url)?;
            cmd = cmd.arg("--patch").stdin_string(patch)
        }
        if let Some(patches_dir) = &settings.python_patches_directory {
            let patch_file = patches_dir.join(format!("{}.patch", &ctx.tv.version));
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
        pr.set_message("installing default packages".into());
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
        if let Some(virtualenv) = tv.opts.get("virtualenv") {
            let settings = Settings::try_get()?;
            if !settings.experimental {
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
                if settings.python_venv_auto_create {
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

impl Forge for PythonPlugin {
    fn fa(&self) -> &ForgeArg {
        &self.core.fa
    }

    fn _list_remote_versions(&self) -> eyre::Result<Vec<String>> {
        self.core
            .remote_version_cache
            .get_or_try_init(|| self.fetch_remote_versions())
            .cloned()
    }

    fn legacy_filenames(&self) -> eyre::Result<Vec<String>> {
        Ok(vec![".python-version".to_string()])
    }

    fn install_version_impl(&self, ctx: &InstallContext) -> eyre::Result<()> {
        let config = Config::get();
        let settings = Settings::try_get()?;
        if settings.python_compile {
            self.install_compiled(ctx)?;
        } else {
            self.install_precompiled(ctx)?;
        }
        self.test_python(&config, &ctx.tv, ctx.pr.as_ref())?;
        if let Err(e) = self.get_virtualenv(&config, &ctx.tv, Some(ctx.pr.as_ref())) {
            warn!("failed to get virtualenv: {e}");
        }
        if let Some(default_file) = &settings.python_default_packages_file {
            self.install_default_packages(&config, default_file, &ctx.tv, ctx.pr.as_ref())?;
        }
        Ok(())
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
}

fn python_os(settings: &Settings) -> String {
    if let Some(os) = &settings.python_precompiled_os {
        return os.clone();
    }
    if cfg!(target_os = "macos") {
        "apple-darwin".into()
    } else {
        let os = &built_info::CFG_OS;
        let env = &built_info::CFG_ENV;
        format!("unknown-{os}-{env}")
    }
}

fn python_arch(settings: &Settings) -> &str {
    if let Some(arch) = &settings.python_precompiled_arch {
        return arch.as_str();
    }
    if cfg!(target_arch = "x86_64") {
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
