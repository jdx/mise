use crate::backend::{Backend, VersionCacheManager};
use crate::build_time::built_info;
use crate::cache::CacheManagerBuilder;
use crate::cli::args::BackendArg;
use crate::cmd::CmdLineRunner;
use crate::config::{Config, Settings};
use crate::file::{TarFormat, TarOptions};
use crate::http::{HTTP, HTTP_FETCH};
use crate::install_context::InstallContext;
use crate::toolset::ToolVersion;
use crate::ui::progress_report::SingleReport;
use crate::{env, file, gpg, hash, http, plugins};
use async_trait::async_trait;
use eyre::{Result, bail, ensure};
use serde_derive::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::OnceLock;
use tempfile::tempdir_in;
use tokio::sync::Mutex;
use url::Url;
use xx::regex;

#[derive(Debug)]
pub struct NodePlugin {
    ba: Arc<BackendArg>,
}

impl NodePlugin {
    pub fn new() -> Self {
        Self {
            ba: plugins::core::new_backend_arg("node").into(),
        }
    }

    async fn install_precompiled(
        &self,
        ctx: &InstallContext,
        tv: &mut ToolVersion,
        opts: &BuildOpts,
    ) -> Result<()> {
        let settings = Settings::get();
        match self
            .fetch_tarball(
                ctx,
                tv,
                &ctx.pr,
                &opts.binary_tarball_url,
                &opts.binary_tarball_path,
                &opts.version,
            )
            .await
        {
            Err(e)
                if settings.node.compile != Some(false)
                    && matches!(http::error_code(&e), Some(404)) =>
            {
                debug!("precompiled node not found");
                return self.install_compiled(ctx, tv, opts).await;
            }
            e => e,
        }?;
        let tarball_name = &opts.binary_tarball_name;
        ctx.pr.set_message(format!("extract {tarball_name}"));
        file::remove_all(&opts.install_path)?;
        file::untar(
            &opts.binary_tarball_path,
            &opts.install_path,
            &TarOptions {
                format: TarFormat::TarGz,
                strip_components: 1,
                pr: Some(&ctx.pr),
            },
        )?;
        Ok(())
    }

    async fn install_windows(
        &self,
        ctx: &InstallContext,
        tv: &mut ToolVersion,
        opts: &BuildOpts,
    ) -> Result<()> {
        match self
            .fetch_tarball(
                ctx,
                tv,
                &ctx.pr,
                &opts.binary_tarball_url,
                &opts.binary_tarball_path,
                &opts.version,
            )
            .await
        {
            Err(e) if matches!(http::error_code(&e), Some(404)) => {
                bail!("precompiled node not found {e}");
            }
            e => e,
        }?;
        let tarball_name = &opts.binary_tarball_name;
        ctx.pr.set_message(format!("extract {tarball_name}"));
        let tmp_extract_path = tempdir_in(opts.install_path.parent().unwrap())?;
        file::unzip(&opts.binary_tarball_path, tmp_extract_path.path())?;
        file::remove_all(&opts.install_path)?;
        file::rename(
            tmp_extract_path.path().join(slug(&opts.version)),
            &opts.install_path,
        )?;
        Ok(())
    }

    async fn install_compiled(
        &self,
        ctx: &InstallContext,
        tv: &mut ToolVersion,
        opts: &BuildOpts,
    ) -> Result<()> {
        let tarball_name = &opts.source_tarball_name;
        self.fetch_tarball(
            ctx,
            tv,
            &ctx.pr,
            &opts.source_tarball_url,
            &opts.source_tarball_path,
            &opts.version,
        )
        .await?;
        ctx.pr.set_message(format!("extract {tarball_name}"));
        file::remove_all(&opts.build_dir)?;
        file::untar(
            &opts.source_tarball_path,
            opts.build_dir.parent().unwrap(),
            &TarOptions {
                format: TarFormat::TarGz,
                pr: Some(&ctx.pr),
                ..Default::default()
            },
        )?;
        self.exec_configure(ctx, opts)?;
        self.exec_make(ctx, opts)?;
        self.exec_make_install(ctx, opts)?;
        Ok(())
    }

