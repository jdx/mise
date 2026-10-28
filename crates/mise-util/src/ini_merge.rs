//! The INI side of [`structured_merge`](crate::structured_merge): set some keys
//! of an INI file and leave the rest alone.
//!
//! INI has no standard. These rules read the files Python's `configparser`
//! writes, such as `setup.cfg`, `tox.ini` and `pip.conf`:
//!
//! - `[name]` starts a section. Keys before the first section belong to none.
//! - `key = value` or `key: value` sets a key. The first `=` or `:` splits the
//!   line, and the whitespace around the key and the value is not part of
//!   them. A line with neither is a key with no value.
//! - A line indented further than the key above it continues that key's value.
//! - A line whose first non-blank character is `#` or `;` is a comment, and
//!   nothing else is: `a = b ; c` sets `a` to `b ; c`.
//! - Keys and section names are compared as written, case included.
//! - A key repeated in a section, or a section repeated in the file, has its
//!   last value, and a merge sets that one.
//!
//! Values are strings and sections do not nest, so a source section merges key
//! by key and every key's value replaces the target's. The target is edited
//! line by line: lines of keys the source does not set are never rewritten, so
//! comments, blank lines and spacing stay.

use eyre::{Result, eyre};
use indexmap::IndexMap;
use std::collections::HashMap;

/// A key and the lines it spans.
struct Entry {
    key: String,
    /// `None` for a key with no `=` or `:`
    value: Option<String>,
    /// the key line up to where its value starts, such as `key = `; `None`
    /// for a key with no value
    prefix: Option<String>,
    /// bytes of whitespace before the key
    indent: usize,
    /// the key line and its continuation lines
    start: usize,
    end: usize,
}

struct Section {
    /// `None` for the keys before the first section header
    name: Option<String>,
    header: Option<usize>,
    entries: Vec<Entry>,
}

struct Doc {
    /// without their line endings
    lines: Vec<String>,
    /// the keys before the first header come first, even when there are none
    sections: Vec<Section>,
}

/// Each section's keys with their last entries, sections in the order they
/// first appear. The keys before the first header are left out when there
/// are none.
type Effective<'a> = IndexMap<Option<&'a str>, IndexMap<&'a str, &'a Entry>>;

fn parse(text: &str, what: &str) -> Result<Doc> {
    let fail =
        |line: usize, msg: &str| eyre!("failed to parse the INI {what}: line {}: {msg}", line + 1);
    let lines: Vec<String> = text.lines().map(str::to_string).collect();
    let mut sections = vec![];
    let mut current = Section {
        name: None,
        header: None,
        entries: vec![],
    };
    // the indent of the key whose value the next line may continue
    let mut open: Option<usize> = None;
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        let indent = line.len() - line.trim_start().len();
        if trimmed.is_empty() {
            open = None;
            continue;
        }
        // as in configparser, a comment inside a value does not end it
        if trimmed.starts_with(['#', ';']) {
            continue;
        }
        if let Some(entry) = current.entries.last_mut()
            && open.is_some_and(|key_indent| indent > key_indent)
        {
            if let Some(value) = &mut entry.value {
                value.push('\n');
                value.push_str(trimmed);
            }
            entry.end = i + 1;
            continue;
        }
        if trimmed.starts_with('[') {
            let Some(close) = trimmed.rfind(']') else {
                return Err(fail(i, "a section header needs a closing ]"));
            };
            let name = trimmed[1..close].trim();
            if name.is_empty() {
                return Err(fail(i, "a section needs a name"));
            }
            sections.push(std::mem::replace(
                &mut current,
                Section {
                    name: Some(name.to_string()),
                    header: Some(i),
                    entries: vec![],
                },
            ));
            open = None;
            continue;
        }
        let rest = &line[indent..];
        let (key, value, prefix) = match rest.find(['=', ':']) {
            Some(delim) => {
                let after = &rest[delim + 1..];
                let value = after.trim();
                let space = after.len() - after.trim_start().len();
                let mut prefix = line[..indent + delim + 1 + space].to_string();
                // `key =` gets a space before a value mise writes
                if value.is_empty() && space == 0 {
                    prefix.push(' ');
                }
                (rest[..delim].trim(), Some(value.to_string()), Some(prefix))
            }
            None => (rest.trim(), None, None),
        };
        if key.is_empty() {
            return Err(fail(i, "a key needs a name"));
        }
        open = value.is_some().then_some(indent);
        current.entries.push(Entry {
            key: key.to_string(),
            value,
            prefix,
            indent,
            start: i,
            end: i + 1,
        });
    }
    sections.push(current);
    Ok(Doc { lines, sections })
}

