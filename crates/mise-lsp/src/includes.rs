//! Task include files.
//!
//! `[task_config] includes` names files and directories of tasks. A file there can be called
//! `mise.tasks.toml`, which a name check alone would take for a config, so the server looks at
//! the configs above a file (parsing them, nothing more) to see whether one includes it.

use std::path::{Path, PathBuf};

use regex::Regex;
use toml_edit::{Document, Item};

/// A config file is never a bigger thing than this; skip anything that is.
const MAX_CONFIG_BYTES: u64 = 1 << 20;

/// More entries than this in a directory is not a config directory worth reading.
const MAX_DIR_ENTRIES: usize = 500;

/// Whether the file at `path` (with `/` between its parts) is one mise loads as a config:
/// `mise.toml`, `.mise.toml`, `mise.<env>.toml`, `mise.local.toml`, `mise/config.toml`,
/// `.config/mise/config.toml`, and the files of a `conf.d` beside them.
pub(crate) fn is_config_file(path: &str) -> bool {
    let mut parts = path.rsplit('/');
    let name = parts.next().unwrap_or_default();
    let parent = parts.next().unwrap_or_default();
    let grandparent = parts.next().unwrap_or_default();
    let in_mise_dir = |dir: &str| dir == "mise" || dir == ".mise";
    let Some(stem) = name.strip_suffix(".toml") else {
        return false;
    };
    let stem = stem.strip_prefix('.').unwrap_or(stem);
    let first = stem.split('.').next().unwrap_or_default();
    (first == "mise" && stem != "miserc")
        || (first == "config" && in_mise_dir(parent))
        || (parent == "conf.d" && in_mise_dir(grandparent))
}

/// The names mise itself gives configs. A directory include or a glob does not claim these, so
/// that `includes = ["."]` does not turn the configs below it into task files. `mise.<env>.toml`
/// and `mise.<env>.local.toml` are among them, except `mise.tasks.toml`, `mise.tasks.<x>.toml` and
/// the same with `task`, which name task files; an include does claim those.
pub(crate) fn is_standard_config(path: &str) -> bool {
    let mut parts = path.rsplit('/');
    let name = parts.next().unwrap_or_default();
    let parent = parts.next().unwrap_or_default();
    let grandparent = parts.next().unwrap_or_default();
    let in_mise_dir = |dir: &str| dir == "mise" || dir == ".mise";
    if (name == "config.toml" && in_mise_dir(parent))
        || (parent == "conf.d" && in_mise_dir(grandparent))
    {
        return true;
    }
    let Some(stem) = name
        .strip_prefix('.')
        .unwrap_or(name)
        .strip_suffix(".toml")
        .filter(|stem| is_config_file(name) && !stem.is_empty())
    else {
        return false;
    };
    let mut segments = stem.split('.');
    segments.next() == Some("mise") && !matches!(segments.next(), Some("tasks" | "task"))
}

/// The config files in `dir` and the places a project keeps them below it.
pub(crate) fn config_files(dir: &Path) -> Vec<PathBuf> {
    // The names mise itself uses are read outright, so a directory too big to scan still gets
    // its standard configs read.
    let mut out: Vec<PathBuf> = [
        "mise.toml",
        ".mise.toml",
        "mise.local.toml",
        ".mise.local.toml",
    ]
    .iter()
    .map(|name| dir.join(name))
    .collect();
    for sub in [".config/mise", "mise", ".mise"] {
        out.push(dir.join(sub).join("config.toml"));
    }
    let mut scan = |d: PathBuf| {
        let Ok(entries) = std::fs::read_dir(&d) else {
            return;
        };
        for entry in entries.flatten().take(MAX_DIR_ENTRIES) {
            let path = entry.path();
            if is_config_file(&slashes(&path.to_string_lossy(), cfg!(windows)))
                && !out.contains(&path)
            {
                out.push(path);
            }
        }
    };
    scan(dir.to_path_buf());
    for sub in [".config", ".config/mise", "mise", ".mise"] {
        scan(dir.join(sub));
        scan(dir.join(sub).join("conf.d"));
    }
    out
}

/// Whether a config in one of `file`'s ancestor directories lists it in `task_config.includes`.
pub(crate) fn is_task_include(file: &Path) -> bool {
    file.ancestors().skip(1).any(|dir| {
        config_files(dir)
            .into_iter()
            .filter(|config| config != file)
            .any(|config| includes_file(&config, file))
    })
}

fn includes_file(config: &Path, file: &Path) -> bool {
    let Some(includes) = read_includes(config) else {
        return false;
    };
    let bases = include_bases(config);
    includes
        .iter()
        .any(|include| bases.iter().any(|base| matches(&base.join(include), file)))
}

