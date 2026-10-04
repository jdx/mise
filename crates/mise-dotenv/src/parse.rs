use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    error, fmt,
    rc::Rc,
};

pub type SubstitutionTracker = Rc<RefCell<HashSet<String>>>;

/// Options controlling how dotenv values are parsed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParseOptions {
    substitution: bool,
    multiline: bool,
}

impl ParseOptions {
    /// Creates options using the default dotenv syntax.
    ///
    /// Variable substitution is disabled by default so dollar signs are
    /// preserved literally. Multiline quoted values are enabled.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            substitution: false,
            multiline: true,
        }
    }

    /// Enables or disables `$NAME` and `${NAME}` substitution.
    #[must_use]
    pub const fn substitution(mut self, enabled: bool) -> Self {
        self.substitution = enabled;
        self
    }

    /// Enables or disables physical newlines inside quoted values.
    #[must_use]
    pub const fn multiline(mut self, enabled: bool) -> Self {
        self.multiline = enabled;
        self
    }

    /// Returns whether variable substitution is enabled.
    #[must_use]
    pub const fn substitution_enabled(self) -> bool {
        self.substitution
    }

    /// Returns whether quoted values may span physical lines.
    #[must_use]
    pub const fn multiline_enabled(self) -> bool {
        self.multiline
    }
}

impl Default for ParseOptions {
    fn default() -> Self {
        Self::new()
    }
}

/// The reason a dotenv document could not be parsed.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ParseErrorKind {
    /// An assignment did not contain a key.
    MissingKey,
    /// A key was not followed by `=`.
    MissingEquals,
    /// A key contains a character that cannot be represented in an environment variable.
    InvalidKeyCharacter(char),
    /// A value contains a character that cannot be represented in an environment variable.
    InvalidValueCharacter(char),
    /// Non-comment text followed a quoted value.
    TrailingCharacters,
    /// A quoted value was not closed.
    UnterminatedQuote(char),
    /// A `${...}` expression was not closed.
    UnterminatedSubstitution,
    /// A `${...}` expression used unsupported syntax.
    InvalidSubstitution,
    /// A required variable substitution failed.
    RequiredVariable {
        /// The missing or empty variable.
        name: String,
        /// The optional message supplied in the substitution expression.
        message: String,
    },
    /// A quoted value crossed a line while multiline values were disabled.
    MultilineDisabled,
}

impl fmt::Display for ParseErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingKey => f.write_str("expected an environment variable name"),
            Self::MissingEquals => f.write_str("expected `=` after the environment variable name"),
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
            Self::TrailingCharacters => {
                f.write_str("expected a comment or end of line after the quoted value")
            }
            Self::UnterminatedQuote(quote) => write!(f, "unterminated {quote:?} quoted value"),
            Self::UnterminatedSubstitution => f.write_str("unterminated `${...}` substitution"),
            Self::InvalidSubstitution => f.write_str("invalid variable substitution"),
            Self::RequiredVariable { name, message } if message.is_empty() => {
                write!(f, "required variable `{name}` is not set")
            }
            Self::RequiredVariable { name, message } => {
                write!(f, "required variable `{name}` is not set: {message}")
            }
            Self::MultilineDisabled => f.write_str("multiline quoted values are disabled"),
        }
    }
}

/// A source-aware dotenv syntax error.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParseError {
    kind: ParseErrorKind,
    line: usize,
    column: usize,
    byte_offset: usize,
    source_line: String,
}

impl ParseError {
    /// Returns the specific parsing failure.
    #[must_use]
    pub const fn kind(&self) -> &ParseErrorKind {
        &self.kind
    }

    /// Returns the one-based source line containing the error.
    #[must_use]
    pub const fn line(&self) -> usize {
        self.line
    }

    /// Returns the one-based Unicode character column containing the error.
    #[must_use]
    pub const fn column(&self) -> usize {
        self.column
    }

    /// Returns the zero-based byte offset of the error.
    #[must_use]
    pub const fn byte_offset(&self) -> usize {
        self.byte_offset
    }

    /// Returns the physical source line containing the error.
    #[must_use]
    pub fn source_line(&self) -> &str {
        &self.source_line
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let marker_padding: String = self
            .source_line
            .chars()
            .take(self.column.saturating_sub(1))
            .map(|character| if character == '\t' { '\t' } else { ' ' })
            .collect();
        let line_width = self.line.to_string().len();

        writeln!(
            f,
            "dotenv syntax error at line {}, column {}: {}",
            self.line, self.column, self.kind
        )?;
        writeln!(f, "{space:>line_width$} |", space = "")?;
        writeln!(f, "{} | {}", self.line, self.source_line)?;
        write!(f, "{space:>line_width$} | {marker_padding}^", space = "")
    }
}

impl error::Error for ParseError {}

