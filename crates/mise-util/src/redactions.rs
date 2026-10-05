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
/// matches in a single pass through the text - O(n + z) vs O(n * m).
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
    /// This is O(n + z) where n is the input length and z is the number of matches,
    /// compared to O(n * m) for the naive approach of iterating through m patterns.
    pub fn redact(&self, input: &str) -> String {
        match &self.automaton {
            Some(ac) => {
                // Each pattern needs its own replacement string
                let replacements: Vec<&str> = vec!["[redacted]"; self.patterns.len()];
                ac.replace_all(input, &replacements)
            }
            None if self.patterns.is_empty() => input.to_string(),
            None => {
                // Fallback to naive approach if automaton failed to build
                let mut result = input.to_string();
                for pattern in self.patterns.iter() {
                    result = result.replace(pattern, "[redacted]");
                }
                result
            }
        }
    }
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
    fn test_secret_patterns_json_escaped_form() {
        let p = secret_patterns("a\nb\"c");
        assert!(p.contains(&"a\\nb\\\"c".to_string()));
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

    #[test]
    fn test_empty_patterns_filtered() {
        let r = Redactor::new(["".to_string(), "secret".to_string(), "".to_string()]);
        assert_eq!(r.patterns_arc().len(), 1);
        assert_eq!(r.redact("my secret"), "my [redacted]");
    }
}
