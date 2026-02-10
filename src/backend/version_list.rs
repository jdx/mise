/// Version list parsing utilities for fetching remote versions from URLs.
///
/// Supports multiple formats:
/// - Single version (plain text)
/// - Line-separated versions
/// - JSON arrays of strings
/// - JSON arrays of objects with version fields
/// - JSON objects with nested version arrays
///
/// Options:
/// - `version_list_url`: URL to fetch version list from
/// - `version_regex`: Regex pattern to extract versions (first capturing group or full match)
/// - `version_json_path`: JQ-like path to extract versions from JSON (e.g., `.[].version`)
/// - `version_expr`: Expression using expr-lang syntax to extract versions
use crate::backend::jq;
use expr::{Context, Environment, Value};
use eyre::Result;
use regex::Regex;
use std::collections::HashSet;

/// Fetch and parse versions from a version list URL
pub async fn fetch_versions(
    version_list_url: &str,
    version_regex: Option<&str>,
    version_json_path: Option<&str>,
    version_expr: Option<&str>,
) -> Result<Vec<String>> {
    use crate::http::HTTP;

    // Fetch the content
    let response = HTTP.get_text(version_list_url).await?;
    let content = response.trim();

    // Parse versions based on format
    parse_version_list(content, version_regex, version_json_path, version_expr)
}

/// Parse version list from content using optional regex pattern, JSON path, or expr
pub fn parse_version_list(
    content: &str,
    version_regex: Option<&str>,
    version_json_path: Option<&str>,
    version_expr: Option<&str>,
) -> Result<Vec<String>> {
    let mut versions = Vec::new();
    let trimmed = content.trim();

    // If an expr is provided, use it to evaluate and extract versions
    // Fail hard if the expression is invalid - don't silently fall through
    if let Some(expr_str) = version_expr {
        versions = eval_version_expr(expr_str, trimmed)?;
    }
    // If a JSON path is provided (like ".[].version" or ".versions"), try to use it
    // but fall back to text parsing if JSON parsing fails
    else if let Some(json_path) = version_json_path {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(trimmed)
            && let Ok(extracted) = jq::extract(&json, json_path)
        {
            versions = extracted;
        }
        // If JSON parsing failed or path extraction failed, fall through to text parsing below
    }
    // If a regex is provided, use it to extract versions
    else if let Some(pattern) = version_regex {
        let re = Regex::new(pattern)?;
        for cap in re.captures_iter(content) {
            // Use the first capturing group if present, otherwise the whole match
            let version = cap
                .get(1)
                .or_else(|| cap.get(0))
                .map(|m| m.as_str().to_string());
            if let Some(v) = version {
                let v = v.trim();
                if !v.is_empty() {
                    versions.push(v.to_string());
                }
            }
        }
    } else {
        // Try to detect the format automatically

        // Check if it looks like JSON array or object
        if trimmed.starts_with('[') || trimmed.starts_with('{') {
            // Try to parse as JSON
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(trimmed) {
                versions = jq::extract_auto(&json);
            }
        }
    }

    // If no versions extracted yet, treat as line-separated or single version
    // This provides fallback for all cases including failed JSON parsing
    if versions.is_empty() {
        for line in trimmed.lines() {
            let line = line.trim();
            // Skip empty lines and comments
            if !line.is_empty() && !line.starts_with('#') {
                // Strip common version prefixes
                let version = line.trim_start_matches('v');
                versions.push(version.to_string());
            }
        }
    }

    // Remove duplicates while preserving order
    let mut seen = HashSet::new();
    versions.retain(|v| seen.insert(v.clone()));

    // DO NOT sort versions here - the backend/upstream determines version order.
    // Sorting is handled elsewhere (e.g., versions host, resolve logic).

    Ok(versions)
}

/// Evaluate a version expression using expr-lang
fn eval_version_expr(expr_str: &str, body: &str) -> Result<Vec<String>> {
    use versions::Versioning;

    let mut ctx = Context::default();
    ctx.insert("body".to_string(), Value::String(body.to_string()));

    // expr-lang 1.0+ has built-in fromJSON, keys, values, len, toJSON functions
    let mut env = Environment::new();

    // Add sortVersions function for semver-aware sorting
    env.add_function("sortVersions", |c| {
        if c.args.len() != 1 {
            return Err("sortVersions() takes exactly one argument"
                .to_string()
                .into());
        }
        let Value::Array(arr) = &c.args[0] else {
            return Err("sortVersions() takes an array as the first argument"
                .to_string()
                .into());
        };
        let mut versions: Vec<_> = arr
            .iter()
            .filter_map(|v| v.as_string().map(|s| s.to_string()))
            .collect();
        versions.sort_by_cached_key(|v| Versioning::new(v));
        Ok(Value::Array(
            versions.into_iter().map(Value::String).collect(),
        ))
    });

    let result = env.eval(expr_str, &ctx)?;
    value_to_strings(result)
}

