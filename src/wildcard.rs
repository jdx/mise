pub struct Wildcard {
    patterns: Vec<String>,
}

impl Wildcard {
    pub fn new(patterns: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            patterns: patterns.into_iter().map(Into::into).collect(),
        }
    }

    pub fn match_any(&self, input: &str) -> bool {
        for pattern in &self.patterns {
            if wildcard_match(input, pattern) {
                return true;
            }
        }
        false
    }
}

pub(crate) fn wildcard_match(input: &str, wildcard: &str) -> bool {
    let input_chars: Vec<_> = input.chars().collect();
    let wildcard_chars: Vec<_> = wildcard.chars().collect();
    let mut input_idx = 0;
    let mut wildcard_idx = 0;
    let mut star_idx = None;
    let mut star_input_idx = 0;

    while input_idx < input_chars.len() {
        if wildcard_idx < wildcard_chars.len()
            && (wildcard_chars[wildcard_idx] == '?'
                || wildcard_chars[wildcard_idx] == input_chars[input_idx])
        {
            input_idx += 1;
            wildcard_idx += 1;
        } else if wildcard_idx < wildcard_chars.len() && wildcard_chars[wildcard_idx] == '*' {
            star_idx = Some(wildcard_idx);
            wildcard_idx += 1;
            star_input_idx = input_idx;
        } else if let Some(idx) = star_idx {
            wildcard_idx = idx + 1;
            star_input_idx += 1;
            input_idx = star_input_idx;
        } else {
            return false;
        }
    }

    while wildcard_idx < wildcard_chars.len() && wildcard_chars[wildcard_idx] == '*' {
        wildcard_idx += 1;
    }

    wildcard_idx == wildcard_chars.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wildcard_match_exact() {
        assert!(wildcard_match("FOO", "FOO"));
        assert!(!wildcard_match("FOO", "FOOBAR"));
        assert!(!wildcard_match("FOO", "BAR"));
    }

    #[test]
    fn test_wildcard_match_star() {
        assert!(wildcard_match("SECRET_FOO", "SECRET*"));
        assert!(wildcard_match("API_KEY", "*_KEY"));
        assert!(wildcard_match("AUTH_PROD_KEY", "AUTH_*_KEY"));
        assert!(wildcard_match("AUTH__KEY", "AUTH_*_KEY"));
        assert!(wildcard_match("ANYTHING", "*"));
        assert!(wildcard_match("", "*"));
        assert!(!wildcard_match("AUTH_KEY", "AUTH_*_KEY"));
    }

    #[test]
    fn test_wildcard_match_question() {
        assert!(wildcard_match("FOO", "F?O"));
        assert!(!wildcard_match("FO", "F?O"));
    }

    #[test]
    fn test_wildcard_match_does_not_treat_suffix_as_subsequence() {
        assert!(!wildcard_match("DISABLE_FEEDBACK_SURVEY", "*_KEY"));
        assert!(!wildcard_match("CLOUDSDK_CORE_DISABLE_PROMPTS", "*_CREDS"));
        assert!(!wildcard_match("NETWORK_CLIENTS_UPDATE", "*_TOKEN"));
    }

    #[test]
    fn test_wildcard_match_multiple_stars_backtracks() {
        assert!(wildcard_match("abcde", "a*d?"));
        assert!(wildcard_match("abcde", "a*?e"));
        assert!(wildcard_match("abcde", "*b*d*"));
        assert!(!wildcard_match("abcde", "a*c?f"));
    }
}
