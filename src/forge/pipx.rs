use std::fmt::Debug;

use crate::cache::CacheManager;
use crate::cli::args::ForgeArg;
use crate::cmd::CmdLineRunner;
use crate::config::{Config, Settings};

use crate::forge::{Forge, ForgeType};
use crate::install_context::InstallContext;
use crate::toolset::ToolVersion;

#[derive(Debug)]
pub struct PIPXForge {
    fa: ForgeArg,
    remote_version_cache: CacheManager<Vec<String>>,
    latest_version_cache: CacheManager<Option<String>>,
}

impl Forge for PIPXForge {
    fn get_type(&self) -> ForgeType {
        ForgeType::Pipx
    }

    fn fa(&self) -> &ForgeArg {
        &self.fa
    }

    fn get_dependencies(&self, _tv: &ToolVersion) -> eyre::Result<Vec<String>> {
        Ok(vec!["pipx".into()])
    }

    // NOTE: there isn't as sensible a 'list versions' unless we assume it's reading
    // from a git repo, in which case it could fetch git refs and read tags for semvar tags
    fn list_remote_versions(&self) -> eyre::Result<Vec<String>> {
        self.remote_version_cache
            .get_or_try_init(|| Ok(vec!["latest".to_string()]))
            .cloned()
    }

    // NOTE: see note above about irrelevance of a list_stable_version
    fn latest_stable_version(&self) -> eyre::Result<Option<String>> {
        self.latest_version_cache
            .get_or_try_init(|| {
                let latest = self.latest_version(Some("latest".into())).unwrap();
                Ok(latest)
            })
            .cloned()
    }

    fn install_version_impl(&self, ctx: &InstallContext) -> eyre::Result<()> {
        let config = Config::try_get()?;
        let settings = Settings::get();
        settings.ensure_experimental("pipx backend")?;
        let project_name = transform_project_name(ctx, self.name());

        // Get environmental variable PATH and append to it
        // Path is listed correctly /Users/zph/.local/share/mise/installs/pipx-zph-runbook/latest/bin
        CmdLineRunner::new("pipx")
            .arg("install")
            .arg(project_name)
            .with_pr(ctx.pr.as_ref())
            .env("PIPX_HOME", ctx.tv.install_path())
            .env("PIPX_BIN_DIR", ctx.tv.install_path().join("bin"))
            .envs(config.env()?)
            .prepend_path(ctx.ts.list_paths())?
            .execute()?;

        Ok(())
    }
}

impl PIPXForge {
    pub fn new(name: String) -> Self {
        let fa = ForgeArg::new(ForgeType::Pipx, &name);
        Self {
            remote_version_cache: CacheManager::new(
                fa.cache_path.join("remote_versions.msgpack.z"),
            ),
            latest_version_cache: CacheManager::new(fa.cache_path.join("latest_version.msgpack.z")),
            fa,
        }
    }
}

// Used to normalize name because when `latest` the name should not include a ref for pipx
// eg when passing user/project it should create git+https://github.com/user/project
// NOT git+https://github.com/user/project@latest because pipx cannot handle that directive
// If given a non-shorthand we treat it as intentional and do not transform it
/*
 * Supports the following formats
 * - git+https://github.com/psf/black.git@24.2.0 => github longhand and version
 * - psf/black@24.2.0 => github shorthand and version
 * - black@24.2.0 => pypi shorthand and version
 */
fn transform_project_name(ctx: &InstallContext, name: &str) -> String {
    debug!("transform_project_name: name={} ctx={:?}", name, ctx.tv);
    let parts: Vec<&str> = name.split('/').collect();
    match (
        name,
        name.starts_with("git+http"),
        parts.len(),
        ctx.tv.version.as_str(),
    ) {
        (_, false, 2, "latest") => format!("git+https://github.com/{}.git", name),
        (_, false, 2, v) => format!("git+https://github.com/{}.git@{}", name, v),
        (_, true, _, "latest") => name.to_string(),
        (_, true, _, v) => format!("{}@{}", name, v),
        (".", false, _, _) => name.to_string(),
        (_, false, _, "latest") => name.to_string(),
        // Treat this as a pypi package@version
        (_, false, 1, v) => format!("{}=={}", name, v),
        _ => name.to_string(),
    }
}
