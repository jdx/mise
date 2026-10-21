//! Tasks and task templates: where a config defines them and where it refers to them.
//!
//! A task is `[tasks.<name>]`. Other tasks name it in `depends`, `depends_post`, `wait_for` and
//! `run = [{ task = "<name>" }]`; `extends = "<name>"` names a template, `[task_templates.<name>]`,
//! instead. The server finds both from the parsed config, so nothing is rendered or run. Besides
//! the open document it reads the configs in the directories above it and the TOML files their
//! `[task_config] includes` name, the way [`crate::includes`] does.

use std::collections::HashSet;
use std::ops::Range;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::complete::unquoted;
use crate::doc::{self, Parsed};
use crate::includes;
use crate::keypath;
use crate::text::LineIndex;

/// More files than this are not read for tasks.
const MAX_FILES: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Namespace {
    Task,
    Template,
}

impl Namespace {
    /// The top-level table the definitions are keys of.
    fn root(self) -> &'static str {
        match self {
            Namespace::Task => "tasks",
            Namespace::Template => "task_templates",
        }
    }

    pub(crate) fn noun(self) -> &'static str {
        match self {
            Namespace::Task => "task",
            Namespace::Template => "task template",
        }
    }
}

/// Where a definition is written.
#[derive(Debug, Clone)]
pub(crate) enum Origin {
    /// The open document, by byte offsets into its text.
    Open(Range<usize>),
    /// A file on disk, by `(line, character)` positions.
    File {
        path: PathBuf,
        start: (usize, usize),
        end: (usize, usize),
    },
}

#[derive(Debug, Clone)]
pub(crate) struct Definition {
    pub name: String,
    pub aliases: Vec<String>,
    pub description: Option<String>,
    pub run: Option<String>,
    pub origin: Origin,
}

/// What the open document and the files around it define.
#[derive(Default)]
pub(crate) struct Index {
    pub tasks: Vec<Definition>,
    pub templates: Vec<Definition>,
    /// The names that configs and include files other than the open document refer to.
    disk_references: HashSet<(Namespace, String)>,
}

/// A name offered where a task or template is expected.
#[derive(Default, Clone)]
pub(crate) struct Candidate {
    pub name: String,
    pub detail: Option<String>,
    pub documentation: Option<String>,
}

#[derive(Default)]
pub(crate) struct Candidates {
    pub tasks: Vec<Candidate>,
    pub templates: Vec<Candidate>,
}

impl Index {
    /// `open` is the text of the open document, `file` its path. The definitions of the document
    /// come first, so they win over a file that defines the same name.
    pub(crate) fn build(open: &str, file: Option<&Path>) -> Self {
        let mut index = Index::default();
        let mut seen: HashSet<PathBuf> = HashSet::new();
        if let Ok(parsed) = doc::parse(open) {
            index.add(&parsed, Layout::Config, &|range| Origin::Open(range));
            // The open document's own includes are read from its text, not from disk.
            if let Some(file) = file {
                seen.insert(file.to_path_buf());
                index.add_includes(file, &parsed, &mut seen);
            }
        }
        let Some(file) = file else {
            return index;
        };
        seen.insert(file.to_path_buf());
        for dir in file.ancestors().skip(1) {
            for config in includes::config_files(dir) {
                if seen.len() >= MAX_FILES || !seen.insert(config.clone()) {
                    continue;
                }
                let Some(text) = includes::read_capped(&config) else {
                    continue;
                };
                let Ok(parsed) = doc::parse(&text) else {
                    continue;
                };
                index.add(&parsed, Layout::Config, &origin_in(&config, &text));
                index.note_references(&text, &parsed);
                index.add_includes(&config, &parsed, &mut seen);
            }
        }
        index
    }

