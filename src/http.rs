use std::io::Write;
use std::path::Path;
use std::time::Duration;

use eyre::{Report, Result, bail, ensure};
use reqwest::header::{HeaderMap, HeaderValue};
use reqwest::{ClientBuilder, IntoUrl, Response};
use std::sync::LazyLock as Lazy;
use url::Url;

use crate::cli::version;
use crate::config::Settings;
use crate::file::display_path;
use crate::ui::progress_report::SingleReport;
use crate::ui::time::format_duration;
use crate::{env, file};

#[cfg(not(test))]
pub static HTTP_VERSION_CHECK: Lazy<Client> =
    Lazy::new(|| Client::new(Duration::from_secs(3), ClientKind::VersionCheck).unwrap());

pub static HTTP: Lazy<Client> =
    Lazy::new(|| Client::new(Settings::get().http_timeout(), ClientKind::Http).unwrap());

pub static HTTP_FETCH: Lazy<Client> = Lazy::new(|| {
    Client::new(
        Settings::get().fetch_remote_versions_timeout(),
        ClientKind::Fetch,
    )
    .unwrap()
});

#[derive(Debug)]
pub struct Client {
    reqwest: reqwest::Client,
    timeout: Duration,
    kind: ClientKind,
}

#[derive(Debug, Clone, Copy)]
enum ClientKind {
    Http,
    Fetch,
    VersionCheck,
}

impl Client {
    fn new(timeout: Duration, kind: ClientKind) -> Result<Self> {
        Ok(Self {
            reqwest: Self::_new()
                .read_timeout(timeout)
                .connect_timeout(timeout)
                .build()?,
            timeout,
            kind,
        })
    }

    fn _new() -> ClientBuilder {
        let v = &*version::VERSION;
        let shell = env::MISE_SHELL.map(|s| s.to_string()).unwrap_or_default();
        ClientBuilder::new()
            .user_agent(format!("mise/{v} {shell}").trim())
            .gzip(true)
            .zstd(true)
    }

    pub async fn get_bytes<U: IntoUrl>(&self, url: U) -> Result<impl AsRef<[u8]>> {
        let url = url.into_url().unwrap();
        let resp = self.get_async(url.clone()).await?;
        Ok(resp.bytes().await?)
    }

    pub async fn get_async<U: IntoUrl>(&self, url: U) -> Result<Response> {
        let url = url.into_url().unwrap();
        let headers = github_headers(&url);
        self.get_async_with_headers(url, &headers).await
    }

    async fn get_async_with_headers<U: IntoUrl>(
        &self,
        url: U,
        headers: &HeaderMap,
    ) -> Result<Response> {
        ensure!(!*env::OFFLINE, "offline mode is enabled");
        let mut url = url.into_url().unwrap();
        let do_get = |url: Url,
                      client: reqwest::Client,
                      headers: HeaderMap,
                      timeout: Duration,
                      kind: ClientKind| async move {
            debug!("GET {}", &url);
            let mut req = client.get(url.clone());
            req = req.headers(headers.clone());
            let resp = match req.send().await {
                Ok(resp) => resp,
                Err(err) => {
                    if err.is_timeout() {
                        let (setting, env_var) = match kind {
                            ClientKind::Http => ("http_timeout", "MISE_HTTP_TIMEOUT"),
                            ClientKind::Fetch => (
                                "fetch_remote_versions_timeout",
                                "MISE_FETCH_REMOTE_VERSIONS_TIMEOUT",
                            ),
                            ClientKind::VersionCheck => ("version_check_timeout", ""),
                        };
                        let hint = if env_var.is_empty() {
                            format!(
                                "HTTP timed out after {} for {}.",
                                format_duration(timeout),
                                url
                            )
                        } else {
                            format!(
                                "HTTP timed out after {} for {} (change with `{}` or env `{}`).",
                                format_duration(timeout),
                                url,
                                setting,
                                env_var
                            )
                        };
                        bail!(hint);
                    }
                    return Err(err.into());
                }
            };
            if *env::MISE_LOG_HTTP {
                eprintln!("GET {url} {}", resp.status());
            }
            debug!("GET {url} {}", resp.status());
            display_github_rate_limit(&resp);
            resp.error_for_status_ref()?;
            Ok(resp)
        };

        let resp = match do_get(
            url.clone(),
            self.reqwest.clone(),
            headers.clone(),
            self.timeout,
            self.kind,
        )
        .await
        {
            Ok(resp) => resp,
            Err(_) if url.scheme() == "http" => {
                url.set_scheme("https").unwrap();
                do_get(
                    url,
                    self.reqwest.clone(),
                    headers.clone(),
                    self.timeout,
                    self.kind,
                )
                .await?
            }
            Err(err) => return Err(err),
        };

        resp.error_for_status_ref()?;
        Ok(resp)
    }

