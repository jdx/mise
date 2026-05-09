use indexmap::IndexMap;

/// Option keys that are only relevant during initial installation and should not
/// be persisted in the manifest or included in `full_with_opts()`.
// install_env is a named field on ToolVersionOptions (serde puts it in self.install_env),
// but parse_tool_options() can still place it in opts, so we filter it here as well.
pub const EPHEMERAL_OPT_KEYS: &[&str] = &[
    "postinstall",
    "install_env",
    "depends",
    "install_before",
    "minimum_release_age",
];

#[derive(Debug, Default, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct ToolVersionOptions {
    pub os: Option<Vec<String>>,
    pub depends: Option<Vec<String>>,
    pub install_env: IndexMap<String, String>,
    #[serde(flatten)]
    pub opts: IndexMap<String, toml::Value>,
}

// toml::Value doesn't implement Eq (due to floats), but we control the values
// and won't have NaN, so this is safe in practice.
impl Eq for ToolVersionOptions {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolOptionSource {
    Registry,
    BackendAlias,
    Config,
    InlineBackendArg,
}

#[derive(Debug, Clone, Default)]
pub struct ResolvedToolOptions {
    options: ToolVersionOptions,
    sources: IndexMap<String, ToolOptionSource>,
}

impl ResolvedToolOptions {
    pub fn options(&self) -> &ToolVersionOptions {
        &self.options
    }

    pub fn into_options(self) -> ToolVersionOptions {
        self.options
    }

    pub fn source_for_key(&self, key: &str) -> Option<ToolOptionSource> {
        self.sources.get(key).copied().or_else(|| {
            key.split_once('.')
                .and_then(|(root, _)| self.sources.get(root).copied())
        })
    }

    pub fn has_key_from_sources(&self, key: &str, sources: &[ToolOptionSource]) -> bool {
        if key == "install_env" {
            return !self.options.install_env.is_empty()
                && self.sources.iter().any(|(source_key, source)| {
                    option_key_matches(source_key, key) && sources.contains(source)
                });
        }
        self.source_for_key(key)
            .is_some_and(|source| sources.contains(&source))
            && self.options.contains_key(key)
    }

    pub fn has_any_key_from_sources(&self, keys: &[&str], sources: &[ToolOptionSource]) -> bool {
        keys.iter()
            .any(|key| self.has_key_from_sources(key, sources))
    }

    pub fn has_any_key_except_from_sources(
        &self,
        except_keys: &[&str],
        sources: &[ToolOptionSource],
    ) -> bool {
        self.sources.iter().any(|(key, source)| {
            sources.contains(source)
                && !except_keys
                    .iter()
                    .any(|except_key| option_key_matches(key, except_key))
        })
    }

    pub fn apply_overrides(&mut self, options: &ToolVersionOptions, source: ToolOptionSource) {
        self.options.apply_overrides(options);
        for key in options.opts.keys() {
            self.sources.insert(key.clone(), source);
        }
        if options.os.is_some() {
            self.sources.insert("os".to_string(), source);
        }
        if options.depends.is_some() {
            self.sources.insert("depends".to_string(), source);
        }
        if !options.install_env.is_empty() {
            self.sources.insert("install_env".to_string(), source);
            for key in options.install_env.keys() {
                self.sources.insert(format!("install_env.{key}"), source);
            }
        }
    }
}

fn option_key_matches(key: &str, expected: &str) -> bool {
    key == expected
        || key
            .strip_prefix(expected)
            .is_some_and(|suffix| suffix.starts_with('.'))
}

// Implement Hash manually to ensure deterministic hashing across IndexMap
impl std::hash::Hash for ToolVersionOptions {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.os.hash(state);
        self.depends.hash(state);

        // Hash install_env in sorted order for deterministic hashing
        let mut install_env_sorted: Vec<_> = self.install_env.iter().collect();
        install_env_sorted.sort_by_key(|(k, _)| *k);
        install_env_sorted.hash(state);

