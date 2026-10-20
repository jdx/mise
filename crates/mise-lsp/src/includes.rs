//! Task include files.
//!
//! `[task_config] includes` names files and directories of tasks. A file there can be called
//! `mise.tasks.toml`, which a name check alone would take for a config, so the server looks at
//! the configs above a file (parsing them, nothing more) to see whether one includes it.

use std::path::{Path, PathBuf};

use regex::Regex;
use toml_edit::{Document, Item};

/// Where a project keeps its config, relative to a directory.
const CONFIGS: &[&str] = &[
    "mise.toml",
    ".mise.toml",
    "mise.local.toml",
    ".mise.local.toml",
    ".config/mise.toml",
    ".config/mise/config.toml",
    "mise/config.toml",
    ".mise/config.toml",
];

/// A config file is never a bigger thing than this; skip anything that is.
const MAX_CONFIG_BYTES: u64 = 1 << 20;

/// Whether a config in one of `file`'s ancestor directories lists it in `task_config.includes`.
pub(crate) fn is_task_include(file: &Path) -> bool {
    file.ancestors().skip(1).any(|dir| {
        CONFIGS
            .iter()
            .map(|config| dir.join(config))
            .filter(|config| config != file)
            .any(|config| includes_file(&config, file))
    })
}

fn includes_file(config: &Path, file: &Path) -> bool {
    let Some(includes) = read_includes(config) else {
        return false;
    };
    let dir = config.parent().unwrap_or(Path::new(""));
    // `mise/config.toml` and its kin keep tasks relative to the project, not the config dir:
    // the project is the directory above `mise`, `.mise` or `.config`, and above `.config` for
    // `.config/mise/config.toml`.
    let mut bases = vec![dir];
    if let Some(parent) = dir.parent()
        && dir
            .file_name()
            .is_some_and(|n| n == "mise" || n == ".mise" || n == ".config")
    {
        bases.push(parent);
        if parent.file_name().is_some_and(|n| n == ".config")
            && let Some(project) = parent.parent()
        {
            bases.push(project);
        }
    }
    includes
        .iter()
        .any(|include| bases.iter().any(|base| matches(&base.join(include), file)))
}

fn read_includes(config: &Path) -> Option<Vec<String>> {
    let meta = std::fs::metadata(config).ok()?;
    if !meta.is_file() || meta.len() > MAX_CONFIG_BYTES {
        return None;
    }
    let text = std::fs::read_to_string(config).ok()?;
    let doc = Document::parse(text).ok()?;
    let includes = doc.get("task_config")?.get("includes")?;
    let Item::Value(value) = includes else {
        return None;
    };
    Some(
        value
            .as_array()?
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect(),
    )
}

/// `file` is `pattern`, is inside the directory `pattern`, or matches the glob `pattern`.
fn matches(pattern: &Path, file: &Path) -> bool {
    if file == pattern || file.starts_with(pattern) {
        return true;
    }
    let pattern = pattern.to_string_lossy();
    if !pattern.contains(['*', '?', '[']) {
        return false;
    }
    let mut regex = String::from("^");
    let mut chars = pattern.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '*' if chars.peek() == Some(&'*') => {
                chars.next();
                if chars.peek() == Some(&'/') {
                    chars.next();
                    regex.push_str("(.*/)?");
                } else {
                    regex.push_str(".*");
                }
            }
            '*' => regex.push_str("[^/]*"),
            '?' => regex.push_str("[^/]"),
            '[' => {
                // a character class: `[ab]`, `[a-z]`, `[!x]`
                let mut class = String::new();
                let mut closed = false;
                for c in chars.by_ref() {
                    if c == ']' && !class.is_empty() {
                        closed = true;
                        break;
                    }
                    class.push(c);
                }
                if closed {
                    let class = match class.strip_prefix('!') {
                        Some(rest) => format!("^{rest}"),
                        None => class,
                    };
                    regex.push_str(&format!("[{}]", class.replace('\\', "\\\\")));
                } else {
                    regex.push_str(&regex::escape(&format!("[{class}")));
                }
            }
            c => regex.push_str(&regex::escape(&c.to_string())),
        }
    }
    regex.push('$');
    let file = file.to_string_lossy().into_owned();
    Regex::new(&regex).is_ok_and(|re| re.is_match(&file))
}

/// The file a `file://` URI names, with escapes decoded.
pub(crate) fn uri_path(uri: &str) -> Option<PathBuf> {
    let path = percent_decode(uri.strip_prefix("file://")?);
    // `file:///C:/dir/file` is the Windows path `C:/dir/file`.
    let bytes = path.as_bytes();
    if bytes.len() >= 3 && bytes[0] == b'/' && bytes[1].is_ascii_alphabetic() && bytes[2] == b':' {
        return Some(PathBuf::from(&path[1..]));
    }
    Some(PathBuf::from(path))
}

pub(crate) fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(byte) = s
                .get(i + 1..i + 3)
                .and_then(|hex| u8::from_str_radix(hex, 16).ok())
        {
            out.push(byte);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(config: &str) -> (PathBuf, PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "mise-lsp-test-{}-{}",
            std::process::id(),
            config.len()
        ));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("mise.toml"), config).unwrap();
        let file = dir.join("mise.tasks.toml");
        std::fs::write(&file, "[build]\nrun = \"x\"\n").unwrap();
        (dir, file)
    }

    #[test]
    fn a_listed_file_is_an_include() {
        let (_, file) = project("[task_config]\nincludes = [\"mise.tasks.toml\"]\n");
        assert!(is_task_include(&file));
    }

    #[test]
    fn an_unlisted_file_is_not() {
        let (_, file) = project("[task_config]\nincludes = [\"other.toml\"]\n# unlisted\n");
        assert!(!is_task_include(&file));
    }

    #[test]
    fn windows_drive_uris_drop_the_leading_slash() {
        assert_eq!(
            uri_path("file:///C:/Users/me/mise.toml").unwrap(),
            PathBuf::from("C:/Users/me/mise.toml")
        );
        assert_eq!(
            uri_path("file:///home/me/mise.toml").unwrap(),
            PathBuf::from("/home/me/mise.toml")
        );
    }

    #[test]
    fn xdg_configs_include_relative_to_the_project() {
        let root = std::env::temp_dir().join(format!("mise-lsp-xdg-{}", std::process::id()));
        std::fs::create_dir_all(root.join(".config/mise")).unwrap();
        std::fs::write(
            root.join(".config/mise/config.toml"),
            "[task_config]\nincludes = [\"mise.tasks.toml\"]\n",
        )
        .unwrap();
        let file = root.join("mise.tasks.toml");
        std::fs::write(&file, "[build]\n").unwrap();
        assert!(is_task_include(&file));
    }

    #[test]
    fn globs_and_directories_match() {
        let dir = Path::new("/p");
        assert!(matches(
            &dir.join("*.tasks.toml"),
            &dir.join("mise.tasks.toml")
        ));
        assert!(matches(&dir.join("tasks"), &dir.join("tasks/a/b.toml")));
        assert!(matches(&dir.join("**/*.toml"), &dir.join("a/b/c.toml")));
        assert!(!matches(&dir.join("tasks"), &dir.join("other/a.toml")));
        assert!(matches(
            &dir.join("mise.tasks.[ab].toml"),
            &dir.join("mise.tasks.a.toml")
        ));
        assert!(!matches(
            &dir.join("mise.tasks.[ab].toml"),
            &dir.join("mise.tasks.c.toml")
        ));
        assert!(matches(&dir.join("t.[!ab].toml"), &dir.join("t.c.toml")));
    }
}
