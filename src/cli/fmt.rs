use crate::Result;
use crate::config::{ALL_TOML_CONFIG_FILES, TASK_INPUT_GROUP_PREFIX, is_glob_pattern};
use crate::{config, dirs, file};
use eyre::bail;
use std::io::{self, Read, Write};
use taplo::formatter::Options;
use toml_edit::{Array, DocumentMut, RawString, TableLike, Value};

/// Format mise TOML configuration
///
/// Sorts keys and normalizes whitespace using TOML 1.1 syntax, including multiline
/// inline tables. Lists whose order carries no meaning are sorted as well: task
/// `sources` and `outputs`, `task_config.global_inputs` and `input_groups`, and
/// `redactions`. File pattern lists sort by reach — `@group:` references, then
/// globs, then literal paths — and a list is left as written when an entry
/// excludes with `!` or carries a comment. By default, formats config files in
/// the current directory; `--all` includes every loaded config. Use `--check`
/// in CI or `--stdin` to format a supplied document without rewriting a file.
#[derive(Debug, usage_rs::Args)]
#[usage(
    verbatim_doc_comment,
    example(
        r###"mise fmt
mise fmt --check
cat mise.toml | mise fmt --stdin"###
    )
)]
pub(crate) struct Fmt {
    /// Format every config file mise currently loads, not just those in the current directory
    #[usage(short, long)]
    pub all: bool,

    /// Check whether the configs are formatted without rewriting them; exits 1 if any are not
    #[usage(short, long)]
    pub check: bool,

    /// Read config from stdin and write its formatted version into
    /// stdout
    #[usage(short, long)]
    pub stdin: bool,
}

impl Fmt {
    pub(crate) fn run(self) -> eyre::Result<()> {
        if self.stdin {
            let mut toml = String::new();
            io::stdin().read_to_string(&mut toml)?;

            let toml = sort(toml)?;
            let toml = format(toml)?;
            let mut stdout = io::stdout();
            write!(stdout, "{toml}")?;

            return Ok(());
        }

        let cwd = dirs::CWD.clone().unwrap_or_default();
        let configs = if self.all {
            ALL_TOML_CONFIG_FILES.clone()
        } else {
            config::config_files_in_dir(&cwd)
        };
        if configs.is_empty() {
            bail!("No config file found in current directory");
        }
        let mut errors = Vec::new();
        for p in configs {
            if !p
                .file_name()
                .is_some_and(|f| f.to_string_lossy().ends_with("toml"))
            {
                continue;
            }
            let source = file::read_to_string(&p)?;
            let toml = source.clone();
            let toml = sort(toml)?;
            let toml = format(toml)?;
            if self.check {
                if source != toml {
                    errors.push(p.display().to_string());
                }
                continue;
            }
            file::write(&p, &toml)?;
        }

        if !errors.is_empty() {
            bail!(
                "Following config files are not properly formatted:\n{}",
                errors.join("\n")
            );
        }

        Ok(())
    }
}

/// Put the document's top-level keys and its unordered lists into a canonical
/// order. Whitespace and layout are left to [`format`].
fn sort(toml: String) -> Result<String> {
    let mut doc: DocumentMut = toml.parse()?;
    let order = |k: String| match k.as_str() {
        "min_version" => 0,
        "env_file" => 1,
        "env_path" => 2,
        "_" => 3,
        "env" => 4,
        "vars" => 5,
        "hooks" => 6,
        "watch_files" => 7,
        "tools" => 8,
        "tasks" => 10,
        "task_config" => 11,
        "redactions" => 12,
        "alias" => 13,
        "plugins" => 14,
        "settings" => 15,
        _ => 9,
    };
    doc.sort_values_by(|a, _, b, _| order(a.to_string()).cmp(&order(b.to_string())));
    sort_unordered_sets(&mut doc);
    Ok(doc.to_string())
}

