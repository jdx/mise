use crate::parse::is_horizontal_whitespace;
use std::{borrow::Cow, error, fmt};

/// An error encountered while rendering a dotenv assignment.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum RenderError {
    /// Environment variable names cannot be empty.
    EmptyKey,
    /// An environment variable name contains a character unsupported by dotenv syntax.
    InvalidKeyCharacter(char),
    /// An environment variable value contains a character unsupported by dotenv syntax.
    InvalidValueCharacter(char),
}

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyKey => f.write_str("environment variable name cannot be empty"),
            Self::InvalidKeyCharacter(character) => {
                write!(
                    f,
                    "environment variable name contains invalid character {character:?}"
                )
            }
            Self::InvalidValueCharacter(character) => {
                write!(
                    f,
                    "environment variable value contains invalid character {character:?}"
                )
            }
        }
    }
}

impl error::Error for RenderError {}

/// Renders a value using the minimum quoting required by the default dotenv syntax.
///
/// Values that already round-trip as unquoted input are returned unchanged and without an
/// allocation. Other values are double-quoted, with control characters and syntax-significant
/// characters escaped. Dollar signs remain unquoted when possible because variable substitution
/// is disabled by default.
///
/// # Examples
///
/// ```
/// use mise_dotenv::render_value;
///
/// assert_eq!(render_value("hello world")?.as_ref(), "hello world");
/// assert_eq!(render_value(" trailing ")?.as_ref(), r#"" trailing ""#);
/// # Ok::<(), mise_dotenv::RenderError>(())
/// ```
pub fn render_value(value: &str) -> Result<Cow<'_, str>, RenderError> {
    if value.contains('\0') {
        return Err(RenderError::InvalidValueCharacter('\0'));
    }

    if is_unquoted_safe(value) {
        return Ok(Cow::Borrowed(value));
    }

    let mut rendered = String::with_capacity(value.len() + 2);
    rendered.push('"');
    for character in value.chars() {
        match character {
            '\u{0007}' => rendered.push_str("\\a"),
            '\u{0008}' => rendered.push_str("\\b"),
            '\u{000c}' => rendered.push_str("\\f"),
            '\n' => rendered.push_str("\\n"),
            '\r' => rendered.push_str("\\r"),
            '\t' => rendered.push_str("\\t"),
            '\u{000b}' => rendered.push_str("\\v"),
            '\\' => rendered.push_str("\\\\"),
            '"' => rendered.push_str("\\\""),
            '$' => rendered.push_str("\\$"),
            _ => rendered.push(character),
        }
    }
    rendered.push('"');
    Ok(Cow::Owned(rendered))
}

/// Renders one `key=value` assignment without a trailing newline.
///
/// The key is validated against this crate's dotenv syntax. In particular, it must be non-empty
/// and cannot contain whitespace, `=`, `#`, or control characters. A key cannot begin with a
/// byte-order mark because dotenv parsers treat one at the start of a document as an encoding
/// marker rather than part of the key.
pub fn render_var(key: &str, value: &str) -> Result<String, RenderError> {
    validate_key(key)?;
    let value = render_value(value)?;
    let mut rendered = String::with_capacity(key.len() + value.len() + 1);
    rendered.push_str(key);
    rendered.push('=');
    rendered.push_str(&value);
    Ok(rendered)
}

/// Renders assignments as a dotenv document in iterator order.
///
/// Assignments are separated by newlines. The returned document has no trailing newline.
/// Callers that need stable output from an unordered map should sort its entries first.
pub fn render<I, K, V>(variables: I) -> Result<String, RenderError>
where
    I: IntoIterator<Item = (K, V)>,
    K: AsRef<str>,
    V: AsRef<str>,
{
    let mut document = String::new();
    for (key, value) in variables {
        if !document.is_empty() {
            document.push('\n');
        }
        document.push_str(&render_var(key.as_ref(), value.as_ref())?);
    }
    Ok(document)
}

fn validate_key(key: &str) -> Result<(), RenderError> {
    if key.is_empty() {
        return Err(RenderError::EmptyKey);
    }
    if key.starts_with('\u{feff}') {
        return Err(RenderError::InvalidKeyCharacter('\u{feff}'));
    }

    if let Some(character) = key.chars().find(|character| {
        matches!(character, '=' | '#') || character.is_whitespace() || character.is_control()
    }) {
        return Err(RenderError::InvalidKeyCharacter(character));
    }

    Ok(())
}

