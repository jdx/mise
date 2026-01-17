use indexmap::IndexMap;
use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize};
use std::fmt;
use std::fmt::{Display, Formatter};
use std::str::FromStr;

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct TaskDep {
    pub task: String,
    pub args: Vec<String>,
    pub env: IndexMap<String, String>,
}

impl TaskDep {
    pub fn render(
        &mut self,
        tera: &mut tera::Tera,
        tera_ctx: &tera::Context,
    ) -> crate::Result<&mut Self> {
        self.task = tera.render_str(&self.task, tera_ctx)?;
        for a in &mut self.args {
            *a = tera.render_str(a, tera_ctx)?;
        }
        // Render env values through Tera
        for v in self.env.values_mut() {
            *v = tera.render_str(v, tera_ctx)?;
        }
        // Parse shell-style "FOO=bar BAZ=qux taskname arg1 arg2" if args/env not already set
        if self.args.is_empty() && self.env.is_empty() {
            let s = self.task.clone();
            let parts: Vec<String> = shell_words::split(&s)
                .unwrap_or_else(|_| s.split_whitespace().map(String::from).collect());

            // Only parse env vars if there are multiple parts
            // Single token like "build=release" should be treated as task name, not env var
            if parts.len() > 1 {
                let mut task_found = false;

                for part in parts {
                    if !task_found {
                        // Check if this looks like KEY=value (env var)
                        if let Some((key, value)) = part.split_once('=') {
                            // Only treat as env var if key looks like a valid env var name
                            if !key.is_empty()
                                && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                            {
                                self.env.insert(key.to_string(), value.to_string());
                                continue;
                            }
                        }
                        // First non-env-var token is the task name
                        self.task = part;
                        task_found = true;
                    } else {
                        self.args.push(part);
                    }
                }

                // Validate that a task name was found (not just env vars)
                if !task_found {
                    return Err(eyre::eyre!(
                        "invalid task dependency '{}': missing task name (only environment variables found)",
                        s
                    ));
                }
            } else if let Some(task) = parts.into_iter().next() {
                // Single token - use as task name directly (even if it contains '=')
                self.task = task;
            }
        }
        Ok(self)
    }
}

impl Display for TaskDep {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        for (k, v) in &self.env {
            write!(f, "{}={} ", k, v)?;
        }
        write!(f, "{}", self.task)?;
        if !self.args.is_empty() {
            write!(f, " {}", self.args.join(" "))?;
        }
        Ok(())
    }
}

impl From<String> for TaskDep {
    fn from(s: String) -> Self {
        s.parse().unwrap()
    }
}

impl FromStr for TaskDep {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self {
            task: s.to_string(),
            args: Default::default(),
            env: Default::default(),
        })
    }
}

impl<'de> Deserialize<'de> for TaskDep {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct TaskDepVisitor;

        impl<'de> Visitor<'de> for TaskDepVisitor {
            type Value = TaskDep;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a string, array, or object")
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(TaskDep {
                    task: v.to_string(),
                    args: Default::default(),
                    env: Default::default(),
                })
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                let mut items: Vec<String> = Vec::new();
                while let Some(item) = seq.next_element()? {
                    items.push(item);
                }
                if items.is_empty() {
                    return Err(de::Error::custom("Task name is required"));
                }
                Ok(TaskDep {
                    task: items[0].clone(),
                    args: items[1..].to_vec(),
                    env: Default::default(),
                })
            }

            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
                let mut task: Option<String> = None;
                let mut args: Vec<String> = Vec::new();
                let mut env: IndexMap<String, String> = IndexMap::new();

                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "task" => task = Some(map.next_value()?),
                        "args" => args = map.next_value()?,
                        "env" => env = map.next_value()?,
                        _ => {
                            return Err(de::Error::unknown_field(&key, &["task", "args", "env"]));
                        }
                    }
                }

                Ok(TaskDep {
                    task: task.ok_or_else(|| de::Error::missing_field("task"))?,
                    args,
                    env,
                })
            }
        }

        deserializer.deserialize_any(TaskDepVisitor)
    }
}

impl Serialize for TaskDep {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if !self.env.is_empty() {
            // Use object format when env is present
            let mut map = serializer.serialize_map(None)?;
            map.serialize_entry("task", &self.task)?;
            if !self.args.is_empty() {
                map.serialize_entry("args", &self.args)?;
            }
            map.serialize_entry("env", &self.env)?;
            map.end()
        } else if self.args.is_empty() {
            serializer.serialize_str(&self.task)
        } else {
            // TODO: it would be possible to track if the user specified a string and if so, continue that format
            use serde::ser::SerializeSeq;
            let mut seq = serializer.serialize_seq(Some(1 + self.args.len()))?;
            seq.serialize_element(&self.task)?;
            for arg in &self.args {
                seq.serialize_element(arg)?;
            }
            seq.end()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        };
        assert_eq!(td.to_string(), "task arg1 arg2");

        // With env vars
        let mut env = IndexMap::new();
        env.insert("FOO".to_string(), "bar".to_string());
        let td = TaskDep {
            task: "task".to_string(),
            args: vec![],
            env,
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
        };
        let json = serde_json::to_string(&td).unwrap();
        assert!(json.contains(r#""task":"mytask""#));
        assert!(json.contains(r#""env""#));
        assert!(json.contains(r#""FOO":"bar""#));
    }

    #[test]
    fn test_task_dep_render_shell_style_env() {
        let mut td: TaskDep = "FOO=bar mytask arg1".parse().unwrap();
        let mut tera = tera::Tera::default();
        let ctx = tera::Context::new();
        td.render(&mut tera, &ctx).unwrap();

        assert_eq!(td.task, "mytask");
        assert_eq!(td.args, vec!["arg1"]);
        assert_eq!(td.env.get("FOO"), Some(&"bar".to_string()));
    }

    #[test]
    fn test_task_dep_render_multiple_env() {
        let mut td: TaskDep = "FOO=bar BAZ=qux mytask".parse().unwrap();
        let mut tera = tera::Tera::default();
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
        let mut tera = tera::Tera::default();
        let ctx = tera::Context::new();
        td.render(&mut tera, &ctx).unwrap();

        assert_eq!(td.task, "mytask");
        assert_eq!(td.args, vec!["arg1", "arg2"]);
        assert!(td.env.is_empty());
    }

    #[test]
    fn test_task_dep_single_token_with_equals() {
        // Single token like "build=release" should be treated as task name, not env var
        let mut td: TaskDep = "build=release".parse().unwrap();
        let mut tera = tera::Tera::default();
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
        let mut tera = tera::Tera::default();
        let ctx = tera::Context::new();
        let result = td.render(&mut tera, &ctx);

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("missing task name"));
    }
}
