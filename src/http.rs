use std::fs::File;

#[cfg(not(test))]
pub static HTTP_VERSION_CHECK: Lazy<Client> =
    Lazy::new(|| Client::new(Duration::from_secs(3)).unwrap());

pub static HTTP: Lazy<Client> = Lazy::new(|| Client::new(Duration::from_secs(30)).unwrap());

pub static HTTP_FETCH: Lazy<Client> =
    Lazy::new(|| Client::new(*MISE_FETCH_REMOTE_VERSIONS_TIMEOUT).unwrap());

use std::path::Path;
use std::time::Duration;

use crate::env::MISE_FETCH_REMOTE_VERSIONS_TIMEOUT;
use crate::file::display_path;
use crate::{env, file};
use miette::{IntoDiagnostic, Report, Result};
use once_cell::sync::Lazy;
use reqwest::blocking::{ClientBuilder, Response};
use reqwest::IntoUrl;

#[derive(Debug)]
pub struct Client {
    reqwest: reqwest::blocking::Client,
}

impl Client {
    fn new(timeout: Duration) -> Result<Self> {
        Ok(Self {
            reqwest: Self::_new()
                .timeout(timeout)
                .connect_timeout(timeout)
                .build()
                .into_diagnostic()?,
        })
    }

    fn _new() -> ClientBuilder {
        ClientBuilder::new()
            .user_agent(format!("mise/{}", env!("CARGO_PKG_VERSION")))
            .gzip(true)
    }

    pub fn get<U: IntoUrl>(&self, url: U) -> Result<Response> {
        let url = url.into_url().unwrap();
        debug!("GET {}", url);
        let mut req = self.reqwest.get(url.clone());
        if url.host_str() == Some("api.github.com") {
            if let Some(token) = &*env::GITHUB_API_TOKEN {
                req = req.header("authorization", format!("token {}", token));
            }
        }
        let resp = req.send().into_diagnostic()?;
        debug!("GET {url} {}", resp.status());
        resp.error_for_status_ref().into_diagnostic()?;
        Ok(resp)
    }

    pub fn get_text<U: IntoUrl>(&self, url: U) -> Result<String> {
        let url = url.into_url().unwrap();
        let resp = self.get(url.clone())?;
        let text = resp.text().into_diagnostic()?;
        if text.starts_with("<!DOCTYPE html>") {
            bail!("Got HTML instead of text from {}", url);
        }
        Ok(text)
    }

    pub fn json<T, U: IntoUrl>(&self, url: U) -> Result<T>
    where
        T: serde::de::DeserializeOwned,
    {
        let url = url.into_url().unwrap();
        let resp = self.get(url)?;
        let json = resp.json().into_diagnostic()?;
        Ok(json)
    }

    pub fn download_file<U: IntoUrl>(&self, url: U, path: &Path) -> Result<()> {
        let url = url.into_url().into_diagnostic()?;
        debug!("GET Downloading {} to {}", &url, display_path(path));
        let mut resp = self.get(url)?;

        file::create_dir_all(path.parent().unwrap())?;
        let mut file = File::create(path).into_diagnostic()?;
        resp.copy_to(&mut file).into_diagnostic()?;
        Ok(())
    }
}

pub fn error_code(e: &Report) -> Option<u16> {
    if e.to_string().contains("404") {
        // TODO: not this when I can figure out how to use miette properly
        return Some(404);
    }
    if let Some(err) = e.downcast_ref::<reqwest::Error>() {
        err.status().map(|s| s.as_u16())
    } else {
        None
    }
}