/// Convert expr Value to a list of strings
fn value_to_strings(value: Value) -> Result<Vec<String>> {
    match value {
        Value::Array(arr) => {
            let mut result = Vec::new();
            for v in arr {
                if let Some(s) = value_as_string(&v)
                    && !s.is_empty()
                {
                    result.push(s);
                }
            }
            Ok(result)
        }
        _ => {
            if let Some(s) = value_as_string(&value)
                && !s.is_empty()
            {
                return Ok(vec![s]);
            }
            Ok(vec![])
        }
    }
}

fn value_as_string(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_single_version() {
        let content = "2.0.53";
        let versions = parse_version_list(content, None, None, None).unwrap();
        assert_eq!(versions, vec!["2.0.53"]);
    }

    #[test]
    fn test_parse_single_version_with_v_prefix() {
        let content = "v2.0.53";
        let versions = parse_version_list(content, None, None, None).unwrap();
        assert_eq!(versions, vec!["2.0.53"]);
    }

    #[test]
    fn test_parse_line_separated_versions() {
        let content = "1.0.0\n1.1.0\n2.0.0";
        let versions = parse_version_list(content, None, None, None).unwrap();
        assert_eq!(versions, vec!["1.0.0", "1.1.0", "2.0.0"]);
    }

    #[test]
    fn test_parse_line_separated_with_comments() {
        let content = "# Latest versions\n1.0.0\n# Stable\n2.0.0";
        let versions = parse_version_list(content, None, None, None).unwrap();
        assert_eq!(versions, vec!["1.0.0", "2.0.0"]);
    }

    #[test]
    fn test_parse_json_array_of_strings() {
        let content = r#"["1.0.0", "1.1.0", "2.0.0"]"#;
        let versions = parse_version_list(content, None, None, None).unwrap();
        assert_eq!(versions, vec!["1.0.0", "1.1.0", "2.0.0"]);
    }

    #[test]
    fn test_parse_json_array_with_v_prefix() {
        let content = r#"["v1.0.0", "v1.1.0", "v2.0.0"]"#;
        let versions = parse_version_list(content, None, None, None).unwrap();
        assert_eq!(versions, vec!["1.0.0", "1.1.0", "2.0.0"]);
    }

    #[test]
    fn test_parse_json_array_of_objects_with_version() {
        let content = r#"[{"version": "1.0.0"}, {"version": "2.0.0"}]"#;
        let versions = parse_version_list(content, None, None, None).unwrap();
        assert_eq!(versions, vec!["1.0.0", "2.0.0"]);
    }

    #[test]
    fn test_parse_json_array_of_objects_with_tag_name() {
        let content = r#"[{"tag_name": "v1.0.0"}, {"tag_name": "v2.0.0"}]"#;
        let versions = parse_version_list(content, None, None, None).unwrap();
        assert_eq!(versions, vec!["1.0.0", "2.0.0"]);
    }

    #[test]
    fn test_parse_json_object_with_versions_array() {
        let content = r#"{"versions": ["1.0.0", "2.0.0"]}"#;
        let versions = parse_version_list(content, None, None, None).unwrap();
        assert_eq!(versions, vec!["1.0.0", "2.0.0"]);
    }

    #[test]
    fn test_parse_with_regex() {
        let content = "version 1.0.0\nversion 2.0.0\nother stuff";
        let versions =
            parse_version_list(content, Some(r"version (\d+\.\d+\.\d+)"), None, None).unwrap();
        assert_eq!(versions, vec!["1.0.0", "2.0.0"]);
    }

    #[test]
    fn test_parse_with_json_path_array_field() {
        let content = r#"{"data": {"versions": ["1.0.0", "2.0.0"]}}"#;
        let versions = parse_version_list(content, None, Some(".data.versions[]"), None).unwrap();
        assert_eq!(versions, vec!["1.0.0", "2.0.0"]);
    }

    #[test]
    fn test_parse_with_json_path_object_array() {
        let content = r#"[{"version": "1.0.0"}, {"version": "2.0.0"}]"#;
        let versions = parse_version_list(content, None, Some(".[].version"), None).unwrap();
        assert_eq!(versions, vec!["1.0.0", "2.0.0"]);
    }

    #[test]
    fn test_parse_with_json_path_nested() {
        let content =
            r#"{"releases": [{"info": {"version": "1.0.0"}}, {"info": {"version": "2.0.0"}}]}"#;
        let versions =
            parse_version_list(content, None, Some(".releases[].info.version"), None).unwrap();
        assert_eq!(versions, vec!["1.0.0", "2.0.0"]);
    }

    #[test]
    fn test_parse_removes_duplicates() {
        let content = "1.0.0\n1.0.0\n2.0.0\n2.0.0";
        let versions = parse_version_list(content, None, None, None).unwrap();
        assert_eq!(versions, vec!["1.0.0", "2.0.0"]);
    }

    #[test]
    fn test_parse_empty_content() {
        let content = "";
        let versions = parse_version_list(content, None, None, None).unwrap();
        assert!(versions.is_empty());
    }

    #[test]
    fn test_parse_whitespace_only() {
        let content = "   \n\n   ";
        let versions = parse_version_list(content, None, None, None).unwrap();
        assert!(versions.is_empty());
    }

    #[test]
    fn test_parse_json_path_with_invalid_json_falls_back_to_text() {
        // When version_json_path is provided but content is not valid JSON,
        // it should gracefully fall back to text parsing
        let content = "1.0.0\n2.0.0";
        let versions = parse_version_list(content, None, Some(".[].version"), None).unwrap();
        assert_eq!(versions, vec!["1.0.0", "2.0.0"]);
    }

    #[test]
    fn test_parse_json_path_with_wrong_path_falls_back_to_text() {
        // When version_json_path doesn't match the JSON structure,
        // it should fall back to text parsing
        let content = r#"{"other": "data"}"#;
        let versions = parse_version_list(content, None, Some(".[].version"), None).unwrap();
        // Falls back to treating JSON as a single line of text
        assert_eq!(versions, vec![r#"{"other": "data"}"#]);
    }

    #[test]
    fn test_parse_flutter_json_with_filter() {
        // Test Flutter-style JSON with channel filter
        let content = r#"{
            "releases": [
                {"version": "3.38.7", "channel": "stable"},
                {"version": "3.41.0-0.0.pre", "channel": "beta"},
                {"version": "3.38.6", "channel": "stable"}
            ]
        }"#;
        let versions = parse_version_list(
            content,
            None,
            Some(".releases[?channel=stable].version"),
            None,
        )
        .unwrap();
        assert_eq!(versions, vec!["3.38.7", "3.38.6"]);
    }

    #[test]
    fn test_parse_with_version_expr_split() {
        // Test version_expr with split function
        let content = "1.0.0\n2.0.0\n3.0.0";
        let versions =
            parse_version_list(content, None, None, Some(r#"split(body, "\n")"#)).unwrap();
        assert_eq!(versions, vec!["1.0.0", "2.0.0", "3.0.0"]);
    }

    #[test]
    fn test_parse_flutter_with_version_expr() {
        let content = r#"{
            "releases": [
                {"version": "3.38.7", "channel": "stable"},
                {"version": "3.41.0-0.0.pre", "channel": "beta"},
                {"version": "3.38.6", "channel": "stable"},
                {"version": "1.0.0", "channel": "stable"}
            ]
        }"#;
        let versions = parse_version_list(
            content,
            None,
            None,
            Some(r#"fromJSON(body).releases | filter({#.channel == "stable"}) | map({#.version}) | sortVersions()"#),
        )
        .unwrap();
        assert_eq!(versions, vec!["1.0.0", "3.38.6", "3.38.7"]);
    }

    #[test]
    fn test_parse_with_version_expr_json_keys() {
        // Test version_expr with fromJSON and keys for hashicorp-style JSON
        let content = r#"{"name":"sentinel","versions":{"0.1.0":{},"0.2.0":{},"1.0.0":{}}}"#;
        let versions = parse_version_list(
            content,
            None,
            None,
            Some(r#"keys(fromJSON(body).versions)"#),
        )
        .unwrap();
        // Keys may not be in order, so just check we got all versions
        assert_eq!(versions.len(), 3);
        assert!(versions.contains(&"0.1.0".to_string()));
        assert!(versions.contains(&"0.2.0".to_string()));
        assert!(versions.contains(&"1.0.0".to_string()));
    }
}