#[derive(Clone, Copy)]
enum ValueStyle {
    Unquoted,
    SingleQuoted,
    DoubleQuoted,
}

pub struct Parser {
    source: String,
    position: usize,
    options: ParseOptions,
    base_variables: HashMap<String, String>,
    protected_variables: HashMap<String, String>,
    parsed_variables: HashMap<String, String>,
    substitution_tracker: Option<SubstitutionTracker>,
    input_overrides: bool,
    failed: bool,
}

impl Parser {
    pub(super) fn new(
        mut source: String,
        options: ParseOptions,
        base_variables: HashMap<String, String>,
        protected_variables: HashMap<String, String>,
        substitution_tracker: Option<SubstitutionTracker>,
        input_overrides: bool,
    ) -> Self {
        if source.starts_with('\u{feff}') {
            source.drain(..'\u{feff}'.len_utf8());
        }

        Self {
            source,
            position: 0,
            options,
            base_variables,
            protected_variables,
            parsed_variables: HashMap::new(),
            substitution_tracker,
            input_overrides,
            failed: false,
        }
    }

    pub(super) fn next_entry(&mut self) -> Option<Result<(String, String), ParseError>> {
        if self.failed {
            return None;
        }

        loop {
            self.skip_horizontal_whitespace();

            if self.at_end() {
                return None;
            }
            if self.consume_newline() {
                continue;
            }
            if self.peek() == Some('#') {
                self.skip_comment();
                continue;
            }

            let result = self.parse_assignment();
            match result {
                Ok((key, value)) => {
                    crate::key::insert(&mut self.parsed_variables, key.clone(), value.clone());
                    return Some(Ok((key, value)));
                }
                Err(error) => {
                    self.failed = true;
                    return Some(Err(error));
                }
            }
        }
    }

    fn parse_assignment(&mut self) -> Result<(String, String), ParseError> {
        self.consume_export_prefix();

        let key_start = self.position;
        while let Some(character) = self.peek() {
            if character == '=' || is_horizontal_whitespace(character) {
                break;
            }
            if character == '\n' || character == '\r' || character == '#' {
                break;
            }
            if character == '\0' || character.is_control() {
                return Err(self.error(
                    self.position,
                    ParseErrorKind::InvalidKeyCharacter(character),
                ));
            }
            self.advance();
        }

        if self.position == key_start {
            return Err(self.error(key_start, ParseErrorKind::MissingKey));
        }

        let key = self.source[key_start..self.position].to_owned();
        self.skip_horizontal_whitespace();
        if self.peek() != Some('=') {
            return Err(self.error(self.position, ParseErrorKind::MissingEquals));
        }
        self.advance();
        self.skip_horizontal_whitespace();

        let value = match self.peek() {
            None | Some('\n' | '\r' | '#') => {
                if self.peek() == Some('#') {
                    self.skip_comment();
                } else {
                    self.consume_newline();
                }
                String::new()
            }
            Some('\'') => self.parse_quoted('\'', ValueStyle::SingleQuoted)?,
            Some('"') => self.parse_quoted('"', ValueStyle::DoubleQuoted)?,
            Some(_) => self.parse_unquoted()?,
        };

        Ok((key, value))
    }

    fn consume_export_prefix(&mut self) {
        let Some(after_export) = self.source[self.position..].strip_prefix("export") else {
            return;
        };
        let Some(next) = after_export.chars().next() else {
            return;
        };
        if !is_horizontal_whitespace(next) {
            return;
        }

        self.position += "export".len();
        self.skip_horizontal_whitespace();
    }

    fn parse_quoted(&mut self, quote: char, style: ValueStyle) -> Result<String, ParseError> {
        let quote_position = self.position;
        self.advance();
        let value_start = self.position;
        let mut escaped = false;

        loop {
            let Some(character) = self.peek() else {
                return Err(self.error(quote_position, ParseErrorKind::UnterminatedQuote(quote)));
            };

            if character == '\n' || character == '\r' {
                if !self.options.multiline {
                    return Err(self.error(self.position, ParseErrorKind::MultilineDisabled));
                }
                escaped = false;
                self.consume_newline();
                continue;
            }
            if character == '\0' {
                return Err(self.error(
                    self.position,
                    ParseErrorKind::InvalidValueCharacter(character),
                ));
            }

            if character == quote && !escaped {
                let value_end = self.position;
                self.advance();
                let raw = &self.source[value_start..value_end];
                let value = self.expand(raw, value_start, style)?;
                self.finish_quoted_value()?;
                return Ok(value);
            }

            if character == '\\' {
                escaped = !escaped;
            } else {
                escaped = false;
            }
            self.advance();
        }
    }

