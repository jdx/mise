use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};

use crate::backend::Backend;
use crate::cli::args::BackendArg;
use crate::cli::version::OS;
use crate::cmd::CmdLineRunner;
use crate::config::{Config, Settings};
use crate::duration::DAILY;
use crate::file::TarOptions;
use crate::http::{HTTP, HTTP_FETCH};
use crate::install_context::InstallContext;
use crate::toolset::ToolVersion;
use crate::ui::progress_report::SingleReport;
use crate::{file, minisign, plugins};
use async_trait::async_trait;
use eyre::Result;
use itertools::Itertools;
use rand::seq::SliceRandom;
use versions::Versioning;
use xx::regex;

#[derive(Debug)]
pub struct ZigPlugin {
    ba: Arc<BackendArg>,
}

const ZIG_MINISIGN_KEY: &str = "RWSGOq2NVecA2UPNdBUZykf1CCb147pkmdtYxgb3Ti+JO/wCYvhbAb/U";
const REQUEST_SUFFIX: &str = "?source=mise-en-place";
const MIRRORS_FILENAME: &str = "community-mirrors.txt";

impl ZigPlugin {
    pub fn new() -> Self {
        Self {
            ba: Arc::new(plugins::core::new_backend_arg("zig")),
        }
    }

    fn zig_bin(&self, tv: &ToolVersion) -> PathBuf {
        if cfg!(windows) {
            tv.install_path().join("zig.exe")
        } else {
            tv.install_path().join("bin").join("zig")
        }
    }

    fn test_zig(&self, ctx: &InstallContext, tv: &ToolVersion) -> Result<()> {
        ctx.pr.set_message("zig version".into());
        CmdLineRunner::new(self.zig_bin(tv))
            .with_pr(ctx.pr.as_ref())
            .arg("version")
            .execute()
    }

    async fn download(&self, tv: &ToolVersion, pr: &dyn SingleReport) -> Result<PathBuf> {
        let settings = Settings::get();
        let indexes = HashMap::from([
            ("zig", "https://ziglang.org/download/index.json"),
            ("mach", "https://machengine.org/zig/index.json"),
        ]);

        let url = if regex!(r"^mach-|-mach$").is_match(&tv.version) {
            self.get_tarball_url_from_json(
                indexes["mach"],
                tv.version.as_str(),
                arch(&settings),
                os(),
            )
            .await?
        } else {
            self.get_tarball_url_from_json(
                indexes["zig"],
                tv.version.as_str(),
                arch(&settings),
                os(),
            )
            .await
            .or_else(|err| {
                // We can construct the tarball name for numbered versions without the index
                if regex!(r"^\d+\.\d+\.\d+$").is_match(&tv.version) {
                    let (version, arch, os) = (tv.version.as_str(), arch(&settings), os());
                    Ok(format!(
                        "https://ziglang.org/download/{version}/zig-{arch}-{os}-{version}.tar.xz"
                    ))
                } else {
                    Err(err)
                }
            })?
        };

        let filename = url.split('/').next_back().unwrap();
        let tarball_path = tv.download_path().join(filename);

        let mut downloaded = false;
        let mut used_url = url.clone();
        // The ziglang.org website kindly asks for trying mirrors for automated downloads,
        // read more on https://ziglang.org/download/community-mirrors/
        let community_mirrors = if url.starts_with("https://ziglang.org") {
            self.get_community_mirrors().await
        } else {
            None
        };

        if settings.zig.use_community_mirrors
            && let Some(mirrors) = community_mirrors {
                for i in 0..mirrors.len() {
                    let disp_i = i + 1;
                    let disp_len = mirrors.len();
                    pr.set_message(format!("mirror {disp_i}/{disp_len} {filename}"));

                    let mirror_url = &mirrors[i];
                    used_url = format!("{mirror_url}/{filename}");

                    if HTTP
                        .download_file(
                            format!("{used_url}{REQUEST_SUFFIX}"),
                            &tarball_path,
                            Some(pr),
                        )
                        .await
                        .is_ok()
                    {
                        downloaded = true;
                        break;
                    }
                }
            }

        if !downloaded {
            // Try the usual ziglang.org or machengine.org download
            pr.set_message(format!("download {filename}"));
            used_url = url.clone();
            HTTP.download_file(&url, &tarball_path, Some(pr)).await?;
            // If this was ziglang.org and error is not 404 and community_mirrors is None,
            // the user might want to place the mirror list in cache dir by hand
        }

        pr.set_message(format!("minisign {filename}"));
        let tarball_data = file::read(&tarball_path)?;
        let sig = HTTP
            .get_text(format!("{used_url}.minisig{REQUEST_SUFFIX}"))
            .await?;
        minisign::verify(ZIG_MINISIGN_KEY, &tarball_data, &sig)?;
        // Since this passed the verify step, the format is guaranteed to be correct
        let trusted_comment = sig.split('\n').nth(2).unwrap().to_string();
        // Verify that this is the desired version using trusted comment to prevent downgrade attacks
        if !trusted_comment.contains(&format!("file:{filename}")) {
            return Err(eyre::eyre!(
                "Expected {}, but signature {}.minisig had:\n{}",
                filename,
                used_url,
                trusted_comment
            ));
        }

        Ok(tarball_path)
    }

