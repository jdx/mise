use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use color_eyre::eyre::Result;
use itertools::Itertools;
use versions::Versioning;

use crate::cli::version::{ARCH, OS};
use crate::cmd::CmdLineRunner;
use crate::http::HTTP;
use crate::install_context::InstallContext;
use crate::plugins::{core::CorePlugin, Plugin};
use crate::toolset::{ToolVersion, ToolVersionRequest};
use crate::ui::progress_report::SingleReport;
use crate::{env, file};

#[derive(Debug)]
pub struct RustPlugin {
    core: CorePlugin,
}

impl RustPlugin {
    pub fn new() -> Self {
        Self {
            core: CorePlugin::new("rust"),
        }
    }

    fn fetch_remote_versions(&self) -> Result<Vec<String>> {
        match self.core.fetch_remote_versions_from_rtx() {
            Ok(Some(versions)) => return Ok(versions),
            Ok(None) => {}
            Err(e) => warn!("failed to fetch remote versions: {}", e),
        }

        CorePlugin::run_fetch_task_with_timeout(|| {
            let repo = &*env::RTX_RUST_REPO;
            let output = cmd!("git", "ls-remote", "--tags", repo).read()?;
            let lines = output.split('\n');

            let versions = lines
                .map(|s| s.split("refs/tags/").last().unwrap_or_default().to_string())
                .filter(|s| !s.is_empty())
                .filter(|s| regex!(r"^1\.[0-9]+\.[0-9]+$").is_match(s))
                .unique()
                .sorted_by_cached_key(|s| (Versioning::new(s), s.to_string()))
                .collect();

            Ok(versions)
        })
    }

    fn rustc_bin(&self, tv: &ToolVersion) -> PathBuf {
        tv.install_path().join("bin/rustc")
    }

    fn test_rustc(&self, ctx: &InstallContext) -> Result<()> {
        if env::RTX_RUST_WITHOUT.contains(&"rustc".into()) {
            return Ok(());
        }

        ctx.pr.set_message("rustc -V".into());

        CmdLineRunner::new(self.rustc_bin(&ctx.tv))
            .with_pr(ctx.pr.as_ref())
            .arg("-V")
            .execute()
    }

    fn download(&self, tv: &ToolVersion, pr: &dyn SingleReport) -> Result<PathBuf> {
        let url = format!(
            "https://static.rust-lang.org/dist/rust-{}-{}-{}.tar.gz",
            tv.version,
            arch(),
            os()
        );

        let filename = url.split('/').last().unwrap();
        let tarball_path = tv.download_path().join(filename);

        pr.set_message(format!("downloading {}", &url));
        HTTP.download_file(&url, &tarball_path)?;

        // TODO: verify GPG signature

        Ok(tarball_path)
    }

    fn install(&self, ctx: &InstallContext, tarball_path: &Path) -> Result<()> {
        ctx.pr
            .set_message(format!("installing {}", tarball_path.display()));

        file::remove_all(ctx.tv.install_path())?;
        file::create_dir_all(ctx.tv.install_path())?;
        file::untar(tarball_path, &ctx.tv.download_path())?;

        cmd!(
            "sh",
            "install.sh",
            format!("--prefix={}", ctx.tv.install_path().display()),
            format!("--without={}", env::RTX_RUST_WITHOUT.join(","))
        )
        .dir(
            ctx.tv
                .download_path()
                .join(format!("rust-{}-{}-{}", ctx.tv.version, arch(), os())),
        )
        .stdout_null()
        .run()?;

        Ok(())
    }

    fn verify(&self, ctx: &InstallContext) -> Result<()> {
        self.test_rustc(ctx)
    }
}

impl Plugin for RustPlugin {
    fn name(&self) -> &str {
        "rust"
    }

    fn list_remote_versions(&self) -> Result<Vec<String>> {
        self.core
            .remote_version_cache
            .get_or_try_init(|| self.fetch_remote_versions())
            .cloned()
    }

    fn get_aliases(&self) -> Result<BTreeMap<String, String>> {
        let aliases = [("stable", "latest")]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();

        Ok(aliases)
    }

    fn install_version_impl(&self, ctx: &InstallContext) -> Result<()> {
        assert!(matches!(
            &ctx.tv.request,
            ToolVersionRequest::Version { .. }
        ));

        let tarball_path = self.download(&ctx.tv, ctx.pr.as_ref())?;
        self.install(ctx, &tarball_path)?;
        self.verify(ctx)?;

        Ok(())
    }
}

fn os() -> &'static str {
    if cfg!(target_os = "macos") {
        "apple-darwin"
    } else if cfg!(target_os = "linux") {
        "unknown-linux-gnu"
    } else if cfg!(target_os = "freebsd") {
        "unknown-freebsd"
    } else if cfg!(target_os = "openbsd") {
        "unknown-openbsd"
    } else if cfg!(target_os = "netbsd") {
        "unknown-netbsd"
    } else {
        &OS
    }
}

fn arch() -> &'static str {
    if cfg!(target_arch = "x86_64") || cfg!(target_arch = "amd64") {
        "x86_64"
    } else if cfg!(target_arch = "aarch64") || cfg!(target_arch = "arm64") {
        "aarch64"
    } else if cfg!(target_arch = "x86")
        || cfg!(target_arch = "i386")
        || cfg!(target_arch = "i486")
        || cfg!(target_arch = "i686")
        || cfg!(target_arch = "i786")
    {
        "i686"
    } else {
        &ARCH
    }
}
