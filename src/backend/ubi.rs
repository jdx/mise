use crate::backend::VersionInfo;
use crate::backend::backend_type::BackendType;
use crate::backend::platform_target::PlatformTarget;
use crate::backend::static_helpers::{lookup_platform_key, try_with_v_prefix};
use crate::cli::args::BackendArg;
use crate::config::{Config, Settings};
use crate::env::{
    GITHUB_TOKEN, GITLAB_TOKEN, MISE_GITHUB_ENTERPRISE_TOKEN, MISE_GITLAB_ENTERPRISE_TOKEN,
};
use crate::install_context::InstallContext;
use crate::plugins::VERSION_REGEX;
use crate::toolset::{ToolRequest, ToolVersion};
use crate::{backend::Backend, toolset::ToolVersionOptions};
use crate::{file, github, gitlab, hash};
use async_trait::async_trait;
use eyre::bail;
use regex::Regex;
use std::collections::BTreeMap;
use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::OnceLock;
use std::{fmt::Debug, sync::LazyLock};
use ubi::{ForgeType, UbiBuilder};
use xx::regex;

#[derive(Debug)]
pub struct UbiBackend {
    ba: Arc<BackendArg>,
}

#[async_trait]
impl Backend for UbiBackend {
    fn get_type(&self) -> BackendType {
        BackendType::Ubi
    }

    fn ba(&self) -> &Arc<BackendArg> {
        &self.ba
    }

    async fn _list_remote_versions(&self, _config: &Arc<Config>) -> eyre::Result<Vec<VersionInfo>> {
        if name_is_url(&self.tool_name()) {
            Ok(vec![VersionInfo {
                version: "latest".to_string(),
                ..Default::default()
            }])
        } else {
            let opts = self.ba.opts();
            let forge = match opts.get("provider") {
                Some(forge) => ForgeType::from_str(forge)?,
                None => ForgeType::default(),
            };
            let api_url = match opts.get("api_url") {
                Some(api_url) => api_url.strip_suffix("/").unwrap_or(api_url),
                None => match forge {
                    ForgeType::GitHub => github::API_URL,
                    ForgeType::GitLab => gitlab::API_URL,
                    _ => bail!("Unsupported forge type {:?}", forge),
                },
            };

            let tag_regex_cell = OnceLock::new();

            // Build release URL base based on forge type and api_url
            let release_url_base = match forge {
                ForgeType::GitHub => {
                    if api_url == github::API_URL {
                        format!("https://github.com/{}", self.tool_name())
                    } else {
                        // Enterprise GitHub - derive web URL from API URL
                        let web_url = api_url.replace("/api/v3", "").replace("api.", "");
                        format!("{}/{}", web_url, self.tool_name())
                    }
                }
                ForgeType::GitLab => {
                    if api_url == gitlab::API_URL {
                        format!("https://gitlab.com/{}", self.tool_name())
                    } else {
                        // Enterprise GitLab - derive web URL from API URL
                        let web_url = api_url.replace("/api/v4", "");
                        format!("{}/{}", web_url, self.tool_name())
                    }
                }
                _ => bail!("Unsupported forge type {:?}", forge),
            };

            // Helper to check if tag matches tag_regex (if provided)
            let matches_tag_regex = |tag: &str| -> bool {
                if let Some(re_str) = opts.get("tag_regex") {
                    let re = tag_regex_cell.get_or_init(|| Regex::new(re_str).unwrap());
                    re.is_match(tag)
                } else {
                    true
                }
            };

            // Helper to strip 'v' prefix from version
            let strip_v_prefix = |tag: &str| -> String {
                if regex!(r"^v[0-9]").is_match(tag) {
                    tag[1..].to_string()
                } else {
                    tag.to_string()
                }
            };

            let mut version_infos: Vec<VersionInfo> = match forge {
                ForgeType::GitHub => {
                    let releases =
                        github::list_releases_from_url(api_url, &self.tool_name()).await?;
                    if releases.is_empty() {
                        // Fall back to tags (no created_at available)
                        github::list_tags_from_url(api_url, &self.tool_name())
                            .await?
                            .into_iter()
                            .filter(|tag| matches_tag_regex(tag))
                            .map(|tag| {
                                let release_url =
                                    format!("{}/releases/tag/{}", release_url_base, tag);
                                VersionInfo {
                                    version: strip_v_prefix(&tag),
                                    release_url: Some(release_url),
                                    ..Default::default()
                                }
                            })
                            .collect()
                    } else {
                        releases
                            .into_iter()
                            .filter(|r| matches_tag_regex(&r.tag_name))
                            .map(|r| {
                                let release_url =
                                    format!("{}/releases/tag/{}", release_url_base, r.tag_name);
                                VersionInfo {
                                    version: strip_v_prefix(&r.tag_name),
                                    created_at: Some(r.created_at),
                                    release_url: Some(release_url),
                                    ..Default::default()
                                }
                            })
                            .collect()
                    }
                }
                ForgeType::GitLab => {
                    let releases =
                        gitlab::list_releases_from_url(api_url, &self.tool_name()).await?;
                    if releases.is_empty() {
                        // Fall back to tags (no created_at available)
                        gitlab::list_tags_from_url(api_url, &self.tool_name())
                            .await?
                            .into_iter()
                            .filter(|tag| matches_tag_regex(tag))
                            .map(|tag| {
                                // Use /-/tags/ for tag-only URLs (no release exists)
                                let release_url = format!("{}/-/tags/{}", release_url_base, tag);
                                VersionInfo {
                                    version: strip_v_prefix(&tag),
                                    release_url: Some(release_url),
                                    ..Default::default()
                                }
                            })
                            .collect()
                    } else {
                        releases
                            .into_iter()
                            .filter(|r| matches_tag_regex(&r.tag_name))
                            .map(|r| {
                                let release_url =
                                    format!("{}/-/releases/{}", release_url_base, r.tag_name);
                                VersionInfo {
                                    version: strip_v_prefix(&r.tag_name),
                                    created_at: r.released_at,
                                    release_url: Some(release_url),
                                    ..Default::default()
                                }
                            })
                            .collect()
                    }
                }
                _ => bail!("Unsupported forge type {:?}", forge),
            };

            // Sort: versions starting with digits first, then reverse
            version_infos.sort_by_cached_key(|vi| !regex!(r"^[0-9]").is_match(&vi.version));
            version_infos.reverse();

            Ok(version_infos)
        }
    }

