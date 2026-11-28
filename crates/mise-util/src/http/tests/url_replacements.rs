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

fn rewritten(replacements: &IndexMap<String, String>, url: &str) -> String {
    with_test_settings(replacements.clone(), || {
        let mut url = Url::parse(url).unwrap();
        apply_url_replacements(&mut url);
        url.to_string()
    })
}

#[test]
fn test_replacement_credentials_only_reach_the_replacement_host() {
    // A rule skipped for one URL leaves that URL as it was.
    let unchanged = |replacements: &IndexMap<String, String>, url: &str| {
        assert_eq!(rewritten(replacements, url), url);
    };

    let mut plain = IndexMap::new();
    plain.insert(
        "https://github.com/".to_string(),
        "https://user:secret@mirror.example.com/github/".to_string(),
    );
    assert_eq!(
        rewritten(&plain, "https://github.com/owner/repo"),
        "https://user:secret@mirror.example.com/github/owner/repo"
    );
    // The key in another host's query or path does not hand that host the login.
    unchanged(&plain, "http://evil.example/?u=https://github.com/");
    unchanged(&plain, "http://evil.example/x/https://github.com/tool");

    // Nor does a key without a trailing slash, in front of a longer host name.
    let mut prefix = IndexMap::new();
    prefix.insert(
        "https://github.com".to_string(),
        "https://user:secret@mirror.example.com".to_string(),
    );
    assert_eq!(
        rewritten(&prefix, "https://github.com/owner/repo"),
        "https://user:secret@mirror.example.com/owner/repo"
    );
    unchanged(&prefix, "https://github.com.evil.example/owner/repo");
    unchanged(&prefix, "https://evil.example/?u=https://github.com");

    let mut bare = IndexMap::new();
    bare.insert(
        "github.com".to_string(),
        "user:secret@mirror.example.com".to_string(),
    );
    assert_eq!(
        rewritten(&bare, "https://github.com/owner/repo"),
        "https://user:secret@mirror.example.com/owner/repo"
    );
    unchanged(&bare, "https://evil.example/github.com/owner/repo");

    let mut regex = IndexMap::new();
    regex.insert(
        r"regex:https://([^./]+)\.cdn\.example/".to_string(),
        "https://user:secret@$1.mirror.example.com/".to_string(),
    );
    assert_eq!(
        rewritten(&regex, "https://eu.cdn.example/tool.tar.gz"),
        "https://user:secret@eu.mirror.example.com/tool.tar.gz"
    );
    unchanged(&regex, "http://evil.example/?u=https://eu.cdn.example/");

    // A capture in front of the credentials cannot move them out of place.
    let mut leading_capture = IndexMap::new();
    leading_capture.insert(
        r"regex:^(.*)github\.com".to_string(),
        "${1}user:secret@mirror.example.com".to_string(),
    );
    assert_eq!(
        rewritten(&leading_capture, "https://github.com/owner/repo"),
        "https://user:secret@mirror.example.com/owner/repo"
    );
    unchanged(&leading_capture, "http://evil.example/?github.com");

    // A skipped rule does not stop a later one, and rules without credentials
    // still match anywhere.
    plain.insert("evil.example".to_string(), "other.example".to_string());
    assert_eq!(
        rewritten(&plain, "http://evil.example/?u=https://github.com/"),
        "http://other.example/?u=https://github.com/"
    );
    let mut no_login = IndexMap::new();
    no_login.insert(
        "https://github.com/".to_string(),
        "https://mirror.example.com/scope@1/".to_string(),
    );
    no_login.insert("@old/".to_string(), "@new/".to_string());
    assert_eq!(
        rewritten(&no_login, "https://proxy.example/?u=https://github.com/"),
        "https://proxy.example/?u=https://mirror.example.com/scope@1/"
    );
    assert_eq!(
        rewritten(&no_login, "https://registry.example/@old/package"),
        "https://registry.example/@new/package"
    );
}

/// The URL parser reads the server of an `http(s)` URL after any run of `/` and
/// `\` that follows the colon, and drops tabs, line breaks and leading spaces,
/// so every such spelling of a replacement with a login names the same server,
/// and the login reaches only that server.
#[test]
fn test_replacement_credentials_are_found_in_every_spelling_of_the_server() {
    for replacement in [
        "https://user:secret@mirror.example.com/",
        "https:////user:secret@mirror.example.com/",
        "https:/user:secret@mirror.example.com/",
        r"https:\\user:secret@mirror.example.com/",
        r"https:/\user:secret@mirror.example.com/",
        "HTTPS:///user:secret@mirror.example.com/",
        "https:/\t/user:secret@mirror.example.com/",
        "https:\n//user:secret@mirror.example.com/",
        " https://user:sec\rret@mirror.example.com/",
    ] {
        let mut rules = IndexMap::new();
        rules.insert("https://github.com/".to_string(), replacement.to_string());
        assert_eq!(
            rewritten(&rules, "https://github.com/owner/repo"),
            "https://user:secret@mirror.example.com/owner/repo",
            "{replacement}"
        );
        for elsewhere in [
            "https://evil.example/?u=https://github.com/",
            "https://evil.example/x/https://github.com/tool",
        ] {
            assert_eq!(rewritten(&rules, elsewhere), elsewhere, "{replacement}");
        }
    }
}