    fn finish_quoted_value(&mut self) -> Result<(), ParseError> {
        self.skip_horizontal_whitespace();
        match self.peek() {
            None => Ok(()),
            Some('#') => {
                self.skip_comment();
                Ok(())
            }
            Some('\n' | '\r') => {
                self.consume_newline();
                Ok(())
            }
            Some(_) => Err(self.error(self.position, ParseErrorKind::TrailingCharacters)),
        }
    }

    fn parse_unquoted(&mut self) -> Result<String, ParseError> {
        let value_start = self.position;
        let mut quote = None;
        let mut escaped = false;
        let mut previous = None;

        while let Some(character) = self.peek() {
            if character == '\n' || character == '\r' {
                break;
            }
            if character == '\0' {
                return Err(self.error(
                    self.position,
                    ParseErrorKind::InvalidValueCharacter(character),
                ));
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
                    break;
                }
                if matches!(character, '\'' | '"') && !escaped {
                    quote = Some(character);
                }
                escaped = character == '\\' && !escaped;
            }

            previous = Some(character);
            self.advance();
        }

        let value_end = self.position;
        let raw = trim_unquoted_trailing_whitespace(&self.source[value_start..value_end]);
        let value = self.expand(raw, value_start, ValueStyle::Unquoted)?;

        if self.peek() == Some('#') {
            self.skip_comment();
        } else {
            self.consume_newline();
        }
        Ok(value)
    }

    fn expand(
        &self,
        raw: &str,
        source_offset: usize,
        style: ValueStyle,
    ) -> Result<String, ParseError> {
        let mut output = String::with_capacity(raw.len());
        let mut position = 0;

        while position < raw.len() {
            let character = next_char(raw, position);

            if character == '\\' {
                position += character.len_utf8();
                let Some(escaped) = raw[position..].chars().next() else {
                    output.push('\\');
                    break;
                };
                position += escaped.len_utf8();
                push_escape(&mut output, style, escaped);
                continue;
            }

            if character == '$'
                && self.options.substitution
                && !matches!(style, ValueStyle::SingleQuoted)
            {
                position += character.len_utf8();
                self.expand_dollar(raw, &mut position, source_offset, style, &mut output)?;
                continue;
            }

            if character == '\r' {
                position += character.len_utf8();
                if raw[position..].starts_with('\n') {
                    position += '\n'.len_utf8();
                }
                output.push('\n');
                continue;
            }

            output.push(character);
            position += character.len_utf8();
        }

        Ok(output)
    }

    fn expand_dollar(
        &self,
        raw: &str,
        position: &mut usize,
        source_offset: usize,
        style: ValueStyle,
        output: &mut String,
    ) -> Result<(), ParseError> {
        if raw[*position..].starts_with('$') {
            *position += '$'.len_utf8();
            output.push('$');
            return Ok(());
        }

        if raw[*position..].starts_with('{') {
            let expression_start = *position + '{'.len_utf8();
            let expression_end =
                Self::find_substitution_end(raw, expression_start).ok_or_else(|| {
                    self.error(
                        source_offset + (*position).saturating_sub(1),
                        ParseErrorKind::UnterminatedSubstitution,
                    )
                })?;
            let expression = &raw[expression_start..expression_end];
            // Braced text that is not a substitution (`${VAR:=x}`, `${}`, ...) stays literal
            // rather than failing the whole file; the dotenv parser mise used before did the same.
            match self.evaluate_substitution(expression, source_offset + expression_start, style) {
                Ok(expanded) => output.push_str(&expanded),
                Err(error) if error.kind == ParseErrorKind::InvalidSubstitution => {
                    output.push_str("${");
                    output.push_str(expression);
                    output.push('}');
                }
                Err(error) => return Err(error),
            }
            *position = expression_end + '}'.len_utf8();
            return Ok(());
        }

        let name_start = *position;
        let Some(first) = raw[*position..].chars().next() else {
            output.push('$');
            return Ok(());
        };
        if !is_substitution_start(first) {
            output.push('$');
            return Ok(());
        }

        *position += first.len_utf8();
        while let Some(character) = raw[*position..].chars().next() {
            if !is_substitution_continue(character) {
                break;
            }
            *position += character.len_utf8();
        }

        let name = &raw[name_start..*position];
        if let Some(value) = self.variable(name) {
            output.push_str(value);
        }
        Ok(())
    }

    fn find_substitution_end(raw: &str, mut position: usize) -> Option<usize> {
        let mut depth = 1_usize;
        while position < raw.len() {
            if raw[position..].starts_with("${") {
                depth += 1;
                position += 2;
                continue;
            }

            let character = next_char(raw, position);
            if character == '\\' {
                position += character.len_utf8();
                if let Some(next) = raw[position..].chars().next() {
                    position += next.len_utf8();
                }
                continue;
            }
            if character == '}' {
                depth -= 1;
                if depth == 0 {
                    return Some(position);
                }
            }
            position += character.len_utf8();
        }
        None
    }

    fn evaluate_substitution(
        &self,
        expression: &str,
        source_offset: usize,
        style: ValueStyle,
    ) -> Result<String, ParseError> {
        if expression.is_empty() {
            return Err(self.error(source_offset, ParseErrorKind::InvalidSubstitution));
        }

        // Braces can address non-shell names when an exact variable is present.
        if let Some(value) = self.variable(expression) {
            return Ok(value.to_owned());
        }

        let mut name_end = 0;
        for (index, character) in expression.char_indices() {
            let valid = if index == 0 {
                is_substitution_start(character)
            } else {
                is_substitution_continue(character)
            };
            if !valid {
                break;
            }
            name_end = index + character.len_utf8();
        }
        if name_end == 0 {
            return Err(self.error(source_offset, ParseErrorKind::InvalidSubstitution));
        }

        let name = &expression[..name_end];
        let remainder = &expression[name_end..];
        if remainder.is_empty() {
            return Ok(self.variable(name).unwrap_or_default().to_owned());
        }

        let (operator, word) = [":-", ":?", ":+", "-", "?", "+"]
            .into_iter()
            .find_map(|operator| {
                remainder
                    .strip_prefix(operator)
                    .map(|word| (operator, word))
            })
            .ok_or_else(|| {
                self.error(
                    source_offset + name_end,
                    ParseErrorKind::InvalidSubstitution,
                )
            })?;

        let value = self.variable(name);
        let is_set = value.is_some();
        let is_nonempty = value.is_some_and(|value| !value.is_empty());
        let word_offset = source_offset + name_end + operator.len();
        let expand_word = || self.expand(word, word_offset, style);

        match operator {
            "-" if !is_set => expand_word(),
            ":-" if !is_nonempty => expand_word(),
            "+" if is_set => expand_word(),
            ":+" if is_nonempty => expand_word(),
            "+" | ":+" => Ok(String::new()),
            "?" if !is_set => {
                let message = expand_word()?;
                Err(self.error(
                    source_offset,
                    ParseErrorKind::RequiredVariable {
                        name: name.to_owned(),
                        message,
                    },
                ))
            }
            ":?" if !is_nonempty => {
                let message = expand_word()?;
                Err(self.error(
                    source_offset,
                    ParseErrorKind::RequiredVariable {
                        name: name.to_owned(),
                        message,
                    },
                ))
            }
            "-" | ":-" | "?" | ":?" => Ok(value.unwrap_or_default().to_owned()),
            _ => unreachable!("all substitution operators are handled"),
        }
    }

    fn variable(&self, name: &str) -> Option<&str> {
        if let Some(tracker) = &self.substitution_tracker {
            tracker.borrow_mut().insert(name.to_owned());
        }

        let value = if self.input_overrides {
            crate::key::get(&self.parsed_variables, name)
                .or_else(|| crate::key::get(&self.base_variables, name))
                .or_else(|| crate::key::get(&self.protected_variables, name))
        } else {
            crate::key::get(&self.protected_variables, name)
                .or_else(|| crate::key::get(&self.parsed_variables, name))
                .or_else(|| crate::key::get(&self.base_variables, name))
        };
        value.map(String::as_str)
    }

    fn error(&self, byte_offset: usize, kind: ParseErrorKind) -> ParseError {
        let byte_offset = byte_offset.min(self.source.len());
        let (line, line_start) = source_line_and_start(&self.source, byte_offset);
        let line_end = self.source[line_start..]
            .find(['\n', '\r'])
            .map_or(self.source.len(), |position| line_start + position);
        let column = self.source[line_start..byte_offset].chars().count() + 1;

        ParseError {
            kind,
            line,
            column,
            byte_offset,
            source_line: self.source[line_start..line_end].to_owned(),
        }
    }

    fn skip_horizontal_whitespace(&mut self) {
        while self.peek().is_some_and(is_horizontal_whitespace) {
            self.advance();
        }
    }

    fn skip_comment(&mut self) {
        while self
            .peek()
            .is_some_and(|character| !matches!(character, '\n' | '\r'))
        {
            self.advance();
        }
        self.consume_newline();
    }

    fn consume_newline(&mut self) -> bool {
        match self.peek() {
            Some('\r') => {
                self.advance();
                if self.peek() == Some('\n') {
                    self.advance();
                }
                true
            }
            Some('\n') => {
                self.advance();
                true
            }
            _ => false,
        }
    }

    fn peek(&self) -> Option<char> {
        self.source[self.position..].chars().next()
    }

    fn advance(&mut self) {
        if let Some(character) = self.peek() {
            self.position += character.len_utf8();
        }
    }

    fn at_end(&self) -> bool {
        self.position == self.source.len()
    }
}

