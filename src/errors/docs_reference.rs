//! Checks that the messages `docs/errors.md` quotes are still in the source.
//!
//! The page is written by hand, so a message reworded in the source would
//! otherwise keep its old text there. This test only catches a quoted message
//! that no longer exists: it does not notice a message the page never listed.
//!
//! What counts as a quoted message:
//!
//! - every code span in a heading;
//! - a code span in the body, unless it is a single word (`mise.lock`,
//!   `--verbose`), a mise command line (`mise use <tool>@<version>`), or a
//!   config line (`node = "26"`).
//!
//! Fenced blocks are sample output with real paths and versions in it, so they
//! are not checked; the heading above each one carries the message.
//!
//! How a message is matched: it is split on its placeholders (`<tool>`) and on
//! numbers, which the source formats in. Each remaining piece that has a word
//! in it must occur in one source file, all within [`WINDOW`] bytes of the
//! longest piece. The order is not checked, because a format string can take
//! its arguments in another order than it prints them. The sources are
//! compared with their format holes (`{}`, `{name}`), backticks and string
//! continuations removed.
//!
//! A message that cannot be matched this way, because mise assembles it from
//! several strings or a dependency words it, gets a marker anywhere on the
//! page, with the reason:
//!
//! ```text
//! <!-- errors: unchecked `the message as the page quotes it`: reason -->
//! ```

use std::collections::BTreeSet;
use std::fmt::Write;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

/// How far from the longest piece of a message its other pieces may be.
const WINDOW: usize = 200;

/// The shortest longest-piece that still identifies a message.
const MIN_ANCHOR: usize = 7;

#[test]
fn quoted_messages_are_in_the_source() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    // the page lives in the repository, not in the published crate
    if !root.join("docs").is_dir() || !root.join("crates").is_dir() {
        eprintln!("skipping: docs/ and crates/ are only in a checkout of the repository");
        return;
    }
    let page = std::fs::read_to_string(root.join("docs/errors.md")).unwrap();
    let page = parse_page(&page).unwrap_or_else(|err| panic!("docs/errors.md: {err}"));
    let sources = sources(root);
    // guards against the extraction silently finding nothing
    assert!(
        sources.len() > 500,
        "found only {} source files",
        sources.len()
    );
    assert!(
        page.quotes.len() > 40,
        "found only {} quoted messages in docs/errors.md",
        page.quotes.len()
    );

    let mut failures = String::new();
    for quote in &page.quotes {
        if page.unchecked.contains(&quote.text) {
            continue;
        }
        let Some(pieces) = pieces(&quote.text) else {
            writeln!(
                failures,
                "docs/errors.md:{}: `{}` has too little fixed text to check",
                quote.line, quote.text
            )
            .unwrap();
            continue;
        };
        if !sources.iter().any(|source| occurs(source, &pieces)) {
            writeln!(
                failures,
                "docs/errors.md:{}: `{}` is not in the source (looked for {pieces:?})",
                quote.line, quote.text
            )
            .unwrap();
        }
    }
    for text in &page.unchecked {
        if !page.quotes.iter().any(|quote| &quote.text == text) {
            writeln!(
                failures,
                "docs/errors.md: `{text}` is marked `errors: unchecked` but the page does not quote it"
            )
            .unwrap();
        }
    }
    assert!(
        failures.is_empty(),
        "docs/errors.md quotes messages that mise no longer prints with that wording.\n\
         Update the page to the current message. If the message is assembled from \
         several strings or comes from a dependency, mark it on the page with\n\
         <!-- errors: unchecked `the message as quoted`: reason -->\n\n{failures}"
    );
}

struct Quote {
    /// 1-based line in the page
    line: usize,
    text: String,
}

struct Page {
    quotes: Vec<Quote>,
    /// the messages an `errors: unchecked` marker names
    unchecked: BTreeSet<String>,
}

