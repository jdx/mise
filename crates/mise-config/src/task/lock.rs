use std::fmt;

use serde::de::{self, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// What a task's `lock` declares: the resources it holds while its commands run.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TaskLock {
    /// `lock = false`, or an empty list. Kept apart from an absent key so an overlay or a
    /// task can turn off a lock its template set.
    Off,
    /// `lock = true`: this task, in this repository.
    Task,
    /// `lock = "name"` or a list of names, each shared by every task on the machine that
    /// names it.
    Names(Vec<String>),
}

impl TaskLock {
    /// Whether `name` can name a lock. `MISE_TASK_LOCKS` joins names with commas, so a name
    /// cannot hold one.
    pub fn validate_name(name: &str) -> Result<(), String> {
        if name.trim().is_empty() {
            return Err("a lock name cannot be empty".into());
        }
        if name.contains(',') || name.chars().any(char::is_control) {
            return Err(format!(
                "lock name {name:?} cannot contain a comma or a control character"
            ));
        }
        Ok(())
    }
}

impl Serialize for TaskLock {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            TaskLock::Off => serializer.serialize_bool(false),
            TaskLock::Task => serializer.serialize_bool(true),
            TaskLock::Names(names) => names.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for TaskLock {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct LockVisitor;

        impl<'de> Visitor<'de> for LockVisitor {
            type Value = TaskLock;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a boolean, a lock name, or a list of lock names")
            }

            fn visit_bool<E: de::Error>(self, v: bool) -> Result<TaskLock, E> {
                Ok(if v { TaskLock::Task } else { TaskLock::Off })
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<TaskLock, E> {
                TaskLock::validate_name(v).map_err(E::custom)?;
                Ok(TaskLock::Names(vec![v.to_string()]))
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<TaskLock, A::Error> {
                let mut names = vec![];
                while let Some(name) = seq.next_element::<String>()? {
                    TaskLock::validate_name(&name).map_err(de::Error::custom)?;
                    names.push(name);
                }
                Ok(if names.is_empty() {
                    TaskLock::Off
                } else {
                    TaskLock::Names(names)
                })
            }
        }

        deserializer.deserialize_any(LockVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(value: &str) -> Result<TaskLock, toml::de::Error> {
        #[derive(Deserialize)]
        struct Wrapper {
            lock: TaskLock,
        }
        toml::from_str::<Wrapper>(&format!("lock = {value}")).map(|w| w.lock)
    }

    #[test]
    fn reads_every_shape() {
        assert_eq!(parse("true").unwrap(), TaskLock::Task);
        assert_eq!(parse("false").unwrap(), TaskLock::Off);
        assert_eq!(parse("[]").unwrap(), TaskLock::Off);
        assert_eq!(
            parse("\"test-db\"").unwrap(),
            TaskLock::Names(vec!["test-db".into()])
        );
        assert_eq!(
            parse("[\"sim\", \"test-db\"]").unwrap(),
            TaskLock::Names(vec!["sim".into(), "test-db".into()])
        );
    }

    #[test]
    fn rejects_names_mise_task_locks_cannot_carry() {
        assert!(parse("\"\"").is_err());
        assert!(parse("\"a,b\"").is_err());
        assert!(parse("[\"ok\", \" \"]").is_err());
        assert!(parse("3").is_err());
    }
}
