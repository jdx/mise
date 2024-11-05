use crate::backend::{Backend, BackendType};
use crate::cache::{CacheManager, CacheManagerBuilder};
use crate::cli::args::BackendArg;
use crate::config::SETTINGS;
use crate::env::GITHUB_TOKEN;
use crate::github;
use crate::install_context::InstallContext;
use crate::plugins::VERSION_REGEX;
use crate::toolset::ToolRequest;
use eyre::bail;
use regex::Regex;
use std::fmt::Debug;
use std::sync::OnceLock;
use ubi::UbiBuilder;
use xx::regex;

#[derive(Debug)]
pub struct UbiBackend {
    ba: BackendArg,
    remote_version_cache: CacheManager<Vec<String>>,
}

// Uses ubi for installations https://github.com/houseabsolute/ubi
impl Backend for UbiBackend {
    fn get_type(&self) -> BackendType {
        BackendType::Ubi
    }

    fn fa(&self) -> &BackendArg {
        &self.ba
    }

    fn get_dependencies(&self, _tvr: &ToolRequest) -> eyre::Result<Vec<BackendArg>> {
        Ok(vec![])
    }

    fn _list_remote_versions(&self) -> eyre::Result<Vec<String>> {
        if name_is_url(self.name()) {
            Ok(vec!["latest".to_string()])
        } else {
            self.remote_version_cache
                .get_or_try_init(|| {
                    let opts = self.ba.opts.clone().unwrap_or_default();
                    let tag_regex = OnceLock::new();
                    Ok(github::list_releases(self.name())?
                        .into_iter()
                        .map(|r| r.tag_name)
                        // trim 'v' prefixes if they exist
                        .map(|t| match regex!(r"^v[0-9]").is_match(&t) {
                            true => t[1..].to_string(),
                            false => t,
                        })
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
                })
                .cloned()
        }
    }

    fn install_version_impl(&self, ctx: &InstallContext) -> eyre::Result<()> {
        let mut v = ctx.tv.version.to_string();

        if let Err(err) = github::get_release(self.name(), &ctx.tv.version) {
            // this can fail with a rate limit error or 404, either way, try prefixing and if it fails, try without the prefix
            // if http::error_code(&err) == Some(404) {
            debug!(
                "Failed to get release for {}, trying with 'v' prefix: {}",
                ctx.tv, err
            );
            v = format!("v{v}");
            // }
        }

        let install = |v: &str| {
            let opts = ctx.tv.request.options();
            // Workaround because of not knowing how to pull out the value correctly without quoting
            let path_with_bin = ctx.tv.install_path().join("bin");

            let mut builder = UbiBuilder::new()
                .project(self.name())
                .install_dir(path_with_bin);

            if let Some(token) = &*GITHUB_TOKEN {
                builder = builder.github_token(token);
            }

            if v != "latest" {
                builder = builder.tag(v);
            }

            if let Some(exe) = opts.get("exe") {
                builder = builder.exe(exe);
            }
            if let Some(matching) = opts.get("matching") {
                builder = builder.matching(matching);
            }

            let mut ubi = builder.build()?;

            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_io()
                .enable_time()
                .build()?;

            rt.block_on(ubi.install_binary())
        };

        if let Err(err) = install(&v) {
            if v != ctx.tv.version {
                debug!(
                    "Failed to install with ubi version '{}': {}, trying with '{}'",
                    v, err, ctx.tv
                );
                if let Err(_err2) = install(&ctx.tv.version) {
                    bail!("Failed to install with ubi '{}': {}", ctx.tv, err);
                }
            }
        }

        Ok(())
    }

    fn fuzzy_match_filter(&self, versions: Vec<String>, query: &str) -> eyre::Result<Vec<String>> {
        let escaped_query = regex::escape(query);
        let query = if query == "latest" {
            "\\D*[0-9].*"
        } else {
            &escaped_query
        };
        let query_regex = Regex::new(&format!("^{}([-.].+)?$", query))?;
        let versions = versions
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
            .collect();
        Ok(versions)
    }
}

impl UbiBackend {
    pub fn from_arg(ba: BackendArg) -> Self {
        Self {
            remote_version_cache: CacheManagerBuilder::new(
                ba.cache_path.join("remote_versions.msgpack.z"),
            )
            .with_fresh_duration(SETTINGS.fetch_remote_versions_cache())
            .build(),
            ba,
        }
    }
}

fn name_is_url(n: &str) -> bool {
    n.starts_with("http")
}
