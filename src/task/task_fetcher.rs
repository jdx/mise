use crate::config::config_file::{trust_check, trust_check_remote_fetch};
use crate::config::{Config, Settings};
use crate::task::task_file_providers::{TaskFileArtifact, TaskFileProvidersBuilder};
use crate::task::{Task, script_header_has_decoded_template};
use dashmap::DashMap;
use eyre::{Result, WrapErr};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, LazyLock, Mutex},
};
use tokio::sync::OnceCell;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct RemoteTaskArtifactKey {
    config_source: PathBuf,
    task_name: String,
    source: String,
}

type RemoteTaskArtifacts = DashMap<RemoteTaskArtifactKey, Arc<OnceCell<TaskFileArtifact>>>;
static REMOTE_TASK_ARTIFACTS: LazyLock<RemoteTaskArtifacts> = LazyLock::new(DashMap::new);
static REMOTE_TASK_ARTIFACT_SCOPES: Mutex<usize> = Mutex::new(0);

/// Keeps no-cache remote task snapshots alive for one command or direct caller.
pub(crate) struct RemoteTaskArtifactsGuard(());

impl RemoteTaskArtifactsGuard {
    pub(crate) fn new() -> Self {
        *REMOTE_TASK_ARTIFACT_SCOPES.lock().unwrap() += 1;
        Self(())
    }
}

impl Drop for RemoteTaskArtifactsGuard {
    fn drop(&mut self) {
        let (include_artifacts, task_artifacts) = {
            let mut scopes = REMOTE_TASK_ARTIFACT_SCOPES.lock().unwrap();
            *scopes -= 1;
            if *scopes != 0 {
                return;
            }
            (
                crate::config::take_remote_task_include_artifacts(),
                take_remote_task_artifacts(),
            )
        };
        drop(include_artifacts);
        drop(task_artifacts);
    }
}

fn take_remote_task_artifacts() -> Vec<Arc<OnceCell<TaskFileArtifact>>> {
    let keys = REMOTE_TASK_ARTIFACTS
        .iter()
        .map(|entry| entry.key().clone())
        .collect::<Vec<_>>();
    keys.into_iter()
        .filter_map(|key| {
            REMOTE_TASK_ARTIFACTS
                .remove(&key)
                .map(|(_, artifact)| artifact)
        })
        .collect()
}

/// Handles fetching remote task files and converting them to local paths
pub struct TaskFetcher {
    no_cache: bool,
    trust_before_fetch: bool,
}

impl TaskFetcher {
    pub fn new(no_cache: bool) -> Self {
        Self {
            no_cache,
            trust_before_fetch: false,
        }
    }

    /// Require trust before a remote provider performs network or Git work.
    pub fn require_trust_before_fetch(mut self) -> Self {
        self.trust_before_fetch = true;
        self
    }

