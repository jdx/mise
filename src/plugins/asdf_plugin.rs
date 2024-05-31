use crate::config::Settings;
use crate::dirs;
use crate::git::Git;
use crate::plugins::{Plugin, PluginList, PluginType};
use rayon::prelude::*;
use xx::file;

#[derive(Debug)]
pub struct AsdfPlugin {
    pub name: String,
    pub repo: Git,
    pub repo_url: Option<String>,
}

impl AsdfPlugin {
    pub fn new(name: String) -> Self {
        let dir = dirs::PLUGINS.join(&name);
        Self {
            name,
            repo: Git::new(dir),
            repo_url: None,
        }
    }

    pub fn list() -> eyre::Result<PluginList> {
        let settings = Settings::get();
        Ok(file::ls(*dirs::PLUGINS)?
            .into_par_iter()
            .map(|dir| {
                let name = dir.file_name().unwrap().to_string_lossy().to_string();
                Box::new(AsdfPlugin::new(name)) as Box<dyn Plugin>
            })
            .filter(|p| !settings.disable_tools.contains(p.name()))
            .collect())
    }
}

impl Plugin for AsdfPlugin {
    fn name(&self) -> &str {
        &self.name
    }

    fn get_plugin_type(&self) -> PluginType {
        PluginType::Asdf
    }

    fn get_remote_url(&self) -> eyre::Result<Option<String>> {
        let url = self.repo.get_remote_url();
        Ok(url.or(self.repo_url.clone()))
    }

    fn current_abbrev_ref(&self) -> eyre::Result<Option<String>> {
        if !self.is_installed() {
            return Ok(None);
        }
        self.repo.current_abbrev_ref().map(Some)
    }

    fn current_sha_short(&self) -> eyre::Result<Option<String>> {
        if !self.is_installed() {
            return Ok(None);
        }
        self.repo.current_sha_short().map(Some)
    }

    fn is_installed(&self) -> bool {
        self.repo.exists()
    }
}
