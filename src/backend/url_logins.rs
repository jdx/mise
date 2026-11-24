//! Tool keys that name a URL to fetch, and the logins such a fetch can send.
//!
//! git sends the user's credential-helper and netrc logins to the host of an
//! `http(s)` remote, and mise's HTTP client sends netrc logins to the host of a
//! URL it downloads. A tool key that is such a URL lets the config holding it
//! choose where those logins go, so the config needs trust before mise fetches
//! it, and safe mode keeps project config from choosing that host.

use eyre::Result;

use crate::config::{Settings, SettingsExt};

/// Hosts whose logins only ever reach the forge itself; a config naming a
/// repository there chooses the repository, not who receives the login.
const PUBLIC_FORGE_HOSTS: [&str; 3] = ["github.com", "gitlab.com", "bitbucket.org"];

/// Whether fetching `url` can send the user's logins to a host that `url`
/// chooses: an `http` or `https` URL of a host other than a public forge.
pub(crate) fn url_takes_user_logins(url: &str) -> bool {
    url::Url::parse(url).is_ok_and(|url| {
        matches!(url.scheme(), "http" | "https")
            && url
                .host_str()
                .is_some_and(|host| !PUBLIC_FORGE_HOSTS.contains(&host))
    })
}

/// The URL a tool key names when fetching it can send the user's logins to a
/// host the key chooses: an `npm:` tarball, or an `npm:` or `pipx:` git source,
/// served over `http(s)` by a host other than github.com, gitlab.com or
/// bitbucket.org.
pub fn tool_key_login_url(tool: &str) -> Option<String> {
    let tool = tool.trim();
    let url = if let Some(name) = tool.strip_prefix("npm:") {
        super::npm::fetched_url(name)?
    } else {
        let name = tool
            .strip_prefix("pipx:")
            .or_else(|| tool.strip_prefix("pypi:"))?;
        super::pipx::fetched_url(name)?
    };
    url_takes_user_logins(&url).then_some(url)
}

/// In safe mode, refuses to run git against `remote` when project config named
/// it for a tool that global or system config does not declare. git sends the
/// user's credential-helper and netrc logins with the request, and mise cannot
/// withhold them from git as it does from its own HTTP client.
pub(crate) fn ensure_safe_mode_git_remote(remote: &str) -> Result<()> {
    if !Settings::safe_mode() || !url_takes_user_logins(remote) {
        return Ok(());
    }
    let Ok(url) = url::Url::parse(remote) else {
        return Ok(());
    };
    if mise_util::untrusted_hosts::netrc_denied(&url) {
        Settings::ensure_not_safe(&format!(
            "fetching a git source from {}, which project config names,",
            url.host_str().unwrap_or_default()
        ))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_tool_keys_off_public_forges_name_a_login_url() {
        for (tool, url) in [
            (
                "npm:git+https://git.example.test/o/r.git",
                "https://git.example.test/o/r.git",
            ),
            (
                "npm:git+https://git.example.test/o/r.git#v1",
                "https://git.example.test/o/r.git",
            ),
            (
                "npm:https://pkg.example.test/tool.tgz",
                "https://pkg.example.test/tool.tgz",
            ),
            (
                "pipx:git+https://git.example.test/o/r.git",
                "https://git.example.test/o/r.git",
            ),
            (
                "pypi:git+https://git.example.test/o/r#subdirectory=cli",
                "https://git.example.test/o/r",
            ),
        ] {
            assert_eq!(tool_key_login_url(tool).as_deref(), Some(url), "{tool}");
        }
        for tool in [
            "npm:prettier",
            "npm:@biomejs/biome",
            "npm:github:o/r",
            "npm:git+https://github.com/o/r.git",
            "npm:git+ssh://git@git.example.test/o/r.git",
            "npm:https://github.com/o/r/archive/v1.tar.gz",
            "pipx:black",
            "pipx:psf/black",
            "pipx:git+https://github.com/psf/black.git",
            "pipx:git+file:///tmp/project",
            "node",
        ] {
            assert_eq!(tool_key_login_url(tool), None, "{tool}");
        }
    }
}
