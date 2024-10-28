use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use std::time::Duration;

use eyre::{bail, Report, Result};
use flate2::read::GzDecoder;
use once_cell::sync::Lazy;
use reqwest::header::HeaderMap;
use reqwest::{ClientBuilder, IntoUrl, Response};
use tokio::runtime::Runtime;
use url::Url;

use crate::cli::version;
use crate::config::SETTINGS;
use crate::file::display_path;
use crate::ui::progress_report::SingleReport;
use crate::{env, file};

#[cfg(not(test))]
pub static HTTP_VERSION_CHECK: Lazy<Client> =
    Lazy::new(|| Client::new(Duration::from_secs(3)).unwrap());

pub static HTTP: Lazy<Client> = Lazy::new(|| {
    let duration = humantime::parse_duration(&SETTINGS.http_timeout)
        .unwrap_or_else(|_| Duration::from_secs(SETTINGS.http_timeout.parse().unwrap()));
    Client::new(duration).unwrap()
});

pub static HTTP_FETCH: Lazy<Client> = Lazy::new(|| {
    Client::new(humantime::parse_duration(&SETTINGS.fetch_remote_versions_timeout).unwrap())
        .unwrap()
});

#[derive(Debug)]
pub struct Client {
    reqwest: reqwest::Client,
}

impl Client {
    fn new(timeout: Duration) -> Result<Self> {
        Ok(Self {
            reqwest: Self::_new()
                .read_timeout(timeout)
                .connect_timeout(timeout)
                .build()?,
        })
    }

    fn _new() -> ClientBuilder {
        ClientBuilder::new()
            .user_agent(format!("mise/{}", &*version::VERSION))
            .gzip(true)
    }

    async fn get<U: IntoUrl>(&self, url: U) -> Result<Response> {
        let get = |url: Url| async move {
            debug!("GET {}", &url);
            let mut req = self.reqwest.get(url.clone());
            if url.host_str() == Some("api.github.com") {
                if let Some(token) = &*env::GITHUB_TOKEN {
                    req = req.header("authorization", format!("token {}", token));
                }
            }
            let resp = req.send().await?;
            debug!("GET {url} {}", resp.status());
            resp.error_for_status_ref()?;
            Ok(resp)
        };
        let mut url = url.into_url().unwrap();
        let resp = match get(url.clone()).await {
            Ok(resp) => resp,
            Err(_) if url.scheme() == "http" => {
                // try with https since http may be blocked
                url.set_scheme("https").unwrap();
                get(url).await?
            }
            Err(err) => return Err(err),
        };

        resp.error_for_status_ref()?;
        Ok(resp)
    }

    pub fn get_text<U: IntoUrl>(&self, url: U) -> Result<String> {
        let mut url = url.into_url().unwrap();
        let rt = self.runtime()?;
        let text = rt.block_on(async {
            let resp = self.get(url.clone()).await?;
            Ok::<String, eyre::Error>(resp.text().await?)
        })?;
        if text.starts_with("<!DOCTYPE html>") {
            if url.scheme() == "http" {
                // try with https since http may be blocked
                url.set_scheme("https").unwrap();
                return self.get_text(url);
            }
            bail!("Got HTML instead of text from {}", url);
        }
        Ok(text)
    }

    pub fn get_text_gz<U: IntoUrl>(&self, url: U) -> Result<String> {
        let text = self.get_text(url)?;
        let bytes = GzDecoder::new(text.as_bytes())
            .bytes()
            .collect::<Result<Vec<u8>, _>>()?;
        Ok(String::from_utf8(bytes)?)
    }

    pub fn json_headers<T, U: IntoUrl>(&self, url: U) -> Result<(T, HeaderMap)>
    where
        T: serde::de::DeserializeOwned,
    {
        let url = url.into_url().unwrap();
        let rt = self.runtime()?;
        let (json, headers) = rt.block_on(async {
            let resp = self.get(url).await?;
            let headers = resp.headers().clone();
            Ok::<(T, HeaderMap), eyre::Error>((resp.json().await?, headers))
        })?;
        Ok((json, headers))
    }

    pub fn json<T, U: IntoUrl>(&self, url: U) -> Result<T>
    where
        T: serde::de::DeserializeOwned,
    {
        self.json_headers(url).map(|(json, _)| json)
    }

    pub fn download_file<U: IntoUrl>(
        &self,
        url: U,
        path: &Path,
        pr: Option<&dyn SingleReport>,
    ) -> Result<()> {
        let url = url.into_url()?;
        debug!("GET Downloading {} to {}", &url, display_path(path));

        let rt = self.runtime()?;
        rt.block_on(async {
            let mut resp = self.get(url).await?;
            if let Some(length) = resp.content_length() {
                if let Some(pr) = pr {
                    pr.set_length(length);
                }
            }

            file::create_dir_all(path.parent().unwrap())?;
            let mut file = File::create(path)?;
            while let Some(chunk) = resp.chunk().await? {
                file.write_all(&chunk)?;
                if let Some(pr) = pr {
                    pr.inc(chunk.len() as u64);
                }
            }
            Ok::<(), eyre::Error>(())
        })?;
        Ok(())
    }

    fn runtime(&self) -> Result<Runtime, Report> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .enable_io()
            .build()?;
        Ok(rt)
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
