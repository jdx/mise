use eyre::{Result, bail};
use regex::Regex;
use std::sync::LazyLock as Lazy;

static SSH_GIT_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^git::(?P<url>ssh://((?P<user>[^@]+)@)?(?P<host>[^/]+)/(?P<repo>.+)\.git)//(?P<path>[^?]+)(\?ref=(?P<ref>[^?&]+)(&.*)?)?$").unwrap()
});

static AZURE_DEVOPS_SSH_GIT_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^git::(?P<url>(ssh://((?P<user>[^@]+)@)?(?P<host>[^/]+)/(?P<org>[^/]+)/(?P<project>[^/]+)/_git/(?P<repo>[^/]+))|git@ssh.dev.azure.com:v3/(?P<cloud_org>[^/]+)/(?P<cloud_project>[^/]+)/(?P<cloud_repo>[^/]+))//(?P<path>[^?]+)(\?ref=(?P<ref>[^?&]+)(&.*)?)?$").unwrap()
});

static HTTPS_GIT_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^git::(?P<url>https?://(?P<host>[^/]+)/(?P<repo>.+)\.git)//(?P<path>[^?]+)(\?ref=(?P<ref>[^?&]+)(&.*)?)?$").unwrap()
});

static AZURE_DEVOPS_HTTPS_GIT_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^git::(?P<url>https?://(?P<host>[^/]+)/(?P<org>[^/]+)/(?P<project>[^/]+)/_git/(?P<repo>[^/]+))//(?P<path>[^?]+)(\?ref=(?P<ref>[^?&]+)(&.*)?)?$").unwrap()
});

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteGitSource {
    pub url: String,
    pub path: String,
    pub git_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteHttpSource {
    pub url: String,
}

pub struct RemoteSource;

impl RemoteSource {
    pub fn parse_git(file: &str) -> Option<RemoteGitSource> {
        Self::parse_git_ssh(file).or_else(|| Self::parse_git_https(file))
    }

    pub fn parse_git_ssh(file: &str) -> Option<RemoteGitSource> {
        parse_git_with(&SSH_GIT_REGEX, file)
            .or_else(|| parse_git_with(&AZURE_DEVOPS_SSH_GIT_REGEX, file))
    }

    pub fn parse_git_https(file: &str) -> Option<RemoteGitSource> {
        parse_git_with(&HTTPS_GIT_REGEX, file)
            .or_else(|| parse_git_with(&AZURE_DEVOPS_HTTPS_GIT_REGEX, file))
    }

    /// Splits a whole-repository URL into the repository and an optional git ref.
    ///
    /// Unlike [`Self::parse_git`], the source has no `//path` and is not limited
    /// to `.git` URLs: it accepts an optional `git::` prefix and a
    /// `?ref=<branch|tag|commit>` query parameter on any URL git can clone.
    /// Other query parameters stay on the URL.
    pub fn parse_git_repo(file: &str) -> Result<(String, Option<String>)> {
        let file = file.strip_prefix("git::").unwrap_or(file);
        let Some((url, query)) = file.split_once('?') else {
            return Ok((file.to_string(), None));
        };
        let mut git_ref = None;
        let mut rest = vec![];
        for pair in query.split('&') {
            match pair.strip_prefix("ref=") {
                Some(value) => git_ref = Some(value),
                None => rest.push(pair),
            }
        }
        let Some(git_ref) = git_ref else {
            return Ok((file.to_string(), None));
        };
        // HEAD is not a branch or tag: omit `ref` to use the default branch
        if git_ref.is_empty() || git_ref.starts_with('-') || git_ref == "HEAD" {
            bail!("invalid git ref {git_ref:?} in {file:?}");
        }
        let url = if rest.is_empty() {
            url.to_string()
        } else {
            format!("{url}?{}", rest.join("&"))
        };
        Ok((url, Some(git_ref.to_string())))
    }

    pub fn parse_http(file: &str) -> Option<RemoteHttpSource> {
        let url = url::Url::parse(file).ok()?;
        ((url.scheme() == "http" || url.scheme() == "https")
            && url.path().len() > 1
            && !url.path().ends_with('/'))
        .then(|| RemoteHttpSource {
            url: file.to_string(),
        })
    }
}

fn parse_git_with(regex: &Regex, file: &str) -> Option<RemoteGitSource> {
    let captures = regex.captures(file)?;
    let path = captures.name("path").unwrap().as_str();
    let mut components = path.split('/');
    let first = components.next()?;
    if path.contains('\\')
        || is_windows_drive_component(first)
        || std::iter::once(first)
            .chain(components)
            .any(|component| component.is_empty() || component == "." || component == "..")
    {
        return None;
    }
    Some(RemoteGitSource {
        url: captures.name("url").unwrap().as_str().to_string(),
        path: path.to_string(),
        git_ref: captures.name("ref").map(|m| m.as_str().to_string()),
    })
}

// Windows `Path::join` discards the base path when joining a drive-prefixed
// path even without a root (e.g. `C:outside`), so any leading `<letter>:`
// must be rejected, not just a bare `C:` component.
fn is_windows_drive_component(component: &str) -> bool {
    let bytes = component.as_bytes();
    bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_git_repo_ref_query() {
        let parse = |from| RemoteSource::parse_git_repo(from).unwrap();
        let with_ref = |url: &str, git_ref: &str| (url.to_string(), Some(git_ref.to_string()));
        let plain = |url: &str| (url.to_string(), None);

        assert_eq!(
            parse("https://github.com/o/r.git?ref=feature/x"),
            with_ref("https://github.com/o/r.git", "feature/x")
        );
        assert_eq!(
            parse("git::https://github.com/o/r.git?ref=v1"),
            with_ref("https://github.com/o/r.git", "v1")
        );
        assert_eq!(
            parse("git::ssh://git@host/o/r.git?ref=main"),
            with_ref("ssh://git@host/o/r.git", "main")
        );
        assert_eq!(
            parse("https://host/r.git?a=1&ref=v1&b=2"),
            with_ref("https://host/r.git?a=1&b=2", "v1")
        );
        assert_eq!(
            parse("git@github.com:o/r.git"),
            plain("git@github.com:o/r.git")
        );
        assert_eq!(
            parse("https://host/r.git?a=1"),
            plain("https://host/r.git?a=1")
        );
        assert!(RemoteSource::parse_git_repo("https://host/r.git?ref=").is_err());
        assert!(RemoteSource::parse_git_repo("https://host/r.git?ref=HEAD").is_err());
        assert!(RemoteSource::parse_git_repo("https://host/r.git?ref=--upload-pack=x").is_err());
    }

    #[test]
    fn parses_git_ssh_sources() {
        let source = RemoteSource::parse_git(
            "git::ssh://git@github.com/myorg/example.git//terraform/myfile?ref=master",
        )
        .unwrap();
        assert_eq!(source.url, "ssh://git@github.com/myorg/example.git");
        assert_eq!(source.path, "terraform/myfile");
        assert_eq!(source.git_ref, Some("master".to_string()));
    }

    #[test]
    fn parses_git_ssh_sources_without_user() {
        let source =
            RemoteSource::parse_git("git::ssh://github.com/myorg/example.git//terraform/myfile")
                .unwrap();
        assert_eq!(source.url, "ssh://github.com/myorg/example.git");
        assert_eq!(source.path, "terraform/myfile");
        assert_eq!(source.git_ref, None);
    }

    #[test]
    fn parses_git_https_sources() {
        let source = RemoteSource::parse_git(
            "git::https://git.acme.com:8080/myorg/example.git//terraform/myfile?ref=master",
        )
        .unwrap();
        assert_eq!(source.url, "https://git.acme.com:8080/myorg/example.git");
        assert_eq!(source.path, "terraform/myfile");
        assert_eq!(source.git_ref, Some("master".to_string()));
    }

    #[test]
    fn parses_git_ref_before_additional_query_params() {
        let source = RemoteSource::parse_git(
            "git::https://git.acme.com/myorg/example.git//terraform/myfile?ref=master&depth=1",
        )
        .unwrap();
        assert_eq!(source.git_ref, Some("master".to_string()));
    }

    #[test]
    fn rejects_git_sources_without_paths() {
        assert!(
            RemoteSource::parse_git("git::https://myserver.com/example.git?ref=master").is_none()
        );
        assert!(RemoteSource::parse_git("git::ssh://user@myserver.com/example.git").is_none());
    }

    #[test]
    fn rejects_git_sources_with_unsafe_paths() {
        assert!(
            RemoteSource::parse_git("git::https://myserver.com/example.git//../plugin").is_none()
        );
        assert!(
            RemoteSource::parse_git("git::https://myserver.com/example.git//plugin/../other")
                .is_none()
        );
        assert!(
            RemoteSource::parse_git("git::https://myserver.com/example.git//plugin//other")
                .is_none()
        );
        assert!(
            RemoteSource::parse_git("git::https://myserver.com/example.git//plugin/./other")
                .is_none()
        );
        assert!(
            RemoteSource::parse_git("git::https://myserver.com/example.git//..\\outside").is_none()
        );
        assert!(
            RemoteSource::parse_git("git::https://myserver.com/example.git//C:/outside").is_none()
        );
        assert!(
            RemoteSource::parse_git("git::https://myserver.com/example.git//C:\\outside").is_none()
        );
        assert!(
            RemoteSource::parse_git("git::https://myserver.com/example.git//C:outside").is_none()
        );
        assert!(
            RemoteSource::parse_git("git::https://myserver.com/example.git//C:dir/file").is_none()
        );
    }

    #[test]
    fn parses_azure_devops_git_ssh_sources() {
        let test_cases: Vec<(&str, &str, &str, Option<String>)> = vec![
            (
                "git::ssh://git@dev.azure/myorg/myproj/_git/example//terraform/myfile?ref=master",
                "ssh://git@dev.azure/myorg/myproj/_git/example",
                "terraform/myfile",
                Some("master".to_string()),
            ),
            (
                "git::git@ssh.dev.azure.com:v3/myorg/myproj/example//terraform/myfile?ref=master",
                "git@ssh.dev.azure.com:v3/myorg/myproj/example",
                "terraform/myfile",
                Some("master".to_string()),
            ),
        ];

        for (url, expected_url, expected_path, expected_git_ref) in test_cases {
            let source = RemoteSource::parse_git(url).unwrap();
            assert_eq!(source.url, expected_url);
            assert_eq!(source.path, expected_path);
            assert_eq!(source.git_ref, expected_git_ref);
        }
    }

    #[test]
    fn parses_azure_devops_git_ssh_sources_without_user() {
        let source = RemoteSource::parse_git(
            "git::ssh://dev.azure/myorg/myproj/_git/example//terraform/myfile",
        )
        .unwrap();
        assert_eq!(source.url, "ssh://dev.azure/myorg/myproj/_git/example");
        assert_eq!(source.path, "terraform/myfile");
        assert_eq!(source.git_ref, None);
    }

    #[test]
    fn parses_azure_devops_git_https_sources() {
        let source = RemoteSource::parse_git(
            "git::https://dev.azure:8080/myorg/myproj/_git/example//terraform/myfile?ref=master",
        )
        .unwrap();
        assert_eq!(
            source.url,
            "https://dev.azure:8080/myorg/myproj/_git/example"
        );
        assert_eq!(source.path, "terraform/myfile");
        assert_eq!(source.git_ref, Some("master".to_string()));
    }

    #[test]
    fn parses_azure_devops_git_ref_before_additional_query_params() {
        let source = RemoteSource::parse_git(
            "git::https://dev.azure/myorg/myproj/_git/example//terraform/myfile?ref=master&depth=1",
        )
        .unwrap();
        assert_eq!(source.git_ref, Some("master".to_string()));
    }

    #[test]
    fn rejects_azure_devops_git_sources_without_paths() {
        assert!(
            RemoteSource::parse_git("git::https://dev.azure/myorg/myproj/_git/example?ref=master")
                .is_none()
        );
        assert!(
            RemoteSource::parse_git("git::ssh://user@dev.azure/myorg/myproj/_git/example")
                .is_none()
        );
        assert!(
            RemoteSource::parse_git("git::git@ssh.dev.azure.com:v3/myorg/myproj/example").is_none()
        );
    }

    #[test]
    fn rejects_azure_devops_git_sources_with_unsafe_paths() {
        assert!(
            RemoteSource::parse_git("git::https://dev.azure/myorg/myproj/_git/example//../plugin")
                .is_none()
        );
        assert!(
            RemoteSource::parse_git(
                "git::https://dev.azure/myorg/myproj/_git/example//plugin/../other"
            )
            .is_none()
        );
        assert!(
            RemoteSource::parse_git(
                "git::https://dev.azure/myorg/myproj/_git/example//plugin//other"
            )
            .is_none()
        );
        assert!(
            RemoteSource::parse_git(
                "git::https://dev.azure/myorg/myproj/_git/example//plugin/./other"
            )
            .is_none()
        );
    }

    #[test]
    fn parses_http_sources() {
        assert!(RemoteSource::parse_http("http://myhost.com/test.txt").is_some());
        assert!(RemoteSource::parse_http("https://myhost.com/test.txt?query=1").is_some());
    }

    #[test]
    fn rejects_http_directories() {
        assert!(RemoteSource::parse_http("https://myhost.com/js/").is_none());
        assert!(RemoteSource::parse_http("https://myhost.com").is_none());
    }
}