impl Doc {
    fn effective(&self) -> Effective<'_> {
        let mut out = Effective::new();
        for section in &self.sections {
            if section.header.is_none() && section.entries.is_empty() {
                continue;
            }
            let keys = out.entry(section.name.as_deref()).or_default();
            for entry in &section.entries {
                keys.insert(&entry.key, entry);
            }
        }
        out
    }

    fn has_section(&self, name: Option<&str>) -> bool {
        name.is_none() || self.sections.iter().any(|s| s.name.as_deref() == name)
    }

    /// The entry that sets `key` in `section`: the last one.
    fn get(&self, section: Option<&str>, key: &str) -> Option<&Entry> {
        self.sections
            .iter()
            .rev()
            .filter(|s| s.name.as_deref() == section)
            .flat_map(|s| s.entries.iter().rev())
            .find(|e| e.key == key)
    }

    fn header_line(&self, name: &str) -> &str {
        self.sections
            .iter()
            .find(|s| s.name.as_deref() == Some(name))
            .and_then(|s| s.header)
            .map_or("", |h| &self.lines[h])
    }

    fn indent_of(&self, entry: &Entry) -> &str {
        &self.lines[entry.start][..entry.indent]
    }

    /// An entry's lines with its key moved to `indent`.
    fn entry_lines(&self, entry: &Entry, indent: &str) -> Vec<String> {
        self.lines[entry.start..entry.end]
            .iter()
            .map(|line| {
                // only whitespace comes off: a comment inside a value may sit
                // left of its key
                let space = line.len() - line.trim_start().len();
                let rest = line.get(space.min(entry.indent)..).unwrap_or(line);
                format!("{indent}{rest}")
            })
            .collect()
    }

    /// Where a new key of an existing section goes, and the indent of the
    /// keys already there: after the section's last key. A section with no
    /// keys gets it after its header; keys before the first section go above
    /// the comments that lead into it.
    fn insert_point(&self, name: Option<&str>) -> (usize, &str) {
        let section = self
            .sections
            .iter()
            .rev()
            .find(|s| s.name.as_deref() == name);
        match section.map(|s| (s.entries.last(), s.header)) {
            Some((Some(entry), _)) => (entry.end, self.indent_of(entry)),
            Some((None, Some(header))) => (header + 1, ""),
            _ => {
                let Some(mut at) = self.sections.iter().find_map(|s| s.header) else {
                    return (self.lines.len(), "");
                };
                while at > 0 && self.lines[at - 1].trim_start().starts_with(['#', ';']) {
                    at -= 1;
                }
                (at, "")
            }
        }
    }
}

fn parse_pair(target: &str, source: &str) -> Result<(Doc, Doc)> {
    Ok((parse(target, "target")?, parse(source, "source")?))
}

pub fn contains(target: &str, source: &str) -> Result<bool> {
    let (target, source) = parse_pair(target, source)?;
    Ok(source.effective().iter().all(|(name, keys)| {
        target.has_section(*name)
            && keys.iter().all(|(key, entry)| {
                target
                    .get(*name, key)
                    .is_some_and(|t| t.value == entry.value)
            })
    }))
}

pub fn missing(target: &str, source: &str) -> Result<Option<String>> {
    let (target, source) = parse_pair(target, source)?;
    let mut out: Vec<String> = vec![];
    for (name, keys) in source.effective() {
        let present = target.has_section(name);
        let absent: Vec<&Entry> = keys
            .values()
            .filter(|e| !present || target.get(name, &e.key).is_none())
            .copied()
            .collect();
        if present && absent.is_empty() {
            continue;
        }
        if let Some(name) = name {
            if !out.is_empty() {
                out.push(String::new());
            }
            out.push(source.header_line(name).to_string());
        }
        for entry in absent {
            out.extend(source.lines[entry.start..entry.end].iter().cloned());
        }
    }
    Ok((!out.is_empty()).then(|| out.join("\n") + "\n"))
}

pub fn conflicts(a: &str, b: &str) -> Result<Vec<String>> {
    let (a, b) = (parse(a, "source")?, parse(b, "source")?);
    let b = b.effective();
    let mut paths = vec![];
    for (name, keys) in a.effective() {
        let Some(other) = b.get(&name) else { continue };
        for (key, entry) in keys {
            if other.get(key).is_some_and(|e| e.value != entry.value) {
                paths.push(match name {
                    Some(name) => format!("{name}.{key}"),
                    None => key.to_string(),
                });
            }
        }
    }
    Ok(paths)
}

