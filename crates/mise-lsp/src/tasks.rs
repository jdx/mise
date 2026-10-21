//! Tasks and task templates: where a config defines them and where it refers to them.
//!
//! A task is `[tasks.<name>]`. Other tasks name it in `depends`, `depends_post`, `wait_for` and
//! `run = [{ task = "<name>" }]`; `extends = "<name>"` names a template, `[task_templates.<name>]`,
//! instead. The server finds both from the parsed config, so nothing is rendered or run. Besides
//! the open document it reads the configs in the directories above it and the TOML files their
//! `[task_config] includes` name, the way [`crate::includes`] does.

use std::collections::{HashMap, HashSet, VecDeque};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use regex::Regex;
use serde_json::Value;

use crate::complete::unquoted;
use crate::diagnostics::{Diagnostic, Severity};
use crate::doc::{self, Parsed};
use crate::includes;
use crate::keypath;
use crate::text::LineIndex;

/// More files than this are not read for tasks.
const MAX_FILES: usize = 200;

/// Directory entries the scans for file tasks look at, in all the directories together.
const MAX_SCAN: usize = 5000;

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
pub(crate) struct Index {
    pub tasks: Vec<Definition>,
    pub templates: Vec<Definition>,
    /// The names that configs and include files other than the open document refer to.
    disk_references: HashSet<(Namespace, String)>,
    /// The names file tasks (scripts) answer to: their path with and without the extension, and
    /// the aliases their `#MISE alias=` lines give.
    file_tasks: HashSet<String>,
    /// Whether every place tasks can come from was read. A glob in `includes`, a remote include,
    /// a custom `task_config.dir` or a scan that hit its limit leaves names this index cannot know.
    pub complete: bool,
    /// Directory entries left to look at, for the scans of all the task directories.
    scan_budget: usize,
    /// The task files already read, so one a directory and an `includes` entry both reach is read once.
    read_files: HashSet<PathBuf>,
}

