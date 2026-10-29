use serde::ser::{SerializeMap, SerializeSeq};
use serde::{Deserialize, Deserializer, Serialize};

#[derive(Debug, Clone, Eq, PartialEq, strum::EnumIs)]
pub enum TaskOutputs {
    Files(Vec<String>),
    NoFiles,
    Auto,
}

impl Default for TaskOutputs {
    fn default() -> Self {
        TaskOutputs::Files(vec![])
    }
}

impl TaskOutputs {
    pub fn is_empty(&self) -> bool {
        match self {
            TaskOutputs::Files(files) => files.is_empty(),
            TaskOutputs::NoFiles | TaskOutputs::Auto => false,
        }
    }

    pub fn patterns(&self) -> Vec<String> {
        match self {
            TaskOutputs::Files(files) => files.clone(),
            TaskOutputs::NoFiles | TaskOutputs::Auto => vec![],
        }
    }
}

impl From<&toml::Value> for TaskOutputs {
    fn from(value: &toml::Value) -> Self {
        match value {
            toml::Value::String(file) => TaskOutputs::Files(vec![file.to_string()]),
            toml::Value::Array(files) if files.is_empty() => TaskOutputs::NoFiles,
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
                if files.is_empty() {
                    Ok(TaskOutputs::NoFiles)
                } else {
                    Ok(TaskOutputs::Files(files))
                }
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
            TaskOutputs::NoFiles => serializer.serialize_seq(Some(0))?.end(),
            TaskOutputs::Auto => {
                let mut m = serializer.serialize_map(Some(1))?;
                m.serialize_entry("auto", &true)?;
                m.end()
            }
        }
    }
}
