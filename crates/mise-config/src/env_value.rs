//! The value of an environment variable in mise config: a string, an integer, or a
//! boolean where `false` unsets it.

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum EnvValue {
    String(String),
    Integer(i64),
    Boolean(bool),
}

impl EnvValue {
    pub fn into_string(self) -> Option<String> {
        match self {
            Self::String(value) => Some(value),
            Self::Integer(value) => Some(value.to_string()),
            Self::Boolean(true) => Some("true".to_string()),
            Self::Boolean(false) => None,
        }
    }

    pub fn into_default_string(self) -> Option<String> {
        match self {
            Self::String(value) => Some(value),
            Self::Integer(value) => Some(value.to_string()),
            Self::Boolean(_) => None,
        }
    }
}

impl TryFrom<&toml::Value> for EnvValue {
    type Error = String;

    fn try_from(value: &toml::Value) -> Result<Self, Self::Error> {
        match value {
            toml::Value::String(value) => Ok(Self::String(value.clone())),
            toml::Value::Integer(value) => Ok(Self::Integer(*value)),
            toml::Value::Boolean(value) => Ok(Self::Boolean(*value)),
            _ => Err("environment values must be strings, integers, or booleans".to_string()),
        }
    }
}

impl From<String> for EnvValue {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

impl From<&str> for EnvValue {
    fn from(value: &str) -> Self {
        Self::String(value.to_string())
    }
}

impl From<i64> for EnvValue {
    fn from(value: i64) -> Self {
        Self::Integer(value)
    }
}

impl From<bool> for EnvValue {
    fn from(value: bool) -> Self {
        Self::Boolean(value)
    }
}

impl From<EnvValue> for toml_edit::Value {
    fn from(value: EnvValue) -> Self {
        match value {
            EnvValue::String(value) => Self::from(value),
            EnvValue::Integer(value) => Self::from(value),
            EnvValue::Boolean(value) => Self::from(value),
        }
    }
}
