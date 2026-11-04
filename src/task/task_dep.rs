use crate::tera::{TeraEngine, contains_template_syntax, render_str};

pub(crate) use mise_config::task::TaskDep;

/// Rendering a dependency's templates needs mise's tera engine, so it lives here rather than on
/// the declaration type in mise-config.
pub(crate) trait TaskDepExt {
    fn render(&mut self, tera: &mut TeraEngine, tera_ctx: &tera::Context) -> crate::Result<bool>;
}

impl TaskDepExt for TaskDep {
    fn render(&mut self, tera: &mut TeraEngine, tera_ctx: &tera::Context) -> crate::Result<bool> {
        if contains_template_syntax(&self.task) {
            self.task = render_str(tera, &self.task, tera_ctx)?;
            if self.task.trim().is_empty() {
                return Ok(false);
            }
        }
        let mut rendered_args = Vec::with_capacity(self.args.len());
        for a in &self.args {
            if contains_template_syntax(a) {
                let rendered = render_str(tera, a, tera_ctx)?;
                if !rendered.is_empty() {
                    rendered_args.push(rendered);
                }
            } else {
                rendered_args.push(a.clone());
            }
        }
        self.args = rendered_args;
        // Render env values through Tera
        for v in self.env.values_mut() {
            if contains_template_syntax(v) {
                *v = render_str(tera, v, tera_ctx)?;
            }
        }
        self.parse_shell_style_env()?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use indexmap::IndexMap;

    #[test]
    fn test_task_dep_from_str() {
        let td: TaskDep = "task".parse().unwrap();
        assert_eq!(td.task, "task");
        assert!(td.args.is_empty());
        assert!(td.env.is_empty());
    }

    #[test]
    fn test_task_dep_display() {
        let td = TaskDep {
            task: "task".to_string(),
            args: vec!["arg1".to_string(), "arg2".to_string()],
            env: Default::default(),
            optional: false,
        };
        assert_eq!(td.to_string(), "task arg1 arg2");

        // With env vars
        let mut env = IndexMap::new();
        env.insert("FOO".to_string(), "bar".to_string());
        let td = TaskDep {
            task: "task".to_string(),
            args: vec![],
            env,
            optional: false,
        };
        assert_eq!(td.to_string(), "FOO=bar task");
    }

    #[test]
    fn test_task_dep_deserialize_string() {
        let td: TaskDep = serde_json::from_str(r#""task""#).unwrap();
        assert_eq!(td.task, "task");
        assert!(td.args.is_empty());
        assert!(td.env.is_empty());
        assert_eq!(&serde_json::to_string(&td).unwrap(), r#""task""#);
    }

    #[test]
    fn test_task_dep_deserialize_array() {
        let td: TaskDep = serde_json::from_str(r#"["task", "arg1", "arg2"]"#).unwrap();
        assert_eq!(td.task, "task");
        assert_eq!(td.args, vec!["arg1", "arg2"]);
        assert!(td.env.is_empty());
        assert_eq!(
            &serde_json::to_string(&td).unwrap(),
            r#"["task","arg1","arg2"]"#
        );
    }

    #[test]
    fn test_task_dep_deserialize_object() {
        let td: TaskDep =
            serde_json::from_str(r#"{"task": "mytask", "env": {"FOO": "bar"}, "args": ["arg1"]}"#)
                .unwrap();
        assert_eq!(td.task, "mytask");
        assert_eq!(td.args, vec!["arg1"]);
        assert_eq!(td.env.get("FOO"), Some(&"bar".to_string()));
        assert!(!td.optional);
    }

    #[test]
    fn test_task_dep_optional_round_trip() {
        let td: TaskDep = serde_json::from_str(r#"{"task":"missing:*","optional":true}"#).unwrap();
        assert_eq!(td.task, "missing:*");
        assert!(td.args.is_empty());
        assert!(td.env.is_empty());
        assert!(td.optional);
        assert_eq!(
            serde_json::to_string(&td).unwrap(),
            r#"{"task":"missing:*","optional":true}"#
        );
    }

    #[test]
    fn test_task_dep_deserialize_object_env_only() {
        let td: TaskDep =
            serde_json::from_str(r#"{"task": "mytask", "env": {"FOO": "bar", "BAZ": "qux"}}"#)
                .unwrap();
        assert_eq!(td.task, "mytask");
        assert!(td.args.is_empty());
        assert_eq!(td.env.get("FOO"), Some(&"bar".to_string()));
        assert_eq!(td.env.get("BAZ"), Some(&"qux".to_string()));
    }

    #[test]
    fn test_task_dep_serialize_with_env() {
        let mut env = IndexMap::new();
        env.insert("FOO".to_string(), "bar".to_string());
        let td = TaskDep {
            task: "mytask".to_string(),
            args: vec![],
            env,
            optional: false,
        };
        let json = serde_json::to_string(&td).unwrap();
        assert!(json.contains(r#""task":"mytask""#));
        assert!(json.contains(r#""env""#));
        assert!(json.contains(r#""FOO":"bar""#));
    }

    #[test]
    fn test_task_dep_render_shell_style_env() {
        let mut td: TaskDep = "FOO=bar mytask arg1".parse().unwrap();
        let mut tera = TeraEngine::V2(Box::default());
        let ctx = tera::Context::new();
        td.render(&mut tera, &ctx).unwrap();

        assert_eq!(td.task, "mytask");
        assert_eq!(td.args, vec!["arg1"]);
        assert_eq!(td.env.get("FOO"), Some(&"bar".to_string()));
    }

    #[test]
    fn test_task_dep_render_multiple_env() {
        let mut td: TaskDep = "FOO=bar BAZ=qux mytask".parse().unwrap();
        let mut tera = TeraEngine::V2(Box::default());
        let ctx = tera::Context::new();
        td.render(&mut tera, &ctx).unwrap();

        assert_eq!(td.task, "mytask");
        assert!(td.args.is_empty());
        assert_eq!(td.env.get("FOO"), Some(&"bar".to_string()));
        assert_eq!(td.env.get("BAZ"), Some(&"qux".to_string()));
    }

    #[test]
    fn test_task_dep_render_no_env() {
        let mut td: TaskDep = "mytask arg1 arg2".parse().unwrap();
        let mut tera = TeraEngine::V2(Box::default());
        let ctx = tera::Context::new();
        td.render(&mut tera, &ctx).unwrap();

        assert_eq!(td.task, "mytask");
        assert_eq!(td.args, vec!["arg1", "arg2"]);
        assert!(td.env.is_empty());
    }

    #[test]
    fn test_task_dep_render_omits_empty_templated_args() {
        let mut td = TaskDep {
            task: "lint".to_string(),
            args: vec![
                "{% if usage.fix %}--fix{% endif %}".to_string(),
                String::new(),
            ],
            env: Default::default(),
            optional: false,
        };
        let mut tera = TeraEngine::V2(Box::default());
        let mut ctx = tera::Context::new();
        ctx.insert("usage", &serde_json::json!({ "fix": false }));
        td.render(&mut tera, &ctx).unwrap();

        assert_eq!(td.args, vec![""]);
    }

    #[test]
    fn test_task_dep_render_keeps_non_empty_templated_args() {
        let mut td = TaskDep {
            task: "lint".to_string(),
            args: vec!["{% if usage.fix %}--fix{% endif %}".to_string()],
            env: Default::default(),
            optional: false,
        };
        let mut tera = TeraEngine::V2(Box::default());
        let mut ctx = tera::Context::new();
        ctx.insert("usage", &serde_json::json!({ "fix": true }));
        td.render(&mut tera, &ctx).unwrap();

        assert_eq!(td.args, vec!["--fix"]);
    }

    #[test]
    fn test_task_dep_render_skips_empty_templated_task() {
        let mut td: TaskDep = "{% if false %}lint{% endif %}".parse().unwrap();
        let mut tera = TeraEngine::V2(Box::default());
        let ctx = tera::Context::new();

        assert!(!td.render(&mut tera, &ctx).unwrap());
        assert!(td.task.is_empty());
    }

    #[test]
    fn test_task_dep_render_skips_whitespace_only_templated_task() {
        let mut td: TaskDep = "\n{% if false %}lint{% endif %}\n".parse().unwrap();
        let mut tera = TeraEngine::V2(Box::default());
        let ctx = tera::Context::new();

        assert!(!td.render(&mut tera, &ctx).unwrap());
        assert_eq!(td.task, "\n\n");
    }

    #[test]
    fn test_task_dep_render_keeps_non_empty_templated_task() {
        let mut td: TaskDep = "{% if true %}lint{% endif %}".parse().unwrap();
        let mut tera = TeraEngine::V2(Box::default());
        let ctx = tera::Context::new();

        assert!(td.render(&mut tera, &ctx).unwrap());
        assert_eq!(td.task, "lint");
    }

    #[test]
    fn test_task_dep_render_keeps_literal_empty_task() {
        let mut td: TaskDep = "".parse().unwrap();
        let mut tera = TeraEngine::V2(Box::default());
        let ctx = tera::Context::new();

        assert!(td.render(&mut tera, &ctx).unwrap());
        assert!(td.task.is_empty());
    }

    #[test]
    fn test_task_dep_single_token_with_equals() {
        // Single token like "build=release" should be treated as task name, not env var
        let mut td: TaskDep = "build=release".parse().unwrap();
        let mut tera = TeraEngine::V2(Box::default());
        let ctx = tera::Context::new();
        td.render(&mut tera, &ctx).unwrap();

        assert_eq!(td.task, "build=release");
        assert!(td.args.is_empty());
        assert!(td.env.is_empty());
    }

    #[test]
    fn test_task_dep_only_env_vars_error() {
        // Only env vars without task name should error
        let mut td: TaskDep = "FOO=bar BAZ=qux".parse().unwrap();
        let mut tera = TeraEngine::V2(Box::default());
        let ctx = tera::Context::new();
        let result = td.render(&mut tera, &ctx);

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("missing task name"));
    }
}
