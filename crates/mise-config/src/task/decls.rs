//! `[tasks]` and `[task_templates]`: the tables of task and template declarations.

use std::fmt::Formatter;

use indexmap::IndexMap;
use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer};

use super::{RunEntry, TaskDecl, TaskTemplate};

/// `[tasks]`: task name to its declaration. A task may be written as a script string or a list
/// of run entries instead of a table; either is a declaration with only `run` set.
#[derive(Debug, Default, Clone)]
pub struct TaskDecls(pub IndexMap<String, TaskDecl>);

impl<'de> Deserialize<'de> for TaskDecls {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct TasksVisitor;

        impl<'de> Visitor<'de> for TasksVisitor {
            type Value = TaskDecls;

            fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
                formatter.write_str("task, string, or array of strings")
            }

            fn visit_map<M: de::MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
                let mut tasks = IndexMap::new();
                while let Some(name) = map.next_key::<String>()? {
                    let task = map.next_value::<TaskEntry>()?.0;
                    tasks.insert(name, task);
                }
                Ok(TaskDecls(tasks))
            }
        }

        deserializer.deserialize_any(TasksVisitor)
    }
}

/// One `[tasks]` value: a table, a script string, or a list of run entries.
struct TaskEntry(TaskDecl);

impl<'de> Deserialize<'de> for TaskEntry {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct TaskEntryVisitor;

        impl<'de> Visitor<'de> for TaskEntryVisitor {
            type Value = TaskEntry;

            fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
                formatter.write_str("task definition")
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(TaskEntry(TaskDecl {
                    run: vec![RunEntry::Script(v.to_string())],
                    ..Default::default()
                }))
            }

            fn visit_seq<S: de::SeqAccess<'de>>(self, mut seq: S) -> Result<Self::Value, S::Error> {
                let mut run = vec![];
                while let Some(entry) = seq.next_element::<RunEntry>()? {
                    run.push(entry);
                }
                Ok(TaskEntry(TaskDecl {
                    run,
                    ..Default::default()
                }))
            }

            fn visit_map<M: de::MapAccess<'de>>(self, map: M) -> Result<Self::Value, M::Error> {
                TaskDecl::deserialize(de::value::MapAccessDeserializer::new(map)).map(TaskEntry)
            }
        }

        deserializer.deserialize_any(TaskEntryVisitor)
    }
}

/// `[task_templates]`: template name to its declaration.
#[derive(Debug, Default, Clone)]
pub struct TaskTemplateDecls(pub IndexMap<String, TaskTemplate>);

impl<'de> Deserialize<'de> for TaskTemplateDecls {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct TaskTemplatesVisitor;

        impl<'de> Visitor<'de> for TaskTemplatesVisitor {
            type Value = TaskTemplateDecls;

            fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
                formatter.write_str("map of task template names to template definitions")
            }

            fn visit_map<M: de::MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
                let mut templates = IndexMap::new();
                while let Some(name) = map.next_key::<String>()? {
                    let template: TaskTemplate = map.next_value()?;
                    templates.insert(name, template);
                }
                Ok(TaskTemplateDecls(templates))
            }
        }

        deserializer.deserialize_any(TaskTemplatesVisitor)
    }
}
