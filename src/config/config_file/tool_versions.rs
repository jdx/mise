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

use crate::args::BackendArg;
use crate::config::config_file::{ConfigFile, ensure_versions_are_not_templates, trust_check};
use crate::file;
use crate::file::display_path;
use crate::tera::{BASE_CONTEXT, contains_template_syntax, get_tera, render_config_str};
use crate::toolset::{ToolRequest, ToolRequestSet, ToolSource};

// python 3.11.0 3.10.0
// shellcheck 0.9.0
// shfmt 3.6.0

/// represents asdf's .tool-versions file
#[derive(Debug, Default)]
pub(crate) struct ToolVersions {
    context: Context,
    path: PathBuf,
    pre: String,
    plugins: Mutex<IndexMap<BackendArg, ToolVersionPlugin>>,
    tools: Mutex<ToolRequestSet>,
    /// Skipped because it links to a file of another name; never written back.
    unread: bool,
    /// Rendered from templates whose lines cannot be told apart in the result
    /// (`{% ... %}` or `{# ... #}`, or a value that spans lines); never written back.
    unmapped_templates: bool,
}

#[derive(Debug, Clone)]
struct ToolVersionPlugin {
    orig_name: String,
    versions: Vec<String>,
    post: String,
    /// The tool's line as written, up to any comment, when it holds a template.
    /// It is written back as it is until the tool's versions change.
    raw: Option<String>,
}

impl ToolVersions {
    pub(crate) fn init(filename: &Path) -> ToolVersions {
        let mut context = BASE_CONTEXT.clone();
        context.insert("config_root", &filename.parent().unwrap().to_string_lossy());
        context.insert(
            "config_source",
            &crate::config::config_file::config_root::config_source(filename),
        );
        ToolVersions {
            context,
            tools: Mutex::new(ToolRequestSet::new()),
            path: filename.to_path_buf(),
            ..Default::default()
        }
    }

    pub(crate) fn from_file(path: &Path) -> Result<Self> {
        trace!("parsing tool-versions: {}", path.display());
        if super::diagnostic::links_to_another_name(path) {
            super::diagnostic::warn_linked_version_file(path, |target| {
                matches!(
                    super::detect_config_file_type_by_filename(target),
                    Some(super::ConfigFileType::ToolVersions)
                )
            });
            return Ok(Self {
                unread: true,
                ..Self::init(path)
            });
        }
        let body = file::read_config_to_string(path)?;
        // The first whitespace-separated token on a line is the tool name, so a leading
        // byte-order mark becomes part of it and the entry silently resolves to nothing.
        Self::parse_str(file::strip_utf8_bom(&body), path.to_path_buf())
    }

    pub(crate) fn path_requires_trust(path: &Path) -> bool {
        match file::read_config_to_string(path) {
            Ok(body) => body_requires_trust(file::strip_utf8_bom(&body)),
            Err(_) => true,
        }
    }

    pub(crate) fn parse_str(s: &str, path: PathBuf) -> Result<Self> {
        let mut cf = Self::init(&path);
        let dir = path.parent();
        if body_requires_trust(s) {
            trust_check(&path)?;
        }
        let raw = s;
        let s = if contains_template_syntax(s) {
            let mut tera = get_tera(dir);
            render_config_str(&mut tera, s, &cf.context, &path)?
        } else {
            s.to_string()
        };
        // Each line of the file renders to the line of the same number, unless a tag or a
        // value spans lines. Comments and the lines of tools a command does not change are
        // then written back from the file as written, so saving keeps their templates.
        let mapped =
            raw.lines().count() == s.lines().count() && !raw.contains("{%") && !raw.contains("{#");
        cf.unmapped_templates = !mapped;
        let raw = if mapped { raw } else { s.as_str() };
        for line in raw.lines() {
            if !line.trim_start().starts_with('#') {
                break;
            }
            cf.pre.push_str(line);
            cf.pre.push('\n');
        }

        cf.plugins = Mutex::new(Self::parse_lines(s.lines().zip(raw.lines())));
        cf.populate_toolset()?;
        trace!("{cf}");
        Ok(cf)
    }

    #[cfg(test)]
    fn parse_plugins(input: &str) -> IndexMap<BackendArg, ToolVersionPlugin> {
        Self::parse_lines(input.lines().zip(input.lines()))
    }

