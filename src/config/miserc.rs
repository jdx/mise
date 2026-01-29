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

use crate::config::settings::MisercSettings;
use crate::dirs;
use crate::env;
use crate::file;

static MISERC: OnceLock<MisercSettings> = OnceLock::new();

/// Initialize miserc settings by loading .miserc.toml files.
/// This must be called early in the initialization process, before
/// MISE_ENV or other early settings are accessed.
pub fn init() -> Result<()> {
    let settings = load_miserc_settings()?;
    let _ = MISERC.set(settings);
    Ok(())
}

/// Get the loaded miserc settings, or default if not initialized.
pub fn get() -> &'static MisercSettings {
    MISERC.get_or_init(|| load_miserc_settings().unwrap_or_default())
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

/// Load and merge all miserc settings files.
/// Precedence (highest to lowest):
/// 1. Local .miserc.toml and .config/miserc.toml (closest to cwd wins)
/// 2. Global ~/.config/mise/miserc.toml
/// 3. System /etc/mise/miserc.toml
fn load_miserc_settings() -> Result<MisercSettings> {
    let mut merged = MisercSettings::default();

    // Load in reverse precedence order so later loads override earlier ones
    let files = find_miserc_files();

    for path in files.into_iter().rev() {
        if let Ok(content) = file::read_to_string(&path) {
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

    // System: /etc/mise/miserc.toml (or MISE_SYSTEM_DIR)
    let system_dir = env::var("MISE_SYSTEM_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/etc/mise"));
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
}