/// Without a special scheme, only `//` introduces an authority, so a replacement
/// with more slashes or a backslash there names no server for its login, and
/// would put it in the query or path of whatever URL the key matched. Such a
/// rule is refused for every URL; other rules still apply.
#[test]
fn test_replacements_that_name_no_server_are_refused() {
    for replacement in [
        "///user:secret@mirror.example.com/",
        "////user:secret@mirror.example.com/",
        r"\\user:secret@mirror.example.com/",
        r"/\user:secret@mirror.example.com/",
        r"\user:secret@mirror.example.com/",
        "/\t//user:secret@mirror.example.com/",
        "git:///user:secret@mirror.example.com/",
    ] {
        let mut rules = IndexMap::new();
        rules.insert("github.com".to_string(), replacement.to_string());
        rules.insert("other.example".to_string(), "kept.example".to_string());
        for url in [
            "https://evil.example/?u=github.com",
            "https://github.com/owner/repo",
        ] {
            assert_eq!(rewritten(&rules, url), url, "{replacement:?}");
        }
        assert_eq!(
            rewritten(&rules, "https://other.example/?u=github.com"),
            "https://kept.example/?u=github.com",
            "{replacement:?}"
        );
    }

    // A path, a `//host` and a host with a port are still replacements.
    for (key, replacement, url, expected) in [
        (
            "/old/",
            "/new/",
            "https://files.example.com/old/a",
            "https://files.example.com/new/a",
        ),
        (
            "//github.com/",
            "//mirror.example.com/",
            "https://github.com/a",
            "https://mirror.example.com/a",
        ),
        (
            "github.com",
            "mirror.example.com:8443",
            "https://github.com/a",
            "https://mirror.example.com:8443/a",
        ),
    ] {
        let mut rules = IndexMap::new();
        rules.insert(key.to_string(), replacement.to_string());
        assert_eq!(rewritten(&rules, url), expected, "{replacement}");
    }
}

/// A rule that writes a login for its mirror keeps it, even when the URL had
/// the same login for its own server.
#[test]
fn test_a_rule_login_survives_credential_scoping() {
    let mut rules = IndexMap::new();
    rules.insert(
        "https://github.com/".to_string(),
        "https://user:pw@mirror.example.com/".to_string(),
    );
    with_test_settings(rules, || {
        let original = Url::parse("https://user:pw@github.com/file").unwrap();
        let mut url = original.clone();
        apply_url_replacements(&mut url);
        let mut headers = HeaderMap::new();
        headers.insert(
            reqwest::header::AUTHORIZATION,
            reqwest::header::HeaderValue::from_static("Bearer github-token"),
        );
        clear_cross_host_credentials(&mut headers, &original, &url);
        assert_eq!(url.as_str(), "https://user:pw@mirror.example.com/file");
        assert!(headers.is_empty());
    });
}

#[test]
fn test_rules_never_move_a_url_login_into_the_rewritten_url() {
    // The documented subdomain-to-path rule: `[^./]+` would also match
    // `ci:CDN-PASSWORD@eu` if the rule saw the login.
    let mut subdomain = IndexMap::new();
    subdomain.insert(
        r"regex:^https://([^./]+)\.cdn\.example\.com/(.+)".to_string(),
        "http://127.0.0.1:18921/$1/$2".to_string(),
    );
    assert_eq!(
        rewritten(
            &subdomain,
            "https://ci:CDN-PASSWORD@eu.cdn.example.com/tool-1.0"
        ),
        "http://127.0.0.1:18921/eu/tool-1.0"
    );

    // A key with the scheme and host matches a URL with a login, and the login
    // stays with its host.
    let mut host = IndexMap::new();
    host.insert(
        "https://files.example.com/".to_string(),
        "https://mirror.example.com/files/".to_string(),
    );
    assert_eq!(
        rewritten(&host, "https://u:p@files.example.com/a"),
        "https://mirror.example.com/files/a"
    );
    let mut path = IndexMap::new();
    path.insert("/old/".to_string(), "/new/".to_string());
    assert_eq!(
        rewritten(&path, "https://u:p@files.example.com/old/a"),
        "https://u:p@files.example.com/new/a"
    );
    // Another port on the same host is another server.
    let mut port = IndexMap::new();
    port.insert(
        "files.example.com/".to_string(),
        "files.example.com:8443/".to_string(),
    );
    assert_eq!(
        rewritten(&port, "https://u:p@files.example.com/a"),
        "https://files.example.com:8443/a"
    );
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

/// A tab or line break in a regex rule's server ends a capture name as it does
/// in the rewrite, so the login of `https://ci:secret@$1<TAB>mirror.example/`
/// is checked against the server the rewrite reaches, and the rule applies.
#[test]
fn test_regex_replacement_credentials_keep_capture_boundaries_at_tabs() {
    for replacement in [
        "https://ci:secret@$1\tmirror.example/$2",
        "https://ci:secret@$1\nmirror.example/$2",
        " https://ci:secret@$1\rmirror.example\t/$2",
    ] {
        let mut replacements = IndexMap::new();
        replacements.insert(
            r"regex:^https://([^./]+)\.cdn\.example\.com/(.+)".to_string(),
            replacement.to_string(),
        );
        assert_eq!(
            rewritten(&replacements, "https://eu.cdn.example.com/tool-1.0"),
            "https://ci:secret@eumirror.example/tool-1.0",
            "{replacement:?}"
        );
    }
}
