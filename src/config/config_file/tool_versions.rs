use std::fmt::{Display, Formatter};
use std::path::{Path, PathBuf};

use console::{measure_text_width, pad_str, Alignment};
use eyre::Result;
use indexmap::IndexMap;
use itertools::Itertools;
use tera::Context;

use crate::cli::args::ForgeArg;
use crate::config::config_file;
use crate::config::config_file::{ConfigFile, ConfigFileType};
use crate::file;
use crate::file::display_path;
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
    plugins: IndexMap<ForgeArg, ToolVersionPlugin>,
    toolset: Toolset,
}

#[derive(Debug)]
struct ToolVersionPlugin {
    orig_name: String,
    versions: Vec<String>,
    post: String,
}

impl ToolVersions {
    pub fn init(filename: &Path) -> ToolVersions {
        let mut context = BASE_CONTEXT.clone();
        context.insert("config_root", filename.parent().unwrap().to_str().unwrap());
        ToolVersions {
            context,
            toolset: Toolset::new(ToolSource::ToolVersions(filename.to_path_buf())),
            path: filename.to_path_buf(),
            ..Default::default()
        }
    }

    pub fn from_file(path: &Path) -> Result<Self> {
        trace!("parsing tool-versions: {}", path.display());
        Self::parse_str(&file::read_to_string(path)?, path.to_path_buf())
    }

    pub fn parse_str(s: &str, path: PathBuf) -> Result<Self> {
        let mut cf = Self::init(&path);
        let dir = path.parent().unwrap();
        let s = if config_file::is_trusted(&path) {
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
        trace!("{cf}");
        Ok(cf)
    }

    fn get_or_create_plugin(&mut self, fa: &ForgeArg) -> &mut ToolVersionPlugin {
        self.plugins
            .entry(fa.clone())
            .or_insert_with(|| ToolVersionPlugin {
                orig_name: fa.to_string(),
                versions: vec![],
                post: "".into(),
            })
    }

    fn parse_plugins(input: &str) -> Result<IndexMap<ForgeArg, ToolVersionPlugin>> {
        let mut plugins: IndexMap<ForgeArg, ToolVersionPlugin> = IndexMap::new();
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
                let orig_plugin = plugin.trim_end_matches(':');
                let fa = orig_plugin.parse()?;

                let tvp = ToolVersionPlugin {
                    orig_name: orig_plugin.to_string(),
                    versions: parts.map(|v| v.to_string()).collect(),
                    post: match post {
                        "" => String::from("\n"),
                        _ => [" #", post, "\n"].join(""),
                    },
                };
                plugins.insert(fa, tvp);
            }
        }
        Ok(plugins)
    }

    fn add_version(&mut self, fa: &ForgeArg, version: &str) {
        self.get_or_create_plugin(fa)
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

    fn remove_plugin(&mut self, fa: &ForgeArg) {
        self.plugins.remove(fa);
    }

    fn replace_versions(&mut self, fa: &ForgeArg, versions: &[String]) {
        self.get_or_create_plugin(fa).versions.clear();
        for version in versions {
            self.add_version(fa, version);
        }
    }

    fn save(&self) -> Result<()> {
        let s = self.dump();
        file::write(&self.path, s)
    }

    fn dump(&self) -> String {
        let mut s = self.pre.clone();

        let max_plugin_len = self
            .plugins
            .keys()
            .map(|p| measure_text_width(&p.to_string()))
            .max()
            .unwrap_or_default();
        for (_, tv) in &self.plugins {
            let plugin = pad_str(&tv.orig_name, max_plugin_len, Alignment::Left, None);
            s.push_str(&format!("{} {}{}", plugin, tv.versions.join(" "), tv.post));
        }

        s.trim_end().to_string() + "\n"
    }

    fn to_toolset(&self) -> &Toolset {
        &self.toolset
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use pretty_assertions::assert_eq;

    use crate::env;

    use super::*;

    #[test]
    fn test_parse() {
        let tv = ToolVersions::from_file(
            env::current_dir()
                .unwrap()
                .join(".test-tool-versions")
                .as_path(),
        )
        .unwrap();
        assert_eq!(
            tv.path,
            env::current_dir().unwrap().join(".test-tool-versions")
        );
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
        let path = env::current_dir().unwrap().join(".test-tool-versions");
        let tv = ToolVersions::parse_str(orig, path).unwrap();
        assert_eq!(tv.dump(), orig);
    }

    #[test]
    fn test_parse_colon() {
        let orig = indoc! {"
        ruby: 3.0.5
        "};
        let path = env::current_dir().unwrap().join(".test-tool-versions");
        let tv = ToolVersions::parse_str(orig, path).unwrap();
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
        let path = env::current_dir().unwrap().join(".test-tool-versions");
        assert_cli!("trust", path.to_string_lossy());
        let tv = ToolVersions::parse_str(orig, path.clone()).unwrap();
        assert_cli!("trust", "--untrust", path.to_string_lossy());
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
        let path = env::current_dir().unwrap().join(".test-tool-versions");
        let tv = ToolVersions::parse_str(orig, path).unwrap();
        assert_display_snapshot!(tv.to_toolset(), @"ruby@3.0.5 ruby@3.1");
    }
}