        // Hash opts in sorted order for deterministic hashing
        let mut opts_sorted: Vec<_> = self.opts.iter().collect();
        opts_sorted.sort_by_key(|(k, _)| k.as_str());
        for (k, v) in opts_sorted {
            k.hash(state);
            hash_toml_value(v, state);
        }
    }
}

fn hash_toml_value<H: std::hash::Hasher>(v: &toml::Value, state: &mut H) {
    use std::hash::Hash;
    match v {
        toml::Value::Table(t) => {
            let mut sorted: Vec<_> = t.iter().collect();
            sorted.sort_by_key(|(k, _)| k.as_str());
            for (k, v) in sorted {
                k.hash(state);
                hash_toml_value(v, state);
            }
        }
        toml::Value::Array(arr) => {
            for v in arr {
                hash_toml_value(v, state);
            }
        }
        _ => v.to_string().hash(state),
    }
}

impl ToolVersionOptions {
    pub fn is_empty(&self) -> bool {
        self.os.as_ref().is_none_or(|os| os.is_empty())
            && self.depends.as_ref().is_none_or(|d| d.is_empty())
            && self.install_env.is_empty()
            && self.opts.is_empty()
    }

    /// Get a string value for a key. Returns the str for String values,
    /// or None for non-string values.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.opts.get(key).and_then(|v| v.as_str())
    }

    pub fn minimum_release_age(&self) -> Option<&str> {
        if let Some(value) = self.get("minimum_release_age") {
            return Some(value);
        }
        if let Some(value) = self.get("install_before") {
            deprecated_at!(
                "2026.10.0",
                "2027.10.0",
                "tool_option.install_before",
                "`install_before` tool option is deprecated. Use `minimum_release_age` instead."
            );
            return Some(value);
        }
        None
    }

    /// Get a scalar value for a key as an owned string.
    pub fn get_string(&self, key: &str) -> Option<String> {
        self.opts.get(key).and_then(Self::value_to_string)
    }

    /// Convert opts to string values, extracting inner strings from
    /// `toml::Value::String` and calling `to_string()` on other types.
    pub fn opts_as_strings(&self) -> IndexMap<String, String> {
        self.opts
            .iter()
            .map(|(k, v)| {
                (
                    k.clone(),
                    match v {
                        toml::Value::String(s) => s.clone(),
                        _ => v.to_string(),
                    },
                )
            })
            .collect()
    }

    pub fn merge(&mut self, other: &IndexMap<String, toml::Value>) {
        for (key, value) in other {
            self.opts.entry(key.to_string()).or_insert(value.clone());
        }
    }

    pub fn apply_overrides(&mut self, overrides: &ToolVersionOptions) {
        for (key, value) in &overrides.opts {
            self.opts.insert(key.clone(), value.clone());
        }
        for (key, value) in &overrides.install_env {
            self.install_env.insert(key.clone(), value.clone());
        }
        if overrides.os.is_some() {
            self.os = overrides.os.clone();
        }
        if overrides.depends.is_some() {
            self.depends = overrides.depends.clone();
        }
    }

    pub fn insert_option(&mut self, key: String, value: toml::Value) -> Result<(), String> {
        if self.insert_core_option(&key, &value)? {
            return Ok(());
        }
        self.opts.insert(key, normalize_backend_option_value(value));
        Ok(())
    }

    fn insert_core_option(&mut self, key: &str, value: &toml::Value) -> Result<bool, String> {
        match key {
            "os" => {
                self.os = Some(parse_string_or_array(value, "os")?);
                Ok(true)
            }
            "depends" => {
                self.depends = Some(parse_string_or_array(value, "depends")?);
                Ok(true)
            }
            "install_env" => {
                let env = value
                    .as_table()
                    .ok_or_else(|| "install_env must be a table".to_string())?;
                for (key, value) in env {
                    self.install_env
                        .insert(key.clone(), env_value_to_string(value)?);
                }
                Ok(true)
            }
            "postinstall" | "minimum_release_age" | "install_before" => {
                self.opts.insert(
                    key.to_string(),
                    toml::Value::String(scalar_value_to_string(value).ok_or_else(|| {
                        format!("{key} must be a string, integer, boolean, float, or datetime")
                    })?),
                );
                Ok(true)
            }
            _ => {
                if let Some(env_key) = key.strip_prefix("install_env.") {
                    self.install_env.insert(
                        env_key.to_string(),
                        env_value_to_string(value)
                            .map_err(|_| format!("{key} must be a string, integer, or boolean"))?,
                    );
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
        }
    }

    pub fn contains_key(&self, key: &str) -> bool {
        if self.opts.contains_key(key) {
            return true;
        }
        if key == "os" {
            return self.os.is_some();
        }
        if key == "depends" {
            return self.depends.is_some();
        }
        if key == "install_env" {
            return !self.install_env.is_empty();
        }
        if let Some(env_key) = key.strip_prefix("install_env.") {
            return self.install_env.contains_key(env_key);
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
            return Self::value_to_string(value);
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

    fn value_to_string(value: &toml::Value) -> Option<String> {
        match value {
            toml::Value::String(s) => Some(s.clone()),
            toml::Value::Integer(i) => Some(i.to_string()),
            toml::Value::Boolean(b) => Some(b.to_string()),
            toml::Value::Float(f) => Some(f.to_string()),
            toml::Value::Datetime(d) => Some(d.to_string()),
            _ => None,
        }
    }
}

fn parse_string_or_array(value: &toml::Value, key: &str) -> Result<Vec<String>, String> {
    match value {
        toml::Value::String(s) => Ok(vec![s.clone()]),
        toml::Value::Array(values) => values
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .map(str::to_string)
                    .ok_or_else(|| format!("{key} array must contain only strings"))
            })
            .collect(),
        _ => Err(format!("{key} must be a string or array")),
    }
}

fn env_value_to_string(value: &toml::Value) -> Result<String, String> {
    match value {
        toml::Value::String(s) => Ok(s.clone()),
        toml::Value::Integer(i) => Ok(i.to_string()),
        toml::Value::Boolean(b) => Ok(b.to_string()),
        _ => Err("install_env values must be strings, integers, or booleans".to_string()),
    }
}

fn normalize_backend_option_value(value: toml::Value) -> toml::Value {
    match value {
        toml::Value::Table(_) | toml::Value::Array(_) | toml::Value::String(_) => value,
        _ => toml::Value::String(value.to_string().trim_matches('"').to_string()),
    }
}

fn scalar_value_to_string(value: &toml::Value) -> Option<String> {
    match value {
        toml::Value::String(s) => Some(s.clone()),
        toml::Value::Integer(i) => Some(i.to_string()),
        toml::Value::Boolean(b) => Some(b.to_string()),
        toml::Value::Float(f) => Some(f.to_string()),
        toml::Value::Datetime(d) => Some(d.to_string()),
        _ => None,
    }
}

pub fn parse_tool_options(s: &str) -> ToolVersionOptions {
    // Keep this legacy entry point forgiving: callers use it for registry/cache
    // paths where dropping every backend option because one core key is malformed
    // is worse than skipping only the invalid key.
    if let Some(options) = parse_as_toml_lenient(s) {
        return options;
    }
    parse_tool_options_manual_lenient(s)
}

pub fn try_parse_tool_options(s: &str) -> Result<ToolVersionOptions, String> {
    // Try TOML parsing first (handles nested structures like platforms={...} correctly)
    if let Some(result) = try_parse_as_toml(s) {
        return result;
    }
    // Fall back to manual parsing for legacy formats with unquoted values
    parse_tool_options_manual(s)
}

/// Serialize tool options to the bracketed `key=value,key2=value2` form used by
/// task tool specs and backend args.
///
/// Complex values that cannot be round-tripped through that syntax (arrays and
/// tables) are omitted entirely, matching `BackendArg::full_with_opts()`.
pub fn serialize_tool_options<'a, I>(opts: I) -> Option<String>
where
    I: IntoIterator<Item = (&'a String, &'a toml::Value)>,
{
    let serialized = opts
        .into_iter()
        .filter_map(|(key, value)| serialize_tool_option(key, value))
        .collect::<Vec<_>>();

    (!serialized.is_empty()).then(|| serialized.join(","))
}

fn serialize_tool_option(key: &str, value: &toml::Value) -> Option<String> {
    match value {
        toml::Value::Table(_) | toml::Value::Array(_) => None,
        // Strings that contain delimiters or quotes must be TOML-quoted so they
        // round-trip through both the TOML parser and the legacy manual parser.
        // Brackets also need quoting because `split_bracketed_opts()` uses a
        // regex to peel off the outer `[...]` payload from backend args.
        toml::Value::String(s) if string_requires_tool_option_quotes(s) => {
            Some(format!("{key}={}", toml::Value::String(s.clone())))
        }
        toml::Value::String(s) => Some(format!("{key}={s}")),
        _ => Some(format!("{key}={value}")),
    }
}

fn string_requires_tool_option_quotes(s: &str) -> bool {
    s.contains(',') || s.contains('"') || s.contains('\'') || s.contains('[') || s.contains(']')
}

/// Try parsing an options string as a TOML inline table.
/// Returns `Some(opts)` if the string is valid TOML, `None` otherwise.
fn try_parse_as_toml(s: &str) -> Option<Result<ToolVersionOptions, String>> {
    let toml_str = format!("_x_ = {{ {s} }}");
    let value: toml::Value = toml::from_str(&toml_str).ok()?;
    let table = value.get("_x_")?.as_table()?;
    let mut tvo = ToolVersionOptions::default();
    for (k, v) in table {
        if let Err(err) = tvo.insert_option(k.clone(), v.clone()) {
            return Some(Err(err));
        }
    }
    Some(Ok(tvo))
}

fn parse_as_toml_lenient(s: &str) -> Option<ToolVersionOptions> {
    let toml_str = format!("_x_ = {{ {s} }}");
    let value: toml::Value = toml::from_str(&toml_str).ok()?;
    let table = value.get("_x_")?.as_table()?;
    let mut tvo = ToolVersionOptions::default();
    for (key, value) in table {
        insert_option_lenient(&mut tvo, key.clone(), value.clone());
    }
    Some(tvo)
}

/// Legacy manual parser for option strings with unquoted values (e.g. `exe=rg,match=musl`).
/// Splits by commas, but segments without `=` are appended to the previous key's value.
fn parse_tool_options_manual(s: &str) -> Result<ToolVersionOptions, String> {
    let raw = parse_manual_tool_options_raw(s);
    let mut tvo = ToolVersionOptions::default();
    for (key, value) in raw {
        tvo.insert_option(key, value)?;
    }
    Ok(tvo)
}

fn parse_tool_options_manual_lenient(s: &str) -> ToolVersionOptions {
    let raw = parse_manual_tool_options_raw(s);
    let mut tvo = ToolVersionOptions::default();
    for (key, value) in raw {
        insert_option_lenient(&mut tvo, key, value);
    }
    tvo
}

fn parse_manual_tool_options_raw(s: &str) -> IndexMap<String, toml::Value> {
    let mut raw = IndexMap::new();
    let mut current_key: Option<String> = None;
    for opt in split_tool_option_segments(s) {
        if let Some((k, v)) = opt.split_once('=') {
            if !k.trim().is_empty() {
                raw.insert(k.trim().to_string(), parse_tool_option_value(v));
                current_key = Some(k.trim().to_string());
            }
        } else if !opt.is_empty() {
            // No '=' found, append to the previous value or create a new key
            if let Some(key) = &current_key
                && let Some(existing_value) = raw.get_mut(key)
                && let toml::Value::String(s) = existing_value
            {
                s.push(',');
                s.push_str(&opt);
            }
        }
    }

    raw
}

fn insert_option_lenient(options: &mut ToolVersionOptions, key: String, value: toml::Value) {
    if let Err(err) = options.insert_option(key, value) {
        warn!("{err}");
    }
}

fn split_tool_option_segments(s: &str) -> Vec<String> {
    let mut segments = Vec::new();
    let mut current = String::new();
    let mut in_double_quotes = false;
    let mut in_single_quotes = false;
    let mut escaped = false;

    for ch in s.chars() {
        match ch {
            '"' if !escaped && !in_single_quotes => in_double_quotes = !in_double_quotes,
            '\'' if !in_double_quotes => in_single_quotes = !in_single_quotes,
            ',' if !in_double_quotes && !in_single_quotes => {
                segments.push(current);
                current = String::new();
                escaped = false;
                continue;
            }
            _ => {}
        }

        current.push(ch);
        escaped = in_double_quotes && ch == '\\' && !escaped;
    }

    segments.push(current);
    segments
}

fn parse_tool_option_value(raw: &str) -> toml::Value {
    let trimmed = raw.trim();

    if ((trimmed.starts_with('"') && trimmed.ends_with('"'))
        || (trimmed.starts_with('\'') && trimmed.ends_with('\'')))
        && trimmed.len() >= 2
    {
        let toml_str = format!("_x_ = {trimmed}");
        if let Ok(value) = toml::from_str::<toml::Value>(&toml_str)
            && let Some(parsed) = value.get("_x_")
        {
            return parsed.clone();
        }
    }

    toml::Value::String(raw.to_string())
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
        t(
            r#"query="first,second=value",bin_path=bin"#,
            ToolVersionOptions {
                opts: [
                    ("query".to_string(), s("first,second=value")),
                    ("bin_path".to_string(), s("bin")),
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
    fn test_parse_tool_options_integer_strip_components() {
        // strip_components=1 (integer, not string) should be converted to string
        let input = r#"bin_path="bin",strip_components=1"#;
        let opts = parse_tool_options(input);
        assert_eq!(opts.get("bin_path"), Some("bin"));
        assert_eq!(opts.get("strip_components"), Some("1"));
    }

    #[test]
    fn test_parse_tool_options_core_keys_from_toml() {
        let input = r#"depends=["python","node"],os="linux",install_env={ FOO = "bar", RETRIES = 2 },postinstall="echo hi",minimum_release_age="7d",install_before="2024-01-01""#;
        let opts = parse_tool_options(input);

        assert_eq!(
            opts.depends,
            Some(vec!["python".to_string(), "node".to_string()])
        );
        assert_eq!(opts.os, Some(vec!["linux".to_string()]));
        assert_eq!(opts.install_env.get("FOO").map(String::as_str), Some("bar"));
        assert_eq!(
            opts.install_env.get("RETRIES").map(String::as_str),
            Some("2")
        );
        assert_eq!(opts.get("postinstall"), Some("echo hi"));
        assert_eq!(opts.get("minimum_release_age"), Some("7d"));
        assert_eq!(opts.get("install_before"), Some("2024-01-01"));
        assert!(!opts.opts.contains_key("depends"));
        assert!(!opts.opts.contains_key("os"));
        assert!(!opts.opts.contains_key("install_env"));
    }

    #[test]
    fn test_parse_tool_options_skips_invalid_core_option_without_dropping_backend_opts() {
        let input = r#"exe="rg",depends={ name = "dummy" },match="musl""#;
        let opts = parse_tool_options(input);

        assert_eq!(opts.get("exe"), Some("rg"));
        assert_eq!(opts.get("match"), Some("musl"));
        assert_eq!(opts.depends, None);
        assert!(!opts.opts.contains_key("depends"));
        assert!(try_parse_tool_options(input).is_err());
    }

    #[test]
    fn test_parse_tool_options_install_before_unquoted_date() {
        let opts = parse_tool_options("install_before=2024-06-01");

        assert_eq!(opts.get("install_before"), Some("2024-06-01"));
    }

    #[test]
    fn test_parse_tool_options_core_keys_from_manual_syntax() {
        let opts = parse_tool_options(
            "depends=python,os=linux,install_env.FOO=bar,postinstall=echo hi,minimum_release_age=7d",
        );

        assert_eq!(opts.depends, Some(vec!["python".to_string()]));
        assert_eq!(opts.os, Some(vec!["linux".to_string()]));
        assert_eq!(opts.install_env.get("FOO").map(String::as_str), Some("bar"));
        assert_eq!(opts.get("postinstall"), Some("echo hi"));
        assert_eq!(opts.get("minimum_release_age"), Some("7d"));
        assert!(!opts.opts.contains_key("depends"));
        assert!(!opts.opts.contains_key("os"));
        assert!(!opts.opts.contains_key("install_env.FOO"));
    }

    #[test]
    fn test_get_string_handles_scalar_values() {
        let mut opts = ToolVersionOptions::default();
        opts.opts
            .insert("integer".to_string(), toml::Value::Integer(124));
        opts.opts
            .insert("boolean".to_string(), toml::Value::Boolean(true));

        assert_eq!(opts.get_string("integer"), Some("124".to_string()));
        assert_eq!(opts.get_string("boolean"), Some("true".to_string()));
    }

    #[test]
    fn test_serialize_tool_options_quotes_comma_strings() {
        let mut opts = IndexMap::new();
        opts.insert(
            "query".to_string(),
            toml::Value::String("first,second=value".to_string()),
        );
        opts.insert(
            "bin_path".to_string(),
            toml::Value::String("bin".to_string()),
        );

        assert_eq!(
            serialize_tool_options(opts.iter()),
            Some(r#"query="first,second=value",bin_path=bin"#.to_string())
        );
        assert_eq!(
            parse_tool_options(serialize_tool_options(opts.iter()).unwrap().as_str()).get("query"),
            Some("first,second=value")
        );
    }

    #[test]
    fn test_serialize_tool_options_quotes_strings_with_quotes_or_brackets() {
        let mut opts = IndexMap::new();
        opts.insert(
            "pattern".to_string(),
            toml::Value::String(r#"a"b"#.to_string()),
        );
        opts.insert(
            "bin_path".to_string(),
            toml::Value::String("bin[debug]".to_string()),
        );

        let serialized = serialize_tool_options(opts.iter()).unwrap();
        assert_eq!(
            serialized,
            r#"pattern='a"b',bin_path="bin[debug]""#.to_string()
        );

        let reparsed = parse_tool_options(&serialized);
        assert_eq!(reparsed.get("pattern"), Some(r#"a"b"#));
        assert_eq!(reparsed.get("bin_path"), Some("bin[debug]"));
    }

    #[test]
    fn test_serialize_tool_options_preserves_single_quote_wrapped_strings() {
        let mut opts = IndexMap::new();
        opts.insert(
            "pattern".to_string(),
            toml::Value::String("'hi'".to_string()),
        );
        opts.insert(
            "bin_path".to_string(),
            toml::Value::String("bin".to_string()),
        );

        let serialized = serialize_tool_options(opts.iter()).unwrap();
        let reparsed = parse_tool_options(&serialized);

        assert_eq!(reparsed.get("pattern"), Some("'hi'"));
        assert_eq!(reparsed.get("bin_path"), Some("bin"));
    }

    #[test]
    fn test_parse_tool_options_manual_supports_single_quoted_literals() {
        let reparsed = parse_tool_options(r#"pattern='a"b',bin_path=bin"#);

        assert_eq!(reparsed.get("pattern"), Some(r#"a"b"#));
        assert_eq!(reparsed.get("bin_path"), Some("bin"));
    }

    #[test]
    fn test_serialize_tool_options_skips_complex_values_and_empty_output() {
        let mut opts = IndexMap::new();
        opts.insert(
            "targets".to_string(),
            toml::Value::Array(vec![toml::Value::String("x86_64".to_string())]),
        );
        opts.insert(
            "platforms".to_string(),
            toml::Value::Table(toml::map::Map::new()),
        );

        assert_eq!(serialize_tool_options(opts.iter()), None);
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
    fn test_contains_key_with_named_fields() {
        let tool_opts = ToolVersionOptions {
            os: Some(vec!["linux".to_string()]),
            depends: Some(vec!["node".to_string()]),
            install_env: [(
                "NPM_CONFIG_REGISTRY".to_string(),
                "https://example.com".to_string(),
            )]
            .iter()
            .cloned()
            .collect(),
            ..Default::default()
        };

        assert!(tool_opts.contains_key("os"));
        assert!(tool_opts.contains_key("depends"));
        assert!(tool_opts.contains_key("install_env"));
        assert!(tool_opts.contains_key("install_env.NPM_CONFIG_REGISTRY"));
        assert!(!tool_opts.contains_key("install_env.MISSING"));
    }

    #[test]
    fn test_resolved_tool_options_tracks_named_field_sources() {
        let config_opts = ToolVersionOptions {
            os: Some(vec!["linux".to_string()]),
            install_env: [("CONFIG_ONLY".to_string(), "1".to_string())]
                .iter()
                .cloned()
                .collect(),
            ..Default::default()
        };
        let inline_opts = ToolVersionOptions {
            depends: Some(vec!["node".to_string()]),
            install_env: [("INLINE_ONLY".to_string(), "2".to_string())]
                .iter()
                .cloned()
                .collect(),
            ..Default::default()
        };

        let mut resolved = ResolvedToolOptions::default();
        resolved.apply_overrides(&config_opts, ToolOptionSource::Config);
        resolved.apply_overrides(&inline_opts, ToolOptionSource::InlineBackendArg);

        assert_eq!(
            resolved.source_for_key("os"),
            Some(ToolOptionSource::Config)
        );
        assert_eq!(
            resolved.source_for_key("install_env.CONFIG_ONLY"),
            Some(ToolOptionSource::Config)
        );
        assert_eq!(
            resolved.source_for_key("install_env.INLINE_ONLY"),
            Some(ToolOptionSource::InlineBackendArg)
        );
        assert!(resolved.has_key_from_sources("install_env", &[ToolOptionSource::Config]));
        assert!(resolved.has_key_from_sources("depends", &[ToolOptionSource::InlineBackendArg]));
        assert!(!resolved.has_any_key_except_from_sources(
            &["install_env", "depends"],
            &[ToolOptionSource::InlineBackendArg],
        ));
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

    #[test]
    fn test_depends_field() {
        let tvo = ToolVersionOptions {
            depends: Some(vec!["python".to_string(), "node".to_string()]),
            ..Default::default()
        };
        assert_eq!(
            tvo.depends,
            Some(vec!["python".to_string(), "node".to_string()])
        );
        assert!(!tvo.is_empty());
    }

    #[test]
    fn test_depends_none_is_empty() {
        let tvo = ToolVersionOptions {
            depends: None,
            ..Default::default()
        };
        assert!(tvo.is_empty());
    }

    #[test]
    fn test_depends_empty_vec_is_empty() {
        let tvo = ToolVersionOptions {
            depends: Some(vec![]),
            ..Default::default()
        };
        assert!(tvo.is_empty());
    }

    #[test]
    fn test_os_field_is_not_empty() {
        let tvo = ToolVersionOptions {
            os: Some(vec!["linux".to_string()]),
            ..Default::default()
        };
        assert!(!tvo.is_empty());
    }

    #[test]
    fn test_apply_overrides_replaces_existing_values() {
        let mut base = ToolVersionOptions {
            os: Some(vec!["linux".to_string()]),
            depends: Some(vec!["node".to_string()]),
            install_env: [("BASE".to_string(), "1".to_string())]
                .iter()
                .cloned()
                .collect(),
            opts: [
                ("api_url".to_string(), s("https://config.example")),
                ("version_prefix".to_string(), s("v")),
            ]
            .iter()
            .cloned()
            .collect(),
        };
        let overrides = ToolVersionOptions {
            os: Some(vec!["macos".to_string()]),
            depends: Some(vec!["python".to_string()]),
            install_env: [("BASE".to_string(), "2".to_string())]
                .iter()
                .cloned()
                .collect(),
            opts: [("api_url".to_string(), s("https://inline.example"))]
                .iter()
                .cloned()
                .collect(),
        };

        base.apply_overrides(&overrides);

        assert_eq!(base.os, Some(vec!["macos".to_string()]));
        assert_eq!(base.depends, Some(vec!["python".to_string()]));
        assert_eq!(base.install_env.get("BASE").map(String::as_str), Some("2"));
        assert_eq!(base.get("api_url"), Some("https://inline.example"));
        assert_eq!(base.get("version_prefix"), Some("v"));
    }
}
