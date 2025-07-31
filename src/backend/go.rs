use crate::backend::backend_type::BackendType;
use crate::cli::args::BackendArg;
use crate::cmd::CmdLineRunner;
use crate::config::Settings;
use crate::install_context::InstallContext;
use crate::timeout;
use crate::toolset::ToolVersion;
use crate::{backend::Backend, config::Config};
use async_trait::async_trait;
use itertools::Itertools;
use std::{fmt::Debug, sync::Arc};
use xx::regex;

#[derive(Debug)]
pub struct GoBackend {
    ba: Arc<BackendArg>,
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

    async fn _list_remote_versions(&self, config: &Arc<Config>) -> eyre::Result<Vec<String>> {
        timeout::run_with_timeout_async(
            async || {
                let tool_name = self.tool_name();
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
                    let res = cmd!("go", "list", "-m", "-versions", "-json", mod_path)
                        .full_env(self.dependency_env(config).await?)
                        .read();
                    if let Ok(raw) = res {
                        let res = serde_json::from_str::<GoModInfo>(&raw);
                        if let Ok(mut mod_info) = res {
                            // remove the leading v from the versions
                            mod_info.versions = mod_info
                                .versions
                                .into_iter()
                                .map(|v| v.trim_start_matches('v').to_string())
                                .collect();
                            return Ok(mod_info.versions);
                        }
                    };
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
        tv: ToolVersion,
    ) -> eyre::Result<ToolVersion> {
        Settings::get().ensure_experimental("go backend")?;
        let opts = self.ba.opts();

        let install = async |v| {
            let mut cmd = CmdLineRunner::new("go").arg("install");

            if let Some(tags) = opts.get("tags") {
                cmd = cmd.arg("-tags").arg(tags);
            }

            cmd.arg(format!("{}@{v}", self.tool_name()))
                .with_pr(&ctx.pr)
                .envs(self.dependency_env(&ctx.config).await?)
                .env("GOBIN", tv.install_path().join("bin"))
                .execute()
        };

        // try "v" prefix if the version starts with semver
        let use_v = regex!(r"^\d+\.\d+\.\d+").is_match(&tv.version);

        if use_v {
            if install(format!("v{}", tv.version)).await.is_err() {
                warn!("Failed to install, trying again without added 'v' prefix");
            } else {
                return Ok(tv);
            }
        }

        install(tv.version.clone()).await?;

        Ok(tv)
    }
}

impl GoBackend {
    pub fn from_arg(ba: BackendArg) -> Self {
        Self { ba: Arc::new(ba) }
    }
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct GoModInfo {
    versions: Vec<String>,
}