pub fn merge(target: &str, source: &str) -> Result<String> {
    let (t, s) = parse_pair(target, source)?;
    let len = t.lines.len();
    // replacement lines for `start..end`, by `start`
    let mut replace: HashMap<usize, (usize, Vec<String>)> = HashMap::new();
    // lines to insert before each line; the last slot appends
    let mut insert: Vec<Vec<String>> = vec![vec![]; len + 1];
    let mut new_sections = vec![];
    for (name, keys) in s.effective() {
        if !t.has_section(name) {
            new_sections.push((name, keys));
            continue;
        }
        let (at, indent) = t.insert_point(name);
        let before = insert[at].len();
        for (key, entry) in keys {
            match t.get(name, key) {
                Some(existing) if existing.value == entry.value => {}
                Some(existing) => {
                    let lines = replacement(&t, existing, &s, entry);
                    replace.insert(existing.start, (existing.end, lines));
                }
                None => insert[at].extend(s.entry_lines(entry, indent)),
            }
        }
        // keys placed above the first section stay apart from it
        let top_level_above_header = name.is_none()
            && t.sections.first().is_some_and(|s| s.entries.is_empty())
            && at < len
            && !t.lines[at].trim().is_empty();
        if top_level_above_header && insert[at].len() > before {
            insert[at].push(String::new());
        }
    }
    for (name, keys) in new_sections {
        let Some(name) = name else { continue };
        let last = insert[len].last().or(t.lines.last());
        if last.is_some_and(|line| !line.trim().is_empty()) {
            insert[len].push(String::new());
        }
        insert[len].push(s.header_line(name).to_string());
        for entry in keys.values() {
            insert[len].extend(s.lines[entry.start..entry.end].iter().cloned());
        }
    }

    let mut out: Vec<String> = Vec::with_capacity(len);
    let mut i = 0;
    loop {
        out.append(&mut insert[i]);
        if i == len {
            break;
        }
        match replace.remove(&i) {
            Some((end, mut lines)) => {
                out.append(&mut lines);
                i = end;
            }
            None => {
                out.push(t.lines[i].clone());
                i += 1;
            }
        }
    }
    let nl = if target.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut text = out.join(nl);
    if target.is_empty() || target.ends_with('\n') {
        text.push_str(nl);
    }
    Ok(text)
}