    /// The tasks of the TOML files `config` lists in `[task_config] includes`.
    fn add_includes(&mut self, config: &Path, parsed: &Parsed, seen: &mut HashSet<PathBuf>) {
        let listed: Vec<String> = parsed.json["task_config"]["includes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect();
        for path in includes::task_include_files(config, &listed) {
            if seen.len() >= MAX_FILES || !seen.insert(path.clone()) {
                continue;
            }
            let Some(text) = includes::read_capped(&path) else {
                continue;
            };
            if let Ok(parsed) = doc::parse(&text) {
                self.add(&parsed, Layout::TaskFile, &origin_in(&path, &text));
                self.note_references(&text, &parsed);
            }
        }
    }

    /// Remembers what a file other than the open one refers to.
    fn note_references(&mut self, text: &str, parsed: &Parsed) {
        for reference in references(text, parsed) {
            self.disk_references
                .insert((reference.namespace, reference.name));
        }
    }

    /// Whether a config or include file besides the open document names the task.
    pub(crate) fn referenced_elsewhere(&self, namespace: Namespace, name: &str) -> bool {
        self.disk_references
            .contains(&(namespace, name.to_string()))
    }

    fn add(&mut self, parsed: &Parsed, layout: Layout, origin: &dyn Fn(Range<usize>) -> Origin) {
        let namespaces: &[Namespace] = match layout {
            Layout::Config => &[Namespace::Task, Namespace::Template],
            Layout::TaskFile => &[Namespace::Task],
        };
        for &namespace in namespaces {
            for entry in &parsed.entries {
                let (name, body) = match layout {
                    Layout::Config => match entry.path.as_slice() {
                        [root, name] if root == namespace.root() => (name, &entry.path),
                        _ => continue,
                    },
                    Layout::TaskFile => match entry.path.as_slice() {
                        [name] => (name, &entry.path),
                        _ => continue,
                    },
                };
                let Some(key) = entry.key.clone() else {
                    continue;
                };
                let Some(value) = value_at(&parsed.json, body) else {
                    continue;
                };
                let definition = Definition {
                    name: name.clone(),
                    aliases: strings(value.get("alias")),
                    description: value
                        .get("description")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    run: command(value),
                    origin: origin(key),
                };
                match namespace {
                    Namespace::Task => self.tasks.push(definition),
                    Namespace::Template => self.templates.push(definition),
                }
            }
        }
    }

    fn definitions(&self, namespace: Namespace) -> &[Definition] {
        match namespace {
            Namespace::Task => &self.tasks,
            Namespace::Template => &self.templates,
        }
    }

    /// The definition a task is named by, as its name or an alias.
    pub(crate) fn find(&self, namespace: Namespace, name: &str) -> Option<&Definition> {
        let definitions = self.definitions(namespace);
        definitions.iter().find(|d| d.name == name).or_else(|| {
            definitions
                .iter()
                .find(|d| d.aliases.iter().any(|a| a == name))
        })
    }

    pub(crate) fn candidates(&self) -> Candidates {
        let list = |definitions: &[Definition]| {
            let mut seen = HashSet::new();
            definitions
                .iter()
                .filter(|d| seen.insert(d.name.as_str()))
                .map(|d| Candidate {
                    name: d.name.clone(),
                    detail: d
                        .description
                        .as_deref()
                        .and_then(|description| description.lines().next())
                        .map(str::to_string),
                    documentation: Some(describe(d)),
                })
                .collect()
        };
        Candidates {
            tasks: list(&self.tasks),
            templates: list(&self.templates),
        }
    }
}

#[derive(Clone, Copy)]
enum Layout {
    /// A `mise.toml`: tasks are under `[tasks]`.
    Config,
    /// A file `[task_config] includes` lists: each top-level table is a task.
    TaskFile,
}

fn origin_in<'a>(path: &'a Path, text: &'a str) -> impl Fn(Range<usize>) -> Origin + 'a {
    let index = LineIndex::new(text);
    move |range| Origin::File {
        path: path.to_path_buf(),
        start: index.position(range.start),
        end: index.position(range.end),
    }
}

/// The markdown that says what a definition is: its description and what it runs.
pub(crate) fn describe(definition: &Definition) -> String {
    let mut markdown = String::new();
    if let Some(description) = &definition.description {
        markdown.push_str(description);
    }
    if let Some(run) = &definition.run {
        if !markdown.is_empty() {
            markdown.push_str("\n\n");
        }
        markdown.push_str(&format!("```sh\n{run}\n```"));
    }
    if let Origin::File { path, .. } = &definition.origin
        && let Some(name) = path.file_name()
    {
        if !markdown.is_empty() {
            markdown.push_str("\n\n");
        }
        markdown.push_str(&format!("Defined in `{}`", name.to_string_lossy()));
    }
    markdown
}

/// What a task runs, as text: a script, or the tasks it runs one after another.
fn command(task: &Value) -> Option<String> {
    let run = match task {
        Value::String(script) => return Some(script.clone()),
        _ => task.get("run")?,
    };
    let line = |entry: &Value| match entry {
        Value::String(script) => Some(script.clone()),
        Value::Object(table) => {
            if let Some(task) = table.get("task").and_then(Value::as_str) {
                let args = strings(table.get("args")).join(" ");
                Some(format!("mise run {task} {args}").trim_end().to_string())
            } else {
                let tasks = strings(table.get("tasks"));
                (!tasks.is_empty()).then(|| format!("mise run {}", tasks.join(" ::: ")))
            }
        }
        _ => None,
    };
    let lines: Vec<String> = match run {
        Value::Array(entries) => entries.iter().filter_map(line).collect(),
        entry => line(entry).into_iter().collect(),
    };
    (!lines.is_empty()).then(|| lines.join("\n"))
}

/// A string or a list of strings, as a list.
fn strings(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::String(s)) => vec![s.clone()],
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect(),
        _ => vec![],
    }
}

