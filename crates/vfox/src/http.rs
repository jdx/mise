use reqwest::{Client, ClientBuilder};
use std::sync::LazyLock;

pub static CLIENT: LazyLock<Client> = LazyLock::new(|| {
    ClientBuilder::new()
        .user_agent(format!("vfox.rs/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .expect("Failed to create reqwest client")
});
