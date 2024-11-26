use std::fmt::Debug;

use crate::backend::backend_type::BackendType;
use crate::backend::Backend;
use crate::cli::args::BackendArg;
use crate::cmd::CmdLineRunner;
use crate::config::SETTINGS;
use crate::install_context::InstallContext;
use crate::toolset::ToolVersion;

#[derive(Debug)]
pub struct GoBackend {
    ba: BackendArg,
}

impl Backend for GoBackend {
    fn get_type(&self) -> BackendType {
        BackendType::Go
    }

    fn ba(&self) -> &BackendArg {
        &self.ba
    }

    fn get_dependencies(&self) -> eyre::Result<Vec<&str>> {
        Ok(vec!["go"])
    }

    fn _list_remote_versions(&self) -> eyre::Result<Vec<String>> {
        let mut mod_path = Some(self.tool_name());

        while let Some(cur_mod_path) = mod_path {
            let res = cmd!("go", "list", "-m", "-versions", "-json", &cur_mod_path)
                .full_env(self.dependency_env()?)
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
    }

    fn install_version_impl(
        &self,
        ctx: &InstallContext,
        tv: ToolVersion,
    ) -> eyre::Result<ToolVersion> {
        SETTINGS.ensure_experimental("go backend")?;

        let version = if tv.version.starts_with("v") {
            warn!("usage of a 'v' prefix in the version is discouraged");
            tv.version.to_string().replacen("v", "", 1)
        } else {
            tv.version.to_string()
        };

        let install = |v| {
            CmdLineRunner::new("go")
                .arg("install")
                .arg(format!("{}@{v}", self.tool_name()))
                .with_pr(ctx.pr.as_ref())
                .envs(self.dependency_env()?)
                .env("GOBIN", tv.install_path().join("bin"))
                .execute()
        };

        if install(format!("v{}", version)).is_err() {
            warn!("Failed to install, trying again without added 'v' prefix");
            install(version)?;
        }

        Ok(tv)
    }
}

impl GoBackend {
    pub fn from_arg(ba: BackendArg) -> Self {
        Self { ba }
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
