use crate::cli::args::BackendArg;
use crate::config::SETTINGS;
use crate::http::HTTP_FETCH;
use crate::registry::REGISTRY;
use crate::{backend, http, registry};
use url::Url;

pub fn list_versions(ba: &BackendArg) -> eyre::Result<Option<Vec<String>>> {
    if !SETTINGS.use_versions_host
        || ba.short.contains(':')
        || !REGISTRY.contains_key(ba.short.as_str())
    {
        return Ok(None);
    }
    // ensure that we're using a default shorthand plugin
    if let Some(plugin) = backend::get(ba).plugin() {
        if let Ok(Some(remote_url)) = plugin.get_remote_url() {
            let normalized_remote = normalize_remote(&remote_url).unwrap_or("INVALID_URL".into());
            let shorthand_remote = REGISTRY
                .get(plugin.name())
                .map(|s| registry::full_to_url(&s[0]))
                .unwrap_or_default();
            if normalized_remote != normalize_remote(&shorthand_remote).unwrap_or_default() {
                trace!(
                    "Skipping versions host check for {} because it has a non-default remote",
                    ba.short
                );
                return Ok(None);
            }
        }
    }
    let response = match SETTINGS.paranoid {
        true => HTTP_FETCH.get_text(format!("https://mise-versions.jdx.dev/{}", &ba.short)),
        false => HTTP_FETCH.get_text(format!("http://mise-versions.jdx.dev/{}", &ba.short)),
    };
    let versions =
        // using http is not a security concern and enabling tls makes mise significantly slower
        match response {
            Err(err) if http::error_code(&err) == Some(404) => return Ok(None),
            res => res?,
        };
    let versions = versions
        .lines()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .collect::<Vec<String>>();
    trace!(
        "got {} {} versions from versions host",
        versions.len(),
        &ba.short
    );
    match versions.is_empty() {
        true => Ok(None),
        false => Ok(Some(versions)),
    }
}

fn normalize_remote(remote: &str) -> eyre::Result<String> {
    let url = Url::parse(remote)?;
    let host = url.host_str().unwrap();
    let path = url.path().trim_end_matches(".git");
    Ok(format!("{host}{path}"))
}
