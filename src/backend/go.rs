use crate::backend::Backend;
use crate::backend::VersionInfo;
use crate::backend::backend_type::BackendType;
use crate::backend::platform_target::PlatformTarget;
use crate::cache::{CacheManager, CacheManagerBuilder};
use crate::cli::args::BackendArg;
use crate::cmd::CmdLineRunner;
use crate::config::Config;
use crate::config::Settings;
use crate::hash::hash_to_str;
use crate::install_context::InstallContext;
use crate::timeout;
use crate::toolset::{ToolRequest, ToolVersion};
use async_trait::async_trait;
use dashmap::DashMap;
use itertools::Itertools;
use std::collections::BTreeMap;
use std::{fmt::Debug, sync::Arc};
use xx::regex;

#[derive(Debug)]
pub struct GoBackend {
    ba: Arc<BackendArg>,
    module_versions_cache: DashMap<String, CacheManager<Option<Vec<VersionInfo>>>>,
}

#[async_trait]
impl Backend for GoBackend {
    fn get_type(&self) -> BackendType {
        BackendType::Go
    }

    fn ba(&self) -> &Arc<BackendArg> {
        &self.ba
    }

    fn get_dependencies(&self) -> eyre::Result<Vec<&str>> {
        Ok(vec!["go"])
    }

    fn supports_lockfile_url(&self) -> bool {
        false
    }

    async fn _list_remote_versions(&self, config: &Arc<Config>) -> eyre::Result<Vec<VersionInfo>> {
        // Check if go is available
        self.warn_if_dependency_missing(
            config,
            "go",
            "To use go packages with mise, you need to install Go first:\n\
              mise use go@latest\n\n\
            Or install Go via https://go.dev/dl/",
        )
        .await;

        timeout::run_with_timeout_async(
            async || {
                let tool_name = self.tool_name();

                // First try the exact tool path. If this succeeds but returns no versions,
                // treat that as authoritative so installs can continue with `@latest`
                // instead of resolving to a parent module version.
                if let Some(versions) = self.fetch_go_module_versions(config, &tool_name).await? {
                    return Ok(versions);
                }

                let parts = tool_name.split('/').collect::<Vec<_>>();
                let module_root_index = if parts[0] == "github.com" {
                    // Try likely module root index first
                    if parts.len() >= 3 {
                        if parts.len() > 3 && regex!(r"^v\d+$").is_match(parts[3]) {
                            Some(3)
                        } else {
                            Some(2)
                        }
                    } else {
                        None
                    }
                } else {
                    None
                };
                let indices = module_root_index
                    .into_iter()
                    .chain((1..parts.len()).rev())
                    .unique()
                    .collect::<Vec<_>>();

                for i in indices {
                    let mod_path = parts[..=i].join("/");
                    if mod_path == tool_name {
                        continue;
                    }
                    if let Some(versions) = self.fetch_go_module_versions(config, &mod_path).await?
                    {
                        return Ok(versions);
                    }
                }

                Ok(vec![])
            },
            Settings::get().fetch_remote_versions_timeout(),
        )
        .await
    }

