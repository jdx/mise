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
    pub optional: bool,
}

impl TaskDep {
    pub fn parse_shell_style_env(&mut self) -> eyre::Result<&mut Self> {
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
            optional: false,
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
                    optional: false,
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
                    optional: false,
                })
            }

            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
                let mut task: Option<String> = None;
                let mut args: Vec<String> = Vec::new();
                let mut env: IndexMap<String, String> = IndexMap::new();
                let mut optional = false;

                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "task" => task = Some(map.next_value()?),
                        "args" => args = map.next_value()?,
                        "env" => env = map.next_value()?,
                        "optional" => optional = map.next_value()?,
                        _ => {
                            return Err(de::Error::unknown_field(
                                &key,
                                &["task", "args", "env", "optional"],
                            ));
                        }
                    }
                }

                Ok(TaskDep {
                    task: task.ok_or_else(|| de::Error::missing_field("task"))?,
                    args,
                    env,
                    optional,
                })
            }
        }

        deserializer.deserialize_any(TaskDepVisitor)
    }
}

impl Serialize for TaskDep {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if !self.env.is_empty() || self.optional {
            // Use object format when env or optional metadata is present
            let mut map = serializer.serialize_map(None)?;
            map.serialize_entry("task", &self.task)?;
            if !self.args.is_empty() {
                map.serialize_entry("args", &self.args)?;
            }
            if !self.env.is_empty() {
                map.serialize_entry("env", &self.env)?;
            }
            if self.optional {
                map.serialize_entry("optional", &true)?;
            }
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
