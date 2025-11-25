/// Simplified jq-like JSON path extraction.
///
/// Supports a subset of jq syntax for extracting values from JSON:
/// - `.` - root value
/// - `.[]` - iterate over array elements
/// - `.[].field` - extract field from each array element
/// - `.field` - extract field from object
/// - `.field[]` - iterate over array in field
/// - `.field[].subfield` - extract subfield from array elements
/// - `.field.subfield` - nested field access
///
/// Values are extracted as strings, with 'v' prefix stripped from version-like values.
use eyre::Result;

/// Extract string values from JSON using a jq-like path expression
///
/// # Examples
/// ```
/// use mise::backend::jq::extract;
/// use serde_json::json;
///
/// let data = json!(["1.0.0", "2.0.0"]);
/// assert_eq!(extract(&data, ".[]").unwrap(), vec!["1.0.0", "2.0.0"]);
///
/// let data = json!([{"version": "v1.0.0"}, {"version": "v2.0.0"}]);
/// assert_eq!(extract(&data, ".[].version").unwrap(), vec!["1.0.0", "2.0.0"]);
/// ```
pub fn extract(json: &serde_json::Value, path: &str) -> Result<Vec<String>> {
    let mut results = Vec::new();
    let path = path.trim();

    // Handle empty path or "." as root
    if path.is_empty() || path == "." {
        extract_values(json, &mut results);
        return Ok(results);
    }

    // Remove leading dot if present
    let path = path.strip_prefix('.').unwrap_or(path);

    // Parse the path and extract values
    extract_recursive(json, path, &mut results);

    Ok(results)
}

/// Extract values with auto-detection of common version patterns
pub fn extract_auto(json: &serde_json::Value) -> Vec<String> {
    let mut results = Vec::new();

    match json {
        serde_json::Value::String(s) => {
            let v = normalize_version(s);
            if !v.is_empty() {
                results.push(v);
            }
        }
        serde_json::Value::Array(arr) => {
            for val in arr {
                if let Some(v) = val.as_str() {
                    let v = normalize_version(v);
                    if !v.is_empty() {
                        results.push(v);
                    }
                } else if let Some(obj) = val.as_object() {
                    // Try common version field names
                    for field in ["version", "tag_name", "name", "tag", "v"] {
                        if let Some(v) = obj.get(field).and_then(|v| v.as_str()) {
                            let v = normalize_version(v);
                            if !v.is_empty() {
                                results.push(v);
                                break;
                            }
                        }
                    }
                }
            }
        }
        serde_json::Value::Object(obj) => {
            // Check for common patterns like {"versions": [...]} or {"releases": [...]}
            for field in ["versions", "releases", "tags", "version", "release"] {
                if let Some(val) = obj.get(field) {
                    let extracted = extract_auto(val);
                    if !extracted.is_empty() {
                        return extracted;
                    }
                }
            }
        }
        _ => {}
    }

    results
}

fn extract_recursive(json: &serde_json::Value, path: &str, results: &mut Vec<String>) {
    if path.is_empty() {
        // End of path, extract value(s)
        extract_values(json, results);
        return;
    }

    // Handle array iteration "[]"
    if path == "[]" {
        if let Some(arr) = json.as_array() {
            for val in arr {
                extract_values(val, results);
            }
        }
        return;
    }

    // Handle "[]." prefix (iterate then continue path)
    if let Some(rest) = path.strip_prefix("[].") {
        if let Some(arr) = json.as_array() {
            for val in arr {
                extract_recursive(val, rest, results);
            }
        }
        return;
    }

    // Handle field access with possible continuation
    // Find where the field name ends (at '.' or '[')
    let (field, rest) = if let Some(idx) = path.find(['.', '[']) {
        let (f, r) = path.split_at(idx);
        // Strip the leading dot if present, but preserve '[' for array handling
        let rest = if r.starts_with('.') { &r[1..] } else { r };
        (f, rest)
    } else {
        (path, "")
    };

    if let Some(obj) = json.as_object()
        && let Some(val) = obj.get(field)
    {
        extract_recursive(val, rest, results);
    }
}