    pub async fn head<U: IntoUrl>(&self, url: U) -> Result<Response> {
        let url = url.into_url().unwrap();
        let headers = github_headers(&url);
        self.head_async_with_headers(url, &headers).await
    }

    pub async fn head_async_with_headers<U: IntoUrl>(
        &self,
        url: U,
        headers: &HeaderMap,
    ) -> Result<Response> {
        ensure!(!*env::OFFLINE, "offline mode is enabled");
        let mut url = url.into_url().unwrap();
        let do_head = |url: Url,
                       client: reqwest::Client,
                       headers: HeaderMap,
                       timeout: Duration,
                       kind: ClientKind| async move {
            debug!("HEAD {}", &url);
            let mut req = client.head(url.clone());
            req = req.headers(headers.clone());
            let resp = match req.send().await {
                Ok(resp) => resp,
                Err(err) => {
                    if err.is_timeout() {
                        let (setting, env_var) = match kind {
                            ClientKind::Http => ("http_timeout", "MISE_HTTP_TIMEOUT"),
                            ClientKind::Fetch => (
                                "fetch_remote_versions_timeout",
                                "MISE_FETCH_REMOTE_VERSIONS_TIMEOUT",
                            ),
                            ClientKind::VersionCheck => ("version_check_timeout", ""),
                        };
                        let hint = if env_var.is_empty() {
                            format!(
                                "HTTP timed out after {} for {}.",
                                format_duration(timeout),
                                url
                            )
                        } else {
                            format!(
                                "HTTP timed out after {} for {} (change with `{}` or env `{}`).",
                                format_duration(timeout),
                                url,
                                setting,
                                env_var
                            )
                        };
                        bail!(hint);
                    }
                    return Err(err.into());
                }
            };
            if *env::MISE_LOG_HTTP {
                eprintln!("HEAD {url} {}", resp.status());
            }
            debug!("HEAD {url} {}", resp.status());
            display_github_rate_limit(&resp);
            resp.error_for_status_ref()?;
            Ok(resp)
        };
        let resp = match do_head(
            url.clone(),
            self.reqwest.clone(),
            headers.clone(),
            self.timeout,
            self.kind,
        )
        .await
        {
            Ok(resp) => resp,
            Err(_) if url.scheme() == "http" => {
                url.set_scheme("https").unwrap();
                do_head(
                    url,
                    self.reqwest.clone(),
                    headers.clone(),
                    self.timeout,
                    self.kind,
                )
                .await?
            }
            Err(err) => return Err(err),
        };

        resp.error_for_status_ref()?;
        Ok(resp)
    }

    pub async fn get_text<U: IntoUrl>(&self, url: U) -> Result<String> {
        let mut url = url.into_url().unwrap();
        let resp = self.get_async(url.clone()).await?;
        let text = resp.text().await?;
        if text.starts_with("<!DOCTYPE html>") {
            if url.scheme() == "http" {
                // try with https since http may be blocked
                url.set_scheme("https").unwrap();
                return Box::pin(self.get_text(url)).await;
            }
            bail!("Got HTML instead of text from {}", url);
        }
        Ok(text)
    }

    pub async fn get_html<U: IntoUrl>(&self, url: U) -> Result<String> {
        let url = url.into_url().unwrap();
        let resp = self.get_async(url.clone()).await?;
        let html = resp.text().await?;
        if !html.starts_with("<!DOCTYPE html>") {
            bail!("Got non-HTML text from {}", url);
        }
        Ok(html)
    }

    pub async fn json_headers<T, U: IntoUrl>(&self, url: U) -> Result<(T, HeaderMap)>
    where
        T: serde::de::DeserializeOwned,
    {
        let url = url.into_url().unwrap();
        let resp = self.get_async(url).await?;
        let headers = resp.headers().clone();
        let json = resp.json().await?;
        Ok((json, headers))
    }

