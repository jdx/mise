//! Early initialization settings from .miserc.toml and .miserc.local.toml
//!
//! This module handles loading settings that need to be known before the main
//! config files are parsed. The primary use case is setting MISE_ENV, which
//! determines which environment-specific config files (e.g., mise.development.toml)
//! to load.

use crate::config::SettingsExt;
use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;

use eyre::Result;
use path_absolutize::Absolutize;
use tera::Context;

use crate::config::config_file::diagnostic::toml_parse_error;
use crate::config::settings::{MisercSettings, warn_settings_file_error};
use crate::dirs;
use crate::env;
use crate::env_diff::EnvMap;
use crate::file;
use crate::tera::{
    TeraEngine, contains_template_syntax, get_miserc_tera, render_str, render_str_with_env,
    take_tera_accessed_files,
};

static MISERC: OnceLock<MisercSettings> = OnceLock::new();
static INVOCATION_CWD: OnceLock<Option<PathBuf>> = OnceLock::new();

/// Load operator-owned environment selection without discovering project files.
pub(crate) fn init_global_only() {
    let _ = MISERC.set(load_global_miserc_settings());
    let _ = take_tera_accessed_files();
}

/// Merge the system and global miserc files, skipping project files found
/// from the working directory.
fn load_global_miserc_settings() -> MisercSettings {
    if super::Settings::no_config() {
        return MisercSettings::default();
    }
    let mut settings = MisercSettings::default();
    // A broken file must not block credentials or discard another valid layer.
    for path in [
        env::MISE_SYSTEM_CONFIG_DIR.join("miserc.toml"),
        dirs::CONFIG.join("miserc.toml"),
        dirs::CONFIG.join("miserc.local.toml"),
    ] {
        if let Ok(layer) = load_miserc_files(vec![path]) {
            merge_settings(&mut settings, layer);
        }
    }
    settings
}

/// Initialize miserc settings by loading shared and local miserc files.
/// This must be called early in the initialization process, before
/// MISE_ENV or other early settings are accessed.
pub fn init() -> Result<()> {
    let _ = invocation_cwd();
    // `main` initializes this before its hook-env fast path, and `cli::run`
    // calls again when that path falls through; a second load would only be
    // discarded.
    if MISERC.get().is_some() {
        return Ok(());
    }
    let settings = load_miserc_settings()?;
    let _ = MISERC.set(settings);
    // Discard any files tracked via hash_file/file_size/last_modified during miserc
    // template rendering. Those filters write to TERA_ACCESSED_FILES (used by hook-env
    // for file-watch detection), but miserc is loaded before config and should not
    // contribute to that list.
    let _ = take_tera_accessed_files();
    Ok(())
}

/// The working directory mise was invoked from, before settings such as `cd`
/// change the process working directory.
pub(crate) fn invocation_cwd() -> Option<&'static Path> {
    INVOCATION_CWD
        .get_or_init(|| std::env::current_dir().ok())
        .as_deref()
}

/// Get the loaded miserc settings, or default if not initialized.
pub(crate) fn get() -> &'static MisercSettings {
    if super::Settings::is_package_query() || super::Settings::no_config() {
        static QUERY_MISERC: std::sync::LazyLock<MisercSettings> =
            std::sync::LazyLock::new(MisercSettings::default);
        return &QUERY_MISERC;
    }
    MISERC.get_or_init(|| {
        let settings = load_miserc_settings().unwrap_or_default();
        let _ = take_tera_accessed_files();
        settings
    })
}

/// Get the MISE_ENV value from miserc, if set.
pub(crate) fn get_env() -> Option<&'static Vec<String>> {
    get().env.as_ref()
}

/// Get the auto_env value from miserc, if set.
pub(crate) fn get_auto_env() -> Option<bool> {
    get().auto_env
}

/// Get the env_conf_d value from miserc, if set.
pub(crate) fn get_env_conf_d() -> Option<bool> {
    get().env_conf_d
}

/// Get the ceiling_paths value from miserc, if set.
pub(crate) fn get_ceiling_paths() -> Option<&'static BTreeSet<PathBuf>> {
    get().ceiling_paths.as_ref()
}

/// Get the ignored_config_paths value from miserc, if set.
pub(crate) fn get_ignored_config_paths() -> Option<&'static BTreeSet<PathBuf>> {
    get().ignored_config_paths.as_ref()
}

