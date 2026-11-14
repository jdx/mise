use std::fmt;
use std::num::NonZeroU32;

use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// What a task's `lock` declares: the resources it holds while its commands run.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TaskLock {
    /// `lock = false`, or an empty list. Kept apart from an absent key so an overlay or a
    /// task can turn off a lock its template set.
    Off,
    /// `lock = true`: this task, in this repository.
    Task,
    /// `lock = "name"`, a `{ name, count }` table, or a list of them, each shared by every task
    /// on the machine that names it.
    Names(Vec<NamedLock>),
}

/// A lock shared by every task on the machine that names it. It is a pool of slots, one slot
/// unless `task.lock_pools` gives it more, and a run holds `count` of them while it runs.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct NamedLock {
    pub name: String,
    pub count: NonZeroU32,
}

impl NamedLock {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            count: NonZeroU32::MIN,
        }
    }

    pub fn with_count(mut self, count: NonZeroU32) -> Self {
        self.count = count;
        self
    }
}

impl TaskLock {
    /// Whether `name` holds template syntax. A template is checked after it renders, since
    /// its source can hold a comma that the rendered name does not.
    pub fn is_template(name: &str) -> bool {
        name.contains("{{") || name.contains("{%") || name.contains("{#")
    }

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
        if let Some((_, tail)) = name.rsplit_once(':')
            && !tail.is_empty()
            && tail.bytes().all(|b| b.is_ascii_digit())
        {
            return Err(format!(
                "lock name {name:?} cannot end in `:` and a number: mise names the slots of a \
                 lock that way"
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

impl Serialize for NamedLock {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        if self.count == NonZeroU32::MIN {
            return serializer.serialize_str(&self.name);
        }
        let mut map = serializer.serialize_map(Some(2))?;
        map.serialize_entry("name", &self.name)?;
        map.serialize_entry("count", &self.count)?;
        map.end()
    }
}

impl<'de> Deserialize<'de> for TaskLock {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct LockVisitor;

        impl<'de> Visitor<'de> for LockVisitor {
            type Value = TaskLock;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a boolean, a lock name, a `{ name, count }` table, or a list of them")
            }

            fn visit_bool<E: de::Error>(self, v: bool) -> Result<TaskLock, E> {
                Ok(if v { TaskLock::Task } else { TaskLock::Off })
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<TaskLock, E> {
                NamedLockVisitor
                    .visit_str(v)
                    .map(|lock| TaskLock::Names(vec![lock]))
            }

            fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<TaskLock, A::Error> {
                NamedLockVisitor
                    .visit_map(map)
                    .map(|lock| TaskLock::Names(vec![lock]))
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<TaskLock, A::Error> {
                let mut names = vec![];
                while let Some(lock) = seq.next_element::<NamedLock>()? {
                    names.push(lock);
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

struct NamedLockVisitor;

impl<'de> Visitor<'de> for NamedLockVisitor {
    type Value = NamedLock;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a lock name or a `{ name, count }` table")
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<NamedLock, E> {
        if !TaskLock::is_template(v) {
            TaskLock::validate_name(v).map_err(E::custom)?;
        }
        Ok(NamedLock::new(v))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<NamedLock, A::Error> {
        let mut name: Option<String> = None;
        let mut count: Option<NonZeroU32> = None;
        while let Some(key) = map.next_key::<String>()? {
            match key.as_str() {
                "name" => name = Some(map.next_value()?),
                "count" => {
                    let n: i64 = map.next_value()?;
                    count = Some(u32::try_from(n).ok().and_then(NonZeroU32::new).ok_or_else(
                        || {
                            de::Error::custom(format!(
                                "lock count must be a positive whole number, not {n}"
                            ))
                        },
                    )?);
                }
                other => return Err(de::Error::unknown_field(other, &["name", "count"])),
            }
        }
        let name = name.ok_or_else(|| de::Error::missing_field("name"))?;
        if !TaskLock::is_template(&name) {
            TaskLock::validate_name(&name).map_err(de::Error::custom)?;
        }
        Ok(NamedLock {
            name,
            count: count.unwrap_or(NonZeroU32::MIN),
        })
    }
}

impl<'de> Deserialize<'de> for NamedLock {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(NamedLockVisitor)
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
    fn a_template_is_checked_after_it_renders() {
        let name = "{{ config_root | replace(from='/', to='-') }}";
        assert_eq!(
            parse(&format!("\"{name}\"")).unwrap(),
            TaskLock::Names(vec![NamedLock::new(name)])
        );
        assert!(parse("\"a,b\"").is_err());
    }

    #[test]
    fn reads_every_shape() {
        assert_eq!(parse("true").unwrap(), TaskLock::Task);
        assert_eq!(parse("false").unwrap(), TaskLock::Off);
        assert_eq!(parse("[]").unwrap(), TaskLock::Off);
        assert_eq!(
            parse("\"test-db\"").unwrap(),
            TaskLock::Names(vec![NamedLock::new("test-db")])
        );
        assert_eq!(
            parse("[\"sim\", \"test-db\"]").unwrap(),
            TaskLock::Names(vec![NamedLock::new("sim"), NamedLock::new("test-db")])
        );
    }

    #[test]
    fn rejects_names_mise_task_locks_cannot_carry() {
        assert!(parse("\"\"").is_err());
        assert!(parse("\"a,b\"").is_err());
        assert!(parse("[\"ok\", \" \"]").is_err());
        assert!(parse("3").is_err());
    }

    #[test]
    fn reads_a_count() {
        let four = NonZeroU32::new(4).unwrap();
        assert_eq!(
            parse("{ name = \"cpu\", count = 4 }").unwrap(),
            TaskLock::Names(vec![NamedLock::new("cpu").with_count(four)])
        );
        assert_eq!(
            parse("[{ name = \"cpu\", count = 4 }, \"test-db\"]").unwrap(),
            TaskLock::Names(vec![
                NamedLock::new("cpu").with_count(four),
                NamedLock::new("test-db")
            ])
        );
        assert_eq!(
            parse("{ name = \"cpu\" }").unwrap(),
            TaskLock::Names(vec![NamedLock::new("cpu")])
        );
    }

    #[test]
    fn rejects_bad_tables() {
        assert!(parse("{ count = 2 }").is_err());
        assert!(parse("{ name = \"cpu\", count = 0 }").is_err());
        assert!(parse("{ name = \"cpu\", count = -1 }").is_err());
        assert!(parse("{ name = \"cpu\", count = \"2\" }").is_err());
        assert!(parse("{ name = \"a,b\", count = 2 }").is_err());
        assert!(parse("{ name = \"cpu\", slots = 2 }").is_err());
    }

    #[test]
    fn rejects_names_that_look_like_slots() {
        assert!(parse("\"cpu:1\"").is_err());
        assert!(parse("{ name = \"cpu:12\", count = 2 }").is_err());
        assert!(parse("\"cpu:x\"").is_ok());
        assert!(parse("\"cpu:\"").is_ok());
        assert!(parse("\"cpu0\"").is_ok());
    }

    #[test]
    fn writes_a_name_unless_it_has_a_count() {
        let lock = TaskLock::Names(vec![
            NamedLock::new("sim"),
            NamedLock::new("cpu").with_count(NonZeroU32::new(2).unwrap()),
        ]);
        assert_eq!(
            serde_json::to_string(&lock).unwrap(),
            r#"["sim",{"name":"cpu","count":2}]"#
        );
    }
}
