use crate::dirs;
use crate::task::Task;
use crate::tera::{TeraEngine, contains_template_syntax, render_str};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::Path;

pub(crate) use mise_config::task::TaskOutputs;

/// Stores raw (pre-render) output templates so they can be re-rendered after
/// dependency env overrides and usage arguments are available.
#[derive(Debug, Clone, Default)]
pub struct RawOutputTemplates {
    pub templates: Option<Vec<String>>,
}

/// The parts of [`TaskOutputs`] that need mise: rendering templates with tera, and the state
/// directory an `auto` output lives in.
pub trait TaskOutputsExt {
    fn paths(&self, task: &Task, root: &Path) -> Vec<String>;
    fn has_tera_template(&self) -> bool;
    fn raw_templates_without_env(&self) -> RawOutputTemplates;
    fn render(
        &mut self,
        tera: &mut TeraEngine,
        ctx: &tera::Context,
        defer_usage: bool,
    ) -> eyre::Result<RawOutputTemplates>;
    fn re_render_with_env(
        &mut self,
        raw: &mut RawOutputTemplates,
        path_env: &mut Option<std::collections::BTreeMap<String, String>>,
        env: &indexmap::IndexMap<String, String>,
        config_root: &std::path::Path,
    ) -> eyre::Result<()>;
}

impl TaskOutputsExt for TaskOutputs {
    fn paths(&self, task: &Task, root: &Path) -> Vec<String> {
        match self {
            TaskOutputs::Files(files) => files.clone(),
            TaskOutputs::NoFiles => vec![],
            TaskOutputs::Auto => vec![auto_output_path(task, root)],
        }
    }

    fn has_tera_template(&self) -> bool {
        match self {
            TaskOutputs::Files(files) => files.iter().any(|file| contains_template_syntax(file)),
            TaskOutputs::NoFiles | TaskOutputs::Auto => false,
        }
    }

    fn raw_templates_without_env(&self) -> RawOutputTemplates {
        match self {
            TaskOutputs::Files(files) => RawOutputTemplates {
                templates: Some(files.clone()),
            },
            TaskOutputs::NoFiles | TaskOutputs::Auto => RawOutputTemplates::default(),
        }
    }

    fn render(
        &mut self,
        tera: &mut TeraEngine,
        ctx: &tera::Context,
        defer_usage: bool,
    ) -> eyre::Result<RawOutputTemplates> {
        match self {
            TaskOutputs::Files(files) => {
                let raw = files.clone();
                for file in files.iter_mut() {
                    if contains_template_syntax(file)
                        && !(defer_usage && super::tera_template_has_usage_ref(file))
                    {
                        *file = render_str(tera, file, ctx)?;
                    }
                }
                Ok(RawOutputTemplates {
                    templates: Some(raw),
                })
            }
            TaskOutputs::NoFiles | TaskOutputs::Auto => Ok(RawOutputTemplates::default()),
        }
    }

    /// Re-render output templates with additional env vars injected into the
    /// tera context. Used after dependency env overrides are applied.
    fn re_render_with_env(
        &mut self,
        raw: &mut RawOutputTemplates,
        path_env: &mut Option<std::collections::BTreeMap<String, String>>,
        env: &indexmap::IndexMap<String, String>,
        config_root: &std::path::Path,
    ) -> eyre::Result<()> {
        // Keep the dependency-level environment for the later usage render of
        // both sources and outputs. Sources share this context because the env
        // override applies to the task invocation as a whole.
        let mut env_map = path_env.clone().unwrap_or_default();
        for (k, v) in env {
            env_map.insert(k.clone(), v.clone());
        }
        *path_env = Some(env_map.clone());
        if let TaskOutputs::Files(files) = self
            && let Some(raw_templates) = raw.templates.clone()
        {
            if raw_templates
                .iter()
                .any(|tmpl| contains_template_syntax(tmpl))
            {
                let mut tera = crate::tera::get_tera(Some(config_root));
                let mut ctx = crate::tera::BASE_CONTEXT.clone();
                ctx.insert("env", &env_map);
                ctx.insert("config_root", &config_root.to_string_lossy().to_string());
                *files = raw_templates
                    .iter()
                    .map(|tmpl| {
                        if contains_template_syntax(tmpl)
                            && !super::tera_template_has_usage_ref(tmpl)
                        {
                            render_str(&mut tera, tmpl, &ctx)
                        } else {
                            Ok(tmpl.clone())
                        }
                    })
                    .collect::<Result<Vec<_>, _>>()?;
            } else {
                *files = raw_templates.clone();
            }
        }
        Ok(())
    }
}

fn auto_output_path(task: &Task, root: &Path) -> String {
    let mut hasher = DefaultHasher::new();
    task.hash(&mut hasher);
    task.config_sources().hash(&mut hasher);
    root.hash(&mut hasher);
    let hash = format!("{:x}", hasher.finish());
    dirs::STATE
        .join("task-auto-outputs")
        .join(&hash)
        .to_string_lossy()
        .to_string()
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::*;

    #[test]
    fn test_task_outputs_from_toml() {
        let value: toml::Table = toml::from_str("outputs = \"file1\"").unwrap();
        let value = value.get("outputs").unwrap();
        let outputs = TaskOutputs::from(value);
        assert_eq!(outputs, TaskOutputs::Files(vec!["file1".to_string()]));

        let value: toml::Table = toml::from_str("outputs = [\"file1\"]").unwrap();
        let value = value.get("outputs").unwrap();
        let outputs = TaskOutputs::from(value);
        assert_eq!(outputs, TaskOutputs::Files(vec!["file1".to_string()]));

        let value: toml::Table = toml::from_str("outputs = []").unwrap();
        let value = value.get("outputs").unwrap();
        let outputs = TaskOutputs::from(value);
        assert_eq!(outputs, TaskOutputs::NoFiles);

        let value: toml::Table = toml::from_str("outputs = { auto = true }").unwrap();
        let value = value.get("outputs").unwrap();
        let outputs = TaskOutputs::from(value);
        assert_eq!(outputs, TaskOutputs::Auto);
    }

    #[test]
    fn test_task_outputs_serialize() {
        let outputs = TaskOutputs::Files(vec!["file1".to_string()]);
        let serialized = serde_json::to_string(&outputs).unwrap();
        assert_eq!(serialized, "[\"file1\"]");

        let outputs = TaskOutputs::Auto;
        let serialized = serde_json::to_string(&outputs).unwrap();
        assert_eq!(serialized, "{\"auto\":true}");

        let outputs = TaskOutputs::NoFiles;
        let serialized = serde_json::to_string(&outputs).unwrap();
        assert_eq!(serialized, "[]");
    }

    #[test]
    fn test_task_outputs_deserialize() {
        let deserialized: TaskOutputs = serde_json::from_str("\"file1\"").unwrap();
        assert_eq!(deserialized, TaskOutputs::Files(vec!["file1".to_string()]));

        let deserialized: TaskOutputs = serde_json::from_str("[\"file1\"]").unwrap();
        assert_eq!(deserialized, TaskOutputs::Files(vec!["file1".to_string()]));

        let deserialized: TaskOutputs = serde_json::from_str("[]").unwrap();
        assert_eq!(deserialized, TaskOutputs::NoFiles);

        let deserialized: TaskOutputs = serde_json::from_str("{ \"auto\": true }").unwrap();
        assert_eq!(deserialized, TaskOutputs::Auto);
    }
}
