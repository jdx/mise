use color_eyre::eyre::Result;
use reqwest::blocking::RequestBuilder;
use reqwest::IntoUrl;

pub struct Client {
    reqwest: reqwest::blocking::Client,
}

impl Client {
    pub fn new() -> Result<Self> {
        let reqwest = reqwest::blocking::ClientBuilder::new()
            .user_agent(format!("rtx/{}", env!("CARGO_PKG_VERSION")))
            .build()?;
        Ok(Self { reqwest })
    }

    pub fn get<U: IntoUrl>(&self, url: U) -> RequestBuilder {
        self.reqwest.get(url)
    }
}