pub(crate) fn value_at<'a>(json: &'a Value, path: &[String]) -> Option<&'a Value> {
    path.iter().try_fold(json, |value, segment| match value {
        Value::Object(map) => map.get(segment),
        Value::Array(items) => items.get(segment.parse::<usize>().ok()?),
        _ => None,
    })
}

fn is_index(segment: &str) -> bool {
    segment.parse::<usize>().is_ok()
}

/// What the string at `path` names, if it names a task or a template. Array elements are
/// counted by index; the completion lexer writes `0` for all of them.
pub(crate) fn reference_kind(path: &[String]) -> Option<Namespace> {
    let path: Vec<&str> = path.iter().map(String::as_str).collect();
    let [root, _, field, rest @ ..] = path.as_slice() else {
        return None;
    };
    if *root != "tasks" && *root != "task_templates" {
        return None;
    }
    match (*field, rest) {
        ("extends", []) if *root == "tasks" => Some(Namespace::Template),
        ("depends" | "depends_post" | "wait_for", rest) => match rest {
            [] | ["task"] => Some(Namespace::Task),
            [i] | [i, "task"] if is_index(i) => Some(Namespace::Task),
            // `["name", "arg"]`: only the first element is the name
            [i, "0"] if is_index(i) => Some(Namespace::Task),
            _ => None,
        },
        ("run" | "run_windows", rest) => match rest {
            ["task"] => Some(Namespace::Task),
            [i, "task"] if is_index(i) => Some(Namespace::Task),
            ["tasks", j] if is_index(j) => Some(Namespace::Task),
            [i, "tasks", j] if is_index(i) && is_index(j) => Some(Namespace::Task),
            _ => None,
        },
        _ => None,
    }
}

/// Whether the path is an argument in `depends = [["name", "arg"]]`, as the completion lexer
/// sees it: it writes `0` for every element, so the argument looks like the name. Nothing is
/// offered there.
pub(crate) fn is_dependency_argument_slot(path: &[String]) -> bool {
    matches!(path, [_, _, field, i, last]
        if matches!(field.as_str(), "depends" | "depends_post" | "wait_for")
            && is_index(i)
            && last == "0")
}

