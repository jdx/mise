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
            if let Ok(toml_value) = toml::de::from_str::<toml::Value>(value) {
                return Self::value_exists_at_path(&toml_value, nested_path);
            } else if value.trim().starts_with('{') && value.trim().ends_with('}') {
                // Try to parse as inline TOML table
                if let Ok(toml_value) =
                    toml::de::from_str::<toml::Value>(&format!("value = {value}"))
                    && let Some(table_value) = toml_value.get("value")
                {
                    return Self::value_exists_at_path(table_value, nested_path);
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
            if let Ok(toml_value) = toml::de::from_str::<toml::Value>(value) {
                return Self::get_string_at_path(&toml_value, nested_path);
            } else if value.trim().starts_with('{') && value.trim().ends_with('}') {
                // Try to parse as inline TOML table
                if let Ok(toml_value) =
                    toml::de::from_str::<toml::Value>(&format!("value = {value}"))
                    && let Some(table_value) = toml_value.get("value")
                {
                    return Self::get_string_at_path(table_value, nested_path);
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

/// Try parsing an options string as a TOML inline table.
/// Returns `Some(opts)` if the string is valid TOML, `None` otherwise.
fn try_parse_as_toml(s: &str) -> Option<ToolVersionOptions> {
    let toml_str = format!("_x_ = {{ {s} }}");
    let value: toml::Value = toml::from_str(&toml_str).ok()?;
    let table = value.get("_x_")?.as_table()?;
    let mut tvo = ToolVersionOptions::default();
    for (k, v) in table {
        let s = match v {
            toml::Value::String(s) => s.clone(),
            // Nested tables (e.g. platforms) are stored as serialized TOML
            toml::Value::Table(_) => v.to_string(),
            _ => v.to_string(),
        };
        tvo.opts.insert(k.clone(), s);
    }
    Some(tvo)
}

/// Legacy manual parser for option strings with unquoted values (e.g. `exe=rg,match=musl`).
/// Splits by commas, but segments without `=` are appended to the previous key's value.
fn parse_tool_options_manual(s: &str) -> ToolVersionOptions {
    let mut tvo = ToolVersionOptions::default();
    let mut current_key: Option<String> = None;
    for opt in s.split(',') {
        if let Some((k, v)) = opt.split_once('=') {
            if !k.trim().is_empty() {
                tvo.opts.insert(k.trim().to_string(), v.to_string());
                current_key = Some(k.trim().to_string());
            }
        } else if !opt.is_empty() {
            // No '=' found, append to the previous value or create a new key
            if let Some(key) = &current_key
                && let Some(existing_value) = tvo.opts.get_mut(key)
            {
                existing_value.push(',');
                existing_value.push_str(opt);
            }
        }
    }
    tvo
}

pub fn parse_tool_options(s: &str) -> ToolVersionOptions {
    // Try TOML parsing first (handles nested structures like platforms={...} correctly)
    if let Some(tvo) = try_parse_as_toml(s) {
        return tvo;
    }
    // TODO(2027.1.0): remove manual fallback once all manifests use quoted TOML values
    debug_assert!(
        *crate::cli::version::V < versions::Versioning::new("2027.1.0").unwrap(),
        "parse_tool_options manual fallback should be removed"
    );
    // Fall back to manual parsing for legacy formats with unquoted values
    parse_tool_options_manual(s)
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
        t(
            "profile=minimal,components=rust-src,llvm-tools,targets=wasm32-unknown-unknown,thumbv2-none-eabi",
            ToolVersionOptions {
                opts: [
                    ("profile".to_string(), "minimal".to_string()),
                    ("components".to_string(), "rust-src,llvm-tools".to_string()),
                    (
                        "targets".to_string(),
                        "wasm32-unknown-unknown,thumbv2-none-eabi".to_string(),
                    ),
                ]
                .iter()
                .cloned()
                .collect(),
                ..Default::default()
            },
        );
        // test trimming of key whitespace
        t(
            "  exe =  rg  ,  match = musl  ",
            ToolVersionOptions {
                opts: [
                    ("exe".to_string(), "  rg  ".to_string()),
                    ("match".to_string(), " musl  ".to_string()),
                ]
                .iter()
                .cloned()
                .collect(),
                ..Default::default()
            },
        );
        // test value-less keys
        t(
            "foo=,bar=baz,baz=",
            ToolVersionOptions {
                opts: [
                    ("foo".to_string(), "".to_string()),
                    ("bar".to_string(), "baz".to_string()),
                    ("baz".to_string(), "".to_string()),
                ]
                .iter()
                .cloned()
                .collect(),
                ..Default::default()
            },
        );
    }

    #[test]
    fn test_parse_tool_options_with_nested_braces() {
        // Regression test for https://github.com/jdx/mise/discussions/7034
        // platforms={ linux-x64 = { url = "..." }, macos-arm64 = { url = "..." } }
        // should be parsed as a single "platforms" key, not split on the inner commas
        let input = r#"platforms={ linux-x64 = { url = "https://example.com/linux.tar.gz" }, macos-arm64 = { url = "https://example.com/macos.tar.gz" } }"#;
        let opts = parse_tool_options(input);
        assert_eq!(opts.opts.len(), 1, "should have exactly one key");
        let platforms_val = opts
            .opts
            .get("platforms")
            .expect("should have platforms key");
        assert!(
            platforms_val.contains("linux-x64"),
            "platforms value should contain linux-x64"
        );
        assert!(
            platforms_val.contains("macos-arm64"),
            "platforms value should contain macos-arm64"
        );

        // Also verify nested lookup works on the round-tripped value
        let tvo = ToolVersionOptions {
            opts: opts.opts,
            ..Default::default()
        };
        assert_eq!(
            tvo.get_nested_string("platforms.linux-x64.url"),
            Some("https://example.com/linux.tar.gz".to_string())
        );
        assert_eq!(
            tvo.get_nested_string("platforms.macos-arm64.url"),
            Some("https://example.com/macos.tar.gz".to_string())
        );
    }

    #[test]
    fn test_parse_tool_options_mixed_braces_and_simple() {
        // Mix of simple key=value and nested brace values
        let input = r#"bin_path=bin,platforms={ linux-x64 = { url = "https://example.com/linux.tar.gz" } },strip_components=1"#;
        let opts = parse_tool_options(input);
        assert_eq!(opts.opts.get("bin_path"), Some(&"bin".to_string()));
        assert_eq!(opts.opts.get("strip_components"), Some(&"1".to_string()));
        assert!(opts.opts.get("platforms").is_some());
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
