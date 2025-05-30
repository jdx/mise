use crate::backend::backend_type::BackendType;
use crate::cli::args::BackendArg;
use crate::cmd::CmdLineRunner;
use crate::config::Settings;
use crate::install_context::InstallContext;
use crate::timeout;
use crate::toolset::ToolVersion;
use crate::{backend::Backend, config::Config};
use async_trait::async_trait;
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
                let mut mod_path = Some(self.tool_name());

                while let Some(cur_mod_path) = mod_path {
                    let res = cmd!("go", "list", "-m", "-versions", "-json", &cur_mod_path)
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

                    mod_path = trim_after_last_slash(cur_mod_path);
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

fn trim_after_last_slash(s: String) -> Option<String> {
    s.rsplit_once('/').map(|(new_path, _)| new_path.to_string())
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct GoModInfo {
    versions: Vec<String>,
}
