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
/// - `version_date_url`: URL holding release dates, when not `version_list_url`
/// - `version_date_next_expr`: Expression naming the next page of `version_date_url`
/// - `version_date_expr`: Expression using expr-lang syntax to map each version to its release date
use crate::backend::jq;
use expr::{Context, Environment, Value};
use eyre::Result;
use regex::Regex;
use reqwest::header::HeaderMap;
use std::collections::{HashMap, HashSet};

/// Fetch and parse versions from a version list URL, also returning the trimmed body
/// they came from so `version_date_expr` can read it without a second request.
pub(crate) async fn fetch_versions_with_body(
    version_list_url: &str,
    headers: &HeaderMap,
    version_regex: Option<&str>,
    version_json_path: Option<&str>,
    version_expr: Option<&str>,
) -> Result<(Vec<String>, String)> {
    use crate::http::HTTP;

    let content = if version_regex.is_some() {
        // When a regex is provided, the caller expects to parse arbitrary
        // content (including HTML directory listings), so bypass the HTML rejection
        // in get_text.
        HTTP.get_text_request(version_list_url)
            .headers(headers)
            .allow_html()
            .send()
            .await?
    } else {
        HTTP.get_text_request(version_list_url)
            .headers(headers)
            .send()
            .await?
    };
    let content = content.trim();

    // Parse versions based on format
    let versions = parse_version_list(content, version_regex, version_json_path, version_expr)?;
    Ok((versions, content.to_string()))
}

