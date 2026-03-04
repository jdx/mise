use indexmap::IndexMap;

/// Option keys that are only relevant during initial installation and should not
/// be persisted in the manifest or included in `full_with_opts()`.
pub const EPHEMERAL_OPT_KEYS: &[&str] = &["postinstall", "install_env"];

#[derive(Debug, Default, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct ToolVersionOptions {
    pub os: Option<Vec<String>>,
    pub install_env: IndexMap<String, String>,
    #[serde(flatten)]
    pub opts: IndexMap<String, toml::Value>,
}

// toml::Value doesn't implement Eq (due to floats), but we control the values
// and won't have NaN, so this is safe in practice.
impl Eq for ToolVersionOptions {}

// Implement Hash manually to ensure deterministic hashing across IndexMap
impl std::hash::Hash for ToolVersionOptions {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.os.hash(state);

        // Hash install_env in sorted order for deterministic hashing
        let mut install_env_sorted: Vec<_> = self.install_env.iter().collect();
        install_env_sorted.sort_by_key(|(k, _)| *k);
        install_env_sorted.hash(state);

        // Hash opts in sorted order for deterministic hashing
        // toml::Value doesn't implement Hash, so hash its string representation
        let mut opts_sorted: Vec<_> = self
            .opts
            .iter()
            .map(|(k, v)| (k.clone(), v.to_string()))
            .collect();
        opts_sorted.sort_by_key(|(k, _)| k.clone());
        opts_sorted.hash(state);
    }
}

impl ToolVersionOptions {
    pub fn is_empty(&self) -> bool {
        self.install_env.is_empty() && self.opts.is_empty()
    }

