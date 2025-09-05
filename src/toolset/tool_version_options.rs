use indexmap::IndexMap;

#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct ToolVersionOptions {
    pub os: Option<Vec<String>>,
    pub install_env: IndexMap<String, String>,
    #[serde(flatten)]
    pub opts: IndexMap<String, String>,
}

// Implement Hash manually to ensure deterministic hashing across IndexMap
impl std::hash::Hash for ToolVersionOptions {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.os.hash(state);

        // Hash install_env in sorted order for deterministic hashing
        let mut install_env_sorted: Vec<_> = self.install_env.iter().collect();
        install_env_sorted.sort_by_key(|(k, _)| *k);
        install_env_sorted.hash(state);

        // Hash opts in sorted order for deterministic hashing
        let mut opts_sorted: Vec<_> = self.opts.iter().collect();
        opts_sorted.sort_by_key(|(k, _)| *k);
        opts_sorted.hash(state);
    }
}

impl ToolVersionOptions {
    pub fn is_empty(&self) -> bool {
        self.install_env.is_empty() && self.opts.is_empty()
    }

    pub fn get(&self, key: &str) -> Option<&String> {
        // First try direct lookup
        if let Some(value) = self.opts.get(key) {
            return Some(value);
        }

        // We can't return references to temporarily parsed TOML values,
        // so nested lookup is not possible with this API.
        // For nested values, users should access the raw opts and parse themselves.
        None
    }

    pub fn merge(&mut self, other: &IndexMap<String, String>) {
        for (key, value) in other {
            self.opts
                .entry(key.to_string())
                .or_insert(value.to_string());
        }
    }

    pub fn contains_key(&self, key: &str) -> bool {
        if self.opts.contains_key(key) {
            return true;
        }

        // Check if it's a nested key that exists
        self.get_nested_value_exists(key)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &String)> {
        self.opts.iter()
    }

    // Check if a nested value exists without returning a reference
    fn get_nested_value_exists(&self, key: &str) -> bool {
        // Split the key by dots to navigate nested structure
        let parts: Vec<&str> = key.split('.').collect();
        if parts.len() < 2 {
            return false;
        }

        let root_key = parts[0];
        let nested_path = &parts[1..];

        // Get the root value and try to parse it as TOML
        if let Some(value) = self.opts.get(root_key) {
            if let Ok(toml_value) = value.parse::<toml::Value>() {
                return Self::value_exists_at_path(&toml_value, nested_path);
            } else if value.trim().starts_with('{') && value.trim().ends_with('}') {
                // Try to parse as inline TOML table
                if let Ok(toml_value) = format!("value = {value}").parse::<toml::Value>() {
                    if let Some(table_value) = toml_value.get("value") {
                        return Self::value_exists_at_path(table_value, nested_path);
                    }
                }
            }
        }

        false
    }