    /// Fetch remote task files, converting remote paths to local cached paths
    pub async fn fetch_tasks(&self, config: &Arc<Config>, tasks: &mut Vec<Task>) -> Result<()> {
        let no_cache = self.no_cache || Settings::get().task.remote_no_cache.unwrap_or(false);
        let task_file_providers = TaskFileProvidersBuilder::new()
            .with_cache(!no_cache)
            .build();

        for t in tasks {
            if let Some(file) = &t.file {
                let source = file.to_string_lossy().to_string();

                // Skip local files - they don't need provider resolution
                if !Self::is_remote_source(&source) {
                    continue;
                }

                let original = t.clone();
                let defining_cf = original
                    .cf
                    .clone()
                    .or_else(|| config.config_files.get(&original.config_source).cloned());
                let defining_config_source = original
                    .remote_config_source
                    .clone()
                    .or_else(|| defining_cf.as_ref().map(|cf| cf.get_path().to_path_buf()))
                    .unwrap_or_else(|| original.config_source.clone());

                if self.trust_before_fetch {
                    trust_check_remote_fetch(&defining_config_source).wrap_err_with(|| {
                        format!(
                            "fetching remote task {source} requires its defining config to be trusted"
                        )
                    })?;
                }

                let provider = task_file_providers
                    .get_provider(&source)
                    .ok_or_else(|| eyre::eyre!("No provider found for file: {}", source))?;
                let artifact = if no_cache {
                    // Reuse one snapshot when dependency resolution materializes
                    // the same task again, while keeping separate task definitions
                    // that happen to use the same URL independent.
                    let key = RemoteTaskArtifactKey {
                        config_source: original.config_source.clone(),
                        task_name: original.name.clone(),
                        source: source.clone(),
                    };
                    let artifact = REMOTE_TASK_ARTIFACTS
                        .entry(key)
                        .or_insert_with(|| Arc::new(OnceCell::new()))
                        .clone();
                    artifact
                        .get_or_try_init(|| async { provider.get_local_artifact(&source).await })
                        .await?
                        .clone()
                } else {
                    provider.get_local_artifact(&source).await?
                };
                let local_path = artifact.path;
                let config_root = original
                    .config_root
                    .clone()
                    .or_else(|| original.config_source.parent().map(|p| p.to_path_buf()))
                    .unwrap_or_default();
                let prefix = local_path.parent().unwrap_or(&local_path);

                // Parse the downloaded script as a regular file task so all #MISE
                // metadata is honored. The inline TOML task remains the higher-
                // precedence overlay, matching local file-task behavior.
                let body = crate::file::read_to_string(&local_path).wrap_err_with(|| {
                    format!("failed to read remote task metadata from {source}")
                })?;
                let header_has_templates = script_header_has_decoded_template(&body);
                let mut remote = Task::from_path_unrendered_with_cf(
                    &local_path,
                    prefix,
                    &config_root,
                    original.cf.clone(),
                )
                .wrap_err_with(|| format!("failed to parse remote task metadata from {source}"))?;
                if header_has_templates {
                    trust_check(&defining_config_source).wrap_err_with(|| {
                        format!(
                            "remote task metadata from {source} requires its defining config to be trusted"
                        )
                    })?;
                }
                let remote_metadata_has_tools = !remote.tools.is_empty();
                remote.name.clone_from(&original.name);
                remote.display_name.clone_from(&original.display_name);

                // Restore runtime render context before rendering remote headers.
                // Templates in those headers may depend on task vars or env inherited
                // from the invocation that selected this task.
                remote.args.clone_from(&original.args);
                remote.trailing_args.clone_from(&original.trailing_args);
                remote.show_args_in_prefix = original.show_args_in_prefix;
                remote.inherited_env.clone_from(&original.inherited_env);
                remote.overlay_env.clone_from(&original.overlay_env);
                remote.overlay_vars.clone_from(&original.overlay_vars);
                remote
                    .render(config, &config_root)
                    .await
                    .wrap_err_with(|| {
                        format!("failed to render remote task metadata from {source}")
                    })?;
                remote.merge_toml_overlay(original.clone());

                // Preserve runtime state that is not task metadata and therefore is
                // intentionally not handled by merge_toml_overlay().
                remote.global = original.global;
                remote.remote_file_source = Some(source);
                remote.remote_config_source = Some(defining_config_source);
                remote.remote_metadata_has_tools = remote_metadata_has_tools;
                *t = remote;
            }
        }

        Ok(())
    }

    /// Clone and resolve a task map so consumers can retry alias/dependency
    /// matching against metadata that only exists in remote headers.
    pub async fn fetch_task_map(
        &self,
        config: &Arc<Config>,
        tasks: &BTreeMap<String, Task>,
    ) -> Result<BTreeMap<String, Task>> {
        let mut resolved = tasks.values().cloned().collect::<Vec<_>>();
        self.fetch_tasks(config, &mut resolved).await?;
        Ok(resolved
            .into_iter()
            .map(|task| (task.name.clone(), task))
            .collect())
    }

