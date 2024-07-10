use std::path::{Path, PathBuf};

use contracts::requires;
use eyre::Result;
use itertools::Itertools;
use versions::Versioning;

use crate::backend::Backend;
use crate::cli::args::BackendArg;
use crate::cmd::CmdLineRunner;
use crate::file;
use crate::github::GithubTag;
use crate::http::{HTTP, HTTP_FETCH};
use crate::install_context::InstallContext;
use crate::plugins::core::CorePlugin;
use crate::toolset::{ToolRequest, ToolVersion};
use crate::ui::progress_report::SingleReport;

#[derive(Debug)]
pub struct LuajitPlugin {
    core: CorePlugin,
}

impl LuajitPlugin {
    pub fn new() -> Self {
        let core = CorePlugin::new("luajit".into());
        Self { core }
    }

    fn luajit_bin(&self, tv: &ToolVersion) -> PathBuf {
        tv.install_path().join("bin/luajit")
    }

    fn test_luajit(&self, ctx: &InstallContext) -> Result<()> {
        ctx.pr.set_message("luajit -v".into());
        CmdLineRunner::new(self.luajit_bin(&ctx.tv))
            .with_pr(ctx.pr.as_ref())
            .arg("-v")
            .execute()
    }

    fn fetch_remote_versions(&self) -> Result<Vec<String>> {
        match self.core.fetch_remote_versions_from_mise() {
            Ok(Some(versions)) => return Ok(versions),
            Ok(None) => {}
            Err(e) => warn!("failed to fetch remote versions: {}", e),
        }

        let tags: Vec<GithubTag> =
            HTTP_FETCH.json("https://api.github.com/repos/LuaJIT/LuaJIT/tags?per_page=100")?;
        let versions = tags
            .into_iter()
            .map(|r| r.name)
            .unique()
            .sorted_by_cached_key(|s| (Versioning::new(s), s.to_string()))
            .collect();
        Ok(versions)
    }

    fn download(&self, tv: &ToolVersion, pr: &dyn SingleReport) -> Result<PathBuf> {
        let vers = match tv.request {
            ToolRequest::Version { ref version, .. } => version,
            ToolRequest::Ref { ref ref_, .. } => ref_,
            _ => unimplemented!("unsupported luajit tool request"),
        };

        let url = format!("https://github.com/LuaJIT/LuaJIT/archive/{}.tar.gz", vers,);

        let filename = url.split('/').last().unwrap();
        let tarball_path = tv.download_path().join(filename);

        pr.set_message(format!("downloading {filename}"));
        HTTP.download_file(&url, &tarball_path, Some(pr))?;

        Ok(tarball_path)
    }

    fn macos_version(&self) -> Result<String> {
        let output = std::process::Command::new("sw_vers")
            .arg("--productVersion")
            .output()?;

        if !output.status.success() {
            let err = String::from_utf8_lossy(&output.stderr);
            return Err(eyre::eyre!("Failed to get macOS version: {}", err));
        }

        Ok(String::from_utf8_lossy(&output.stdout)
            .trim_end()
            .to_owned())
    }

    fn ldflags(&self) -> Option<String> {
        if std::env::consts::ARCH == "aarch64" && std::env::consts::OS == "macos" {
            // preserve existing ldflags
            let ldflags = std::env::var("LDFLAGS").unwrap_or_default();
            // help the FFI module find Homebrew-installed libraries
            let ldflags = ldflags + " -Wl,-rpath,/opt/homebrew/lib";
            Some(ldflags)
        } else {
            None
        }
    }

    fn install(&self, ctx: &InstallContext, tarball_path: &Path) -> Result<()> {
        let filename = tarball_path.file_name().unwrap().to_string_lossy();
        ctx.pr.set_message(format!("installing {filename}"));
        file::remove_all(ctx.tv.install_path())?;
        let source_dir = untar_xy(tarball_path, &ctx.tv.download_path())?;

        let prefix_arg = format!("PREFIX={}", ctx.tv.install_path().display());

        ctx.pr.set_message(String::from("building luajit"));
        let mut cmd = CmdLineRunner::new("make")
            .with_pr(&*ctx.pr)
            .arg("amalg")
            .arg(&prefix_arg)
            .current_dir(&source_dir);
        if cfg!(target_os = "macos") {
            let target = self.macos_version()?;
            cmd = cmd.env("MACOSX_DEPLOYMENT_TARGET", target);
            if let Some(ldflags) = self.ldflags() {
                cmd = cmd.env("LDFLAGS", ldflags);
            }
        }
        cmd.execute()?;

        ctx.pr.set_message(format!("installing luajit"));
        let mut cmd = CmdLineRunner::new("make")
            .with_pr(&*ctx.pr)
            .arg("install")
            .arg(prefix_arg)
            .current_dir(&source_dir);
        if cfg!(target_os = "macos") {
            let target = self.macos_version()?;
            cmd = cmd.env("MACOSX_DEPLOYMENT_TARGET", target);
            if let Some(ldflags) = self.ldflags() {
                cmd = cmd.env("LDFLAGS", ldflags);
            }
        }
        cmd.execute()?;

        Ok(())
    }

    fn verify(&self, ctx: &InstallContext) -> Result<()> {
        self.test_luajit(ctx)
    }
}

impl Backend for LuajitPlugin {
    fn fa(&self) -> &BackendArg {
        &self.core.fa
    }

    fn _list_remote_versions(&self) -> Result<Vec<String>> {
        self.core
            .remote_version_cache
            .get_or_try_init(|| self.fetch_remote_versions())
            .cloned()
    }

    // #[requires(matches ! (ctx.tv.request, ToolRequest::Version { .. } | ToolRequest::Prefix { .. } | ToolRequest::Ref { .. }), "unsupported tool version request type")]
    #[requires(matches ! (ctx.tv.request, ToolRequest::Version { .. } | ToolRequest::Ref { .. }), "unsupported tool version request type")]
    fn install_version_impl(&self, ctx: &InstallContext) -> Result<()> {
        let tarball_path = self.download(&ctx.tv, ctx.pr.as_ref())?;
        self.install(ctx, &tarball_path)?;
        self.verify(ctx)?;
        Ok(())
    }
}

pub fn untar_xy(archive: &Path, dest: &Path) -> Result<PathBuf> {
    file::untar(archive, dest)?;

    // find top level source tree produced from tarball
    let Ok(paths) = std::fs::read_dir(dest) else {
        return Err(eyre::eyre!(
            "Failed to find source tree in download path: {}",
            dest.display()
        ));
    };
    let mut source_dir = None;
    for path in paths {
        if let Ok(entry) = path {
            if let Ok(ty) = entry.file_type() {
                if ty.is_dir() {
                    source_dir = Some(entry.path());
                    break;
                }
            }
        };
    }
    let Some(source_dir) = source_dir else {
        return Err(eyre::eyre!(
            "Failed to find source tree in download path: {}",
            dest.display()
        ));
    };

    Ok(source_dir)
}
