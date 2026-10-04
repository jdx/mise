use crate::{
    EnvMap, ParseError, ParseOptions,
    parse::{Parser, SubstitutionTracker},
};
use std::{
    collections::HashMap,
    io::{self, Read},
};

pub struct Iter<B> {
    reader: Option<B>,
    parser: Option<Parser>,
    options: ParseOptions,
    base_variables: HashMap<String, String>,
    protected_variables: HashMap<String, String>,
    substitution_tracker: Option<SubstitutionTracker>,
    input_overrides: bool,
    read_error: Option<io::Error>,
}

impl<B: Read> Iter<B> {
    pub(super) const fn new(
        reader: B,
        options: ParseOptions,
        base_variables: HashMap<String, String>,
        protected_variables: HashMap<String, String>,
        substitution_tracker: Option<SubstitutionTracker>,
        input_overrides: bool,
    ) -> Self {
        Self {
            reader: Some(reader),
            parser: None,
            options,
            base_variables,
            protected_variables,
            substitution_tracker,
            input_overrides,
            read_error: None,
        }
    }

    fn initialize(&mut self) {
        if let Some(mut reader) = self.reader.take() {
            let mut source = String::new();
            match reader.read_to_string(&mut source) {
                Ok(_) => {
                    self.parser = Some(Parser::new(
                        source,
                        self.options,
                        std::mem::take(&mut self.base_variables),
                        std::mem::take(&mut self.protected_variables),
                        self.substitution_tracker.take(),
                        self.input_overrides,
                    ));
                }
                Err(error) => self.read_error = Some(error),
            }
        }
    }

    fn internal_load(mut self) -> Result<EnvMap, ParseBufError> {
        let mut map = EnvMap::new();
        for item in &mut self {
            let (key, value) = item?;
            map.insert(key, value);
        }
        Ok(map)
    }

    pub(super) fn load(self) -> Result<EnvMap, ParseBufError> {
        self.internal_load()
    }
}

impl<B: Read> Iterator for Iter<B> {
    type Item = Result<(String, String), ParseBufError>;

    fn next(&mut self) -> Option<Self::Item> {
        self.initialize();

        if let Some(error) = self.read_error.take() {
            return Some(Err(ParseBufError::Io(error)));
        }

        let parser = self.parser.as_mut()?;
        let result = parser.next_entry()?;
        Some(result.map_err(ParseBufError::Parse))
    }
}

/// An internal error that does not yet have path context.
#[derive(Debug)]
pub enum ParseBufError {
    Parse(ParseError),
    Io(io::Error),
}

impl From<io::Error> for ParseBufError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

#[cfg(test)]
mod tests {
    use super::{Iter, ParseBufError};
    use crate::ParseOptions;
    use std::{collections::HashMap, io};

    #[test]
    fn iterator_strips_bom() {
        let input = b"\xef\xbb\xbfkey=value\n";
        let entries: Result<Vec<_>, _> = Iter::new(
            &input[..],
            ParseOptions::new(),
            HashMap::new(),
            HashMap::new(),
            None,
            true,
        )
        .collect();
        assert_eq!(entries.unwrap(), [("key".to_owned(), "value".to_owned())]);
    }

    #[test]
    fn read_errors_are_returned_once() {
        struct BrokenReader;

        impl io::Read for BrokenReader {
            fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::other("broken reader"))
            }
        }

        let mut iter = Iter::new(
            BrokenReader,
            ParseOptions::new(),
            HashMap::new(),
            HashMap::new(),
            None,
            true,
        );
        assert!(matches!(iter.next(), Some(Err(ParseBufError::Io(_)))));
        assert!(iter.next().is_none());
    }

    #[test]
    fn successful_iterators_remain_exhausted() {
        let mut iter = Iter::new(
            &b"KEY=value\n"[..],
            ParseOptions::new(),
            HashMap::new(),
            HashMap::new(),
            None,
            true,
        );

        assert_eq!(
            iter.next().unwrap().unwrap(),
            ("KEY".to_owned(), "value".to_owned())
        );
        assert!(iter.next().is_none());
        assert!(iter.next().is_none());
    }

    #[test]
    fn io_errors_convert_to_internal_buffer_errors() {
        let error = ParseBufError::from(io::Error::other("reader failed"));
        let ParseBufError::Io(error) = error else {
            panic!("expected an I/O error");
        };
        assert_eq!(error.to_string(), "reader failed");
    }
}
