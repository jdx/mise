use regex::Regex;
use std::sync::LazyLock as Lazy;

static SSH_GIT_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^git::(?P<url>ssh://((?P<user>[^@]+)@)?(?P<host>[^/]+)/(?P<repo>.+)\.git)//(?P<path>[^?]+)(\?ref=(?P<ref>[^?&]+)(&.*)?)?$").unwrap()
});

static HTTPS_GIT_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^git::(?P<url>https?://(?P<host>[^/]+)/(?P<repo>.+)\.git)//(?P<path>[^?]+)(\?ref=(?P<ref>[^?&]+)(&.*)?)?$").unwrap()
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

    pub(crate) fn parse_git_ssh(file: &str) -> Option<RemoteGitSource> {
        parse_git_with(&SSH_GIT_REGEX, file)
    }

    pub(crate) fn parse_git_https(file: &str) -> Option<RemoteGitSource> {
        parse_git_with(&HTTPS_GIT_REGEX, file)
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
    if path
        .split('/')
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

#[cfg(test)]
mod tests {
    use super::*;

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