    /// Tools from pairs of a rendered line and the line as written.
    fn parse_lines<'a>(
        lines: impl Iterator<Item = (&'a str, &'a str)>,
    ) -> IndexMap<BackendArg, ToolVersionPlugin> {
        let mut plugins: IndexMap<BackendArg, ToolVersionPlugin> = IndexMap::new();
        for (line, raw_line) in lines {
            if raw_line.trim_start().starts_with('#') || line.trim_start().starts_with('#') {
                if let Some(prev) = &mut plugins.values_mut().last() {
                    prev.post.push_str(raw_line);
                    prev.post.push('\n');
                }
                continue;
            }
            let raw_tool = raw_line.split_once('#').map_or(raw_line, |(tool, _)| tool);
            let raw = (raw_line != line).then(|| raw_tool.trim_end().to_string());
            let post = raw_line.split_once('#').map_or("", |(_, post)| post);
            let (line, _) = line.split_once('#').unwrap_or((line, ""));
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
                    raw,
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
    fn get_path(&self) -> &Path {
        self.path.as_path()
    }

    fn remove_tool(&self, fa: &BackendArg) -> Result<()> {
        self.plugins.lock().unwrap().shift_remove(fa);
        Ok(())
    }

    fn replace_versions(&self, fa: &BackendArg, versions: Vec<ToolRequest>) -> eyre::Result<()> {
        ensure_versions_are_not_templates(&self.path, fa, &versions)?;
        let mut plugins = self.plugins.lock().unwrap();
        let plugin = get_or_create_plugin(&mut plugins, fa);
        if let Some(raw) = plugin.raw.take() {
            // Its versions are rewritten from what they rendered to. A template in the
            // name would be replaced by its value as well.
            if raw.split_whitespace().next() != Some(plugin.orig_name.as_str()) {
                plugin.raw = Some(raw);
                eyre::bail!(
                    "not writing {}: the line of {fa} is written with a template, which mise would replace with the value it renders to. Edit the file by hand",
                    display_path(&self.path)
                );
            }
        }
        plugin.versions.clear();
        for tr in versions {
            if !tr.options().is_empty() {
                warn!("tool options are not supported in .tool-versions files");
            }
            self.add_version(&mut plugins, fa, tr.version());
        }
        Ok(())
    }

    fn save(&self) -> Result<()> {
        if self.unread {
            eyre::bail!(
                "not writing {}: it is a symlink to a file of another name, which mise did not read",
                display_path(&self.path)
            );
        }
        if self.unmapped_templates {
            eyre::bail!(
                "not writing {}: it has a template tag or a value that spans lines, so mise cannot tell which line a template renders to and would replace every template with its value. Edit the file by hand",
                display_path(&self.path)
            );
        }
        let s = self.dump()?;
        file::write_atomic(&self.path, s)
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
            if let Some(raw) = &tv.raw {
                s.push_str(&format!("{raw}{}", tv.post));
                continue;
            }
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
            post: "\n".into(),
            raw: None,
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
            unread: self.unread,
            unmapped_templates: self.unmapped_templates,
        }
    }
}