/// Arrays whose entries form a set: reordering them cannot change what mise
/// does, so `mise fmt` can put them in a canonical order.
///
/// Paths are matched key by key from the document root, and `*` matches any
/// single key. Order-sensitive lists are deliberately absent: `env_file`,
/// `env_path`, and the `_.path`/`_.source` directives establish precedence,
/// `tools.*` treats the first version as the primary one,
/// `task_config.includes` fixes task discovery order, and
/// `depends`/`depends_post`/`wait_for` are often grouped by hand.
///
/// `mise fmt` only ever sees whole config files, so tasks are always nested
/// under `tasks`; a standalone task TOML file listed in
/// `task_config.includes` is not formatted and needs no path of its own.
const UNORDERED_SETS: &[(&[&str], SetOrder)] = &[
    (&["redactions"], SetOrder::Lexical),
    (&["task_config", "global_inputs"], SetOrder::FilePatterns),
    (
        &["task_config", "input_groups", "*"],
        SetOrder::FilePatterns,
    ),
    (&["tasks", "*", "sources"], SetOrder::FilePatterns),
    (&["tasks", "*", "outputs"], SetOrder::FilePatterns),
    (&["task_templates", "*", "sources"], SetOrder::FilePatterns),
    (&["task_templates", "*", "outputs"], SetOrder::FilePatterns),
];

#[derive(Clone, Copy)]
enum SetOrder {
    /// Sort entries lexically.
    Lexical,
    /// Sort by how broadly an entry reaches — `@group:` references, then
    /// globs, then the literal paths those globs broaden — and lexically
    /// within each rank. Reading a list top to bottom then narrows from the
    /// inputs a task inherits down to the individual files it names.
    FilePatterns,
}

/// Put every array named in [`UNORDERED_SETS`] into its canonical order.
fn sort_unordered_sets(doc: &mut DocumentMut) {
    for (path, order) in UNORDERED_SETS {
        visit_arrays(doc.as_table_mut(), path, &mut |array| {
            sort_set(array, *order);
        });
    }
}

/// Call `visit` on every array the document holds at `path`, where a `*`
/// segment matches any key. Tables and inline tables are both walked, so a
/// task written as `[tasks.build]`, as `build = { .. }`, or with dotted keys
/// is reached the same way.
fn visit_arrays(table: &mut dyn TableLike, path: &[&str], visit: &mut impl FnMut(&mut Array)) {
    let Some((segment, rest)) = path.split_first() else {
        return;
    };
    for (key, item) in table.iter_mut() {
        if *segment != "*" && key.get() != *segment {
            continue;
        }
        if rest.is_empty() {
            if let Some(array) = item.as_array_mut() {
                visit(array);
            }
        } else if let Some(child) = item.as_table_like_mut() {
            visit_arrays(child, rest, visit);
        }
    }
}

/// Sort one array, leaving it as written when reordering could change what
/// it means.
fn sort_set(array: &mut Array, order: SetOrder) {
    if !is_sortable(array, order) {
        return;
    }
    array.sort_by(|a, b| sort_key(a, order).cmp(&sort_key(b, order)));
}

/// Where `value` sorts: its rank first, then the entry itself to break ties
/// within a rank.
fn sort_key(value: &Value, order: SetOrder) -> (u8, &str) {
    let entry = value.as_str().unwrap_or_default();
    match order {
        SetOrder::Lexical => (0, entry),
        SetOrder::FilePatterns => (pattern_rank(entry), entry),
    }
}

/// How broadly a file pattern reaches, lowest first.
fn pattern_rank(entry: &str) -> u8 {
    if entry.starts_with(TASK_INPUT_GROUP_PREFIX) {
        0
    } else if is_glob_pattern(entry) {
        1
    } else {
        2
    }
}

/// Whether reordering `array` is guaranteed to preserve its meaning.
///
/// Entries that are not plain strings are left alone because their order may
/// carry meaning this function cannot see. A comment blocks the sort for the
/// same reason: once entries move there is no way to tell which one it was
/// written about. A comment that follows the last entry lives in the array's
/// trailing decor rather than on a value, so it is checked separately.
fn is_sortable(array: &Array, order: SetOrder) -> bool {
    if has_comment(Some(array.trailing())) {
        return false;
    }
    array.iter().all(|value| {
        value.as_str().is_some_and(|entry| match order {
            SetOrder::Lexical => true,
            SetOrder::FilePatterns => !may_exclude(entry),
        }) && !has_comment(value.decor().prefix())
            && !has_comment(value.decor().suffix())
    })
}

/// Whether `entry` excludes the files it matches, which makes the position of
/// every entry around it meaningful: `sources` and `outputs` are evaluated in
/// order and the last matching entry wins.
///
/// `\!` escapes a literal `!` and so does not exclude, but a template can
/// render into either and is treated as if it excludes.
fn may_exclude(entry: &str) -> bool {
    entry.starts_with('!') || entry.starts_with("{{") || entry.starts_with("{%")
}

