use crate::env;
use aho_corasick::AhoCorasick;
use indexmap::IndexSet;
use std::collections::BTreeSet;
use std::sync::LazyLock;
use std::sync::{Arc, Mutex};

/// Process-wide redaction patterns, registered by config loading.
///
/// Seeded with the secrets inherited from a parent mise, so the first log
/// record of the process is already redacted.
pub static GLOBAL_REDACTOR: LazyLock<Mutex<Redactor>> = LazyLock::new(|| {
    Mutex::new(Redactor::new(inherited_secret_patterns(
        &env::INHERITED_SECRET_KEYS,
        &|k| std::env::var(k).ok(),
    )))
});

/// Every form of `value` that should be redacted: the value itself; for a
/// multi-line value each trimmed line of at least 4 bytes that contains an
/// ASCII alphanumeric (output is redacted line by line); and its JSON-escaped
/// form (without quotes) when that differs from the value.
pub fn secret_patterns(value: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |p: String| {
        if !p.is_empty() && !out.contains(&p) {
            out.push(p);
        }
    };
    push(value.to_string());
    if value.contains('\n') {
        for line in value.lines().map(str::trim) {
            if line.len() >= 4 && line.bytes().any(|b| b.is_ascii_alphanumeric()) {
                push(line.to_string());
            }
        }
    }
    if let Ok(json) = serde_json::to_string(value)
        && let Some(inner) = json.strip_prefix('"').and_then(|j| j.strip_suffix('"'))
        && inner != value
    {
        push(inner.to_string());
    }
    // Rust's `{:?}` spelling (task argument trace logging): U+0001 is `\u{1}`.
    let debug = format!("{value:?}");
    if let Some(inner) = debug.strip_prefix('"').and_then(|d| d.strip_suffix('"')) {
        push(inner.to_string());
    }
    out
}

pub(crate) fn inherited_secret_patterns(
    keys: &BTreeSet<String>,
    get: &dyn Fn(&str) -> Option<String>,
) -> Vec<String> {
    keys.iter()
        .filter_map(|k| get(k))
        .flat_map(|v| secret_patterns(&v))
        .collect()
}

/// Redact registered secrets without needing a loaded `Config`.
///
/// Some of the places a secret can surface have no `Config` to hand:
/// `Display for CmdLineRunner` is rendered into `eyre` errors that are printed
/// long after any config went out of scope.
pub fn redact_global(input: &str) -> String {
    GLOBAL_REDACTOR.lock().unwrap().redact(input)
}

#[derive(Default, Clone, Debug, serde::Deserialize)]
pub struct Redactions(pub IndexSet<String>);

/// A redactor that uses Aho-Corasick for efficient multi-pattern string replacement.
///
/// This is more efficient than iterating through patterns and calling `str::replace()`
/// for each one, especially when there are many patterns. Aho-Corasick finds all
/// matches in a single pass through the text - O(n + z) vs O(n * m). Overlapping
/// matches are merged, so no part of any registered value is printed.
#[derive(Clone)]
pub struct Redactor {
    patterns: Arc<IndexSet<String>>,
    automaton: Option<Arc<AhoCorasick>>,
}

impl Default for Redactor {
    fn default() -> Self {
        Self {
            patterns: Arc::new(IndexSet::new()),
            automaton: None,
        }
    }
}

impl Redactor {
    /// Create a new redactor from a set of patterns to redact.
    pub fn new(patterns: impl IntoIterator<Item = String>) -> Self {
        let patterns: IndexSet<String> = patterns.into_iter().filter(|p| !p.is_empty()).collect();
        let automaton = if patterns.is_empty() {
            None
        } else {
            // Build the Aho-Corasick automaton - O(m) where m is total pattern length
            AhoCorasick::new(patterns.iter()).ok().map(Arc::new)
        };
        Self {
            patterns: Arc::new(patterns),
            automaton,
        }
    }

    /// Create a new redactor by adding more patterns to an existing one.
    pub fn with_additional(&self, additional: impl IntoIterator<Item = String>) -> Self {
        let mut patterns = (*self.patterns).clone();
        for p in additional {
            if !p.is_empty() {
                patterns.insert(p);
            }
        }
        Self::new(patterns)
    }

    /// Returns the patterns as an Arc for efficient sharing.
    pub fn patterns_arc(&self) -> Arc<IndexSet<String>> {
        Arc::clone(&self.patterns)
    }

