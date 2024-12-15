use crate::config::config_file::toml::deserialize_arr;
use itertools::Itertools;
use serde::ser::SerializeSeq;
use serde::{Deserialize, Deserializer, Serialize};
use std::fmt;
use std::fmt::{Display, Formatter};
use std::str::FromStr;

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct TaskDep {
    pub task: String,
    pub args: Vec<String>,
}

impl Display for TaskDep {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
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
        let parts = s.split_whitespace().collect_vec();
        if parts.is_empty() {
            return Err("Task name is required".to_string());
        }
        Ok(Self {
            task: parts[0].to_string(),
            args: parts[1..].iter().map(|s| s.to_string()).collect(),
        })
    }
}

impl<'de> Deserialize<'de> for TaskDep {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let input: Vec<String> = deserialize_arr(deserializer)?;
        if input.is_empty() {
            Err(serde::de::Error::custom("Task name is required"))
        } else if input.len() == 1 {
            Ok(input[0].to_string().into())
        } else {
            Ok(Self {
                task: input[0].clone(),
                args: input[1..].to_vec(),
            })
        }
    }
}

impl Serialize for TaskDep {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if self.args.is_empty() {
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

        let td: TaskDep = "task arg1 arg2".parse().unwrap();
        assert_eq!(td.task, "task");
        assert_eq!(td.args, vec!["arg1", "arg2"]);
    }

    #[test]
    fn test_task_dep_display() {
        let td = TaskDep {
            task: "task".to_string(),
            args: vec!["arg1".to_string(), "arg2".to_string()],
        };
        assert_eq!(td.to_string(), "task arg1 arg2");
    }

    #[test]
    fn test_task_dep_deserialize() {
        let td: TaskDep = serde_json::from_str(r#""task""#).unwrap();
        assert_eq!(td.task, "task");
        assert!(td.args.is_empty());
        assert_eq!(&serde_json::to_string(&td).unwrap(), r#""task""#);

        let td: TaskDep = serde_json::from_str(r#"["task", "arg1", "arg2"]"#).unwrap();
        assert_eq!(td.task, "task");
        assert_eq!(td.args, vec!["arg1", "arg2"]);
        assert_eq!(
            &serde_json::to_string(&td).unwrap(),
            r#"["task","arg1","arg2"]"#
        );
    }
}
