//! Schema information generated from mise.json at build time

// Include the generated schema sections, entries, settings, and hooks
include!(concat!(env!("OUT_DIR"), "/schema_sections.rs"));

/// Check if a section name is valid according to the schema
pub fn is_valid_section(name: &str) -> bool {
    SCHEMA_SECTIONS.iter().any(|(n, _)| *n == name)
}

/// Get the description for a section, if it exists
pub fn section_description(name: &str) -> Option<&'static str> {
    SCHEMA_SECTIONS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, desc)| *desc)
}

/// Check if a top-level entry name is valid according to the schema
pub fn is_valid_entry(name: &str) -> bool {
    SCHEMA_ENTRIES.iter().any(|(n, _, _)| *n == name)
}

/// Get the description for a top-level entry, if it exists
pub fn entry_description(name: &str) -> Option<&'static str> {
    SCHEMA_ENTRIES
        .iter()
        .find(|(n, _, _)| *n == name)
        .map(|(_, desc, _)| *desc)
}

/// Get the type for a top-level entry, if it exists
pub fn entry_type(name: &str) -> Option<SchemaType> {
    SCHEMA_ENTRIES
        .iter()
        .find(|(n, _, _)| *n == name)
        .map(|(_, _, t)| *t)
}

/// Check if a setting key is valid according to the schema
pub fn is_valid_setting(name: &str) -> bool {
    SCHEMA_SETTINGS.iter().any(|(n, _, _)| *n == name)
}

/// Get the description for a setting key, if it exists
pub fn setting_description(name: &str) -> Option<&'static str> {
    SCHEMA_SETTINGS
        .iter()
        .find(|(n, _, _)| *n == name)
        .map(|(_, desc, _)| *desc)
}

/// Get the type for a setting key, if it exists
pub fn setting_type(name: &str) -> Option<SchemaType> {
    SCHEMA_SETTINGS
        .iter()
        .find(|(n, _, _)| *n == name)
        .map(|(_, _, t)| *t)
}

/// Check if a hook name is a common/known hook
pub fn is_common_hook(name: &str) -> bool {
    SCHEMA_HOOKS.iter().any(|(n, _)| *n == name)
}

/// Get the description for a hook, if it exists
pub fn hook_description(name: &str) -> Option<&'static str> {
    SCHEMA_HOOKS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, desc)| *desc)
}

/// Check if a task_config key is valid according to the schema
pub fn is_valid_task_config(name: &str) -> bool {
    SCHEMA_TASK_CONFIG.iter().any(|(n, _, _)| *n == name)
}

/// Get the description for a task_config key, if it exists
pub fn task_config_description(name: &str) -> Option<&'static str> {
    SCHEMA_TASK_CONFIG
        .iter()
        .find(|(n, _, _)| *n == name)
        .map(|(_, desc, _)| *desc)
}

/// Get the type for a task_config key, if it exists
pub fn task_config_type(name: &str) -> Option<SchemaType> {
    SCHEMA_TASK_CONFIG
        .iter()
        .find(|(n, _, _)| *n == name)
        .map(|(_, _, t)| *t)
}

/// Check if a monorepo key is valid according to the schema
pub fn is_valid_monorepo(name: &str) -> bool {
    SCHEMA_MONOREPO.iter().any(|(n, _, _)| *n == name)
}

/// Get the description for a monorepo key, if it exists
pub fn monorepo_description(name: &str) -> Option<&'static str> {
    SCHEMA_MONOREPO
        .iter()
        .find(|(n, _, _)| *n == name)
        .map(|(_, desc, _)| *desc)
}

/// Get the type for a monorepo key, if it exists
pub fn monorepo_type(name: &str) -> Option<SchemaType> {
    SCHEMA_MONOREPO
        .iter()
        .find(|(n, _, _)| *n == name)
        .map(|(_, _, t)| *t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_sections() {
        assert!(is_valid_section("tools"));
        assert!(is_valid_section("env"));
        assert!(is_valid_section("tasks"));
        assert!(is_valid_section("settings"));
        assert!(!is_valid_section("invalid_section"));
        // min_version should NOT be a section
        assert!(!is_valid_section("min_version"));
    }

    #[test]
    fn test_section_descriptions() {
        assert!(section_description("tools").is_some());
        assert!(section_description("invalid").is_none());
    }

    #[test]
    fn test_valid_entries() {
        // min_version should be an entry, not a section
        assert!(is_valid_entry("min_version"));
        // tools should NOT be an entry
        assert!(!is_valid_entry("tools"));
    }

    #[test]
    fn test_entry_descriptions() {
        assert!(entry_description("min_version").is_some());
        assert!(entry_description("invalid").is_none());
    }

    #[test]
    fn test_entry_types() {
        // redactions is an array
        assert_eq!(entry_type("redactions"), Some(SchemaType::Array));
        // experimental_monorepo_root is a boolean
        assert_eq!(
            entry_type("experimental_monorepo_root"),
            Some(SchemaType::Boolean)
        );
    }

    #[test]
    fn test_valid_settings() {
        // Common settings should be valid
        assert!(is_valid_setting("experimental"));
        assert!(is_valid_setting("color"));
        // Nested settings should use dot notation
        assert!(is_valid_setting("aqua.baked_registry"));
        // Invalid settings
        assert!(!is_valid_setting("invalid_setting"));
    }

    #[test]
    fn test_setting_descriptions() {
        assert!(setting_description("experimental").is_some());
        assert!(setting_description("invalid").is_none());
    }

    #[test]
    fn test_setting_types() {
        // quiet is a boolean
        assert_eq!(setting_type("quiet"), Some(SchemaType::Boolean));
        // jobs is a number
        assert_eq!(setting_type("jobs"), Some(SchemaType::Number));
    }

    #[test]
    fn test_common_hooks() {
        assert!(is_common_hook("enter"));
        assert!(is_common_hook("leave"));
        assert!(is_common_hook("cd"));
        // Custom hooks are allowed but not "common"
        assert!(!is_common_hook("my_custom_hook"));
    }

    #[test]
    fn test_hook_descriptions() {
        assert!(hook_description("enter").is_some());
        assert!(hook_description("invalid").is_none());
    }

    #[test]
    fn test_valid_task_config() {
        assert!(is_valid_task_config("dir"));
        assert!(is_valid_task_config("includes"));
        assert!(!is_valid_task_config("invalid"));
    }

    #[test]
    fn test_task_config_descriptions() {
        assert!(task_config_description("dir").is_some());
        assert!(task_config_description("invalid").is_none());
    }

    #[test]
    fn test_valid_monorepo() {
        assert!(is_valid_monorepo("config_roots"));
        assert!(!is_valid_monorepo("invalid"));
    }

    #[test]
    fn test_monorepo_descriptions() {
        assert!(monorepo_description("config_roots").is_some());
        assert!(monorepo_description("invalid").is_none());
    }
}