/// Whether a span of decor holds a comment rather than only whitespace.
fn has_comment(raw: Option<&RawString>) -> bool {
    raw.and_then(RawString::as_str)
        .is_some_and(|decor| decor.contains('#'))
}

fn format(toml: String) -> Result<String> {
    let tmp = taplo::formatter::format(
        &toml,
        Options {
            align_comments: true,
            align_entries: false,
            align_single_comments: true,
            allowed_blank_lines: 2,
            array_auto_collapse: true,
            array_auto_expand: true,
            array_trailing_comma: true,
            column_width: 80,
            compact_arrays: true,
            compact_entries: false,
            compact_inline_tables: false,
            crlf: false,
            indent_entries: false,
            indent_string: "  ".to_string(),
            indent_tables: false,
            inline_table_expand: true,
            reorder_arrays: false,
            reorder_keys: false,
            reorder_inline_tables: false,
            trailing_newline: true,
        },
    );

    Ok(tmp)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run the same pipeline `mise fmt` writes back to a config file.
    fn fmt(toml: &str) -> String {
        format(sort(toml.to_string()).unwrap()).unwrap()
    }

    #[test]
    fn sorts_groups_then_globs_then_literal_paths() {
        let toml = fmt(r#"
[tasks.build]
sources = ["Cargo.toml", "src/**/*.rs", "@group:rust", "@group:lock"]
"#);

        assert!(
            toml.contains(
                r#"sources = ["@group:lock", "@group:rust", "src/**/*.rs", "Cargo.toml"]"#
            ),
            "{toml}"
        );
    }

    #[test]
    fn ranks_every_glob_metacharacter_ahead_of_a_literal_path() {
        let toml = fmt(r#"
[tasks.build]
sources = ["a.rs", "b?.rs", "c[0-9].rs", "d{x,y}.rs", "e*.rs"]
"#);

        assert!(
            toml.contains(r#""b?.rs", "c[0-9].rs", "d{x,y}.rs", "e*.rs", "a.rs""#),
            "{toml}"
        );
    }

    #[test]
    fn sorts_the_task_input_sets_outside_tasks() {
        let toml = fmt(r#"
redactions = ["TOKEN", "API_KEY"]

[task_config]
global_inputs = ["mise.toml", "@group:lockfiles", ".tool-versions"]

[task_config.input_groups]
lockfiles = ["pnpm-lock.yaml", "Cargo.lock"]
"#);

        assert!(
            toml.contains(r#"redactions = ["API_KEY", "TOKEN"]"#),
            "{toml}"
        );
        assert!(
            toml.contains(r#"global_inputs = ["@group:lockfiles", ".tool-versions", "mise.toml"]"#),
            "{toml}"
        );
        assert!(
            toml.contains(r#"lockfiles = ["Cargo.lock", "pnpm-lock.yaml"]"#),
            "{toml}"
        );
    }

    #[test]
    fn sorts_a_task_written_as_an_inline_table() {
        let toml = fmt(r#"
[tasks]
build = { run = "cargo build", sources = ["src/b.rs", "src/a.rs"] }
"#);

        assert!(
            toml.contains(r#"sources = ["src/a.rs", "src/b.rs"]"#),
            "{toml}"
        );
    }

    #[test]
    fn sorts_a_task_template() {
        let toml = fmt(r#"
[task_templates.rust]
sources = ["Cargo.toml", "src/**/*.rs"]
outputs = ["target/debug/b", "target/debug/a"]
"#);

        assert!(
            toml.contains(r#"sources = ["src/**/*.rs", "Cargo.toml"]"#),
            "{toml}"
        );
        assert!(
            toml.contains(r#"outputs = ["target/debug/a", "target/debug/b"]"#),
            "{toml}"
        );
    }

    #[test]
    fn sorts_sources_that_escape_a_literal_exclamation_mark() {
        let toml = fmt(r#"
[tasks.build]
sources = ["src/b.rs", "\\!important.txt"]
"#);

        assert!(
            toml.contains(r#"sources = ["\\!important.txt", "src/b.rs"]"#),
            "{toml}"
        );
    }

    #[test]
    fn formatting_a_sorted_set_again_changes_nothing() {
        let once = fmt(r#"
redactions = ["TOKEN", "API_KEY"]

[task_config]
global_inputs = ["mise.toml", "@group:lockfiles"]

[task_config.input_groups]
lockfiles = ["pnpm-lock.yaml", "Cargo.lock"]

[tasks.build]
outputs = ["dist/bundle.js", "dist/bundle.css"]
sources = ["src/b.ts", "@group:lockfiles", "src/**/*.ts", "src/a.ts"]
"#);

        assert_eq!(fmt(&once), once);
    }

    #[test]
    fn keeps_the_order_of_sources_that_exclude() {
        let toml = fmt(r#"
[tasks.build]
sources = ["src/**/*.ts", "!src/**/*.test.ts", "src/keep.test.ts"]
"#);

        assert!(
            toml.contains(r#"sources = ["src/**/*.ts", "!src/**/*.test.ts", "src/keep.test.ts"]"#),
            "{toml}"
        );
    }

    #[test]
    fn keeps_the_order_of_outputs_that_exclude() {
        let toml = fmt(r#"
[tasks.build]
outputs = ["dist", "!dist/**/*.map"]
"#);

        assert!(
            toml.contains(r#"outputs = ["dist", "!dist/**/*.map"]"#),
            "{toml}"
        );
    }

    #[test]
    fn keeps_the_order_of_sources_that_start_with_a_template() {
        let toml = fmt(r#"
[tasks.build]
sources = ["{{ arg(name = 'src') }}", "Cargo.toml"]
"#);

        assert!(
            toml.contains(r#"sources = ["{{ arg(name = 'src') }}", "Cargo.toml"]"#),
            "{toml}"
        );
    }

    #[test]
    fn keeps_the_order_of_a_set_that_carries_a_comment() {
        let toml = fmt(r#"
[tasks.build]
sources = [
  "src/b.rs", # this comment must not end up describing another entry
  "src/a.rs",
]
"#);

        let b = toml.find("src/b.rs").unwrap();
        let a = toml.find("src/a.rs").unwrap();
        assert!(b < a, "entries moved out from under a comment:\n{toml}");
    }

    #[test]
    fn keeps_the_order_of_a_set_whose_last_entry_carries_a_comment() {
        let toml = fmt(r#"
[tasks.build]
sources = [
  "src/b.rs",
  "src/a.rs", # this comment must not end up describing another entry
]
"#);

        let b = toml.find("src/b.rs").unwrap();
        let a = toml.find("src/a.rs").unwrap();
        assert!(b < a, "entries moved out from under a comment:\n{toml}");
    }

    #[test]
    fn keeps_the_order_of_a_set_followed_by_a_dangling_comment() {
        let toml = fmt(r#"
[tasks.build]
sources = [
  "src/b.rs",
  "src/a.rs",
  # a note about the list, or about the entry above it
]
"#);

        let b = toml.find("src/b.rs").unwrap();
        let a = toml.find("src/a.rs").unwrap();
        assert!(b < a, "entries moved out from under a comment:\n{toml}");
    }

    #[test]
    fn keeps_the_order_of_lists_whose_order_has_meaning() {
        let toml = fmt(r#"
env_file = [".env.local", ".env"]

[env]
_.path = ["./node_modules/.bin", "./bin"]

[tools]
node = ["22", "20"]

[tasks.build]
depends = ["render", "build:deps"]
"#);

        assert!(
            toml.contains(r#"env_file = [".env.local", ".env"]"#),
            "{toml}"
        );
        assert!(
            toml.contains(r#"_.path = ["./node_modules/.bin", "./bin"]"#),
            "{toml}"
        );
        assert!(toml.contains(r#"node = ["22", "20"]"#), "{toml}");
        assert!(
            toml.contains(r#"depends = ["render", "build:deps"]"#),
            "{toml}"
        );
    }

    #[test]
    fn keeps_an_outputs_table_intact() {
        let toml = fmt(r#"
[tasks.build]
outputs = { auto = true }
sources = ["src/b.rs", "src/a.rs"]
"#);

        assert!(toml.contains("auto = true"), "{toml}");
        assert!(
            toml.contains(r#"sources = ["src/a.rs", "src/b.rs"]"#),
            "{toml}"
        );
    }

    #[test]
    fn sorts_the_sources_of_a_task_named_after_a_sorted_key() {
        let toml = fmt(r#"
[tasks.sources]
depends = ["b", "a"]
sources = ["src/b.rs", "src/a.rs"]
"#);

        assert!(toml.contains(r#"depends = ["b", "a"]"#), "{toml}");
        assert!(
            toml.contains(r#"sources = ["src/a.rs", "src/b.rs"]"#),
            "{toml}"
        );
    }
}
