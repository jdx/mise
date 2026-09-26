//! Validation for a registry entry's `url`, shared by the build-time registry and
//! the floating registry so both reject what `schema/mise-registry-tool.json` rejects.

/// Whether `url` is a project homepage or repository: `http(s)://` followed by a
/// non-empty value with no whitespace and no `{`/`}`, which rules out backend
/// download templates such as `https://example.com/{{ version }}`.
pub(crate) fn is_project_url(url: &str) -> bool {
    url.strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .is_some_and(|rest| {
            !rest.is_empty() && !rest.contains(|c: char| c.is_whitespace() || c == '{' || c == '}')
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_http_and_https_urls() {
        assert!(is_project_url("https://dart.dev"));
        assert!(is_project_url("http://example.com/tool"));
    }

    #[test]
    fn rejects_templates_whitespace_and_bare_schemes() {
        for url in [
            "",
            "https://",
            "ftp://example.com",
            "example.com",
            "https://example.com/{{ version }}",
            "https://example.com/{",
            "https://example.com/a b",
            "https://example.com\n",
        ] {
            assert!(!is_project_url(url), "{url:?} should be rejected");
        }
    }
}
