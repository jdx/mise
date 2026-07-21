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
use serde::{Deserialize, Serialize};
use std::sync::LazyLock as Lazy;

use crate::env::PATH_KEY;
use crate::file;

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
        // The into_string().unwrap() panic on invalid UTF-8 is pre-existing
        // behavior (the conversion used to happen after the spawn).
        let env: EnvMap = env
            .into_iter()
            .map(|(k, v)| {
                (
                    k.into().into_string().unwrap(),
                    v.into().into_string().unwrap(),
                )
            })
            .collect();

        // On Windows, resolve a real POSIX bash (Git Bash / MSYS2) instead of
        // whatever `bash.exe` happens to be first on PATH — which is usually the
        // WSL launcher at C:\Windows\System32\bash.exe when mise is invoked from
        // PowerShell, and WSL cannot read `C:\...` script paths. On GitHub
        // runners Git Bash is not on PATH at all, so the candidate probe inside
        // resolve_posix_shell_program_path is what finds it. See discussion #6513.
        #[cfg(windows)]
        let bash_path: PathBuf =
            crate::path::resolve_posix_shell_program_path(std::ffi::OsStr::new("bash"), &env)
                .map(PathBuf::from)
                .or_else(|| {
                    file::which("bash.exe").filter(|p| !crate::path::is_wsl_launcher_bash(p))
                })
                .ok_or_else(|| {
                    eyre::eyre!(
                        "no POSIX bash found to source {}; install Git for Windows or MSYS2, or \
                         set MISE_BASH_PATH (the WSL launcher at C:\\Windows\\System32\\bash.exe \
                         cannot read Windows script paths)",
                        script.display()
                    )
                })?;
        #[cfg(not(windows))]
        let bash_path = file::which("bash").unwrap_or("/bin/bash".into());

        // Windows: dump the exported env BEFORE and AFTER sourcing, separated
        // by a NUL byte (environment values cannot contain NUL, so the split is
        // unambiguous even for hostile multi-line values). The first dump
        // self-captures the baseline the MSYS runtime and the Git-for-Windows
        // bash wrapper create (PATH converted to `/c/...` form with
        // `/mingw64/bin:/usr/bin` prepended, TMP/TEMP rewritten to `/tmp`,
        // MSYSTEM exported, ...), so diffing the two dumps leaves only what the
        // script itself changed. The script path is passed as a positional
        // argument (`$1`, in forward-slash form so MSYS bash reads it verbatim)
        // rather than interpolated into the command, so `$`/backticks in the
        // path are not expanded by bash.
        #[cfg(windows)]
        let out = cmd!(
            bash_path,
            "--noprofile",
            "-c",
            indoc::indoc! {r#"
                export -p
                printf '\0'
                . "$1"
                export -p
            "#},
            "mise", // $0 for the -c body
            script.to_string_lossy().replace('\\', "/"),
        )
        .dir(dir)
        .full_env(&env)
        .read()?;
        #[cfg(not(windows))]
        let out = cmd!(
            bash_path,
            "--noprofile",
            "-c",
            indoc::formatdoc! {"
                . \"{script}\"
                export -p
            ", script = script.display()}
        )
        .dir(dir)
        .full_env(&env)
        .read()?;

        #[cfg(windows)]
        let (mut additions, baseline_path) = {
            let (before, after) = out.split_once('\0').ok_or_else(|| {
                eyre::eyre!("failed to parse env after sourcing {}", script.display())
            })?;
            let baseline = parse_export_p(before, opts);
            let after = parse_export_p(after, opts);
            let additions: EnvMap = after
                .into_iter()
                .filter(|(k, v)| baseline.get(k) != Some(v))
                .collect();
            let baseline_path = baseline
                .get(PATH_KEY.as_str())
                .map(|v| normalize_escape_sequences(v));
            (additions, baseline_path)
        };
        #[cfg(not(windows))]
        let mut additions = parse_export_p(&out, opts);

        for (k, v) in additions.clone().iter() {
            let v = normalize_escape_sequences(v);
            if let Some(orig) = env.get(k)
                && &v == orig
            {
                additions.remove(k);
                continue;
            }
            additions.insert(k.into(), v);
        }
        // After the normalize loop — the reconstructed Windows-form PATH value
        // must not run through normalize_escape_sequences (`\f` in `C:\fake`
        // would turn into a form feed).
        #[cfg(windows)]
        fixup_windows_path(&mut additions, baseline_path.as_deref(), &env, script);
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

    /// Build an EnvDiff describing the transformation from `pristine` → `final_env`,
    /// suitable for serialization into `__MISE_DIFF`. PATH is excluded from
    /// `old`/`new` and instead tracked in `path` (entries present in `final_env`'s
    /// PATH but not in `pristine`'s PATH). `__MISE_DIFF` itself is also excluded
    /// so an inherited value can't end up nested inside the diff we're about to
    /// write. This mirrors what `mise hook-env` writes during shell activation,
    /// so nested `mise` invocations can reverse it via `get_pristine_env` and
    /// avoid stacking outer tool paths on top of inner ones.
    pub fn from_final_env(pristine: &EnvMap, final_env: &EnvMap) -> EnvDiff {
        use std::collections::HashSet;

        let path_key = PATH_KEY.as_str();
        let additions = final_env
            .iter()
            .filter(|(k, _)| k.as_str() != path_key && k.as_str() != "__MISE_DIFF")
            .map(|(k, v)| (k.clone(), v.clone()));
        let mut diff = EnvDiff::new(pristine, additions);

        let pristine_paths: HashSet<PathBuf> = pristine
            .get(path_key)
            .map(|p| crate::env::split_paths(p).collect())
            .unwrap_or_default();
        if let Some(final_path) = final_env.get(path_key) {
            diff.path = crate::env::split_paths(final_path)
                .filter(|p| !pristine_paths.contains(p))
                .collect();
        }

        diff
    }
}

/// Parse the output of bash's `export -p` into a map of exported variables.
/// Multi-line values continue until the next `declare -x ` line.
fn parse_export_p(out: &str, opts: &EnvDiffOptions) -> EnvMap {
    let mut additions = EnvMap::new();
    let mut cur_key = None;
    for line in out.lines() {
        match line.strip_prefix("declare -x ") {
            Some(line) => {
                // A new declaration always ends any multi-line continuation;
                // only a valid one re-arms it below. Otherwise continuation
                // lines of a skipped declaration (e.g. an exported bash
                // function body) would be appended to the previous variable.
                cur_key = None;
                let Some((k, v)) = line.split_once('=') else {
                    continue;
                };
                // bash always reports the PATH variable as `PATH`, but the
                // Windows host key is usually `Path`. Normalize to the host
                // casing so ignore_keys, EnvDiff::new, and callers comparing
                // against env::PATH_KEY behave identically cross-platform.
                let k = if cfg!(windows) && k.eq_ignore_ascii_case("PATH") {
                    PATH_KEY.to_string()
                } else {
                    k.to_string()
                };
                if invalid_key(&k, opts) {
                    continue;
                }
                cur_key = Some(k.clone());
                additions.insert(k, v.to_string());
            }
            None => {
                if let Some(k) = &cur_key {
                    let v = format!("\n{line}");
                    additions.get_mut(k).unwrap().push_str(&v);
                }
            }
        }
    }
    additions
}

/// Recover only what a sourced script *prepended* to PATH: strip the
/// self-captured MSYS-form baseline off the tail, convert each remaining entry
/// back to Windows form, and re-attach the ORIGINAL Windows-form PATH so that
/// callers' suffix logic (`EnvResults::source` strips the original PATH and
/// splits the rest on `;`) works unchanged cross-platform.
#[cfg(windows)]
fn fixup_windows_path(
    additions: &mut EnvMap,
    baseline_path: Option<&str>,
    orig_env: &EnvMap,
    script: &Path,
) {
    let Some(new_path) = additions.remove(PATH_KEY.as_str()) else {
        return;
    };
    let Some(prepended) = baseline_path.and_then(|b| new_path.strip_suffix(b)) else {
        // The script rewrote or appended to PATH rather than prepending —
        // matches the unix semantics, where only prepended entries are
        // recovered (EnvResults::source's strip_suffix).
        trace!(
            "sourcing {}: PATH was not prepended-to; ignoring PATH changes",
            script.display()
        );
        return;
    };
    let converted = prepended
        .trim_end_matches(':')
        .split(':')
        .filter(|e| !e.is_empty())
        .filter_map(|e| {
            let win = crate::path::unix_path_to_windows(e);
            if win.is_none() {
                trace!(
                    "sourcing {}: skipping PATH entry {e} with no Windows equivalent",
                    script.display()
                );
            }
            win
        })
        .collect::<Vec<_>>();
    if converted.is_empty() {
        return;
    }
    let orig = orig_env.get(PATH_KEY.as_str()).cloned().unwrap_or_default();
    let joined = if orig.is_empty() {
        converted.join(";")
    } else {
        format!("{};{}", converted.join(";"), orig)
    };
    additions.insert(PATH_KEY.to_string(), joined);
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
    async fn test_from_final_env() {
        let _config = Config::get().await.unwrap();
        let path_key = PATH_KEY.as_str();
        let pristine_paths = [PathBuf::from("/usr/bin"), PathBuf::from("/bin")];
        let final_paths = [
            PathBuf::from("/tool/bin"),
            PathBuf::from("/usr/bin"),
            PathBuf::from("/bin"),
        ];
        let pristine_path = std::env::join_paths(pristine_paths.iter())
            .unwrap()
            .into_string()
            .unwrap();
        let final_path = std::env::join_paths(final_paths.iter())
            .unwrap()
            .into_string()
            .unwrap();
        let pristine: EnvMap = [
            (path_key, pristine_path.as_str()),
            ("EXISTING", "old"),
            ("__MISE_DIFF", "outer-diff"),
        ]
        .into_iter()
        .map(|(k, v)| (k.into(), v.into()))
        .collect();
        let final_env: EnvMap = [
            (path_key, final_path.as_str()),
            ("EXISTING", "new"),
            ("ADDED", "yes"),
            ("__MISE_DIFF", "should-be-ignored"),
        ]
        .into_iter()
        .map(|(k, v)| (k.into(), v.into()))
        .collect();

        let diff = EnvDiff::from_final_env(&pristine, &final_env);

        // PATH entries new in final_env land in diff.path; shared entries don't.
        assert_eq!(diff.path, vec![PathBuf::from("/tool/bin")]);
        // Non-PATH adds/changes are tracked in diff.new (with diff.old for changes).
        assert_eq!(diff.new.get("ADDED"), Some(&"yes".to_string()));
        assert_eq!(diff.new.get("EXISTING"), Some(&"new".to_string()));
        assert_eq!(diff.old.get("EXISTING"), Some(&"old".to_string()));
        // PATH and __MISE_DIFF are filtered out of old/new.
        assert!(!diff.new.contains_key(path_key));
        assert!(!diff.old.contains_key(path_key));
        assert!(!diff.new.contains_key("__MISE_DIFF"));
        assert!(!diff.old.contains_key("__MISE_DIFF"));

        // Round-trip: applying the reversed diff to final_env should restore pristine
        // for the keys we tracked, and stripping diff.path from final's PATH should
        // give us pristine's PATH back.
        let reversed = diff.reverse();
        let mut restored: EnvMap = final_env.clone();
        for patch in reversed.to_patches() {
            match patch {
                EnvDiffOperation::Add(k, v) | EnvDiffOperation::Change(k, v) => {
                    restored.insert(k, v);
                }
                EnvDiffOperation::Remove(k) => {
                    restored.remove(&k);
                }
            }
        }
        assert_eq!(restored.get("EXISTING"), Some(&"old".to_string()));
        assert!(!restored.contains_key("ADDED"));
        let to_remove: std::collections::HashSet<_> = diff.path.iter().collect();
        let restored_path: Vec<PathBuf> = crate::env::split_paths(&final_env[path_key])
            .filter(|p| !to_remove.contains(p))
            .collect();
        assert_eq!(restored_path, pristine_paths);
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

    #[test]
    fn test_parse_export_p_skipped_declaration_does_not_pollute_previous_key() {
        // A skipped declaration with a multi-line value (e.g. an exported bash
        // function) must not have its continuation lines appended to the
        // previous valid key, and a valueless `declare -x FOO` must end any
        // running continuation.
        let out = indoc::indoc! {r#"
            declare -x GOOD="value"
            declare -x BASH_FUNC_foo%%="() { echo a
            echo b
            }"
            declare -x AFTER="after"
        "#};
        let parsed = parse_export_p(out, &EnvDiffOptions::default());
        assert_eq!(parsed.get("GOOD").map(String::as_str), Some("\"value\""));
        assert_eq!(parsed.get("AFTER").map(String::as_str), Some("\"after\""));
        assert!(!parsed.keys().any(|k| k.starts_with("BASH_FUNC_")));
    }

    /// Source `script_body` through the real resolved bash (Git Bash on CI)
    /// with PATH pinned to a known Windows-form value, mirroring how
    /// `EnvResults::source` calls `from_bash_script` (PATH not ignored).
    #[cfg(windows)]
    fn from_bash_script_windows(script_body: &str, orig_path: &str) -> EnvDiff {
        let tmp = tempfile::tempdir().unwrap();
        let script = tmp.path().join("env.sh");
        std::fs::write(&script, script_body).unwrap();
        // Inherit the real env (bash needs SYSTEMROOT etc.) but pin PATH and
        // add a marker var.
        let mut env: Vec<(String, String)> = crate::env::vars_safe().collect();
        env.retain(|(k, _)| !k.eq_ignore_ascii_case("PATH"));
        env.push(((*crate::env::PATH_KEY).to_string(), orig_path.to_string()));
        env.push(("EXISTING_VAR".to_string(), "unchanged".to_string()));
        let mut opts = EnvDiffOptions::default();
        opts.ignore_keys.shift_remove(&*crate::env::PATH_KEY);
        EnvDiff::from_bash_script(&script, tmp.path(), env, &opts).unwrap()
    }

    // https://github.com/jdx/mise/discussions/6513 — `_.source` was broken on
    // Windows (WSL launcher routing / literal /bin/bash fallback).
    #[tokio::test]
    #[cfg(windows)]
    async fn test_from_bash_script_windows() {
        let _config = Config::get().await.unwrap();
        let orig_path = r"C:\Windows\System32;C:\Windows";
        let ed = from_bash_script_windows(
            "export SOURCED_VAR=\"hello world\"\nexport PATH=\"/c/fake/prepended:$PATH\"\n",
            orig_path,
        );
        assert_eq!(
            ed.new.get("SOURCED_VAR").map(String::as_str),
            Some("hello world")
        );
        assert!(!ed.new.contains_key("EXISTING_VAR"));
        // the two-dump baseline keeps MSYS runtime/wrapper noise out of the diff
        assert!(!ed.new.contains_key("MSYSTEM"));
        assert!(
            !ed.new
                .keys()
                .any(|k| k.eq_ignore_ascii_case("TMP") || k.eq_ignore_ascii_case("TEMP"))
        );
        // the prepended entry comes back in Windows form, re-attached to the
        // original Windows-form PATH so EnvResults::source's strip_suffix works
        assert_eq!(
            ed.new.get(&*crate::env::PATH_KEY).map(String::as_str),
            Some(format!(r"C:\fake\prepended;{orig_path}").as_str())
        );
    }

    #[tokio::test]
    #[cfg(windows)]
    async fn test_from_bash_script_windows_skips_unconvertible_path_entries() {
        let _config = Config::get().await.unwrap();
        let ed = from_bash_script_windows(
            "export PATH=\"/usr/local/custom:$PATH\"\n",
            r"C:\Windows\System32;C:\Windows",
        );
        // `/usr/local/custom` has no Windows equivalent → no PATH change at all
        assert!(!ed.new.contains_key(&*crate::env::PATH_KEY));
    }

    #[tokio::test]
    #[cfg(windows)]
    async fn test_from_bash_script_windows_ignores_path_rewrite() {
        let _config = Config::get().await.unwrap();
        let ed = from_bash_script_windows(
            "export PATH=\"/c/only\"\n",
            r"C:\Windows\System32;C:\Windows",
        );
        // wholesale rewrite (not a prepend) → ignored, matching unix semantics
        assert!(!ed.new.contains_key(&*crate::env::PATH_KEY));
    }
}
