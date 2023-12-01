use eyre::{Report, Result};
use reqwest::blocking::{ClientBuilder, RequestBuilder};
use reqwest::IntoUrl;
use std::fs::File;
use std::path::Path;
use std::time::Duration;

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

    pub fn get<U: IntoUrl>(&self, url: U) -> RequestBuilder {
        let url = url.into_url().unwrap();
        debug!("GET {}", url);
        self.reqwest.get(url)
    }

    pub fn get_text<U: IntoUrl>(&self, url: U) -> Result<String> {
        let url = url.into_url().unwrap();
        let resp = self.get(url).send()?;
        resp.error_for_status_ref()?;
        let text = resp.text()?;
        Ok(text)
    }

    pub fn json<T, U: IntoUrl>(&self, url: U) -> Result<T>
    where
        T: serde::de::DeserializeOwned,
    {
        let url = url.into_url().unwrap();
        let resp = self.get(url.clone()).send()?;
        resp.error_for_status_ref()?;
        let json = resp.json()?;
        Ok(json)
    }

    pub fn download_file<U: IntoUrl>(&self, url: U, path: &Path) -> Result<()> {
        let url = url.into_url()?;
        debug!("Downloading {} to {}", &url, path.display());
        let mut resp = self.get(url).send()?;
        resp.error_for_status_ref()?;
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
