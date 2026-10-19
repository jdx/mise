use std::collections::{BTreeMap, HashMap};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use async_trait::async_trait;
use eyre::Result;
use itertools::Itertools;
use serde::Deserialize;
use versions::Versioning;

use crate::args::BackendArg;
use crate::backend::Backend;
use crate::backend::VersionInfo;
use crate::backend::platform_target::PlatformTarget;
use crate::backend::static_helpers::fetch_checksum_from_file;
use crate::backend::static_helpers::lock_checksum;
use crate::cmd::CmdLineRunner;
use crate::config::Config;
use crate::github::{self, ReleaseVersion};
use crate::http::{HTTP, HTTP_FETCH};
use crate::install_context::InstallContext;
use crate::lockfile::PlatformInfo;
use crate::toolset::{ToolRequest, ToolVersion, Toolset};
use crate::ui::progress_report::SingleReport;
use crate::{file, plugins};

#[derive(Debug)]
pub(super) struct DenoPlugin {
    ba: Arc<BackendArg>,
}

impl DenoPlugin {
    pub(super) fn new() -> Self {
        Self {
            ba: Arc::new(plugins::core::new_backend_arg("deno")),
        }
    }

    fn deno_bin(&self, tv: &ToolVersion) -> PathBuf {
        tv.install_path().join(if cfg!(target_os = "windows") {
            "bin/deno.exe"
        } else {
            "bin/deno"
        })
    }

    fn test_deno(&self, tv: &ToolVersion, pr: &dyn SingleReport) -> Result<()> {
        pr.set_message("deno -V".into());
        CmdLineRunner::new(self.deno_bin(tv))
            .with_pr(pr)
            .arg("-V")
            .env_values(tv.install_env())
            .execute()
    }

    async fn download(&self, tv: &ToolVersion, pr: &dyn SingleReport) -> Result<PathBuf> {
        let url = self
            .get_tarball_url(tv, &PlatformTarget::from_current())
            .await?
            .ok_or_else(|| eyre::eyre!("Failed to get deno tarball URL"))?;
        let filename = url.split('/').next_back().unwrap();
        let tarball_path = tv.download_path().join(filename);

        pr.set_message(format!("download {filename}"));
        HTTP.download_file_stored(
            &url,
            &tarball_path,
            lock_checksum(tv, &self.get_platform_key(), &url).as_deref(),
            Some(pr),
        )
        .await?;

        Ok(tarball_path)
    }

    fn install(&self, tv: &ToolVersion, pr: &dyn SingleReport, tarball_path: &Path) -> Result<()> {
        let filename = tarball_path.file_name().unwrap().to_string_lossy();
        pr.set_message(format!("extract {filename}"));
        file::remove_all(tv.install_path())?;
        file::create_dir_all(tv.install_path().join("bin"))?;
        file::unzip(tarball_path, &tv.download_path(), &Default::default())?;
        file::move_file(
            tv.download_path().join(if cfg!(target_os = "windows") {
                "deno.exe"
            } else {
                "deno"
            }),
            self.deno_bin(tv),
        )?;
        file::make_executable(self.deno_bin(tv))?;
        Ok(())
    }

    fn verify(&self, tv: &ToolVersion, pr: &dyn SingleReport) -> Result<()> {
        self.test_deno(tv, pr)
    }
}

#[async_trait]
impl Backend for DenoPlugin {
    fn ba(&self) -> &Arc<BackendArg> {
        &self.ba
    }

    async fn security_info(&self) -> Vec<crate::backend::SecurityFeature> {
        use crate::backend::SecurityFeature;

        vec![SecurityFeature::Checksum {
            algorithm: Some("sha256".to_string()),
        }]
    }

    async fn _list_remote_versions(&self, _config: &Arc<Config>) -> Result<Vec<VersionInfo>> {
        let versions: DenoVersions = HTTP_FETCH.json("https://deno.com/versions.json").await?;
        // deno.com lists every version but no dates. GitHub's releases carry
        // them; without them `minimum_release_age` could not hold a version back.
        let dates = match deno_release_dates(&versions.cli).await {
            Ok(dates) => dates,
            Err(err) => {
                warn!("could not read deno release dates from GitHub: {err}");
                HashMap::new()
            }
        };
        Ok(deno_versions(versions.cli, &dates))
    }

    async fn install_version_(
        &self,
        ctx: &InstallContext,
        mut tv: ToolVersion,
    ) -> Result<ToolVersion> {
        let tarball_path = self.download(&tv, ctx.pr.as_ref()).await?;
        ctx.pr.next_operation();
        self.verify_checksum(ctx, &mut tv, &tarball_path)?;
        ctx.pr.next_operation();
        self.install(&tv, ctx.pr.as_ref(), &tarball_path)?;
        self.verify(&tv, ctx.pr.as_ref())?;

        Ok(tv)
    }

    async fn list_bin_paths(
        &self,
        _config: &Arc<Config>,
        tv: &ToolVersion,
    ) -> Result<Vec<PathBuf>> {
        if let ToolRequest::System { .. } = tv.request {
            return Ok(vec![]);
        }
        let bin_paths = vec![
            tv.install_path().join("bin"),
            tv.install_path().join(".deno/bin"),
        ];
        Ok(bin_paths)
    }

    async fn exec_env(
        &self,
        _config: &Arc<Config>,
        _ts: &Toolset,
        tv: &ToolVersion,
    ) -> eyre::Result<BTreeMap<String, String>> {
        let map = BTreeMap::from([(
            "DENO_INSTALL_ROOT".into(),
            tv.install_path().join(".deno").to_string_lossy().into(),
        )]);
        Ok(map)
    }

