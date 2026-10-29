//! Checks `schema/mise.json` against the parser that actually reads mise.toml.
//!
//! Every document in the corpus is parsed with [`MiseToml`] (without the trust
//! check) and validated against the schema, and the two must agree: the schema
//! has to accept every config the parser reads cleanly, and reject every config
//! the parser refuses or reads with unknown fields. Known, deliberate
//! differences are listed in [`GAPS`].
//!
//! The corpus is:
//!
//! - every fenced `toml` block in `docs/**/*.md`, except blocks whose
//!   VitePress title names a file that is not a mise config (such as
//!   ```` ```toml [mise.lock] ````),
//! - every mise config an e2e test writes with a heredoc (`cat >mise.toml
//!   <<'EOF'`; unquoted heredocs only when the shell has nothing to expand),
//! - the configs in [`INVALID`].
//!
//! A marker comment on the nearest non-blank line above a block
//! (`<!-- schema: … -->` in markdown) or a heredoc (`# schema: …` in an e2e
//! script) changes what is expected of it:
//!
//! - `schema: skip` leaves it out: it is an excerpt, or a TOML file that is
//!   not a mise config.
//! - `schema: invalid` says mise refuses it, or reads it only with a warning
//!   or deprecation, possibly after parsing (when the config is used). The
//!   schema must reject it; what the parser does is not checked.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

use super::MiseToml;

/// Known disagreements between the schema and the parser, each with the
/// reason it is allowed. An entry that no document needs fails the test, so
/// the list shrinks as gaps are closed.
const GAPS: &[Gap] = &[
    Gap::SchemaStricter {
        at: "settings/ci",
        reason: "`ci` mirrors the CI variable, so it also takes strings such as \"true\" or \"1\"; \
                 a config should write a boolean",
    },
    Gap::SchemaLooser {
        at: "doctor/*",
        reason: "unknown [doctor] containers are forward compatible: the schema accepts them \
                 (see e2e/config/test_schema_ajv) and mise only warns",
    },
    Gap::ParserOnly {
        error: "{{ secrets.",
        reason: "where `{{ secrets.* }}` may appear is checked on the decoded document; a schema \
                 cannot see inside template strings",
    },
];

/// Configs that both the parser and the schema must reject.
const INVALID: &[(&str, &str)] = &[
    ("unknown root key", "not_a_section = true\n"),
    ("unknown setting", "[settings]\nnot_a_setting = true\n"),
    ("setting of the wrong type", "[settings]\njobs = \"many\"\n"),
    (
        "unknown task field",
        "[tasks.build]\nrun = \"cargo build\"\nnot_a_field = 1\n",
    ),
    ("task run is not a number", "[tasks.build]\nrun = 1\n"),
    ("env_file is not a number", "env_file = 1\n"),
    ("monorepo_root is a boolean", "monorepo_root = \"yes\"\n"),
    ("unknown hook", "[hooks]\nnot_a_hook = \"echo\"\n"),
];

enum Gap {
    /// The schema rejects values the parser reads cleanly. `at` is matched
    /// against the location of each schema error.
    SchemaStricter {
        at: &'static str,
        reason: &'static str,
    },
    /// The schema accepts fields the parser ignores with a warning. `at` is
    /// matched against each ignored field.
    SchemaLooser {
        at: &'static str,
        reason: &'static str,
    },
    /// The parser rejects configs with an error containing `error`, which
    /// the schema accepts.
    ParserOnly {
        error: &'static str,
        reason: &'static str,
    },
}

impl Gap {
    fn describe(&self) -> String {
        match self {
            Gap::SchemaStricter { at, reason } => format!("SchemaStricter at {at:?} ({reason})"),
            Gap::SchemaLooser { at, reason } => format!("SchemaLooser at {at:?} ({reason})"),
            Gap::ParserOnly { error, reason } => format!("ParserOnly {error:?} ({reason})"),
        }
    }
}