fn source_line_and_start(source: &str, byte_offset: usize) -> (usize, usize) {
    let bytes = source.as_bytes();
    let mut line = 1;
    let mut line_start = 0;
    let mut position = 0;

    while position < byte_offset {
        match bytes[position] {
            b'\r' => {
                if position + 1 < byte_offset && bytes[position + 1] == b'\n' {
                    position += 1;
                }
                line += 1;
                line_start = position + 1;
            }
            b'\n' => {
                line += 1;
                line_start = position + 1;
            }
            _ => {}
        }
        position += 1;
    }

    (line, line_start)
}

fn next_char(value: &str, position: usize) -> char {
    value[position..]
        .chars()
        .next()
        .expect("position must be on a character boundary before the end")
}

pub fn is_horizontal_whitespace(character: char) -> bool {
    character.is_whitespace() && !matches!(character, '\n' | '\r')
}

fn trim_unquoted_trailing_whitespace(value: &str) -> &str {
    let mut end = value.len();

    while let Some((whitespace_start, character)) = value[..end].char_indices().next_back() {
        if !is_horizontal_whitespace(character) {
            break;
        }

        let preceding_backslashes = value[..whitespace_start]
            .chars()
            .rev()
            .take_while(|character| *character == '\\')
            .count();
        if preceding_backslashes % 2 == 1 {
            break;
        }
        end = whitespace_start;
    }

    &value[..end]
}

