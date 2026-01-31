use aho_corasick::AhoCorasick;
use indexmap::IndexSet;
use std::sync::Arc;

#[derive(Default, Clone, Debug, serde::Deserialize)]
pub struct Redactions(pub IndexSet<String>);

impl Redactions {
    pub fn merge(&mut self, other: Self) {
        self.0.extend(other.0);
    }

    pub fn render(&mut self, tera: &mut tera::Tera, ctx: &tera::Context) -> eyre::Result<()> {
        for r in self.0.clone().drain(..) {
            self.0.insert(tera.render_str(&r, ctx)?);
        }
        Ok(())
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

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

    /// Returns the patterns being redacted.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn patterns(&self) -> &IndexSet<String> {
        &self.patterns
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
        assert_eq!(r.patterns().len(), 1);
        assert_eq!(r.redact("my secret"), "my [redacted]");
    }
}