/// Where else configs and tasks live: the user's and the system's.
#[derive(Default, Clone)]
pub(crate) struct Roots {
    /// Directories that hold configs the way a project does (`<dir>/.config/mise/config.toml`,
    /// `<dir>/mise/config.toml`).
    pub homes: Vec<PathBuf>,
    /// Directories that are a mise config directory themselves (`<dir>/config.toml`,
    /// `<dir>/conf.d`, `<dir>/tasks`), such as the one `MISE_CONFIG_DIR` names.
    pub config_dirs: Vec<PathBuf>,
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

/// The directories below a project that hold file tasks unless `task_config.dir` says otherwise.
const FILE_TASK_DIRS: [&str; 5] = [
    "mise-tasks",
    ".mise-tasks",
    ".mise/tasks",
    "mise/tasks",
    ".config/mise/tasks",
];

impl Index {
    /// `open` is the text of the open document, `file` its path. The definitions of the document
    /// come first, so they win over a file that defines the same name. `roots` are places outside
    /// the file's own ancestors whose configs also apply.
    pub(crate) fn build(open: &str, file: Option<&Path>, roots: &Roots) -> Self {
        let mut index = Index {
            tasks: vec![],
            templates: vec![],
            disk_references: HashSet::new(),
            file_tasks: HashSet::new(),
            complete: true,
            scan_budget: MAX_SCAN,
            read_files: HashSet::new(),
        };
        let mut seen: HashSet<PathBuf> = HashSet::new();
        if let Ok(parsed) = doc::parse(open) {
            index.add(&parsed, Layout::Config, &|range| Origin::Open(range));
            index.complete &= parsed.json["task_config"].get("dir").is_none();
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
        let homes = file
            .ancestors()
            .skip(1)
            // a root that is also an ancestor is walked once
            .chain(
                roots
                    .homes
                    .iter()
                    .map(PathBuf::as_path)
                    .filter(|root| !file.ancestors().skip(1).any(|a| a == *root)),
            );
        for dir in homes {
            for sub in FILE_TASK_DIRS {
                index.add_file_tasks(&dir.join(sub), false);
            }
            let configs = includes::config_files(dir);
            index.add_configs(configs, &mut seen);
        }
        for dir in &roots.config_dirs {
            index.add_file_tasks(&dir.join("tasks"), false);
            let configs = config_dir_files(dir);
            index.add_configs(configs, &mut seen);
        }
        index
    }

    fn add_configs(&mut self, configs: Vec<PathBuf>, seen: &mut HashSet<PathBuf>) {
        for config in configs {
            if seen.contains(&config) {
                continue;
            }
            if seen.len() >= MAX_FILES {
                // a config past the limit is not read
                self.complete = false;
                return;
            }
            seen.insert(config.clone());
            let Some(text) = includes::read_capped(&config) else {
                continue;
            };
            let Ok(parsed) = doc::parse(&text) else {
                continue;
            };
            self.add(&parsed, Layout::Config, &origin_in(&config, &text));
            self.complete &= parsed.json["task_config"].get("dir").is_none();
            self.note_references(&text, &parsed, Layout::Config);
            self.add_includes(&config, &parsed, seen);
        }
    }

    /// The tasks of the TOML files `config` lists in `[task_config] includes`, and the file
    /// tasks of the directories it lists.
    fn add_includes(&mut self, config: &Path, parsed: &Parsed, seen: &mut HashSet<PathBuf>) {
        let listed: Vec<String> = parsed.json["task_config"]["includes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect();
        let included = includes::task_include_files(config, &listed);
        self.complete &= !included.skipped;
        for dir in &included.dirs {
            // an included directory may be a whole project: only its executables are tasks
            self.add_file_tasks(dir, true);
        }
        for path in included.files {
            if seen.contains(&path) {
                continue;
            }
            if seen.len() >= MAX_FILES {
                self.complete = false;
                return;
            }
            seen.insert(path.clone());
            if !self.read_files.insert(path.clone()) {
                continue;
            }
            let Some(text) = includes::read_capped(&path) else {
                continue;
            };
            if let Ok(parsed) = doc::parse(&text) {
                self.add(&parsed, Layout::TaskFile, &origin_in(&path, &text));
                self.note_references(&text, &parsed, Layout::TaskFile);
            }
        }
    }

    /// Remembers what a file other than the open one refers to.
    fn note_references(&mut self, text: &str, parsed: &Parsed, layout: Layout) {
        for reference in references_in(text, parsed, layout) {
            self.disk_references
                .insert((reference.namespace, reference.name));
        }
    }

    /// Whether a config or include file besides the open document names the task.
    pub(crate) fn referenced_elsewhere(&self, namespace: Namespace, name: &str) -> bool {
        self.disk_references
            .contains(&(namespace, name.to_string()))
    }

    /// The scripts below `dir` are tasks named by their path: `test/unit.sh` is `test:unit`, and
    /// `mise run test:unit.sh` and `test:unit` both find it. A `#MISE alias="x"` line in a script
    /// adds a name. Symlinks are not followed, and the scan of all directories shares one budget.
    fn add_file_tasks(&mut self, dir: &Path, executables_only: bool) {
        fn walk(dir: &Path, prefix: &str, depth: usize, only: bool, index: &mut Index) {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };
            for entry in entries.flatten() {
                if index.scan_budget == 0 {
                    index.complete = false;
                    return;
                }
                index.scan_budget -= 1;
                let path = entry.path();
                // the runner reads a colon in a file or directory name as an underscore
                let name = entry.file_name().to_string_lossy().replace(':', "_");
                let task = format!("{prefix}{name}");
                let Ok(kind) = entry.file_type() else {
                    continue;
                };
                // A link to a file is a file task, as it is to the runner; one to a directory is
                // not followed, so what it holds is unknown.
                let is_file = kind.is_file() || (kind.is_symlink() && path.is_file());
                if kind.is_dir() || (kind.is_symlink() && path.is_dir()) {
                    if only && matches!(name.as_str(), ".git" | "node_modules" | "target") {
                        // Skipped, because they can be huge; only `.git` surely holds no tasks.
                        if name != ".git" {
                            index.complete = false;
                        }
                    } else if kind.is_dir() && depth < 4 {
                        walk(&path, &format!("{task}:"), depth + 1, only, index);
                    } else {
                        index.complete = false;
                    }
                } else if is_file
                    && includes::is_standard_config(&path.to_string_lossy().replace('\\', "/"))
                {
                    // a config mise reads as one, not a task file or a script
                } else if is_file
                    && path
                        .extension()
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("toml"))
                {
                    // a TOML file here holds task definitions, `[lint]` defining `lint`
                    if index.read_files.insert(path.clone())
                        && let Some(text) = includes::read_capped(&path)
                        && let Ok(parsed) = doc::parse(&text)
                    {
                        index.add(&parsed, Layout::TaskFile, &origin_in(&path, &text));
                        index.note_references(&text, &parsed, Layout::TaskFile);
                    }
                } else if is_file && (!only || is_executable(&path)) {
                    // only a known script extension is dropped: `gen.proto` stays `gen.proto`
                    let stem = if is_script_extension(&name) {
                        Path::new(&name)
                            .file_stem()
                            .map_or(name.clone(), |s| s.to_string_lossy().into_owned())
                    } else {
                        name.clone()
                    };
                    // `test/_default` is the task `test`
                    if stem == "_default" && !prefix.is_empty() {
                        index
                            .file_tasks
                            .insert(prefix.trim_end_matches(':').to_string());
                    }
                    index.file_tasks.insert(format!("{prefix}{stem}"));
                    index.file_tasks.insert(task);
                    index.file_tasks.extend(script_aliases(&path));
                }
            }
        }
        walk(dir, "", 0, executables_only, self);
    }