    async fn install_version_(
        &self,
        ctx: &InstallContext,
        mut tv: ToolVersion,
    ) -> eyre::Result<ToolVersion> {
        deprecated!(
            "ubi",
            "The ubi backend is deprecated. Use the github backend instead (e.g., github:owner/repo)"
        );
        // Check if lockfile has URL for this platform
        let platform_key = self.get_platform_key();
        let lockfile_url = tv
            .lock_platforms
            .get(&platform_key)
            .and_then(|p| p.url.clone());

        let v = tv.version.to_string();
        let opts = tv.request.options();
        let bin_path = lookup_platform_key(&opts, "bin_path")
            .or_else(|| opts.get("bin_path").cloned())
            .unwrap_or_else(|| "bin".to_string());
        let extract_all = opts.get("extract_all").is_some_and(|v| v == "true");
        let bin_dir = tv.install_path();

        // Use lockfile URL if available, otherwise fall back to standard resolution
        if let Some(url) = &lockfile_url {
            install(url, &v, &bin_dir, extract_all, &opts)
                .await
                .map_err(|e| eyre::eyre!(e))?;
        } else if name_is_url(&self.tool_name()) {
            install(&self.tool_name(), &v, &bin_dir, extract_all, &opts)
                .await
                .map_err(|e| eyre::eyre!(e))?;
        } else {
            try_with_v_prefix(&v, None, |candidate| {
                let opts = opts.clone();
                let bin_dir = bin_dir.clone();
                async move {
                    install(
                        &self.tool_name(),
                        &candidate,
                        &bin_dir,
                        extract_all,
                        &opts,
                    )
                    .await
                }
            })
            .await?;
        }

        let mut possible_exes = vec![
            tv.request
                .options()
                .get("exe")
                .cloned()
                .unwrap_or(tv.ba().short.to_string()),
        ];
        if cfg!(windows) {
            possible_exes.push(format!("{}.exe", possible_exes[0]));
        }
        let full_binary_path = if let Some(bin_file) = possible_exes
            .into_iter()
            .map(|e| bin_dir.join(e))
            .find(|f| f.exists())
        {
            bin_file
        } else {
            let mut bin_dir = bin_dir.to_path_buf();
            if extract_all && bin_dir.join(&bin_path).exists() {
                bin_dir = bin_dir.join(&bin_path);
            }
            file::ls(&bin_dir)?
                .into_iter()
                .find(|f| {
                    !f.file_name()
                        .is_some_and(|f| f.to_string_lossy().starts_with("."))
                })
                .unwrap()
        };
        self.verify_checksum(ctx, &mut tv, &full_binary_path)?;

        Ok(tv)
    }

