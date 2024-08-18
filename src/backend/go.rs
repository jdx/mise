use std::fmt::Debug;

use crate::backend::{Backend, BackendType};
use crate::cache::CacheManager;
use crate::cli::args::BackendArg;
use crate::cmd::CmdLineRunner;
use crate::config::Settings;
use crate::install_context::InstallContext;
use crate::toolset::ToolRequest;

#[derive(Debug)]
pub struct GoBackend {
    ba: BackendArg,
    remote_version_cache: CacheManager<Vec<String>>,
}

impl Backend for GoBackend {
    fn get_type(&self) -> BackendType {
        BackendType::Go
    }

    fn fa(&self) -> &BackendArg {
        &self.ba
    }

    fn get_dependencies(&self, _tvr: &ToolRequest) -> eyre::Result<Vec<BackendArg>> {
        Ok(vec!["go".into()])
    }

    fn _list_remote_versions(&self) -> eyre::Result<Vec<String>> {
        self.remote_version_cache
            .get_or_try_init(|| {
                let mut mod_path = Some(self.name());
                let env = self.dependency_env()?;

                while let Some(cur_mod_path) = mod_path {
                    let res = cmd!("go", "list", "-m", "-versions", "-json", cur_mod_path)
                        .full_env(&env)
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
            })
            .cloned()
    }

    fn install_version_impl(&self, ctx: &InstallContext) -> eyre::Result<()> {
        let settings = Settings::get();
        settings.ensure_experimental("go backend")?;

        // if the (semantic) version has no v prefix, add it
        // we allow max. 6 digits for the major version to prevent clashes with Git commit hashes
        let version = if regex!(r"^\d{1,6}(\.\d+)*([+-.].+)?$").is_match(&ctx.tv.version) {
            format!("v{}", ctx.tv.version)
        } else {
            ctx.tv.version.clone()
        };

        CmdLineRunner::new("go")
            .arg("install")
            .arg(format!("{}@{}", self.name(), version))
            .with_pr(ctx.pr.as_ref())
            .envs(self.dependency_env()?)
            .env("GOBIN", ctx.tv.install_path().join("bin"))
            .execute()?;

        Ok(())
    }
}

impl GoBackend {
    pub fn from_arg(ba: BackendArg) -> Self {
        Self {
            remote_version_cache: CacheManager::new(
                ba.cache_path.join("remote_versions-$KEY.msgpack.z"),
            ),
            ba,
        }
    }
}

fn trim_after_last_slash(s: &str) -> Option<&str> {
    match s.rsplit_once('/') {
        Some((new_path, _)) => Some(new_path),
        None => None,
    }
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct GoModInfo {
    versions: Vec<String>,
}
