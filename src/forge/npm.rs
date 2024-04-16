use std::fmt::Debug;

use crate::cache::CacheManager;
use crate::cli::args::ForgeArg;
use crate::cmd::CmdLineRunner;
use crate::config::{Config, Settings};

use crate::forge::{Forge, ForgeType};
use crate::install_context::InstallContext;
use crate::toolset::ToolVersion;
use serde_json::Value;

#[derive(Debug)]
pub struct NPMForge {
    fa: ForgeArg,
    remote_version_cache: CacheManager<Vec<String>>,
    latest_version_cache: CacheManager<Option<String>>,
}

impl Forge for NPMForge {
    fn get_type(&self) -> ForgeType {
        ForgeType::Npm
    }

    fn fa(&self) -> &ForgeArg {
        &self.fa
    }

    fn get_dependencies(&self, _tv: &ToolVersion) -> eyre::Result<Vec<String>> {
        Ok(vec!["node".into()])
    }

    fn list_remote_versions(&self) -> eyre::Result<Vec<String>> {
        self.remote_version_cache
            .get_or_try_init(|| {
                let raw = cmd!("npm", "view", self.name(), "versions", "--json").read()?;
                let versions: Vec<String> = serde_json::from_str(&raw)?;
                Ok(versions)
            })
            .cloned()
    }

    fn latest_stable_version(&self) -> eyre::Result<Option<String>> {
        self.latest_version_cache
            .get_or_try_init(|| {
                let raw = cmd!("npm", "view", self.name(), "dist-tags", "--json").read()?;
                let dist_tags: Value = serde_json::from_str(&raw)?;
                let latest = match dist_tags["latest"] {
                    Value::String(ref s) => Some(s.clone()),
                    _ => self.latest_version(Some("latest".into())).unwrap(),
                };
                Ok(latest)
            })
            .cloned()
    }

    fn install_version_impl(&self, ctx: &InstallContext) -> eyre::Result<()> {
        let config = Config::try_get()?;
        let settings = Settings::get();
        settings.ensure_experimental("npm backend")?;

        CmdLineRunner::new("npm")
            .arg("install")
            .arg("-g")
            .arg(&format!("{}@{}", self.name(), ctx.tv.version))
            .arg("--prefix")
            .arg(ctx.tv.install_path())
            .with_pr(ctx.pr.as_ref())
            .envs(config.env()?)
            .prepend_path(ctx.ts.list_paths())?
            .execute()?;

        Ok(())
    }
}

impl NPMForge {
    pub fn new(name: String) -> Self {
        let fa = ForgeArg::new(ForgeType::Npm, &name);
        Self {
            remote_version_cache: CacheManager::new(
                fa.cache_path.join("remote_versions.msgpack.z"),
            ),
            latest_version_cache: CacheManager::new(fa.cache_path.join("latest_version.msgpack.z")),
            fa,
        }
    }

    pub fn from_dirname(dirname: String) -> Self {
        NPMForge::new(un_dirname(dirname))
    }
}

// NPM packages can have dashes and slashes in their name.
// - If scoped, replace first dash after the @ with a slash. Will not work for scopes using dashes
fn un_dirname(dirname: String) -> String {
    if dirname.contains('@') {
        return dirname.replacen('-', "/", 1);
    }
    dirname
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_dirname() {
        let dirnames = vec![
            ("@scope-my-package", "@scope/my-package"),
            ("my-package", "my-package"),
        ];
        for (dirname, name) in dirnames {
            let npm_forge = NPMForge::from_dirname(dirname.to_string());
            assert_eq!(npm_forge.fa().forge_type, ForgeType::Npm);
            assert_eq!(npm_forge.fa().name, name);
        }
    }
}
