use std::path::{Path, PathBuf};
use std::{
    fmt::{Display, Formatter},
    sync::{Arc, Mutex},
};

use console::{Alignment, measure_text_width, pad_str};
use eyre::Result;
use indexmap::IndexMap;
use itertools::Itertools;
use tera::Context;

use crate::cli::args::BackendArg;
use crate::config::config_file::ConfigFile;
use crate::file;
use crate::file::display_path;
use crate::tera::{BASE_CONTEXT, get_tera};
use crate::toolset::{ToolRequest, ToolRequestSet, ToolSource};

use super::ConfigFileType;

// python 3.11.0 3.10.0
// shellcheck 0.9.0
// shfmt 3.6.0

/// represents asdf's .tool-versions file
#[derive(Debug, Default)]
pub struct ToolVersions {
    context: Context,
    path: PathBuf,
    pre: String,
    plugins: Mutex<IndexMap<BackendArg, ToolVersionPlugin>>,
    tools: Mutex<ToolRequestSet>,
}

#[derive(Debug, Clone)]
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
            tools: Mutex::new(ToolRequestSet::new()),
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
        let dir = path.parent();
        let s = get_tera(dir).render_str(s, &cf.context)?;
        for line in s.lines() {
            if !line.trim_start().starts_with('#') {
                break;
            }
            cf.pre.push_str(line);
            cf.pre.push('\n');
        }

        cf.plugins = Mutex::new(Self::parse_plugins(&s));
        cf.populate_toolset()?;
        trace!("{cf}");
        Ok(cf)
    }

    fn parse_plugins(input: &str) -> IndexMap<BackendArg, ToolVersionPlugin> {
        let mut plugins: IndexMap<BackendArg, ToolVersionPlugin> = IndexMap::new();
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
                let ba = orig_plugin.into();

                let tvp = ToolVersionPlugin {
                    orig_name: orig_plugin.to_string(),
                    versions: parts.map(|v| v.to_string()).collect(),
                    post: match post {
                        "" => String::from("\n"),
                        _ => [" #", post, "\n"].join(""),
                    },
                };
                plugins.insert(ba, tvp);
            }
        }
        plugins
    }

    fn add_version(
        &self,
        plugins: &mut IndexMap<BackendArg, ToolVersionPlugin>,
        fa: &BackendArg,
        version: String,
    ) {
        get_or_create_plugin(plugins, fa).versions.push(version);
    }

    fn populate_toolset(&self) -> eyre::Result<()> {
        let source = ToolSource::ToolVersions(self.path.clone());
        for (ba, tvp) in &*self.plugins.lock().unwrap() {
            for version in &tvp.versions {
                let tvr = ToolRequest::new(Arc::new(ba.clone()), version, source.clone())?;
                self.tools.lock().unwrap().add_version(tvr, &source)
            }
        }
        Ok(())
    }
}

impl Display for ToolVersions {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        let plugins = &self
            .plugins
            .lock()
            .unwrap()
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
    fn config_type(&self) -> ConfigFileType {
        ConfigFileType::ToolVersions
    }

    fn get_path(&self) -> &Path {
        self.path.as_path()
    }

    fn remove_tool(&self, fa: &BackendArg) -> Result<()> {
        self.plugins.lock().unwrap().shift_remove(fa);
        Ok(())
    }

    fn replace_versions(&self, fa: &BackendArg, versions: Vec<ToolRequest>) -> eyre::Result<()> {
        let mut plugins = self.plugins.lock().unwrap();
        get_or_create_plugin(&mut plugins, fa).versions.clear();
        for tr in versions {
            if !tr.options().is_empty() {
                warn!("tool options are not supported in .tool-versions files");
            }
            self.add_version(&mut plugins, fa, tr.version());
        }
        Ok(())
    }

    fn save(&self) -> Result<()> {
        let s = self.dump()?;
        file::write(&self.path, s)
    }

    fn dump(&self) -> eyre::Result<String> {
        let mut s = self.pre.clone();

        let plugins = self.plugins.lock().unwrap();
        let max_plugin_len = plugins
            .keys()
            .map(|p| measure_text_width(&p.to_string()))
            .max()
            .unwrap_or_default();
        for (_, tv) in &*plugins {
            let mut plugin = tv.orig_name.to_string();
            if plugin == "node" {
                plugin = "nodejs".into();
            } else if plugin == "go" {
                plugin = "golang".into();
            }
            let plugin = pad_str(&plugin, max_plugin_len, Alignment::Left, None);
            s.push_str(&format!("{} {}{}", plugin, tv.versions.join(" "), tv.post));
        }

        Ok(s.trim_end().to_string() + "\n")
    }

    fn source(&self) -> ToolSource {
        ToolSource::ToolVersions(self.path.clone())
    }

    fn to_tool_request_set(&self) -> eyre::Result<ToolRequestSet> {
        Ok(self.tools.lock().unwrap().clone())
    }
}

fn get_or_create_plugin<'a>(
    plugins: &'a mut IndexMap<BackendArg, ToolVersionPlugin>,
    fa: &BackendArg,
) -> &'a mut ToolVersionPlugin {
    plugins
        .entry(fa.clone())
        .or_insert_with(|| ToolVersionPlugin {
            orig_name: fa.short.to_string(),
            versions: vec![],
            post: "".into(),
        })
}

impl Clone for ToolVersions {
    fn clone(&self) -> Self {
        Self {
            context: self.context.clone(),
            path: self.path.clone(),
            pre: self.pre.clone(),
            plugins: Mutex::new(self.plugins.lock().unwrap().clone()),
            tools: Mutex::new(self.tools.lock().unwrap().clone()),
        }
    }
}