fn is_unquoted_safe(value: &str) -> bool {
    let Some(first) = value.chars().next() else {
        return true;
    };
    if matches!(first, '\'' | '"' | '#' | '\n' | '\r') || is_horizontal_whitespace(first) {
        return false;
    }
    // The parser trims trailing whitespace from an unquoted value unless a
    // backslash escapes it (an odd run of backslashes before it), so only
    // unescaped trailing whitespace needs quoting. Escapes the parser would
    // decode (a backslash before a space or a tab) are rejected below.
    if let Some((index, last)) = value.char_indices().next_back()
        && is_horizontal_whitespace(last)
        && value[..index]
            .chars()
            .rev()
            .take_while(|character| *character == '\\')
            .count()
            % 2
            == 0
    {
        return false;
    }

    let mut quote = None;
    let mut escaped = false;
    let mut previous = None;
    for character in value.chars() {
        if matches!(character, '\0' | '\n' | '\r') {
            return false;
        }

        if let Some(open_quote) = quote {
            if character == open_quote && !escaped {
                quote = None;
            }
            if character == '\\' {
                escaped = !escaped;
            } else {
                escaped = false;
            }
        } else {
            if character == '#' && previous.is_none_or(is_horizontal_whitespace) && !escaped {
                return false;
            }
            if matches!(character, '\'' | '"') && !escaped {
                quote = Some(character);
            }
            escaped = character == '\\' && !escaped;
        }

        previous = Some(character);
    }

    let mut characters = value.chars();
    while let Some(character) = characters.next() {
        if character != '\\' {
            continue;
        }
        let Some(escaped) = characters.next() else {
            break;
        };
        if matches!(escaped, '\\' | '\'' | '"' | '$' | '#' | ' ' | '\t') {
            return false;
        }
    }

    true
}

#[cfg(test)]
mod tests {
    use super::{RenderError, render, render_value, render_var};
    use crate::{EnvLoader, EnvMap, EnvSequence};
    use proptest::prelude::*;
    use std::io::Cursor;

    fn parse(document: &str) -> Result<EnvMap, crate::Error> {
        EnvLoader::with_reader(Cursor::new(document))
            .sequence(EnvSequence::InputOnly)
            .multiline(false)
            .load()
    }

    fn arbitrary_value() -> impl Strategy<Value = String> {
        let unicode = any::<String>().prop_filter("dotenv values cannot contain NUL", |value| {
            !value.contains('\0')
        });
        let syntax_heavy = proptest::string::string_regex(r"[\x01-\x7F]{0,64}")
            .expect("the static property-test regular expression should be valid");
        prop_oneof![unicode, syntax_heavy]
    }

    #[test]
    fn leaves_values_unquoted_when_they_already_round_trip() {
        for value in [
            "",
            "plain",
            "hello world",
            r"C:\Users\me",
            "value#literal",
            "$2b$12$literal-bcrypt-hash",
            "trailing\\",
            r"unknown\qescape",
            r"literal\nsequence",
            "internal\twhitespace",
        ] {
            assert_eq!(render_value(value).unwrap().as_ref(), value);
        }
    }