/// Parse version list from content using optional regex pattern, JSON path, or expr
pub(crate) fn parse_version_list(
    content: &str,
    version_regex: Option<&str>,
    version_json_path: Option<&str>,
    version_expr: Option<&str>,
) -> Result<Vec<String>> {
    let mut versions = Vec::new();
    let trimmed = content.trim();
    let mut parsed_json_path = false;

    // If a JSON path is provided (like ".[].version" or ".versions"), try to use it
    // but fall back to text parsing only when the body is not JSON.
    if let Some(json_path) = version_json_path {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(trimmed) {
            parsed_json_path = true;
            if let Ok(extracted) = jq::extract(&json, json_path) {
                versions = extracted;
            }
        }
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

    // If no versions extracted yet and no explicit extraction method was provided,
    // treat as line-separated or single version.
    // When version_regex or version_expr is set, zero matches means the content
    // didn't contain the expected data — don't fall through to line-splitting
    // which would emit garbage (e.g. raw HTML lines as "versions").
    let explicit_method = version_regex.is_some() || version_expr.is_some() || parsed_json_path;
    if versions.is_empty() && !explicit_method {
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
    // When a registry entry needs explicit sorting, it can use version_expr
    // (e.g. sortVersions) to post-process the extracted versions.
    // When no other extraction method produced results, the expr operates on
    // `body` alone (the original behavior).
    if let Some(expr_str) = version_expr {
        versions = eval_version_expr(expr_str, trimmed, &versions)?;
    }

    Ok(versions)
}

/// Evaluate a version expression using expr-lang.
/// The previously extracted version list is always injected as a `versions`
/// variable so the expression can post-process it (e.g. sortVersions).
fn eval_version_expr(expr_str: &str, body: &str, versions: &[String]) -> Result<Vec<String>> {
    let mut ctx = Context::default();
    ctx.insert("body".to_string(), Value::String(body.to_string()));
    ctx.insert("versions".to_string(), string_array(versions));

    // expr-lang 1.0+ has built-in fromJSON, keys, values, len, toJSON functions
    let env = expr_environment();
    let result = env.eval(expr_str, &ctx)?;
    value_to_strings(result)
}

fn string_array(items: &[String]) -> Value {
    Value::Array(items.iter().map(|v| Value::String(v.clone())).collect())
}

/// The expr environment shared by `version_expr` and `version_date_expr`.
fn expr_environment() -> Environment<'static> {
    use versions::Versioning;

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
    env
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

/// The most pages `version_date_next_expr` may chain before the listing fails, so an
/// expression that never returns an empty answer cannot loop forever.
const MAX_DATE_PAGES: usize = 100;

/// Fetch the pages `version_date_expr` reads: `first_url`, then whatever
/// `next_expr` names after each page until it answers nothing. `next_expr` sees
/// `body` (the page just fetched) and `url` (its address), and must name a page on
/// the same host, since `headers` follow every request.
pub(crate) async fn fetch_date_sources(
    first_url: &str,
    next_expr: Option<&str>,
    headers: &HeaderMap,
) -> Result<Vec<String>> {
    use crate::http::HTTP;

    let origin = url::Url::parse(first_url)?.origin();
    let mut bodies = Vec::new();
    let mut url = first_url.to_string();
    loop {
        let body = HTTP
            .get_text_request(&url)
            .headers(headers)
            .allow_html()
            .send()
            .await?;
        let body = body.trim().to_string();
        let next = match next_expr {
            Some(expr) => next_date_url(expr, &body, &url)?,
            None => None,
        };
        bodies.push(body);
        let Some(next) = next else {
            return Ok(bodies);
        };
        if url::Url::parse(&next)?.origin() != origin {
            eyre::bail!(
                "version_date_next_expr named {next}, which is not on the host of {first_url}"
            );
        }
        if next == url || bodies.len() >= MAX_DATE_PAGES {
            eyre::bail!(
                "version_date_next_expr did not finish after {} pages of {first_url}",
                bodies.len()
            );
        }
        url = next;
    }
}

/// Evaluate `version_date_next_expr` for one fetched page.
fn next_date_url(expr_str: &str, body: &str, url: &str) -> Result<Option<String>> {
    let mut ctx = Context::default();
    ctx.insert("body".to_string(), Value::String(body.to_string()));
    ctx.insert("url".to_string(), Value::String(url.to_string()));
    match expr_environment().eval(expr_str, &ctx)? {
        Value::Nil => Ok(None),
        Value::String(next) if next.is_empty() => Ok(None),
        Value::String(next) => Ok(Some(next)),
        _ => eyre::bail!("version_date_next_expr must return a url string or nil"),
    }
}

/// Evaluate `version_date_expr` and return each version's release date.
///
/// The expression sees `body` (the first page), `bodies` (every page, for a paged `version_date_url`) and
/// `versions` (the resolved list). It returns a list of `[version, date]` pairs
/// or a map of version to date. Entries for versions that are not in
/// `versions`, or whose date is empty, are dropped, so a version missing from
/// the date source stays undated rather than failing the listing.
pub(crate) fn eval_version_dates(
    expr_str: &str,
    bodies: &[String],
    versions: &[String],
) -> Result<HashMap<String, String>> {
    let mut ctx = Context::default();
    ctx.insert(
        "body".to_string(),
        Value::String(bodies.first().cloned().unwrap_or_default()),
    );
    ctx.insert("bodies".to_string(), string_array(bodies));
    ctx.insert("versions".to_string(), string_array(versions));

    let result = expr_environment().eval(expr_str, &ctx)?;
    let mut pairs = Vec::new();
    match result {
        Value::Map(map) => {
            for (version, date) in map {
                pairs.push((Some(version), value_as_string(&date)));
            }
        }
        Value::KeyedMap(entries) => {
            for (version, date) in entries {
                pairs.push((value_as_string(&version), value_as_string(&date)));
            }
        }
        Value::Array(items) => {
            for item in items {
                match item {
                    Value::Array(pair) if pair.len() == 2 => {
                        pairs.push((value_as_string(&pair[0]), value_as_string(&pair[1])));
                    }
                    _ => eyre::bail!("version_date_expr must return [version, date] pairs"),
                }
            }
        }
        _ => eyre::bail!("version_date_expr must return [version, date] pairs or a map"),
    }

    let known: HashSet<&str> = versions.iter().map(String::as_str).collect();
    let mut dates = HashMap::new();
    let mut unreadable = Vec::new();
    for (version, date) in pairs {
        let (Some(version), Some(date)) = (version, date) else {
            continue;
        };
        let date = date.trim();
        if date.is_empty() || !known.contains(version.as_str()) {
            continue;
        }
        match normalize_date(date) {
            Some(date) => {
                dates.insert(version, date);
            }
            None => unreadable.push(date.to_string()),
        }
    }
    if let Some(example) = unreadable.first() {
        warn!(
            "version_date_expr returned {} dates mise cannot read, such as {example:?}; \
             use RFC 3339, an HTTP date or YYYY-MM-DD",
            unreadable.len()
        );
    }
    Ok(dates)
}

/// A date `created_at` can hold: what `parse_into_timestamp` reads, plus the RFC 2822
/// form of an HTTP `Last-Modified` header (`Thu, 15 Oct 2026 11:33:41 GMT`).
fn normalize_date(date: &str) -> Option<String> {
    if crate::duration::parse_into_timestamp(date).is_ok() {
        return Some(date.to_string());
    }
    let zoned = jiff::fmt::rfc2822::parse(date).ok()?;
    Some(zoned.timestamp().to_string())
}

fn value_as_string(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Integer(n) => Some(n.to_string()),
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
    fn test_parse_json_path_with_wrong_path_returns_empty_for_json() {
        // When version_json_path doesn't match a valid JSON document, it should
        // return no versions instead of treating the JSON body as a version.
        let content = r#"{"other": "data"}"#;
        let versions = parse_version_list(content, None, Some(".[].version"), None).unwrap();
        assert!(versions.is_empty());
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
            Some(r#"fromJSON(body).releases | filter({ #.channel == "stable" }) | map({ #.version }) | sortVersions()"#),
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

    #[test]
    fn test_parse_with_version_expr_filter_keys_with_regex() {
        // Test version_expr with filter + # matches for Julia-style JSON (keys with non-version entries)
        let content = r#"{"1.0.0":{},"1.1.0":{},"nightly":{},"latest":{},"1.2.0-rc1":{}}"#;
        let versions = parse_version_list(
            content,
            None,
            None,
            Some(r#"sortVersions(filter(keys(fromJSON(body)), { # matches "^\\d+\\.\\d+\\.\\d+(-[0-9A-Za-z\\.-]+)?$" }))"#),
        )
        .unwrap();
        assert_eq!(versions, vec!["1.0.0", "1.1.0", "1.2.0-rc1"]);
    }

    #[test]
    fn test_parse_with_regex_and_version_expr_pipeline() {
        let content =
            r#"<a href="4.9.9/">4.9.9</a><a href="4.21.3/">4.21.3</a><a href="4.10.1/">4.10.1</a>"#;
        let versions = parse_version_list(
            content,
            Some(r#"href="(\d+\.\d+\.\d+)/""#),
            None,
            Some("sortVersions(versions)"),
        )
        .unwrap();
        assert_eq!(versions, vec!["4.9.9", "4.10.1", "4.21.3"]);
    }

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn test_version_dates_from_pairs() {
        let body = r#"{"releases":[
            {"version":"1.0.0","date":"2024-01-02T03:04:05Z"},
            {"version":"2.0.0","date":"2024-05-06"},
            {"version":"9.9.9","date":"2024-07-08"}
        ]}"#;
        let dates = eval_version_dates(
            "map(fromJSON(body).releases, { [#.version, #.date] })",
            &[body.to_string()],
            &strings(&["1.0.0", "2.0.0", "3.0.0"]),
        )
        .unwrap();
        assert_eq!(dates.len(), 2);
        assert_eq!(dates["1.0.0"], "2024-01-02T03:04:05Z");
        assert_eq!(dates["2.0.0"], "2024-05-06");
    }

    #[test]
    fn test_version_dates_from_from_pairs() {
        let dates = eval_version_dates(
            r#"fromPairs([["1.0.0", "2024-01-02"]])"#,
            &[String::new()],
            &strings(&["1.0.0"]),
        )
        .unwrap();
        assert_eq!(dates["1.0.0"], "2024-01-02");
    }

    #[test]
    fn test_version_dates_from_map() {
        let dates = eval_version_dates(
            r#"{"1.0.0": "2024-01-02", "2.0.0": ""}"#,
            &[String::new()],
            &strings(&["1.0.0", "2.0.0"]),
        )
        .unwrap();
        assert_eq!(dates.len(), 1);
        assert_eq!(dates["1.0.0"], "2024-01-02");
    }

    #[test]
    fn test_version_dates_read_every_page() {
        let pages = [
            r#"[{"v":"1.0.0","d":"2024-01-02"}]"#.to_string(),
            r#"[{"v":"2.0.0","d":"2024-03-04"}]"#.to_string(),
        ];
        let dates = eval_version_dates(
            "map(concat(fromJSON(bodies[0]), fromJSON(bodies[1])), { [#.v, #.d] })",
            &pages,
            &strings(&["1.0.0", "2.0.0"]),
        )
        .unwrap();
        assert_eq!(dates["1.0.0"], "2024-01-02");
        assert_eq!(dates["2.0.0"], "2024-03-04");
    }

    #[test]
    fn test_version_dates_accept_http_dates_and_drop_unreadable_ones() {
        let dates = eval_version_dates(
            r#"{"1.0.0": "Thu, 15 Oct 2026 11:33:41 GMT", "2.0.0": "last tuesday"}"#,
            &[String::new()],
            &strings(&["1.0.0", "2.0.0"]),
        )
        .unwrap();
        assert_eq!(dates.len(), 1);
        assert_eq!(dates["1.0.0"], "2026-10-15T11:33:41Z");
    }

    #[test]
    fn test_version_dates_reject_other_shapes() {
        assert!(eval_version_dates("\"2024-01-02\"", &[String::new()], &[]).is_err());
        assert!(eval_version_dates("[1, 2]", &[String::new()], &[]).is_err());
        assert!(eval_version_dates("[[1, 2, 3]]", &[String::new()], &[]).is_err());
    }
}
