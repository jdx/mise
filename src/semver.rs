use nodejs_semver::{Range, Version as NodeVersion};
use versions::{Mess, Versioning};

/// splits a version number into an optional prefix and the remaining version string
pub fn split_version_prefix(version: &str) -> (String, String) {
    version
        .char_indices()
        .find_map(|(i, c)| {
            if c.is_ascii_digit() {
                if i == 0 {
                    return Some(i);
                }
                // If the previous char is a delimiter or 'v', we found a split point.
                let prev_char = version.chars().nth(i - 1).unwrap();
                if ['-', '_', '/', '.', 'v', 'V'].contains(&prev_char) {
                    return Some(i);
                }
            }
            None
        })
        .map_or_else(
            || ("".into(), version.into()),
            |i| {
                let (prefix, version) = version.split_at(i);
                (prefix.into(), version.into())
            },
        )
}

/// split a version number into chunks
/// given v: "1.2-3a4" return ["1", ".2", "-3a4"]
pub fn chunkify_version(v: &str) -> Vec<String> {
    fn chunkify(m: &Mess, sep0: &str, chunks: &mut Vec<String>) {
        for (i, chunk) in m.chunks.iter().enumerate() {
            let sep = if i == 0 { sep0 } else { "." };
            chunks.push(format!("{sep}{chunk}"));
        }
        if let Some((next_sep, next_mess)) = &m.next {
            chunkify(next_mess, next_sep.to_string().as_ref(), chunks)
        }
    }

    let mut chunks = vec![];
    // don't parse "latest", otherwise bump from latest to any version would have one chunk only
    if v != "latest"
        && let Some(v) = Versioning::new(v)
    {
        let m = match v {
            Versioning::Ideal(sem_ver) => sem_ver.to_mess(),
            Versioning::General(version) => version.to_mess(),
            Versioning::Complex(mess) => mess,
        };
        chunkify(&m, "", &mut chunks);
    }
    chunks
}

/// Filter a list of version strings with an npm-compatible semver range.
///
/// Returns `None` for non-range queries so callers can fall back to mise's
/// existing fuzzy matching for aliases and non-semver tools.
pub fn npm_semver_range_filter(versions: &[String], query: &str) -> Option<Vec<String>> {
    let query = query.trim();
    if !is_npm_semver_range_query(query) {
        return None;
    }
    let range = Range::parse(query).ok()?;

    Some(
        versions
            .iter()
            .filter(|v| {
                let version = v.as_str();
                NodeVersion::parse(version)
                    .or_else(|_| NodeVersion::parse(version.trim_start_matches(['v', 'V'])))
                    .is_ok_and(|version| range.satisfies(&version))
            })
            .cloned()
            .collect(),
    )
}

pub fn is_npm_semver_range_query(query: &str) -> bool {
    if query.is_empty() || query.eq_ignore_ascii_case("latest") {
        return false;
    }
    if query == "*" || query.eq_ignore_ascii_case("x") {
        return true;
    }
    if query.contains("||") || query.contains(" - ") {
        return true;
    }
    if matches!(
        query.as_bytes().first().copied(),
        Some(b'<' | b'>' | b'=' | b'^' | b'~')
    ) || query.contains('<')
        || query.contains('>')
    {
        return true;
    }
    if query.split_whitespace().count() > 1 {
        return true;
    }
    query.split('.').any(|part| matches!(part, "*" | "x" | "X"))
}

#[cfg(test)]
mod tests {
    use super::{chunkify_version, npm_semver_range_filter, split_version_prefix};

    #[test]
    fn test_split_version_prefix() {
        assert_eq!(split_version_prefix("latest"), ("".into(), "latest".into()));
        assert_eq!(split_version_prefix("v1.2.3"), ("v".into(), "1.2.3".into()));
        assert_eq!(
            split_version_prefix("mountpoint-s3-v1.2.3-5_beta.5"),
            ("mountpoint-s3-v".into(), "1.2.3-5_beta.5".into())
        );
        assert_eq!(
            split_version_prefix("cli/1.2.3"),
            ("cli/".into(), "1.2.3".into())
        );
        assert_eq!(
            split_version_prefix("temurin-17.0.7+7"),
            ("temurin-".into(), "17.0.7+7".into())
        );
        assert_eq!(split_version_prefix("1.2"), ("".into(), "1.2".into()));
        assert_eq!(
            split_version_prefix("2:1.2.1"),
            ("".into(), "2:1.2.1".into())
        );
        assert_eq!(
            split_version_prefix("2025-05-17"),
            ("".into(), "2025-05-17".into())
        );
    }

    #[test]
    fn test_chunkify_version() {
        assert_eq!(chunkify_version("1.2-3a4"), vec!["1", ".2", "-3a4"]);
        assert_eq!(chunkify_version("latest"), Vec::<String>::new());
        assert_eq!(chunkify_version("1.0.0"), vec!["1", ".0", ".0"]);
        assert_eq!(
            chunkify_version("2.3.4-beta"),
            vec!["2", ".3", ".4", "-beta"]
        );
    }

    #[test]
    fn test_npm_semver_range_filter_lower_bound() {
        let versions = ["25.5.0", "25.6.1", "25.8.2"].map(String::from).to_vec();

        assert_eq!(
            npm_semver_range_filter(&versions, ">=25.6.1").unwrap(),
            vec!["25.6.1".to_string(), "25.8.2".to_string()]
        );
        assert_eq!(
            npm_semver_range_filter(&versions, ">= 25.6.1").unwrap(),
            vec!["25.6.1".to_string(), "25.8.2".to_string()]
        );
    }

    #[test]
    fn test_npm_semver_range_filter_compound_bounds() {
        let versions = ["25.5.0", "25.6.1", "25.8.2", "26.0.0"]
            .map(String::from)
            .to_vec();

        assert_eq!(
            npm_semver_range_filter(&versions, ">=25.6.1 <26").unwrap(),
            vec!["25.6.1".to_string(), "25.8.2".to_string()]
        );
    }

    #[test]
    fn test_npm_semver_range_filter_caret() {
        let versions = ["20.0.0", "20.0.1", "20.1.0", "21.0.0"]
            .map(String::from)
            .to_vec();

        assert_eq!(
            npm_semver_range_filter(&versions, "^20.0.1").unwrap(),
            vec!["20.0.1".to_string(), "20.1.0".to_string()]
        );
    }

    #[test]
    fn test_npm_semver_range_filter_alternatives() {
        let versions = ["18.19.0", "20.0.0", "21.9.0", "22.0.0"]
            .map(String::from)
            .to_vec();

        assert_eq!(
            npm_semver_range_filter(&versions, ">=18 <20 || >=22").unwrap(),
            vec!["18.19.0".to_string(), "22.0.0".to_string()]
        );
    }

    #[test]
    fn test_npm_semver_range_filter_preserves_v_prefix() {
        let versions = ["v25.6.1", "v25.8.2"].map(String::from).to_vec();

        assert_eq!(
            npm_semver_range_filter(&versions, ">=25.8.0").unwrap(),
            vec!["v25.8.2".to_string()]
        );
    }

    #[test]
    fn test_npm_semver_range_filter_non_range_queries_fall_back() {
        assert_eq!(
            npm_semver_range_filter(&["1.0.0".to_string()], "latest"),
            None
        );
        assert_eq!(
            npm_semver_range_filter(&["1.0.0".to_string()], "temurin-"),
            None
        );
        assert_eq!(
            npm_semver_range_filter(&["1.0.0".to_string()], "1.0.0"),
            None
        );
    }
}
