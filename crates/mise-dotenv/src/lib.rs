#![deny(clippy::uninlined_format_args, clippy::wildcard_imports)]

//! Shared dotenv parsing and loading implementation.
//!
//! This library allows for loading environment variables from an env file or a reader.
use crate::{iter::Iter, parse::SubstitutionTracker};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    env::{self, VarError},
    fs::File,
    io::{BufReader, Read},
    ops::{Deref, DerefMut},
    path::{Path, PathBuf},
    rc::Rc,
};

mod err;
mod iter;
mod key;
mod parse;
mod render;

pub use crate::parse::{ParseError, ParseErrorKind, ParseOptions};
pub use crate::render::{RenderError, render, render_value, render_var};

/// A map of environment variables.
///
/// This is a newtype around `HashMap<String, String>`. Its name-based operations follow the
/// platform's environment semantics: case-insensitive on Windows and case-sensitive elsewhere.
#[derive(Default, Clone, Debug, PartialEq, Eq)]
pub struct EnvMap(HashMap<String, String>);

impl Deref for EnvMap {
    type Target = HashMap<String, String>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for EnvMap {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl FromIterator<(String, String)> for EnvMap {
    fn from_iter<I: IntoIterator<Item = (String, String)>>(iter: I) -> Self {
        let mut map = Self::new();
        map.extend(iter);
        map
    }
}

impl Extend<(String, String)> for EnvMap {
    fn extend<I: IntoIterator<Item = (String, String)>>(&mut self, iter: I) {
        for (key, value) in iter {
            self.insert(key, value);
        }
    }
}

impl IntoIterator for EnvMap {
    type Item = (String, String);
    type IntoIter = std::collections::hash_map::IntoIter<String, String>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl EnvMap {
    #[must_use]
    pub fn new() -> Self {
        Self(HashMap::new())
    }

    /// Returns the value for an environment variable name.
    ///
    /// Names are case-insensitive on Windows and case-sensitive on other platforms, matching the
    /// operating system's process-environment behavior.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&String> {
        key::get(&self.0, key)
    }

    /// Returns whether the map contains an environment variable name.
    ///
    /// Names are case-insensitive on Windows and case-sensitive on other platforms.
    #[must_use]
    pub fn contains_key(&self, key: &str) -> bool {
        key::contains(&self.0, key)
    }

    /// Inserts an environment variable, returning the previous value if present.
    ///
    /// On Windows, an existing key that differs only by case is replaced instead of creating a
    /// second entry.
    pub fn insert(&mut self, key: String, value: String) -> Option<String> {
        key::insert(&mut self.0, key, value)
    }

    /// Removes an environment variable, returning its value if present.
    ///
    /// Names are case-insensitive on Windows and case-sensitive on other platforms.
    pub fn remove(&mut self, key: &str) -> Option<String> {
        key::remove(&mut self.0, key)
    }

    pub fn var(&self, key: &str) -> Result<String, crate::Error> {
        self.get(key)
            .cloned()
            .ok_or_else(|| Error::NotPresent(key.to_owned()))
    }
}

pub use crate::err::Error;

fn environment_variables() -> Result<EnvMap, Error> {
    env::vars_os()
        .map(|(name, value)| {
            let name = name.into_string().map_err(Error::NotUnicodeName)?;
            let value = value
                .into_string()
                .map_err(|value| Error::NotUnicode(value, name.clone()))?;
            Ok((name, value))
        })
        .collect()
}

/// Fetches the environment variable `key` from the current process.
///
/// This is `std::env::var` but with an error type of [`Error`].
/// [`Error`] uses `NotPresent(String)` instead of `NotPresent`, reporting the name of the missing key.
///
/// # Errors
///
/// This function will return an error if the environment variable isn't set.
///
/// This function may return an error if the environment variable's name contains
/// the equal sign character (`=`) or the NUL character.
///
/// This function will return an error if the environment variable's value is
/// not valid Unicode.
///
/// # Examples
///
/// ```
/// let key = "HOME";
/// match mise_dotenv::var(key) {
///     Ok(val) => println!("{key}: {val:?}"),
///     Err(e) => println!("couldn't interpret {key}: {e}"),
/// }
/// ```
pub fn var(key: &str) -> Result<String, crate::Error> {
    env::var(key).map_err(|e| match e {
        VarError::NotPresent => Error::NotPresent(key.to_owned()),
        VarError::NotUnicode(os_str) => Error::NotUnicode(os_str, key.to_owned()),
    })
}

/// Parses dotenv `content`, returning the assignments in file order.
///
/// With `substitution` enabled, `$NAME` and `${NAME}` resolve against earlier assignments in
/// `content` first and then against `substitutions`. The process environment is never consulted,
/// so ambient variables cannot shadow a value defined in the file itself.
pub fn parse<I>(
    content: &str,
    substitution: bool,
    substitutions: I,
) -> Result<Vec<(String, String)>, Error>
where
    I: IntoIterator<Item = (String, String)>,
{
    let mut protected = HashMap::new();
    for (key, value) in substitutions {
        key::insert(&mut protected, key, value);
    }
    let iter = Iter::new(
        content.as_bytes(),
        ParseOptions::new().substitution(substitution),
        HashMap::new(),
        protected,
        None,
        true,
    );
    iter.collect::<Result<Vec<_>, _>>()
        .map_err(|error| Error::from((error, None::<PathBuf>)))
}

/// Like [`parse`], but keeps the assignments read before a syntax error.
///
/// Returns every assignment that parsed cleanly, in file order, plus the error that stopped
/// parsing, if any. Nothing after the error is read.
pub fn parse_partial<I>(
    content: &str,
    substitution: bool,
    substitutions: I,
) -> (Vec<(String, String)>, Option<Error>)
where
    I: IntoIterator<Item = (String, String)>,
{
    let mut protected = HashMap::new();
    for (key, value) in substitutions {
        key::insert(&mut protected, key, value);
    }
    let iter = Iter::new(
        content.as_bytes(),
        ParseOptions::new().substitution(substitution),
        HashMap::new(),
        protected,
        None,
        true,
    );
    let mut items = Vec::new();
    for item in iter {
        match item {
            Ok(item) => items.push(item),
            Err(error) => return (items, Some(Error::from((error, None::<PathBuf>)))),
        }
    }
    (items, None)
}

/// The sequence in which to load environment variables.
///
/// Values in the latter override values in the former.
#[derive(Default, Debug, PartialEq, Eq, Clone, Copy)]
pub enum EnvSequence {
    /// Inherit the existing environment without loading from input.
    EnvOnly,
    /// Inherit the existing environment, and then load from input, overriding existing values.
    EnvThenInput,
    /// Load from input only.
    InputOnly,
    /// Load from input and then inherit the existing environment. Values in the existing environment are not overwritten.
    #[default]
    InputThenEnv,
}

enum Input<'a> {
    Path(PathBuf),
    Reader {
        reader: Box<dyn Read + 'a>,
        path: Option<PathBuf>,
    },
}

pub struct EnvLoader<'a> {
    inputs: Vec<Input<'a>>,
    sequence: EnvSequence,
    parse_options: ParseOptions,
    substitutions: EnvMap,
    substitution_tracker: Option<SubstitutionTracker>,
    ignore_missing: bool,
}

