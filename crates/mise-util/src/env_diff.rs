use std::collections::{BTreeMap, BTreeSet};
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

/// Schema version of the `__MISE_DIFF` and `__MISE_SESSION` blobs. Version 1 (a blob with no
/// version field) stores the added env values as plaintext; version 2 stores a
/// [`hash_env_value`] digest instead.
pub const ENV_STATE_VERSION: u32 = 2;

/// What a blob without a version field is.
pub fn legacy_env_state_version() -> u32 {
    1
}

#[derive(Default, Serialize, Deserialize)]
pub struct EnvDiff {
    /// See [`ENV_STATE_VERSION`]. Decides how `new` is compared, see [`env_value_matches`].
    #[serde(default = "legacy_env_state_version")]
    pub v: u32,
    #[serde(default)]
    pub old: IndexMap<String, String>,
    #[serde(default)]
    pub new: IndexMap<String, String>,
    #[serde(default)]
    pub path: Vec<PathBuf>,
}

const HASH_PREFIX: &str = "blake3:";

/// Digest stored in place of an env value in `__MISE_DIFF` and `__MISE_SESSION`.
/// This is not a secret store: a low-entropy value can be brute-forced from its
/// digest, and the value itself is in every child's environment anyway. The
/// point is not to keep a second plaintext copy around.
pub fn hash_env_value(value: &str) -> String {
    format!("{HASH_PREFIX}{}", blake3::hash(value.as_bytes()).to_hex())
}