#[test]
fn schema_matches_parser() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    // the corpus lives in the repository, not in the published crate
    if !root.join("docs").is_dir() || !root.join("e2e").is_dir() {
        eprintln!("skipping: docs/ and e2e/ are only in a checkout of the repository");
        return;
    }
    let schema = std::fs::read_to_string(root.join("schema/mise.json")).unwrap();
    let schema: serde_json::Value = serde_json::from_str(&schema).unwrap();
    let validator = jsonschema::draft202012::new(&schema).unwrap();

    let mut corpus = docs_corpus(root);
    let docs = corpus.len();
    corpus.extend(e2e_corpus(root));
    let e2e = corpus.len() - docs;
    // guards against the extraction silently finding nothing
    assert!(docs > 300, "found only {docs} configs in docs/");
    assert!(e2e > 1000, "found only {e2e} configs in e2e/");
    corpus.extend(INVALID.iter().map(|(name, body)| Document {
        origin: format!("INVALID {name:?}"),
        body: body.to_string(),
        expect: Expect::BothReject,
    }));

    let mut failures = String::new();
    let mut used = BTreeSet::new();
    for (doc, outcome) in corpus.iter().zip(check_all(&validator, &corpus)) {
        let Some(outcome) = outcome else {
            if doc.expect != Expect::Agree {
                writeln!(
                    failures,
                    "{}: expected to be rejected but is not TOML",
                    doc.origin
                )
                .unwrap();
            }
            continue;
        };
        let schema_accepts = outcome.schema_errors.is_empty();
        let problem = match doc.expect {
            Expect::Agree => outcome.disagreement(&mut used),
            Expect::Invalid | Expect::BothReject if schema_accepts => {
                Some("expected to be rejected, but the schema accepts it".to_string())
            }
            Expect::BothReject if outcome.parser_clean() => {
                Some("expected to be rejected, but the parser reads it cleanly".to_string())
            }
            Expect::Invalid | Expect::BothReject => None,
        };
        if let Some(problem) = problem {
            writeln!(failures, "{}: {problem}", doc.origin).unwrap();
        }
    }
    for (i, gap) in GAPS.iter().enumerate() {
        if !used.contains(&i) {
            let gap = gap.describe();
            writeln!(failures, "GAPS: nothing needs {gap} any more; remove it").unwrap();
        }
    }
    assert!(
        failures.is_empty(),
        "schema/mise.json and the mise.toml parser disagree. Fix the schema, mark a config \
         that mise refuses with `schema: invalid`, or list a deliberate gap in GAPS \
         ({}):\n{failures}",
        file!(),
    );
}

struct Document {
    origin: String,
    body: String,
    expect: Expect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Expect {
    /// The schema and the parser agree on it.
    Agree,
    /// The schema rejects it.
    Invalid,
    /// The schema rejects it, and the parser fails or warns.
    BothReject,
}

#[derive(Debug)]
struct Outcome {
    parser_error: Option<String>,
    unknown: Vec<String>,
    /// (location, message)
    schema_errors: Vec<(String, String)>,
}

impl Outcome {
    fn parser_clean(&self) -> bool {
        self.parser_error.is_none() && self.unknown.is_empty()
    }

