pub(crate) use mise_config::task::TaskConfirm;

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    #[cfg(unix)]
    use crate::config::Config;
    #[cfg(unix)]
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
                default: Some("yes".to_string()),
                yes: None,
                no: None,
            })
        );
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn test_from_path_confirm_custom_labels() {
        use std::fs;
        use tempfile::tempdir;

        let config = Config::get().await.unwrap();
        let temp_dir = tempdir().unwrap();
        let task_path = temp_dir.path().join("test_task");

        fs::write(
            &task_path,
            r#"#!/bin/bash
#MISE confirm={message="Deploy to prod?", yes="Deploy", no="Cancel"}
echo \"hello world\"
"#,
        )
        .unwrap();
        fs::set_permissions(&task_path, std::fs::Permissions::from_mode(0o755)).unwrap();

        let task = Task::from_path(&config, &task_path, temp_dir.path(), temp_dir.path())
            .await
            .unwrap();
        let confirm = task.confirm.unwrap();
        assert_eq!(confirm.message(), "Deploy to prod?");
        assert_eq!(confirm.default_value(), None);
        assert_eq!(confirm.yes_label(), Some("Deploy"));
        assert_eq!(confirm.no_label(), Some("Cancel"));
    }
}
