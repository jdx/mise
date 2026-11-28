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
    pre: Mutex<String>,
    plugins: Mutex<IndexMap<BackendArg, ToolVersionPlugin>>,
    tools: Mutex<ToolRequestSet>,
    /// Skipped because it links to a file of another name; never written back.
    unread: bool,
    /// Rendered from templates whose lines cannot be told apart in the result
    /// (`{% ... %}` or `{# ... #}`, a `{{ ... }}` or a value that spans lines, or a
    /// `{{- ... }}` or `{{ ... -}}` that removes a line break); never written back.
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
        // Comments and the lines of tools a command does not change are written back from
        // the file as written, so saving keeps their templates.
        let mapped = lines_map(raw, &s);
        cf.unmapped_templates = !mapped;
        let raw = if mapped { raw } else { s.as_str() };
        let (pre, plugins) = Self::parse_lines(file_lines(&s).zip(file_lines(raw)));
        cf.pre = Mutex::new(pre);
        cf.plugins = Mutex::new(plugins);
        cf.populate_toolset()?;
        trace!("{cf}");
        Ok(cf)
    }

    #[cfg(test)]
    fn parse_plugins(input: &str) -> IndexMap<BackendArg, ToolVersionPlugin> {
        Self::parse_lines(input.lines().zip(input.lines())).1
    }

    /// The lines before the first tool, and the tools, from pairs of a rendered line
    /// and the line as written. A line that is not a tool is kept as written, after the
    /// tool before it, when it is a comment or a template: a template that renders to a
    /// comment or to nothing here can declare a tool in another environment. Blank lines
    /// without a template are dropped.
    fn parse_lines<'a>(
        lines: impl Iterator<Item = (&'a str, &'a str)>,
    ) -> (String, IndexMap<BackendArg, ToolVersionPlugin>) {
        let mut pre = String::new();
        let mut plugins: IndexMap<BackendArg, ToolVersionPlugin> = IndexMap::new();
        for (line, raw_line) in lines {
            let comment =
                raw_line.trim_start().starts_with('#') || line.trim_start().starts_with('#');
            if comment || (line.trim().is_empty() && raw_line != line) {
                let kept = match plugins.values_mut().last() {
                    Some(prev) => &mut prev.post,
                    None => &mut pre,
                };
                kept.push_str(raw_line);
                kept.push('\n');
                continue;
            }
            let (raw_tool, post) = match comment_start(raw_line) {
                Some(i) => (&raw_line[..i], &raw_line[i + 1..]),
                None => (raw_line, ""),
            };
            let raw = (raw_line != line).then(|| raw_tool.trim_end().to_string());
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
        (pre, plugins)
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

    /// The template lines kept after the tool stay in the file, after the tool
    /// before it: they are not part of the tool, and can declare another tool in
    /// another environment. The comments after the tool go with it.
    fn remove_tool(&self, fa: &BackendArg) -> Result<()> {
        let mut plugins = self.plugins.lock().unwrap();
        let Some(index) = plugins.get_index_of(fa) else {
            return Ok(());
        };
        let (_, removed) = plugins.shift_remove_index(index).unwrap();
        // The first line of `post` ends the tool's own line; a later line that
        // holds `{{` was kept for its template.
        let templates: String = removed
            .post
            .split_inclusive('\n')
            .skip(1)
            .filter(|line| line.contains("{{"))
            .collect();
        match index.checked_sub(1) {
            Some(prev) => plugins[prev].post.push_str(&templates),
            None => self.pre.lock().unwrap().push_str(&templates),
        }
        Ok(())
    }

    fn replace_versions(&self, fa: &BackendArg, versions: Vec<ToolRequest>) -> eyre::Result<()> {
        ensure_versions_are_not_templates(&self.path, fa, &versions)?;
        let mut plugins = self.plugins.lock().unwrap();
        let plugin = get_or_create_plugin(&mut plugins, fa);
        let refuse = |what: &str| {
            eyre::eyre!(
                "not writing {}: {what} of {fa} is written with a template, which mise would replace with the value it renders to. Edit the file by hand",
                display_path(&self.path)
            )
        };
        // A version the line keeps is written as the line has it. That needs each word
        // of the line to render to one version, which holds when the line has as many
        // words as versions, at most one of them is a template, and every other word is
        // its version: with two templates, one could render to nothing and the other to
        // two versions, and a word that is not its version was joined with a template
        // (`2 {{- ... }}`). Otherwise a kept version
        // could come from a template, and the edit is refused rather than writing what
        // it rendered to.
        let mut as_written: Vec<(String, String)> = vec![];
        if let Some(raw) = &plugin.raw {
            let words = words_as_written(raw);
            // A template in the name would be replaced by its value.
            if words.first() != Some(&plugin.orig_name.as_str()) {
                return Err(refuse("the name"));
            }
            let templates = words[1..].iter().filter(|w| w.contains("{{")).count();
            let literals_are_versions = words[1..]
                .iter()
                .zip(&plugin.versions)
                .all(|(word, version)| word.contains("{{") || word == version);
            if words.len() - 1 == plugin.versions.len() && templates <= 1 && literals_are_versions {
                as_written = plugin
                    .versions
                    .iter()
                    .cloned()
                    .zip(words[1..].iter().map(|w| w.to_string()))
                    .collect();
            } else if versions
                .iter()
                .any(|tr| plugin.versions.contains(&tr.version()))
            {
                return Err(refuse("a version"));
            }
        }
        let mut words = vec![plugin.orig_name.clone()];
        let mut templated = false;
        plugin.versions.clear();
        for tr in versions {
            if !tr.options().is_empty() {
                warn!("tool options are not supported in .tool-versions files");
            }
            let version = tr.version();
            let word = match as_written.iter().position(|(v, _)| *v == version) {
                Some(i) => as_written.remove(i).1,
                None => version.clone(),
            };
            templated |= word != version;
            words.push(word);
            plugin.versions.push(version);
        }
        plugin.raw = templated.then(|| words.join(" "));
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
                "not writing {}: it has a template tag, a template or a value that spans lines, or a template that removes a line break, so mise cannot tell which line a template renders to and would replace every template with its value. Edit the file by hand",
                display_path(&self.path)
            );
        }
        let s = self.dump()?;
        super::symlink_check_before_write(&self.path)?;
        file::write_atomic(&self.path, s)
    }

    fn dump(&self) -> eyre::Result<String> {
        let mut s = self.pre.lock().unwrap().clone();

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

/// Whether each line of a file renders to the line of the same number. That fails
/// when a tag (`{% ... %}`, `{# ... #}`), a value or a `{{ ... }}` spans lines, or
/// when a `{{- ... }}` that starts a line or a `{{ ... -}}` that ends one removes a
/// line break; a value that adds a line break next to one of those can leave the
/// count unchanged. A `{{- ... }}` inside a line removes only spaces. The count
/// includes a last line with no line break, which can render to nothing, and a
/// `}}` in a quoted string does not close a `{{`.
fn lines_map(raw: &str, rendered: &str) -> bool {
    raw.split('\n').count() == rendered.split('\n').count()
        && !raw.contains("{%")
        && !raw.contains("{#")
        && raw.lines().all(|line| {
            // a `-}}` with no `{{` before it on the line closes nothing
            let closes = line.trim_end().ends_with("-}}") && line.contains("{{");
            !line.trim_start().starts_with("{{-") && !closes
        })
        && raw.lines().all(expressions_close)
}

/// The lines of a file as `lines_map` counts them: split at each `\n`, with a
/// last line after the final line break, which is empty unless the file does not
/// end with one. Pairing the rendered and the raw lines this way keeps a last
/// template with no line break that renders to nothing.
fn file_lines(s: &str) -> impl Iterator<Item = &str> {
    s.split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
}

/// Where the comment of a line as written starts: its first `#` outside a
/// `{{ ... }}` expression, so `node {{ "20# note" }}` has none.
fn comment_start(line: &str) -> Option<usize> {
    let mut i = 0;
    while i < line.len() {
        let rest = &line[i..];
        if let Some(inner) = rest.strip_prefix("{{") {
            match expression_end(inner) {
                Some(end) => i += 2 + end,
                // an expression that does not close leaves the file unmapped, so
                // this line is the rendered one: its first `#` starts the comment
                None => return rest.find('#').map(|j| i + j),
            }
        } else if rest.starts_with('#') {
            return Some(i);
        } else {
            i += rest.chars().next().map_or(1, char::len_utf8);
        }
    }
    None
}

/// The length of a `{{ ... }}` expression after its `{{`, through its `}}`,
/// skipping a `}}` in a quoted string; None when it does not close.
fn expression_end(rest: &str) -> Option<usize> {
    let mut quote = None;
    for (i, c) in rest.char_indices() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if matches!(c, '"' | '\'' | '`') => quote = Some(c),
            None if rest[i..].starts_with("}}") => return Some(i + 2),
            None => {}
        }
    }
    None
}

