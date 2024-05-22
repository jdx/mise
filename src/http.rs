use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::time::Duration;

use eyre::{Report, Result};
use once_cell::sync::Lazy;
use reqwest::IntoUrl;
use reqwest::{ClientBuilder, Response};

use crate::cli::version;
use crate::env::MISE_FETCH_REMOTE_VERSIONS_TIMEOUT;
use crate::file::display_path;
use crate::ui::progress_report::SingleReport;
use crate::{env, file};

#[cfg(not(test))]
pub static HTTP_VERSION_CHECK: Lazy<Client> =
    Lazy::new(|| Client::new(Duration::from_secs(3)).unwrap());

pub static HTTP: Lazy<Client> = Lazy::new(|| Client::new(Duration::from_secs(30)).unwrap());

pub static HTTP_FETCH: Lazy<Client> =
    Lazy::new(|| Client::new(*MISE_FETCH_REMOTE_VERSIONS_TIMEOUT).unwrap());

#[derive(Debug)]
pub struct Client {
    reqwest: reqwest::Client,
}

impl Client {
    fn new(timeout: Duration) -> Result<Self> {
        Ok(Self {
            reqwest: Self::_new()
                .timeout(timeout)
                .connect_timeout(timeout)
                .build()?,
        })
    }

    fn _new() -> ClientBuilder {
        ClientBuilder::new()
            .user_agent(format!("mise/{}", &*version::VERSION))
            .gzip(true)
    }

    pub async fn get<U: IntoUrl>(&self, url: U) -> Result<Response> {
        let mut url = url.into_url().unwrap();

        match self._get(url.clone()).await {
            Ok(resp) => Ok(resp),
            Err(_) if url.scheme() == "http" => {
                // try with https since http may be blocked
                url.set_scheme("https").unwrap();
                self._get(url).await
            }
            Err(err) => return Err(err.into()),
        }
    }

    async fn _get<U: IntoUrl>(&self, url: U) -> Result<Response> {
        let url = url.into_url().unwrap();
        debug!("GET {}", url);
        let mut req = self.reqwest.get(url.clone());
        if url.host_str() == Some("api.github.com") {
            if let Some(token) = &*env::GITHUB_API_TOKEN {
                req = req.header("authorization", format!("token {}", token));
            }
        }

        let resp = req.send().await?;
        debug!("GET {url} {}", resp.status());
        resp.error_for_status_ref()?;
        Ok(resp)
    }

    pub async fn get_text<U: IntoUrl>(&self, url: U) -> Result<String> {
        let mut url = url.into_url().unwrap();
        let resp = self.get(url.clone()).await?;
        let text = resp.text().await?;
        if text.starts_with("<!DOCTYPE html>") {
            if url.scheme() == "http" {
                // try with https since http may be blocked
                url.set_scheme("https").unwrap();
                return self.get_text(url).await;
            }
            bail!("Got HTML instead of text from {}", url);
        }
        Ok(text)
    }

    pub async fn json<T, U: IntoUrl>(&self, url: U) -> Result<T>
    where
        T: serde::de::DeserializeOwned,
    {
        let url = url.into_url().unwrap();
        let resp = self.get(url).await?;
        let json = resp.json().await?;
        Ok(json)
    }

    pub async fn download_file<U: IntoUrl>(
        &self,
        url: U,
        path: &Path,
        pr: Option<&dyn SingleReport>,
    ) -> Result<()> {
        let url = url.into_url()?;
        debug!("GET Downloading {} to {}", &url, display_path(path));
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
