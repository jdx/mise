use std::collections::HashMap;
use std::fmt::{Display, Formatter};
use std::fs;
use std::fs::read_to_string;
use std::path::{Path, PathBuf};

use crate::config::AliasMap;
use color_eyre::eyre::Result;
use console::{measure_text_width, pad_str, Alignment};
use indexmap::IndexMap;
use itertools::Itertools;
use tera::Context;

use crate::config::config_file::{ConfigFile, ConfigFileType};
use crate::config::settings::SettingsBuilder;
use crate::file::display_path;
use crate::plugins::{unalias_plugin, PluginName};
use crate::tera::{get_tera, BASE_CONTEXT};
use crate::toolset::{ToolSource, ToolVersionRequest, Toolset};

// python 3.11.0 3.10.0
// shellcheck 0.9.0
// shfmt 3.6.0

/// represents asdf's .tool-versions file
#[derive(Debug, Default)]
pub struct ToolVersions {
    context: Context,
    path: PathBuf,
    pre: String,
    plugins: IndexMap<PluginName, ToolVersionPlugin>,
    toolset: Toolset,
    is_trusted: bool,
}

#[derive(Debug, Default)]
struct ToolVersionPlugin {
    versions: Vec<String>,
    post: String,
}

impl ToolVersions {
    pub fn init(filename: &Path, is_trusted: bool) -> ToolVersions {
        let mut context = BASE_CONTEXT.clone();
        context.insert("config_root", filename.parent().unwrap().to_str().unwrap());
        ToolVersions {
            context,
            toolset: Toolset::new(ToolSource::ToolVersions(filename.to_path_buf())),
            path: filename.to_path_buf(),
            is_trusted,
            ..Default::default()
        }
    }

    pub fn from_file(path: &Path, is_trusted: bool) -> Result<Self> {
        trace!("parsing tool-versions: {}", path.display());
        Self::parse_str(&read_to_string(path)?, path.to_path_buf(), is_trusted)
    }

    pub fn parse_str(s: &str, path: PathBuf, is_trusted: bool) -> Result<Self> {
        let mut cf = Self::init(&path, is_trusted);
        let dir = path.parent().unwrap();
        let s = if cf.is_trusted {
            get_tera(dir).render_str(s, &cf.context)?
        } else {
            s.to_string()
        };
        for line in s.lines() {
            if !line.trim_start().starts_with('#') {
                break;
            }
            cf.pre.push_str(line);
            cf.pre.push('\n');
        }

        cf.plugins = Self::parse_plugins(&s)?;
        cf.populate_toolset();
        Ok(cf)
    }

    fn get_or_create_plugin(&mut self, plugin: &str) -> &mut ToolVersionPlugin {
        self.plugins.entry(plugin.to_string()).or_default()
    }

    fn parse_plugins(input: &str) -> Result<IndexMap<PluginName, ToolVersionPlugin>> {
        let mut plugins: IndexMap<PluginName, ToolVersionPlugin> = IndexMap::new();
        for line in input.lines() {
            if line.trim_start().starts_with('#') {
                if let Some(prev) = &mut plugins.values_mut().last() {
                    prev.post.push_str(line);
                    prev.post.push('\n');
                }
                continue;
            }
            let (line, post) = line.split_once('#').unwrap_or((line, ""));
            let mut parts = line.split_whitespace();
            if let Some(plugin) = parts.next() {
                // handle invalid trailing colons in `.tool-versions` files
                // note that this method will cause the colons to be removed
                // permanently if saving the file again, but I think that's fine
                let plugin = unalias_plugin(plugin.trim_end_matches(':'));

                let tvp = ToolVersionPlugin {
                    versions: parts.map(|v| v.to_string()).collect(),
                    post: match post {
                        "" => String::from("\n"),
                        _ => [" #", post, "\n"].join(""),
                    },
                };
                plugins.insert(plugin.to_string(), tvp);
            }
        }
        Ok(plugins)
    }

    fn add_version(&mut self, plugin: &PluginName, version: &str) {
        self.get_or_create_plugin(plugin)
            .versions
            .push(version.to_string());
    }

    fn populate_toolset(&mut self) {
        for (plugin, tvp) in &self.plugins {
            for version in &tvp.versions {
                let tvr = ToolVersionRequest::new(plugin.clone(), version);
                self.toolset.add_version(tvr, Default::default())
            }
        }
    }
}

