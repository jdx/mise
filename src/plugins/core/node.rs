use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use color_eyre::eyre::Result;
use serde_derive::Deserialize;
use tempfile::tempdir_in;
use url::Url;

use crate::build_time::built_info;
use crate::cmd::CmdLineRunner;
use crate::config::{Config, Settings};
use crate::env::{RTX_FETCH_REMOTE_VERSIONS_TIMEOUT, RTX_NODE_MIRROR_URL};
use crate::install_context::InstallContext;
use crate::plugins::core::CorePlugin;
use crate::plugins::Plugin;
use crate::toolset::ToolVersion;
use crate::ui::progress_report::ProgressReport;
use crate::{env, file, hash, http};

#[derive(Debug)]
pub struct NodePlugin {
    core: CorePlugin,
    http: http::Client,
}

impl NodePlugin {
    pub fn new() -> Self {
        Self {
            core: CorePlugin::new("node"),
            http: http::Client::new_with_timeout(*RTX_FETCH_REMOTE_VERSIONS_TIMEOUT).unwrap(),
        }
    }

    fn fetch_remote_versions(&self) -> Result<Vec<String>> {
        let versions = self
            .http
            .json::<Vec<NodeVersion>, _>(RTX_NODE_MIRROR_URL.join("index.json")?)?
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
            &ctx.pr,
            &opts.binary_tarball_url,
            &opts.binary_tarball_path,
            &opts.version,
        ) {
            Err(e) if matches!(http::error_code(&e), Some(404)) => {
                debug!("precompiled node not found");
                self.install_compiled(ctx, opts)
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
            &ctx.pr,
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
        pr: &ProgressReport,
        url: &Url,
        local: &Path,
        version: &str,
    ) -> Result<()> {
        let tarball_name = local.file_name().unwrap().to_string_lossy().to_string();
        if local.exists() {
            pr.set_message(format!("using previously downloaded {tarball_name}"));
        } else {
            pr.set_message(format!("downloading {tarball_name}"));
            self.http.download_file(url.clone(), local)?;
        }
        if *env::RTX_NODE_VERIFY {
            pr.set_message(format!("verifying {tarball_name}"));
            self.verify(local, version)?;
        }
        Ok(())
    }

    fn sh<'a>(&'a self, ctx: &'a InstallContext, opts: &BuildOpts) -> CmdLineRunner {
        let mut cmd = CmdLineRunner::new(&ctx.config.settings, "sh");
        for p in &opts.path {
            cmd.prepend_path_env(p.clone());
        }
        cmd = cmd.with_pr(&ctx.pr).current_dir(&opts.build_dir).arg("-c");
        if let Some(cflags) = &*env::RTX_NODE_CFLAGS {
            cmd = cmd.env("CFLAGS", cflags);
        }
        cmd
    }

    fn exec_configure(&self, ctx: &InstallContext, opts: &BuildOpts) -> Result<()> {
        self.sh(ctx, opts).arg(&opts.configure_cmd).execute()
    }
    fn exec_make(&self, ctx: &InstallContext, opts: &BuildOpts) -> Result<()> {
        self.sh(ctx, opts).arg(&opts.make_cmd).execute()
    }
    fn exec_make_install(&self, ctx: &InstallContext, opts: &BuildOpts) -> Result<()> {
        self.sh(ctx, opts).arg(&opts.make_install_cmd).execute()
    }

    fn verify(&self, tarball: &Path, version: &str) -> Result<()> {
        let tarball_name = tarball.file_name().unwrap().to_string_lossy().to_string();
        // TODO: verify gpg signature
        let shasums = self.http.get_text(self.shasums_url(version)?)?;
        let shasums = hash::parse_shasums(&shasums);
        let shasum = shasums.get(&tarball_name).unwrap();
        hash::ensure_checksum_sha256(tarball, shasum)
    }

    fn node_path(&self, tv: &ToolVersion) -> PathBuf {
        tv.install_path().join("bin/node")
    }

    fn npm_path(&self, tv: &ToolVersion) -> PathBuf {
        tv.install_path().join("bin/npm")
    }