/// A place in the document that names a task or a template.
#[derive(Debug, Clone)]
pub(crate) struct Reference {
    pub namespace: Namespace,
    pub name: String,
    /// The name, without the arguments that may follow it in the string.
    pub range: Range<usize>,
}

/// The task a reference string names and where the name starts in the string. A dependency may
/// start with `KEY=value` words that set its environment, as in `"PROFILE=release build"`; the
/// name is the first word after them, or the only word. `None` when an environment value is
/// quoted, which this does not follow.
fn name_of(value: &str) -> Option<(&str, usize)> {
    let mut words = value
        .split_whitespace()
        .map(|word| (word, word.as_ptr() as usize - value.as_ptr() as usize))
        .peekable();
    while let Some((word, at)) = words.next() {
        let is_env = word.split_once('=').is_some_and(|(key, _)| {
            !key.is_empty() && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        });
        if is_env && words.peek().is_some() {
            if word.contains(['"', '\'', '\\']) {
                return None;
            }
            continue;
        }
        return Some((word, at));
    }
    None
}

pub(crate) fn references(text: &str, parsed: &Parsed) -> Vec<Reference> {
    let mut out = vec![];
    for entry in &parsed.entries {
        let (Some(namespace), Some(span)) = (reference_kind(&entry.path), entry.value.clone())
        else {
            continue;
        };
        let Some(Value::String(value)) = value_at(&parsed.json, &entry.path) else {
            continue;
        };
        let Some((name, at)) = name_of(value) else {
            continue;
        };
        // Only a plain one-line string has a range that is the name itself; an escape or a
        // template makes the name something other than what the text spells.
        let Some(raw) = text.get(span.clone()) else {
            continue;
        };
        let content = if raw.starts_with("\"\"\"") || raw.starts_with("'''") {
            None
        } else {
            raw.strip_prefix(['"', '\''])
        };
        let Some(content) = content else {
            continue;
        };
        if name.contains("{{") || !content.starts_with(&value[..at + name.len()]) {
            continue;
        }
        let start = span.start + 1 + at;
        out.push(Reference {
            namespace,
            name: name.to_string(),
            range: start..start + name.len(),
        });
    }
    out
}

/// What the cursor is on: a definition's name, or a reference to one.
#[derive(Debug, Clone)]
pub(crate) struct Symbol {
    pub namespace: Namespace,
    pub name: String,
    pub range: Range<usize>,
    pub is_definition: bool,
}

pub(crate) fn symbol_at(text: &str, parsed: &Parsed, offset: usize) -> Option<Symbol> {
    let at = |range: &Range<usize>| range.contains(&offset) || range.end == offset;
    for reference in references(text, parsed) {
        if at(&reference.range) {
            return Some(Symbol {
                namespace: reference.namespace,
                name: reference.name,
                range: reference.range,
                is_definition: false,
            });
        }
    }
    for entry in &parsed.entries {
        let [root, name] = entry.path.as_slice() else {
            continue;
        };
        let namespace = match root.as_str() {
            "tasks" => Namespace::Task,
            "task_templates" => Namespace::Template,
            _ => continue,
        };
        if let Some(range) = entry.key.clone().filter(at) {
            return Some(Symbol {
                namespace,
                name: name.clone(),
                range,
                is_definition: true,
            });
        }
    }
    None
}