impl Display for ToolVersions {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        let plugins = &self
            .plugins
            .iter()
            .map(|(p, v)| format!("{}@{}", p, v.versions.join("|")))
            .collect_vec();
        write!(
            f,
            "ToolVersions({}): {}",
            display_path(&self.path),
            plugins.join(", ")
        )
    }
}

impl ConfigFile for ToolVersions {
    fn get_type(&self) -> ConfigFileType {
        ConfigFileType::ToolVersions
    }

    fn get_path(&self) -> &Path {
        self.path.as_path()
    }

    fn plugins(&self) -> HashMap<PluginName, String> {
        Default::default()
    }

    fn env(&self) -> HashMap<PluginName, String> {
        HashMap::new()
    }

    fn path_dirs(&self) -> Vec<PathBuf> {
        vec![]
    }

    fn remove_plugin(&mut self, plugin: &PluginName) {
        self.plugins.remove(plugin);
    }

    fn replace_versions(&mut self, plugin_name: &PluginName, versions: &[String]) {
        self.get_or_create_plugin(plugin_name).versions.clear();
        for version in versions {
            self.add_version(plugin_name, version);
        }
    }

    fn save(&self) -> Result<()> {
        let s = self.dump();
        Ok(fs::write(&self.path, s)?)
    }

    fn dump(&self) -> String {
        let mut s = self.pre.clone();

        let max_plugin_len = self
            .plugins
            .keys()
            .map(|p| measure_text_width(p))
            .max()
            .unwrap_or_default();
        for (plugin, tv) in &self.plugins {
            let plugin = pad_str(plugin, max_plugin_len, Alignment::Left, None);
            s.push_str(&format!("{} {}{}", plugin, tv.versions.join(" "), tv.post));
        }

        s.trim_end().to_string() + "\n"
    }

    fn to_toolset(&self) -> &Toolset {
        &self.toolset
    }

    fn settings(&self) -> SettingsBuilder {
        SettingsBuilder::default()
    }

    fn aliases(&self) -> AliasMap {
        AliasMap::default()
    }

    fn watch_files(&self) -> Vec<PathBuf> {
        vec![self.path.clone()]
    }
}

#[cfg(test)]
pub(crate) mod tests {

    use indoc::indoc;
    use insta::{assert_display_snapshot, assert_snapshot};
    use pretty_assertions::assert_eq;

    use crate::dirs;

    use super::*;

    #[test]
    fn test_parse() {
        let tv = ToolVersions::from_file(dirs::CURRENT.join(".test-tool-versions").as_path(), true)
            .unwrap();
        assert_eq!(tv.path, dirs::CURRENT.join(".test-tool-versions"));
        assert_display_snapshot!(tv, @"ToolVersions(~/cwd/.test-tool-versions): tiny@3");
    }

    #[test]
    fn test_parse_comments() {
        let orig = indoc! {"
        # intro comment
        python 3.11.0 3.10.0 # some comment # more comment
        #shellcheck 0.9.0
        shfmt  3.6.0
        # tail comment
        "};
        let path = dirs::CURRENT.join(".test-tool-versions");
        let tv = ToolVersions::parse_str(orig, path, false).unwrap();
        assert_eq!(tv.dump(), orig);
    }

    #[test]
    fn test_parse_colon() {
        let orig = indoc! {"
        ruby: 3.0.5
        "};
        let path = dirs::CURRENT.join(".test-tool-versions");
        let tv = ToolVersions::parse_str(orig, path, false).unwrap();
        assert_snapshot!(tv.dump(), @r###"
        ruby 3.0.5
        "###);
    }

    #[test]
    fn test_parse_tera() {
        let orig = indoc! {"
        ruby {{'3.0.5'}}
        python {{exec(command='echo 3.11.0')}}
        "};
        let path = dirs::CURRENT.join(".test-tool-versions");
        let tv = ToolVersions::parse_str(orig, path, true).unwrap();
        assert_snapshot!(tv.dump(), @r###"
        ruby   3.0.5
        python 3.11.0
        "###);
    }

    #[test]
    fn test_from_toolset() {
        let orig = indoc! {"
        ruby: 3.0.5 3.1
        "};
        let path = dirs::CURRENT.join(".test-tool-versions");
        let tv = ToolVersions::parse_str(orig, path, true).unwrap();
        assert_display_snapshot!(tv.to_toolset(), @"ruby@3.0.5 ruby@3.1");
    }
}