    async fn fetch_tarball(
        &self,
        ctx: &InstallContext,
        tv: &mut ToolVersion,
        pr: &Box<dyn SingleReport>,
        url: &Url,
        local: &Path,
        version: &str,
    ) -> Result<()> {
        let tarball_name = local.file_name().unwrap().to_string_lossy().to_string();
        if local.exists() {
            pr.set_message(format!("using previously downloaded {tarball_name}"));
        } else {
            pr.set_message(format!("download {tarball_name}"));
            HTTP.download_file(url.clone(), local, Some(pr)).await?;
        }
        if *env::MISE_NODE_VERIFY && !tv.checksums.contains_key(&tarball_name) {
            tv.checksums
                .insert(tarball_name, self.get_checksum(ctx, local, version).await?);
        }
        self.verify_checksum(ctx, tv, local)?;
        Ok(())
    }

    fn sh<'a>(&self, ctx: &'a InstallContext, opts: &BuildOpts) -> eyre::Result<CmdLineRunner<'a>> {
        let mut cmd = CmdLineRunner::new("sh")
            .prepend_path(opts.path.clone())?
            .with_pr(&ctx.pr)
            .current_dir(&opts.build_dir)
            .arg("-c");
        if let Some(cflags) = &*env::MISE_NODE_CFLAGS {
            cmd = cmd.env("CFLAGS", cflags);
        }
        Ok(cmd)
    }

    fn exec_configure(&self, ctx: &InstallContext, opts: &BuildOpts) -> Result<()> {
        self.sh(ctx, opts)?.arg(&opts.configure_cmd).execute()
    }
    fn exec_make(&self, ctx: &InstallContext, opts: &BuildOpts) -> Result<()> {
        self.sh(ctx, opts)?.arg(&opts.make_cmd).execute()
    }
    fn exec_make_install(&self, ctx: &InstallContext, opts: &BuildOpts) -> Result<()> {
        self.sh(ctx, opts)?.arg(&opts.make_install_cmd).execute()
    }

    async fn get_checksum(
        &self,
        ctx: &InstallContext,
        tarball: &Path,
        version: &str,
    ) -> Result<String> {
        let tarball_name = tarball.file_name().unwrap().to_string_lossy().to_string();
        let shasums_file = tarball.parent().unwrap().join("SHASUMS256.txt");
        HTTP.download_file(self.shasums_url(version)?, &shasums_file, Some(&ctx.pr))
            .await?;
        if Settings::get().node.gpg_verify != Some(false) && version.starts_with("2") {
            self.verify_with_gpg(ctx, &shasums_file, version).await?;
        }
        let shasums = file::read_to_string(&shasums_file)?;
        let shasums = hash::parse_shasums(&shasums);
        let shasum = shasums.get(&tarball_name).unwrap();
        Ok(format!("sha256:{shasum}"))
    }

    async fn verify_with_gpg(
        &self,
        ctx: &InstallContext,
        shasums_file: &Path,
        v: &str,
    ) -> Result<()> {
        if file::which_non_pristine("gpg").is_none() && Settings::get().node.gpg_verify.is_none() {
            warn!("gpg not found, skipping verification");
            return Ok(());
        }
        let sig_file = shasums_file.with_extension("asc");
        let sig_url = format!("{}.sig", self.shasums_url(v)?);
        if let Err(e) = HTTP.download_file(sig_url, &sig_file, Some(&ctx.pr)).await {
            if matches!(http::error_code(&e), Some(404)) {
                warn!("gpg signature not found, skipping verification");
                return Ok(());
            }
            return Err(e);
        }
        gpg::add_keys_node(ctx)?;
        CmdLineRunner::new("gpg")
            .arg("--quiet")
            .arg("--trust-model")
            .arg("always")
            .arg("--verify")
            .arg(sig_file)
            .arg(shasums_file)
            .with_pr(&ctx.pr)
            .execute()?;
        Ok(())
    }

    fn node_path(&self, tv: &ToolVersion) -> PathBuf {
        if cfg!(windows) {
            tv.install_path().join("node.exe")
        } else {
            tv.install_path().join("bin").join("node")
        }
    }

    fn npm_path(&self, tv: &ToolVersion) -> PathBuf {
        if cfg!(windows) {
            tv.install_path().join("npm.cmd")
        } else {
            tv.install_path().join("bin").join("npm")
        }
    }

    fn corepack_path(&self, tv: &ToolVersion) -> PathBuf {
        if cfg!(windows) {
            tv.install_path().join("corepack.cmd")
        } else {
            tv.install_path().join("bin").join("corepack")
        }
    }

    async fn install_default_packages(
        &self,
        config: &Arc<Config>,
        tv: &ToolVersion,
        pr: &Box<dyn SingleReport>,
    ) -> Result<()> {
        let body = file::read_to_string(&*env::MISE_NODE_DEFAULT_PACKAGES_FILE).unwrap_or_default();
        for package in body.lines() {
            let package = package.split('#').next().unwrap_or_default().trim();
            if package.is_empty() {
                continue;
            }
            pr.set_message(format!("install default package: {package}"));
            let npm = self.npm_path(tv);
            CmdLineRunner::new(npm)
                .with_pr(pr)
                .arg("install")
                .arg("--global")
                .arg(package)
                .envs(config.env().await?)
                .env(&*env::PATH_KEY, plugins::core::path_env_with_tv_path(tv)?)
                .execute()?;
        }
        Ok(())
    }

    fn install_npm_shim(&self, tv: &ToolVersion) -> Result<()> {
        file::remove_file(self.npm_path(tv)).ok();
        file::write(self.npm_path(tv), include_str!("assets/node_npm_shim"))?;
        file::make_executable(self.npm_path(tv))?;
        Ok(())
    }

    fn enable_default_corepack_shims(
        &self,
        tv: &ToolVersion,
        pr: &Box<dyn SingleReport>,
    ) -> Result<()> {
        pr.set_message("enable corepack shims".into());
        let corepack = self.corepack_path(tv);
        CmdLineRunner::new(corepack)
            .with_pr(pr)
            .arg("enable")
            .env(&*env::PATH_KEY, plugins::core::path_env_with_tv_path(tv)?)
            .execute()?;
        Ok(())
    }

    async fn test_node(
        &self,
        config: &Arc<Config>,
        tv: &ToolVersion,
        pr: &Box<dyn SingleReport>,
    ) -> Result<()> {
        pr.set_message("node -v".into());
        CmdLineRunner::new(self.node_path(tv))
            .with_pr(pr)
            .arg("-v")
            .envs(config.env().await?)
            .execute()
    }

    async fn test_npm(
        &self,
        config: &Arc<Config>,
        tv: &ToolVersion,
        pr: &Box<dyn SingleReport>,
    ) -> Result<()> {
        pr.set_message("npm -v".into());
        CmdLineRunner::new(self.npm_path(tv))
            .env(&*env::PATH_KEY, plugins::core::path_env_with_tv_path(tv)?)
            .with_pr(pr)
            .arg("-v")
            .envs(config.env().await?)
            .execute()
    }

    fn shasums_url(&self, v: &str) -> Result<Url> {
        // let url = MISE_NODE_MIRROR_URL.join(&format!("v{v}/SHASUMS256.txt.asc"))?;
        let settings = Settings::get();
        let url = settings
            .node
            .mirror_url()
            .join(&format!("v{v}/SHASUMS256.txt"))?;
        Ok(url)
    }
}

