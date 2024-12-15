use crate::dirs;
use crate::task::Task;
use serde::ser::{SerializeMap, SerializeSeq};
use serde::{Deserialize, Deserializer, Serialize};
use std::hash::{DefaultHasher, Hash, Hasher};

#[derive(Debug, Clone, Eq, PartialEq, strum::EnumIs)]
pub enum TaskOutputs {
    Files(Vec<String>),
    Auto,
}

impl Default for TaskOutputs {
    fn default() -> Self {
        TaskOutputs::Files(vec![])
    }
}

impl TaskOutputs {
    pub fn paths(&self, task: &Task) -> Vec<String> {
        match self {
            TaskOutputs::Files(files) => files.clone(),
            TaskOutputs::Auto => vec![self.auto_path(task)],
        }
    }

    fn auto_path(&self, task: &Task) -> String {
        let mut hasher = DefaultHasher::new();
        task.hash(&mut hasher);
        task.config_source.hash(&mut hasher);
        let hash = format!("{:x}", hasher.finish());
        dirs::STATE
            .join("task-auto-outputs")
            .join(&hash)
            .to_string_lossy()
            .to_string()
    }

    pub fn render(&mut self, tera: &mut tera::Tera, ctx: &tera::Context) -> eyre::Result<()> {
        match self {
            TaskOutputs::Files(files) => {
                for file in files.iter_mut() {
                    *file = tera.render_str(file, ctx)?;
                }
            }
            TaskOutputs::Auto => {}
        }
        Ok(())
    }
}

impl From<&toml::Value> for TaskOutputs {
    fn from(value: &toml::Value) -> Self {
        match value {
            toml::Value::String(file) => TaskOutputs::Files(vec![file.to_string()]),
            toml::Value::Array(files) => TaskOutputs::Files(
                files
                    .iter()
                    .map(|v| v.as_str().unwrap().to_string())
                    .collect(),
            ),
            toml::Value::Table(table) => {
                let auto = table
                    .get("auto")
                    .and_then(|v| v.as_bool())
                    .unwrap_or_default();
                if auto {
                    TaskOutputs::Auto
                } else {
                    TaskOutputs::default()
                }
            }
            _ => TaskOutputs::default(),
        }
    }
}

impl<'de> Deserialize<'de> for TaskOutputs {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct TaskOutputsVisitor;

        impl<'de> serde::de::Visitor<'de> for TaskOutputsVisitor {
            type Value = TaskOutputs;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a string, a sequence of strings, or a map")
            }

            fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Self::Value, E> {
                Ok(TaskOutputs::Files(vec![value.to_string()]))
            }

            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> Result<Self::Value, A::Error> {
                let mut files = vec![];
                while let Some(file) = seq.next_element()? {
                    files.push(file);
                }
                Ok(TaskOutputs::Files(files))
            }

            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<Self::Value, A::Error> {
                if let Some(key) = map.next_key::<String>()? {
                    if key == "auto" {
                        if map.next_value::<bool>()? {
                            Ok(TaskOutputs::Auto)
                        } else {
                            Ok(TaskOutputs::default())
                        }
                    } else {
                        Err(serde::de::Error::custom("Invalid TaskOutputs map"))
                    }
                } else {
                    Ok(TaskOutputs::default())
                }
            }
        }

        deserializer.deserialize_any(TaskOutputsVisitor)
    }
}

impl Serialize for TaskOutputs {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            TaskOutputs::Files(files) => {
                let mut seq = serializer.serialize_seq(Some(files.len()))?;
                for file in files {
                    seq.serialize_element(file)?;
                }
                seq.end()
            }
            TaskOutputs::Auto => {
                let mut m = serializer.serialize_map(Some(1))?;
                m.serialize_entry("auto", &true)?;
                m.end()
            }
        }
    }
}

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
    }

    #[test]
    fn test_task_outputs_deserialize() {
        let deserialized: TaskOutputs = serde_json::from_str("\"file1\"").unwrap();
        assert_eq!(deserialized, TaskOutputs::Files(vec!["file1".to_string()]));

        let deserialized: TaskOutputs = serde_json::from_str("[\"file1\"]").unwrap();
        assert_eq!(deserialized, TaskOutputs::Files(vec!["file1".to_string()]));

        let deserialized: TaskOutputs = serde_json::from_str("{ \"auto\": true }").unwrap();
        assert_eq!(deserialized, TaskOutputs::Auto);
    }
}
