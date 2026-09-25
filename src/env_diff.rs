pub(crate) use mise_util::env_diff::*;

#[cfg(test)]
mod tests {
    use crate::config::Config;
    use crate::env::PATH_KEY;
    use std::path::PathBuf;

    use super::*;

    use insta::assert_debug_snapshot;

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