/// Every spelling of the key `<root>.<name>` in the text: in a header, as a dotted key at the
/// top of the document, or as a key of the `[<root>]` table. Lines inside a multi-line value
/// are left alone.
pub(crate) fn key_occurrences(
    text: &str,
    parsed: &Parsed,
    namespace: Namespace,
    name: &str,
) -> Vec<Range<usize>> {
    let root = namespace.root();
    let inside: Vec<Range<usize>> = parsed
        .entries
        .iter()
        .filter(|e| !e.table)
        .filter_map(|e| e.value.clone())
        .filter(|range| text.get(range.clone()).is_some_and(|s| s.contains('\n')))
        .collect();
    let mut out = vec![];
    let mut header: Vec<String> = vec![];
    let mut start = 0;
    for line in text.split_inclusive('\n') {
        let line_start = start;
        start += line.len();
        if inside
            .iter()
            .any(|r| r.start < line_start && line_start < r.end)
        {
            continue;
        }
        let trimmed = line.trim_start();
        let lead = line_start + line.len() - trimmed.len();
        if trimmed.starts_with('#') {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix('[') {
            let brackets = 1 + usize::from(rest.starts_with('['));
            let rest = rest.trim_start_matches('[');
            let end = unquoted(rest, ']').unwrap_or(rest.len());
            let segments = key_segments(&rest[..end], lead + brackets);
            header = segments.iter().map(|(name, _)| name.clone()).collect();
            if let [first, second, ..] = segments.as_slice()
                && first.0 == root
                && second.0 == name
            {
                out.push(second.1.clone());
            }
        } else if let Some(eq) = unquoted(trimmed, '=') {
            let segments = key_segments(&trimmed[..eq], lead);
            match (header.as_slice(), segments.as_slice()) {
                ([], [first, second, ..]) if first.0 == root && second.0 == name => {
                    out.push(second.1.clone());
                }
                ([only], [first, ..]) if only == root && first.0 == name => {
                    out.push(first.1.clone());
                }
                _ => {}
            }
        }
    }
    out
}

/// The segments of a dotted key with the bytes each takes (quotes included), `base` being where
/// `key` starts in the text.
fn key_segments(key: &str, base: usize) -> Vec<(String, Range<usize>)> {
    let mut segments = vec![];
    let mut from = 0;
    let mut quote = None;
    let push = |from: usize, to: usize, segments: &mut Vec<(String, Range<usize>)>| {
        let raw = &key[from..to];
        let trimmed = raw.trim();
        let lead = raw.len() - raw.trim_start().len();
        let name = keypath::split(trimmed).pop().unwrap_or_default();
        let at = base + from + lead;
        segments.push((name, at..at + trimmed.len()));
    };
    for (i, c) in key.char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '.') => {
                push(from, i, &mut segments);
                from = i + 1;
            }
            _ => {}
        }
    }
    push(from, key.len(), &mut segments);
    segments
}

/// The edits that rename a task or template and every reference to it in `text`, or why they
/// cannot be made. The result is parsed again and compared with the document it should be, so a
/// spelling this does not know about is an error and never a half-done rename.
pub(crate) fn rename(
    text: &str,
    parsed: &Parsed,
    namespace: Namespace,
    old: &str,
    new: &str,
) -> Result<Vec<(Range<usize>, String)>, String> {
    let mut edits: Vec<(Range<usize>, String)> = key_occurrences(text, parsed, namespace, old)
        .into_iter()
        .map(|range| (range, keypath::quote(new)))
        .collect();
    if edits.is_empty() {
        return Err(format!("`{old}` is not defined in this file"));
    }
    edits.extend(
        references(text, parsed)
            .into_iter()
            .filter(|r| r.namespace == namespace && r.name == old)
            .map(|r| (r.range, new.to_string())),
    );
    edits.sort_by_key(|(range, _)| range.start);
    edits.dedup_by(|a, b| a.0 == b.0);
    let mut renamed = text.to_string();
    for (range, replacement) in edits.iter().rev() {
        renamed.replace_range(range.clone(), replacement);
    }
    let expected = renamed_json(&parsed.json, &mut vec![], namespace, old, new);
    match doc::parse(&renamed) {
        Ok(after) if after.json == expected => Ok(edits),
        _ => Err(format!(
            "`{old}` is written in a way a rename cannot follow; rename it by hand"
        )),
    }
}