    async fn install_version_(
        &self,
        ctx: &InstallContext,
        mut tv: ToolVersion,
    ) -> eyre::Result<ToolVersion> {
        // Check if go is available
        self.warn_if_dependency_missing(
            &ctx.config,
            "go",
            "To use go packages with mise, you need to install Go first:\n\
              mise use go@latest\n\n\
            Or install Go via https://go.dev/dl/",
        )
        .await;

        // Some deep modules return no Versions from `go list -versions`.
        // If the original request was `latest`, force `@latest` install for
        // those modules instead of using a parent module's resolved version.
        let mut install_version = tv.version.clone();
        if tv.request.version() == "latest"
            && tv.version != "latest"
            && self
                .fetch_go_module_versions(&ctx.config, &self.tool_name())
                .await?
                .is_some_and(|v| v.is_empty())
        {
            install_version = "latest".to_string();
            tv.version = "latest".to_string();
        }

        let opts = self.ba.opts();

        let install = async |v| {
            let mut cmd = CmdLineRunner::new("go").arg("install").arg("-mod=readonly");

            if let Some(tags) = opts.get("tags") {
                cmd = cmd.arg("-tags").arg(tags);
            }

            cmd.arg(format!("{}@{v}", self.tool_name()))
                .with_pr(ctx.pr.as_ref())
                .envs(self.dependency_env(&ctx.config).await?)
                .env("GOBIN", tv.install_path().join("bin"))
                .execute()
        };

        // try "v" prefix if the version starts with semver
        let use_v = regex!(r"^\d+\.\d+\.\d+").is_match(&install_version);

        if use_v {
            if install(format!("v{}", install_version)).await.is_err() {
                warn!("Failed to install, trying again without added 'v' prefix");
            } else {
                return Ok(tv);
            }
        }

        install(install_version).await?;

        Ok(tv)
    }

    fn resolve_lockfile_options(
        &self,
        request: &ToolRequest,
        _target: &PlatformTarget,
    ) -> BTreeMap<String, String> {
        let opts = request.options();
        let mut result = BTreeMap::new();

        // tags affect compilation
        if let Some(value) = opts.get("tags") {
            result.insert("tags".to_string(), value.to_string());
        }

        result
    }
}

/// Returns install-time-only option keys for Go backend.
pub fn install_time_option_keys() -> Vec<String> {
    vec!["tags".into()]
}

impl GoBackend {
    pub fn from_arg(ba: BackendArg) -> Self {
        Self {
            ba: Arc::new(ba),
            module_versions_cache: Default::default(),
        }
    }

    async fn fetch_go_module_versions(
        &self,
        config: &Arc<Config>,
        mod_path: &str,
    ) -> eyre::Result<Option<Vec<VersionInfo>>> {
        let cache = self
            .module_versions_cache
            .entry(mod_path.to_string())
            .or_insert_with(|| {
                let filename = format!("{}.msgpack.z", hash_to_str(&mod_path.to_string()));
                CacheManagerBuilder::new(
                    self.ba.cache_path.join("go_module_versions").join(filename),
                )
                .with_fresh_duration(Settings::get().fetch_remote_versions_cache())
                .build()
            });

        cache
            .get_or_try_init_async(async || {
                let raw = match cmd!(
                    "go",
                    "list",
                    "-mod=readonly",
                    "-m",
                    "-versions",
                    "-json",
                    mod_path
                )
                .full_env(self.dependency_env(config).await?)
                .read()
                {
                    Ok(raw) => raw,
                    Err(_) => return Ok(None),
                };

                let mod_info = match serde_json::from_str::<GoModInfo>(&raw) {
                    Ok(info) => info,
                    Err(_) => return Ok(None),
                };

                // remove the leading v from the versions
                let versions = mod_info
                    .versions
                    .into_iter()
                    .map(|v| VersionInfo {
                        version: v.trim_start_matches('v').to_string(),
                        ..Default::default()
                    })
                    .collect();

                Ok(Some(versions))
            })
            .await
            .cloned()
    }
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct GoModInfo {
    #[serde(default)]
    versions: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::GoModInfo;

    #[test]
    fn parse_go_mod_info_without_versions() {
        let raw = r#"{"Path":"github.com/go-kratos/kratos/cmd/kratos/v2"}"#;
        let info: GoModInfo = serde_json::from_str(raw).unwrap();
        assert!(info.versions.is_empty());
    }

    #[test]
    fn parse_go_mod_info_with_versions() {
        let raw = r#"{"Path":"example.com/mod","Versions":["v1.0.0","v1.1.0"]}"#;
        let info: GoModInfo = serde_json::from_str(raw).unwrap();
        assert_eq!(info.versions, vec!["v1.0.0", "v1.1.0"]);
    }
}
