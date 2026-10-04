use std::{error, ffi::OsString, fmt, io, path::PathBuf};

use crate::ParseError;
use crate::iter::ParseBufError;

#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// The input did not conform to the supported dotenv syntax.
    Parse(Box<ParseError>, Option<PathBuf>),
    /// An IO error may be encountered when reading from a file or reader.
    Io(io::Error, Option<PathBuf>),
    /// The variable was not found in the environment. The `String` is the name of the variable.
    NotPresent(String),
    /// The variable was not valid unicode. The `String` is the name of the variable.
    NotUnicode(OsString, String),
    /// An environment variable name was not valid Unicode.
    NotUnicodeName(OsString),
    /// When a load function is called with no path or reader.
    ///
    /// This can occur when `EnvLoader::with_paths` or `EnvLoader::with_readers` receives an empty
    /// iterator.
    NoInput,
}

impl Error {
    #[must_use]
    pub fn not_found(&self) -> bool {
        if let Self::Io(e, _) = self {
            e.kind() == io::ErrorKind::NotFound
        } else {
            false
        }
    }
}

impl error::Error for Error {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::Io(e, _) => Some(e),
            Self::Parse(error, _) => Some(error),
            Self::NotPresent(_)
            | Self::NotUnicode(_, _)
            | Self::NotUnicodeName(_)
            | Self::NoInput => None,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Io(e, path) => {
                if let Some(path) = path {
                    write!(f, "error reading '{}': {e}", path.to_string_lossy())
                } else {
                    e.fmt(f)
                }
            }
            Self::Parse(error, Some(path)) => {
                write!(f, "error parsing '{}':\n{error}", path.to_string_lossy())
            }
            Self::Parse(error, None) => error.fmt(f),
            Self::NotPresent(s) => write!(f, "{s} is not set"),
            Self::NotUnicode(os_str, s) => {
                write!(f, "{s} is not valid Unicode: {os_str:?}")
            }
            Self::NotUnicodeName(os_str) => {
                write!(
                    f,
                    "environment variable name is not valid Unicode: {os_str:?}"
                )
            }
            Self::NoInput => write!(f, "no input provided"),
        }
    }
}

impl From<(io::Error, PathBuf)> for Error {
    fn from((e, path): (io::Error, PathBuf)) -> Self {
        Self::Io(e, Some(path))
    }
}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Self::Io(error, None)
    }
}

impl From<(ParseBufError, Option<PathBuf>)> for Error {
    fn from((e, path): (ParseBufError, Option<PathBuf>)) -> Self {
        match e {
            ParseBufError::Parse(error) => Self::Parse(Box::new(error), path),
            ParseBufError::Io(e) => Self::Io(e, path),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Error;
    use crate::{EnvLoader, EnvSequence, iter::ParseBufError};
    use std::{
        error::Error as StdError,
        ffi::OsString,
        io::{self, Cursor},
        path::PathBuf,
    };

    fn parse_error() -> Box<crate::ParseError> {
        let error = EnvLoader::with_reader(Cursor::new("BROKEN value"))
            .sequence(EnvSequence::InputOnly)
            .load()
            .unwrap_err();
        let Error::Parse(error, _) = error else {
            panic!("expected a parse error");
        };
        error
    }

    #[test]
    fn reports_underlying_error_sources() {
        let io_error = Error::from(io::Error::other("reader failed"));
        assert_eq!(io_error.source().unwrap().to_string(), "reader failed");

        let parse_error = Error::Parse(parse_error(), None);
        assert!(parse_error.source().is_some());

        assert!(Error::NotPresent("MISSING".to_owned()).source().is_none());
    }

    #[test]
    fn displays_every_context_free_error_variant() {
        assert_eq!(
            Error::from(io::Error::other("reader failed")).to_string(),
            "reader failed"
        );
        assert!(
            Error::Parse(parse_error(), None)
                .to_string()
                .starts_with("dotenv syntax error at line 1, column 8")
        );
        assert_eq!(
            Error::NotPresent("MISSING".to_owned()).to_string(),
            "MISSING is not set"
        );
        assert_eq!(
            Error::NotUnicode(OsString::from("raw-value"), "VALUE".to_owned()).to_string(),
            "VALUE is not valid Unicode: \"raw-value\""
        );
        assert_eq!(
            Error::NotUnicodeName(OsString::from("raw-name")).to_string(),
            "environment variable name is not valid Unicode: \"raw-name\""
        );
        assert_eq!(Error::NoInput.to_string(), "no input provided");
        assert!(!Error::NoInput.not_found());
    }

    #[test]
    fn converts_reader_errors_with_optional_path_context() {
        let error = Error::from((
            ParseBufError::Io(io::Error::other("reader failed")),
            Some(PathBuf::from("config/.env")),
        ));
        assert_eq!(
            error.to_string(),
            "error reading 'config/.env': reader failed"
        );
    }
}
