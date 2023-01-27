use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("plugin not installed: {0}")]
    PluginNotInstalled(String),
    // #[error("No version found for: {0}@{1}")]
    // VersionNotFound(String, String),
    #[error("runtime version not installed: {0}@{1}")]
    VersionNotInstalled(String, String),
}
