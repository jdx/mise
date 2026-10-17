use serde::Deserialize;

#[derive(Debug, Default, Clone, Deserialize)]
pub struct RepoTomlConfig {
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default, rename = "ref")]
    pub git_ref: Option<String>,
    #[serde(default)]
    pub filter: Option<String>,
    #[serde(default)]
    pub depth: Option<u32>,
    #[serde(default)]
    pub submodules: Option<bool>,
}