    async fn get_tarball_url(
        &self,
        tv: &ToolVersion,
        target: &PlatformTarget,
    ) -> Result<Option<String>> {
        let arch = match target.arch_name() {
            "x64" => "x86_64",
            "arm64" => "aarch64",
            other => other,
        };
        let os = match target.os_name() {
            "macos" => "apple-darwin",
            "linux" => "unknown-linux-gnu",
            "windows" => "pc-windows-msvc",
            _ => "unknown-linux-gnu",
        };
        Ok(Some(format!(
            "https://dl.deno.land/release/v{}/deno-{}-{}.zip",
            tv.version, arch, os
        )))
    }

    async fn resolve_lock_info(
        &self,
        tv: &ToolVersion,
        target: &PlatformTarget,
    ) -> Result<PlatformInfo> {
        let url = self
            .get_tarball_url(tv, target)
            .await?
            .ok_or_else(|| eyre::eyre!("Failed to get deno tarball URL"))?;

        // Deno provides .sha256sum files alongside each zip
        let checksum_url = format!("{}.sha256sum", url);
        let checksum = fetch_checksum_from_file(&checksum_url, "sha256").await;

        Ok(PlatformInfo {
            url: Some(url),
            checksum,
            size: None,
            url_api: None,
            conda_deps: None,
            ..Default::default()
        })
    }
}

#[derive(Debug, Deserialize)]
struct DenoVersions {
    cli: Vec<String>,
}

/// How many versions missing from the release list get a lookup of their own.
const MAX_MISSING_DATE_LOOKUPS: usize = 5;

/// Dates for the versions deno.com lists (newest first). The release list may
/// be cached from before a new release, which would leave exactly the version
/// a cooldown exists for undated, so the newest versions it lacks are looked
/// up one by one.
async fn deno_release_dates(cli: &[String]) -> Result<HashMap<String, String>> {
    let repo = "denoland/deno";
    let releases = github::list_release_versions(github::API_URL, repo, false).await?;
    let mut dates = release_dates(&releases);
    let missing = cli
        .iter()
        .filter_map(|v| v.strip_prefix('v'))
        .filter(|v| !dates.contains_key(*v))
        .take(MAX_MISSING_DATE_LOOKUPS)
        .map(str::to_string)
        .collect::<Vec<_>>();
    for version in missing {
        match github::get_release(repo, &format!("v{version}")).await {
            Ok(release) => {
                dates.insert(version, release.released_at().to_string());
            }
            Err(err) => debug!("no GitHub release for deno {version}: {err}"),
        }
    }
    Ok(dates)
}

/// Publication date by version, from the `vX.Y.Z` tags of denoland/deno releases.
fn release_dates(releases: &[ReleaseVersion]) -> HashMap<String, String> {
    releases
        .iter()
        .filter_map(|r| {
            let version = r.tag_name.strip_prefix('v')?;
            Some((version.to_string(), r.released_at.clone()?))
        })
        .collect()
}

fn deno_versions(cli: Vec<String>, dates: &HashMap<String, String>) -> Vec<VersionInfo> {
    cli.into_iter()
        .filter(|v| v.starts_with('v'))
        .map(|v| v.trim_start_matches('v').to_string())
        .map(|version| VersionInfo {
            created_at: dates.get(&version).cloned(),
            version,
            ..Default::default()
        })
        .unique_by(|v| v.version.clone())
        .sorted_by_cached_key(|v| (Versioning::new(&v.version), v.version.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::duration::parse_into_timestamp;

    fn release(tag: &str, released_at: Option<&str>) -> ReleaseVersion {
        ReleaseVersion {
            tag_name: tag.to_string(),
            released_at: released_at.map(str::to_string),
            prerelease: false,
            has_assets: true,
        }
    }

    fn dated_versions() -> Vec<VersionInfo> {
        let dates = release_dates(&[
            release("v2.0.0", Some("2024-10-09T13:00:00Z")),
            release("v2.0.1", Some("2024-10-17T09:00:00Z")),
            release("v1.46.3", None),
            release("canary", Some("2024-10-20T00:00:00Z")),
        ]);
        let cli = ["v1.46.3", "v2.0.1", "v2.0.0", "v2.0.0", "0.1.0"];
        deno_versions(cli.map(String::from).to_vec(), &dates)
    }

    #[test]
    fn deno_versions_are_sorted_deduped_and_dated() {
        let dated: Vec<_> = dated_versions()
            .into_iter()
            .map(|v| (v.version, v.created_at))
            .collect();
        assert_eq!(
            dated,
            [
                ("1.46.3".to_string(), None),
                (
                    "2.0.0".to_string(),
                    Some("2024-10-09T13:00:00Z".to_string())
                ),
                (
                    "2.0.1".to_string(),
                    Some("2024-10-17T09:00:00Z".to_string())
                ),
            ]
        );
    }

    #[test]
    fn deno_versions_released_after_the_cutoff_are_hidden() {
        let cutoff = parse_into_timestamp("2024-10-10T00:00:00Z").unwrap();
        let kept: Vec<_> = VersionInfo::filter_by_date(dated_versions(), cutoff)
            .into_iter()
            .map(|v| v.version)
            .collect();
        assert_eq!(kept, ["1.46.3", "2.0.0"]);
    }
}
