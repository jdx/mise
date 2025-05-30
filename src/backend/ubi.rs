use crate::backend::backend_type::BackendType;
use crate::cli::args::BackendArg;
use crate::config::{Config, Settings};
use crate::env::{
    GITHUB_TOKEN, GITLAB_TOKEN, MISE_GITHUB_ENTERPRISE_TOKEN, MISE_GITLAB_ENTERPRISE_TOKEN,
};
use crate::install_context::InstallContext;
use crate::plugins::VERSION_REGEX;
use crate::toolset::ToolVersion;
use crate::{backend::Backend, toolset::ToolVersionOptions};
use crate::{file, github, gitlab, hash};
use async_trait::async_trait;
use eyre::bail;
use itertools::Itertools;
use regex::Regex;
use std::path::Path;
use std::str::FromStr;
use std::sync::OnceLock;
use std::{env, sync::Arc};
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

    async fn _list_remote_versions(&self, _config: &Arc<Config>) -> eyre::Result<Vec<String>> {
        if name_is_url(&self.tool_name()) {
            Ok(vec!["latest".to_string()])
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
                },
            };
            let tag_regex = OnceLock::new();
            let mut versions = match forge {
                ForgeType::GitHub => github::list_releases_from_url(api_url, &self.tool_name())
                    .await?
                    .into_iter()
                    .map(|r| r.tag_name)
                    .collect::<Vec<String>>(),
                ForgeType::GitLab => gitlab::list_releases_from_url(api_url, &self.tool_name())
                    .await?
                    .into_iter()
                    .map(|r| r.tag_name)
                    .collect::<Vec<String>>(),
            };
            if versions.is_empty() {
                match forge {
                    ForgeType::GitHub => {
                        versions = github::list_tags_from_url(api_url, &self.tool_name())
                            .await?
                            .into_iter()
                            .collect();
                    }
                    ForgeType::GitLab => {
                        versions = gitlab::list_tags_from_url(api_url, &self.tool_name())
                            .await?
                            .into_iter()
                            .collect();
                    }
                }
            }

            Ok(versions
                .into_iter()
                // trim 'v' prefixes if they exist
                .map(|t| match regex!(r"^v[0-9]").is_match(&t) {
                    true => t[1..].to_string(),
                    false => t,
                })
                .sorted_by_cached_key(|v| !regex!(r"^[0-9]").is_match(v))
                .filter(|v| {
                    if let Some(re) = opts.get("tag_regex") {
                        let re = tag_regex.get_or_init(|| Regex::new(re).unwrap());
                        re.is_match(v)
                    } else {
                        true
                    }
                })
                .rev()
                .collect())
        }
    }

    async fn install_version_(
        &self,
        ctx: &InstallContext,
        mut tv: ToolVersion,
    ) -> eyre::Result<ToolVersion> {
        let mut v = tv.version.to_string();
        let opts = tv.request.options();
        let forge = match opts.get("provider") {
            Some(forge) => ForgeType::from_str(forge)?,
            None => ForgeType::default(),
        };
        let api_url = match opts.get("api_url") {
            Some(api_url) => api_url.strip_suffix("/").unwrap_or(api_url),
            None => match forge {
                ForgeType::GitHub => github::API_URL,
                ForgeType::GitLab => gitlab::API_URL,
            },
        };
        let extract_all = opts.get("extract_all").is_some_and(|v| v == "true");
        let bin_dir = tv.install_path();

        if !name_is_url(&self.tool_name()) {
            let release: Result<_, eyre::Report> = match forge {
                ForgeType::GitHub => github::get_release_for_url(api_url, &self.tool_name(), &v)
                    .await
                    .map(|_| "github"),
                ForgeType::GitLab => gitlab::get_release_for_url(api_url, &self.tool_name(), &v)
                    .await
                    .map(|_| "gitlab"),
            };
            if let Err(err) = release {
                // this can fail with a rate limit error or 404, either way, try prefixing and if it fails, try without the prefix
                // if http::error_code(&err) == Some(404) {
                debug!(
                    "Failed to get release for {}, trying with 'v' prefix: {}",
                    tv, err
                );
                v = format!("v{v}");
                // }
            }
        }

        if let Err(err) = install(&self.tool_name(), &v, &bin_dir, extract_all, &opts).await {
            debug!(
                "Failed to install with ubi version '{}': {}, trying with '{}'",
                v, err, tv
            );
            if let Err(err) =
                install(&self.tool_name(), &tv.version, &bin_dir, extract_all, &opts).await
            {
                bail!("Failed to install with ubi '{}': {}", tv, err);
            }
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
        let mut checksum_key = file.file_name().unwrap().to_string_lossy().to_string();
        if let Some(exe) = tv.request.options().get("exe") {
            checksum_key = format!("{checksum_key}-{exe}");
        }
        if let Some(matching) = tv.request.options().get("matching") {
            checksum_key = format!("{checksum_key}-{matching}");
        }
        checksum_key = format!("{}-{}-{}", checksum_key, env::consts::OS, env::consts::ARCH);
        if let Some(checksum) = &tv.checksums.get(&checksum_key) {
            ctx.pr
                .set_message(format!("checksum verify {checksum_key}"));
            if let Some((algo, check)) = checksum.split_once(':') {
                hash::ensure_checksum(file, check, Some(&ctx.pr), algo)?;
            } else {
                bail!("Invalid checksum: {checksum_key}");
            }
        } else if Settings::get().lockfile && Settings::get().experimental {
            ctx.pr
                .set_message(format!("checksum generate {checksum_key}"));
            let hash = hash::file_hash_sha256(file, Some(&ctx.pr))?;
            tv.checksums.insert(checksum_key, format!("sha256:{hash}"));
        }
        Ok(())
    }

    async fn list_bin_paths(
        &self,
        _config: &Arc<Config>,
        tv: &ToolVersion,
    ) -> eyre::Result<Vec<std::path::PathBuf>> {
        let opts = tv.request.options();
        if let Some(bin_path) = opts.get("bin_path") {
            Ok(vec![tv.install_path().join(bin_path)])
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
    }
}

async fn install(
    name: &str,
    v: &str,
    bin_dir: &Path,
    extract_all: bool,
    opts: &ToolVersionOptions,
) -> eyre::Result<()> {
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

    let forge = match opts.get("provider") {
        Some(forge) => ForgeType::from_str(forge)?,
        None => ForgeType::default(),
    };
    builder = builder.forge(forge.clone());
    builder = set_token(builder, &forge);

    if let Some(api_url) = opts.get("api_url") {
        if !api_url.contains("github.com") && !api_url.contains("gitlab.com") {
            builder = builder.api_base_url(api_url.strip_suffix("/").unwrap_or(api_url));
            builder = set_enterprise_token(builder, &forge);
        }
    }

    let mut ubi = builder.build().map_err(|e| eyre::eyre!(e))?;

    // TODO: hacky but does not compile without it
    tokio::task::block_in_place(|| {
        static RT: LazyLock<tokio::runtime::Runtime> = LazyLock::new(|| {
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .unwrap()
        });
        RT.block_on(async {
            match ubi.install_binary().await {
                Ok(_) => Ok(()),
                Err(e) => Err(eyre::eyre!(e)),
            }
        })
    })
}