/// The directories a config's `includes` are relative to. `mise/config.toml` and its kin keep
/// tasks relative to the project, not the config dir: the project is above `conf.d`, `mise`,
/// `.mise` and `.config`, whichever of them the config is in.
pub(crate) fn include_bases(config: &Path) -> Vec<&Path> {
    let dir = config.parent().unwrap_or(Path::new(""));
    let mut bases = vec![];
    let mut base = Some(dir);
    while let Some(d) = base {
        bases.push(d);
        base = d
            .file_name()
            .is_some_and(|n| n == "conf.d" || n == "mise" || n == ".mise" || n == ".config")
            .then(|| d.parent())
            .flatten();
    }
    bases
}

/// What a config's `[task_config] includes` names outright.
#[derive(Default)]
pub(crate) struct IncludeFiles {
    /// The TOML files: the ones named, and those below a directory named.
    pub files: Vec<PathBuf>,
    /// The directories named, which may hold file tasks (scripts) as well.
    pub dirs: Vec<PathBuf>,
    /// Whether an include was left unread: a glob, a `git::` or `oci::` source, anything else
    /// that is not a path on disk, or a walk that ran out of its budget.
    pub skipped: bool,
}

/// Entries one `task_include_files` call looks at, in all the directories it walks.
const MAX_SCAN: usize = 2000;

pub(crate) fn task_include_files(config: &Path, includes: &[String]) -> IncludeFiles {
    let mut out = IncludeFiles::default();
    // One budget for everything an include walks, so a broad include (`.`) costs a bounded scan.
    let mut budget = MAX_SCAN;
    for include in includes {
        // a template is never rendered here, so what it names is unknown
        if include.contains("::") || include.contains(['*', '?', '[', '{']) {
            out.skipped = true;
            continue;
        }
        for base in include_bases(config) {
            let path = base.join(include);
            if path.is_dir() {
                toml_files_below(&path, 0, &mut budget, &mut out);
                if !out.dirs.contains(&path) {
                    out.dirs.push(path);
                }
            } else if path.extension().is_some_and(|ext| ext == "toml")
                && !out.files.contains(&path)
            {
                out.files.push(path);
            }
        }
    }
    out.skipped |= budget == 0;
    out
}

/// The TOML files below `dir` that are task files: not the configs mise names itself, and not
/// behind a symlink. Stops when `budget` entries have been looked at, or at a depth of three,
/// and says so in `out.skipped`.
fn toml_files_below(dir: &Path, depth: usize, budget: &mut usize, out: &mut IncludeFiles) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if *budget == 0 {
            return;
        }
        *budget -= 1;
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            if depth < 3 {
                toml_files_below(&path, depth + 1, budget, out);
            } else {
                out.skipped = true;
            }
        } else if kind.is_file()
            && path.extension().is_some_and(|ext| ext == "toml")
            && !is_standard_config(&slashes(&path.to_string_lossy(), cfg!(windows)))
            && !out.files.contains(&path)
        {
            out.files.push(path);
        }
    }
}

/// The text of a config file, unless it is too big to be one.
pub(crate) fn read_capped(path: &Path) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_CONFIG_BYTES {
        return None;
    }
    std::fs::read_to_string(path).ok()
}

fn read_includes(config: &Path) -> Option<Vec<String>> {
    let doc = Document::parse(read_capped(config)?).ok()?;
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
    if file == pattern {
        return true;
    }
    if file.starts_with(pattern) {
        return !is_standard_config(&slashes(&file.to_string_lossy(), cfg!(windows)));
    }
    // A glob that opens its last part with a wildcard (`*.toml`) does not claim the configs mise
    // names itself, or it would take `mise.toml`. One that spells the start of the name
    // (`tasks/mise.*.toml`) means the files it matches, whatever they are called.
    let file = slashes(&file.to_string_lossy(), cfg!(windows));
    let pattern = slashes(&pattern.to_string_lossy(), cfg!(windows));
    let last = pattern.rsplit('/').next().unwrap_or_default();
    let names_the_files = !last.starts_with(['*', '?', '[']);
    glob_matches(&pattern, &file) && (names_the_files || !is_standard_config(&file))
}

/// A path with `/` between its parts, as a glob has: `Path::join` writes `\` on Windows.
fn slashes(path: &str, windows: bool) -> String {
    if windows {
        path.replace('\\', "/")
    } else {
        path.to_string()
    }
}

