use std::str::Chars;

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
            if wildcard_match_single(input, pattern) {
                return true;
            }
        }
        false
    }
}

fn wildcard_match_single(input: &str, wildcard: &str) -> bool {
    let mut input_chars = input.chars();
    let mut wildcard_chars = wildcard.chars();

    loop {
        match (input_chars.next(), wildcard_chars.next()) {
            (Some(input_char), Some(wildcard_char)) => {
                if wildcard_char == '*' {
                    return wildcard_match_single_star(input_chars, wildcard_chars);
                } else if wildcard_char == '?' || input_char == wildcard_char {
                    continue;
                } else {
                    return false;
                }
            }
            (None, None) => return true,
            (None, Some(wildcard_char)) => return wildcard_char == '*',
            (Some(_), None) => return false,
        }
    }
}

fn wildcard_match_single_star(mut input_chars: Chars, mut wildcard_chars: Chars) -> bool {
    loop {
        match wildcard_chars.next() {
            Some(wildcard_char) => {
                if wildcard_char == '*' {
                    continue;
                } else {
                    while let Some(input_char) = input_chars.next() {
                        if wildcard_match_single(
                            &input_char.to_string(),
                            &wildcard_char.to_string(),
                        ) {
                            return wildcard_match_single_star(input_chars, wildcard_chars);
                        }
                    }
                    return false;
                }
            }
            None => return true,
        }
    }
}
