pub fn encode_component(input: &str) -> String {
    url::form_urlencoded::byte_serialize(input.as_bytes())
        .collect::<String>()
        .replace('+', "%20")
        .replace('*', "%2A")
        .replace("%7E", "~")
}

pub fn decode_component(input: &str) -> Option<String> {
    let input = input.replace('+', "%2B").replace('&', "%26");
    let query = format!("x={input}");
    url::form_urlencoded::parse(query.as_bytes())
        .next()
        .filter(|(key, _)| key == "x")
        .map(|(_, value)| value.into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_component_matches_existing_component_semantics() {
        assert_eq!(
            encode_component("ubi:https://example.com/foo/bar"),
            "ubi%3Ahttps%3A%2F%2Fexample.com%2Ffoo%2Fbar"
        );
        assert_eq!(encode_component("a b+c*d~e"), "a%20b%2Bc%2Ad~e");
    }

    #[test]
    fn decode_component_keeps_plus_literal() {
        assert_eq!(
            decode_component("file%20with+plus%26amp.tgz").as_deref(),
            Some("file with+plus&amp.tgz")
        );
    }
}