    /// Get a string value for a key. Returns the str for String values,
    /// or None for non-string values.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.opts.get(key).and_then(|v| v.as_str())
    }

    /// Get the raw toml::Value for a key.
    pub fn get_value(&self, key: &str) -> Option<&toml::Value> {
        self.opts.get(key)
    }

    pub fn merge(&mut self, other: &IndexMap<String, toml::Value>) {
        for (key, value) in other {
            self.opts.entry(key.to_string()).or_insert(value.clone());
        }
    }

    pub fn contains_key(&self, key: &str) -> bool {
        if self.opts.contains_key(key) {
            return true;
        }

        // Check if it's a nested key that exists
        self.get_nested_value_exists(key)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &toml::Value)> {
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

        if let Some(value) = self.opts.get(root_key) {
            return Self::value_exists_at_path(value, nested_path);
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

    /// Get nested values as owned Strings by navigating the toml::Value tree.
    pub fn get_nested_string(&self, key: &str) -> Option<String> {
        let parts: Vec<&str> = key.split('.').collect();
        if parts.len() < 2 {
            return None;
        }

        let root_key = parts[0];
        let nested_path = &parts[1..];

        if let Some(value) = self.opts.get(root_key) {
            return Self::get_string_at_path(value, nested_path);
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
    // Try TOML parsing first (handles nested structures like platforms={...} correctly)
    if let Some(tvo) = try_parse_as_toml(s) {
        return tvo;
    }
    // Fall back to manual parsing for legacy formats with unquoted values
    parse_tool_options_manual(s)
}

/// Try parsing an options string as a TOML inline table.
/// Returns `Some(opts)` if the string is valid TOML, `None` otherwise.
fn try_parse_as_toml(s: &str) -> Option<ToolVersionOptions> {
    let toml_str = format!("_x_ = {{ {s} }}");
    let value: toml::Value = toml::from_str(&toml_str).ok()?;
    let table = value.get("_x_")?.as_table()?;
    let mut tvo = ToolVersionOptions::default();
    for (k, v) in table {
        tvo.opts.insert(k.clone(), v.clone());
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
                tvo.opts
                    .insert(k.trim().to_string(), toml::Value::String(v.to_string()));
                current_key = Some(k.trim().to_string());
            }
        } else if !opt.is_empty() {
            // No '=' found, append to the previous value or create a new key
            if let Some(key) = &current_key
                && let Some(existing_value) = tvo.opts.get_mut(key)
                && let toml::Value::String(s) = existing_value
            {
                s.push(',');
                s.push_str(opt);
            }
        }
    }
    tvo
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use test_log::test;

    fn s(v: &str) -> toml::Value {
        toml::Value::String(v.to_string())
    }

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
                opts: [("exe".to_string(), s("rg"))].iter().cloned().collect(),
                ..Default::default()
            },
        );
        t(
            "exe=rg,match=musl",
            ToolVersionOptions {
                opts: [
                    ("exe".to_string(), s("rg")),
                    ("match".to_string(), s("musl")),
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
                    ("profile".to_string(), s("minimal")),
                    ("components".to_string(), s("rust-src,llvm-tools")),
                    (
                        "targets".to_string(),
                        s("wasm32-unknown-unknown,thumbv2-none-eabi"),
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
                    ("exe".to_string(), s("  rg  ")),
                    ("match".to_string(), s(" musl  ")),
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
                    ("foo".to_string(), s("")),
                    ("bar".to_string(), s("baz")),
                    ("baz".to_string(), s("")),
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
        let input = r#"platforms={ linux-x64 = { url = "https://example.com/linux.tar.gz" }, macos-arm64 = { url = "https://example.com/macos.tar.gz" } }"#;
        let opts = parse_tool_options(input);
        assert_eq!(opts.opts.len(), 1, "should have exactly one key");
        assert!(opts.opts.get("platforms").unwrap().is_table());

        assert_eq!(
            opts.get_nested_string("platforms.linux-x64.url"),
            Some("https://example.com/linux.tar.gz".to_string())
        );
        assert_eq!(
            opts.get_nested_string("platforms.macos-arm64.url"),
            Some("https://example.com/macos.tar.gz".to_string())
        );
    }

    #[test]
    fn test_parse_tool_options_mixed_braces_and_simple() {
        let input = r#"bin_path="bin",platforms={ linux-x64 = { url = "https://example.com/linux.tar.gz" } },strip_components="1""#;
        let opts = parse_tool_options(input);
        assert_eq!(opts.get("bin_path"), Some("bin"));
        assert_eq!(opts.get("strip_components"), Some("1"));
        assert!(opts.opts.get("platforms").is_some());
    }

    #[test]
    fn test_nested_option_with_os_arch_dash() {
        let mut opts = IndexMap::new();
        let mut platforms = toml::map::Map::new();
        let mut macos = toml::map::Map::new();
        macos.insert(
            "url".to_string(),
            toml::Value::String("https://example.com/macos-x64.tar.gz".to_string()),
        );
        macos.insert(
            "checksum".to_string(),
            toml::Value::String("sha256:abc123".to_string()),
        );
        platforms.insert("macos-x64".to_string(), toml::Value::Table(macos));

        let mut linux = toml::map::Map::new();
        linux.insert(
            "url".to_string(),
            toml::Value::String("https://example.com/linux-x64.tar.gz".to_string()),
        );
        linux.insert(
            "checksum".to_string(),
            toml::Value::String("sha256:def456".to_string()),
        );
        platforms.insert("linux-x64".to_string(), toml::Value::Table(linux));
        opts.insert("platforms".to_string(), toml::Value::Table(platforms));

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
        let mut config = toml::map::Map::new();
        let mut database = toml::map::Map::new();
        database.insert(
            "host".to_string(),
            toml::Value::String("localhost".to_string()),
        );
        database.insert("port".to_string(), toml::Value::Integer(5432));
        config.insert("database".to_string(), toml::Value::Table(database));

        let mut cache = toml::map::Map::new();
        let mut redis = toml::map::Map::new();
        redis.insert(
            "host".to_string(),
            toml::Value::String("redis.example.com".to_string()),
        );
        redis.insert("port".to_string(), toml::Value::Integer(6379));
        cache.insert("redis".to_string(), toml::Value::Table(redis));
        config.insert("cache".to_string(), toml::Value::Table(cache));

        opts.insert("config".to_string(), toml::Value::Table(config));

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
        let mut platforms = toml::map::Map::new();
        let mut macos = toml::map::Map::new();
        macos.insert(
            "url".to_string(),
            toml::Value::String("https://example.com/macos-x64.tar.gz".to_string()),
        );
        platforms.insert("macos-x64".to_string(), toml::Value::Table(macos));
        opts.insert("platforms".to_string(), toml::Value::Table(platforms));
        opts.insert(
            "simple_option".to_string(),
            toml::Value::String("value".to_string()),
        );

        let tool_opts = ToolVersionOptions {
            opts,
            ..Default::default()
        };

        assert_eq!(
            tool_opts.get_nested_string("platforms.macos-x64.url"),
            Some("https://example.com/macos-x64.tar.gz".to_string())
        );
        assert_eq!(tool_opts.get("simple_option"), Some("value"));
    }

    #[test]
    fn test_contains_key_with_nested_options() {
        let mut opts = IndexMap::new();
        let mut platforms = toml::map::Map::new();
        let mut macos = toml::map::Map::new();
        macos.insert(
            "url".to_string(),
            toml::Value::String("https://example.com/macos-x64.tar.gz".to_string()),
        );
        platforms.insert("macos-x64".to_string(), toml::Value::Table(macos));
        opts.insert("platforms".to_string(), toml::Value::Table(platforms));

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
        let mut platforms = toml::map::Map::new();
        let mut macos = toml::map::Map::new();
        macos.insert(
            "url".to_string(),
            toml::Value::String("https://example.com/macos-x64.tar.gz".to_string()),
        );
        platforms.insert("macos-x64".to_string(), toml::Value::Table(macos));
        opts.insert("platforms".to_string(), toml::Value::Table(platforms));

        let mut tool_opts = ToolVersionOptions {
            opts,
            ..Default::default()
        };

        assert!(tool_opts.contains_key("platforms.macos-x64.url"));

        let mut new_opts = IndexMap::new();
        new_opts.insert(
            "simple_option".to_string(),
            toml::Value::String("value".to_string()),
        );
        tool_opts.merge(&new_opts);

        assert!(tool_opts.contains_key("platforms.macos-x64.url"));
        assert!(tool_opts.contains_key("simple_option"));
    }

    #[test]
    fn test_non_existent_nested_paths() {
        let mut opts = IndexMap::new();
        let mut platforms = toml::map::Map::new();
        let mut macos = toml::map::Map::new();
        macos.insert(
            "url".to_string(),
            toml::Value::String("https://example.com/macos-x64.tar.gz".to_string()),
        );
        platforms.insert("macos-x64".to_string(), toml::Value::Table(macos));
        opts.insert("platforms".to_string(), toml::Value::Table(platforms));

        let tool_opts = ToolVersionOptions {
            opts,
            ..Default::default()
        };

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

        tvo.opts.insert("zebra".to_string(), s("last"));
        tvo.opts.insert("alpha".to_string(), s("first"));
        tvo.opts.insert("beta".to_string(), s("second"));

        let keys: Vec<_> = tvo.opts.keys().collect();
        assert_eq!(keys, vec!["zebra", "alpha", "beta"]);
    }
}
