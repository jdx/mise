use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fmt::Debug;
use std::io::prelude::*;
use std::iter::once;
use std::path::{Path, PathBuf};

use base64::prelude::*;
use eyre::Result;
use flate2::Compression;
use flate2::write::{ZlibDecoder, ZlibEncoder};
use indexmap::{IndexMap, IndexSet};
use itertools::Itertools;
use serde_derive::{Deserialize, Serialize};
use std::sync::LazyLock as Lazy;

use crate::env::PATH_KEY;
use crate::{cmd, file};

#[derive(Default, Serialize, Deserialize)]
pub struct EnvDiff {
    #[serde(default)]
    pub old: IndexMap<String, String>,
    #[serde(default)]
    pub new: IndexMap<String, String>,
    #[serde(default)]
    pub path: Vec<PathBuf>,
}

#[derive(Debug)]
pub enum EnvDiffOperation {
    Add(String, String),
    Change(String, String),
    Remove(String),
}

pub type EnvDiffPatches = Vec<EnvDiffOperation>;
pub type EnvMap = BTreeMap<String, String>;

impl EnvDiff {
    pub fn new<T>(original: &EnvMap, additions: T) -> EnvDiff
    where
        T: IntoIterator<Item = (String, String)>,
    {
        let mut diff = EnvDiff::default();

        for (key, new_val) in additions.into_iter() {
            let key: String = key;
            match original.get(&key) {
                Some(original_val) => {
                    if original_val != &new_val {
                        diff.old.insert(key.clone(), original_val.into());
                        diff.new.insert(key, new_val);
                    }
                }
                None => {
                    diff.new.insert(key, new_val);
                }
            }
        }

        diff
    }