impl Default for EnvLoader<'_> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> EnvLoader<'a> {
    fn empty() -> Self {
        Self {
            inputs: Vec::new(),
            sequence: EnvSequence::default(),
            parse_options: ParseOptions::default(),
            substitutions: EnvMap::default(),
            substitution_tracker: None,
            ignore_missing: false,
        }
    }

    #[must_use]
    /// Creates a new `EnvLoader` with the path set to `./.env` in the current directory.
    pub fn new() -> Self {
        Self::with_path("./.env")
    }

    /// Creates a new `EnvLoader` with the path as input.
    ///
    /// This operation is infallible. IO is deferred until `load` is called.
    pub fn with_path<P: AsRef<Path>>(path: P) -> Self {
        Self {
            inputs: vec![Input::Path(path.as_ref().to_owned())],
            ..Self::empty()
        }
    }

    /// Creates a loader for multiple paths.
    ///
    /// Files are parsed in the supplied order. Later files override earlier files, and values
    /// from earlier files are available to substitutions in later files. All files are parsed
    /// successfully before [`load`](Self::load) returns.
    pub fn with_paths<I, P>(paths: I) -> Self
    where
        I: IntoIterator<Item = P>,
        P: AsRef<Path>,
    {
        Self {
            inputs: paths
                .into_iter()
                .map(|path| Input::Path(path.as_ref().to_owned()))
                .collect(),
            ..Self::empty()
        }
    }

