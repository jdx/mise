//! Credentials for a package index whose URL names a user but no password,
//! read from a `keyring` executable the way uv's `subprocess` provider does.
use crate::cmd::cmd;
use crate::config::Settings;
use base64::{Engine, prelude::BASE64_STANDARD};
use eyre::Result;
use reqwest::Url;
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};

/// The URL to request and the headers to send with it. A URL with a username
/// and no password gets a Basic `Authorization` header from the keyring, and its
/// userinfo is dropped so the empty password is not sent as well. The keyring is
/// asked about the index URL uv installs from (`registry` with its `{}` removed),
/// not the package page, because that is the URL its entry is stored under.
pub(super) async fn index_request(registry: &str, url: &str) -> Result<(Url, HeaderMap)> {
    let mut url = Url::parse(url)?;
    let mut headers = crate::http::with_host_auth(&url, &HeaderMap::new())?;
    if Settings::get().pypi.keyring_provider.as_deref() != Some("subprocess")
        || url.username().is_empty()
        || url.password().is_some()
    {
        return Ok((url, headers));
    }
    let username = percent_decode(url.username());
    let service = super::lock::uv_index_url(registry)?;
    let name = username.clone();
    let password = tokio::task::spawn_blocking(move || keyring_get(&service, &name)).await?;
    match password {
        Some(password) => {
            headers.insert(AUTHORIZATION, basic_auth(&username, &password)?);
            let _ = url.set_username("");
        }
        None => debug!("keyring has no password for {username} at the package index"),
    }
    Ok((url, headers))
}

fn keyring_get(url: &str, username: &str) -> Option<String> {
    let output = match cmd("keyring", ["get", url, username]).stderr_null().read() {
        Ok(output) => output,
        Err(err) => {
            debug!("keyring get failed: {err}");
            return None;
        }
    };
    let password = output.trim_end_matches(['\r', '\n']);
    (!password.is_empty()).then(|| password.to_string())
}

fn basic_auth(username: &str, password: &str) -> Result<HeaderValue> {
    let mut value = HeaderValue::from_str(&format!(
        "Basic {}",
        BASE64_STANDARD.encode(format!("{username}:{password}"))
    ))?;
    value.set_sensitive(true);
    Ok(value)
}

fn percent_decode(value: &str) -> String {
    urlencoding::decode(value).map_or_else(|_| value.to_string(), |decoded| decoded.into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_auth_encodes_user_and_password() {
        let value = basic_auth("user", "s3cret").unwrap();
        assert_eq!(value.to_str().unwrap(), "Basic dXNlcjpzM2NyZXQ=");
        assert!(value.is_sensitive());
    }

    #[test]
    fn percent_decode_unescapes_userinfo() {
        assert_eq!(percent_decode("me%40corp"), "me@corp");
    }
}