fn extract_values(json: &serde_json::Value, results: &mut Vec<String>) {
    match json {
        serde_json::Value::String(s) => {
            let v = normalize_version(s);
            if !v.is_empty() {
                results.push(v);
            }
        }
        serde_json::Value::Array(arr) => {
            for val in arr {
                if let Some(s) = val.as_str() {
                    let v = normalize_version(s);
                    if !v.is_empty() {
                        results.push(v);
                    }
                }
            }
        }
        serde_json::Value::Number(n) => {
            results.push(n.to_string());
        }
        _ => {}
    }
}

/// Normalize a version string by trimming whitespace and stripping 'v' prefix
fn normalize_version(s: &str) -> String {
    s.trim().trim_start_matches('v').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_extract_root_string() {
        let data = json!("v2.0.53");
        assert_eq!(extract(&data, ".").unwrap(), vec!["2.0.53"]);
    }

    #[test]
    fn test_extract_root_array() {
        let data = json!(["1.0.0", "2.0.0"]);
        assert_eq!(extract(&data, ".").unwrap(), vec!["1.0.0", "2.0.0"]);
    }

    #[test]
    fn test_extract_array_iterate() {
        let data = json!(["v1.0.0", "v2.0.0"]);
        assert_eq!(extract(&data, ".[]").unwrap(), vec!["1.0.0", "2.0.0"]);
    }

    #[test]
    fn test_extract_array_field() {
        let data = json!([{"version": "1.0.0"}, {"version": "2.0.0"}]);
        assert_eq!(
            extract(&data, ".[].version").unwrap(),
            vec!["1.0.0", "2.0.0"]
        );
    }

    #[test]
    fn test_extract_nested_field() {
        let data = json!({"data": {"version": "1.0.0"}});
        assert_eq!(extract(&data, ".data.version").unwrap(), vec!["1.0.0"]);
    }

    #[test]
    fn test_extract_nested_array() {
        let data = json!({"data": {"versions": ["1.0.0", "2.0.0"]}});
        assert_eq!(
            extract(&data, ".data.versions[]").unwrap(),
            vec!["1.0.0", "2.0.0"]
        );
    }

    #[test]
    fn test_extract_deeply_nested() {
        let data =
            json!({"releases": [{"info": {"version": "1.0.0"}}, {"info": {"version": "2.0.0"}}]});
        assert_eq!(
            extract(&data, ".releases[].info.version").unwrap(),
            vec!["1.0.0", "2.0.0"]
        );
    }

    #[test]
    fn test_extract_object_field_array() {
        let data = json!({"versions": ["1.0.0", "2.0.0"]});
        assert_eq!(
            extract(&data, ".versions[]").unwrap(),
            vec!["1.0.0", "2.0.0"]
        );
    }

    #[test]
    fn test_extract_empty_path() {
        let data = json!("1.0.0");
        assert_eq!(extract(&data, "").unwrap(), vec!["1.0.0"]);
    }

    #[test]
    fn test_extract_missing_field() {
        let data = json!({"foo": "bar"});
        assert!(extract(&data, ".missing").unwrap().is_empty());
    }

    #[test]
    fn test_extract_auto_string() {
        let data = json!("v1.0.0");
        assert_eq!(extract_auto(&data), vec!["1.0.0"]);
    }

    #[test]
    fn test_extract_auto_array_strings() {
        let data = json!(["v1.0.0", "v2.0.0"]);
        assert_eq!(extract_auto(&data), vec!["1.0.0", "2.0.0"]);
    }

    #[test]
    fn test_extract_auto_array_objects() {
        let data = json!([{"version": "1.0.0"}, {"tag_name": "v2.0.0"}]);
        assert_eq!(extract_auto(&data), vec!["1.0.0", "2.0.0"]);
    }

    #[test]
    fn test_extract_auto_object_versions_field() {
        let data = json!({"versions": ["1.0.0", "2.0.0"]});
        assert_eq!(extract_auto(&data), vec!["1.0.0", "2.0.0"]);
    }

    #[test]
    fn test_extract_auto_object_releases_field() {
        let data = json!({"releases": ["1.0.0", "2.0.0"]});
        assert_eq!(extract_auto(&data), vec!["1.0.0", "2.0.0"]);
    }
}