/// The document `json` with the key `<root>.<old>` and the references to it renamed.
fn renamed_json(
    json: &Value,
    path: &mut Vec<String>,
    namespace: Namespace,
    old: &str,
    new: &str,
) -> Value {
    match json {
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, value)| {
                    let key_renamed = path.len() == 1 && path[0] == namespace.root() && key == old;
                    path.push(key.clone());
                    let value = renamed_json(value, path, namespace, old, new);
                    path.pop();
                    (
                        if key_renamed {
                            new.to_string()
                        } else {
                            key.clone()
                        },
                        value,
                    )
                })
                .collect(),
        ),
        Value::Array(items) => Value::Array(
            items
                .iter()
                .enumerate()
                .map(|(i, value)| {
                    path.push(i.to_string());
                    let value = renamed_json(value, path, namespace, old, new);
                    path.pop();
                    value
                })
                .collect(),
        ),
        Value::String(s) if reference_kind(path) == Some(namespace) => match name_of(s) {
            Some((name, at)) if name == old => {
                Value::String(format!("{}{new}{}", &s[..at], &s[at + name.len()..]))
            }
            _ => json.clone(),
        },
        other => other.clone(),
    }
}

/// Whether `name` can be written as a task name in a reference string.
pub(crate) fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && !name
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || c == '"' || c == '\\' || c == '\'')
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: &str = r#"[tasks.build]
description = "compile it"
alias = "b"
run = "cargo build"

[tasks.test]
depends = ["build", "lint --fix"]
run = [{ task = "build" }, "echo done"]

[tasks.build.env]
FOO = "1"

[task_templates.rust]
description = "a rust task"
run = "cargo"

