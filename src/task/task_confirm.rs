use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TaskConfirm {
    Message(String),
    Options { message: String, default: String },
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
            TaskConfirm::Options { default, .. } => Some(default.as_str()),
        }
    }
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    use crate::config::Config;
    use crate::task::{Task, TaskConfirm};

    #[tokio::test]
    #[cfg(unix)]
    async fn test_from_path_confirm_object() {
        use std::fs;
        use tempfile::tempdir;

        let config = Config::get().await.unwrap();
        let temp_dir = tempdir().unwrap();
        let task_path = temp_dir.path().join("test_task");

        fs::write(
            &task_path,
            r#"#!/bin/bash
#MISE confirm={message="Proceed?", default="yes"}
echo \"hello world\"
"#,
        )
        .unwrap();
        fs::set_permissions(&task_path, std::fs::Permissions::from_mode(0o755)).unwrap();

        let task = Task::from_path(&config, &task_path, temp_dir.path(), temp_dir.path())
            .await
            .unwrap();
        assert_eq!(
            task.confirm,
            Some(TaskConfirm::Options {
                message: "Proceed?".to_string(),
                default: "yes".to_string()
            })
        );
    }
}