const fn is_substitution_start(character: char) -> bool {
    character.is_ascii_alphabetic() || character == '_'
}

const fn is_substitution_continue(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '_'
}

fn push_escape(output: &mut String, style: ValueStyle, escaped: char) {
    let decoded = match style {
        ValueStyle::DoubleQuoted => match escaped {
            'a' => Some('\u{0007}'),
            'b' => Some('\u{0008}'),
            'f' => Some('\u{000c}'),
            'n' => Some('\n'),
            'r' => Some('\r'),
            't' => Some('\t'),
            'v' => Some('\u{000b}'),
            '\\' | '\'' | '"' | '$' => Some(escaped),
            _ => None,
        },
        ValueStyle::SingleQuoted => match escaped {
            '\\' | '\'' => Some(escaped),
            _ => None,
        },
        ValueStyle::Unquoted => match escaped {
            '\\' | '\'' | '"' | '$' | '#' | ' ' | '\t' => Some(escaped),
            _ => None,
        },
    };

    if let Some(decoded) = decoded {
        output.push(decoded);
    } else {
        output.push('\\');
        output.push(escaped);
    }
}

#[cfg(test)]
mod tests {
    use super::{ParseErrorKind, ParseOptions, Parser};
    use std::collections::HashMap;

    fn parse(input: &str) -> Result<Vec<(String, String)>, super::ParseError> {
        parse_with_options(input, ParseOptions::new())
    }

    fn parse_with_options(
        input: &str,
        options: ParseOptions,
    ) -> Result<Vec<(String, String)>, super::ParseError> {
        Parser::new(
            input.to_owned(),
            options,
            HashMap::new(),
            HashMap::new(),
            None,
            true,
        )
        .collect()
    }

    impl Iterator for Parser {
        type Item = Result<(String, String), super::ParseError>;

        fn next(&mut self) -> Option<Self::Item> {
            self.next_entry()
        }
    }

