//! The human-readable half of an install directory name, `<label>-<hash>`.
//!
//! The label only tells a person (and an error message) what a directory is;
//! the hash carries the identity. It is derived from the backend identifier,
//! not the registry shorthand, so a registry change never moves an install.
//! The rule is fixed once shipped, because changing it would change every path.
//! The same Windows-safe rule runs on every platform.

const MAX_LABEL_CHARS: usize = 24;

/// Derive the label for a canonical backend identifier.
pub(crate) fn label_for(backend: &str) -> String {
    let mut name = backend;
    // 1. Drop `[options]`, the backend prefix, a trailing `.git`.
    if let Some(i) = name.find('[') {
        name = &name[..i];
    }
    if let Some((prefix, rest)) = name.split_once(':')
        && !prefix.contains('/')
    {
        name = rest;
    }
    let name = name.strip_suffix(".git").unwrap_or(name);

    // 2. The last path segment, skipping Go major-version segments (`v2`).
    let mut segment = name
        .rsplit('/')
        .find(|s| !s.is_empty() && !is_go_major_version(s))
        .unwrap_or("");
    // A trailing `@version`; the `@` of an npm scope sits at the start.
    if let Some(i) = segment.find('@')
        && i > 0
    {
        segment = &segment[..i];
    }

    // 3. Drop a leading `@`, lowercase, make it filesystem safe, cap it.
    // A leading dot would make the directory hidden, and `.mise` is reserved
    // for the catalog, so dots are dropped from the front (`.dotfiles` -> `dotfiles`).
    let label: String = segment
        .trim_start_matches('@')
        .to_lowercase()
        .chars()
        .map(|c| match c {
            'a'..='z' | '0'..='9' | '.' | '_' | '-' => c,
            _ => '-',
        })
        .skip_while(|c| *c == '.')
        .take(MAX_LABEL_CHARS)
        .collect();
    if label.is_empty() {
        "tool".to_string()
    } else {
        label
    }
}

fn is_go_major_version(segment: &str) -> bool {
    segment
        .strip_prefix('v')
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_the_documented_table() {
        for (backend, label) in [
            ("aqua:FiloSottile/age", "age"),
            ("core:node", "node"),
            ("aqua:yarnpkg/berry", "berry"),
            (
                "go:github.com/slsa-framework/slsa-verifier/v2/cli/slsa-verifier",
                "slsa-verifier",
            ),
            (
                "go:github.com/oapi-codegen/oapi-codegen/v2/cmd/oapi-codegen",
                "oapi-codegen",
            ),
            ("npm:@yarnpkg/cli-dist", "cli-dist"),
        ] {
            assert_eq!(label_for(backend), label, "{backend}");
        }
    }

    #[test]
    fn strips_options_version_and_git_suffix() {
        assert_eq!(
            label_for("github:restatedev/restate[matching=restate-server]"),
            "restate"
        );
        assert_eq!(label_for("npm:prettier@3.0.0"), "prettier");
        assert_eq!(label_for("asdf:https://github.com/x/asdf-y.git"), "asdf-y");
        assert_eq!(label_for("go:github.com/foo/bar/v2"), "bar");
        // `v0` and `v1` are major-version segments too; `vx` is a real name.
        assert_eq!(label_for("go:example.com/tool/v10"), "tool");
        assert_eq!(label_for("github:owner/vx"), "vx");
    }

    #[test]
    fn is_lowercase_safe_and_capped() {
        assert_eq!(label_for("github:Owner/My_Tool.CLI"), "my_tool.cli");
        assert_eq!(label_for("github:o/spaces and:colons"), "spaces-and-colons");
        assert_eq!(label_for("github:o/ünïcode"), "-n-code");
        let long = label_for("github:o/abcdefghijklmnopqrstuvwxyz0123456789");
        assert_eq!(long, "abcdefghijklmnopqrstuvwx");
        assert_eq!(long.chars().count(), MAX_LABEL_CHARS);
    }

    #[test]
    fn leading_dots_are_dropped_so_the_directory_is_not_hidden() {
        assert_eq!(label_for("github:o/.dotfiles"), "dotfiles");
        assert_eq!(label_for("github:o/...x"), "x");
        assert_eq!(label_for("github:o/.mise"), "mise");
        assert_eq!(label_for("github:o/."), "tool");
        assert_eq!(label_for("github:o/..."), "tool");
    }

    #[test]
    fn falls_back_to_tool() {
        assert_eq!(label_for(""), "tool");
        assert_eq!(label_for("github:"), "tool");
        assert_eq!(label_for("npm:@"), "tool");
    }

    #[test]
    fn plugin_and_bare_names() {
        assert_eq!(label_for("vfox:version-fox/vfox-nodejs"), "vfox-nodejs");
        assert_eq!(label_for("nodejs"), "nodejs");
    }
}