    #[test]
    fn quotes_and_escapes_values_only_when_required() {
        for (value, expected) in [
            (" leading", r#"" leading""#),
            ("trailing ", r#""trailing ""#),
            ("#comment", "\"#comment\""),
            ("value # comment", r#""value # comment""#),
            (r#""quoted""#, r#""\"quoted\"""#),
            ("line\nbreak", r#""line\nbreak""#),
            ("carriage\rreturn", r#""carriage\rreturn""#),
            (r"literal\ value", r#""literal\\ value""#),
            (" leading $HOME", r#"" leading \$HOME""#),
        ] {
            assert_eq!(render_value(value).unwrap().as_ref(), expected);
        }
    }

    #[test]
    fn rejects_unrepresentable_values_and_invalid_keys() {
        assert_eq!(
            render_value("nul\0value"),
            Err(RenderError::InvalidValueCharacter('\0'))
        );
        assert_eq!(render_var("", "value"), Err(RenderError::EmptyKey));

        for (key, character) in [
            ("HAS SPACE", ' '),
            ("HAS=EQUALS", '='),
            ("HAS#HASH", '#'),
            ("HAS\nNEWLINE", '\n'),
            ("HAS\0NUL", '\0'),
            ("\u{feff}BOM", '\u{feff}'),
        ] {
            assert_eq!(
                render_var(key, "value"),
                Err(RenderError::InvalidKeyCharacter(character))
            );
        }
    }

    #[test]
    fn render_error_messages_identify_the_invalid_field() {
        assert_eq!(
            RenderError::EmptyKey.to_string(),
            "environment variable name cannot be empty"
        );
        assert_eq!(
            RenderError::InvalidKeyCharacter('\n').to_string(),
            "environment variable name contains invalid character '\\n'"
        );
        assert_eq!(
            RenderError::InvalidValueCharacter('\0').to_string(),
            "environment variable value contains invalid character '\\0'"
        );
    }

    #[test]
    fn escapes_all_supported_control_characters_when_quoting() {
        let value = " \u{0007}\u{0008}\u{000c}\n\r\t\u{000b}";
        assert_eq!(render_value(value).unwrap(), r#"" \a\b\f\n\r\t\v""#);
        assert_eq!(
            parse(&format!("VALUE={}", render_value(value).unwrap())).unwrap()["VALUE"],
            value
        );
    }

    #[test]
    fn renders_assignments_and_documents() {
        assert_eq!(
            render_var("naïve-key.name", "hello world").unwrap(),
            "naïve-key.name=hello world"
        );
        assert_eq!(
            render_var("embedded\u{feff}bom", "value").unwrap(),
            "embedded\u{feff}bom=value"
        );
        assert_eq!(
            render([("FIRST", "one"), ("SECOND", " two")]).unwrap(),
            "FIRST=one\nSECOND=\" two\""
        );
        assert_eq!(render(Vec::<(&str, &str)>::new()).unwrap(), "");
    }

    #[test]
    fn escaped_trailing_whitespace_is_left_unquoted_when_it_round_trips() {
        // A backslash keeps the parser from trimming the vertical tab after it, and
        // the pair is kept as written.
        let value = "a\\\u{b}";
        assert_eq!(render_value(value).unwrap(), value);
        assert_eq!(parse(&format!("VALUE={value}")).unwrap()["VALUE"], value);
        // An unescaped one is trimmed, and a backslash before a space is decoded,
        // so both are quoted.
        assert_eq!(render_value("a\u{b}").unwrap(), "\"a\\v\"");
        assert_eq!(render_value("a\\ ").unwrap(), "\"a\\\\ \"");
    }

    proptest! {
        #[test]
        fn rendered_values_round_trip(value in arbitrary_value()) {
            let assignment = render_var("PROPERTY", &value).unwrap();
            let parsed = parse(&assignment).unwrap();
            prop_assert_eq!(parsed.len(), 1);
            prop_assert_eq!(parsed.get("PROPERTY").map(String::as_str), Some(value.as_str()));
        }

        #[test]
        fn values_are_quoted_exactly_when_raw_input_would_not_round_trip(
            value in arbitrary_value()
        ) {
            let rendered = render_value(&value).unwrap();
            let raw_assignment = format!("PROPERTY={value}");
            let raw_round_trips = parse(&raw_assignment).is_ok_and(|parsed| {
                parsed.len() == 1
                    && parsed.get("PROPERTY").is_some_and(|parsed| parsed == &value)
            });

            prop_assert_eq!(rendered.as_ref() == value, raw_round_trips);
        }

        #[test]
        fn rendered_documents_round_trip(values in prop::collection::vec(arbitrary_value(), 0..32)) {
            let document = render(
                values
                    .iter()
                    .enumerate()
                    .map(|(index, value)| (format!("KEY_{index}"), value)),
            )
            .unwrap();
            let parsed = parse(&document).unwrap();

            prop_assert_eq!(parsed.len(), values.len());
            for (index, value) in values.iter().enumerate() {
                let key = format!("KEY_{index}");
                prop_assert_eq!(parsed.get(&key), Some(value));
            }
        }
    }
}
