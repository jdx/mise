use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use eyre::Result;
use serde_derive::Deserialize;
use tempfile::tempdir_in;
use url::Url;

use crate::build_time::built_info;
use crate::cli::args::ForgeArg;
use crate::cmd::CmdLineRunner;
use crate::config::{Config, Settings};
use crate::env::MISE_NODE_MIRROR_URL;
use crate::forge::Forge;
use crate::http::{HTTP, HTTP_FETCH};
use crate::install_context::InstallContext;
use crate::plugins::core::CorePlugin;
use crate::toolset::ToolVersion;
use crate::ui::progress_report::SingleReport;
use crate::{env, file, hash, http};

#[derive(Debug)]
pub struct NodePlugin {
    core: CorePlugin,
}

impl NodePlugin {
    pub fn new() -> Self {
        Self {
            core: CorePlugin::new("node"),
        }
    }

    fn fetch_remote_versions(&self) -> Result<Vec<String>> {
        let node_url_overridden = env::var("MISE_NODE_MIRROR_URL")
            .or(env::var("NODE_BUILD_MIRROR_URL"))
            .is_ok();
        if !node_url_overridden {
            match self.core.fetch_remote_versions_from_mise() {
                Ok(Some(versions)) => return Ok(versions),
                Ok(None) => {}
                Err(e) => warn!("failed to fetch remote versions: {}", e),
            }
        }
        self.fetch_remote_versions_from_node(&MISE_NODE_MIRROR_URL)
    }
    fn fetch_remote_versions_from_node(&self, base: &Url) -> Result<Vec<String>> {
        let versions = HTTP_FETCH
            .json::<Vec<NodeVersion>, _>(base.join("index.json")?)?
            .into_iter()
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

    fn install_precompiled(&self, ctx: &InstallContext, opts: &BuildOpts) -> Result<()> {
        match self.fetch_tarball(
            ctx.pr.as_ref(),
            &opts.binary_tarball_url,
            &opts.binary_tarball_path,
            &opts.version,
        ) {
            Err(e) if matches!(http::error_code(&e), Some(404)) => {
                debug!("precompiled node not found");
                return self.install_compiled(ctx, opts);
            }
            e => e,
        }?;
        let tarball_name = &opts.binary_tarball_name;
        ctx.pr.set_message(format!("extracting {tarball_name}"));
        let tmp_extract_path = tempdir_in(opts.install_path.parent().unwrap())?;
        file::untar(&opts.binary_tarball_path, tmp_extract_path.path())?;
        file::remove_all(&opts.install_path)?;
        let slug = format!("node-v{}-{}-{}", &opts.version, os(), arch());
        file::rename(tmp_extract_path.path().join(slug), &opts.install_path)?;
        Ok(())
    }

    fn install_compiled(&self, ctx: &InstallContext, opts: &BuildOpts) -> Result<()> {
        let tarball_name = &opts.source_tarball_name;
        self.fetch_tarball(
            ctx.pr.as_ref(),
            &opts.source_tarball_url,
            &opts.source_tarball_path,
            &opts.version,
        )?;
        ctx.pr.set_message(format!("extracting {tarball_name}"));
        file::remove_all(&opts.build_dir)?;
        file::untar(&opts.source_tarball_path, opts.build_dir.parent().unwrap())?;
        self.exec_configure(ctx, opts)?;
        self.exec_make(ctx, opts)?;
        self.exec_make_install(ctx, opts)?;
        Ok(())
    }

    fn fetch_tarball(
        &self,
        pr: &dyn SingleReport,
        url: &Url,
        local: &Path,
        version: &str,
    ) -> Result<()> {
        let tarball_name = local.file_name().unwrap().to_string_lossy().to_string();
        if local.exists() {
            pr.set_message(format!("using previously downloaded {tarball_name}"));
        } else {
            pr.set_message(format!("downloading {tarball_name}"));
            HTTP.download_file(url.clone(), local, Some(pr))?;
        }
        if *env::MISE_NODE_VERIFY {
            pr.set_message(format!("verifying {tarball_name}"));
            self.verify(local, version, pr)?;
        }
        Ok(())
    }

    fn sh<'a>(&'a self, ctx: &'a InstallContext, opts: &BuildOpts) -> eyre::Result<CmdLineRunner> {
        let mut cmd = CmdLineRunner::new("sh")
            .prepend_path(opts.path.clone())?
            .with_pr(ctx.pr.as_ref())
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

    fn verify(&self, tarball: &Path, version: &str, pr: &dyn SingleReport) -> Result<()> {
        let tarball_name = tarball.file_name().unwrap().to_string_lossy().to_string();
        // TODO: verify gpg signature
        let shasums = HTTP.get_text(self.shasums_url(version)?)?;
        let shasums = hash::parse_shasums(&shasums);
        let shasum = shasums.get(&tarball_name).unwrap();
        hash::ensure_checksum_sha256(tarball, shasum, Some(pr))
    }

    fn node_path(&self, tv: &ToolVersion) -> PathBuf {
        tv.install_path().join("bin/node")
    }

    fn npm_path(&self, tv: &ToolVersion) -> PathBuf {
        tv.install_path().join("bin/npm")
    }

    fn corepack_path(&self, tv: &ToolVersion) -> PathBuf {
        tv.install_path().join("bin/corepack")
    }

    fn install_default_packages(
        &self,
        config: &Config,
        tv: &ToolVersion,
        pr: &dyn SingleReport,
    ) -> Result<()> {
        let body = file::read_to_string(&*env::MISE_NODE_DEFAULT_PACKAGES_FILE).unwrap_or_default();
        for package in body.lines() {
            let package = package.split('#').next().unwrap_or_default().trim();
            if package.is_empty() {
                continue;
            }
            pr.set_message(format!("installing default package: {}", package));
            let npm = self.npm_path(tv);
            CmdLineRunner::new(npm)
                .with_pr(pr)
                .arg("install")
                .arg("--global")
                .arg(package)
                .envs(config.env()?)
                .env("PATH", CorePlugin::path_env_with_tv_path(tv)?)
                .execute()?;
        }
        Ok(())
    }

    fn install_npm_shim(&self, tv: &ToolVersion) -> Result<()> {
        file::remove_file(self.npm_path(tv)).ok();
        file::write(self.npm_path(tv), include_str!("assets/node_npm_shim"))?;
        file::make_executable(&self.npm_path(tv))?;
        Ok(())
    }

    fn enable_default_corepack_shims(&self, tv: &ToolVersion, pr: &dyn SingleReport) -> Result<()> {
        pr.set_message("enabling corepack shims".into());
        let corepack = self.corepack_path(tv);
        CmdLineRunner::new(corepack)
            .with_pr(pr)
            .arg("enable")
            .env("PATH", CorePlugin::path_env_with_tv_path(tv)?)
            .execute()?;
        Ok(())
    }

    fn test_node(&self, config: &Config, tv: &ToolVersion, pr: &dyn SingleReport) -> Result<()> {
        pr.set_message("node -v".into());
        CmdLineRunner::new(self.node_path(tv))
            .with_pr(pr)
            .arg("-v")
            .envs(config.env()?)
            .execute()
    }

    fn test_npm(&self, config: &Config, tv: &ToolVersion, pr: &dyn SingleReport) -> Result<()> {
        pr.set_message("npm -v".into());
        CmdLineRunner::new(self.npm_path(tv))
            .env("PATH", CorePlugin::path_env_with_tv_path(tv)?)
            .with_pr(pr)
            .arg("-v")
            .envs(config.env()?)
            .execute()
    }

    fn shasums_url(&self, v: &str) -> Result<Url> {
        // let url = MISE_NODE_MIRROR_URL.join(&format!("v{v}/SHASUMS256.txt.asc"))?;
        let url = MISE_NODE_MIRROR_URL.join(&format!("v{v}/SHASUMS256.txt"))?;
        Ok(url)
    }
}

impl Forge for NodePlugin {
    fn fa(&self) -> &ForgeArg {
        &self.core.fa
    }