    pub fn from_bash_script<T, U, V>(
        script: &Path,
        dir: &Path,
        env: T,
        opts: &EnvDiffOptions,
    ) -> Result<Self>
    where
        T: IntoIterator<Item = (U, V)>,
        U: Into<OsString>,
        V: Into<OsString>,
    {
        let env: IndexMap<OsString, OsString> =
            env.into_iter().map(|(k, v)| (k.into(), v.into())).collect();
        let bash_path = file::which("bash").unwrap_or("/bin/bash".into());
        let out = cmd!(
            bash_path,
            "--noprofile",
            "-c",
            indoc::formatdoc! {"
                . {script}
                export -p
            ", script = script.display()}
        )
        .dir(dir)
        .full_env(&env)
        .read()?;
        let env: EnvMap = env
            .into_iter()
            .map(|(k, v)| (k.into_string().unwrap(), v.into_string().unwrap()))
            .collect();

        let mut additions = EnvMap::new();
        let mut cur_key = None;
        for line in out.lines() {
            match line.strip_prefix("declare -x ") {
                Some(line) => {
                    let (k, v) = line.split_once('=').unwrap_or_default();
                    if invalid_key(k, opts) {
                        continue;
                    }
                    cur_key = Some(k.to_string());
                    additions.insert(k.to_string(), v.to_string());
                }
                None => {
                    if let Some(k) = &cur_key {
                        let v = format!("\n{line}");
                        additions.get_mut(k).unwrap().push_str(&v);
                    }
                }
            }
        }
        for (k, v) in additions.clone().iter() {
            let v = normalize_escape_sequences(v);
            if let Some(orig) = env.get(k) {
                if &v == orig {
                    additions.remove(k);
                    continue;
                }
            }
            additions.insert(k.into(), v);
        }
        Ok(Self::new(&env, additions))
    }

    pub fn deserialize(raw: &str) -> Result<EnvDiff> {
        let mut writer = Vec::new();
        let mut decoder = ZlibDecoder::new(writer);
        let bytes = BASE64_STANDARD_NO_PAD.decode(raw)?;
        decoder.write_all(&bytes[..])?;
        writer = decoder.finish()?;
        Ok(rmp_serde::from_slice(&writer[..])?)
    }

    pub fn serialize(&self) -> Result<String> {
        let mut gz = ZlibEncoder::new(Vec::new(), Compression::fast());
        gz.write_all(&rmp_serde::to_vec_named(self)?)?;
        Ok(BASE64_STANDARD_NO_PAD.encode(gz.finish()?))
    }

    pub fn to_patches(&self) -> EnvDiffPatches {
        let mut patches = EnvDiffPatches::new();

        for k in self.old.keys() {
            match self.new.get(k) {
                Some(v) => patches.push(EnvDiffOperation::Change(k.into(), v.into())),
                None => patches.push(EnvDiffOperation::Remove(k.into())),
            };
        }
        for (k, v) in self.new.iter() {
            if !self.old.contains_key(k) {
                patches.push(EnvDiffOperation::Add(k.into(), v.into()))
            };
        }

        patches
    }

    pub fn reverse(&self) -> EnvDiff {
        EnvDiff {
            old: self.new.clone(),
            new: self.old.clone(),
            path: self.path.clone(),
        }
    }
}

fn invalid_key(k: &str, opts: &EnvDiffOptions) -> bool {
    k.is_empty()
        || opts.ignore_keys.contains(k)
        // following two ignores are for exported bash functions and exported bash
        // functions which are multiline, they appear in the environment as e.g.:
        // BASH_FUNC_exported-bash-function%%=() { echo "this is an"
        //  echo "exported bash function"
        //  echo "with multiple lines"
        // }
        || k.starts_with("BASH_FUNC_")
        || k.starts_with(' ')
}

static DEFAULT_IGNORE_KEYS: Lazy<IndexSet<String>> = Lazy::new(|| {
    [
        "_",
        "SHLVL",
        "PWD",
        "OLDPWD",
        "HOME",
        "USER",
        "SHELL",
        "SHELLOPTS",
        "COMP_WORDBREAKS",
        "PS1",
        "PROMPT_DIRTRIM",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
});

impl Debug for EnvDiff {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let print_sorted = |hashmap: &IndexMap<String, String>| {
            hashmap
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .sorted()
                .collect::<Vec<_>>()
        };
        f.debug_struct("EnvDiff")
            .field("old", &print_sorted(&self.old))
            .field("new", &print_sorted(&self.new))
            .finish()
    }
}

fn normalize_escape_sequences(input: &str) -> String {
    let input = if input.starts_with('"') && input.ends_with('"') {
        input[1..input.len() - 1].to_string()
    } else if input.starts_with("$'") && input.ends_with('\'') {
        input[2..input.len() - 1].to_string()
    } else {
        input.to_string()
    };

    let mut result = String::with_capacity(input.len());
    let mut chars = input.chars();

    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some(val) => match val {
                    'a' => result.push('\u{07}'),
                    'b' => result.push('\u{08}'),
                    'e' | 'E' => result.push('\u{1b}'),
                    'f' => result.push('\u{0c}'),
                    'n' => result.push('\n'),
                    'r' => result.push('\r'),
                    't' => result.push('\t'),
                    'v' => result.push('\u{0b}'),
                    '\\' => result.push('\\'),
                    '\'' => result.push('\''),
                    '"' => result.push('"'),
                    '?' => result.push('?'),
                    '`' => result.push('`'),
                    '$' => result.push('$'),
                    _ => {
                        result.push('\\');
                        result.push(val);
                    }
                },
                None => {
                    warn!("Invalid escape sequence: {}", input);
                }
            }
        } else {
            result.push(c)
        }
    }

    result
}

pub struct EnvDiffOptions {
    pub ignore_keys: IndexSet<String>,
}