    #[test]
    fn parses_common_dotenv_syntax() {
        let parsed = parse(
            r##"
# comment
KEY=1
KEY2 = "two words"
KEY3='literal $KEY'
KEY4=unquoted values may contain spaces
KEY5={ "json": "works", "hash": "# literal" }
EMPTY=
export EXPORTED=yes
export=also-a-key
"##,
        )
        .unwrap();

        assert_eq!(
            parsed,
            [
                ("KEY", "1"),
                ("KEY2", "two words"),
                ("KEY3", "literal $KEY"),
                ("KEY4", "unquoted values may contain spaces"),
                ("KEY5", r##"{ "json": "works", "hash": "# literal" }"##),
                ("EMPTY", ""),
                ("EXPORTED", "yes"),
                ("export", "also-a-key"),
            ]
            .map(|(key, value)| (key.to_owned(), value.to_owned()))
        );
    }

    #[test]
    fn comments_only_start_outside_quotes_after_whitespace() {
        let parsed = parse(
            r##"
A=value#literal
B=value # comment with ' and "
C="value # literal" # comment
D='value # literal' # comment
E=before "# literal" after # comment
"##,
        )
        .unwrap();

        assert_eq!(
            parsed,
            [
                ("A", "value#literal"),
                ("B", "value"),
                ("C", "value # literal"),
                ("D", "value # literal"),
                ("E", "before \"# literal\" after"),
            ]
            .map(|(key, value)| (key.to_owned(), value.to_owned()))
        );
    }

    #[test]
    fn supports_multiline_quotes_and_normalizes_crlf() {
        let parsed = parse("A=\"one\r\ntwo\"\r\nB='three\nfour'\n").unwrap();
        assert_eq!(
            parsed,
            [
                ("A".to_owned(), "one\ntwo".to_owned()),
                ("B".to_owned(), "three\nfour".to_owned()),
            ]
        );
    }

    #[test]
    fn unknown_escapes_are_preserved_for_paths_and_structured_values() {
        let parsed = parse(
            r#"
WINDOWS=C:\Users\me\Desktop
REGEX="\d+\s+"
ESCAPED="line one\nline two\t\\\""
"#,
        )
        .unwrap();
        assert_eq!(parsed[0].1, r"C:\Users\me\Desktop");
        assert_eq!(parsed[1].1, r"\d+\s+");
        assert_eq!(parsed[2].1, "line one\nline two\t\\\"");
    }

    #[test]
    fn accepts_environment_names_beyond_shell_identifiers() {
        let parsed = parse("SERVICE-NAME=value\n%TEMP%=tmp\n1NUMBER=one\nключ=значение\n").unwrap();
        assert_eq!(parsed.len(), 4);
        assert_eq!(parsed[0], ("SERVICE-NAME".to_owned(), "value".to_owned()));
        assert_eq!(parsed[3], ("ключ".to_owned(), "значение".to_owned()));
    }

    #[test]

    fn substitutes_names_with_underscores_and_shell_operators() {
        let parsed = parse_with_options(
            r"
FOO_BAR=test
EMPTY=
DIRECT=$FOO_BAR
BRACED=${FOO_BAR}
DEFAULT=${MISSING:-fallback}
EMPTY_DEFAULT=${EMPTY:-fallback}
SET_DEFAULT=${EMPTY-fallback}
ALTERNATE=${FOO_BAR:+yes}
LITERAL=$$FOO_BAR
",
            ParseOptions::new().substitution(true),
        )
        .unwrap();
        let values: HashMap<_, _> = parsed.into_iter().collect();

        assert_eq!(values["DIRECT"], "test");
        assert_eq!(values["BRACED"], "test");
        assert_eq!(values["DEFAULT"], "fallback");
        assert_eq!(values["EMPTY_DEFAULT"], "fallback");
        assert_eq!(values["SET_DEFAULT"], "");
        assert_eq!(values["ALTERNATE"], "yes");
        assert_eq!(values["LITERAL"], "$FOO_BAR");
    }

    #[test]
    fn required_substitution_is_a_source_aware_error() {
        let error = parse_with_options(
            "OK=yes\nVALUE=${MISSING:?configure it}\n",
            ParseOptions::new().substitution(true),
        )
        .unwrap_err();
        assert_eq!(error.line(), 2);
        assert_eq!(error.column(), 9);
        assert_eq!(error.source_line(), "VALUE=${MISSING:?configure it}");
        assert!(matches!(
            error.kind(),
            ParseErrorKind::RequiredVariable { name, message }
                if name == "MISSING" && message == "configure it"
        ));
    }

    #[test]
    fn substitution_is_disabled_by_default() {
        assert!(!ParseOptions::new().substitution_enabled());
        assert!(!ParseOptions::default().substitution_enabled());
        assert!(ParseOptions::new().multiline_enabled());
        assert!(!ParseOptions::new().multiline(false).multiline_enabled());

        let parsed = parse("A=value\nB=$A/${A}/$$\n").unwrap();
        assert_eq!(parsed[1].1, "$A/${A}/$$");
    }

    #[test]
    fn explicit_base_variables_do_not_require_process_environment_mutation() {
        let mut base = HashMap::new();
        base.insert("APP_VAR".to_owned(), "provided".to_owned());
        let parser = Parser::new(
            "VALUE=${APP_VAR}-from-file".to_owned(),
            ParseOptions::new().substitution(true),
            base,
            HashMap::new(),
            None,
            true,
        );
        let parsed: Result<Vec<_>, _> = parser.collect();
        assert_eq!(parsed.unwrap()[0].1, "provided-from-file");
    }

    #[test]
    fn substitution_precedence_is_explicit() {
        let mut base = HashMap::new();
        base.insert("VALUE".to_owned(), "environment".to_owned());

        let input_wins: Result<Vec<_>, _> = Parser::new(
            "VALUE=input\nRESULT=$VALUE".to_owned(),
            ParseOptions::new().substitution(true),
            base.clone(),
            HashMap::new(),
            None,
            true,
        )
        .collect();
        assert_eq!(input_wins.unwrap()[1].1, "input");

        let base_wins: Result<Vec<_>, _> = Parser::new(
            "VALUE=input\nRESULT=$VALUE".to_owned(),
            ParseOptions::new().substitution(true),
            HashMap::new(),
            base,
            None,
            false,
        )
        .collect();
        assert_eq!(base_wins.unwrap()[1].1, "environment");
    }

    #[test]
    fn rejects_multiline_when_disabled() {
        let mut parser = Parser::new(
            "VALUE=\"first\nsecond\"".to_owned(),
            ParseOptions::new().multiline(false),
            HashMap::new(),
            HashMap::new(),
            None,
            true,
        );
        let error = parser.next().unwrap().unwrap_err();
        assert_eq!(error.line(), 1);
        assert_eq!(error.column(), 13);
        assert_eq!(error.kind(), &ParseErrorKind::MultilineDisabled);
    }

    #[test]
    fn reports_the_whole_assignment_with_character_column() {
        let error = parse("ключ value\n").unwrap_err();
        assert_eq!(error.line(), 1);
        assert_eq!(error.column(), 6);
        assert_eq!(error.byte_offset(), 9);
        assert_eq!(error.kind(), &ParseErrorKind::MissingEquals);
        assert_eq!(error.source_line(), "ключ value");
    }

    #[test]
    fn strips_a_utf8_bom_from_every_entry_point() {
        let parsed = parse("\u{feff}KEY=value\n").unwrap();
        assert_eq!(parsed, [("KEY".to_owned(), "value".to_owned())]);
    }

    #[test]
    fn rejects_nul_in_keys_and_values_before_environment_mutation() {
        let key_error = parse("BAD\0KEY=value").unwrap_err();
        assert_eq!(key_error.kind(), &ParseErrorKind::InvalidKeyCharacter('\0'));

        let value_error = parse("KEY=bad\0value").unwrap_err();
        assert_eq!(
            value_error.kind(),
            &ParseErrorKind::InvalidValueCharacter('\0')
        );
    }

    #[test]
    fn preserves_escaped_trailing_whitespace() {
        let parsed = parse("A=one\\ \nB=two\\\\ \nC=three\\ \t\nD=four\\  # comment\n").unwrap();
        assert_eq!(parsed[0].1, "one ");
        assert_eq!(parsed[1].1, "two\\");
        assert_eq!(parsed[2].1, "three ");
        assert_eq!(parsed[3].1, "four ");
    }

    #[test]
    fn reports_each_structural_error_kind() {
        assert_eq!(
            parse("=value").unwrap_err().kind(),
            &ParseErrorKind::MissingKey
        );
        assert_eq!(
            parse("KEY value").unwrap_err().kind(),
            &ParseErrorKind::MissingEquals
        );
        assert_eq!(
            parse("KEY=\"value\"tail").unwrap_err().kind(),
            &ParseErrorKind::TrailingCharacters
        );
        assert_eq!(
            parse("KEY='value").unwrap_err().kind(),
            &ParseErrorKind::UnterminatedQuote('\'')
        );
    }

    #[test]
    fn parser_stops_after_the_first_error() {
        let mut parser = Parser::new(
            "BROKEN value\nVALID=yes\n".to_owned(),
            ParseOptions::new(),
            HashMap::new(),
            HashMap::new(),
            None,
            true,
        );

        assert!(parser.next().unwrap().is_err());
        assert!(parser.next().is_none());
    }

    #[test]
    fn error_kind_messages_are_actionable() {
        let cases = [
            (
                ParseErrorKind::MissingKey,
                "expected an environment variable name",
            ),
            (
                ParseErrorKind::MissingEquals,
                "expected `=` after the environment variable name",
            ),
            (
                ParseErrorKind::InvalidKeyCharacter('\0'),
                "environment variable name contains invalid character '\\0'",
            ),
            (
                ParseErrorKind::InvalidValueCharacter('\0'),
                "environment variable value contains invalid character '\\0'",
            ),
            (
                ParseErrorKind::TrailingCharacters,
                "expected a comment or end of line after the quoted value",
            ),
            (
                ParseErrorKind::UnterminatedQuote('"'),
                "unterminated '\"' quoted value",
            ),
            (
                ParseErrorKind::UnterminatedSubstitution,
                "unterminated `${...}` substitution",
            ),
            (
                ParseErrorKind::InvalidSubstitution,
                "invalid variable substitution",
            ),
            (
                ParseErrorKind::RequiredVariable {
                    name: "TOKEN".to_owned(),
                    message: String::new(),
                },
                "required variable `TOKEN` is not set",
            ),
            (
                ParseErrorKind::RequiredVariable {
                    name: "TOKEN".to_owned(),
                    message: "provide one".to_owned(),
                },
                "required variable `TOKEN` is not set: provide one",
            ),
            (
                ParseErrorKind::MultilineDisabled,
                "multiline quoted values are disabled",
            ),
        ];

        for (kind, expected) in cases {
            assert_eq!(kind.to_string(), expected);
        }
    }

    #[test]
    fn parses_empty_commented_values_and_export_as_a_regular_key() {
        assert_eq!(
            parse("EMPTY=# intentionally empty\n").unwrap(),
            [("EMPTY".to_owned(), String::new())]
        );

        let error = parse("export").unwrap_err();
        assert_eq!(error.kind(), &ParseErrorKind::MissingEquals);
        assert_eq!(error.column(), 7);
    }

    #[test]
    fn rejects_nul_inside_quoted_values() {
        let error = parse("KEY=\"before\0after\"").unwrap_err();
        assert_eq!(error.kind(), &ParseErrorKind::InvalidValueCharacter('\0'));
        assert_eq!(error.column(), 12);
    }

    #[test]
    fn unquoted_embedded_quotes_honor_escaped_quotes() {
        let parsed = parse(r#"KEY=before "a\"b" after"#).unwrap();
        assert_eq!(parsed[0].1, r#"before "a"b" after"#);
    }

    #[test]
    fn preserves_literal_dollars_and_a_terminal_backslash() {
        let parsed = parse_with_options(
            "TRAILING_DOLLAR=$\nDIGIT=$9\nBACKSLASH=value\\",
            ParseOptions::new().substitution(true),
        )
        .unwrap();
        assert_eq!(parsed[0].1, "$");
        assert_eq!(parsed[1].1, "$9");
        assert_eq!(parsed[2].1, "value\\");
    }

    #[test]
    fn escaped_closing_braces_do_not_end_nested_substitutions() {
        let parsed = parse_with_options(
            r"VALUE=${MISSING:-left\}right}",
            ParseOptions::new().substitution(true),
        )
        .unwrap();
        assert_eq!(parsed[0].1, r"left\}right");
    }

    #[test]
    fn keeps_an_empty_braced_substitution_literal() {
        let parsed =
            parse_with_options("VALUE=${}", ParseOptions::new().substitution(true)).unwrap();
        assert_eq!(parsed[0].1, "${}");
    }

    #[test]
    fn reports_errors_after_crlf_on_the_correct_line() {
        let error = parse("FIRST=ok\r\nSECOND value").unwrap_err();
        assert_eq!(error.line(), 2);
        assert_eq!(error.column(), 8);
        assert_eq!(error.source_line(), "SECOND value");
    }

    #[test]
    fn decodes_all_documented_double_quote_control_escapes() {
        let parsed = parse(r#"VALUE="\a\b\f\r\v""#).unwrap();
        assert_eq!(parsed[0].1, "\u{0007}\u{0008}\u{000c}\r\u{000b}");
    }

    #[test]
    fn keeps_invalid_substitutions_literal_and_reports_unterminated_ones() {
        let options = ParseOptions::new().substitution(true);
        assert_eq!(
            parse_with_options("KEY=${VALUE", options)
                .unwrap_err()
                .kind(),
            &ParseErrorKind::UnterminatedSubstitution
        );
        assert_eq!(
            parse_with_options("KEY=${9VALUE}", options).unwrap()[0].1,
            "${9VALUE}"
        );
        assert_eq!(
            parse_with_options("KEY=${VALUE:=default}", options).unwrap()[0].1,
            "${VALUE:=default}"
        );
    }

    #[test]
    fn supports_nested_and_all_alternative_substitution_forms() {
        let parsed = parse_with_options(
            "SET=value\nEMPTY=\n\
             NESTED=${MISSING:-${ALSO_MISSING:-fallback}}\n\
             SET_PLUS=${SET+alternate}\n\
             SET_COLON_PLUS=${SET:+alternate}\n\
             EMPTY_PLUS=${EMPTY+alternate}\n\
             EMPTY_COLON_PLUS=${EMPTY:+alternate}\n\
             EMPTY_QUESTION=${EMPTY?message}\n",
            ParseOptions::new().substitution(true),
        )
        .unwrap();
        let values: HashMap<_, _> = parsed.into_iter().collect();
        assert_eq!(values["NESTED"], "fallback");
        assert_eq!(values["SET_PLUS"], "alternate");
        assert_eq!(values["SET_COLON_PLUS"], "alternate");
        assert_eq!(values["EMPTY_PLUS"], "alternate");
        assert_eq!(values["EMPTY_COLON_PLUS"], "");
        assert_eq!(values["EMPTY_QUESTION"], "");
    }

    #[test]
    fn required_substitution_distinguishes_unset_from_empty() {
        let options = ParseOptions::new().substitution(true);
        let unset = parse_with_options("KEY=${MISSING?set it}", options).unwrap_err();
        assert!(matches!(
            unset.kind(),
            ParseErrorKind::RequiredVariable { name, message }
                if name == "MISSING" && message == "set it"
        ));

        let empty =
            parse_with_options("EMPTY=\nKEY=${EMPTY:?must not be empty}", options).unwrap_err();
        assert!(matches!(
            empty.kind(),
            ParseErrorKind::RequiredVariable { name, message }
                if name == "EMPTY" && message == "must not be empty"
        ));
    }
}
