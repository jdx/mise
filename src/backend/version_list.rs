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
use crate::backend::jq;
use eyre::Result;
use regex::Regex;
use std::collections::HashSet;

/// Fetch and parse versions from a version list URL
pub async fn fetch_versions(
    version_list_url: &str,
    version_regex: Option<&str>,
    version_json_path: Option<&str>,
) -> Result<Vec<String>> {
    use crate::http::HTTP;

    // Fetch the content
    let response = HTTP.get_text(version_list_url).await?;
    let content = response.trim();

    // Parse versions based on format
    parse_version_list(content, version_regex, version_json_path)
}

/// Parse version list from content using optional regex pattern or JSON path
pub fn parse_version_list(
    content: &str,
    version_regex: Option<&str>,
    version_json_path: Option<&str>,
) -> Result<Vec<String>> {
    let mut versions = Vec::new();
    let trimmed = content.trim();

    // If a JSON path is provided (like ".[].version" or ".versions"), try to use it
    // but fall back to text parsing if JSON parsing fails
    if let Some(json_path) = version_json_path {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(trimmed) {
            if let Ok(extracted) = jq::extract(&json, json_path) {
                versions = extracted;
            }
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

    Ok(versions)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_single_version() {
        let content = "2.0.53";
        let versions = parse_version_list(content, None, None).unwrap();
        assert_eq!(versions, vec!["2.0.53"]);
    }

    #[test]
    fn test_parse_single_version_with_v_prefix() {
        let content = "v2.0.53";
        let versions = parse_version_list(content, None, None).unwrap();
        assert_eq!(versions, vec!["2.0.53"]);
    }

    #[test]
    fn test_parse_line_separated_versions() {
        let content = "1.0.0\n1.1.0\n2.0.0";
        let versions = parse_version_list(content, None, None).unwrap();
        assert_eq!(versions, vec!["1.0.0", "1.1.0", "2.0.0"]);
    }

    #[test]
    fn test_parse_line_separated_with_comments() {
        let content = "# Latest versions\n1.0.0\n# Stable\n2.0.0";
        let versions = parse_version_list(content, None, None).unwrap();
        assert_eq!(versions, vec!["1.0.0", "2.0.0"]);
    }

    #[test]
    fn test_parse_json_array_of_strings() {
        let content = r#"["1.0.0", "1.1.0", "2.0.0"]"#;
        let versions = parse_version_list(content, None, None).unwrap();
        assert_eq!(versions, vec!["1.0.0", "1.1.0", "2.0.0"]);
    }

    #[test]
    fn test_parse_json_array_with_v_prefix() {
        let content = r#"["v1.0.0", "v1.1.0", "v2.0.0"]"#;
        let versions = parse_version_list(content, None, None).unwrap();
        assert_eq!(versions, vec!["1.0.0", "1.1.0", "2.0.0"]);
    }

    #[test]
    fn test_parse_json_array_of_objects_with_version() {
        let content = r#"[{"version": "1.0.0"}, {"version": "2.0.0"}]"#;
        let versions = parse_version_list(content, None, None).unwrap();
        assert_eq!(versions, vec!["1.0.0", "2.0.0"]);
    }

    #[test]
    fn test_parse_json_array_of_objects_with_tag_name() {
        let content = r#"[{"tag_name": "v1.0.0"}, {"tag_name": "v2.0.0"}]"#;
        let versions = parse_version_list(content, None, None).unwrap();
        assert_eq!(versions, vec!["1.0.0", "2.0.0"]);
    }

    #[test]
    fn test_parse_json_object_with_versions_array() {
        let content = r#"{"versions": ["1.0.0", "2.0.0"]}"#;
        let versions = parse_version_list(content, None, None).unwrap();
        assert_eq!(versions, vec!["1.0.0", "2.0.0"]);
    }

    #[test]
    fn test_parse_with_regex() {
        let content = "version 1.0.0\nversion 2.0.0\nother stuff";
        let versions = parse_version_list(content, Some(r"version (\d+\.\d+\.\d+)"), None).unwrap();
        assert_eq!(versions, vec!["1.0.0", "2.0.0"]);
    }

    #[test]
    fn test_parse_with_json_path_array_field() {
        let content = r#"{"data": {"versions": ["1.0.0", "2.0.0"]}}"#;
        let versions = parse_version_list(content, None, Some(".data.versions[]")).unwrap();
        assert_eq!(versions, vec!["1.0.0", "2.0.0"]);
    }

    #[test]
    fn test_parse_with_json_path_object_array() {
        let content = r#"[{"version": "1.0.0"}, {"version": "2.0.0"}]"#;
        let versions = parse_version_list(content, None, Some(".[].version")).unwrap();
        assert_eq!(versions, vec!["1.0.0", "2.0.0"]);
    }

    #[test]
    fn test_parse_with_json_path_nested() {
        let content =
            r#"{"releases": [{"info": {"version": "1.0.0"}}, {"info": {"version": "2.0.0"}}]}"#;
        let versions = parse_version_list(content, None, Some(".releases[].info.version")).unwrap();
        assert_eq!(versions, vec!["1.0.0", "2.0.0"]);
    }

    #[test]
    fn test_parse_removes_duplicates() {
        let content = "1.0.0\n1.0.0\n2.0.0\n2.0.0";
        let versions = parse_version_list(content, None, None).unwrap();
        assert_eq!(versions, vec!["1.0.0", "2.0.0"]);
    }

    #[test]
    fn test_parse_empty_content() {
        let content = "";
        let versions = parse_version_list(content, None, None).unwrap();
        assert!(versions.is_empty());
    }

    #[test]
    fn test_parse_whitespace_only() {
        let content = "   \n\n   ";
        let versions = parse_version_list(content, None, None).unwrap();
        assert!(versions.is_empty());
    }

    #[test]
    fn test_parse_json_path_with_invalid_json_falls_back_to_text() {
        // When version_json_path is provided but content is not valid JSON,
        // it should gracefully fall back to text parsing
        let content = "1.0.0\n2.0.0";
        let versions = parse_version_list(content, None, Some(".[].version")).unwrap();
        assert_eq!(versions, vec!["1.0.0", "2.0.0"]);
    }

    #[test]
    fn test_parse_json_path_with_wrong_path_falls_back_to_text() {
        // When version_json_path doesn't match the JSON structure,
        // it should fall back to text parsing
        let content = r#"{"other": "data"}"#;
        let versions = parse_version_list(content, None, Some(".[].version")).unwrap();
        // Falls back to treating JSON as a single line of text
        assert_eq!(versions, vec![r#"{"other": "data"}"#]);
    }
}
