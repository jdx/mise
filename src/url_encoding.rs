pub fn encode_component(input: &str) -> String {
    url::form_urlencoded::byte_serialize(input.as_bytes())
        .collect::<String>()
        .replace('+', "%20")
        .replace('*', "%2A")
        .replace("%7E", "~")
}

pub fn decode_component(input: &str) -> Option<String> {
    let bytes = input.as_bytes();
    if !bytes.contains(&b'%') {
        return Some(input.to_string());
    }

    let mut decoded = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'%' {
            decoded.push(bytes[i]);
            i += 1;
            continue;
        }

        match bytes.get(i + 1..i + 3) {
            Some(&[first, second]) => match (hex_digit(first), hex_digit(second)) {
                (Some(first), Some(second)) => {
                    decoded.push((first << 4) | second);
                    i += 3;
                }
                (Some(_), None) => {
                    decoded.extend_from_slice(&bytes[i..i + 2]);
                    i += 2;
                }
                (None, _) => {
                    decoded.push(b'%');
                    i += 1;
                }
            },
            _ => {
                decoded.extend_from_slice(&bytes[i..]);
                break;
            }
        }
    }

    String::from_utf8(decoded).ok()
}

fn hex_digit(digit: u8) -> Option<u8> {
    match digit {
        b'0'..=b'9' => Some(digit - b'0'),
        b'A'..=b'F' => Some(digit - b'A' + 10),
        b'a'..=b'f' => Some(digit - b'a' + 10),
        _ => None,
    }
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

    #[test]
    fn decode_component_rejects_invalid_utf8() {
        assert_eq!(decode_component("file-%FF.tgz"), None);
        assert_eq!(
            decode_component("file-%zz-%A.tgz").as_deref(),
            Some("file-%zz-%A.tgz")
        );
    }
}
