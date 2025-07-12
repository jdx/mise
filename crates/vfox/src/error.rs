use mlua::Error as MLuaError;
use thiserror::Error;
use xx::XXError;

#[derive(Error, Debug)]
#[non_exhaustive]
pub enum VfoxError {
    #[error("{0}")]
    Error(String),
    #[error(transparent)]
    LuaError(#[from] MLuaError),
    #[error("serde_json")]
    SerdeJsonError(#[from] serde_json::Error),
    #[error(transparent)]
    XXError(#[from] XXError),
    #[error(transparent)]
    ReqwestError(#[from] reqwest::Error),
    #[error(transparent)]
    IoError(#[from] std::io::Error),
    #[error(transparent)]
    UrlParseError(#[from] url::ParseError),
}

pub type Result<T> = std::result::Result<T, VfoxError>;

impl From<String> for VfoxError {
    fn from(s: String) -> Self {
        VfoxError::Error(s)
    }
}

impl From<&str> for VfoxError {
    fn from(s: &str) -> Self {
        VfoxError::Error(s.to_string())
    }
}

#[macro_export]
macro_rules! error {
    ($($arg:tt)*) => {
        return Err(VfoxError::Error(format!($($arg)*)));
    };
}