    fn fuzzy_match_filter(&self, versions: Vec<String>, query: &str) -> Vec<String> {
        let escaped_query = regex::escape(query);
        let query = if query == "latest" {
            "\\D*[0-9].*"
        } else {
            &escaped_query
        };
        let query_regex = Regex::new(&format!("^{query}([-.].+)?$")).unwrap();

        versions
            .into_iter()
            .filter(|v| {
                if query == v {
                    return true;
                }
                if VERSION_REGEX.is_match(v) {
                    return false;
                }
                query_regex.is_match(v)
            })
            .collect()
    }

    fn verify_checksum(
        &self,
        ctx: &InstallContext,
        tv: &mut ToolVersion,
        file: &Path,
    ) -> eyre::Result<()> {
        // For ubi backend, generate a more specific platform key that includes tool-specific options
        let mut platform_key = self.get_platform_key();
        let filename = file.file_name().unwrap().to_string_lossy().to_string();

        if let Some(exe) = tv.request.options().get("exe") {
            platform_key = format!("{platform_key}-{exe}");
        }
        if let Some(matching) = tv.request.options().get("matching") {
            platform_key = format!("{platform_key}-{matching}");
        }
        // Include filename to distinguish different downloads for the same platform
        platform_key = format!("{platform_key}-{filename}");

        // Get or create platform info for this platform key
        let platform_info = tv.lock_platforms.entry(platform_key.clone()).or_default();

        if let Some(checksum) = &platform_info.checksum {
            ctx.pr
                .set_message(format!("checksum verify {platform_key}"));
            if let Some((algo, check)) = checksum.split_once(':') {
                hash::ensure_checksum(file, check, Some(ctx.pr.as_ref()), algo)?;
            } else {
                bail!("Invalid checksum: {platform_key}");
            }
        } else if Settings::get().lockfile {
            ctx.pr
                .set_message(format!("checksum generate {platform_key}"));
            let hash = hash::file_hash_blake3(file, Some(ctx.pr.as_ref()))?;
            platform_info.checksum = Some(format!("blake3:{hash}"));
        }
        Ok(())
    }

    async fn list_bin_paths(
        &self,
        _config: &Arc<Config>,
        tv: &ToolVersion,
    ) -> eyre::Result<Vec<std::path::PathBuf>> {
        let opts = tv.request.options();
        if let Some(bin_path) =
            lookup_platform_key(&opts, "bin_path").or_else(|| opts.get("bin_path").cloned())
        {
            // bin_path should always point to a directory containing binaries
            Ok(vec![tv.install_path().join(&bin_path)])
        } else if opts.get("extract_all").is_some_and(|v| v == "true") {
            Ok(vec![tv.install_path()])
        } else {
            let bin_path = tv.install_path().join("bin");
            if bin_path.exists() {
                Ok(vec![bin_path])
            } else {
                Ok(vec![tv.install_path()])
            }
        }
    }

