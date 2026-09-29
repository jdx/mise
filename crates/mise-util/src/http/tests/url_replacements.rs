use super::*;

#[test]
fn test_simple_string_replacement() {
    let mut replacements = IndexMap::new();
    replacements.insert("github.com".to_string(), "my-proxy.com".to_string());

    with_test_settings(replacements, || {
        let mut url = Url::parse("https://github.com/owner/repo").unwrap();
        apply_url_replacements(&mut url);
        assert_eq!(url.as_str(), "https://my-proxy.com/owner/repo");
    });
}

#[test]
fn test_full_url_string_replacement() {
    let mut replacements = IndexMap::new();
    replacements.insert(
        "https://github.com".to_string(),
        "https://my-proxy.com/artifactory/github-remote".to_string(),
    );

    with_test_settings(replacements, || {
        let mut url = Url::parse("https://github.com/owner/repo").unwrap();
        apply_url_replacements(&mut url);
        assert_eq!(
            url.as_str(),
            "https://my-proxy.com/artifactory/github-remote/owner/repo"
        );
    });
}

#[test]
fn test_protocol_specific_replacement() {
    let mut replacements = IndexMap::new();
    replacements.insert(
        "https://github.com".to_string(),
        "https://secure-proxy.com".to_string(),
    );

    with_test_settings(replacements.clone(), || {
        // HTTPS gets replaced
        let mut url1 = Url::parse("https://github.com/owner/repo").unwrap();
        apply_url_replacements(&mut url1);
        assert_eq!(url1.as_str(), "https://secure-proxy.com/owner/repo");
    });

    with_test_settings(replacements, || {
        // HTTP does not get replaced (no match)
        let mut url2 = Url::parse("http://github.com/owner/repo").unwrap();
        apply_url_replacements(&mut url2);
        assert_eq!(url2.as_str(), "http://github.com/owner/repo");
    });
}

#[test]
fn test_regex_replacement() {
    let mut replacements = IndexMap::new();
    replacements.insert(
        r"regex:https://github\.com".to_string(),
        "https://my-proxy.com".to_string(),
    );

    with_test_settings(replacements, || {
        let mut url = Url::parse("https://github.com/owner/repo").unwrap();
        apply_url_replacements(&mut url);
        assert_eq!(url.as_str(), "https://my-proxy.com/owner/repo");
    });
}

#[test]
fn test_regex_with_capture_groups() {
    let mut replacements = IndexMap::new();
    replacements.insert(
        r"regex:https://github\.com/([^/]+)/([^/]+)".to_string(),
        "https://my-proxy.com/mirror/$1/$2".to_string(),
    );

    with_test_settings(replacements, || {
        let mut url = Url::parse("https://github.com/owner/repo/releases").unwrap();
        apply_url_replacements(&mut url);
        assert_eq!(
            url.as_str(),
            "https://my-proxy.com/mirror/owner/repo/releases"
        );
    });
}

#[test]
fn test_regex_invalid_replacement_url() {
    let mut replacements = IndexMap::new();
    replacements.insert(
        r"regex:https://github\.com/([^/]+)".to_string(),
        "not-a-valid-url".to_string(),
    );

    with_test_settings(replacements, || {
        // Invalid result URL should be ignored, original URL unchanged
        let mut url = Url::parse("https://github.com/owner/repo").unwrap();
        let original = url.clone();
        apply_url_replacements(&mut url);
        assert_eq!(url.as_str(), original.as_str());
    });
}

#[test]
fn test_multiple_replacements_first_match_wins() {
    let mut replacements = IndexMap::new();
    replacements.insert("github.com".to_string(), "first-proxy.com".to_string());
    replacements.insert("github".to_string(), "second-proxy.com".to_string());

    with_test_settings(replacements, || {
        let mut url = Url::parse("https://github.com/owner/repo").unwrap();
        apply_url_replacements(&mut url);
        // First replacement should win
        assert_eq!(url.as_str(), "https://first-proxy.com/owner/repo");
    });
}

#[test]
fn test_no_replacements_configured() {
    let replacements = IndexMap::new(); // Empty

    with_test_settings(replacements, || {
        let mut url = Url::parse("https://github.com/owner/repo").unwrap();
        let original = url.clone();
        apply_url_replacements(&mut url);
        assert_eq!(url.as_str(), original.as_str());
    });
}

