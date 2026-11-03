//! TOML key paths as text: `tools."npm:foo".version`.

/// Splits a dotted key into its segments, unquoting quoted ones. A trailing dot yields a final
/// empty segment, the one being typed.
pub(crate) fn split(path: &str) -> Vec<String> {
    let mut segments = vec![];
    let mut current = String::new();
    let mut quote = None;
    for c in path.chars() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => current.push(c),
            (None, '"' | '\'') => quote = Some(c),
            (None, '.') => segments.push(std::mem::take(&mut current).trim().to_string()),
            (None, c) => current.push(c),
        }
    }
    segments.push(current.trim().to_string());
    segments
}

/// The last segment of a dotted key as written, quotes included: the part after the last dot
/// that is not inside quotes.
pub(crate) fn last_segment(path: &str) -> &str {
    let mut quote = None;
    let mut start = 0;
    for (i, c) in path.char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '.') => start = i + 1,
            _ => {}
        }
    }
    path[start..].trim_start()
}

/// A key as it must be written: bare when TOML allows it, else quoted.
pub(crate) fn quote(name: &str) -> String {
    let bare = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if bare {
        name.to_string()
    } else {
        format!("\"{}\"", name.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

/// The nearest of `candidates` to `name`, when it is close enough to be a typo of it.
pub(crate) fn closest<'a>(
    name: &str,
    candidates: impl Iterator<Item = &'a str>,
) -> Option<&'a str> {
    let limit = (name.chars().count() / 3).clamp(1, 3);
    candidates
        .map(|c| (distance(name, c), c))
        .filter(|(d, _)| *d <= limit)
        .min_by_key(|(d, _)| *d)
        .map(|(_, c)| c)
}

fn distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut row = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            let substitution = previous[j] + usize::from(ca != *cb);
            row.push(substitution.min(previous[j + 1] + 1).min(row[j] + 1));
        }
        previous = row;
    }
    previous[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_quoted_segments() {
        assert_eq!(
            split("tools.\"npm:a.b\".version"),
            ["tools", "npm:a.b", "version"]
        );
        assert_eq!(split("settings."), ["settings", ""]);
        assert_eq!(split(""), [""]);
    }

    #[test]
    fn the_last_segment_keeps_its_quotes_and_inner_dots() {
        assert_eq!(last_segment("tools.\"llama.c"), "\"llama.c");
        assert_eq!(last_segment("a. b"), "b");
        assert_eq!(last_segment("plain"), "plain");
    }

    #[test]
    fn quotes_only_when_needed() {
        assert_eq!(quote("node"), "node");
        assert_eq!(quote("npm:foo"), "\"npm:foo\"");
    }

    #[test]
    fn suggests_near_misses() {
        let keys = ["min_version", "settings", "tools"];
        assert_eq!(closest("setings", keys.into_iter()), Some("settings"));
        assert_eq!(closest("zzzzzz", keys.into_iter()), None);
    }
}
