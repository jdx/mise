use crate::backend::Backend;
use crate::cli::args::BackendArg;
use crate::cmd::CmdLineRunner;
use crate::config::SETTINGS;
use crate::http::HTTP;
use crate::install_context::InstallContext;
use crate::toolset::ToolVersion;
use crate::ui::progress_report::SingleReport;
use crate::{env, file, github, gpg, plugins};
use eyre::Result;
use std::path::{Path, PathBuf};
use tempfile::tempdir_in;

#[derive(Debug)]
pub struct SwiftPlugin {
    ba: BackendArg,
}

impl SwiftPlugin {
    pub fn new() -> Self {
        Self {
            ba: plugins::core::new_backend_arg("swift"),
        }
    }

    fn swift_bin(&self, tv: &ToolVersion) -> PathBuf {
        tv.install_path().join("bin").join(swift_bin_name())
    }

    fn test_swift(&self, ctx: &InstallContext, tv: &ToolVersion) -> Result<()> {
        ctx.pr.set_message("swift --version".into());
        CmdLineRunner::new(self.swift_bin(tv))
            .with_pr(ctx.pr.as_ref())
            .arg("--version")
            .execute()
    }

    fn download(&self, tv: &ToolVersion, pr: &dyn SingleReport) -> Result<PathBuf> {
        let url = format!(
            "https://download.swift.org/swift-{version}-release/{platform_directory}/swift-{version}-RELEASE/swift-{version}-RELEASE-{platform}{architecture}.{extension}",
            version = tv.version,
            platform = platform(),
            platform_directory = platform_directory(),
            extension = extension(),
            architecture = match architecture() {
                Some(arch) => format!("-{arch}"),
                None => "".into(),
            }
        );
        let filename = url.split('/').last().unwrap();
        let tarball_path = tv.download_path().join(filename);
        if !tarball_path.exists() {
            pr.set_message(format!("download {filename}"));
            HTTP.download_file(&url, &tarball_path, Some(pr))?;
        }

        Ok(tarball_path)
    }

    fn install(&self, ctx: &InstallContext, tv: &ToolVersion, tarball_path: &Path) -> Result<()> {
        SETTINGS.ensure_experimental("swift")?;
        let filename = tarball_path.file_name().unwrap().to_string_lossy();
        let version = &tv.version;
        ctx.pr.set_message(format!("extract {filename}"));
        if cfg!(macos) {
            let tmp = {
                tempdir_in(tv.install_path().parent().unwrap())?
                    .path()
                    .to_path_buf()
            };
            CmdLineRunner::new("pkgutil")
                .arg("--expand-full")
                .arg(tarball_path)
                .arg(&tmp)
                .with_pr(ctx.pr.as_ref())
                .execute()?;
            file::remove_all(tv.install_path())?;
            file::rename(
                tmp.join(format!("swift-{version}-RELEASE-osx-package.pkg"))
                    .join("Payload"),
                tv.install_path(),
            )?;
        } else if cfg!(windows) {
            todo!("install from exe");
        } else {
            file::untar(
                tarball_path,
                &tv.install_path(),
                &file::TarOptions {
                    format: file::TarFormat::TarGz,
                    pr: Some(ctx.pr.as_ref()),
                    strip_components: 1,
                },
            )?;
        }
        Ok(())
    }

    fn symlink_bins(&self, tv: &ToolVersion) -> Result<()> {
        let usr_bin = tv.install_path().join("usr").join("bin");
        let bin_dir = tv.install_path().join("bin");
        file::create_dir_all(&bin_dir)?;
        for bin in file::ls(&usr_bin)? {
            if !file::is_executable(&bin) {
                continue;
            }
            let file_name = bin.file_name().unwrap().to_string_lossy().to_string();
            if file_name.contains("swift") || file_name.contains("sourcekit") {
                file::make_symlink_or_copy(&bin, &bin_dir.join(file_name))?;
            }
        }
        Ok(())
    }

