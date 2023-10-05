use color_eyre::eyre::{eyre, Result};
use reqwest::blocking::{RequestBuilder, Response};
use reqwest::IntoUrl;
use std::fs::File;
use std::path::Path;

pub struct Client {
    reqwest: reqwest::blocking::Client,
}

impl Client {
    pub fn new() -> Result<Self> {
        let reqwest = reqwest::blocking::ClientBuilder::new()
            .user_agent(format!("rtx/{}", env!("CARGO_PKG_VERSION")))
            .gzip(true)
            .build()?;
        Ok(Self { reqwest })
    }

    pub fn get<U: IntoUrl>(&self, url: U) -> RequestBuilder {
        let url = url.into_url().unwrap();
        debug!("GET {}", url);
        self.reqwest.get(url)
    }

    pub fn get_text<U: IntoUrl>(&self, url: U) -> Result<String> {
        let url = url.into_url().unwrap();
        debug!("GET.txt {}", url);
        let resp = self.get(url).send()?;
        self.ensure_success(&resp)?;
        let text = resp.text()?;
        Ok(text)
    }

    pub fn download_file<U: IntoUrl>(&self, url: U, path: &Path) -> Result<()> {
        let url = url.into_url()?;
        debug!("Downloading {} to {}", &url, path.display());
        let mut resp = self.get(url).send()?;
        self.ensure_success(&resp)?;
        let mut file = File::create(path)?;
        resp.copy_to(&mut file)?;
        Ok(())
    }

    pub fn ensure_success(&self, resp: &Response) -> Result<()> {
        if resp.status().is_success() {
            Ok(())
        } else {
            Err(eyre!("HTTP error: {} on {}", resp.status(), resp.url()))
        }
    }
}