/// Get the ignored_config_paths value from the system and global miserc files
/// only. Unlike [`get_ignored_config_paths`], this does not depend on the
/// directory mise runs from, so machine-wide operations such as `mise prune`
/// reach the same verdict everywhere.
pub(crate) fn get_global_ignored_config_paths() -> Option<&'static BTreeSet<PathBuf>> {
    if super::Settings::no_config() {
        return None;
    }
    static GLOBAL: std::sync::LazyLock<Option<BTreeSet<PathBuf>>> =
        std::sync::LazyLock::new(|| {
            let settings = load_global_miserc_settings();
            let _ = take_tera_accessed_files();
            settings.ignored_config_paths
        });
    GLOBAL.as_ref()
}

/// Get the override_config_filenames value from miserc, if set.
pub(crate) fn get_override_config_filenames() -> Option<&'static Vec<String>> {
    get().override_config_filenames.as_ref()
}

/// Get the override_tool_versions_filenames value from miserc, if set.
pub(crate) fn get_override_tool_versions_filenames() -> Option<&'static Vec<String>> {
    get().override_tool_versions_filenames.as_ref()
}

/// Render any Tera template syntax in miserc content before TOML parsing.
/// Uses a minimal context that is safe to build before the main config is loaded:
/// - `env` – OS environment variables (from PRISTINE_ENV), in the global and
///   system files only; a project file gets an empty map and is checked by
///   [`check_project_template`] first
/// - `config_root` – directory containing the miserc file
/// - `cwd` – current working directory
/// - `xdg_*` – XDG base directory variables
///
/// Notably absent (would cause circular initialization):
/// - `mise_env` (depends on miserc itself)
/// - `exec()` (depends on Settings, which are not yet loaded)
/// - `read_file()` (not registered — needs per-file directory context not set up at this stage)
fn render_miserc_template(
    tera: &mut Option<TeraEngine>,
    content: &str,
    config_root: &Path,
) -> String {
    if !contains_template_syntax(content) {
        return content.to_string();
    }
    // Lazily initialize the Tera instance — only pay the clone cost if at least one file
    // contains template syntax.
    let global = is_global_miserc_dir(config_root);
    if !global && let Err(reason) = check_project_template(content) {
        // queued: the logger is not up yet while miserc loads
        warn_settings_file_error(format!(
            "not rendering the templates of the .miserc.toml in {}: {reason}",
            file::display_path(config_root)
        ));
        return content.to_string();
    }
    let tera = tera.get_or_insert_with(get_miserc_tera);
    let mut context = Context::new();
    // A project file loads without trust, and what it renders becomes
    // `MISE_ENV`, which picks the config files mise loads (and the hosts their
    // tools are fetched from), so it gets no environment.
    if global {
        context.insert("env", &*env::PRISTINE_ENV);
    } else {
        context.insert("env", &EnvMap::new());
    }
    context.insert("config_root", config_root);
    // Use the invocation directory, not the current one: the global ignore list
    // is rendered lazily, possibly after `-C` changed directory, and must match
    // what startup rendered.
    match invocation_cwd() {
        Some(dir) => context.insert("cwd", dir),
        None => debug!("miserc template: could not determine cwd, `cwd` will be unavailable"),
    };
    context.insert("xdg_cache_home", &*env::XDG_CACHE_HOME);
    context.insert("xdg_config_home", &*env::XDG_CONFIG_HOME);
    context.insert("xdg_data_home", &*env::XDG_DATA_HOME);
    context.insert("xdg_state_home", &*env::XDG_STATE_HOME);
    context.insert("mise_config_dir", &*env::MISE_CONFIG_DIR);
    context.insert("system_brew_prefix", &env::system_brew_prefix());
    let rendered = if global {
        render_str_with_env(tera, content, &context)
    } else {
        render_str(tera, content, &context)
    };
    match rendered {
        Ok(rendered) => rendered,
        Err(e) => {
            warn!("Failed to render template in miserc: {e}");
            content.to_string()
        }
    }
}

/// Whether the miserc file in `config_root` is the global or system one, which
/// the operator wrote.
fn is_global_miserc_dir(config_root: &Path) -> bool {
    config_root == *dirs::CONFIG || config_root == env::MISE_SYSTEM_CONFIG_DIR.as_path()
}