    /// Whether a task or template of this name is defined somewhere the index looked.
    pub(crate) fn knows(&self, namespace: Namespace, name: &str) -> bool {
        self.find(namespace, name).is_some()
            || (namespace == Namespace::Task && self.file_tasks.contains(name))
    }

    /// Every name that something is known by, to suggest the near miss of a mistyped one.
    pub(crate) fn names(&self, namespace: Namespace) -> Vec<&str> {
        let mut names: Vec<&str> = self
            .definitions(namespace)
            .iter()
            .map(|d| d.name.as_str())
            .collect();
        if namespace == Namespace::Task {
            names.extend(self.file_tasks.iter().map(String::as_str));
        }
        names.sort_unstable();
        names.dedup();
        names
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

/// A place in the document that names a task or a template.
#[derive(Debug, Clone)]
pub(crate) struct Reference {
    pub namespace: Namespace,
    pub name: String,
    /// The name, without the arguments that may follow it in the string.
    pub range: Range<usize>,
    /// The path of the string in the document.
    pub path: Vec<String>,
}

/// Whether a word is `KEY=value`, which sets the environment of the task that follows it.
pub(crate) fn is_env_assignment(word: &str) -> bool {
    word.split_once('=').is_some_and(|(key, _)| {
        !key.is_empty() && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    })
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
        if is_env_assignment(word) && words.peek().is_some() {
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
    references_in(text, parsed, Layout::Config)
}

/// The references in a document laid out as `layout`: in a task file each top-level table is a
/// task, so its paths read as if they were below `tasks`.
fn references_in(text: &str, parsed: &Parsed, layout: Layout) -> Vec<Reference> {
    let mut out = vec![];
    for entry in &parsed.entries {
        let path = match layout {
            Layout::Config => entry.path.clone(),
            Layout::TaskFile => [&["tasks".to_string()][..], &entry.path].concat(),
        };
        let (Some(namespace), Some(span)) = (reference_kind(&path), entry.value.clone()) else {
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
            path: entry.path.clone(),
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

/// Problems with the tasks a document names: a name nothing defines, and tasks that wait for
/// each other.
pub(crate) fn diagnose(text: &str, parsed: &Parsed, index: &Index) -> Vec<Diagnostic> {
    let mut out = vec![];
    let found = references(text, parsed);
    if index.complete {
        for reference in &found {
            if is_pattern(&reference.name) || is_optional(&parsed.json, &reference.path) {
                continue;
            }
            let namespace = reference.namespace;
            if index.knows(namespace, &reference.name) {
                continue;
            }
            let mut message = format!("unknown {} `{}`", namespace.noun(), reference.name);
            // A name that needs escaping in a TOML string gets no fix.
            let suggestion = keypath::closest(&reference.name, index.names(namespace).into_iter())
                .filter(|name| valid_name(name));
            if let Some(suggestion) = suggestion {
                message.push_str(&format!("; did you mean `{suggestion}`?"));
            }
            let mut d = Diagnostic::new(
                reference.range.clone(),
                &reference.path,
                Severity::Warning,
                "unknown-task",
                message,
            );
            d.suggestion = suggestion.map(str::to_string);
            d.key = Some(reference.name.clone());
            out.push(d);
        }
    }
    cycles(&found, index, &mut out);
    out
}

/// The configs in a directory that is a mise config directory itself: `config.toml`, its local
/// and per-environment variants (`config.local.toml`, `config.dev.toml`), and the TOML files of
/// `conf.d`, which may sit in folders there.
fn config_dir_files(dir: &Path) -> Vec<PathBuf> {
    let is_toml = |p: &Path| {
        p.extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("toml"))
    };
    let mut out = vec![];
    if let Ok(entries) = std::fs::read_dir(dir) {
        out.extend(entries.flatten().map(|e| e.path()).filter(|p| {
            is_toml(p)
                && p.file_name().is_some_and(|n| {
                    let n = n.to_string_lossy();
                    n.starts_with("config.") || n.starts_with("mise.") || n.starts_with(".mise.")
                })
        }));
    }
    if let Ok(entries) = std::fs::read_dir(dir.join("conf.d")) {
        for path in entries.flatten().map(|e| e.path()) {
            if path.is_dir() {
                if let Ok(inner) = std::fs::read_dir(&path) {
                    // only the names mise loads: a helper file in a folder is not a config
                    out.extend(inner.flatten().map(|e| e.path()).filter(|p| {
                        is_toml(p)
                            && p.file_name().is_some_and(|n| {
                                let n = n.to_string_lossy();
                                n.starts_with("mise") || n.starts_with(".mise")
                            })
                    }));
                }
            } else if is_toml(&path) {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

/// The extensions mise drops from a file task's name (`SCRIPT_EXTENSIONS` in mise itself).
const SCRIPT_EXTENSIONS: [&str; 22] = [
    "bash", "bat", "cjs", "cmd", "com", "exe", "fish", "js", "ksh", "lua", "mjs", "mts", "nu",
    "php", "pl", "ps1", "py", "rb", "sh", "ts", "vbs", "zsh",
];

fn is_script_extension(file_name: &str) -> bool {
    Path::new(file_name).extension().is_some_and(|ext| {
        SCRIPT_EXTENSIONS
            .iter()
            .any(|k| ext.eq_ignore_ascii_case(k))
    })
}

/// Whether the file is one mise runs as a task: executable, or on a platform without an executable
/// bit, a script by extension or starting with a `#!` line.
fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path).is_ok_and(|meta| meta.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        use std::io::Read;
        let mut head = [0u8; 2];
        // the extensions the runner runs by default, or a `#!` line
        let by_extension = path.extension().is_some_and(|ext| {
            ["cmd", "bat", "ps1", "exe", "com", "vbs"]
                .iter()
                .any(|known| ext.eq_ignore_ascii_case(known))
        });
        by_extension
            || std::fs::File::open(path)
                .and_then(|mut f| f.read_exact(&mut head))
                .is_ok_and(|()| &head == b"#!")
    }
}

/// The names a script's `#MISE alias="x"` (or `alias = ["x", "y"]`) header lines give it. Only
/// the top of the file is read, and nothing is run.
fn script_aliases(path: &Path) -> Vec<String> {
    use std::io::Read;
    static ALIAS: OnceLock<Regex> = OnceLock::new();
    let re = ALIAS.get_or_init(|| {
        Regex::new(r#"^\s*(?:#|//)\s*\[?MISE\]?\s+alias\s*=\s*(.+?)\s*$"#).unwrap()
    });
    let mut head = vec![0; 4096];
    let Ok(read) = std::fs::File::open(path).and_then(|mut f| f.read(&mut head)) else {
        return vec![];
    };
    head.truncate(read);
    let mut aliases = vec![];
    for line in String::from_utf8_lossy(&head).lines() {
        let Some(value) = re.captures(line).map(|c| c[1].to_string()) else {
            continue;
        };
        if let Ok(doc) = toml_edit::Document::parse(format!("a = {value}"))
            && let Some(item) = doc.get("a")
        {
            match item.as_value() {
                Some(v) if v.is_str() => aliases.extend(v.as_str().map(str::to_string)),
                Some(v) => aliases.extend(
                    v.as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|e| e.as_str().map(str::to_string)),
                ),
                None => {}
            }
        }
    }
    aliases
}

/// A name that stands for several tasks, or for one in another project, which only mise can
/// resolve.
fn is_pattern(name: &str) -> bool {
    name.contains(['*', '?', '[', '{']) || name.starts_with(['/', ':', '^'])
}

/// A dependency written as `{ task = "x", optional = true }` may name a task that is missing.
fn is_optional(json: &Value, path: &[String]) -> bool {
    path.split_last()
        .filter(|(last, _)| *last == "task")
        .and_then(|(_, parent)| value_at(json, parent))
        .and_then(|table| table.get("optional"))
        .is_some_and(|optional| optional == true)
}

/// Reports every `depends` that is part of a cycle among the tasks the document defines.
fn cycles(found: &[Reference], index: &Index, out: &mut Vec<Diagnostic>) {
    let defined: HashSet<&str> = index
        .tasks
        .iter()
        .filter(|d| matches!(d.origin, Origin::Open(_)))
        .map(|d| d.name.as_str())
        .collect();
    // owner -> (target, the reference that says so)
    let mut edges: HashMap<&str, Vec<(String, &Reference)>> = HashMap::new();
    for reference in found {
        let [root, owner, field, ..] = reference.path.as_slice() else {
            continue;
        };
        if root != "tasks" || field != "depends" || reference.namespace != Namespace::Task {
            continue;
        }
        let Some(target) = index.find(Namespace::Task, &reference.name) else {
            continue;
        };
        // The runner drops a task's dependency on itself, so it is not a cycle.
        if target.name != *owner
            && defined.contains(target.name.as_str())
            && defined.contains(owner.as_str())
        {
            edges
                .entry(owner)
                .or_default()
                .push((target.name.clone(), reference));
        }
    }
    // the shortest way from `from` back to `to`, by the tasks on it
    let route = |from: &str, to: &str| -> Option<Vec<String>> {
        let mut previous: HashMap<String, String> = HashMap::new();
        let mut queue = VecDeque::from([from.to_string()]);
        while let Some(task) = queue.pop_front() {
            if task == to {
                let mut path = vec![task.clone()];
                let mut at = task;
                while let Some(before) = previous.get(&at) {
                    path.push(before.clone());
                    at = before.clone();
                }
                path.reverse();
                return Some(path);
            }
            for (next, _) in edges.get(task.as_str()).into_iter().flatten() {
                if next != from && !previous.contains_key(next) {
                    previous.insert(next.clone(), task.clone());
                    queue.push_back(next.clone());
                }
            }
        }
        None
    };
    for (owner, targets) in &edges {
        for (target, reference) in targets {
            let Some(back) = route(target, owner) else {
                continue;
            };
            let mut cycle = vec![owner.to_string()];
            cycle.extend(back);
            out.push(Diagnostic::new(
                reference.range.clone(),
                &reference.path,
                Severity::Error,
                "task-cycle",
                format!("dependency cycle: {}", cycle.join(" -> ")),
            ));
        }
    }
    out.sort_by_key(|d| d.range.start);
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
    // `KEY=value` would read as an environment assignment before the task name.
    let assignment = name.split_once('=').is_some_and(|(key, _)| {
        !key.is_empty() && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    });
    !name.is_empty()
        && !assignment
        && !name
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || matches!(c, '"' | '\\' | '\''))
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
        let index = Index::build(CONFIG, None, &Roots::default());
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
        let index = Index::build(
            "[tasks.mine]\nrun = \"z\"\n",
            Some(&file),
            &Roots::default(),
        );
        let tasks: Vec<_> = index.tasks.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(tasks, ["mine", "root", "lint"]);
        assert!(matches!(index.tasks[2].origin, Origin::File { .. }));
        assert!(describe(&index.tasks[2]).contains("Defined in `more.toml`"));
    }

    #[test]
    fn names_that_read_as_environment_assignments_are_not_valid() {
        assert!(!valid_name("X=y"));
        assert!(!valid_name("a=b:c"));
        assert!(valid_name("build:release"));
        assert!(
            valid_name("--x=y"),
            "a key with a dash is not an assignment"
        );
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
        let index = Index::build(text, Some(&root.join("mise.toml")), &Roots::default());
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
        let index = Index::build("", Some(&root.join("sub/mise.toml")), &Roots::default());
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
    fn references_in_included_task_files_are_remembered() {
        let root = std::env::temp_dir().join(format!("mise-lsp-incl-refs-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("ci.toml"), "[ci]\ndepends = [\"build\"]\n").unwrap();
        let text = "[task_config]\nincludes = [\"ci.toml\"]\n[tasks.build]\n";
        let index = Index::build(text, Some(&root.join("mise.toml")), &Roots::default());
        assert!(index.referenced_elsewhere(Namespace::Task, "build"));
    }

    #[test]
    fn other_files_references_are_remembered() {
        let root = std::env::temp_dir().join(format!("mise-lsp-elsewhere-{}", std::process::id()));
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(root.join("mise.toml"), "[tasks.x]\ndepends = [\"build\"]\n").unwrap();
        let index = Index::build(
            "[tasks.build]\n",
            Some(&root.join("sub/mise.toml")),
            &Roots::default(),
        );
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

    fn problems(text: &str) -> Vec<(&'static str, String)> {
        let parsed = parse(text);
        diagnose(text, &parsed, &Index::build(text, None, &Roots::default()))
            .into_iter()
            .map(|d| (d.code, d.message))
            .collect()
    }

    #[test]
    fn a_name_nothing_defines_is_unknown() {
        let text = "[tasks.build]\n[tasks.test]\ndepends = [\"bild\", \"build\"]\nwait_for = \"nope\"\nextends = \"tmpl\"\n";
        assert_eq!(
            problems(text),
            [
                (
                    "unknown-task",
                    "unknown task `bild`; did you mean `build`?".to_string()
                ),
                ("unknown-task", "unknown task `nope`".to_string()),
                ("unknown-task", "unknown task template `tmpl`".to_string()),
            ]
        );
    }

    #[test]
    fn patterns_aliases_and_optional_dependencies_are_not_unknown() {
        let text = "[tasks.build]\nalias = \"b\"\n[tasks.test]\ndepends = [\"b\", \"lint:*\", \"//other:x\", \":y\", { task = \"z\", optional = true }]\n";
        assert!(problems(text).is_empty());
    }

    #[test]
    fn what_the_index_cannot_see_is_not_called_unknown() {
        let text = "[task_config]\ndir = \"scripts\"\n[tasks.a]\ndepends = [\"x\"]\n";
        assert!(problems(text).is_empty());
        let root = std::env::temp_dir().join(format!("mise-lsp-unseen-{}", std::process::id()));
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(
            root.join("mise.toml"),
            "[task_config]\nincludes = [\"tasks/*.toml\"]\n",
        )
        .unwrap();
        let text = "[tasks.a]\ndepends = [\"x\"]\n";
        let file = root.join("sub/mise.toml");
        let parsed = parse(text);
        let index = Index::build(text, Some(&file), &Roots::default());
        assert!(!index.complete);
        assert!(diagnose(text, &parsed, &index).is_empty());
    }

    #[test]
    fn file_tasks_are_known_with_or_without_their_extension() {
        let root = std::env::temp_dir().join(format!("mise-lsp-scripts-{}", std::process::id()));
        std::fs::create_dir_all(root.join("mise-tasks/test")).unwrap();
        std::fs::write(root.join("mise-tasks/deploy.sh"), "#!/bin/sh\n").unwrap();
        std::fs::write(root.join("mise-tasks/test/unit"), "#!/bin/sh\n").unwrap();
        std::fs::write(root.join("mise.toml"), "").unwrap();
        let file = root.join("sub/mise.toml");
        let index = Index::build("", Some(&file), &Roots::default());
        for name in ["deploy", "deploy.sh", "test:unit"] {
            assert!(index.knows(Namespace::Task, name), "{name}");
        }
        assert!(!index.knows(Namespace::Task, "test"));
    }

    #[test]
    fn bracket_and_brace_patterns_are_not_unknown() {
        let text = "[tasks.a]\ndepends = [\"test:[12]\", \"{build,test}\"]\n";
        assert!(problems(text).is_empty());
    }

    #[test]
    fn toml_files_in_a_task_directory_define_tasks() {
        let root = std::env::temp_dir().join(format!("mise-lsp-tomldir-{}", std::process::id()));
        std::fs::create_dir_all(root.join("mise-tasks")).unwrap();
        std::fs::write(root.join("mise-tasks/checks.toml"), "[lint]\nrun = \"x\"\n").unwrap();
        let index = Index::build("", Some(&root.join("mise.toml")), &Roots::default());
        assert!(index.find(Namespace::Task, "lint").is_some());
    }

    #[cfg(unix)]
    #[test]
    fn links_to_scripts_count_and_links_to_directories_hide_names() {
        let root = std::env::temp_dir().join(format!("mise-lsp-links-{}", std::process::id()));
        std::fs::create_dir_all(root.join("mise-tasks")).unwrap();
        std::fs::create_dir_all(root.join("shared")).unwrap();
        std::fs::write(root.join("shared/deploy.sh"), "").unwrap();
        let _ = std::fs::remove_file(root.join("mise-tasks/deploy"));
        std::os::unix::fs::symlink(
            root.join("shared/deploy.sh"),
            root.join("mise-tasks/deploy"),
        )
        .unwrap();
        let index = Index::build("", Some(&root.join("mise.toml")), &Roots::default());
        assert!(index.knows(Namespace::Task, "deploy"));
        assert!(index.complete);
        let _ = std::fs::remove_file(root.join("mise-tasks/more"));
        std::os::unix::fs::symlink(root.join("shared"), root.join("mise-tasks/more")).unwrap();
        let index = Index::build("", Some(&root.join("mise.toml")), &Roots::default());
        assert!(!index.complete);
    }

    #[cfg(unix)]
    #[test]
    fn an_included_directory_only_contributes_its_executables() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!("mise-lsp-exec-{}", std::process::id()));
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("node_modules/x")).unwrap();
        std::fs::write(root.join("src/build.rs"), "").unwrap();
        std::fs::write(root.join("node_modules/x/tool"), "").unwrap();
        std::fs::write(root.join("deploy"), "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(root.join("deploy"), std::fs::Permissions::from_mode(0o755))
            .unwrap();
        std::fs::write(
            root.join("mise.toml"),
            "[task_config]\nincludes = [\".\"]\n",
        )
        .unwrap();
        let index = Index::build("", Some(&root.join("sub/mise.toml")), &Roots::default());
        assert!(index.knows(Namespace::Task, "deploy"));
        assert!(!index.knows(Namespace::Task, "build"));
        assert!(!index.knows(Namespace::Task, "tool"));
        // what was skipped could hold tasks, so nothing is reported as unknown
        assert!(!index.complete);
    }

    #[test]
    fn references_in_toml_files_of_included_directories_are_remembered() {
        let root = std::env::temp_dir().join(format!("mise-lsp-dirrefs-{}", std::process::id()));
        std::fs::create_dir_all(root.join("tasks")).unwrap();
        std::fs::write(root.join("tasks/ci.toml"), "[ci]\ndepends = [\"build\"]\n").unwrap();
        let text = "[task_config]\nincludes = [\"tasks\"]\n[tasks.build]\n";
        let index = Index::build(text, Some(&root.join("mise.toml")), &Roots::default());
        assert!(index.referenced_elsewhere(Namespace::Task, "build"));
    }

    #[cfg(unix)]
    #[test]
    fn a_colon_in_a_file_name_is_an_underscore() {
        let root = std::env::temp_dir().join(format!("mise-lsp-colon-{}", std::process::id()));
        std::fs::create_dir_all(root.join("mise-tasks")).unwrap();
        std::fs::write(root.join("mise-tasks/a:b"), "").unwrap();
        let index = Index::build("", Some(&root.join("mise.toml")), &Roots::default());
        assert!(index.knows(Namespace::Task, "a_b"));
        assert!(!index.knows(Namespace::Task, "a:b"));
    }

    #[test]
    fn a_default_script_is_its_directorys_task() {
        let root = std::env::temp_dir().join(format!("mise-lsp-default-{}", std::process::id()));
        std::fs::create_dir_all(root.join("mise-tasks/test")).unwrap();
        std::fs::write(root.join("mise-tasks/test/_default"), "").unwrap();
        let index = Index::build("", Some(&root.join("mise.toml")), &Roots::default());
        assert!(index.knows(Namespace::Task, "test"));
    }

    #[test]
    fn configs_in_a_task_directory_are_not_task_files() {
        let root = std::env::temp_dir().join(format!("mise-lsp-stdcfg-{}", std::process::id()));
        std::fs::create_dir_all(root.join("mise-tasks")).unwrap();
        std::fs::write(
            root.join("mise-tasks/mise.toml"),
            "[tools]\nnode = \"24\"\n",
        )
        .unwrap();
        let index = Index::build("", Some(&root.join("mise.toml")), &Roots::default());
        assert!(index.find(Namespace::Task, "tools").is_none());
        assert!(!index.knows(Namespace::Task, "mise"));
    }

    #[test]
    fn only_known_script_extensions_are_dropped_from_a_name() {
        let root = std::env::temp_dir().join(format!("mise-lsp-ext-{}", std::process::id()));
        std::fs::create_dir_all(root.join("mise-tasks")).unwrap();
        std::fs::write(root.join("mise-tasks/gen.proto"), "").unwrap();
        std::fs::write(root.join("mise-tasks/run.sh"), "").unwrap();
        let index = Index::build("", Some(&root.join("mise.toml")), &Roots::default());
        assert!(index.knows(Namespace::Task, "gen.proto"));
        assert!(!index.knows(Namespace::Task, "gen"));
        assert!(index.knows(Namespace::Task, "run"));
        assert!(index.knows(Namespace::Task, "run.sh"));
    }

    #[test]
    fn upstream_references_are_skipped() {
        let text = "[tasks.a]\ndepends = [\"^build\"]\n";
        assert!(problems(text).is_empty());
    }

    #[test]
    fn a_script_alias_is_a_name() {
        let root = std::env::temp_dir().join(format!("mise-lsp-alias-{}", std::process::id()));
        std::fs::create_dir_all(root.join(".mise/tasks")).unwrap();
        std::fs::write(
            root.join(".mise/tasks/filetask"),
            "#!/bin/sh\n#MISE alias=\"ft\"\n#MISE description=\"x\"\n",
        )
        .unwrap();
        std::fs::write(
            root.join(".mise/tasks/two"),
            "#!/bin/sh\n#MISE alias=[\"a\", \"b\"]\n",
        )
        .unwrap();
        let index = Index::build("", Some(&root.join("mise.toml")), &Roots::default());
        for name in ["filetask", "ft", "a", "b"] {
            assert!(index.knows(Namespace::Task, name), "{name}");
        }
    }

    #[test]
    fn a_scan_that_hits_its_limit_is_not_complete() {
        let root = std::env::temp_dir().join(format!("mise-lsp-limit-{}", std::process::id()));
        std::fs::create_dir_all(root.join("mise-tasks")).unwrap();
        std::fs::write(root.join("mise-tasks/a"), "").unwrap();
        let mut index = Index::build("", None, &Roots::default());
        index.scan_budget = 0;
        index.add_file_tasks(&root.join("mise-tasks"), false);
        assert!(!index.complete);
    }

    #[test]
    fn a_config_dir_reads_local_variants_and_conf_d_folders() {
        let root = std::env::temp_dir().join(format!("mise-lsp-confvar-{}", std::process::id()));
        std::fs::create_dir_all(root.join("cfg/conf.d/group")).unwrap();
        std::fs::write(root.join("cfg/mise.toml"), "[tasks.plain]\nrun = \"x\"\n").unwrap();
        std::fs::write(
            root.join("cfg/config.local.toml"),
            "[tasks.local]\nrun = \"x\"\n",
        )
        .unwrap();
        std::fs::write(
            root.join("cfg/conf.d/group/mise.a.toml"),
            "[tasks.nested]\nrun = \"x\"\n",
        )
        .unwrap();
        std::fs::write(
            root.join("cfg/conf.d/group/helper.toml"),
            "[tasks.helper]\nrun = \"x\"\n",
        )
        .unwrap();
        let roots = Roots {
            homes: vec![],
            config_dirs: vec![root.join("cfg")],
        };
        let index = Index::build("", Some(&root.join("proj/mise.toml")), &roots);
        assert!(index.knows(Namespace::Task, "local"));
        assert!(index.knows(Namespace::Task, "plain"));
        assert!(index.knows(Namespace::Task, "nested"));
        assert!(!index.knows(Namespace::Task, "helper"));
    }

    #[test]
    fn a_config_dir_outside_the_project_counts() {
        let root = std::env::temp_dir().join(format!("mise-lsp-confdir-{}", std::process::id()));
        std::fs::create_dir_all(root.join("cfg/tasks")).unwrap();
        std::fs::write(
            root.join("cfg/config.toml"),
            "[tasks.global]\nrun = \"x\"\n",
        )
        .unwrap();
        std::fs::write(root.join("cfg/tasks/script"), "").unwrap();
        let roots = Roots {
            homes: vec![],
            config_dirs: vec![root.join("cfg")],
        };
        let index = Index::build("", Some(&root.join("proj/mise.toml")), &roots);
        assert!(index.knows(Namespace::Task, "global"));
        assert!(index.knows(Namespace::Task, "script"));
    }

    #[test]
    fn dependency_cycles_are_reported_on_each_edge() {
        let text = "[tasks.a]\ndepends = [\"b\"]\n[tasks.b]\ndepends = [\"c\"]\n[tasks.c]\ndepends = [\"a\", \"d\"]\n[tasks.d]\n";
        let cycles: Vec<_> = problems(text);
        assert_eq!(
            cycles,
            [
                (
                    "task-cycle",
                    "dependency cycle: a -> b -> c -> a".to_string()
                ),
                (
                    "task-cycle",
                    "dependency cycle: b -> c -> a -> b".to_string()
                ),
                (
                    "task-cycle",
                    "dependency cycle: c -> a -> b -> c".to_string()
                ),
            ]
        );
    }

    #[test]
    fn a_task_that_depends_on_itself_is_not_a_cycle() {
        let text = "[tasks.a]\ndepends = [\"a\"]\n[tasks.b]\nalias = \"bb\"\ndepends = [\"bb\"]\n";
        assert!(problems(text).is_empty());
    }

    #[test]
    fn a_chain_without_a_loop_is_fine() {
        let text = "[tasks.a]\ndepends = [\"b\"]\n[tasks.b]\ndepends = [\"c\"]\n[tasks.c]\n";
        assert!(problems(text).is_empty());
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
