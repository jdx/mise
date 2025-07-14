use crate::config::config_file::toml::deserialize_arr;
use serde::ser::SerializeSeq;
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::fmt::{Display, Formatter};
use std::str::FromStr;

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct TaskDep {
    pub task: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
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
        for (k, v) in &mut self.env {
            *v = tera.render_str(v, tera_ctx)?;
        }
        if self.args.is_empty() {
            let s = self.task.clone();
            let mut split = s.split_whitespace().map(|s| s.to_string());
            if let Some(task) = split.next() {
                self.task = task;
            }
            self.args = split.collect();
        }
        Ok(self)
    }
}

impl Display for TaskDep {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.task)?;
        if !self.args.is_empty() {
            write!(f, " {}", self.args.join(" "))?;
        }
        if !self.env.is_empty() {
            write!(f, " (env: {})", 
                self.env.iter()
                    .map(|(k, v)| format!("{}={}", k, v))
                    .collect::<Vec<_>>()
                    .join(", "))?;
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

use serde::de::{self, MapAccess, SeqAccess, Visitor};

// Helper struct for deserializing object format
#[derive(Deserialize)]
struct TaskDepObject {
    task: String,
    #[serde(default, deserialize_with = "deserialize_arr")]
    args: Vec<String>,
    #[serde(default)]
    env: BTreeMap<String, String>,
}

struct TaskDepVisitor;

impl<'de> Visitor<'de> for TaskDepVisitor {
    type Value = TaskDep;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a string, array, or object")
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(TaskDep {
            task: value.to_string(),
            args: Default::default(),
            env: Default::default(),
        })
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut input = Vec::new();
        while let Some(elem) = seq.next_element::<String>()? {
            input.push(elem);
        }

        if input.is_empty() {
            Err(de::Error::custom("Task name is required"))
        } else if input.len() == 1 {
            Ok(TaskDep {
                task: input[0].clone(),
                args: Default::default(),
                env: Default::default(),
            })
        } else {
            Ok(TaskDep {
                task: input[0].clone(),
                args: input[1..].to_vec(),
                env: Default::default(),
            })
        }
    }

    fn visit_map<A>(self, map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let obj = TaskDepObject::deserialize(de::value::MapAccessDeserializer::new(map))?;
        Ok(TaskDep {
            task: obj.task,
            args: obj.args,
            env: obj.env,
        })
    }
}

impl<'de> Deserialize<'de> for TaskDep {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(TaskDepVisitor)
    }
}

impl Serialize for TaskDep {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        
        // If we have env vars, always serialize as object
        if !self.env.is_empty() {
            let mut map = serializer.serialize_map(Some(3))?;
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
            let mut seq = serializer.serialize_seq(Some(1 + self.args.len()))?;
            seq.serialize_element(&self.task)?;
            for arg in &self.args {
                seq.serialize_element(arg)?;
            }
            seq.end()
        }
    }
}

mod tests {
    #[allow(unused_imports)] // no idea why I need this
    use super::*;

    #[test]
    fn test_task_dep_from_str() {
        let td: TaskDep = "task".parse().unwrap();
        assert_eq!(td.task, "task");
        assert!(td.args.is_empty());

        // TODO: td.render()
        // let td: TaskDep = "task arg1 arg2".parse().unwrap();
        // assert_eq!(td.task, "task");
        // assert_eq!(td.args, vec!["arg1", "arg2"]);
    }

    #[test]
    fn test_task_dep_display() {
        let td = TaskDep {
            task: "task".to_string(),
            args: vec!["arg1".to_string(), "arg2".to_string()],
            env: Default::default(),
        };
        assert_eq!(td.to_string(), "task arg1 arg2");

        let td = TaskDep {
            task: "task".to_string(),
            args: vec![],
            env: [("VAR".to_string(), "value".to_string())].into_iter().collect(),
        };
        assert_eq!(td.to_string(), "task (env: VAR=value)");

        let td = TaskDep {
            task: "task".to_string(),
            args: vec!["arg1".to_string()],
            env: [("VAR".to_string(), "value".to_string())].into_iter().collect(),
        };
        assert_eq!(td.to_string(), "task arg1 (env: VAR=value)");
    }

    #[test]
    fn test_task_dep_deserialize() {
        // Test string format
        let td: TaskDep = serde_json::from_str(r#""task""#).unwrap();
        assert_eq!(td.task, "task");
        assert!(td.args.is_empty());
        assert!(td.env.is_empty());
        assert_eq!(&serde_json::to_string(&td).unwrap(), r#""task""#);

        // Test array format
        let td: TaskDep = serde_json::from_str(r#"["task", "arg1", "arg2"]"#).unwrap();
        assert_eq!(td.task, "task");
        assert_eq!(td.args, vec!["arg1", "arg2"]);
        assert!(td.env.is_empty());
        assert_eq!(
            &serde_json::to_string(&td).unwrap(),
            r#"["task","arg1","arg2"]"#
        );

        // Test object format with env
        let td: TaskDep = serde_json::from_str(r#"{"task": "test", "env": {"PROP": "a"}}"#).unwrap();
        assert_eq!(td.task, "test");
        assert!(td.args.is_empty());
        assert_eq!(td.env.get("PROP"), Some(&"a".to_string()));

        // Test object format with args and env
        let td: TaskDep = serde_json::from_str(r#"{"task": "test", "args": ["arg1"], "env": {"PROP": "b"}}"#).unwrap();
        assert_eq!(td.task, "test");
        assert_eq!(td.args, vec!["arg1"]);
        assert_eq!(td.env.get("PROP"), Some(&"b".to_string()));
    }
}