    /// Check if a source path is a remote task file (git or http/https)
    pub fn is_remote_source(source: &str) -> bool {
        source.starts_with("git::")
            || source.starts_with("http://")
            || source.starts_with("https://")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::env_directive::EnvDirective;
    use crate::task::TaskToolValue;
    use std::path::PathBuf;

    static REMOTE_TASK_ARTIFACT_TEST_LOCK: tokio::sync::Mutex<()> =
        tokio::sync::Mutex::const_new(());

    #[tokio::test]
    async fn nested_artifact_scopes_clear_only_after_the_last_scope() {
        let _test_lock = REMOTE_TASK_ARTIFACT_TEST_LOCK.lock().await;
        assert_eq!(*REMOTE_TASK_ARTIFACT_SCOPES.lock().unwrap(), 0);
        REMOTE_TASK_ARTIFACTS.clear();

        let outer = RemoteTaskArtifactsGuard::new();
        let inner = RemoteTaskArtifactsGuard::new();
        REMOTE_TASK_ARTIFACTS.insert(
            RemoteTaskArtifactKey {
                config_source: PathBuf::from("scope-test.toml"),
                task_name: "scope-test".into(),
                source: "https://example.test/task".into(),
            },
            Arc::new(OnceCell::new()),
        );

        drop(inner);
        assert_eq!(REMOTE_TASK_ARTIFACTS.len(), 1);

        drop(outer);
        assert!(REMOTE_TASK_ARTIFACTS.is_empty());
    }

    #[tokio::test]
    async fn test_fetch_remote_task_parses_headers_and_applies_toml_overlay() {
        let _test_lock = REMOTE_TASK_ARTIFACT_TEST_LOCK.lock().await;
        let _artifacts = RemoteTaskArtifactsGuard::new();
        let mut server = mockito::Server::new_async().await;
        let remote = server
            .mock("GET", "/task")
            .with_status(200)
            .with_body(
                r#"#!/usr/bin/env bash
#MISE description="remote description"
#MISE hide=true
#MISE quiet=true
#MISE tools={node="24", python="3.12"}
echo ok
"#,
            )
            .expect(1)
            .create_async()
            .await;

        let config = Config::get().await.unwrap();
        let config_root = tempfile::tempdir().unwrap();
        let source = format!("{}/task", server.url());
        let mut task = Task {
            name: "lint".into(),
            display_name: "lint".into(),
            description: "toml description".into(),
            config_source: config_root.path().join("mise.toml"),
            config_root: Some(config_root.path().to_path_buf()),
            file: Some(PathBuf::from(&source)),
            args: vec!["--fix".into()],
            tools: [("python".into(), TaskToolValue::String("3.13".into()))]
                .into_iter()
                .collect(),
            ..Default::default()
        };

        let mut tasks = vec![task];
        TaskFetcher::new(true)
            .fetch_tasks(&config, &mut tasks)
            .await
            .unwrap();
        task = tasks.pop().unwrap();

        remote.assert_async().await;
        assert_eq!(task.name, "lint");
        assert_eq!(task.display_name, "lint");
        assert_eq!(task.description, "toml description");
        assert!(task.hide);
        assert!(task.quiet);
        assert_eq!(task.args, ["--fix"]);
        assert_eq!(task.config_root.as_deref(), Some(config_root.path()));
        assert_eq!(task.remote_file_source.as_deref(), Some(source.as_str()));
        assert!(task.is_remote());
        assert_eq!(
            task.tools.get("node"),
            Some(&TaskToolValue::String("24".into()))
        );
        assert_eq!(
            task.tools.get("python"),
            Some(&TaskToolValue::String("3.13".into()))
        );
    }

    #[tokio::test]
    async fn test_fetch_cached_remote_task_parses_headers() {
        let mut server = mockito::Server::new_async().await;
        let remote = server
            .mock("GET", "/cached-task")
            .with_status(200)
            .with_body("#!/usr/bin/env bash\n#MISE description=\"from cache\"\necho ok\n")
            .expect(1)
            .create_async()
            .await;

        let config = Config::get().await.unwrap();
        let config_root = tempfile::tempdir().unwrap();
        let source = format!("{}/cached-task", server.url());
        let new_task = || Task {
            name: "cached".into(),
            config_source: config_root.path().join("mise.toml"),
            config_root: Some(config_root.path().to_path_buf()),
            file: Some(PathBuf::from(&source)),
            ..Default::default()
        };

        for _ in 0..2 {
            let mut tasks = vec![new_task()];
            TaskFetcher::new(false)
                .fetch_tasks(&config, &mut tasks)
                .await
                .unwrap();
            assert_eq!(tasks[0].description, "from cache");
        }

        remote.assert_async().await;
    }

    #[tokio::test]
    async fn test_no_cache_remote_task_reuses_logical_snapshot() {
        let _test_lock = REMOTE_TASK_ARTIFACT_TEST_LOCK.lock().await;
        let _artifacts = RemoteTaskArtifactsGuard::new();
        let mut server = mockito::Server::new_async().await;
        let remote = server
            .mock("GET", "/command-snapshot")
            .with_status(200)
            .with_body("#!/usr/bin/env bash\n#MISE env={REMOTE_REVISION=\"one\"}\necho snapshot\n")
            .expect(1)
            .create_async()
            .await;

        let config = Config::get().await.unwrap();
        let config_root = tempfile::tempdir().unwrap();
        let source = format!("{}/command-snapshot", server.url());
        let task = Task {
            name: "remote-dependency".into(),
            config_source: config_root.path().join("mise.toml"),
            config_root: Some(config_root.path().to_path_buf()),
            file: Some(PathBuf::from(&source)),
            ..Default::default()
        };

        let mut first = vec![task.clone()];
        TaskFetcher::new(true)
            .fetch_tasks(&config, &mut first)
            .await
            .unwrap();
        let config = Config::reset().await.unwrap();
        let mut second = vec![task];
        TaskFetcher::new(true)
            .fetch_tasks(&config, &mut second)
            .await
            .unwrap();

        remote.assert_async().await;
        assert_eq!(first[0].file, second[0].file);
        assert_eq!(first[0].env.0.len(), 1);
        assert_eq!(second[0].env.0.len(), 1);
    }

    #[tokio::test]
    async fn test_remote_header_templates_use_original_runtime_context() {
        let _test_lock = REMOTE_TASK_ARTIFACT_TEST_LOCK.lock().await;
        let _artifacts = RemoteTaskArtifactsGuard::new();
        let mut server = mockito::Server::new_async().await;
        let remote = server
            .mock("GET", "/templated-task")
            .with_status(200)
            .with_body(
                "#!/usr/bin/env bash\n#MISE description=\"{{vars.runtime_value}}\"\necho ok\n",
            )
            .expect(1)
            .create_async()
            .await;

        let config = Config::get().await.unwrap();
        let config_root = tempfile::tempdir().unwrap();
        let source = format!("{}/templated-task", server.url());
        let config_source = config_root.path().join("mise.toml");
        let mut tasks = vec![Task {
            name: "templated".into(),
            config_source: config_source.clone(),
            config_root: Some(config_root.path().to_path_buf()),
            file: Some(PathBuf::from(&source)),
            overlay_vars: vec![(
                EnvDirective::Val(
                    "runtime_value".into(),
                    "rendered from runtime context".into(),
                    Default::default(),
                ),
                config_source,
            )],
            ..Default::default()
        }];

        TaskFetcher::new(true)
            .fetch_tasks(&config, &mut tasks)
            .await
            .unwrap();

        remote.assert_async().await;
        assert_eq!(tasks[0].description, "rendered from runtime context");
    }
}