    /// Redact all matching patterns in the input string, replacing them with `[redacted]`.
    ///
    /// Every occurrence of every pattern is found, including ones that overlap each
    /// other, and overlapping matches are merged into one `[redacted]`. Choosing a
    /// single match per position would leak the tail of a secret that crosses
    /// another one. This is O(n + z) where n is the input length and z is the
    /// number of matches.
    pub fn redact(&self, input: &str) -> String {
        if self.patterns.is_empty() {
            return input.to_string();
        }
        let spans: Vec<(usize, usize)> = match &self.automaton {
            Some(ac) => ac
                .find_overlapping_iter(input)
                .map(|m| (m.start(), m.end()))
                .collect(),
            // Fallback if the automaton failed to build
            None => naive_spans(input, &self.patterns),
        };
        replace_spans(input, spans)
    }
}

/// Every occurrence of every pattern, including self-overlapping ones.
fn naive_spans(input: &str, patterns: &IndexSet<String>) -> Vec<(usize, usize)> {
    let mut spans = vec![];
    for p in patterns {
        let mut from = 0;
        while let Some(i) = input[from..].find(p.as_str()) {
            let start = from + i;
            spans.push((start, start + p.len()));
            // advance one char so the next search stays on a UTF-8 boundary
            from = start + input[start..].chars().next().map_or(1, char::len_utf8);
        }
    }
    spans
}