    /// Creates a new `EnvLoader` with the reader as input.
    ///
    /// This operation is infallible. IO is deferred until `load` is called.
    pub fn with_reader<R: Read + 'a>(rdr: R) -> Self {
        Self {
            inputs: vec![Input::Reader {
                reader: Box::new(rdr),
                path: None,
            }],
            ..Self::empty()
        }
    }

    /// Creates a loader for multiple readers.
    ///
    /// Readers have the same ordering and substitution behavior as paths supplied to
    /// [`with_paths`](Self::with_paths).
    pub fn with_readers<I, R>(readers: I) -> Self
    where
        I: IntoIterator<Item = R>,
        R: Read + 'a,
    {
        Self {
            inputs: readers
                .into_iter()
                .map(|reader| Input::Reader {
                    reader: Box::new(reader) as Box<dyn Read + 'a>,
                    path: None,
                })
                .collect(),
            ..Self::empty()
        }
    }

    /// Sets the path to the specified path.
    ///
    /// This is useful when constructing with a reader, but still desiring a path to be used in the error message context.
    ///
    /// If a reader exists and a path is specified, loading will be done using the reader.
    #[must_use]
    pub fn path<P: AsRef<Path>>(mut self, path: P) -> Self {
        let path = path.as_ref().to_owned();
        if let [Input::Reader { path: context, .. }] = self.inputs.as_mut_slice() {
            *context = Some(path);
        } else {
            self.inputs = vec![Input::Path(path)];
        }
        self
    }

    /// Controls whether a missing path is an error.
    ///
    /// Missing paths are errors by default. When `required` is false, missing paths are skipped;
    /// parse errors and all other I/O errors are still returned.
    #[must_use]
    pub const fn required(mut self, required: bool) -> Self {
        self.ignore_missing = !required;
        self
    }

    /// Sets the sequence in which to load environment variables.
    #[must_use]
    pub const fn sequence(mut self, sequence: EnvSequence) -> Self {
        self.sequence = sequence;
        self
    }

    /// Enables or disables variable substitution while parsing input.
    ///
    /// Substitution is disabled by default, preserving dollar signs and
    /// substitution expressions literally.
    #[must_use]
    pub const fn substitution(mut self, enabled: bool) -> Self {
        self.parse_options = self.parse_options.substitution(enabled);
        self
    }

    /// Enables or disables physical newlines inside quoted values.
    ///
    /// Multiline quoted values are enabled by default.
    #[must_use]
    pub const fn multiline(mut self, enabled: bool) -> Self {
        self.parse_options = self.parse_options.multiline(enabled);
        self
    }

    /// Sets all parser options at once.
    #[must_use]
    pub const fn parse_options(mut self, options: ParseOptions) -> Self {
        self.parse_options = options;
        self
    }

    /// Adds values that are visible to substitution without modifying the process environment.
    ///
    /// Call [`substitution(true)`](Self::substitution) to enable interpolation. Explicit
    /// substitutions take precedence over process environment values. They are used only while
    /// parsing and are not included in the returned map unless the input defines them.
    #[must_use]
    pub fn substitutions<I, K, V>(mut self, values: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        self.substitutions.extend(
            values
                .into_iter()
                .map(|(key, value)| (key.into(), value.into())),
        );
        self
    }

    fn load_input(self, environment: &EnvMap) -> Result<EnvMap, crate::Error> {
        if self.inputs.is_empty() {
            return Err(Error::NoInput);
        }

        let protects_environment = match self.sequence {
            EnvSequence::EnvThenInput | EnvSequence::InputThenEnv => true,
            EnvSequence::EnvOnly | EnvSequence::InputOnly => false,
        };
        let mut protected_variables = if protects_environment {
            environment.0.clone()
        } else {
            HashMap::new()
        };
        for (key, value) in self.substitutions {
            key::insert(&mut protected_variables, key, value);
        }

        let mut accumulated = EnvMap::new();
        for input in self.inputs {
            let opened: Option<(Box<dyn Read + 'a>, _)> = match input {
                Input::Path(path) => match File::open(&path) {
                    Ok(file) => Some((Box::new(file), Some(path))),
                    Err(error)
                        if self.ignore_missing && error.kind() == std::io::ErrorKind::NotFound =>
                    {
                        None
                    }
                    Err(error) => return Err((error, path).into()),
                },
                Input::Reader { reader, path } => Some((reader, path)),
            };
            if let Some((reader, path)) = opened {
                let iter = Iter::new(
                    BufReader::new(reader),
                    self.parse_options,
                    accumulated.0.clone(),
                    protected_variables.clone(),
                    self.substitution_tracker.clone(),
                    !matches!(self.sequence, EnvSequence::InputThenEnv),
                );
                let parsed = iter
                    .load()
                    .map_err(|error| crate::Error::from((error, path)))?;
                accumulated.extend(parsed);
            }
        }
        Ok(accumulated)
    }

    /// Loads environment variables into a hash map.
    ///
    /// This is the primary method for loading environment variables.
    pub fn load(self) -> Result<EnvMap, crate::Error> {
        let environment = if matches!(self.sequence, EnvSequence::InputOnly) {
            EnvMap::new()
        } else {
            environment_variables()?
        };
        match self.sequence {
            EnvSequence::EnvOnly => Ok(environment),
            EnvSequence::EnvThenInput => {
                let mut existing = environment.clone();
                let input = self.load_input(&environment)?;
                existing.extend(input);
                Ok(existing)
            }
            EnvSequence::InputOnly => self.load_input(&environment),
            EnvSequence::InputThenEnv => {
                let mut input = self.load_input(&environment)?;
                input.extend(environment);
                Ok(input)
            }
        }
    }

    /// Loads the environment and reports every variable consulted during substitution.
    ///
    /// This is an implementation hook for the compile-time macros, which must make all
    /// environment reads visible to Cargo's incremental compilation dependency tracking.
    #[doc(hidden)]
    pub fn load_with_substitution_dependencies(
        mut self,
    ) -> Result<(EnvMap, HashSet<String>), crate::Error> {
        let tracker: SubstitutionTracker = Rc::new(RefCell::new(HashSet::new()));
        self.substitution_tracker = Some(Rc::clone(&tracker));
        let map = self.load()?;
        let dependencies = tracker.borrow().clone();
        Ok((map, dependencies))
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        EnvLoader, EnvSequence, Error, Input, ParseErrorKind, ParseOptions, parse, parse_partial,
    };
    use std::path::Path;
    use std::{
        env, fs,
        io::Cursor,
        time::{SystemTime, UNIX_EPOCH},
    };

    #[cfg(unix)]
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};

    #[cfg(windows)]
    #[test]
    fn windows_environment_names_follow_case_insensitive_precedence() {
        use std::io::Cursor;

        const NAME: &str = "DOTENV_NG_WINDOWS_CASE";
        temp_env::with_var("Dotenv_Ng_Windows_Case", Some("environment"), || {
            let inherited = EnvLoader::with_reader(Cursor::new(
                "dotenv_ng_windows_case=input\nRESULT=$DOTENV_NG_WINDOWS_CASE",
            ))
            .sequence(EnvSequence::InputThenEnv)
            .substitution(true)
            .load()
            .unwrap();
            assert_eq!(inherited.var(NAME).unwrap(), "environment");
            assert_eq!(inherited.var("result").unwrap(), "environment");
            assert_eq!(
                inherited
                    .keys()
                    .filter(|key| key.eq_ignore_ascii_case(NAME))
                    .count(),
                1
            );

            let overridden = EnvLoader::with_reader(Cursor::new(
                "dotenv_ng_windows_case=input\nRESULT=$DOTENV_NG_WINDOWS_CASE",
            ))
            .sequence(EnvSequence::EnvThenInput)
            .substitution(true)
            .load()
            .unwrap();
            assert_eq!(overridden.var(NAME).unwrap(), "input");
            assert_eq!(overridden.var("result").unwrap(), "input");
            assert_eq!(
                overridden
                    .keys()
                    .filter(|key| key.eq_ignore_ascii_case(NAME))
                    .count(),
                1
            );
        });
    }

    #[cfg(windows)]
    #[test]
    fn windows_input_and_explicit_substitutions_are_case_insensitive() {
        use std::io::Cursor;

        let mut map = EnvLoader::with_reader(Cursor::new(
            "dotenv_ng_mixed=first\nDOTENV_NG_MIXED=second\nRESULT=$dotenv_ng_injected",
        ))
        .sequence(EnvSequence::InputOnly)
        .substitution(true)
        .substitutions([("Dotenv_Ng_Injected", "injected")])
        .load()
        .unwrap();

        assert_eq!(map.var("dotenv_ng_mixed").unwrap(), "second");
        assert_eq!(map.var("result").unwrap(), "injected");
        assert_eq!(
            map.insert("Dotenv_Ng_Mixed".to_owned(), "third".to_owned()),
            Some("second".to_owned())
        );
        assert_eq!(map.var("DOTENV_NG_MIXED").unwrap(), "third");
        assert_eq!(
            map.keys()
                .filter(|key| key.eq_ignore_ascii_case("DOTENV_NG_MIXED"))
                .count(),
            1
        );
    }

    #[cfg(unix)]
    #[test]
    fn non_unicode_environment_value_returns_an_error() {
        let value = OsString::from_vec(vec![0xff]);

        temp_env::with_var("DOTENV_NG_NON_UNICODE", Some(value.clone()), || {
            let error = EnvLoader::with_reader(Cursor::new("KEY=value"))
                .load()
                .unwrap_err();

            assert!(matches!(
                error,
                Error::NotUnicode(actual, ref name)
                    if actual == value && name == "DOTENV_NG_NON_UNICODE"
            ));
        });
    }

    #[cfg(unix)]
    #[test]
    fn non_unicode_environment_name_returns_an_error() {
        let name = OsString::from_vec(b"DOTENV_NG_NON_UNICODE_\xff".to_vec());

        temp_env::with_var(name.clone(), Some("value"), || {
            let error = EnvLoader::with_reader(Cursor::new("KEY=value"))
                .load()
                .unwrap_err();

            assert!(matches!(error, Error::NotUnicodeName(actual) if actual == name));
        });
    }

    #[cfg(unix)]
    #[test]
    fn input_only_does_not_inspect_non_unicode_environment_values() {
        let value = OsString::from_vec(vec![0xff]);

        temp_env::with_var("DOTENV_NG_NON_UNICODE", Some(value), || {
            let map = EnvLoader::with_reader(Cursor::new("KEY=value"))
                .sequence(EnvSequence::InputOnly)
                .load()
                .unwrap();

            assert_eq!(map["KEY"], "value");
        });
    }

    #[test]
    fn free_var_reports_present_and_missing_values() {
        temp_env::with_vars(
            [
                ("DOTENV_NG_PRESENT", Some("value")),
                ("DOTENV_NG_MISSING", None),
            ],
            || {
                assert_eq!(crate::var("DOTENV_NG_PRESENT").unwrap(), "value");
                assert!(matches!(
                    crate::var("DOTENV_NG_MISSING"),
                    Err(Error::NotPresent(name)) if name == "DOTENV_NG_MISSING"
                ));
            },
        );
    }

    #[cfg(unix)]
    #[test]
    fn free_var_reports_non_unicode_values() {
        let value = OsString::from_vec(vec![0xff]);
        temp_env::with_var("DOTENV_NG_NON_UNICODE_VAR", Some(value.clone()), || {
            assert!(matches!(
                crate::var("DOTENV_NG_NON_UNICODE_VAR"),
                Err(Error::NotUnicode(actual, name))
                    if actual == value && name == "DOTENV_NG_NON_UNICODE_VAR"
            ));
        });
    }

    #[test]
    fn test_substitution() -> Result<(), crate::Error> {
        temp_env::with_vars([("KEY", Some("value")), ("KEY1", Some("value1"))], || {
            let subs = [
                "$ZZZ", "$KEY", "$KEY1", "${KEY}1", "$KEY_U", "${KEY_U}", "\\$KEY",
            ];

            let common_string = subs.join(">>");
            let s = format!(
                r#"
    KEY1=new_value1
    KEY_U=$KEY+valueU

    STRONG_QUOTES='{common_string}'
    WEAK_QUOTES="{common_string}"
    NO_QUOTES={common_string}
    "#,
            );
            let env_map = EnvLoader::with_reader(Cursor::new(s))
                .sequence(EnvSequence::InputThenEnv)
                .substitution(true)
                .load()?;

            assert_eq!(env_map.var("KEY")?, "value");
            assert_eq!(env_map.var("KEY1")?, "value1");
            assert_eq!(env_map.var("KEY_U")?, "value+valueU");
            assert_eq!(env_map.var("STRONG_QUOTES")?, common_string);
            let expanded = [
                "",
                "value",
                "value1",
                "value1",
                "value+valueU",
                "value+valueU",
                "$KEY",
            ]
            .join(">>");
            assert_eq!(env_map.var("WEAK_QUOTES")?, expanded);
            assert_eq!(env_map.var("NO_QUOTES")?, expanded);
            Ok(())
        })
    }

    #[test]
    fn test_multiline() -> Result<(), crate::Error> {
        let value = "-----BEGIN PRIVATE KEY-----\n-----END PRIVATE KEY-----\\n\\\"QUOTED\\\"";
        let weak = "-----BEGIN PRIVATE KEY-----\n-----END PRIVATE KEY-----\n\"QUOTED\"";

        let s = format!(
            r#"
    KEY=my\ cool\ value
    KEY3="awesome \"stuff\"
    more
    on other
    lines"
    KEY4='hello \'world\'
    good morning'
    WEAK="{value}"
    STRONG='{value}'
    "#
        );

        let env_map = EnvLoader::with_reader(Cursor::new(s))
            .sequence(EnvSequence::InputOnly)
            .load()?;
        assert_eq!(env_map.var("KEY")?, "my cool value");
        assert_eq!(
            env_map.var("KEY3")?,
            r#"awesome "stuff"
    more
    on other
    lines"#
        );
        assert_eq!(
            env_map.var("KEY4")?,
            "hello 'world'
    good morning"
        );
        assert_eq!(env_map.var("WEAK")?, weak);
        assert_eq!(env_map.var("STRONG")?, value);
        Ok(())
    }

    #[test]
    fn test_multiline_comment() -> Result<(), crate::Error> {
        let s = r#"
# Start of env file
# Comment line with single ' quote
# Comment line with double " quote
 # Comment line with double " quote and starts with a space
TESTKEY1=test_val # 1 '" comment
TESTKEY2=test_val_with_#_hash # 2 '" comment
TESTKEY3="test_val quoted with # hash" # 3 '" comment
TESTKEY4="Line 1
# Line 2
Line 3" # 4 Multiline "' comment
TESTKEY5="Line 4
# Line 5
Line 6
" # 5 Multiline "' comment
# End of env file
"#;

        let env_map = EnvLoader::with_reader(Cursor::new(s))
            .sequence(EnvSequence::InputOnly)
            .load()?;
        assert_eq!(env_map.var("TESTKEY1")?, "test_val");
        assert_eq!(env_map.var("TESTKEY2")?, "test_val_with_#_hash");
        assert_eq!(env_map.var("TESTKEY3")?, "test_val quoted with # hash");
        assert_eq!(
            env_map.var("TESTKEY4")?,
            "Line 1
# Line 2
Line 3"
        );
        assert_eq!(
            env_map.var("TESTKEY5")?,
            "Line 4
# Line 5
Line 6
"
        );
        Ok(())
    }

    #[test]
    fn test_non_modify() -> Result<(), crate::Error> {
        temp_env::with_var("SRC", Some("env"), || {
            let s = "SRC=envfile\nFOO=bar";
            let env_map = EnvLoader::with_reader(Cursor::new(s))
                .sequence(EnvSequence::EnvThenInput)
                .load()?;
            assert_eq!("envfile", env_map.var("SRC")?);
            assert_eq!("bar", env_map.var("FOO")?);

            let env_map = EnvLoader::with_reader(Cursor::new(s))
                .sequence(EnvSequence::InputThenEnv)
                .load()?;
            assert_eq!("env", env_map.var("SRC")?);
            Ok(())
        })
    }

    #[test]
    fn input_only_substitution_does_not_read_the_process_environment() {
        temp_env::with_var("DOTENV_NG_EXTERNAL", Some("from-environment"), || {
            let input =
                "LOCAL=from-input\nLOCAL_RESULT=$LOCAL\nEXTERNAL_RESULT=$DOTENV_NG_EXTERNAL";
            let map = EnvLoader::with_reader(Cursor::new(input))
                .sequence(EnvSequence::InputOnly)
                .substitution(true)
                .load()
                .unwrap();

            assert_eq!(map["LOCAL_RESULT"], "from-input");
            assert_eq!(map["EXTERNAL_RESULT"], "");
        });
    }

    #[test]
    fn accepts_explicit_substitution_values_without_environment_mutation() {
        temp_env::with_var_unset("APP_VAR", || {
            let input = "RESULT=${APP_VAR}-from-file";
            let map = EnvLoader::with_reader(Cursor::new(input))
                .sequence(EnvSequence::InputOnly)
                .substitution(true)
                .substitutions([("APP_VAR".to_owned(), "provided".to_owned())])
                .load()
                .unwrap();

            assert_eq!(map["RESULT"], "provided-from-file");
            assert!(env::var_os("APP_VAR").is_none());
        });
    }

    #[test]
    fn multiple_inputs_share_substitutions_and_later_inputs_win() {
        temp_env::with_vars(
            [
                ("DOTENV_NG_MULTI_A", None),
                ("DOTENV_NG_MULTI_SOURCE", Some("environment")),
            ],
            || {
                let readers = [
                    Cursor::new("DOTENV_NG_MULTI_A=base\nDOTENV_NG_MULTI_SOURCE=first\n"),
                    Cursor::new(
                        "DOTENV_NG_MULTI_A=local\nFROM_A=$DOTENV_NG_MULTI_A\n\
                         DOTENV_NG_MULTI_SOURCE=second\nFROM_SOURCE=$DOTENV_NG_MULTI_SOURCE\n",
                    ),
                ];
                let map = EnvLoader::with_readers(readers)
                    .sequence(EnvSequence::EnvThenInput)
                    .substitution(true)
                    .load()
                    .unwrap();

                assert_eq!(map["DOTENV_NG_MULTI_A"], "local");
                assert_eq!(map["FROM_A"], "local");
                assert_eq!(map["DOTENV_NG_MULTI_SOURCE"], "second");
                assert_eq!(map["FROM_SOURCE"], "second");
            },
        );
    }

    #[test]
    fn environment_stays_authoritative_across_multiple_inputs() {
        temp_env::with_var("DOTENV_NG_MULTI_SOURCE", Some("environment"), || {
            let readers = [
                Cursor::new("DOTENV_NG_MULTI_SOURCE=first\n"),
                Cursor::new("FROM_SOURCE=$DOTENV_NG_MULTI_SOURCE\n"),
            ];
            let map = EnvLoader::with_readers(readers)
                .sequence(EnvSequence::InputThenEnv)
                .substitution(true)
                .load()
                .unwrap();

            assert_eq!(map["DOTENV_NG_MULTI_SOURCE"], "environment");
            assert_eq!(map["FROM_SOURCE"], "environment");
        });
    }

    #[test]
    fn optional_missing_paths_are_skipped_before_later_files() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let existing = env::temp_dir().join(format!(
            "dotenv-ng-core-{}-{suffix}.env",
            std::process::id()
        ));
        let missing = existing.with_extension("missing");
        fs::write(&existing, "LOADED_FROM_PATH=yes\n").unwrap();

        let map = EnvLoader::with_paths([&missing, &existing])
            .required(false)
            .sequence(EnvSequence::InputOnly)
            .load()
            .unwrap();
        fs::remove_file(existing).unwrap();

        assert_eq!(map["LOADED_FROM_PATH"], "yes");
    }

    #[test]
    fn default_loader_matches_new_loader() {
        for loader in [EnvLoader::default(), EnvLoader::new()] {
            let [Input::Path(path)] = loader.inputs.as_slice() else {
                panic!("the default loader should contain exactly one path");
            };
            assert_eq!(path, Path::new("./.env"));
        }
    }

    #[test]
    fn empty_input_collections_are_rejected() {
        let error = EnvLoader::with_readers(Vec::<Cursor<&str>>::new())
            .sequence(EnvSequence::InputOnly)
            .load()
            .unwrap_err();
        assert!(matches!(error, Error::NoInput));

        let error = EnvLoader::with_paths(Vec::<&str>::new())
            .sequence(EnvSequence::InputOnly)
            .load()
            .unwrap_err();
        assert!(matches!(error, Error::NoInput));
    }

    #[test]
    fn path_replaces_file_input_and_contextualizes_reader_input() {
        let file_error = EnvLoader::with_path("old.env")
            .path("new.env")
            .sequence(EnvSequence::InputOnly)
            .load()
            .unwrap_err();
        assert!(file_error.to_string().contains("new.env"));
        assert!(!file_error.to_string().contains("old.env"));

        let reader_error = EnvLoader::with_reader(Cursor::new("BROKEN value"))
            .path("reader.env")
            .sequence(EnvSequence::InputOnly)
            .load()
            .unwrap_err();
        assert!(reader_error.to_string().contains("reader.env"));
    }

    #[test]
    fn loader_builder_options_control_parsing() {
        let multiline_error = EnvLoader::with_reader(Cursor::new("VALUE=\"first\nsecond\""))
            .sequence(EnvSequence::InputOnly)
            .multiline(false)
            .load()
            .unwrap_err();
        assert!(matches!(
            multiline_error,
            Error::Parse(ref error, _) if error.kind() == &ParseErrorKind::MultilineDisabled
        ));

        let options = ParseOptions::new().substitution(true).multiline(false);
        let map = EnvLoader::with_reader(Cursor::new("BASE=value\nRESULT=$BASE"))
            .sequence(EnvSequence::InputOnly)
            .parse_options(options)
            .load()
            .unwrap();
        assert_eq!(map["RESULT"], "value");
    }

    #[test]
    fn missing_file_errors_retain_their_path_and_kind() {
        let error = EnvLoader::with_path("definitely/missing/required.env")
            .sequence(EnvSequence::InputOnly)
            .load()
            .unwrap_err();
        assert!(error.not_found());
        assert!(
            error
                .to_string()
                .contains("definitely/missing/required.env")
        );
    }

    #[test]
    fn reports_variables_consulted_during_substitution() {
        let (map, dependencies) = EnvLoader::with_reader(Cursor::new(
            "DIRECT=$EXTERNAL\nBRACED=${ANOTHER}\nLITERAL='$IGNORED'\n",
        ))
        .sequence(EnvSequence::InputOnly)
        .substitution(true)
        .substitutions([("EXTERNAL", "value")])
        .load_with_substitution_dependencies()
        .unwrap();

        assert_eq!(map["DIRECT"], "value");
        assert_eq!(map["BRACED"], "");
        assert_eq!(map["LITERAL"], "$IGNORED");
        assert_eq!(dependencies.len(), 2);
        assert!(dependencies.contains("EXTERNAL"));
        assert!(dependencies.contains("ANOTHER"));
    }

    #[test]
    fn loader_preserves_bcrypt_style_values_by_default() {
        // Regression for https://github.com/cachix/secretspec/issues/73: bcrypt hashes contain
        // dollar-prefixed fragments that must not be treated as variable references.
        let secret = "foo:$2a$10$TWoviNHS27HJMw1PKe4tBeIMlms6tWdYS9hKoHANKCQhluDlEt/gu,\
                      bar:$2a$10$labXlt9fBRMjJu.gOUabjebLVBKGB/xZOFpEn/esCln56USXHMHQW";
        let input = format!("VALUE=\"{secret}\"");
        let map = EnvLoader::with_reader(Cursor::new(input))
            .sequence(EnvSequence::InputOnly)
            .load()
            .unwrap();
        assert_eq!(map["VALUE"], secret);
    }

    #[test]
    fn parse_errors_include_path_line_column_and_source() {
        let error = EnvLoader::with_reader(Cursor::new("VALID=yes\nBROKEN value\n"))
            .path("config/.env")
            .sequence(EnvSequence::InputOnly)
            .load()
            .unwrap_err();

        let Error::Parse(parse_error, Some(path)) = &error else {
            panic!("expected a path-aware parse error, got {error:?}");
        };
        assert_eq!(path, Path::new("config/.env"));
        assert_eq!(parse_error.line(), 2);
        assert_eq!(parse_error.column(), 8);
        assert_eq!(parse_error.kind(), &ParseErrorKind::MissingEquals);
        assert_eq!(parse_error.source_line(), "BROKEN value");
        assert!(error.to_string().contains("2 | BROKEN value"));
    }

    #[test]
    fn parse_prefers_own_assignments_over_substitutions_and_keeps_order() {
        let parsed = parse(
            "VAR_B=\"test_${VAR_A}\"\nVAR_A=test\nVAR_C=\"c_${VAR_A}_${OUTER}\"\n",
            true,
            [
                ("VAR_A".to_owned(), "base".to_owned()),
                ("OUTER".to_owned(), "outer".to_owned()),
            ],
        )
        .unwrap();
        assert_eq!(
            parsed,
            [
                ("VAR_B".to_owned(), "test_base".to_owned()),
                ("VAR_A".to_owned(), "test".to_owned()),
                ("VAR_C".to_owned(), "c_test_outer".to_owned()),
            ]
        );
    }

    #[test]
    fn parse_in_file_reference_beats_substitution_defined_earlier() {
        let parsed = parse(
            "VAR_A=test\nVAR_B=\"test_${VAR_A}\"\n",
            true,
            [("VAR_A".to_owned(), "base".to_owned())],
        )
        .unwrap();
        assert_eq!(parsed[1], ("VAR_B".to_owned(), "test_test".to_owned()));
    }

    #[test]
    fn parse_partial_keeps_assignments_before_the_error() {
        let (items, error) = parse_partial("A=1\nB=2\nBROKEN line\nC=3\n", true, []);
        assert_eq!(
            items,
            [
                ("A".to_owned(), "1".to_owned()),
                ("B".to_owned(), "2".to_owned())
            ]
        );
        assert!(matches!(error, Some(Error::Parse(_, _))));
        assert!(parse_partial("A=1\n", true, []).1.is_none());
    }

    #[test]
    fn parse_ignores_the_process_environment() {
        temp_env::with_var("DOTENV_NG_PARSE_AMBIENT", Some("ambient"), || {
            let parsed = parse("A=${DOTENV_NG_PARSE_AMBIENT}\n", true, []).unwrap();
            assert_eq!(parsed[0].1, "");
        });
    }

    #[test]
    fn environment_only_load_ignores_input() {
        temp_env::with_var("DOTENV_NG_ENV_ONLY", Some("environment"), || {
            let map = EnvLoader::with_reader(Cursor::new("BROKEN input"))
                .sequence(EnvSequence::EnvOnly)
                .load()
                .unwrap();

            assert_eq!(map["DOTENV_NG_ENV_ONLY"], "environment");
        });
    }
}