impl Default for EnvDiffOptions {
    fn default() -> Self {
        Self {
            ignore_keys: DEFAULT_IGNORE_KEYS
                .iter()
                .cloned()
                .chain(once(PATH_KEY.to_string()))
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::config::Config;

    use super::*;

    use insta::assert_debug_snapshot;
    use pretty_assertions::assert_str_eq;

    #[tokio::test]
    async fn test_diff() {
        let _config = Config::get().await.unwrap();
        let diff = EnvDiff::new(&new_from_hashmap(), new_to_hashmap());
        assert_debug_snapshot!(diff.to_patches());
    }

    #[tokio::test]
    async fn test_reverse() {
        let _config = Config::get().await.unwrap();
        let diff = EnvDiff::new(&new_from_hashmap(), new_to_hashmap());
        let patches = diff.reverse().to_patches();
        let to_remove = patches
            .iter()
            .filter_map(|p| match p {
                EnvDiffOperation::Remove(k) => Some(k),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_debug_snapshot!(to_remove, @r#"
        [
            "c",
        ]
        "#);
        let to_add = patches
            .iter()
            .filter_map(|p| match p {
                EnvDiffOperation::Add(k, v) => Some((k, v)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_debug_snapshot!(to_add, @"[]");
        let to_change = patches
            .iter()
            .filter_map(|p| match p {
                EnvDiffOperation::Change(k, v) => Some((k, v)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_debug_snapshot!(to_change, @r#"
        [
            (
                "b",
                "2",
            ),
        ]
        "#);
    }

    fn new_from_hashmap() -> EnvMap {
        [("a", "1"), ("b", "2")]
            .map(|(k, v)| (k.into(), v.into()))
            .into()
    }

    fn new_to_hashmap() -> EnvMap {
        [("a", "1"), ("b", "3"), ("c", "4")]
            .map(|(k, v)| (k.into(), v.into()))
            .into()
    }

    #[tokio::test]
    async fn test_serialize() {
        let _config = Config::get().await.unwrap();
        let diff = EnvDiff::new(&new_from_hashmap(), new_to_hashmap());
        let serialized = diff.serialize().unwrap();
        let deserialized = EnvDiff::deserialize(&serialized).unwrap();
        assert_debug_snapshot!(deserialized.to_patches());
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn test_from_bash_script() {
        let _config = Config::get().await.unwrap();
        use crate::{config::Config, dirs};
        use indexmap::indexmap;
        let path = dirs::HOME.join("fixtures/exec-env");
        let orig = indexmap! {
            "UNMODIFIED_VAR" => "unmodified",
            "UNMODIFIED_NEWLINE_VAR" => "hello\\nworld",
            "UNMODIFIED_SQUOTE_VAR" => "hello\\'world",
            "UNMODIFIED_ESCAPE_VAR" => "hello\\world",
            "MODIFIED_VAR" => "original",
            "ESCAPES" => "\\n\\t\\r\\v\\f\\a\\b\\e\\0\\x1b\\u1234\\U00012345\\a\\b\\e\\E\\f\\n\\r\\t\\v\"?`$\\g'\\0",
            "BACKSPACE" => "\u{08}",
            "BACKTICK" => "`",
            "BELL" => "\u{07}",
            "CARRIAGE_RETURN" => "\r",
            "DOLLAR" => "$",
            "DOUBLE_QUOTE" => "\"",
            "ESCAPE" => "\u{1b}",
            "ESCAPE2" => "\u{1b}",
            "FORM_FEED" => "\u{0c}",
            "G" => "g",
            "NEWLINE" => "\n",
            "QUESTION_MARK" => "?",
            "SINGLE_QUOTE" => "'",
            "TAB" => "\t",
            "VERTICAL_TAB" => "\u{0b}",
        }
            .into_iter()
            .map(|(k, v)| (k.into(), v.into()))
            .collect::<Vec<(String, String)>>();
        let cwd = dirs::CWD.clone().unwrap();
        let ed =
            EnvDiff::from_bash_script(path.as_path(), &cwd, orig, &Default::default()).unwrap();
        assert_debug_snapshot!(ed);
    }

    #[tokio::test]
    async fn test_invalid_escape_sequence() {
        let _config = Config::get().await.unwrap();
        let input = r#""\g\""#;
        let output = normalize_escape_sequences(input);
        // just warns
        assert_str_eq!(output, r"\g");
    }
}