/// Whether `actual` is the value recorded as `stored` in a blob of schema `version`. Only the
/// version decides how values are compared: a shell that keeps running across an upgrade still
/// holds a version 1 blob of plaintext, which must reverse cleanly, while a version 2 blob holds
/// digests and a plaintext value that happens to look like one is not a match.
pub fn env_value_matches(version: u32, stored: &str, actual: &str) -> bool {
    if version >= 2 {
        stored == hash_env_value(actual)
    } else {
        stored == actual
    }
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

    /// Serializes for `__MISE_DIFF`. The `new` values are replaced by
    /// [`hash_env_value`] digests: they are only ever compared against the live
    /// environment (see [`env_value_matches`]), so secrets mise injected are
    /// not duplicated as plaintext in a variable every child process inherits.
    /// `old` stays as-is because restoring the environment needs those values.
    pub fn serialize(&self) -> Result<String> {
        self.serialize_excluding(&crate::env::INHERITED_SECRET_KEYS)
    }

    /// Like [`Self::serialize`], but drops `old` entries for `exclude`d keys so
    /// an inherited secret is never written in plaintext. Leaving a directory
    /// then unsets such a key instead of restoring it, which fails closed.
    pub fn serialize_excluding(&self, exclude: &BTreeSet<String>) -> Result<String> {
        #[derive(Serialize)]
        struct Wire<'a> {
            v: u32,
            old: IndexMap<&'a str, &'a str>,
            new: IndexMap<&'a str, String>,
            path: &'a [PathBuf],
        }
        let wire = Wire {
            v: ENV_STATE_VERSION,
            old: self
                .old
                .iter()
                .filter(|(k, _)| !exclude.iter().any(|e| crate::env::env_key_eq(k, e)))
                .map(|(k, v)| (k.as_str(), v.as_str()))
                .collect(),
            new: self
                .new
                .iter()
                .map(|(k, v)| (k.as_str(), hash_env_value(v)))
                .collect(),
            path: &self.path,
        };
        let mut gz = ZlibEncoder::new(Vec::new(), Compression::fast());
        gz.write_all(&rmp_serde::to_vec_named(&wire)?)?;
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
            // `new` now holds the plaintext originals.
            v: legacy_env_state_version(),
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
    use super::*;
    use pretty_assertions::assert_str_eq;

    #[test]
    fn test_invalid_escape_sequence() {
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

    #[test]
    fn test_serialize_excluding_drops_old_entries_for_excluded_keys() {
        let diff = EnvDiff {
            v: ENV_STATE_VERSION,
            old: [
                ("SECRET".into(), "inherited-s3cr3t".into()),
                ("OTHER".into(), "kept".into()),
            ]
            .into(),
            new: [("SECRET".into(), "overridden".into())].into(),
            path: vec![],
        };
        let exclude: BTreeSet<String> = ["SECRET".to_string()].into();
        let raw = diff.serialize_excluding(&exclude).unwrap();
        let back = EnvDiff::deserialize(&raw).unwrap();
        assert!(!back.old.contains_key("SECRET"));
        assert_eq!(back.old.get("OTHER").map(String::as_str), Some("kept"));
        assert!(back.new.contains_key("SECRET"));
    }

    #[cfg(windows)]
    #[test]
    fn test_serialize_excluding_ignores_key_case_on_windows() {
        let diff = EnvDiff {
            v: ENV_STATE_VERSION,
            old: [("Secret".into(), "inherited-s3cr3t".into())].into(),
            new: IndexMap::new(),
            path: vec![],
        };
        let exclude: BTreeSet<String> = ["SECRET".to_string()].into();
        let back = EnvDiff::deserialize(&diff.serialize_excluding(&exclude).unwrap()).unwrap();
        assert!(back.old.is_empty());
    }

    #[test]
    fn test_serialize_does_not_store_new_values() {
        let original: EnvMap = [("CHANGED".into(), "before".into())].into();
        let diff = EnvDiff::new(
            &original,
            [
                ("CHANGED".to_string(), "after-secret".to_string()),
                ("ADDED".to_string(), "added-secret".to_string()),
            ],
        );
        let raw = diff.serialize().unwrap();
        let back = EnvDiff::deserialize(&raw).unwrap();

        assert_eq!(back.old.get("CHANGED").map(String::as_str), Some("before"));
        for value in back.new.values() {
            assert!(!value.contains("secret"), "{value}");
        }
        assert!(env_value_matches(
            back.v,
            &back.new["CHANGED"],
            "after-secret"
        ));
        assert!(!env_value_matches(back.v, &back.new["CHANGED"], "other"));
        assert!(env_value_matches(
            back.v,
            &back.new["ADDED"],
            "added-secret"
        ));
    }

    #[test]
    fn test_env_value_matches_by_version() {
        // v1: plaintext
        assert!(env_value_matches(1, "plain", "plain"));
        assert!(!env_value_matches(1, "plain", "other"));
        // v2: digests only
        assert!(env_value_matches(2, &hash_env_value("plain"), "plain"));
        assert!(!env_value_matches(2, "plain", "plain"));
        // a plaintext value that looks like a digest is not a match in v2
        let lookalike = hash_env_value("x");
        assert!(!env_value_matches(2, &lookalike, &lookalike));
        // ...and v1 does not hash
        assert!(!env_value_matches(1, &hash_env_value("plain"), "plain"));
    }

    #[test]
    fn test_blob_without_version_is_v1_and_reserializes_as_v2() {
        // what the previous mise wrote: no version field, plaintext `new`
        #[derive(Serialize)]
        struct Legacy {
            old: IndexMap<String, String>,
            new: IndexMap<String, String>,
            path: Vec<PathBuf>,
        }
        let legacy = Legacy {
            old: IndexMap::new(),
            new: [("K".to_string(), "secret".to_string())].into(),
            path: vec![],
        };
        let mut gz = ZlibEncoder::new(Vec::new(), Compression::fast());
        gz.write_all(&rmp_serde::to_vec_named(&legacy).unwrap())
            .unwrap();
        let raw = BASE64_STANDARD_NO_PAD.encode(gz.finish().unwrap());

        let diff = EnvDiff::deserialize(&raw).unwrap();
        assert_eq!(diff.v, 1);
        assert!(env_value_matches(diff.v, &diff.new["K"], "secret"));

        let rewritten = EnvDiff::deserialize(&diff.serialize().unwrap()).unwrap();
        assert_eq!(rewritten.v, ENV_STATE_VERSION);
        assert!(!rewritten.new["K"].contains("secret"));
        assert!(env_value_matches(
            rewritten.v,
            &rewritten.new["K"],
            "secret"
        ));
    }
}