    /// How the parser and the schema disagree, unless they agree or [`GAPS`]
    /// covers every difference. Records which gaps it used.
    fn disagreement(&self, used: &mut BTreeSet<usize>) -> Option<String> {
        let schema_valid = self.schema_errors.is_empty();
        if let Some(error) = &self.parser_error {
            if !schema_valid {
                return None;
            }
            let gap = GAPS.iter().position(
                |gap| matches!(gap, Gap::ParserOnly { error: e, .. } if error.contains(e)),
            );
            if let Some(gap) = gap {
                used.insert(gap);
                return None;
            }
            return Some(format!(
                "the schema accepts a config the parser rejects: {error}"
            ));
        }
        if !self.unknown.is_empty() {
            if !schema_valid {
                return None;
            }
            let gaps = covering_gaps(
                self.unknown.iter().map(|field| field.replace('.', "/")),
                |gap| match gap {
                    Gap::SchemaLooser { at, .. } => Some(*at),
                    _ => None,
                },
            );
            if let Some(gaps) = gaps {
                used.extend(gaps);
                return None;
            }
            return Some(format!(
                "the schema accepts fields the parser ignores: {}",
                self.unknown.join(", ")
            ));
        }
        if schema_valid {
            return None;
        }
        let locations = self.schema_errors.iter().map(|(at, _)| at.clone());
        let gaps = covering_gaps(locations, |gap| match gap {
            Gap::SchemaStricter { at, .. } => Some(*at),
            _ => None,
        });
        if let Some(gaps) = gaps {
            used.extend(gaps);
            return None;
        }
        let errors = self
            .schema_errors
            .iter()
            .map(|(at, message)| format!("{at}: {message}"))
            .collect::<Vec<_>>()
            .join("; ");
        Some(format!(
            "the schema rejects a config the parser reads cleanly: {errors}"
        ))
    }
}

/// For each location, the first [`GAPS`] entry whose pattern (picked by
/// `pattern`) matches it, or `None` when any location is not covered.
fn covering_gaps(
    locations: impl Iterator<Item = String>,
    pattern: impl Fn(&Gap) -> Option<&'static str>,
) -> Option<Vec<usize>> {
    locations
        .map(|location| {
            let location = location.trim_start_matches('/');
            GAPS.iter()
                .position(|gap| pattern(gap).is_some_and(|p| path_matches(p, location)))
        })
        .collect()
}

/// Match a `/`-separated location against a pattern where `*` stands for
/// one segment.
fn path_matches(pattern: &str, location: &str) -> bool {
    let pattern: Vec<_> = pattern.split('/').collect();
    let location: Vec<_> = location.split('/').collect();
    pattern.len() == location.len()
        && pattern
            .iter()
            .zip(&location)
            .all(|(p, l)| *p == "*" || p == l)
}

/// [`check`] every document, spread over the available cores: one at a time,
/// the few thousand configs take about a second in a debug build.
fn check_all(validator: &jsonschema::Validator, corpus: &[Document]) -> Vec<Option<Outcome>> {
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
    let chunk = corpus.len().div_ceil(threads).max(1);
    std::thread::scope(|scope| {
        let workers: Vec<_> = corpus
            .chunks(chunk)
            .map(|docs| {
                std::thread::Builder::new()
                    // deserializing a config nests deeply in a debug build
                    .stack_size(8 << 20)
                    .spawn_scoped(scope, move || {
                        docs.iter()
                            .map(|doc| check(validator, &doc.body))
                            .collect::<Vec<_>>()
                    })
                    .unwrap()
            })
            .collect();
        workers
            .into_iter()
            .flat_map(|worker| worker.join().unwrap())
            .collect()
    })
}

/// Parse and validate one document. `None` when it is not TOML at all, which
/// neither side can read.
fn check(validator: &jsonschema::Validator, body: &str) -> Option<Outcome> {
    let table: toml::Table = toml::from_str(body).ok()?;
    let instance = toml_to_json(toml::Value::Table(table));
    let schema_errors = validator
        .iter_errors(&instance)
        .map(|e| (e.instance_path().to_string(), e.to_string()))
        .collect();
    let mut unknown = vec![];
    // never read: the path only places the config for templates and roots
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("mise.toml");
    let parser_error = MiseToml::parse_body_reporting(body, &path, false, &mut |field| {
        unknown.push(field.to_string());
    })
    .err()
    .map(|e| format!("{e:#}"));
    Some(Outcome {
        parser_error,
        unknown,
        schema_errors,
    })
}

/// Convert TOML to the JSON an editor validates, where datetimes are strings.
fn toml_to_json(value: toml::Value) -> serde_json::Value {
    use serde_json::Value as Json;
    match value {
        toml::Value::String(s) => Json::String(s),
        toml::Value::Integer(i) => Json::from(i),
        toml::Value::Float(f) => Json::from(f),
        toml::Value::Boolean(b) => Json::Bool(b),
        toml::Value::Datetime(d) => Json::String(d.to_string()),
        toml::Value::Array(a) => Json::Array(a.into_iter().map(toml_to_json).collect()),
        toml::Value::Table(t) => {
            Json::Object(t.into_iter().map(|(k, v)| (k, toml_to_json(v))).collect())
        }
    }
}

/// What is expected of the block or heredoc that starts at `lines[start]`,
/// from the marker on the nearest non-blank line above it, or `None` to skip
/// it.
fn expectation(lines: &[&str], start: usize) -> Option<Expect> {
    static MARKER: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^\s*(?:#|<!--)\s*schema:\s*(skip|invalid)\b").unwrap());
    let above = lines[..start].iter().rev().find(|l| !l.trim().is_empty());
    match above.and_then(|line| MARKER.captures(line)) {
        None => Some(Expect::Agree),
        Some(marker) if &marker[1] == "invalid" => Some(Expect::Invalid),
        Some(_) => None,
    }
}

fn docs_corpus(root: &Path) -> Vec<Document> {
    let mut corpus = vec![];
    let walk = walkdir::WalkDir::new(root.join("docs"))
        .sort_by_file_name()
        .into_iter()
        .filter_entry(|e| e.file_name() != "node_modules" && e.file_name() != ".vitepress");
    for entry in walk {
        let entry = entry.unwrap();
        if entry.path().extension().is_none_or(|ext| ext != "md") {
            continue;
        }
        let markdown = std::fs::read_to_string(entry.path()).unwrap();
        let rel = entry.path().strip_prefix(root).unwrap().display();
        let lines: Vec<&str> = markdown.lines().collect();
        for block in fenced_blocks(&lines) {
            if !info_is_config(block.info) {
                continue;
            }
            let Some(expect) = expectation(&lines, block.line) else {
                continue;
            };
            corpus.push(Document {
                origin: format!("{rel}:{}", block.line + 1),
                body: block.body,
                expect,
            });
        }
    }
    corpus
}

struct Block<'a> {
    /// 0-based line of the opening fence
    line: usize,
    info: &'a str,
    /// with the fence's indentation removed
    body: String,
}

fn fenced_blocks<'a>(lines: &[&'a str]) -> Vec<Block<'a>> {
    let mut blocks = vec![];
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        let fence = trimmed.len() - trimmed.trim_start_matches('`').len();
        let start = i;
        i += 1;
        if fence < 3 {
            continue;
        }
        let mut body = String::new();
        while i < lines.len() {
            let content = lines[i].trim_start();
            i += 1;
            if content.len() >= fence && content.trim_end().chars().all(|c| c == '`') {
                break;
            }
            body.push_str(lines[i - 1].get(indent..).unwrap_or(content));
            body.push('\n');
        }
        blocks.push(Block {
            line: start,
            info: trimmed[fence..].trim(),
            body,
        });
    }
    blocks
}