    fn value_exists_at_path(value: &toml::Value, path: &[&str]) -> bool {
        if path.is_empty() {
            return matches!(value, toml::Value::String(_));
        }

        match value {
            toml::Value::Table(table) => {
                if let Some(next_value) = table.get(path[0]) {
                    Self::value_exists_at_path(next_value, &path[1..])
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    // New method to get nested values as owned Strings
    pub fn get_nested_string(&self, key: &str) -> Option<String> {
        // Split the key by dots to navigate nested structure
        let parts: Vec<&str> = key.split('.').collect();
        if parts.len() < 2 {
            return None;
        }

        let root_key = parts[0];
        let nested_path = &parts[1..];

        // Get the root value and try to parse it as TOML
        if let Some(value) = self.opts.get(root_key) {
            if let Ok(toml_value) = value.parse::<toml::Value>() {
                return Self::get_string_at_path(&toml_value, nested_path);
            } else if value.trim().starts_with('{') && value.trim().ends_with('}') {
                // Try to parse as inline TOML table
                if let Ok(toml_value) = format!("value = {value}").parse::<toml::Value>() {
                    if let Some(table_value) = toml_value.get("value") {
                        return Self::get_string_at_path(table_value, nested_path);
                    }
                }
            }
        }

        None
    }

    fn get_string_at_path(value: &toml::Value, path: &[&str]) -> Option<String> {
        if path.is_empty() {
            return match value {
                toml::Value::String(s) => Some(s.clone()),
                toml::Value::Integer(i) => Some(i.to_string()),
                toml::Value::Boolean(b) => Some(b.to_string()),
                toml::Value::Float(f) => Some(f.to_string()),
                _ => None,
            };
        }

        match value {
            toml::Value::Table(table) => {
                if let Some(next_value) = table.get(path[0]) {
                    Self::get_string_at_path(next_value, &path[1..])
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

pub fn parse_tool_options(s: &str) -> ToolVersionOptions {
    let mut tvo = ToolVersionOptions::default();
    for opt in s.split(',') {
        let (k, v) = opt.split_once('=').unwrap_or((opt, ""));
        if k.is_empty() {
            continue;
        }
        tvo.opts.insert(k.to_string(), v.to_string());
    }
    tvo
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use test_log::test;

    #[test]
    fn test_parse_tool_options() {
        let t = |input, expected| {
            let opts = parse_tool_options(input);
            assert_eq!(opts, expected);
        };

        t("", ToolVersionOptions::default());
        t(
            "exe=rg",
            ToolVersionOptions {
                opts: [("exe".to_string(), "rg".to_string())]
                    .iter()
                    .cloned()
                    .collect(),
                ..Default::default()
            },
        );
        t(
            "exe=rg,match=musl",
            ToolVersionOptions {
                opts: [
                    ("exe".to_string(), "rg".to_string()),
                    ("match".to_string(), "musl".to_string()),
                ]
                .iter()
                .cloned()
                .collect(),
                ..Default::default()
            },
        );
    }

    #[test]
    fn test_nested_option_with_os_arch_dash() {
        let mut opts = IndexMap::new();
        opts.insert(
            "platforms".to_string(),
            r#"
[macos-x64]
url = "https://example.com/macos-x64.tar.gz"
checksum = "sha256:abc123"

[linux-x64]
url = "https://example.com/linux-x64.tar.gz"
checksum = "sha256:def456"
"#
            .to_string(),
        );

        let tool_opts = ToolVersionOptions {
            opts,
            ..Default::default()
        };

        assert_eq!(
            tool_opts.get_nested_string("platforms.macos-x64.url"),
            Some("https://example.com/macos-x64.tar.gz".to_string())
        );
        assert_eq!(
            tool_opts.get_nested_string("platforms.macos-x64.checksum"),
            Some("sha256:abc123".to_string())
        );
        assert_eq!(
            tool_opts.get_nested_string("platforms.linux-x64.url"),
            Some("https://example.com/linux-x64.tar.gz".to_string())
        );
        assert_eq!(
            tool_opts.get_nested_string("platforms.linux-x64.checksum"),
            Some("sha256:def456".to_string())
        );
    }

    #[test]
    fn test_generic_nested_options() {
        let mut opts = IndexMap::new();
        opts.insert(
            "config".to_string(),
            r#"
[database]
host = "localhost"
port = 5432

[cache.redis]
host = "redis.example.com"
port = 6379
"#
            .to_string(),
        );

        let tool_opts = ToolVersionOptions {
            opts,
            ..Default::default()
        };

        assert_eq!(
            tool_opts.get_nested_string("config.database.host"),
            Some("localhost".to_string())
        );
        assert_eq!(
            tool_opts.get_nested_string("config.database.port"),
            Some("5432".to_string())
        );
        assert_eq!(
            tool_opts.get_nested_string("config.cache.redis.host"),
            Some("redis.example.com".to_string())
        );
        assert_eq!(
            tool_opts.get_nested_string("config.cache.redis.port"),
            Some("6379".to_string())
        );
    }

    #[test]
    fn test_direct_and_nested_options() {
        let mut opts = IndexMap::new();
        opts.insert(
            "platforms".to_string(),
            r#"
[macos-x64]
url = "https://example.com/macos-x64.tar.gz"
"#
            .to_string(),
        );
        opts.insert("simple_option".to_string(), "value".to_string());

        let tool_opts = ToolVersionOptions {
            opts,
            ..Default::default()
        };

        // Test nested option
        assert_eq!(
            tool_opts.get_nested_string("platforms.macos-x64.url"),
            Some("https://example.com/macos-x64.tar.gz".to_string())
        );
        // Test direct option
        assert_eq!(tool_opts.get("simple_option"), Some(&"value".to_string()));
    }

    #[test]
    fn test_contains_key_with_nested_options() {
        let mut opts = IndexMap::new();
        opts.insert(
            "platforms".to_string(),
            r#"
[macos-x64]
url = "https://example.com/macos-x64.tar.gz"
"#
            .to_string(),
        );

        let tool_opts = ToolVersionOptions {
            opts,
            ..Default::default()
        };

        assert!(tool_opts.contains_key("platforms.macos-x64.url"));
        assert!(!tool_opts.contains_key("platforms.linux-x64.url"));
        assert!(!tool_opts.contains_key("nonexistent"));
    }

    #[test]
    fn test_merge_functionality() {
        let mut opts = IndexMap::new();
        opts.insert(
            "platforms".to_string(),
            r#"
[macos-x64]
url = "https://example.com/macos-x64.tar.gz"
"#
            .to_string(),
        );

        let mut tool_opts = ToolVersionOptions {
            opts,
            ..Default::default()
        };

        // Verify nested option access
        assert!(tool_opts.contains_key("platforms.macos-x64.url"));

        // Merge new options
        let mut new_opts = IndexMap::new();
        new_opts.insert("simple_option".to_string(), "value".to_string());
        tool_opts.merge(&new_opts);

        // Should be able to access both old and new options
        assert!(tool_opts.contains_key("platforms.macos-x64.url"));
        assert!(tool_opts.contains_key("simple_option"));
    }

    #[test]
    fn test_non_existent_nested_paths() {
        let mut opts = IndexMap::new();
        opts.insert(
            "platforms".to_string(),
            r#"
[macos-x64]
url = "https://example.com/macos-x64.tar.gz"
"#
            .to_string(),
        );

        let tool_opts = ToolVersionOptions {
            opts,
            ..Default::default()
        };

        // Test non-existent nested paths
        assert_eq!(
            tool_opts.get_nested_string("platforms.windows-x64.url"),
            None
        );
        assert_eq!(
            tool_opts.get_nested_string("platforms.macos-x64.checksum"),
            None
        );
        assert_eq!(tool_opts.get_nested_string("config.database.host"), None);
    }

    #[test]
    fn test_indexmap_preserves_order() {
        let mut tvo = ToolVersionOptions::default();

        // Insert options in a specific order
        tvo.opts.insert("zebra".to_string(), "last".to_string());
        tvo.opts.insert("alpha".to_string(), "first".to_string());
        tvo.opts.insert("beta".to_string(), "second".to_string());

        // Collect keys to verify order is preserved
        let keys: Vec<_> = tvo.opts.keys().collect();
        assert_eq!(keys, vec!["zebra", "alpha", "beta"]);
    }
}
