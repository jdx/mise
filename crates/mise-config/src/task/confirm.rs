use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TaskConfirm {
    Message(String),
    Options {
        message: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        default: Option<String>,
        /// Custom label for the affirmative answer (e.g. "Deploy")
        #[serde(default, skip_serializing_if = "Option::is_none")]
        yes: Option<String>,
        /// Custom label for the negative answer (e.g. "Cancel")
        #[serde(default, skip_serializing_if = "Option::is_none")]
        no: Option<String>,
    },
}

impl TaskConfirm {
    pub fn message(&self) -> &str {
        match self {
            TaskConfirm::Message(message) => message,
            TaskConfirm::Options { message, .. } => message,
        }
    }

    pub fn default_value(&self) -> Option<&str> {
        match self {
            TaskConfirm::Message(_) => None,
            TaskConfirm::Options { default, .. } => default.as_deref(),
        }
    }

    pub fn yes_label(&self) -> Option<&str> {
        match self {
            TaskConfirm::Message(_) => None,
            TaskConfirm::Options { yes, .. } => yes.as_deref(),
        }
    }

    pub fn no_label(&self) -> Option<&str> {
        match self {
            TaskConfirm::Message(_) => None,
            TaskConfirm::Options { no, .. } => no.as_deref(),
        }
    }
}