/// Whether a fenced block with this info string shows a mise config: a `toml`
/// block with no title, a title that is a label (` ```toml [Inline] `), or a
/// title naming a mise config file.
fn info_is_config(info: &str) -> bool {
    if info.split_whitespace().next() != Some("toml") {
        return false;
    }
    let title = info
        .split_once('[')
        .and_then(|(_, rest)| rest.split_once(']'))
        .map(|(title, _)| title);
    match title {
        Some(title) if title.contains(['.', '/']) => path_is_config(title),
        _ => true,
    }
}

/// Whether a file at this path is read as a mise config.
fn path_is_config(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    let Some(stem) = name.strip_suffix(".toml") else {
        return false;
    };
    let stem = stem.strip_prefix('.').unwrap_or(stem);
    if stem == "mise" || stem.starts_with("mise.") {
        return stem != "mise.plugin";
    }
    let in_config_dir = path.contains("mise/") || path.contains("MISE_CONFIG_DIR");
    in_config_dir && (stem == "config" || stem.starts_with("config.") || path.contains("conf.d/"))
}

fn e2e_corpus(root: &Path) -> Vec<Document> {
    static HEREDOC: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r#"<<(-?)\s*(['"]?)([A-Za-z_][A-Za-z0-9_]*)['"]?"#).unwrap());
    let mut corpus = vec![];
    for entry in walkdir::WalkDir::new(root.join("e2e")).sort_by_file_name() {
        let entry = entry.unwrap();
        if !entry.file_type().is_file() {
            continue;
        }
        let Ok(script) = std::fs::read_to_string(entry.path()) else {
            continue;
        };
        let rel = entry.path().strip_prefix(root).unwrap().display();
        let lines: Vec<&str> = script.lines().collect();
        let mut i = 0;
        while i < lines.len() {
            let line = lines[i];
            let start = i;
            i += 1;
            let Some(heredoc) = HEREDOC.captures(line) else {
                continue;
            };
            let strip_tabs = &heredoc[1] == "-";
            let quoted = !heredoc[2].is_empty();
            let delimiter = &heredoc[3];
            let mut body = String::new();
            while i < lines.len() {
                let mut l = lines[i];
                i += 1;
                if strip_tabs {
                    l = l.trim_start_matches('\t');
                }
                if l == delimiter {
                    break;
                }
                body.push_str(l);
                body.push('\n');
            }
            let Some(target) = redirect_target(line) else {
                continue;
            };
            if !path_is_config(target) {
                continue;
            }
            // the shell expands an unquoted heredoc, so its text is the file
            // only when there is nothing to expand
            if !quoted && body.contains(['$', '`', '\\']) {
                continue;
            }
            let Some(expect) = expectation(&lines, start) else {
                continue;
            };
            corpus.push(Document {
                origin: format!("{rel}:{}", start + 1),
                body,
                expect,
            });
        }
    }
    corpus
}

/// The file a shell line writes with `> file`, unquoted or in single or
/// double quotes. Appends (`>> file`) and other descriptors (`2> file`) are
/// not a whole file.
fn redirect_target(line: &str) -> Option<&str> {
    static TARGET: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"(?:^|[\s;(])>\s*(?:"([^"]+)"|'([^']+)'|([^\s"';&|<>]+))"#).unwrap()
    });
    let captures = TARGET.captures(line)?;
    (1..=3).find_map(|i| captures.get(i)).map(|m| m.as_str())
}

#[test]
fn redirect_targets() {
    for (line, target) in [
        ("cat >mise.toml <<'EOF'", Some("mise.toml")),
        ("cat <<EOF >mise.toml", Some("mise.toml")),
        ("cat > mise.toml <<EOF", Some("mise.toml")),
        (
            r#"cat >"$MISE_CONFIG_DIR/config.toml" <<'TOML'"#,
            Some("$MISE_CONFIG_DIR/config.toml"),
        ),
        (
            "cat >'.mise/conf.d/30-literal.qa*.toml' <<'EOF'",
            Some(".mise/conf.d/30-literal.qa*.toml"),
        ),
        ("cat >>mise.toml <<'EOF'", None),
        ("cat 2>err.log <<'EOF'", None),
        ("cat <<'EOF'", None),
    ] {
        assert_eq!(redirect_target(line), target, "{line}");
    }
}