    fn verify_gpg(
        &self,
        ctx: &InstallContext,
        tv: &ToolVersion,
        tarball_path: &Path,
    ) -> Result<()> {
        if file::which_non_pristine("gpg").is_none() && SETTINGS.swift.gpg_verify.is_none() {
            ctx.pr
                .println("gpg not found, skipping verification".to_string());
            return Ok(());
        }
        gpg::add_keys_swift(ctx)?;
        let sig_path = PathBuf::from(format!("{}.sig", tarball_path.to_string_lossy()));
        HTTP.download_file(format!("{}.sig", url(tv)), &sig_path, Some(ctx.pr.as_ref()))?;
        self.gpg(ctx)
            .arg("--quiet")
            .arg("--trust-model")
            .arg("always")
            .arg("--verify")
            .arg(&sig_path)
            .arg(tarball_path)
            .execute()?;
        Ok(())
    }

    fn verify(&self, ctx: &InstallContext, tv: &ToolVersion) -> Result<()> {
        self.test_swift(ctx, tv)
    }

    fn gpg<'a>(&self, ctx: &'a InstallContext) -> CmdLineRunner<'a> {
        CmdLineRunner::new("gpg").with_pr(ctx.pr.as_ref())
    }
}

impl Backend for SwiftPlugin {
    fn ba(&self) -> &BackendArg {
        &self.ba
    }

    fn _list_remote_versions(&self) -> Result<Vec<String>> {
        let versions = github::list_releases("swiftlang/swift")?
            .into_iter()
            .map(|r| r.tag_name)
            .filter_map(|v| v.strip_prefix("swift-").map(|v| v.to_string()))
            .filter_map(|v| v.strip_suffix("-RELEASE").map(|v| v.to_string()))
            .rev()
            .collect();
        Ok(versions)
    }

    fn idiomatic_filenames(&self) -> Result<Vec<String>> {
        if SETTINGS.experimental {
            Ok(vec![".swift-version".into()])
        } else {
            Ok(vec![])
        }
    }

    fn install_version_(&self, ctx: &InstallContext, mut tv: ToolVersion) -> Result<ToolVersion> {
        let tarball_path = self.download(&tv, ctx.pr.as_ref())?;
        if cfg!(target_os = "linux") && SETTINGS.swift.gpg_verify != Some(false) {
            self.verify_gpg(ctx, &tv, &tarball_path)?;
        }
        self.verify_checksum(ctx, &mut tv, &tarball_path)?;
        self.install(ctx, &tv, &tarball_path)?;
        self.symlink_bins(&tv)?;
        self.verify(ctx, &tv)?;

        Ok(tv)
    }
}

fn swift_bin_name() -> &'static str {
    if cfg!(windows) {
        "swift.exe"
    } else {
        "swift"
    }
}

fn platform_directory() -> String {
    if cfg!(macos) {
        "xcode".into()
    } else if cfg!(windows) {
        "windows10".into()
    } else {
        platform().replace(".", "")
    }
}

fn platform() -> String {
    if cfg!(macos) {
        "osx".to_string()
    } else if cfg!(windows) {
        "windows10".to_string()
    } else if let Ok(os_release) = &*os_release::OS_RELEASE {
        if os_release.id == "amzn" {
            format!("amazonlinux{}", os_release.version_id)
        } else {
            format!("{}{}", os_release.id, os_release.version_id)
        }
    } else {
        "ubi".to_string()
    }
}

fn extension() -> &'static str {
    if cfg!(macos) {
        "pkg"
    } else if cfg!(windows) {
        "exe"
    } else {
        "tar.gz"
    }
}

fn architecture() -> Option<&'static str> {
    if cfg!(target_os = "linux") && !cfg!(target_arch = "x86_64") {
        return Some(env::consts::ARCH);
    } else if cfg!(windows) && cfg!(target_arch = "aarch64") {
        return Some("arm64");
    }
    None
}

fn url(tv: &ToolVersion) -> String {
    format!(
    "https://download.swift.org/swift-{version}-release/{platform_directory}/swift-{version}-RELEASE/swift-{version}-RELEASE-{platform}{architecture}.{extension}",
    version = tv.version,
    platform = platform(),
    platform_directory = platform_directory(),
    extension = extension(),
    architecture = match architecture() {
        Some(arch) => format!("-{arch}"),
        None => "".into(),
    }
)
}