/// The most bytes of a project `.miserc.toml` that mise renders.
const PROJECT_TEMPLATE_MAX_LEN: usize = 16 * 1024;
/// The statements a project `.miserc.toml` may use. None of them loops or
/// assigns, so rendering takes one pass over the template.
const PROJECT_STATEMENTS: [&str; 4] = ["if", "elif", "else", "endif"];
/// The functions a project `.miserc.toml` may call: host facts, none of which
/// reads the environment or a file or builds a value of a requested size.
const PROJECT_FUNCTIONS: [&str; 6] = ["arch", "os", "os_family", "libc", "avx2", "num_cpus"];
/// The filters a project `.miserc.toml` may use. None of them reads the
/// environment or a file, and none makes its input much longer, so chaining
/// them cannot build a large value.
const PROJECT_FILTERS: [&str; 24] = [
    "default",
    "lower",
    "upper",
    "capitalize",
    "title",
    "trim",
    "trim_start",
    "trim_end",
    "basename",
    "dirname",
    "extname",
    "file_stem",
    "first",
    "last",
    "length",
    "str",
    "int",
    "float",
    "kebabcase",
    "snakecase",
    "lowercamelcase",
    "uppercamelcase",
    "shoutykebabcase",
    "shoutysnakecase",
];
/// The tests a project `.miserc.toml` may use; the ones that look at the
/// filesystem are left out.
const PROJECT_TESTS: [&str; 7] = [
    "defined",
    "undefined",
    "string",
    "number",
    "starting_with",
    "ending_with",
    "containing",
];
/// Words of the expression syntax that are followed by `(` without being calls.
const KEYWORDS: [&str; 5] = ["and", "or", "not", "in", "is"];

#[derive(Debug, PartialEq)]
enum TemplateToken<'a> {
    Ident(&'a str),
    Symbol(u8),
}

/// Checks that a project `.miserc.toml` template renders in bounded time and
/// memory. A project file loads without trust, and Tera bounds neither, so a
/// loop or a string doubled in a loop would hang or crash every command run
/// below the project. The template may therefore use only `if` statements and
/// the functions, filters and tests listed above. The scan follows Tera's
/// lexer (strings with `\` escapes, the tag delimiters) and rejects anything it
/// does not recognize.
fn check_project_template(content: &str) -> std::result::Result<(), String> {
    if content.len() > PROJECT_TEMPLATE_MAX_LEN {
        return Err(format!(
            "a template may be at most {PROJECT_TEMPLATE_MAX_LEN} bytes"
        ));
    }
    let bytes = content.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let Some(open) = content[i..].find('{').map(|pos| i + pos) else {
            break;
        };
        let close: &str = match bytes.get(open + 1) {
            Some(b'{') => "}}",
            Some(b'%') => "%}",
            Some(b'#') => {
                let end = content[open + 2..]
                    .find("#}")
                    .ok_or("a comment is not closed")?;
                i = open + 2 + end + 2;
                continue;
            }
            _ => {
                i = open + 1;
                continue;
            }
        };
        let (tokens, end) = scan_tag(content, open + 2, close)?;
        if close == "%}" {
            match tokens.first() {
                Some(TemplateToken::Ident(word)) if PROJECT_STATEMENTS.contains(word) => {}
                Some(TemplateToken::Ident(word)) => {
                    return Err(format!("the `{word}` statement is not allowed"));
                }
                _ => return Err("a statement must start with a keyword".to_string()),
            }
        }
        check_tag_tokens(&tokens)?;
        i = end;
    }
    Ok(())
}

/// Splits the tag that starts at `start` into identifiers and symbols, and
/// returns them with the index after the closing delimiter.
fn scan_tag<'a>(
    content: &'a str,
    start: usize,
    close: &str,
) -> std::result::Result<(Vec<TemplateToken<'a>>, usize), String> {
    let bytes = content.as_bytes();
    let mut tokens = vec![];
    let mut i = start;
    if bytes.get(i) == Some(&b'-') {
        i += 1;
    }
    loop {
        let Some(&c) = bytes.get(i) else {
            return Err("a tag is not closed".to_string());
        };
        if c.is_ascii_whitespace() {
            i += 1;
        } else if bytes[i..].starts_with(close.as_bytes()) {
            return Ok((tokens, i + close.len()));
        } else if c == b'-' && bytes[i + 1..].starts_with(close.as_bytes()) {
            return Ok((tokens, i + 1 + close.len()));
        } else if matches!(c, b'\'' | b'"' | b'`') {
            let mut j = i + 1;
            loop {
                match bytes.get(j) {
                    None => return Err("a string is not closed".to_string()),
                    Some(b'\\') => j += 2,
                    Some(&q) if q == c => break,
                    Some(_) => j += 1,
                }
            }
            i = j + 1;
        } else if c == b'_' || c.is_ascii_alphabetic() {
            let len = bytes[i..]
                .iter()
                .take_while(|&&c| c == b'_' || c.is_ascii_alphanumeric())
                .count();
            tokens.push(TemplateToken::Ident(&content[i..i + len]));
            i += len;
        } else {
            tokens.push(TemplateToken::Symbol(c));
            i += 1;
        }
    }
}