fn glob_matches(pattern: &str, file: &str) -> bool {
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
    Regex::new(&regex).is_ok_and(|re| re.is_match(file))
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

/// The `file://` URI of a path, with the characters a URI does not allow escaped.
pub(crate) fn path_uri(path: &Path) -> String {
    let path = slashes(&path.to_string_lossy(), cfg!(windows));
    let mut uri = String::from("file://");
    if !path.starts_with('/') {
        uri.push('/');
    }
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || b"/-_.~:".contains(&byte) {
            uri.push(byte as char);
        } else {
            uri.push_str(&format!("%{byte:02X}"));
        }
    }
    uri
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
    fn paths_become_uris_and_back() {
        let path = Path::new("/home/me/My Dir/mise.toml");
        let uri = path_uri(path);
        assert_eq!(uri, "file:///home/me/My%20Dir/mise.toml");
        assert_eq!(uri_path(&uri).unwrap(), path);
    }

    #[test]
    fn includes_the_server_cannot_read_are_skipped() {
        let root = std::env::temp_dir().join(format!("mise-lsp-skip-{}", std::process::id()));
        std::fs::create_dir_all(root.join("a/b/c/d/e")).unwrap();
        std::fs::write(root.join("a/b/c/d/e/deep.toml"), "").unwrap();
        let config = root.join("mise.toml");
        let skipped = |include: &str| task_include_files(&config, &[include.to_string()]).skipped;
        assert!(skipped("tasks/{{ env.X }}.toml"));
        assert!(skipped("tasks/*.toml"));
        assert!(skipped("a"), "a directory deeper than the walk goes");
        assert!(!skipped("a/b/c/d/e"));
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
    fn windows_separators_are_normalized_before_matching() {
        let pattern = slashes(r"C:\p\*.tasks.toml", true);
        let file = slashes(r"C:/p/mise.tasks.toml", true);
        assert!(glob_matches(&pattern, &file));
        let class = slashes(r"C:\p\mise.tasks.[ab].toml", true);
        assert!(glob_matches(
            &class,
            &slashes(r"C:\p\mise.tasks.a.toml", true)
        ));
    }

    #[test]
    fn a_directory_include_leaves_standard_configs_alone() {
        let dir = Path::new("/p");
        assert!(!matches(Path::new("/p"), &dir.join("mise.toml")));
        assert!(!matches(Path::new("/p"), &dir.join("mise.test.toml")));
        assert!(!matches(
            Path::new("/p"),
            &dir.join(".config/mise/conf.d/a.toml")
        ));
        assert!(!matches(Path::new("/p"), &dir.join(".mise.test.toml")));
        assert!(matches(Path::new("/p"), &dir.join("tasks/build.toml")));
        assert!(matches(Path::new("/p"), &dir.join("mise.tasks.toml")));
        // named outright, any file is a task file
        assert!(matches(&dir.join("mise.toml"), &dir.join("mise.toml")));
    }

    #[test]
    fn env_local_configs_are_standard() {
        assert!(is_standard_config("/p/mise.dev.local.toml"));
        assert!(is_standard_config("/p/.mise.dev.toml"));
        assert!(is_standard_config("/p/mise.toml"));
        assert!(!is_standard_config("/p/mise.tasks.toml"));
        assert!(!is_standard_config("/p/build.toml"));
        assert!(!is_standard_config("/p/.miserc.toml"));
    }

    #[test]
    fn includes_in_env_and_conf_d_configs_count() {
        let root = std::env::temp_dir().join(format!("mise-lsp-envcfg-{}", std::process::id()));
        std::fs::create_dir_all(root.join(".config/mise/conf.d")).unwrap();
        std::fs::write(root.join("mise.toml"), "[tools]\n").unwrap();
        std::fs::write(
            root.join("mise.dev.toml"),
            "[task_config]\nincludes = [\"dev.tasks.toml\"]\n",
        )
        .unwrap();
        std::fs::write(
            root.join(".config/mise/conf.d/10-tasks.toml"),
            "[task_config]\nincludes = [\"mise.tasks.toml\"]\n",
        )
        .unwrap();
        let dev = root.join("dev.tasks.toml");
        let tasks = root.join("mise.tasks.toml");
        std::fs::write(&dev, "[a]\n").unwrap();
        std::fs::write(&tasks, "[a]\n").unwrap();
        assert!(is_task_include(&dev));
        assert!(is_task_include(&tasks));
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
        // a glob leaves the standard configs alone too
        assert!(!matches(&dir.join("*.toml"), &dir.join("mise.toml")));
        assert!(!matches(
            &dir.join("**/*.toml"),
            &dir.join("mise/config.toml")
        ));
        assert!(matches(&dir.join("*.toml"), &dir.join("build.toml")));
        // a glob that spells the start of the name claims what it matches
        assert!(matches(
            &dir.join("tasks/mise.*.toml"),
            &dir.join("tasks/mise.build.toml")
        ));
    }
}