    fn _list_remote_versions(&self) -> Result<Vec<String>> {
        self.core
            .remote_version_cache
            .get_or_try_init(|| self.fetch_remote_versions())
            .cloned()
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
            ("lts-argon", "4"),
            ("lts-boron", "6"),
            ("lts-carbon", "8"),
            ("lts-dubnium", "10"),
            ("lts-erbium", "12"),
            ("lts-fermium", "14"),
            ("lts-gallium", "16"),
            ("lts-hydrogen", "18"),
            ("lts-iron", "20"),
            ("lts", "20"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        Ok(aliases)
    }

    fn legacy_filenames(&self) -> Result<Vec<String>> {
        Ok(vec![".node-version".into(), ".nvmrc".into()])
    }

    fn parse_legacy_file(&self, path: &Path) -> Result<String> {
        let body = file::read_to_string(path)?;
        // strip comments
        let body = body.split('#').next().unwrap_or_default().to_string();
        // trim "v" prefix
        let body = body.trim().strip_prefix('v').unwrap_or(&body);
        // replace lts/* with lts
        let body = body.replace("lts/*", "lts");
        Ok(body)
    }

    fn install_version_impl(&self, ctx: &InstallContext) -> Result<()> {
        let config = Config::get();
        let settings = Settings::get();
        let opts = BuildOpts::new(ctx)?;
        debug!("node build opts: {:#?}", opts);
        if settings.node_compile {
            self.install_compiled(ctx, &opts)?;
        } else {
            self.install_precompiled(ctx, &opts)?;
        }
        self.test_node(&config, &ctx.tv, ctx.pr.as_ref())?;
        self.install_npm_shim(&ctx.tv)?;
        self.test_npm(&config, &ctx.tv, ctx.pr.as_ref())?;
        self.install_default_packages(&config, &ctx.tv, ctx.pr.as_ref())?;
        if *env::MISE_NODE_COREPACK && self.corepack_path(&ctx.tv).exists() {
            self.enable_default_corepack_shims(&ctx.tv, ctx.pr.as_ref())?;
        }

        Ok(())
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
    fn new(ctx: &InstallContext) -> Result<Self> {
        let v = &ctx.tv.version;
        let install_path = ctx.tv.install_path();
        let source_tarball_name = format!("node-v{v}.tar.gz");
        let binary_tarball_name = format!("node-v{v}-{}-{}.tar.gz", os(), arch());

        Ok(Self {
            version: v.clone(),
            path: ctx.ts.list_paths(),
            build_dir: env::MISE_TMP_DIR.join(format!("node-v{v}")),
            configure_cmd: configure_cmd(&install_path),
            make_cmd: make_cmd(),
            make_install_cmd: make_install_cmd(),
            source_tarball_path: ctx.tv.download_path().join(&source_tarball_name),
            source_tarball_url: env::MISE_NODE_MIRROR_URL
                .join(&format!("v{v}/{source_tarball_name}"))?,
            source_tarball_name,
            binary_tarball_path: ctx.tv.download_path().join(&binary_tarball_name),
            binary_tarball_url: env::MISE_NODE_MIRROR_URL
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
        configure_cmd.push_str(&format!(" {}", opts));
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

fn arch() -> &'static str {
    if cfg!(target_arch = "x86") {
        "x86"
    } else if cfg!(target_arch = "x86_64") {
        "x64"
    } else if cfg!(target_arch = "arm") {
        "armv7l"
    } else if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        built_info::CFG_TARGET_ARCH
    }
}

#[derive(Debug, Deserialize)]
struct NodeVersion {
    version: String,
}