fn parse_page(markdown: &str) -> Result<Page, String> {
    static MARKER: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^<!--\s*errors:\s*unchecked\s+`([^`]+)`(.*)-->$").unwrap());
    static SPAN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"`([^`]+)`").unwrap());
    let mut page = Page {
        quotes: vec![],
        unchecked: BTreeSet::new(),
    };
    let mut in_fence = false;
    for (i, line) in markdown.lines().enumerate() {
        let line_no = i + 1;
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        if trimmed.starts_with("<!--") {
            let Some(marker) = MARKER.captures(trimmed) else {
                if trimmed.contains("errors:") {
                    return Err(format!(
                        "line {line_no}: write the marker as <!-- errors: unchecked `message`: reason -->"
                    ));
                }
                continue;
            };
            if marker[2].trim_start_matches(':').trim().is_empty() {
                return Err(format!(
                    "line {line_no}: an `errors: unchecked` marker needs a reason"
                ));
            }
            page.unchecked.insert(marker[1].to_string());
            continue;
        }
        if line.matches('`').count() % 2 != 0 {
            return Err(format!(
                "line {line_no}: a code span continues on the next line, so it cannot be read; keep it on one line"
            ));
        }
        let heading = line.starts_with('#');
        for span in SPAN.captures_iter(line) {
            let text = span[1].trim();
            if heading || is_message(text) {
                page.quotes.push(Quote {
                    line: line_no,
                    text: text.to_string(),
                });
            }
        }
    }
    Ok(page)
}

/// Whether a code span in the body quotes a message, as opposed to a name, a
/// command line or a config line.
fn is_message(span: &str) -> bool {
    if !span.contains(char::is_whitespace) || span.contains(" = ") {
        return false;
    }
    let Some(args) = span.strip_prefix("mise ") else {
        return true;
    };
    // `mise --cd <dir> tasks ls` has two bare words after `mise`;
    // `mise version <X> is recommended, but you are using <Y>` is a sentence
    let bare_words = args
        .split_whitespace()
        .filter(|word| word.chars().all(|c| c.is_ascii_alphabetic() || c == '-'))
        .filter(|word| word.contains(|c: char| c.is_ascii_alphabetic()))
        .count();
    bare_words > 3
}

/// The fixed pieces of a quoted message, longest first, or `None` when the
/// longest is too short to identify it.
fn pieces(message: &str) -> Option<Vec<String>> {
    static VARIABLE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"<[^<>]+>|\b[0-9]+(?:\.[0-9]+)*\b").unwrap());
    static WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new("[A-Za-z]{3}").unwrap());
    let mut pieces: Vec<String> = VARIABLE
        .split(message)
        .map(str::trim)
        .filter(|piece| WORD.is_match(piece))
        .map(str::to_string)
        .collect();
    pieces.sort_by_key(|piece| std::cmp::Reverse(piece.len()));
    (pieces.first()?.len() >= MIN_ANCHOR).then_some(pieces)
}

/// Whether every piece occurs in `source` within [`WINDOW`] bytes of one
/// occurrence of the first, which is the longest.
fn occurs(source: &str, pieces: &[String]) -> bool {
    let anchor = &pieces[0];
    source.match_indices(anchor.as_str()).any(|(at, _)| {
        let mut start = at.saturating_sub(WINDOW);
        while !source.is_char_boundary(start) {
            start -= 1;
        }
        let mut end = (at + anchor.len() + WINDOW).min(source.len());
        while !source.is_char_boundary(end) {
            end += 1;
        }
        let window = &source[start..end];
        pieces[1..]
            .iter()
            .all(|piece| window.contains(piece.as_str()))
    })
}