#[test]
fn test_regex_complex_patterns() {
    let mut replacements = IndexMap::new();
    // Convert GitHub releases to JFrog Artifactory
    replacements.insert(
        r"regex:https://github\.com/([^/]+)/([^/]+)/releases/download/([^/]+)/(.+)".to_string(),
        "https://artifactory.company.com/artifactory/github-releases/$1/$2/$3/$4".to_string(),
    );

    with_test_settings(replacements, || {
        let mut url =
            Url::parse("https://github.com/owner/repo/releases/download/v1.0.0/file.tar.gz")
                .unwrap();
        apply_url_replacements(&mut url);
        assert_eq!(
            url.as_str(),
            "https://artifactory.company.com/artifactory/github-releases/owner/repo/v1.0.0/file.tar.gz"
        );
    });
}

#[test]
fn test_replacement_affects_full_url_not_just_hostname() {
    // Test that replacement works on the full URL string, not just hostname
    let mut replacements = IndexMap::new();
    replacements.insert(
        "github.com/owner".to_string(),
        "proxy.com/mirror".to_string(),
    );

    with_test_settings(replacements, || {
        let mut url = Url::parse("https://github.com/owner/repo").unwrap();
        apply_url_replacements(&mut url);
        // This demonstrates that replacement happens on full URL, not just hostname
        assert_eq!(url.as_str(), "https://proxy.com/mirror/repo");
    });
}

#[test]
fn test_path_replacement_example() {
    // Test replacing part of the path, proving it's not hostname-only
    let mut replacements = IndexMap::new();
    replacements.insert("/releases/download/".to_string(), "/artifacts/".to_string());

    with_test_settings(replacements, || {
        let mut url =
            Url::parse("https://github.com/owner/repo/releases/download/v1.0.0/file.tar.gz")
                .unwrap();
        apply_url_replacements(&mut url);
        // Path component was replaced, proving it's full URL replacement
        assert_eq!(
            url.as_str(),
            "https://github.com/owner/repo/artifacts/v1.0.0/file.tar.gz"
        );
    });
}

#[test]
fn test_documentation_examples() {
    // Test the examples from the documentation to ensure they work correctly

    // Example 1: Simple hostname replacement
    let mut replacements = IndexMap::new();
    replacements.insert("github.com".to_string(), "myregistry.net".to_string());

    with_test_settings(replacements, || {
        let mut url = Url::parse("https://github.com/user/repo").unwrap();
        apply_url_replacements(&mut url);
        assert_eq!(url.as_str(), "https://myregistry.net/user/repo");
    });

    // Example 2: Protocol + hostname replacement
    let mut replacements2 = IndexMap::new();
    replacements2.insert(
        "https://github.com".to_string(),
        "https://proxy.corp.com/github-mirror".to_string(),
    );

    with_test_settings(replacements2, || {
        let mut url = Url::parse("https://github.com/user/repo").unwrap();
        apply_url_replacements(&mut url);
        assert_eq!(
            url.as_str(),
            "https://proxy.corp.com/github-mirror/user/repo"
        );
    });

    // Example 3: Domain + path replacement
    let mut replacements3 = IndexMap::new();
    replacements3.insert(
        "github.com/releases/download/".to_string(),
        "cdn.example.com/artifacts/".to_string(),
    );

    with_test_settings(replacements3, || {
        let mut url =
            Url::parse("https://github.com/releases/download/v1.0.0/file.tar.gz").unwrap();
        apply_url_replacements(&mut url);
        assert_eq!(
            url.as_str(),
            "https://cdn.example.com/artifacts/v1.0.0/file.tar.gz"
        );
    });
}

#[test]
fn test_no_settings_configured() {
    // Test the real apply_url_replacements function with no settings override
    let _guard = crate::testing::lock_ignoring_poison(&crate::testing::SETTINGS_LOCK);
    crate::testing::reset_settings(None);

    let mut url = Url::parse("https://github.com/owner/repo").unwrap();
    let original = url.clone();

    // This should not crash and should leave URL unchanged
    apply_url_replacements(&mut url);
    assert_eq!(url.as_str(), original.as_str());
}