#[async_trait]
impl Backend for NodePlugin {
    fn ba(&self) -> &Arc<BackendArg> {
        &self.ba
    }

    async fn _list_remote_versions(&self, _config: &Arc<Config>) -> Result<Vec<String>> {
        let settings = Settings::get();
        let base = Settings::get().node.mirror_url();
        let versions = HTTP_FETCH
            .json::<Vec<NodeVersion>, _>(base.join("index.json")?)
            .await?
            .into_iter()
            .filter(|v| {
                if let Some(flavor) = &settings.node.flavor {
                    v.files
                        .iter()
                        .any(|f| f == &format!("{}-{}-{}", os(), arch(&settings), flavor))
                } else {
                    true
                }
            })
            .map(|v| {
                if regex!(r"^v\d+\.").is_match(&v.version) {
                    v.version.strip_prefix('v').unwrap().to_string()
                } else {
                    v.version
                }
            })
            .rev()
            .collect();
        Ok(versions)
    }

    fn get_aliases(&self) -> Result<BTreeMap<String, String>> {
        let aliases = [
            ("lts/argon", "4"),
            ("lts/boron", "6"),
            ("lts/carbon", "8"),
            ("lts/dubnium", "10"),
            ("lts/erbium", "12"),
            ("lts/fermium", "14"),
            ("lts/gallium", "16"),
            ("lts/hydrogen", "18"),
            ("lts/iron", "20"),
            ("lts/jod", "22"),
            ("lts-argon", "4"),
            ("lts-boron", "6"),
            ("lts-carbon", "8"),
            ("lts-dubnium", "10"),
            ("lts-erbium", "12"),
            ("lts-fermium", "14"),
            ("lts-gallium", "16"),
            ("lts-hydrogen", "18"),
            ("lts-iron", "20"),
            ("lts-jod", "22"),
            ("lts", "22"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        Ok(aliases)
    }

    fn idiomatic_filenames(&self) -> Result<Vec<String>> {
        Ok(vec![".node-version".into(), ".nvmrc".into()])
    }

    fn parse_idiomatic_file(&self, path: &Path) -> Result<String> {
        let body = file::read_to_string(path)?;
        // strip comments
        let body = body.split('#').next().unwrap_or_default().to_string();
        // trim "v" prefix
        let body = body.trim().strip_prefix('v').unwrap_or(&body);
        // replace lts/* with lts
        let body = body.replace("lts/*", "lts");
        Ok(body)
    }

    async fn install_version_(
        &self,
        ctx: &InstallContext,
        mut tv: ToolVersion,
    ) -> eyre::Result<ToolVersion> {
        ensure!(
            tv.version != "latest",
            "version should not be 'latest' for node, something is wrong"
        );
        let settings = Settings::get();
        let opts = BuildOpts::new(ctx, &tv).await?;
        trace!("node build opts: {:#?}", opts);
        if cfg!(windows) {
            self.install_windows(ctx, &mut tv, &opts).await?;
        } else if settings.node.compile == Some(true) {
            self.install_compiled(ctx, &mut tv, &opts).await?;
        } else {
            self.install_precompiled(ctx, &mut tv, &opts).await?;
        }
        self.test_node(&ctx.config, &tv, &ctx.pr).await?;
        if !cfg!(windows) {
            self.install_npm_shim(&tv)?;
        }
        self.test_npm(&ctx.config, &tv, &ctx.pr).await?;
        if let Err(err) = self
            .install_default_packages(&ctx.config, &tv, &ctx.pr)
            .await
        {
            warn!("failed to install default npm packages: {err:#}");
        }
        if *env::MISE_NODE_COREPACK && self.corepack_path(&tv).exists() {
            self.enable_default_corepack_shims(&tv, &ctx.pr)?;
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

    fn get_remote_version_cache(&self) -> Arc<Mutex<VersionCacheManager>> {
        static CACHE: OnceLock<Arc<Mutex<VersionCacheManager>>> = OnceLock::new();
        CACHE
            .get_or_init(|| {
                Mutex::new(
                    CacheManagerBuilder::new(
                        self.ba().cache_path.join("remote_versions.msgpack.z"),
                    )
                    .with_fresh_duration(Settings::get().fetch_remote_versions_cache())
                    .with_cache_key(Settings::get().node.mirror_url.clone().unwrap_or_default())
                    .with_cache_key(Settings::get().node.flavor.clone().unwrap_or_default())
                    .build(),
                )
                .into()
            })
            .clone()
    }
}

#[derive(Debug)]
struct BuildOpts {
    version: String,
    path: Vec<PathBuf>,
    install_path: PathBuf,
    build_dir: PathBuf,
    configure_cmd: String,
    make_cmd: String,
    make_install_cmd: String,
    source_tarball_name: String,
    source_tarball_path: PathBuf,
    source_tarball_url: Url,
    binary_tarball_name: String,
    binary_tarball_path: PathBuf,
    binary_tarball_url: Url,
}

impl BuildOpts {
    async fn new(ctx: &InstallContext, tv: &ToolVersion) -> Result<Self> {
        let v = &tv.version;
        let install_path = tv.install_path();
        let source_tarball_name = format!("node-v{v}.tar.gz");

        let slug = slug(v);
        #[cfg(windows)]
        let binary_tarball_name = format!("{slug}.zip");
        #[cfg(not(windows))]
        let binary_tarball_name = format!("{slug}.tar.gz");

        Ok(Self {
            version: v.clone(),
            path: ctx.ts.list_paths(&ctx.config).await,
            build_dir: env::MISE_TMP_DIR.join(format!("node-v{v}")),
            configure_cmd: configure_cmd(&install_path),
            make_cmd: make_cmd(),
            make_install_cmd: make_install_cmd(),
            source_tarball_path: tv.download_path().join(&source_tarball_name),
            source_tarball_url: Settings::get()
                .node
                .mirror_url()
                .join(&format!("v{v}/{source_tarball_name}"))?,
            source_tarball_name,
            binary_tarball_path: tv.download_path().join(&binary_tarball_name),
            binary_tarball_url: Settings::get()
                .node
                .mirror_url()
                .join(&format!("v{v}/{binary_tarball_name}"))?,
            binary_tarball_name,
            install_path,
        })
    }
}

fn configure_cmd(install_path: &Path) -> String {
    let mut configure_cmd = format!("./configure --prefix={}", install_path.display());
    if *env::MISE_NODE_NINJA {
        configure_cmd.push_str(" --ninja");
    }
    if let Some(opts) = &*env::MISE_NODE_CONFIGURE_OPTS {
        configure_cmd.push_str(&format!(" {opts}"));
    }
    configure_cmd
}

fn make_cmd() -> String {
    let mut make_cmd = env::MISE_NODE_MAKE.to_string();
    if let Some(concurrency) = *env::MISE_NODE_CONCURRENCY {
        make_cmd.push_str(&format!(" -j{concurrency}"));
    }
    if let Some(opts) = &*env::MISE_NODE_MAKE_OPTS {
        make_cmd.push_str(&format!(" {opts}"));
    }
    make_cmd
}

fn make_install_cmd() -> String {
    let mut make_install_cmd = format!("{} install", &*env::MISE_NODE_MAKE);
    if let Some(opts) = &*env::MISE_NODE_MAKE_INSTALL_OPTS {
        make_install_cmd.push_str(&format!(" {opts}"));
    }
    make_install_cmd
}

fn os() -> &'static str {
    if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "macos") {
        "darwin"
    } else if cfg!(target_os = "windows") {
        "win"
    } else {
        built_info::CFG_OS
    }
}

fn arch(settings: &Settings) -> &str {
    let arch = settings.arch();
    if arch == "x86" {
        "x86"
    } else if arch == "x86_64" {
        "x64"
    } else if arch == "arm" {
        if cfg!(target_feature = "v6") {
            "armv6l"
        } else {
            "armv7l"
        }
    } else if arch == "loongarch64" {
        "loong64"
    } else if arch == "riscv64" {
        "riscv64"
    } else if arch == "aarch64" {
        "arm64"
    } else {
        arch
    }
}

fn slug(v: &str) -> String {
    let settings = Settings::get();
    if let Some(flavor) = &settings.node.flavor {
        format!("node-v{v}-{}-{}-{flavor}", os(), arch(&settings))
    } else {
        format!("node-v{v}-{}-{}", os(), arch(&settings))
    }
}

#[derive(Debug, Deserialize)]
struct NodeVersion {
    version: String,
    files: Vec<String>,
}
