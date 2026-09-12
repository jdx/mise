use nucleo_matcher::pattern::{Atom, AtomKind, CaseMatching, Normalization};
use nucleo_matcher::{Config, Matcher, Utf32Str};

#[derive(Debug, Clone)]
pub(crate) struct FuzzyPattern(Atom);

impl FuzzyPattern {
    pub(crate) fn new(needle: &str) -> Self {
        Self(Atom::new(
            needle,
            CaseMatching::Smart,
            Normalization::Smart,
            AtomKind::Fuzzy,
            false,
        ))
    }
}

#[derive(Debug, Clone)]
pub(crate) struct FuzzyMatcher {
    matcher: Matcher,
    haystack_buf: Vec<char>,
}

impl Default for FuzzyMatcher {
    fn default() -> Self {
        Self {
            matcher: Matcher::new(Config::DEFAULT),
            haystack_buf: Vec::new(),
        }
    }
}

impl FuzzyMatcher {
    pub(crate) fn score_pattern(&mut self, haystack: &str, pattern: &FuzzyPattern) -> Option<u32> {
        pattern
            .0
            .score(
                Utf32Str::new(haystack, &mut self.haystack_buf),
                &mut self.matcher,
            )
            .map(u32::from)
    }
}