/// Replace each merged span with `[redacted]`. Spans that only touch stay separate.
fn replace_spans(input: &str, mut spans: Vec<(usize, usize)>) -> String {
    spans.sort_unstable();
    let mut out = String::with_capacity(input.len());
    let mut pos = 0;
    let mut spans = spans.into_iter();
    let Some((mut cur_start, mut cur_end)) = spans.next() else {
        return input.to_string();
    };
    for (start, end) in spans {
        if start < cur_end {
            cur_end = cur_end.max(end);
        } else {
            out.push_str(&input[pos..cur_start]);
            out.push_str("[redacted]");
            pos = cur_end;
            (cur_start, cur_end) = (start, end);
        }
    }
    out.push_str(&input[pos..cur_start]);
    out.push_str("[redacted]");
    out.push_str(&input[cur_end..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_secret_patterns_single_line() {
        assert_eq!(secret_patterns("abc123"), vec!["abc123"]);
        assert!(secret_patterns("").is_empty());
    }

    #[test]
    fn test_secret_patterns_pem_lines() {
        let pem = "-----BEGIN KEY-----\nMIIEvQIBADAN\nZm9v\nabc\n}\n-----END KEY-----";
        let p = secret_patterns(pem);
        assert!(p.contains(&pem.to_string()));
        assert!(p.contains(&"Zm9v".to_string()));
        assert!(p.contains(&"MIIEvQIBADAN".to_string()));
        assert!(!p.contains(&"abc".to_string()));
        assert!(!p.contains(&"}".to_string()));
        let dashes = secret_patterns("-----\n-----\nxx");
        assert!(!dashes.contains(&"-----".to_string()));
    }

    #[test]
    fn test_secret_patterns_cover_rust_debug_spelling() {
        let secret = "ab\u{1}cd\"e";
        let r = Redactor::new(secret_patterns(secret));
        let rendered = format!("{secret:?}");
        assert!(rendered.contains("\\u{1}"));
        assert_eq!(r.redact(&format!("args={rendered}")), "args=\"[redacted]\"");
    }

    #[test]
    fn test_secret_patterns_json_escaped_form() {
        let p = secret_patterns("a\nb\"c");
        assert!(p.contains(&"a\\nb\\\"c".to_string()));
    }

    #[test]
    fn test_inherited_values_that_overlap_leave_no_residue() {
        let keys: BTreeSet<String> = ["A", "B"].map(String::from).into();
        let get = |k: &str| Some(if k == "A" { "a" } else { "abcdef" }.to_string());
        let r = Redactor::new(inherited_secret_patterns(&keys, &get));
        assert!(!r.redact("x abcdef y").contains("bcdef"));
    }

    #[test]
    fn test_inherited_secret_patterns_use_injected_getter() {
        let keys: BTreeSet<String> = ["A", "B"].map(String::from).into();
        let get = |k: &str| (k == "A").then(|| "value-a".to_string());
        assert_eq!(inherited_secret_patterns(&keys, &get), vec!["value-a"]);
    }

    #[test]
    fn test_empty_redactor() {
        let r = Redactor::default();
        assert_eq!(r.redact("hello world"), "hello world");
    }

    #[test]
    fn test_single_pattern() {
        let r = Redactor::new(["secret".to_string()]);
        assert_eq!(r.redact("my secret value"), "my [redacted] value");
    }

    #[test]
    fn test_multiple_patterns() {
        let r = Redactor::new(["secret".to_string(), "password".to_string()]);
        assert_eq!(
            r.redact("secret and password here"),
            "[redacted] and [redacted] here"
        );
    }

    #[test]
    fn test_overlapping_patterns() {
        let r = Redactor::new(["abc".to_string(), "bc".to_string()]);
        let result = r.redact("abcd");
        // Should replace "abc" first, leaving "d"
        assert_eq!(result, "[redacted]d");
    }

    #[test]
    fn test_multiple_occurrences() {
        let r = Redactor::new(["token".to_string()]);
        assert_eq!(r.redact("token1 and token2"), "[redacted]1 and [redacted]2");
    }

    #[test]
    fn test_with_additional() {
        let r1 = Redactor::new(["secret".to_string()]);
        let r2 = r1.with_additional(["password".to_string()]);

        assert_eq!(r1.redact("secret password"), "[redacted] password");
        assert_eq!(r2.redact("secret password"), "[redacted] [redacted]");
    }

    /// The same patterns through the automaton and through the naive fallback.
    fn redactors(patterns: &[&str]) -> [Redactor; 2] {
        let r = Redactor::new(patterns.iter().map(|p| p.to_string()));
        let fallback = Redactor {
            patterns: r.patterns.clone(),
            automaton: None,
        };
        [r, fallback]
    }

    fn permutations<'a>(items: &[&'a str]) -> Vec<Vec<&'a str>> {
        if items.len() <= 1 {
            return vec![items.to_vec()];
        }
        let mut out = vec![];
        for i in 0..items.len() {
            let mut rest = items.to_vec();
            let first = rest.remove(i);
            for mut p in permutations(&rest) {
                p.insert(0, first);
                out.push(p);
            }
        }
        out
    }

    #[test]
    fn test_longest_match_wins() {
        for patterns in [["xxabcSECRETPART", "abc"], ["abc", "xxabcSECRETPART"]] {
            for r in redactors(&patterns) {
                assert_eq!(r.redact("token=xxabcSECRETPART"), "token=[redacted]");
                assert_eq!(r.redact("abc alone"), "[redacted] alone");
            }
        }
    }

    #[test]
    fn test_longest_match_prefix_pair() {
        for r in redactors(&["secret", "secret-long"]) {
            assert_eq!(r.redact("secret-long secret"), "[redacted] [redacted]");
        }
    }

    #[test]
    fn test_crossing_values_are_fully_redacted() {
        for patterns in permutations(&["abcdefgh", "cd", "efghSECRET"]) {
            for r in redactors(&patterns) {
                assert_eq!(r.redact("abcdefghSECRET"), "[redacted]", "{patterns:?}");
            }
        }
        for patterns in permutations(&["abc", "bcde"]) {
            for r in redactors(&patterns) {
                assert_eq!(r.redact("abcde"), "[redacted]", "{patterns:?}");
                assert_eq!(r.redact("xabcdey"), "x[redacted]y", "{patterns:?}");
            }
        }
    }

    #[test]
    fn test_self_overlapping_value() {
        for r in redactors(&["aa"]) {
            assert_eq!(r.redact("aaa"), "[redacted]");
        }
    }

    #[test]
    fn test_adjacent_values_stay_separate() {
        for r in redactors(&["abc", "def"]) {
            assert_eq!(r.redact("abcdef"), "[redacted][redacted]");
        }
        for r in redactors(&["secret"]) {
            assert_eq!(r.redact("secretsecret"), "[redacted][redacted]");
        }
    }

    #[test]
    fn test_non_ascii_input() {
        for r in redactors(&["sécret"]) {
            assert_eq!(r.redact("é sécret é"), "é [redacted] é");
        }
        for r in redactors(&["éé"]) {
            assert_eq!(r.redact("ééé"), "[redacted]");
        }
    }

    #[test]
    fn test_value_lines_and_json_escaped_form() {
        let v = "aaaaaaaaaa\nshort";
        for r in redactors(&[v, "aaaaaaaaaa", "short", "aaaaaaaaaa\\nshort"]) {
            assert_eq!(r.redact(v), "[redacted]");
            assert_eq!(
                r.redact(r#"{"k":"aaaaaaaaaa\nshort"}"#),
                r#"{"k":"[redacted]"}"#
            );
        }
    }

    #[test]
    fn test_with_additional_keeps_longest_wins() {
        let r = Redactor::new(["abc".to_string()]).with_additional(["xxabcSECRETPART".to_string()]);
        assert_eq!(r.redact("token=xxabcSECRETPART"), "token=[redacted]");
        let r = r.with_additional(["cSECRETPARTzz".to_string()]);
        assert_eq!(r.redact("xxabcSECRETPARTzz"), "[redacted]");
    }

    #[test]
    fn test_empty_patterns_filtered() {
        let r = Redactor::new(["".to_string(), "secret".to_string(), "".to_string()]);
        assert_eq!(r.patterns_arc().len(), 1);
        assert_eq!(r.redact("my secret"), "my [redacted]");
    }
}
