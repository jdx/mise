use crate::config::env_directive::EnvDirective;
use crate::task::Task;
use serde::Serialize;

/// Canonical executable identity for a task instance.
/// This is the single source of truth for deterministic ordering and
/// deduplication semantics: (task.name, args, env).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct TaskIdentity {
    pub name: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

impl TaskIdentity {
    pub fn from_task(task: &Task) -> Self {
        let mut env: Vec<(String, String)> = task
            .env
            .0
            .iter()
            .filter_map(|d| match d {
                EnvDirective::Val(k, v, _) => Some((k.clone(), v.clone())),
                _ => None,
            })
            .collect();
        env.sort();
        Self {
            name: task.name.clone(),
            args: task.args.clone(),
            env,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::config_file::mise_toml::EnvList;

    #[test]
    fn test_task_identity_sorts_env_pairs() {
        // MatrixRef: B06 / C10
        let task = Task {
            name: "build".to_string(),
            env: EnvList(vec![
                EnvDirective::Val("B".to_string(), "2".to_string(), Default::default()),
                EnvDirective::Val("A".to_string(), "1".to_string(), Default::default()),
            ]),
            ..Default::default()
        };
        let identity = TaskIdentity::from_task(&task);
        assert_eq!(
            identity.env,
            vec![
                ("A".to_string(), "1".to_string()),
                ("B".to_string(), "2".to_string())
            ]
        );
    }

    #[test]
    fn test_task_identity_lexicographic_order_name_args_env() {
        // MatrixRef: B06 / C10
        let mut tasks = vec![
            Task {
                name: "a".to_string(),
                args: vec!["z".to_string()],
                ..Default::default()
            },
            Task {
                name: "a".to_string(),
                args: vec!["a".to_string()],
                ..Default::default()
            },
            Task {
                name: "b".to_string(),
                ..Default::default()
            },
        ];

        tasks.sort_by_key(TaskIdentity::from_task);
        let names_and_args: Vec<(String, Vec<String>)> =
            tasks.into_iter().map(|t| (t.name, t.args)).collect();

        assert_eq!(
            names_and_args,
            vec![
                ("a".to_string(), vec!["a".to_string()]),
                ("a".to_string(), vec!["z".to_string()]),
                ("b".to_string(), vec![]),
            ]
        );
    }
}