    /// UBI is deprecated in favor of the github backend and doesn't resolve download URLs
    /// at lock time. Return false so --locked mode doesn't error for ubi tools.
    fn supports_lockfile_url(&self) -> bool {
        false
    }

    fn resolve_lockfile_options(
        &self,
        request: &ToolRequest,
        _target: &PlatformTarget,
    ) -> BTreeMap<String, String> {
        let opts = request.options();
        let mut result = BTreeMap::new();

        // These options affect which artifact is downloaded
        for key in ["exe", "matching", "matching_regex", "provider"] {
            if let Some(value) = opts.get(key) {
                result.insert(key.to_string(), value.clone());
            }
        }

        result
    }
}

/// Returns install-time-only option keys for UBI backend.
pub fn install_time_option_keys() -> Vec<String> {
    vec![
        "exe".into(),
        "matching".into(),
        "matching_regex".into(),
        "provider".into(),
    ]
}

impl UbiBackend {
    pub fn from_arg(ba: BackendArg) -> Self {
        Self { ba: Arc::new(ba) }
    }
}

fn name_is_url(n: &str) -> bool {
    n.starts_with("http")
}

fn set_token<'a>(mut builder: UbiBuilder<'a>, forge: &ForgeType) -> UbiBuilder<'a> {
    match forge {
        ForgeType::GitHub => {
            if let Some(token) = &*GITHUB_TOKEN {
                builder = builder.token(token)
            }
            builder
        }
        ForgeType::GitLab => {
            if let Some(token) = &*GITLAB_TOKEN {
                builder = builder.token(token)
            }
            builder
        }
        _ => builder,
    }
}

fn set_enterprise_token<'a>(mut builder: UbiBuilder<'a>, forge: &ForgeType) -> UbiBuilder<'a> {
    match forge {
        ForgeType::GitHub => {
            if let Some(token) = &*MISE_GITHUB_ENTERPRISE_TOKEN {
                builder = builder.token(token);
            }
            builder
        }
        ForgeType::GitLab => {
            if let Some(token) = &*MISE_GITLAB_ENTERPRISE_TOKEN {
                builder = builder.token(token);
            }
            builder
        }
        _ => builder,
    }
}

async fn install(
    name: &str,
    v: &str,
    bin_dir: &Path,
    extract_all: bool,
    opts: &ToolVersionOptions,
) -> anyhow::Result<()> {
    let mut builder = UbiBuilder::new().install_dir(bin_dir);

    if name_is_url(name) {
        builder = builder.url(name);
    } else {
        builder = builder.project(name);
        builder = builder.tag(v);
    }

    if extract_all {
        builder = builder.extract_all();
    } else {
        if let Some(exe) = opts.get("exe") {
            builder = builder.exe(exe);
        }
        if let Some(rename_exe) = opts.get("rename_exe") {
            builder = builder.rename_exe_to(rename_exe)
        }
    }
    if let Some(matching) = opts.get("matching") {
        builder = builder.matching(matching);
    }
    if let Some(matching_regex) = opts.get("matching_regex") {
        builder = builder.matching_regex(matching_regex);
    }

    let forge = match opts.get("provider") {
        Some(forge) => ForgeType::from_str(forge)?,
        None => ForgeType::default(),
    };
    builder = builder.forge(forge.clone());
    builder = set_token(builder, &forge);

    if let Some(api_url) = opts.get("api_url")
        && !api_url.contains("github.com")
        && !api_url.contains("gitlab.com")
    {
        builder = builder.api_base_url(api_url.strip_suffix("/").unwrap_or(api_url));
        builder = set_enterprise_token(builder, &forge);
    }

    let mut ubi = builder.build()?;

    // TODO: hacky but does not compile without it
    tokio::task::block_in_place(|| {
        static RT: LazyLock<tokio::runtime::Runtime> = LazyLock::new(|| {
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .unwrap()
        });
        RT.block_on(async { ubi.install_binary().await })
    })
}
