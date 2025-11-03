use std::collections::BTreeMap;
use std::str::FromStr;

use once_cell::sync::Lazy;
use url::Url;

static SDKS: Lazy<BTreeMap<String, Url>> = Lazy::new(|| {
    [
        ("nodejs", "https://github.com/version-fox/vfox-nodejs"),
        ("cmake", "https://github.com/version-fox/vfox-cmake"),
    ]
    .iter()
    .map(|(name, url)| (name.to_string(), Url::from_str(url).unwrap()))
    .collect()
});

pub fn sdk_url(name: &str) -> Option<&Url> {
    SDKS.get(name)
}

pub fn list_sdks() -> &'static BTreeMap<String, Url> {
    &SDKS
}