fn check_tag_tokens(tokens: &[TemplateToken<'_>]) -> std::result::Result<(), String> {
    // Tera 2 also loops inside expressions, in list comprehensions
    // (`[x for x in xs]`), and nesting them multiplies their sizes.
    if tokens.contains(&TemplateToken::Ident("for")) {
        return Err("a `for` loop is not allowed, including in a list comprehension".to_string());
    }
    for (idx, token) in tokens.iter().enumerate() {
        let TemplateToken::Ident(name) = token else {
            continue;
        };
        let prev = idx.checked_sub(1).and_then(|p| tokens.get(p));
        let before_prev = idx.checked_sub(2).and_then(|p| tokens.get(p));
        let next = tokens.get(idx + 1);
        if prev == Some(&TemplateToken::Symbol(b'|')) {
            if !PROJECT_FILTERS.contains(name) {
                return Err(format!("the `{name}` filter is not allowed"));
            }
        } else if prev == Some(&TemplateToken::Ident("is"))
            || (prev == Some(&TemplateToken::Ident("not"))
                && before_prev == Some(&TemplateToken::Ident("is")))
        {
            if *name != "not" && !PROJECT_TESTS.contains(name) {
                return Err(format!("the `{name}` test is not allowed"));
            }
        } else if next == Some(&TemplateToken::Symbol(b'('))
            && !KEYWORDS.contains(name)
            && !PROJECT_FUNCTIONS.contains(name)
        {
            return Err(format!("the `{name}()` function is not allowed"));
        }
    }
    Ok(())
}

/// Load and merge all miserc settings files.
/// Precedence (highest to lowest):
/// 1. Closest directory first: .miserc.local.toml, .miserc.toml, .config/miserc.toml
/// 2. Global ~/.config/mise/miserc.local.toml, then miserc.toml
/// 3. System /etc/mise/miserc.toml
fn load_miserc_settings() -> Result<MisercSettings> {
    if super::Settings::no_config() {
        return Ok(MisercSettings::default());
    }
    load_miserc_files(find_miserc_files())
}

fn load_miserc_files(files: Vec<PathBuf>) -> Result<MisercSettings> {
    let mut merged = MisercSettings::default();
    // Load in reverse precedence order so later loads override earlier ones
    // Tera is initialized lazily inside render_miserc_template — only paid if a file
    // actually contains template syntax. Shared across all files to avoid redundant clones.
    let mut tera: Option<TeraEngine> = None;

    for path in files.into_iter().rev() {
        if let Ok(content) = file::read_to_string(&path) {
            let config_root = path.parent().unwrap_or(Path::new("."));
            let content = render_miserc_template(&mut tera, &content, config_root);
            let mut settings = toml::from_str::<MisercSettings>(&content)
                .map_err(|e| toml_parse_error(&e, &content, &path))?;
            resolve_ignored_config_paths(&mut settings, config_root);
            drop_names_leaving_the_directory(&mut settings, &path);
            merge_settings(&mut merged, settings);
        }
    }

    Ok(merged)
}

/// `env` and the override filenames only choose which files mise loads from the
/// directories it already searches: an environment name is one part of a filename
/// (`mise.<env>.toml`, `mise.<env>.lock`) and an override filename is relative to
/// each directory. A project `.miserc.toml` is read without trust, so a value that
/// would lead out of the directory must not become a path: `mise lock` deletes
/// the lockfiles it migrates and rewrites the ones it finds.
fn drop_names_leaving_the_directory(settings: &mut MisercSettings, path: &Path) {
    if let Some(envs) = settings.env.as_mut() {
        envs.retain(|name| {
            let valid = !name.contains(['/', '\\']);
            if !valid {
                warn_settings_file_error(format!(
                    "{}: ignoring environment {name:?}: a name cannot contain a path separator",
                    file::display_path(path)
                ));
            }
            valid
        });
    }
    for filenames in [
        settings.override_config_filenames.as_mut(),
        settings.override_tool_versions_filenames.as_mut(),
    ]
    .into_iter()
    .flatten()
    {
        filenames.retain(|filename| {
            let valid = !filename.contains('\\')
                && Path::new(filename)
                    .components()
                    .all(|c| matches!(c, Component::Normal(_) | Component::CurDir));
            if !valid {
                warn_settings_file_error(format!(
                    "{}: ignoring override filename {filename:?}: it must stay in its directory",
                    file::display_path(path)
                ));
            }
            valid
        });
    }
}

fn resolve_ignored_config_paths(settings: &mut MisercSettings, config_root: &Path) {
    let Some(paths) = settings.ignored_config_paths.take() else {
        return;
    };
    settings.ignored_config_paths = Some(
        paths
            .into_iter()
            .map(|path| resolve_ignored_config_path(path, config_root))
            .collect(),
    );
}

pub(crate) fn resolve_ignored_config_path(path: PathBuf, relative_to: &Path) -> PathBuf {
    file::replace_path(path)
        .absolutize_from(relative_to)
        .into_owned()
}

/// Merge source settings into target, where source values override target.
fn merge_settings(target: &mut MisercSettings, source: MisercSettings) {
    if source.env.is_some() {
        target.env = source.env;
    }
    if source.auto_env.is_some() {
        target.auto_env = source.auto_env;
    }
    if source.env_conf_d.is_some() {
        target.env_conf_d = source.env_conf_d;
    }
    if source.ceiling_paths.is_some() {
        target.ceiling_paths = source.ceiling_paths;
    }
    if source.ignored_config_paths.is_some() {
        target.ignored_config_paths = source.ignored_config_paths;
    }
    if source.override_config_filenames.is_some() {
        target.override_config_filenames = source.override_config_filenames;
    }
    if source.override_tool_versions_filenames.is_some() {
        target.override_tool_versions_filenames = source.override_tool_versions_filenames;
    }
}

/// Find all miserc.toml files in order of precedence (highest first).
fn find_miserc_files() -> Vec<PathBuf> {
    let mut files = Vec::new();
    let ceiling_paths = env_ceiling_paths();

    // Local overrides precede shared settings at each directory level.
    // Use raw std::env to avoid depending on our lazy statics
    if let Ok(cwd) = std::env::current_dir() {
        // Walk up the directory tree, but stop at home or root
        let home: &Path = &dirs::HOME;
        for dir in cwd.ancestors() {
            if ceiling_paths.contains(dir) {
                break;
            }
            for name in [".miserc.local.toml", ".miserc.toml"] {
                let path = dir.join(name);
                if path.is_file() {
                    files.push(path);
                }
            }
            // Stop at home directory to avoid searching too far
            if dir == home || dir.parent().is_none() {
                break;
            }
            let path = dir.join(".config").join("miserc.toml");
            if path.is_file() {
                files.push(path);
            }
        }
    }

    // Global local overrides the shared global miserc regardless of cwd.
    let global_local_path = dirs::CONFIG.join("miserc.local.toml");
    if global_local_path.is_file() {
        files.push(global_local_path);
    }

    // Global: ~/.config/mise/miserc.toml
    let global_path = dirs::CONFIG.join("miserc.toml");
    if global_path.is_file() {
        files.push(global_path);
    }

    // System: /etc/mise/miserc.toml (or MISE_SYSTEM_CONFIG_DIR)
    let system_dir = env::MISE_SYSTEM_CONFIG_DIR.clone();
    let system_path = system_dir.join("miserc.toml");
    if system_path.is_file() {
        files.push(system_path);
    }

    files
}

fn env_ceiling_paths() -> BTreeSet<PathBuf> {
    // Only the raw env var is available here; env::MISE_CEILING_PATHS also
    // falls back to .miserc, which would recurse during .miserc discovery.
    env::var_os("MISE_CEILING_PATHS")
        .map(|v| {
            std::env::split_paths(&v)
                .filter(|p| !p.as_os_str().is_empty())
                .map(file::replace_path)
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_merge_settings() {
        let mut target = MisercSettings {
            env: Some(vec!["base".to_string()]),
            env_conf_d: Some(false),
            ..Default::default()
        };

        let source = MisercSettings {
            env: Some(vec!["override".to_string()]),
            env_conf_d: Some(true),
            ..Default::default()
        };

        merge_settings(&mut target, source);

        assert_eq!(target.env, Some(vec!["override".to_string()]));
        assert_eq!(target.env_conf_d, Some(true));
    }

    #[test]
    fn test_parse_miserc() {
        let content = r#"
env = ["development", "local"]
env_conf_d = true
ceiling_paths = ["/home/user"]
"#;
        let settings: MisercSettings = toml::from_str(content).unwrap();
        assert_eq!(
            settings.env,
            Some(vec!["development".to_string(), "local".to_string()])
        );
        assert_eq!(settings.env_conf_d, Some(true));
        assert!(settings.ceiling_paths.is_some());
    }

    #[test]
    fn test_drop_names_leaving_the_directory() {
        let mut settings = MisercSettings {
            env: Some(
                ["development", "linux-x64", "x/../../other/Cargo", r"x\..\y"]
                    .map(String::from)
                    .to_vec(),
            ),
            override_config_filenames: Some(
                [
                    "mise.ci.toml",
                    "../other/mise.toml",
                    ".config/mise.toml",
                    "/etc/mise.toml",
                ]
                .map(String::from)
                .to_vec(),
            ),
            override_tool_versions_filenames: Some(
                [".tool-versions", "x/../../.tool-versions"]
                    .map(String::from)
                    .to_vec(),
            ),
            ..Default::default()
        };

        drop_names_leaving_the_directory(&mut settings, Path::new("/work/.miserc.toml"));

        assert_eq!(
            settings.env,
            Some(["development", "linux-x64"].map(String::from).to_vec())
        );
        assert_eq!(
            settings.override_config_filenames,
            Some(
                ["mise.ci.toml", ".config/mise.toml"]
                    .map(String::from)
                    .to_vec()
            )
        );
        assert_eq!(
            settings.override_tool_versions_filenames,
            Some([".tool-versions"].map(String::from).to_vec())
        );
    }

    #[test]
    fn test_resolve_ignored_config_paths_from_declaring_file() {
        let mut settings = MisercSettings {
            ignored_config_paths: Some(BTreeSet::from([PathBuf::from("../vendor/./**/mise.toml")])),
            ..Default::default()
        };

        resolve_ignored_config_paths(&mut settings, Path::new("/workspaces/vcs/.config"));

        assert_eq!(
            settings.ignored_config_paths,
            Some(BTreeSet::from([PathBuf::from(
                "/workspaces/vcs/vendor/**/mise.toml"
            )]))
        );
    }

    #[test]
    fn test_render_miserc_template_no_op() {
        // Content without template syntax should pass through unchanged
        let mut tera = None;
        let content = r#"env = ["development"]"#;
        let result = render_miserc_template(&mut tera, content, Path::new("/home/user"));
        assert_eq!(result, content);
    }

    #[test]
    fn test_render_miserc_template_env_var() {
        // env.HOME should expand using PRISTINE_ENV — the same source the template uses
        let mut tera = None;
        let home = env::PRISTINE_ENV
            .get("HOME")
            .cloned()
            .unwrap_or_else(|| "/root".to_string());
        let content = r#"ceiling_paths = ["{{ env.HOME }}"]"#;
        let result = render_miserc_template(&mut tera, content, &dirs::CONFIG);
        assert!(
            result.contains(&home),
            "Expected HOME ({home}) in rendered output, got: {result}"
        );
    }

    #[test]
    fn test_render_project_miserc_template_has_no_env() {
        let mut tera = None;
        let content = r#"env = ["{{ env.HOME | default(value='none') }}"]"#;
        let result = render_miserc_template(&mut tera, content, Path::new("/some/dir"));
        assert_eq!(result, r#"env = ["none"]"#);
    }

    #[test]
    fn test_check_project_template() {
        for ok in [
            r#"env = ["{{ os() }}-{{ arch() }}"]"#,
            r#"env = ["{{ config_root | basename | lower }}"]"#,
            r#"{% if os() == "linux" %}env = ["a"]{% else %}env = ["b"]{% endif %}"#,
            r#"{%- if xdg_config_home is defined and cwd is not starting_with("/tmp") -%}x{%- endif %}"#,
            r#"env = ["{{ 'a(b' ~ "{{ range(end=3) }}" }}"] # range( in a string"#,
            "{# {% for x in y %} #}env = []",
            "env = [] # { not a tag",
        ] {
            assert_eq!(check_project_template(ok), Ok(()), "{ok}");
        }
        for (bad, why) in [
            (
                r#"{% for a in range(end=100000) %}{% endfor %}"#,
                "`for` statement",
            ),
            (
                r#"{% set s = "aa" %}{% set_global s = s ~ s %}"#,
                "`set` statement",
            ),
            (r#"{% raw %}{% endraw %}"#, "`raw` statement"),
            ("{{ range(end=9) | join }}", "`range()` function"),
            ("{{ get_env(name='HOME') }}", "`get_env()` function"),
            (
                "{{ choice(n=9999999, alphabet='a') }}",
                "`choice()` function",
            ),
            ("{{ '/dev/zero' | hash_file }}", "`hash_file` filter"),
            ("{{ 'a' | replace(from='a', to='bb') }}", "`replace` filter"),
            ("{{ ['a'] | join(sep='b') }}", "`join` filter"),
            ("{% if '/etc' is exists %}{% endif %}", "`exists` test"),
            ("{% if x is not dir %}{% endif %}", "`dir` test"),
            // a string that hides the tag's end, or escapes its own quote
            (r#"{{ "}}" ~ range(end=9) }}"#, "`range()` function"),
            (r#"{{ "\" }}" ~ range(end=9) }}"#, "`range()` function"),
            ("{{ os() ", "not closed"),
            ("{{ 'a }}", "not closed"),
            ("{# a", "not closed"),
        ] {
            let err = check_project_template(bad).unwrap_err();
            assert!(err.contains(why), "{bad}: {err}");
        }
        let long = format!("# {}\n{{{{ os() }}}}", "a".repeat(PROJECT_TEMPLATE_MAX_LEN));
        assert!(check_project_template(&long).is_err());
    }

    #[test]
    fn test_check_project_template_refuses_list_comprehensions() {
        // Tera 2 renders nested list comprehensions, so each level multiplies
        // the work: a few levels over a literal list would never finish.
        let nested = r#"{{ [[x ~ y for x in ["a", "b"]] for y in ["c", "d", "e"]] | length }}"#;
        let mut tera = crate::tera::get_miserc_tera();
        assert_eq!(
            crate::tera::render_str_with_env(&mut tera, nested, &Context::new()).unwrap(),
            "3"
        );
        for template in [
            nested,
            r#"{% if [x for x in ["a"]] %}x{% endif %}"#,
            r#"{{ ["a", "b"] | length }}{{ [y for y in "ab"] }}"#,
        ] {
            let err = check_project_template(template).unwrap_err();
            assert!(err.contains("`for` loop"), "{template}: {err}");
        }
    }

    #[test]
    fn test_render_project_miserc_template_refuses_unbounded_templates() {
        // nested loops that would never finish are not rendered
        let mut tera = None;
        let content = r#"env = ["{% for a in range(end=100000) %}{% for b in range(end=100000) %}{% endfor %}{% endfor %}x"]"#;
        let result = render_miserc_template(&mut tera, content, Path::new("/some/dir"));
        assert_eq!(result, content);
    }

    #[test]
    fn test_render_miserc_template_config_root() {
        let mut tera = None;
        let config_root = Path::new("/my/project");
        let content = r#"ceiling_paths = ["{{ config_root }}"]"#;
        let result = render_miserc_template(&mut tera, content, config_root);
        assert!(
            result.contains("/my/project"),
            "Expected config_root in rendered output, got: {result}"
        );
    }

    #[test]
    fn test_render_miserc_template_os_function() {
        let mut tera = None;
        let content = r#"env = ["{{ os() }}"]"#;
        let result = render_miserc_template(&mut tera, content, Path::new("/some/dir"));
        // os() should return a non-empty string (linux, macos, windows, etc.)
        assert!(
            !result.contains("{{ os() }}"),
            "Template was not rendered: {result}"
        );
    }

    #[test]
    fn test_render_miserc_template_invalid_falls_back() {
        // An invalid template should fall back to the original content (with a warning)
        let mut tera = None;
        let content = r#"ceiling_paths = ["{{ undefined_function_xyz() }}"]"#;
        let result = render_miserc_template(&mut tera, content, Path::new("/some/dir"));
        // Should return original content unchanged on error
        assert_eq!(result, content);
    }
}
