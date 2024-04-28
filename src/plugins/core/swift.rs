use std::io::Read;
use std::path::{Path, PathBuf};

use eyre::Result;
use itertools::Itertools;
use versions::Versioning;

use crate::cli::args::ForgeArg;
use crate::cli::version::{ARCH, OS};
use crate::cmd::CmdLineRunner;
use crate::file;
use crate::forge::Forge;
use crate::http::{HTTP, HTTP_FETCH};
use crate::install_context::InstallContext;
use crate::toolset::{ToolVersion, ToolVersionRequest};
use crate::ui::progress_report::SingleReport;
use crate::{github::GithubRelease, plugins::core::CorePlugin};

#[derive(Debug)]
pub struct SwiftPlugin {
    core: CorePlugin,
}

impl SwiftPlugin {
    pub fn new() -> Self {
        let core = CorePlugin::new("swift");
        Self { core }
    }

    fn fetch_remote_versions(&self) -> Result<Vec<String>> {
        match self.core.fetch_remote_versions_from_mise() {
            Ok(Some(versions)) => return Ok(versions),
            Ok(None) => {}
            Err(e) => warn!("failed to fetch remote versions: {}", e),
        }
        let releases: Vec<GithubRelease> =
            HTTP_FETCH.json("https://api.github.com/repos/apple/swift/releases?per_page=100")?;

        let versions = releases
            .into_iter()
            .map(|r| r.tag_name)
            .filter_map(|v| v.strip_prefix("swift-").map(|v| v.to_string()))
            .filter_map(|v| v.strip_suffix("-RELEASE").map(|v| v.to_string()))
            .unique()
            .sorted_by_cached_key(|s| (Versioning::new(s), s.to_string()))
            .collect();
        Ok(versions)
    }

    fn swift_bin(&self, tv: &ToolVersion) -> PathBuf {
        tv.install_path().join("bin/swift")
    }

    fn test_swift(&self, ctx: &InstallContext) -> Result<()> {
        ctx.pr.set_message("swift -v".into());
        CmdLineRunner::new(self.swift_bin(&ctx.tv))
            .with_pr(ctx.pr.as_ref())
            .arg("-v")
            .execute()
    }

    fn download_url(&self, tv: &ToolVersion) -> String {
        // URL Convention
        // Unix:
        //  https://download.swift.org/swift-{swift-version}-release/{dist}{dist-version}/swift-{swift-version}-RELEASE/swift-{swift-version}-RELEASE-{dist}{dist-version}{arch}.tar.gz
        //  Notes:
        //    * The first {dist-version} has dots removed
        //    * When the architecture is x86_64, the {arch} is omitted
        // macOS:
        //  https://download.swift.org/swift-{swift-version}-release/xcode/swift-{swift-version}-RELEASE/swift-{swift-version}-RELEASE-osx.pkg
        //       Example: https://download.swift.org/swift-5.9.2-release/xcode/swift-5.9.2-RELEASE/swift-5.9.2-RELEASE-osx.pkg
        //  Notes:
        //    * It's distributed as a pkg installer
        let os = os();
        format!(
            "https://download.swift.org/swift-{}-release/{}/swift-{}-RELEASE/swift-{}-RELEASE-{}{}.{}",
            tv.version,
            match os.as_str() {
                "osx" => "xcode".to_string(), // Apple uses "xcode" instead of "osx" for this path segment
                os => os.replace(".", "").to_string(),
            },
            tv.version,
            tv.version,
            os,
            match os.as_str() {
                "osx" => "",
                _ => arch(),
            },
            match os.as_str() {
                "osx" => "pkg",
                _ => "tar.gz",
            }
        )
    }

    fn download(&self, tv: &ToolVersion, pr: &dyn SingleReport) -> Result<PathBuf> {
        let url = self.download_url(&tv);
        let filename = url.split('/').last().unwrap();
        let download_path = tv.download_path().join(filename);
        pr.set_message(format!("downloading {filename}"));
        HTTP.download_file(&url, &download_path, Some(pr))?;
        Ok(download_path)
    }

    fn install(&self, ctx: &InstallContext, download_path: &Path) -> Result<()> {
        let filename = download_path.file_name().unwrap().to_string_lossy();
        ctx.pr.set_message(format!("installing {filename}"));
        file::remove_all(ctx.tv.install_path())?;
        let bin_directory = ctx.tv.install_path().join("bin");
        file::create_dir_all(&bin_directory)?;

        if cfg!(target_os = "macos") {
            CmdLineRunner::new("/usr/sbin/installer")
            .with_pr(ctx.pr.as_ref())
            .arg("-pkg")
            .arg(download_path)
            .arg("-target")
            .arg("CurrentUserHomeDirectory")
            .execute()?;
        } else {
            file::unzip(download_path, &ctx.tv.download_path())?;
            // ASK: Given the following directory structure, where do we place those directories?
            // usr/
            //   bin/
            //   include/
            //   lib/
            //   libexec/
            //   share/
            //   local/
        }
        Ok(())
    }

    fn verify(&self, ctx: &InstallContext) -> Result<()> {
        self.test_swift(ctx)
    }
}

impl Forge for SwiftPlugin {
    fn fa(&self) -> &ForgeArg {
        &self.core.fa
    }

    fn list_remote_versions(&self) -> Result<Vec<String>> {
        self.core
            .remote_version_cache
            .get_or_try_init(|| self.fetch_remote_versions())
            .cloned()
    }

    fn legacy_filenames(&self) -> Result<Vec<String>> {
        Ok(vec![".swift-version".into()])
    }

    fn install_version_impl(&self, ctx: &InstallContext) -> Result<()> {
        assert!(matches!(
            &ctx.tv.request,
            ToolVersionRequest::Version { .. }
        ));

        let download_path = self.download(&ctx.tv, ctx.pr.as_ref())?;
        self.install(ctx, &download_path)?;
        self.verify(ctx)?;

        Ok(())
    }
}

fn linux_distribution() -> String {
    let mut distribution = String::new();
    let mut version = String::new();
    if let Ok(mut file) = std::fs::File::open("/etc/os-release") {
        let mut contents = String::new();
        file.read_to_string(&mut contents)
            .expect("Failed to read /etc/os-release");
        for line in contents.lines() {
            if line.starts_with("ID=") {
                let parts: Vec<&str> = line.split('=').collect();
                if parts.len() > 1 {
                    distribution = parts[1].trim_matches('"').to_lowercase();
                }
            } else if line.starts_with("VERSION_ID=") {
                let parts: Vec<&str> = line.split('=').collect();
                if parts.len() > 1 {
                    version = parts[1].trim_matches('"').to_string();
                }
            }
        }
    }
    if distribution == "amzn" {
        return format!("amazonlinux{}", version);
    } else {
        format!("{}{}", distribution, version)
    }
}

fn os() -> String {
    // ASK: Is it ok if unlike other plugins, we return a String here instead of a static str?
    if cfg!(target_os = "macos") {
        "osx".to_string()
    } else if cfg!(target_os = "linux") {
        linux_distribution()
    } else {
        OS.to_string()
    }
}

fn arch() -> &'static str {
    if cfg!(target_arch = "x86_64") || cfg!(target_arch = "amd64") {
        ""
    } else if cfg!(target_arch = "aarch64") || cfg!(target_arch = "arm64") {
        "arm64"
    } else {
        &ARCH
    }
}