[tasks.ship]
extends = "rust"
depends = { task = "test" }
wait_for = [["build", "arg"]]
"#;

    fn parse(text: &str) -> Parsed {
        doc::parse(text).unwrap()
    }

    fn names(text: &str) -> Vec<(String, Namespace)> {
        references(text, &parse(text))
            .into_iter()
            .map(|r| (r.name, r.namespace))
            .collect()
    }

    #[test]
    fn the_open_document_defines_tasks_and_templates() {
        let index = Index::build(CONFIG, None);
        let tasks: Vec<_> = index.tasks.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(tasks, ["build", "test", "ship"]);
        let build = index.find(Namespace::Task, "b").unwrap();
        assert_eq!(build.name, "build");
        assert_eq!(build.description.as_deref(), Some("compile it"));
        assert_eq!(build.run.as_deref(), Some("cargo build"));
        assert!(index.find(Namespace::Template, "rust").is_some());
        assert!(index.find(Namespace::Task, "rust").is_none());
    }

    #[test]
    fn references_are_found_in_every_field() {
        let found = names(CONFIG);
        let tasks: Vec<_> = found
            .iter()
            .filter(|(_, ns)| *ns == Namespace::Task)
            .map(|(n, _)| n.as_str())
            .collect();
        assert_eq!(tasks, ["build", "lint", "build", "test", "build"]);
        assert!(found.contains(&("rust".to_string(), Namespace::Template)));
    }

    #[test]
    fn a_reference_range_is_the_name_alone() {
        let text = "[tasks.a]\ndepends = [\"lint --fix\"]\n";
        let reference = &references(text, &parse(text))[0];
        assert_eq!(&text[reference.range.clone()], "lint");
    }

    #[test]
    fn disk_files_add_definitions() {
        let root = std::env::temp_dir().join(format!("mise-lsp-tasks-{}", std::process::id()));
        std::fs::create_dir_all(root.join("sub/tasks")).unwrap();
        std::fs::write(
            root.join("mise.toml"),
            "[tasks.root]\nrun = \"x\"\n[task_config]\nincludes = [\"sub/tasks\"]\n",
        )
        .unwrap();
        std::fs::write(
            root.join("sub/tasks/more.toml"),
            "[lint]\ndescription = \"lint it\"\nrun = \"y\"\n",
        )
        .unwrap();
        let file = root.join("sub/mise.toml");
        let index = Index::build("[tasks.mine]\nrun = \"z\"\n", Some(&file));
        let tasks: Vec<_> = index.tasks.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(tasks, ["mine", "root", "lint"]);
        assert!(matches!(index.tasks[2].origin, Origin::File { .. }));
        assert!(describe(&index.tasks[2]).contains("Defined in `more.toml`"));
    }

    #[test]
    fn leading_environment_assignments_are_not_the_name() {
        let text =
            "[tasks.build]\n[tasks.a]\ndepends = [\"PROFILE=release build --x\", \"only=one\"]\n";
        let found: Vec<_> = references(text, &parse(text))
            .into_iter()
            .map(|r| text[r.range].to_string())
            .collect();
        assert_eq!(found, ["build", "only=one"]);
        let edits = rename(text, &parse(text), Namespace::Task, "build", "compile").unwrap();
        assert_eq!(edits.len(), 2);
        // a quoted environment value is not followed
        let quoted = "[tasks.a]\ndepends = [\"X='a b' build\"]\n";
        assert!(references(quoted, &parse(quoted)).is_empty());
    }

    #[test]
    fn the_includes_of_the_open_document_are_read() {
        let root = std::env::temp_dir().join(format!("mise-lsp-open-incl-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("tasks.toml"), "[lint]\nrun = \"x\"\n").unwrap();
        std::fs::write(root.join("mise.toml"), "").unwrap();
        let text = "[task_config]\nincludes = [\"tasks.toml\"]\n[tasks.ci]\ndepends = [\"lint\"]\n";
        let index = Index::build(text, Some(&root.join("mise.toml")));
        assert!(index.find(Namespace::Task, "lint").is_some());
    }

    #[test]
    fn a_directory_include_does_not_take_configs_for_task_files() {
        let root = std::env::temp_dir().join(format!("mise-lsp-dir-incl-{}", std::process::id()));
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(
            root.join("mise.toml"),
            "[task_config]\nincludes = [\".\"]\n[tasks.root]\nrun = \"x\"\n",
        )
        .unwrap();
        std::fs::write(root.join("mise.local.toml"), "[tasks.local]\nrun = \"x\"\n").unwrap();
        std::fs::write(root.join("sub/build.toml"), "[build]\nrun = \"x\"\n").unwrap();
        let index = Index::build("", Some(&root.join("sub/mise.toml")));
        let names: Vec<_> = index.tasks.iter().map(|d| d.name.as_str()).collect();
        assert!(
            names.contains(&"build") && names.contains(&"root"),
            "{names:?}"
        );
        assert_eq!(
            names.iter().filter(|n| **n == "local").count(),
            1,
            "{names:?}"
        );
    }

    #[test]
    fn other_files_references_are_remembered() {
        let root = std::env::temp_dir().join(format!("mise-lsp-elsewhere-{}", std::process::id()));
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(root.join("mise.toml"), "[tasks.x]\ndepends = [\"build\"]\n").unwrap();
        let index = Index::build("[tasks.build]\n", Some(&root.join("sub/mise.toml")));
        assert!(index.referenced_elsewhere(Namespace::Task, "build"));
        assert!(!index.referenced_elsewhere(Namespace::Task, "other"));
    }

    #[test]
    fn the_symbol_under_the_cursor() {
        let parsed = parse(CONFIG);
        let at = |needle: &str, nth: usize| {
            let offset = CONFIG.match_indices(needle).nth(nth).unwrap().0 + 1;
            symbol_at(CONFIG, &parsed, offset)
        };
        let definition = at("build]", 0).unwrap();
        assert!(definition.is_definition);
        assert_eq!(definition.name, "build");
        let reference = at("\"lint", 0).unwrap();
        assert!(!reference.is_definition);
        assert_eq!(reference.name, "lint");
        assert!(at("cargo build", 0).is_none());
    }

    #[test]
    fn renaming_changes_headers_references_and_subtables() {
        let parsed = parse(CONFIG);
        let edits = rename(CONFIG, &parsed, Namespace::Task, "build", "compile").unwrap();
        let mut renamed = CONFIG.to_string();
        for (range, text) in edits.iter().rev() {
            renamed.replace_range(range.clone(), text);
        }
        assert!(renamed.contains("[tasks.compile]"));
        assert!(renamed.contains("[tasks.compile.env]"));
        assert!(renamed.contains("depends = [\"compile\", \"lint --fix\"]"));
        assert!(renamed.contains("{ task = \"compile\" }"));
        assert!(renamed.contains("[[\"compile\", \"arg\"]]"));
        assert!(!renamed.contains("tasks.build"));
        assert!(renamed.contains("run = \"cargo build\""));
    }

    #[test]
    fn renaming_keeps_arguments_and_quotes_odd_names() {
        let text = "[tasks.a]\n[tasks.b]\ndepends = [\"a --x\"]\n";
        let edits = rename(text, &parse(text), Namespace::Task, "a", "x:y").unwrap();
        let mut renamed = text.to_string();
        for (range, replacement) in edits.iter().rev() {
            renamed.replace_range(range.clone(), replacement);
        }
        assert_eq!(
            renamed,
            "[tasks.\"x:y\"]\n[tasks.b]\ndepends = [\"x:y --x\"]\n"
        );
    }

    #[test]
    fn dotted_keys_are_renamed_too() {
        let text = "tasks.a.run = \"x\"\ntasks.a.description = \"d\"\ntasks.b.run = \"y\"\n";
        let edits = rename(text, &parse(text), Namespace::Task, "a", "z").unwrap();
        assert_eq!(edits.len(), 2);
        let text = "[tasks]\na.run = \"x\"\na.description = \"d\"\nb.run = \"y\"\n";
        let edits = rename(text, &parse(text), Namespace::Task, "a", "z").unwrap();
        assert_eq!(edits.len(), 2);
    }

    #[test]
    fn a_rename_that_cannot_be_followed_is_refused() {
        // an inline table the key scan does not read
        let text = "tasks = { a = { run = \"x\" } }\n";
        assert!(rename(text, &parse(text), Namespace::Task, "a", "b").is_err());
    }

    #[test]
    fn templates_rename_through_extends() {
        let parsed = parse(CONFIG);
        let edits = rename(CONFIG, &parsed, Namespace::Template, "rust", "cargo").unwrap();
        assert_eq!(edits.len(), 2);
    }

    #[test]
    fn reference_kinds_follow_the_shapes() {
        let kind =
            |path: &[&str]| reference_kind(&path.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        use Namespace::*;
        assert_eq!(kind(&["tasks", "a", "depends"]), Some(Task));
        assert_eq!(kind(&["tasks", "a", "depends", "0"]), Some(Task));
        assert_eq!(kind(&["tasks", "a", "depends", "0", "task"]), Some(Task));
        assert_eq!(kind(&["tasks", "a", "depends", "1", "1"]), None);
        assert_eq!(kind(&["tasks", "a", "run", "0", "task"]), Some(Task));
        assert_eq!(kind(&["tasks", "a", "run", "0", "tasks", "2"]), Some(Task));
        assert_eq!(kind(&["tasks", "a", "run", "0"]), None);
        assert_eq!(kind(&["tasks", "a", "extends"]), Some(Template));
        assert_eq!(kind(&["tasks", "a", "alias"]), None);
        assert_eq!(kind(&["task_templates", "a", "depends"]), Some(Task));
        assert_eq!(kind(&["task_templates", "a", "extends"]), None);
        assert_eq!(kind(&["settings", "a", "depends"]), None);
    }
}
