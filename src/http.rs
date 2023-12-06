use std::fs::File;
use std::path::Path;
use std::time::Duration;

use crate::file::display_path;
use crate::{env, file};
use eyre::{Report, Result};
use reqwest::blocking::{ClientBuilder, Response};
use reqwest::IntoUrl;

#[derive(Debug)]
pub struct Client {
    reqwest: reqwest::blocking::Client,
}

impl Client {
    pub fn new() -> Result<Self> {
        Ok(Self {
            reqwest: Self::_new().build()?,
        })
    }

    pub fn new_with_timeout(timeout: Duration) -> Result<Self> {
        Ok(Self {
            reqwest: Self::_new().timeout(timeout).build()?,
        })
    }

    fn _new() -> ClientBuilder {
        ClientBuilder::new()
            .user_agent(format!("rtx/{}", env!("CARGO_PKG_VERSION")))
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
        let resp = req.send()?;
        debug!("GET {url} {}", resp.status());
        resp.error_for_status_ref()?;
        Ok(resp)
    }

    pub fn get_text<U: IntoUrl>(&self, url: U) -> Result<String> {
        let url = url.into_url().unwrap();
        let resp = self.get(url)?;
        let text = resp.text()?;
        Ok(text)
    }

    pub fn json<T, U: IntoUrl>(&self, url: U) -> Result<T>
    where
        T: serde::de::DeserializeOwned,
    {
        let url = url.into_url().unwrap();
        let resp = self.get(url.clone())?;
        let json = resp.json()?;
        Ok(json)
    }

    pub fn download_file<U: IntoUrl>(&self, url: U, path: &Path) -> Result<()> {
        let url = url.into_url()?;
        debug!("GET Downloading {} to {}", &url, display_path(path));
        let mut resp = self.get(url.clone())?;

        file::create_dir_all(path.parent().unwrap())?;
        let mut file = File::create(path)?;
        resp.copy_to(&mut file)?;
        Ok(())
    }
}

pub fn error_code(e: &Report) -> Option<u16> {
    if let Some(err) = e.downcast_ref::<reqwest::Error>() {
        err.status().map(|s| s.as_u16())
    } else {
        None
    }
}
