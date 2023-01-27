use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use color_eyre::eyre::{eyre, Result};
use owo_colors::{OwoColorize, Stream};

use crate::config::settings::Settings;
use crate::file::changed_within;
use crate::git::Git;
use crate::{dirs, file};

const ASDF_PLUGINS_REPO: &str = "https://github.com/asdf-vm/asdf-plugins";

#[derive(Debug)]
pub struct ShorthandRepo {
    repo_dir: PathBuf,
    plugin_repository_last_check_duration: Duration,
    disable_plugin_repository: bool,
}

impl ShorthandRepo {
    pub fn new(settings: &Settings) -> Self {
        Self {
            repo_dir: dirs::SHORTHAND_REPOSITORY.to_path_buf(),
            plugin_repository_last_check_duration: settings.plugin_repository_last_check_duration,
            disable_plugin_repository: settings.disable_plugin_short_name_repository,
        }
    }

    pub fn list_all(&self) -> Result<Vec<ShorthandRepoEntry>> {
        self.create_or_update()?;
        let mut output = vec![];
        for dir in self.repo_dir.join("plugins").read_dir()? {
            let file_name = dir?.file_name().to_string_lossy().to_string();
            output.push(ShorthandRepoEntry {
                url: self.lookup(&file_name)?,
                name: file_name,
            });
        }
        output.sort_by(|a, b| a.name.cmp(&b.name));

        Ok(output)
    }

    pub fn lookup(&self, name: &str) -> Result<String> {
        if !self.disable_plugin_repository {
            self.create_or_update()?;
            let plugin = self.repo_dir.join("plugins").join(name);
            let file = fs::read_to_string(plugin).unwrap_or_default();
            for line in file.split('\n') {
                if line.starts_with("repository") {
                    let value = line.split('=').nth(1).unwrap().trim();
                    return Ok(value.to_string());
                }
            }
        }

        Err(eyre!(
            "No plugin found for {}",
            name.if_supports_color(Stream::Stderr, |t| t.cyan())
        ))
    }

    pub fn create_or_update(&self) -> Result<()> {
        self.ensure_created()?;
        if !self.changed_recently()? {
            eprintln!("rtx: Updating shorthand plugins repository...");
            let git = self.get_git();
            git.update(None)?;
            file::touch_dir(&self.repo_dir)?;
        }

        Ok(())
    }

    pub fn ensure_created(&self) -> Result<()> {
        if !self.repo_dir.exists() {
            eprint!("rtx: Cloning shorthand plugins repository...");
            self.get_git().clone(ASDF_PLUGINS_REPO)?;
            eprintln!(" done");
        }
        Ok(())
    }

    /// returns true if repo_dir was modified less than 24 hours ago
    fn changed_recently(&self) -> Result<bool> {
        changed_within(&self.repo_dir, self.plugin_repository_last_check_duration)
    }

    fn get_git(&self) -> Git {
        Git::new(self.repo_dir.clone())
    }
}

pub struct ShorthandRepoEntry {
    pub name: String,
    pub url: String,
}

#[cfg(test)]
mod tests {
    use filetime::FileTime;
    use insta::assert_display_snapshot;
    use pretty_assertions::assert_str_eq;

    use super::*;

    #[test]
    fn test_lookup() {
        let shr = ShorthandRepo::new(&Settings::default());
        let url = shr.lookup("ruby").unwrap();
        assert_str_eq!(url, "https://github.com/asdf-vm/asdf-ruby.git");
    }

    #[test]
    fn test_lookup_err() {
        let shr = ShorthandRepo::new(&Settings::default());
        let err = shr.lookup("xxruby").unwrap_err();
        assert_display_snapshot!(err);
    }

    #[test]
    fn test_update() {
        let shr = ShorthandRepo::new(&Settings::default());
        let url = shr.lookup("ruby").unwrap();
        assert_str_eq!(url, "https://github.com/asdf-vm/asdf-ruby.git");
        filetime::set_file_times(shr.repo_dir.clone(), FileTime::zero(), FileTime::zero()).unwrap();
        let url = shr.lookup("ruby").unwrap();
        assert_str_eq!(url, "https://github.com/asdf-vm/asdf-ruby.git");
    }

    #[test]
    fn test_create() {
        let dir = tempfile::tempdir().unwrap();
        let mut shr = ShorthandRepo::new(&Settings::default());
        shr.repo_dir = dir.path().join("shr");
        let url = shr.lookup("ruby").unwrap();
        assert_str_eq!(url, "https://github.com/asdf-vm/asdf-ruby.git");
    }
}
