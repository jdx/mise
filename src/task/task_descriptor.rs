use crate::task::Task;
use crate::task::task_execution_plan::{format_declaration_location, task_declaration_ref};
use serde_json::{Value, json};

#[derive(Debug, Clone, Default)]
pub struct TaskDescriptorOptions {
    pub include_global: bool,
    pub include_usage: bool,
    pub include_timeout: bool,
    pub include_args: bool,
    pub include_file: bool,
    pub usage_spec: Option<Value>,
    pub use_run_script_strings: bool,
}

pub fn task_descriptor_json(task: &Task, opts: &TaskDescriptorOptions) -> Value {
    let declaration = task_declaration_ref(task);
    let run = if opts.use_run_script_strings {
        task.run_script_strings()
    } else {
        task.run()
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
    };

    let mut map = serde_json::Map::new();
    map.insert("name".to_string(), json!(task.display_name));
    map.insert("aliases".to_string(), json!(task.aliases));
    map.insert("description".to_string(), json!(task.description));
    map.insert(
        "source".to_string(),
        json!(task.config_source.to_string_lossy().to_string()),
    );
    map.insert(
        "source_location".to_string(),
        json!(format_declaration_location(&declaration)),
    );
    map.insert("declaration".to_string(), json!(declaration));
    map.insert(
        "depends".to_string(),
        json!(
            task.depends
                .iter()
                .map(|d| d.task.clone())
                .collect::<Vec<_>>()
        ),
    );
    map.insert(
        "depends_post".to_string(),
        json!(
            task.depends_post
                .iter()
                .map(|d| d.task.clone())
                .collect::<Vec<_>>()
        ),
    );
    map.insert(
        "wait_for".to_string(),
        json!(
            task.wait_for
                .iter()
                .map(|d| d.task.clone())
                .collect::<Vec<_>>()
        ),
    );
    map.insert(
        "env".to_string(),
        json!(task.env.0.iter().map(|d| d.to_string()).collect::<Vec<_>>()),
    );
    map.insert("dir".to_string(), json!(task.dir));
    map.insert("hide".to_string(), json!(task.hide));
    map.insert("raw".to_string(), json!(task.raw));
    map.insert("interactive".to_string(), json!(task.is_interactive()));
    map.insert("sources".to_string(), json!(task.sources));
    map.insert("outputs".to_string(), json!(task.outputs));
    map.insert("shell".to_string(), json!(task.shell));
    map.insert("quiet".to_string(), json!(task.quiet));
    map.insert("silent".to_string(), json!(task.silent));
    map.insert("tools".to_string(), json!(task.tools));
    map.insert("run".to_string(), json!(run));
    if opts.include_file {
        map.insert("file".to_string(), json!(task.file));
    }

    if opts.include_global {
        map.insert("global".to_string(), json!(task.global));
    }
    if opts.include_usage {
        map.insert("usage".to_string(), json!(task.usage));
    }
    if opts.include_timeout {
        map.insert("timeout".to_string(), json!(task.timeout));
    }
    if opts.include_args {
        map.insert("args".to_string(), json!(task.args));
    }
    if let Some(usage_spec) = &opts.usage_spec {
        map.insert("usage_spec".to_string(), usage_spec.clone());
    }

    Value::Object(map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task::RunEntry;
    use std::path::PathBuf;

    #[test]
    fn test_task_descriptor_json_includes_core_fields() {
        let task = Task {
            name: "build".to_string(),
            display_name: "build".to_string(),
            description: "Build the project".to_string(),
            config_source: PathBuf::from("/tmp/mise.toml"),
            run: vec![RunEntry::Script("echo build".to_string())],
            ..Default::default()
        };

        let value = task_descriptor_json(&task, &TaskDescriptorOptions::default());
        assert_eq!(value["name"], "build");
        assert_eq!(value["description"], "Build the project");
        assert_eq!(value["source"], "/tmp/mise.toml");
        assert_eq!(value["source_location"], "/tmp/mise.toml");
        assert_eq!(value["declaration"]["source"], "/tmp/mise.toml");
        assert!(value.get("usage_spec").is_none());
        assert!(value.get("global").is_none());
        assert!(value.get("usage").is_none());
        assert!(value.get("timeout").is_none());
        assert!(value.get("args").is_none());
    }

    #[test]
    fn test_task_descriptor_json_honors_optional_fields_and_run_mode() {
        let task = Task {
            name: "test".to_string(),
            display_name: "test".to_string(),
            config_source: PathBuf::from("/tmp/mise.toml"),
            global: true,
            usage: "arg <name>".to_string(),
            timeout: Some("30s".to_string()),
            args: vec!["--flag".to_string()],
            run: vec![RunEntry::Script("echo one".to_string())],
            run_windows: vec![RunEntry::Script("echo two".to_string())],
            ..Default::default()
        };

        let opts = TaskDescriptorOptions {
            include_global: true,
            include_usage: true,
            include_timeout: true,
            include_args: true,
            include_file: true,
            usage_spec: Some(json!("cmd test")),
            use_run_script_strings: true,
        };

        let value = task_descriptor_json(&task, &opts);
        assert_eq!(value["global"], true);
        assert_eq!(value["usage"], "arg <name>");
        assert_eq!(value["timeout"], "30s");
        assert_eq!(value["args"], json!(vec!["--flag"]));
        assert_eq!(value["usage_spec"], "cmd test");
        #[cfg(windows)]
        let expected_run = json!(vec!["echo two"]);
        #[cfg(not(windows))]
        let expected_run = json!(vec!["echo one"]);
        assert_eq!(value["run"], expected_run);
    }
}