/// Whether a `.tool-versions` body needs trust before it is parsed.
///
/// Templates can run commands, and tool options written inline in the tool name
/// (`tool[opt=value]`) can run code (`postinstall`) or redirect requests
/// (`api_url`), so either one makes the file unsafe to load from an untrusted
/// directory. So does a `path:` version, which puts a directory of the
/// repository on PATH without an install step. Plain `tool version` lines stay
/// trust-free.
fn body_requires_trust(body: &str) -> bool {
    contains_template_syntax(body)
        || body.lines().any(|line| {
            let line = line.split_once('#').map_or(line, |(line, _)| line);
            let mut tokens = line.split_whitespace();
            tokens.next().is_some_and(super::tool_key_requires_trust)
                || tokens.any(super::version_requires_trust)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_tool_options_require_trust() {
        assert!(!body_requires_trust(""));
        assert!(!body_requires_trust("node 20.0.0\npython 3.12 3.11\n"));
        assert!(!body_requires_trust("# a [comment]\nnode 20.0.0 # [x]\n"));
        assert!(body_requires_trust("{{ exec(command='id') }}node 20\n"));
        assert!(body_requires_trust(
            "github:sharkdp/fd[api_url=http://attacker.example/api/v3] latest\n"
        ));
        assert!(body_requires_trust("node 20\n  node[postinstall=id] 20\n"));
        assert!(body_requires_trust(
            "spm:https://evil.example/o/r.git latest\n"
        ));
        assert!(!body_requires_trust(
            "spm:https://github.com/o/r.git latest\n"
        ));
        assert!(body_requires_trust("node path:./local\n"));
        assert!(body_requires_trust("node 20 path:./local # x\n"));
        assert!(!body_requires_trust("node 20 # path:./local\n"));
        // `rev:` and `ref:` name any commit the forge serves, such as a pull request's head
        assert!(body_requires_trust(
            "cargo:https://github.com/o/r rev:refs/pull/1/head\n"
        ));
        assert!(body_requires_trust("cargo:o/r rev-refs/pull/1/head\n"));
        assert!(body_requires_trust("node ref:main\n"));
        assert!(!body_requires_trust("cargo:o/r tag:v1.0.0\n"));
        assert!(!body_requires_trust("cargo:o/r branch:main\n"));
        assert!(!body_requires_trust("node 20 refresh-1\n"));
    }

    /// The whole file is rendered as a template when it is loaded, so a version
    /// from a version list must not be written into it as one.
    #[test]
    fn replace_versions_rejects_template_syntax() {
        let tv = ToolVersions::init(Path::new("/tmp/.tool-versions"));
        let ba: Arc<BackendArg> = Arc::new("node".into());
        let tr = ToolRequest::new_version_for_test(
            ba.clone(),
            "20-{{env.GITHUB_TOKEN}}",
            ToolSource::Unknown,
        );
        let err = tv.replace_versions(&ba, vec![tr]).unwrap_err();
        assert!(err.to_string().contains("template syntax"), "{err}");
        assert!(!tv.dump().unwrap().contains("{{"));
    }

    /// Saving writes the comments and the tools a command did not change as the
    /// file has them, so their templates are kept instead of the values they
    /// rendered to.
    #[test]
    fn saving_keeps_the_templates_of_other_lines() {
        let parsed = |raw: &str, rendered: &str| {
            let tv = ToolVersions::init(Path::new("/tmp/.tool-versions"));
            *tv.plugins.lock().unwrap() =
                ToolVersions::parse_lines(rendered.lines().zip(raw.lines()));
            tv
        };
        let node: Arc<BackendArg> = Arc::new("node".into());
        let node_20 =
            || ToolRequest::new_version_for_test(node.clone(), "20.0.0", ToolSource::Unknown);

        let tv = parsed(
            "http:t[url=https://h/{{ env.TOKEN }}/t.tgz] 1.0.0 # {{ env.TOKEN }}\n# {{ env.TOKEN }}\nnode 18.0.0\n",
            "http:t[url=https://h/s3cr3t/t.tgz] 1.0.0 # s3cr3t\n# s3cr3t\nnode 18.0.0\n",
        );
        tv.replace_versions(&node, vec![node_20()]).unwrap();
        assert_eq!(
            tv.dump().unwrap(),
            "http:t[url=https://h/{{ env.TOKEN }}/t.tgz] 1.0.0 # {{ env.TOKEN }}\n# {{ env.TOKEN }}\nnodejs 20.0.0\n"
        );

        // the versions of a changed line are its new ones
        let tv = parsed("node {{ env.NV }}\n", "node 18.0.0\n");
        tv.replace_versions(&node, vec![node_20()]).unwrap();
        assert_eq!(tv.dump().unwrap(), "nodejs 20.0.0\n");

        // a changed line whose name is a template would lose it
        let tv = parsed("{{ env.TOOL }} 18.0.0\n", "node 18.0.0\n");
        let err = tv.replace_versions(&node, vec![node_20()]).unwrap_err();
        assert!(err.to_string().contains("Edit the file by hand"), "{err}");
        assert_eq!(tv.dump().unwrap(), "{{ env.TOOL }} 18.0.0\n");

        // a file whose lines cannot be told apart once rendered is not written
        let tv = ToolVersions {
            unmapped_templates: true,
            ..ToolVersions::init(Path::new("/tmp/.tool-versions"))
        };
        let err = tv.save().unwrap_err();
        assert!(err.to_string().contains("Edit the file by hand"), "{err}");
    }

    /// `dump` relies on `post` to terminate each line, and `trim_end` only ever
    /// fixes up the last one. A tool that is not already present in the file is
    /// created by `get_or_create_plugin`, so its `post` has to carry the newline
    /// too, otherwise two freshly added tools share a physical line and
    /// `parse_plugins` reads them back as a single tool.
    #[test]
    fn newly_added_tools_each_get_their_own_line() {
        let tv = ToolVersions::init(Path::new("/tmp/.tool-versions"));
        {
            let mut plugins = tv.plugins.lock().unwrap();
            for (tool, version) in [("node", "20.0.0"), ("java", "21.0.0")] {
                let ba: BackendArg = tool.into();
                get_or_create_plugin(&mut plugins, &ba)
                    .versions
                    .push(version.to_string());
            }
        }

        let dumped = tv.dump().unwrap();
        assert_eq!(dumped.lines().count(), 2, "dumped: {dumped:?}");

        let reparsed = ToolVersions::parse_plugins(&dumped);
        let round_tripped: Vec<(String, Vec<String>)> = reparsed
            .values()
            .map(|p| (p.orig_name.clone(), p.versions.clone()))
            .collect();
        assert_eq!(
            round_tripped,
            vec![
                ("nodejs".to_string(), vec!["20.0.0".to_string()]),
                ("java".to_string(), vec!["21.0.0".to_string()]),
            ],
            "dumped: {dumped:?}"
        );
    }
}