    pub async fn json_headers_with_headers<T, U: IntoUrl>(
        &self,
        url: U,
        headers: &HeaderMap,
    ) -> Result<(T, HeaderMap)>
    where
        T: serde::de::DeserializeOwned,
    {
        let url = url.into_url().unwrap();
        let resp = self.get_async_with_headers(url, headers).await?;
        let headers = resp.headers().clone();
        let json = resp.json().await?;
        Ok((json, headers))
    }

    pub async fn json<T, U: IntoUrl>(&self, url: U) -> Result<T>
    where
        T: serde::de::DeserializeOwned,
    {
        self.json_headers(url).await.map(|(json, _)| json)
    }

    pub async fn json_with_headers<T, U: IntoUrl>(&self, url: U, headers: &HeaderMap) -> Result<T>
    where
        T: serde::de::DeserializeOwned,
    {
        self.json_headers_with_headers(url, headers)
            .await
            .map(|(json, _)| json)
    }

    pub async fn download_file<U: IntoUrl>(
        &self,
        url: U,
        path: &Path,
        pr: Option<&Box<dyn SingleReport>>,
    ) -> Result<()> {
        let url = url.into_url()?;
        let headers = github_headers(&url);
        self.download_file_with_headers(url, path, &headers, pr)
            .await
    }

    pub async fn download_file_with_headers<U: IntoUrl>(
        &self,
        url: U,
        path: &Path,
        headers: &HeaderMap,
        pr: Option<&Box<dyn SingleReport>>,
    ) -> Result<()> {
        let url = url.into_url()?;
        debug!("GET Downloading {} to {}", &url, display_path(path));

        let mut resp = self.get_async_with_headers(url, headers).await?;
        if let Some(length) = resp.content_length() {
            if let Some(pr) = pr {
                pr.set_length(length);
            }
        }

        let parent = path.parent().unwrap();
        file::create_dir_all(parent)?;
        let mut file = tempfile::NamedTempFile::with_prefix_in(path, parent)?;
        while let Some(chunk) = resp.chunk().await? {
            file.write_all(&chunk)?;
            if let Some(pr) = pr {
                pr.inc(chunk.len() as u64);
            }
        }
        file.persist(path)?;
        Ok(())
    }
}

pub fn error_code(e: &Report) -> Option<u16> {
    if e.to_string().contains("404") {
        // TODO: not this when I can figure out how to use eyre properly
        return Some(404);
    }
    if let Some(err) = e.downcast_ref::<reqwest::Error>() {
        err.status().map(|s| s.as_u16())
    } else {
        None
    }
}

fn github_headers(url: &Url) -> HeaderMap {
    let mut headers = HeaderMap::new();
    if url.host_str() == Some("api.github.com") {
        if let Some(token) = &*env::GITHUB_TOKEN {
            headers.insert(
                "authorization",
                HeaderValue::from_str(format!("token {token}").as_str()).unwrap(),
            );
            headers.insert(
                "x-github-api-version",
                HeaderValue::from_static("2022-11-28"),
            );
        }
    }
    headers
}

fn display_github_rate_limit(resp: &Response) {
    let status = resp.status().as_u16();
    if status == 403 || status == 429 {
        let remaining = resp
            .headers()
            .get("x-ratelimit-remaining")
            .and_then(|r| r.to_str().ok());
        if remaining.is_some_and(|r| r == "0") {
            if let Some(reset_time) = resp
                .headers()
                .get("x-ratelimit-reset")
                .and_then(|h| h.to_str().ok())
                .and_then(|s| s.parse::<i64>().ok())
                .and_then(|ts| chrono::DateTime::from_timestamp(ts, 0))
            {
                warn!(
                    "GitHub rate limit exceeded. Resets at {}",
                    reset_time.with_timezone(&chrono::Local)
                );
            }
            return;
        }
        // retry-after header is processed only if x-ratelimit-remaining is not 0 or is missing
        if let Some(retry_after) = resp
            .headers()
            .get("retry-after")
            .and_then(|h| h.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok())
        {
            warn!(
                "GitHub rate limit exceeded. Retry after {} seconds",
                retry_after
            );
        }
    }
}
