use crate::config::config_file::{trust_check, trust_check_remote_fetch};
use crate::config::{Config, Settings};
use crate::task::task_file_providers::TaskFileProvidersBuilder;
use crate::task::{Task, script_header_has_decoded_template};
use eyre::{Result, WrapErr, bail};
use std::collections::BTreeMap;
use std::sync::Arc;

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
    /// Passive discovery commands use this because merely listing tasks is not
    /// an explicit request to execute a configured remote task.
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

                let provider = task_file_providers.get_provider(&source);

                if provider.is_none() {
                    bail!("No provider found for file: {}", source);
                }

                let artifact = provider
                    .unwrap()
                    .get_local_artifact(&source)
                    .await
                    .wrap_err_with(|| format!("failed to fetch remote task {source}"))?;
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
                    defining_cf,
                )
                .wrap_err_with(|| format!("failed to parse remote task metadata from {source}"))?;
                // Parsing headers is inert. Check the decoded Task instead of
                // guessing its format from the fetched filename: Git task scripts
                // may legitimately end in `.toml`, and escaped TOML delimiters
                // only become visible after header parsing.
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
                remote
                    .remote_artifact_cleanups
                    .extend(original.remote_artifact_cleanups);
                if let Some(cleanup) = artifact.cleanup {
                    remote.remote_artifact_cleanups.push(cleanup);
                }
                *t = remote;
            }
        }

        Ok(())
    }

    /// Clone and resolve a task map so consumers can retry alias/dependency
    /// matching against metadata that only exists in remote #MISE headers.
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
    use std::{fs, path::PathBuf};

    #[test]
    fn test_remote_script_template_detection_ignores_toml_extension() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("remote-task.toml");
        fs::write(
            &path,
            r#"#!/usr/bin/env bash
#MISE description="\u007b\u007b exec(command='echo unsafe') \u007d\u007d"
echo ok
"#,
        )
        .unwrap();

        let task =
            Task::from_path_unrendered_with_cf(&path, root.path(), root.path(), None).unwrap();

        assert!(task.has_render_templates());
        assert!(script_header_has_decoded_template(
            &fs::read_to_string(path).unwrap()
        ));
    }

    #[test]
    fn test_remote_script_detects_deferred_header_templates() {
        let confirm = r#"#!/usr/bin/env bash
#MISE confirm="\u007b\u007b exec(command='echo unsafe') \u007d\u007d"
echo ok
"#;
        let env = r#"#!/usr/bin/env bash
#MISE env={MARKER="{{ exec(command='echo unsafe') }}"}
echo ok
"#;

        assert!(script_header_has_decoded_template(confirm));
        assert!(script_header_has_decoded_template(env));
    }

    #[tokio::test]
    async fn test_fetch_remote_task_parses_headers_and_applies_toml_overlay() {
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
        assert_eq!(
            task.remote_config_source.as_deref(),
            Some(config_root.path().join("mise.toml").as_path())
        );
        assert!(task.remote_metadata_has_tools);
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
    async fn test_remote_header_templates_use_original_runtime_context() {
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

    #[tokio::test]
    async fn test_remote_header_error_identifies_source_url() {
        let mut server = mockito::Server::new_async().await;
        let remote = server
            .mock("GET", "/invalid-task")
            .with_status(200)
            .with_body("#!/usr/bin/env bash\n#MISE unsupported_remote_field=true\necho ok\n")
            .expect(1)
            .create_async()
            .await;

        let config = Config::get().await.unwrap();
        let config_root = tempfile::tempdir().unwrap();
        let source = format!("{}/invalid-task", server.url());
        let mut tasks = vec![Task {
            name: "invalid".into(),
            config_source: config_root.path().join("mise.toml"),
            config_root: Some(config_root.path().to_path_buf()),
            file: Some(PathBuf::from(&source)),
            ..Default::default()
        }];

        let error = TaskFetcher::new(true)
            .fetch_tasks(&config, &mut tasks)
            .await
            .unwrap_err();

        remote.assert_async().await;
        assert!(error.to_string().contains(&source));
        assert!(
            error
                .to_string()
                .contains("failed to parse remote task metadata")
        );
    }
}
