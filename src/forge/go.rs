use std::fmt::Debug;

use crate::cache::CacheManager;
use crate::cli::args::ForgeArg;
use crate::cmd::CmdLineRunner;
use crate::config::{Config, Settings};
use crate::file;

use crate::forge::{Forge, ForgeType};
use crate::install_context::InstallContext;
use crate::toolset::ToolVersion;

#[derive(Debug)]
pub struct GoForge {
    fa: ForgeArg,
    remote_version_cache: CacheManager<Vec<String>>,
}

impl Forge for GoForge {
    fn get_type(&self) -> ForgeType {
        ForgeType::Go
    }

    fn fa(&self) -> &ForgeArg {
        &self.fa
    }

    fn get_dependencies(&self, _tv: &ToolVersion) -> eyre::Result<Vec<String>> {
        Ok(vec!["go".into()])
    }

    fn _list_remote_versions(&self) -> eyre::Result<Vec<String>> {
        self.remote_version_cache
            .get_or_try_init(|| {
                let mut mod_path = Some(self.name());

                while let Some(cur_mod_path) = mod_path {
                    let res = cmd!("go", "list", "-m", "-versions", "-json", cur_mod_path).read();
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

    fn ensure_dependencies_installed(&self) -> eyre::Result<()> {
        if !is_go_installed() {
            bail!(
                "go is not installed. Please install it in order to install {}",
                self.name()
            );
        }
        Ok(())
    }

    fn install_version_impl(&self, ctx: &InstallContext) -> eyre::Result<()> {
        let config = Config::try_get()?;
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
            .arg(&format!("{}@{}", self.name(), version))
            .with_pr(ctx.pr.as_ref())
            .envs(config.env()?)
            .env("GOBIN", ctx.tv.install_path().join("bin"))
            .execute()?;

        Ok(())
    }
}

impl GoForge {
    pub fn new(name: String) -> Self {
        let fa = ForgeArg::new(ForgeType::Go, &name);
        Self {
            remote_version_cache: CacheManager::new(
                fa.cache_path.join("remote_versions.msgpack.z"),
            ),
            fa,
        }
    }
}

fn trim_after_last_slash(s: &str) -> Option<&str> {
    match s.rsplit_once('/') {
        Some((new_path, _)) => Some(new_path),
        None => None,
    }
}

fn is_go_installed() -> bool {
    file::which("go").is_some()
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct GoModInfo {
    versions: Vec<String>,
}