    fn install_default_packages(
        &self,
        config: &Config,
        tv: &ToolVersion,
        pr: &ProgressReport,
    ) -> Result<()> {
        let body = file::read_to_string(&*env::RTX_NODE_DEFAULT_PACKAGES_FILE).unwrap_or_default();
        for package in body.lines() {
            let package = package.split('#').next().unwrap_or_default().trim();
            if package.is_empty() {
                continue;
            }
            pr.set_message(format!("installing default package: {}", package));
            let npm = self.npm_path(tv);
            CmdLineRunner::new(&config.settings, npm)
                .with_pr(pr)
                .arg("install")
                .arg("--global")
                .arg(package)
                .envs(&config.env)
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

    fn test_node(&self, config: &Config, tv: &ToolVersion, pr: &ProgressReport) -> Result<()> {
        pr.set_message("node -v");
        CmdLineRunner::new(&config.settings, self.node_path(tv))
            .with_pr(pr)
            .arg("-v")
            .envs(&config.env)
            .execute()
    }

    fn test_npm(&self, config: &Config, tv: &ToolVersion, pr: &ProgressReport) -> Result<()> {
        pr.set_message("npm -v");
        CmdLineRunner::new(&config.settings, self.npm_path(tv))
            .env("PATH", CorePlugin::path_env_with_tv_path(tv)?)
            .with_pr(pr)
            .arg("-v")
            .envs(&config.env)
            .execute()
    }

    fn shasums_url(&self, v: &str) -> Result<Url> {
        // let url = RTX_NODE_MIRROR_URL.join(&format!("v{v}/SHASUMS256.txt.asc"))?;
        let url = RTX_NODE_MIRROR_URL.join(&format!("v{v}/SHASUMS256.txt"))?;
        Ok(url)
    }
}

impl Plugin for NodePlugin {
    fn name(&self) -> &str {
        "node"
    }

    fn list_remote_versions(&self, _settings: &Settings) -> Result<Vec<String>> {
        self.core
            .remote_version_cache
            .get_or_try_init(|| self.fetch_remote_versions())
            .cloned()
    }

    fn get_aliases(&self, _settings: &Settings) -> Result<BTreeMap<String, String>> {
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

    fn legacy_filenames(&self, _settings: &Settings) -> Result<Vec<String>> {
        Ok(vec![".node-version".into(), ".nvmrc".into()])
    }

    fn parse_legacy_file(&self, path: &Path, _settings: &Settings) -> Result<String> {
        let body = file::read_to_string(path)?;
        // trim "v" prefix
        let body = body.trim().strip_prefix('v').unwrap_or(&body);
        // replace lts/* with lts
        let body = body.replace("lts/*", "lts");
        Ok(body.to_string())
    }

    fn install_version_impl(&self, ctx: &InstallContext) -> Result<()> {
        let opts = BuildOpts::new(ctx)?;
        debug!("node build opts: {:#?}", opts);
        if *env::RTX_NODE_COMPILE {
            self.install_compiled(ctx, &opts)?;
        } else {
            self.install_precompiled(ctx, &opts)?;
        }
        self.test_node(ctx.config, &ctx.tv, &ctx.pr)?;
        self.install_npm_shim(&ctx.tv)?;
        self.test_npm(ctx.config, &ctx.tv, &ctx.pr)?;
        self.install_default_packages(ctx.config, &ctx.tv, &ctx.pr)?;
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
            path: ctx.ts.list_paths(ctx.config),
            build_dir: env::RTX_TMP_DIR.join(format!("node-v{v}")),
            configure_cmd: configure_cmd(&install_path),
            make_cmd: make_cmd(),
            make_install_cmd: make_install_cmd(),
            source_tarball_path: ctx.tv.download_path().join(&source_tarball_name),
            source_tarball_url: env::RTX_NODE_MIRROR_URL
                .join(&format!("v{v}/{source_tarball_name}"))?,
            source_tarball_name,
            binary_tarball_path: ctx.tv.download_path().join(&binary_tarball_name),
            binary_tarball_url: env::RTX_NODE_MIRROR_URL
                .join(&format!("v{v}/{binary_tarball_name}"))?,
            binary_tarball_name,
            install_path,
        })
    }
}

fn configure_cmd(install_path: &Path) -> String {
    let mut configure_cmd = format!("./configure --prefix={}", install_path.display());
    if *env::RTX_NODE_NINJA {
        configure_cmd.push_str(" --ninja");
    }
    if let Some(opts) = &*env::RTX_NODE_CONFIGURE_OPTS {
        configure_cmd.push_str(&format!(" {}", opts));
    }
    configure_cmd
}

fn make_cmd() -> String {
    let mut make_cmd = env::RTX_NODE_MAKE.to_string();
    if let Some(concurrency) = *env::RTX_NODE_CONCURRENCY {
        make_cmd.push_str(&format!(" -j{concurrency}"));
    }
    if let Some(opts) = &*env::RTX_NODE_MAKE_OPTS {
        make_cmd.push_str(&format!(" {opts}"));
    }
    make_cmd
}

fn make_install_cmd() -> String {
    let mut make_install_cmd = format!("{} install", &*env::RTX_NODE_MAKE);
    if let Some(opts) = &*env::RTX_NODE_MAKE_INSTALL_OPTS {
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