/// The lines that set `existing`'s key to `entry`'s value: the target's own
/// key and spacing, the source's value.
fn replacement(t: &Doc, existing: &Entry, s: &Doc, entry: &Entry) -> Vec<String> {
    let indent = t.indent_of(existing);
    let mut lines = s.entry_lines(entry, indent);
    let first = entry
        .value
        .as_deref()
        .map(|v| v.lines().next().unwrap_or(""));
    lines[0] = match (&existing.prefix, first) {
        (_, None) => format!("{indent}{}", existing.key),
        // a value that starts on the next line
        (Some(prefix), Some("")) => prefix.trim_end().to_string(),
        (Some(prefix), Some(value)) => format!("{prefix}{value}"),
        (None, Some("")) => format!("{indent}{} =", existing.key),
        (None, Some(value)) => format!("{indent}{} = {value}", existing.key),
    };
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn merged(target: &str, source: &str) -> String {
        if contains(target, source).unwrap() {
            return target.to_string();
        }
        merge(target, source).unwrap()
    }

    #[test]
    fn edits_keys_in_place_and_keeps_everything_else() {
        let target = "\
; written by the app
[global]
timeout = 15
index-url: https://old.example/simple
# trusted hosts
trusted-host = a

[install]
user = true
";
        let source = "[global]\nindex-url = https://new.example/simple\nretries = 3\n";
        assert_eq!(
            merged(target, source),
            "\
; written by the app
[global]
timeout = 15
index-url: https://new.example/simple
# trusted hosts
trusted-host = a
retries = 3

[install]
user = true
"
        );
    }

    #[test]
    fn a_missing_section_is_appended() {
        assert_eq!(
            merged("[a]\nx = 1\n", "[b]\ny = 2\n\n[c]\n"),
            "[a]\nx = 1\n\n[b]\ny = 2\n\n[c]\n"
        );
    }

    #[test]
    fn an_empty_target_is_created_from_the_source() {
        let source = "top = 1\n\n[a]\nx = 1\n";
        assert_eq!(merged("", source), source);
    }

    #[test]
    fn keys_before_the_first_section_go_above_its_comments() {
        assert_eq!(
            merged("# about a\n[a]\nx = 1\n", "top = 1\n"),
            "top = 1\n\n# about a\n[a]\nx = 1\n"
        );
        assert_eq!(merged("k = v\n[a]\n", "top = 1\n"), "k = v\ntop = 1\n[a]\n");
    }

    #[test]
    fn a_new_key_takes_the_indent_of_the_section() {
        assert_eq!(
            merged(
                "[user]\n\tname = me\n[core]\n\teditor = vi\n",
                "[user]\nemail = me@x\n"
            ),
            "[user]\n\tname = me\n\temail = me@x\n[core]\n\teditor = vi\n"
        );
    }

    #[test]
    fn continuation_lines_belong_to_the_value() {
        let target = "[options]\npackages =\n    a\n    b\nzip_safe = false\n";
        assert!(contains(target, "[options]\npackages =\n  a\n  b\n").unwrap());
        assert!(!contains(target, "[options]\npackages = a\n").unwrap());
        assert_eq!(
            merged(target, "[options]\npackages =\n    c\n"),
            "[options]\npackages =\n    c\nzip_safe = false\n"
        );
        assert_eq!(
            merged(target, "[options]\npackages = one\n"),
            "[options]\npackages = one\nzip_safe = false\n"
        );
    }

    #[test]
    fn inline_semicolons_and_case_are_part_of_what_is_written() {
        assert!(!contains("[a]\nk = v ; note\n", "[a]\nk = v\n").unwrap());
        assert!(!contains("[a]\nKey = v\n", "[a]\nkey = v\n").unwrap());
        assert!(contains("[a]\nk:v\n", "[a]\nk = v\n").unwrap());
    }

    #[test]
    fn the_last_of_a_repeated_key_or_section_is_the_one_set() {
        let target = "[a]\nk = 1\nk = 2\n[b]\n[a]\nk = 3\n";
        assert!(contains(target, "[a]\nk = 3\n").unwrap());
        assert_eq!(
            merged(target, "[a]\nk = 4\nn = 5\n"),
            "[a]\nk = 1\nk = 2\n[b]\n[a]\nk = 4\nn = 5\n"
        );
    }

    #[test]
    fn keys_without_values() {
        let target = "[mysqld]\nskip-networking\nport = 1\n";
        assert!(contains(target, "[mysqld]\nskip-networking\n").unwrap());
        assert!(!contains(target, "[mysqld]\nskip-networking = 1\n").unwrap());
        assert_eq!(
            merged(target, "[mysqld]\nskip-networking = 1\nport\n"),
            "[mysqld]\nskip-networking = 1\nport\n"
        );
    }

    #[test]
    fn an_empty_value_gets_a_space() {
        assert_eq!(merged("[a]\nk =\n", "[a]\nk = v\n"), "[a]\nk = v\n");
        assert_eq!(merged("[a]\nk=x\n", "[a]\nk = v\n"), "[a]\nk=v\n");
    }

    #[test]
    fn crlf_and_a_missing_final_newline_are_kept() {
        assert_eq!(
            merged("[a]\r\nk = 1\r\n", "[a]\nk = 2\nn = 3\n"),
            "[a]\r\nk = 2\r\nn = 3\r\n"
        );
        assert_eq!(merged("[a]\nk = 1", "[a]\nk = 2\n"), "[a]\nk = 2");
    }

    #[test]
    fn missing_lists_only_absent_keys() {
        let target = "[a]\nk = mine\n";
        assert_eq!(
            missing(target, "top = 1\n[a]\nk = default\nn = 2\n[b]\n").unwrap(),
            Some("top = 1\n\n[a]\nn = 2\n\n[b]\n".to_string())
        );
        assert_eq!(missing(target, "[a]\nk = other\n").unwrap(), None);
    }

    #[test]
    fn conflicts_name_the_section_and_key() {
        assert_eq!(
            conflicts(
                "t = 1\n[a]\nk = 1\nsame = x\n",
                "t = 2\n[a]\nk = 2\nsame = x\n"
            )
            .unwrap(),
            vec!["t".to_string(), "a.k".to_string()]
        );
        assert!(
            conflicts("[a]\nk = 1\n", "[b]\nk = 2\n")
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn merging_twice_changes_nothing() {
        let target = "# c\n[a]\nx = 1\n";
        let source = "top = 1\n[a]\nk =\n  multi\n  line\n[b]\ny = 2\n";
        let once = merged(target, source);
        assert!(contains(&once, source).unwrap(), "{once}");
        assert_eq!(merged(&once, source), once);
    }

    #[test]
    fn malformed_lines_are_errors() {
        for text in ["[a\nk = 1\n", "[]\n", "[a]\n= 1\n"] {
            let err = merge(text, "").unwrap_err().to_string();
            assert!(
                err.starts_with("failed to parse the INI target: line "),
                "{err}"
            );
        }
    }
}