/// Whether every `{{` on a line has its `}}` on the same line, outside quotes.
fn expressions_close(line: &str) -> bool {
    let mut rest = line;
    while let Some(open) = rest.find("{{") {
        rest = &rest[open + 2..];
        match expression_end(rest) {
            Some(end) => rest = &rest[end..],
            None => return false,
        }
    }
    true
}

/// The whitespace-separated words of a line as written, where a `{{ ... }}`
/// expression stays whole within its word.
fn words_as_written(line: &str) -> Vec<&str> {
    let mut words = vec![];
    let mut start = None;
    let mut in_expression = false;
    for (i, c) in line.char_indices() {
        if line[i..].starts_with("{{") {
            in_expression = true;
        } else if in_expression && line[..i].ends_with("}}") {
            in_expression = false;
        }
        if c.is_whitespace() && !in_expression {
            if let Some(s) = start.take() {
                words.push(&line[s..i]);
            }
        } else if start.is_none() {
            start = Some(i);
        }
    }
    if let Some(s) = start {
        words.push(&line[s..]);
    }
    words
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
            pre: Mutex::new(self.pre.lock().unwrap().clone()),
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
        assert!(body_requires_trust(
            "core:spm:https://evil.example/o/r.git latest\n"
        ));
        assert!(body_requires_trust(
            "core:core:spm:https://evil.example/o/r.git latest\n"
        ));
        assert!(!body_requires_trust(
            "spm:https://github.com/o/r.git latest\n"
        ));
        assert!(!body_requires_trust(
            "core:spm:https://github.com/o/r.git latest\n"
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

    #[test]
    fn lines_map_only_when_no_template_spans_lines() {
        for (raw, rendered, maps) in [
            (
                "node {{ env.NV }}\npython 3.12\n",
                "node 20\npython 3.12\n",
                true,
            ),
            ("node {{ env.NV }}\n", "node 20\nruby 3\n", false),
            ("{% if x %}node 20{% endif %}\n", "node 20\n", false),
            ("{# note #}\nnode 20\n", "\nnode 20\n", false),
            (
                "node {{\n env.NV }}\npython {{ env.PV }}\n",
                "node 20\npython 3.12\nruby 3\n",
                false,
            ),
            // whitespace control that removes a line break, next to a value that adds one
            (
                "node 20\n  {{- \" 18\" }}\npython {{ env.PV }}\n",
                "node 20 18\npython 3.12\nruby 3\n",
                false,
            ),
            (
                "node {{ \"20\" -}}  \n 18\npython {{ env.PV }}\n",
                "node 20 18\npython 3.12\nruby 3\n",
                false,
            ),
            // inside a line it removes spaces only
            ("node 2 {{- env.NV }}\n", "node 20.0.0\n", true),
            // written text that only looks like the end of a template
            ("node 20 # see -}}\n", "node 20 # see -}}\n", true),
            // a last line with no line break that renders to nothing, next to a value
            // that adds a line
            (
                "node {{ env.NV }}\n{{ env.EMPTY }}",
                "node 20\npython 3\n",
                false,
            ),
            ("node {{ env.NV }}", "node 20", true),
            // a `}}` in a string closes nothing
            (
                "node {{ \"}}\" |\n upper }}\npython {{ env.PV }}\n",
                "node }}\npython 3.12\nruby 3\n",
                false,
            ),
            ("node {{ \"}}\" }}\n", "node }}\n", true),
            ("node {{ '}}' ~ `x` }}\n", "node }}x\n", true),
        ] {
            assert_eq!(lines_map(raw, rendered), maps, "{raw:?}");
        }
    }

    /// A file parsed from its text as written and the text it rendered to.
    fn parsed(raw: &str, rendered: &str) -> ToolVersions {
        let (pre, plugins) = ToolVersions::parse_lines(file_lines(rendered).zip(file_lines(raw)));
        ToolVersions {
            pre: Mutex::new(pre),
            plugins: Mutex::new(plugins),
            ..ToolVersions::init(Path::new("/tmp/.tool-versions"))
        }
    }

    /// The versions a command keeps on a line are written as the line has them,
    /// so removing one version keeps the template of another.
    #[test]
    fn kept_versions_keep_their_templates() {
        let node: Arc<BackendArg> = Arc::new("node".into());
        let node_version =
            |v: &str| ToolRequest::new_version_for_test(node.clone(), v, ToolSource::Unknown);
        assert_eq!(
            words_as_written("node  {{ env.NV }} 20-{{ x }}{{ y }} 1"),
            ["node", "{{ env.NV }}", "20-{{ x }}{{ y }}", "1"]
        );

        let tv = parsed(
            "node {{ env.NV }} 18.0.0 # {{ env.NOTE }}\n",
            "node 20.0.0 18.0.0 # note\n",
        );
        tv.replace_versions(&node, vec![node_version("20.0.0")])
            .unwrap();
        assert_eq!(tv.dump().unwrap(), "node {{ env.NV }} # {{ env.NOTE }}\n");

        // a version written as it is stays as it is
        let tv = parsed("node {{ env.NV }} 18.0.0\n", "node 20.0.0 18.0.0\n");
        tv.replace_versions(&node, vec![node_version("18.0.0")])
            .unwrap();
        assert_eq!(tv.dump().unwrap(), "nodejs 18.0.0\n");

        // a template that renders to several versions cannot keep only some of them
        let tv = parsed("node {{ env.NV }}\n", "node 20.0.0 18.0.0\n");
        let err = tv
            .replace_versions(&node, vec![node_version("20.0.0")])
            .unwrap_err();
        assert!(err.to_string().contains("Edit the file by hand"), "{err}");
        assert!(!err.to_string().contains("20.0.0"), "{err}");
        assert_eq!(tv.dump().unwrap(), "node {{ env.NV }}\n");

        // with two templates, either could have rendered to both versions
        let tv = parsed("node {{ env.A }} {{ env.B }}\n", "node 20.0.0 18.0.0\n");
        let err = tv
            .replace_versions(&node, vec![node_version("18.0.0")])
            .unwrap_err();
        assert!(err.to_string().contains("Edit the file by hand"), "{err}");
        assert_eq!(tv.dump().unwrap(), "node {{ env.A }} {{ env.B }}\n");

        // a word as written that is not its version was joined with a template
        let tv = parsed("node 2 {{- env.NV }}\n", "node 20.0.0 18.0.0\n");
        let err = tv
            .replace_versions(&node, vec![node_version("20.0.0")])
            .unwrap_err();
        assert!(err.to_string().contains("Edit the file by hand"), "{err}");
        assert_eq!(tv.dump().unwrap(), "node 2 {{- env.NV }}\n");

        // a new version replaces the templates
        let tv = parsed("node {{ env.NV }}\n", "node 20.0.0 18.0.0\n");
        tv.replace_versions(&node, vec![node_version("22.0.0")])
            .unwrap();
        assert_eq!(tv.dump().unwrap(), "nodejs 22.0.0\n");
    }

    /// A template line can render to a comment or to nothing in one environment
    /// and to a tool in another, so saving writes it back as it is.
    #[test]
    fn lines_that_declare_no_tool_keep_their_templates() {
        let node: Arc<BackendArg> = Arc::new("node".into());
        let tv = parsed(
            "{{ env.HEADER }}\n{{ env.FIRST }}\nnode 18.0.0\n{{ env.EXTRA }}\npython 3.12\n",
            "# header\n\nnode 18.0.0\n\npython 3.12\n",
        );
        tv.replace_versions(
            &node,
            vec![ToolRequest::new_version_for_test(
                node.clone(),
                "20.0.0",
                ToolSource::Unknown,
            )],
        )
        .unwrap();
        assert_eq!(
            tv.dump().unwrap(),
            "{{ env.HEADER }}\n{{ env.FIRST }}\nnodejs 20.0.0\n{{ env.EXTRA }}\npython 3.12\n"
        );

        // removing the tool before such a line keeps the line, and drops its comment
        let tv = parsed(
            "node 18.0.0 # lts\n# pinned\n{{ env.EXTRA }}\npython 3.12\n",
            "node 18.0.0 # lts\n# pinned\n\npython 3.12\n",
        );
        tv.remove_tool(&node).unwrap();
        assert_eq!(tv.dump().unwrap(), "{{ env.EXTRA }}\npython 3.12\n");
        let tv = parsed(
            "python 3.12\nnode 18.0.0\n{{ env.EXTRA }}\n",
            "python 3.12\nnode 18.0.0\n\n",
        );
        tv.remove_tool(&node).unwrap();
        assert_eq!(tv.dump().unwrap(), "python 3.12\n{{ env.EXTRA }}\n");
    }

    /// A last line with no line break pairs with what it rendered to, even when
    /// that is nothing.
    #[test]
    fn a_last_template_without_a_line_break_is_kept() {
        let node: Arc<BackendArg> = Arc::new("node".into());
        let (raw, rendered) = ("node 18.0.0\n{{ env.EXTRA }}", "node 18.0.0\n");
        assert!(lines_map(raw, rendered));
        let tv = parsed(raw, rendered);
        tv.replace_versions(
            &node,
            vec![ToolRequest::new_version_for_test(
                node.clone(),
                "20.0.0",
                ToolSource::Unknown,
            )],
        )
        .unwrap();
        assert_eq!(tv.dump().unwrap(), "nodejs 20.0.0\n{{ env.EXTRA }}\n");
    }

    /// A `#` inside a `{{ ... }}` expression does not start the comment of the
    /// line as written, so saving another tool leaves the expression as it is.
    #[test]
    fn a_hash_inside_a_template_is_not_a_comment() {
        let python: Arc<BackendArg> = Arc::new("python".into());
        let tv = parsed(
            "node {{ \"20# note\" }}\nruby {{ \"3\" }} # pinned\npython 3.11\n",
            "node 20# note\nruby 3 # pinned\npython 3.11\n",
        );
        tv.replace_versions(
            &python,
            vec![ToolRequest::new_version_for_test(
                python.clone(),
                "3.12",
                ToolSource::Unknown,
            )],
        )
        .unwrap();
        assert_eq!(
            tv.dump().unwrap(),
            "node {{ \"20# note\" }}\nruby {{ \"3\" }} # pinned\npython 3.12\n"
        );
        assert_eq!(comment_start("node {{ \"}}#\" }} # c"), Some(17));
        assert_eq!(comment_start("node 20 # {{ open"), Some(8));
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