    fn install(&self, ctx: &InstallContext, tv: &ToolVersion, tarball_path: &Path) -> Result<()> {
        let filename = tarball_path.file_name().unwrap().to_string_lossy();
        ctx.pr.set_message(format!("extract {filename}"));
        file::remove_all(tv.install_path())?;
        file::untar(
            tarball_path,
            &tv.install_path(),
            &TarOptions {
                strip_components: 1,
                pr: Some(ctx.pr.as_ref()),
                ..Default::default()
            },
        )?;

        if cfg!(unix) {
            file::create_dir_all(tv.install_path().join("bin"))?;
            file::make_symlink(Path::new("../zig"), &tv.install_path().join("bin/zig"))?;
        }

        Ok(())
    }

    fn verify(&self, ctx: &InstallContext, tv: &ToolVersion) -> Result<()> {
        self.test_zig(ctx, tv)
    }

    async fn get_tarball_url_from_json(
        &self,
        json_url: &str,
        version: &str,
        arch: &str,
        os: &str,
    ) -> Result<String> {
        let version_json: serde_json::Value = HTTP_FETCH.json(json_url).await?;
        let zig_tarball_url = version_json
            .pointer(&format!("/{version}/{arch}-{os}/tarball"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| eyre::eyre!("Failed to get zig tarball url from {:?}", json_url))?;
        Ok(zig_tarball_url.to_string())
    }

    async fn get_community_mirrors(&self) -> Option<Vec<String>> {
        let cache_path = self.ba.cache_path.join(MIRRORS_FILENAME);
        let recent_cache =
            file::modified_duration(&cache_path).is_ok_and(|updated_at| updated_at < DAILY);
        if !recent_cache {
            HTTP.download_file(
                &format!("https://ziglang.org/download/{MIRRORS_FILENAME}"),
                &cache_path,
                None,
            )
            .await
            .unwrap_or_else(|_| {
                // We can still use an older mirror list
                warn!("{}: Could not download {}", self.ba, MIRRORS_FILENAME);
            });
        }

        let mirror_list = String::from_utf8(file::read(cache_path).ok()?).ok()?;
        let mut mirrors: Vec<String> = mirror_list
            .split('\n')
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect();
        let mut rng = rand::rng();
        mirrors.shuffle(&mut rng);
        Some(mirrors)
    }
}

#[async_trait]
impl Backend for ZigPlugin {
    fn ba(&self) -> &Arc<BackendArg> {
        &self.ba
    }

    async fn _list_remote_versions(&self, _config: &Arc<Config>) -> Result<Vec<String>> {
        let indexes = [
            "https://ziglang.org/download/index.json",
            // "https://machengine.org/zig/index.json", // need to handle mach's CalVer
        ];
        let mut versions: Vec<String> = Vec::new();

        for index in indexes {
            let index_json: serde_json::Value = HTTP_FETCH.json(index).await?;
            let index_versions: Vec<String> = index_json
                .as_object()
                .ok_or_else(|| eyre::eyre!("Failed to get zig version from {:?}", index))?
                .keys()
                .cloned()
                .collect();

            versions.extend(index_versions);
        }

        let versions = versions
            .into_iter()
            .unique()
            .sorted_by_cached_key(|s| (Versioning::new(s), s.to_string()))
            .collect();

        Ok(versions)
    }

    async fn list_bin_paths(
        &self,
        _config: &Arc<Config>,
        tv: &ToolVersion,
    ) -> Result<Vec<PathBuf>> {
        if cfg!(windows) {
            Ok(vec![tv.install_path()])
        } else {
            Ok(vec![tv.install_path().join("bin")])
        }
    }

    async fn idiomatic_filenames(&self) -> Result<Vec<String>> {
        Ok(vec![".zig-version".into()])
    }

    async fn install_version_(
        &self,
        ctx: &InstallContext,
        mut tv: ToolVersion,
    ) -> Result<ToolVersion> {
        let tarball_path = self.download(&tv, ctx.pr.as_ref()).await?;
        self.verify_checksum(ctx, &mut tv, &tarball_path)?;
        self.install(ctx, &tv, &tarball_path)?;
        self.verify(ctx, &tv)?;
        Ok(tv)
    }
}

fn os() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "freebsd") {
        "freebsd"
    } else {
        &OS
    }
}

fn arch(settings: &Settings) -> &str {
    match settings.arch() {
        "x64" => "x86_64",
        "arm64" => "aarch64",
        "arm" => "armv7a",
        "riscv64" => "riscv64",
        other => other,
    }
}
