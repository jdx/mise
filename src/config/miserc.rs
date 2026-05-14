//! Early initialization settings from .miserc.toml
//!
//! This module handles loading settings that need to be known before the main
//! config files are parsed. The primary use case is setting MISE_ENV, which
//! determines which environment-specific config files (e.g., mise.development.toml)
//! to load.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use eyre::Result;
use tera::Context;

use crate::config::settings::MisercSettings;
use crate::dirs;
use crate::env;
use crate::file;
use crate::tera::{
    contains_template_syntax, get_miserc_tera, render_str, take_tera_accessed_files,
};

static MISERC: OnceLock<MisercSettings> = OnceLock::new();

/// Initialize miserc settings by loading .miserc.toml files.
/// This must be called early in the initialization process, before
/// MISE_ENV or other early settings are accessed.
pub fn init() -> Result<()> {
    let settings = load_miserc_settings()?;
    let _ = MISERC.set(settings);
    // Discard any files tracked via hash_file/file_size/last_modified during miserc
    // template rendering. Those filters write to TERA_ACCESSED_FILES (used by hook-env
    // for file-watch detection), but miserc is loaded before config and should not
    // contribute to that list.
    let _ = take_tera_accessed_files();
    Ok(())
}

/// Get the loaded miserc settings, or default if not initialized.
pub fn get() -> &'static MisercSettings {
    MISERC.get_or_init(|| {
        let settings = load_miserc_settings().unwrap_or_default();
        let _ = take_tera_accessed_files();
        settings
    })
}

/// Get the MISE_ENV value from miserc, if set.
pub fn get_env() -> Option<&'static Vec<String>> {
    get().env.as_ref()
}

/// Get the ceiling_paths value from miserc, if set.
pub fn get_ceiling_paths() -> Option<&'static BTreeSet<PathBuf>> {
    get().ceiling_paths.as_ref()
}

/// Get the ignored_config_paths value from miserc, if set.
pub fn get_ignored_config_paths() -> Option<&'static BTreeSet<PathBuf>> {
    get().ignored_config_paths.as_ref()
}

/// Get the override_config_filenames value from miserc, if set.
pub fn get_override_config_filenames() -> Option<&'static Vec<String>> {
    get().override_config_filenames.as_ref()
}

/// Get the override_tool_versions_filenames value from miserc, if set.
pub fn get_override_tool_versions_filenames() -> Option<&'static Vec<String>> {
    get().override_tool_versions_filenames.as_ref()
}

/// Render any Tera template syntax in miserc content before TOML parsing.
/// Uses a minimal context that is safe to build before the main config is loaded:
/// - `env` – OS environment variables (from PRISTINE_ENV)
/// - `config_root` – directory containing the miserc file
/// - `cwd` – current working directory
/// - `xdg_*` – XDG base directory variables
///
/// Notably absent (would cause circular initialization):
/// - `mise_env` (depends on miserc itself)
/// - `exec()` (depends on Settings, which are not yet loaded)
/// - `read_file()` (not registered — needs per-file directory context not set up at this stage)
fn render_miserc_template(
    tera: &mut Option<tera::Tera>,
    content: &str,
    config_root: &Path,
) -> String {
    if !contains_template_syntax(content) {
        return content.to_string();
    }
    // Lazily initialize the Tera instance — only pay the clone cost if at least one file
    // contains template syntax.
    let tera = tera.get_or_insert_with(get_miserc_tera);
    let mut context = Context::new();
    context.insert("env", &*env::PRISTINE_ENV);
    context.insert("config_root", config_root);
    match std::env::current_dir() {
        Ok(dir) => context.insert("cwd", &dir),
        Err(e) => {
            debug!("miserc template: could not determine cwd, `cwd` will be unavailable: {e}")
        }
    };
    context.insert("xdg_cache_home", &*env::XDG_CACHE_HOME);
    context.insert("xdg_config_home", &*env::XDG_CONFIG_HOME);
    context.insert("xdg_data_home", &*env::XDG_DATA_HOME);
    context.insert("xdg_state_home", &*env::XDG_STATE_HOME);
    match render_str(tera, content, &context) {
        Ok(rendered) => rendered,
        Err(e) => {
            warn!("Failed to render template in miserc: {e}");
            content.to_string()
        }
    }
}

/// Load and merge all miserc settings files.
/// Precedence (highest to lowest):
/// 1. Local .miserc.toml and .config/miserc.toml (closest to cwd wins)
/// 2. Global ~/.config/mise/miserc.toml
/// 3. System /etc/mise/miserc.toml
fn load_miserc_settings() -> Result<MisercSettings> {
    let mut merged = MisercSettings::default();

    // Load in reverse precedence order so later loads override earlier ones
    let files = find_miserc_files();

    // Tera is initialized lazily inside render_miserc_template — only paid if a file
    // actually contains template syntax. Shared across all files to avoid redundant clones.
    let mut tera: Option<tera::Tera> = None;

    for path in files.into_iter().rev() {
        if let Ok(content) = file::read_to_string(&path) {
            let config_root = path.parent().unwrap_or(Path::new("."));
            let content = render_miserc_template(&mut tera, &content, config_root);
            match toml::from_str::<MisercSettings>(&content) {
                Ok(settings) => {
                    merge_settings(&mut merged, settings);
                }
                Err(e) => {
                    warn!("Failed to parse {}: {}", path.display(), e);
                }
            }
        }
    }

    Ok(merged)
}

/// Merge source settings into target, where source values override target.
fn merge_settings(target: &mut MisercSettings, source: MisercSettings) {
    if source.env.is_some() {
        target.env = source.env;
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

    // Local hierarchy: .miserc.toml and .config/miserc.toml in cwd and ancestors
    // Use raw std::env to avoid depending on our lazy statics
    if let Ok(cwd) = std::env::current_dir() {
        // Walk up the directory tree, but stop at home or root
        let home: &Path = &dirs::HOME;
        for dir in cwd.ancestors() {
            let path = dir.join(".miserc.toml");
            if path.is_file() {
                files.push(path);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_merge_settings() {
        let mut target = MisercSettings {
            env: Some(vec!["base".to_string()]),
            ..Default::default()
        };

        let source = MisercSettings {
            env: Some(vec!["override".to_string()]),
            ..Default::default()
        };

        merge_settings(&mut target, source);

        assert_eq!(target.env, Some(vec!["override".to_string()]));
    }

    #[test]
    fn test_parse_miserc() {
        let content = r#"
env = ["development", "local"]
ceiling_paths = ["/home/user"]
"#;
        let settings: MisercSettings = toml::from_str(content).unwrap();
        assert_eq!(
            settings.env,
            Some(vec!["development".to_string(), "local".to_string()])
        );
        assert!(settings.ceiling_paths.is_some());
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
        let result = render_miserc_template(&mut tera, content, Path::new("/some/dir"));
        assert!(
            result.contains(&home),
            "Expected HOME ({home}) in rendered output, got: {result}"
        );
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