/// Every Rust file of mise and its crates, as [`normalize`] leaves it.
fn sources(root: &Path) -> Vec<String> {
    let mut sources = vec![];
    for dir in ["src", "crates"] {
        let walk = walkdir::WalkDir::new(root.join(dir))
            .into_iter()
            // vendored code is not mise's to word
            .filter_entry(|e| e.file_name() != "embedded-plugins" && e.file_name() != "vendor");
        for entry in walk {
            let entry = entry.unwrap();
            let path = entry.path();
            // this file quotes messages of its own in its tests
            if path.extension().is_none_or(|ext| ext != "rs")
                || path.ends_with("errors/docs_reference.rs")
            {
                continue;
            }
            sources.push(normalize(&std::fs::read_to_string(path).unwrap()));
        }
    }
    sources
}

/// Rust source with what a string literal has and the printed message does
/// not taken out: `\` line continuations, the `\` of `\"`, backticks (the page
/// cannot nest them in a code span) and format holes.
fn normalize(source: &str) -> String {
    static CONTINUATION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\\\r?\n[ \t]*").unwrap());
    static HOLE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"\{[A-Za-z0-9_.]*(?::[^{}\s]*)?\}").unwrap());
    let source = CONTINUATION.replace_all(source, "");
    let source = source.replace("\\\"", "\"").replace('`', "");
    HOLE.replace_all(&source, "").into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(message: &str, source: &str) -> bool {
        occurs(&normalize(source), &pieces(message).unwrap())
    }

    #[test]
    fn matches_a_format_string() {
        let source = r#"bail!("{tool}@{version} is not in the lockfile\nhint: {hint}")"#;
        assert!(found("<tool>@<version> is not in the lockfile", source));
        assert!(!found(
            "<tool>@<version> is missing from the lockfile",
            source
        ));
    }

    #[test]
    fn matches_across_holes_backticks_and_continuations() {
        let source = "format!(\"Tool{} not installed for shim: {}\\n\", s, bin)";
        assert!(found("Tool not installed for shim: <command>", source));
        let source = "bail!(\"{what} is experimental. Enable it with `mise settings \\\n                experimental=true`\")";
        assert!(found(
            "Enable it with mise settings experimental=true",
            source
        ));
    }

    #[test]
    fn numbers_are_placeholders_and_order_is_free() {
        let source =
            r#"format!("PATH is {} characters, longer than the {} cmd.exe accepts", n, MAX)"#;
        assert!(found(
            "PATH is <N> characters, longer than the 8191 cmd.exe accepts",
            source
        ));
        let source = r#"eprintln!("{} {} task(s) failed:", style::ered("ERROR"), count)"#;
        assert!(found("ERROR <N> task(s) failed:", source));
    }

    #[test]
    fn pieces_must_be_near_each_other() {
        let far = format!(
            "\"no task {{}} found\"{}\"unrelated text\"",
            " ".repeat(WINDOW + 1)
        );
        assert!(found("no task <name> found", &far));
        assert!(!found("no task <name> found: unrelated text", &far));
    }

    #[test]
    fn too_little_fixed_text_is_not_checkable() {
        assert!(pieces("[<config file>] <tool>@<version>: <cause>").is_none());
        assert!(pieces("missing: <tool>@<version>").is_some());
    }

    #[test]
    fn reads_headings_messages_and_markers() {
        let page = parse_page(
            "### `<tool> not found` / `other` {#id}\n\
             Run `mise use <tool>@<version>` or set `node = \"26\"` in `mise.toml`.\n\
             It prints `mise version <X> is required, but you are using <Y>` or `task not found: <name>`.\n\
             <!-- errors: unchecked `other`: worded by a dependency -->\n\
             ```text\n`not read`\n```\n",
        )
        .unwrap();
        let quotes: Vec<&str> = page.quotes.iter().map(|q| q.text.as_str()).collect();
        assert_eq!(
            quotes,
            [
                "<tool> not found",
                "other",
                "mise version <X> is required, but you are using <Y>",
                "task not found: <name>"
            ]
        );
        assert!(page.unchecked.contains("other"));
        assert!(parse_page("<!-- errors: unchecked `other` -->").is_err());
        assert!(parse_page("a `span that\nwraps` here").is_err());
    }
}
