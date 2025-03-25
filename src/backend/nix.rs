use std::path::PathBuf;

use crate::cli::args::BackendArg;

use crate::backend::Backend;

use crate::backend::backend_type::BackendType;
use crate::cmd::CmdLineRunner;
use crate::config::SETTINGS;
use crate::http::HTTP_FETCH;

use crate::toolset::ToolVersion;


#[derive(Debug)]
pub struct NixBackend {
    ba: BackendArg,
}

impl Backend for NixBackend {
    fn get_type(&self) -> BackendType {
        BackendType::Nix
    }

    fn ba(&self) -> &BackendArg {
        &self.ba
    }

    fn get_dependencies(&self) -> eyre::Result<Vec<&str>> {
        Ok(vec!["nix"])
    }

    fn _list_remote_versions(&self) -> eyre::Result<Vec<String>> {
        self.nixhub_versions(self.tool_name())
    }

    fn list_bin_paths(&self, tv: &ToolVersion) -> eyre::Result<Vec<PathBuf>> {
        let bin = tv.install_path().join("profile").join("bin");
        Ok(vec![bin])
    }

    fn install_version_(
        &self,
        ctx: &crate::install_context::InstallContext,
        tv: ToolVersion,
    ) -> eyre::Result<ToolVersion> {
        SETTINGS.ensure_experimental("nix backend")?;

        let nix_installable = 
            self.nixhub_installable(self.tool_name(), tv.version.clone())?;

        let profile_path = tv.install_path().join("profile");

        let mut cmd = CmdLineRunner::new("nix")
          .args(&[
            "--experimental-features", 
            "flakes nix-command",
            "profile",
            "install",
            nix_installable.as_str(),
            "--profile"
          ]);

        cmd = cmd.arg(profile_path);

        if !SETTINGS.debug && !SETTINGS.trace {
            cmd = cmd.arg("--quiet");
        }
        if SETTINGS.debug {
            cmd = cmd.arg("--debug");
        }
        if SETTINGS.trace {
            cmd = cmd.arg("-L");
        }

        cmd.with_pr(&ctx.pr).execute()?;

        Ok(tv)
    }
}


impl NixBackend {
    pub fn from_arg(ba: BackendArg) -> Self {
        Self { ba }
    }

    fn nixhub_resource(&self, name: String) -> eyre::Result<NixHubResource> {
        let nixhub_url = "https://search.devbox.sh/v2/pkg";
        let resource: NixHubResource = HTTP_FETCH.json(format!(
            "{}?name={}",
            nixhub_url,
            name,
        ))?;
        return Ok(resource)
    }

    fn nixhub_versions(&self, name: String) -> eyre::Result<Vec<String>> {
        let resource = self.nixhub_resource(name)?;
        let versions = resource.releases.iter().filter_map(|r|
            r.platforms.iter().find_map(|p| 
                if p.arch == ARCH && p.os == OS {
                    Some(r.version.clone())
                } else {
                    None
                }
            )
        ).collect();
        Ok(versions)
    }

    fn nixhub_platform(&self, name: String, version: String) -> eyre::Result<NixHubPlatform> {
        let resource = self.nixhub_resource(name)?;
        resource.releases.iter().find_map(|x| 
            if x.version == version {
                x.platforms.iter().find(|y| y.arch == ARCH && y.os == OS).cloned()
            } else {
                None
            }
        ).ok_or_else(|| eyre::eyre!("No compatible release found"))
    }

    fn nixhub_installable(&self, name: String, version: String) -> eyre::Result<String> {
        let platform = self.nixhub_platform(name.clone(), version.clone())?;
        let installable = format!("nixpkgs/{}#{}", platform.commit_hash, platform.attribute_path);
        Ok(installable)
    }
}

#[derive(serde::Deserialize, Clone)]
struct NixHubPlatform {
    arch: String,
    os: String,
    attribute_path: String,
    commit_hash: String
}

#[derive(serde::Deserialize)]
struct NixHubRelease {
    version: String,
    platforms: Vec<NixHubPlatform>,
}

#[derive(serde::Deserialize)]
struct NixHubResource {
    releases: Vec<NixHubRelease>,
}


#[cfg(target_arch = "x86_64")]
const ARCH: &str = "x86-64";

#[cfg(target_arch = "aarch64")]
const ARCH: &str = "arm64";

#[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
const ARCH: &str = "unsupported";

#[cfg(target_os = "linux")]
const OS: &str = "Linux";

#[cfg(target_os = "macos")]
const OS: &str = "macOS";

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
const OS: &str = "unsupported";