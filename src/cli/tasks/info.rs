use std::sync::Arc;

use eyre::{Result, bail};
use itertools::Itertools;
use serde_json::json;

use crate::config::Config;
use crate::env_diff::EnvMap;
use crate::file::display_path;
use crate::task::Task;
use crate::ui::info;

/// Get information about a task
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct TasksInfo {
    /// Name of the task to get information about
    #[clap(verbatim_doc_comment)]
    pub task: String,
    /// Output in JSON format
    #[clap(short = 'J', long, verbatim_doc_comment)]
    pub json: bool,
}

impl TasksInfo {
    pub async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        let tasks = config.tasks().await?;

        let task = tasks
            .get(&self.task)
            .or_else(|| tasks.values().find(|task| task.display_name == self.task));

        if let Some(task) = task {
            let ts = config.get_toolset().await?;
            let env = task.render_env(&config, ts).await?;
            if self.json {
                self.display_json(&config, task, &env).await?;
            } else {
                self.display(&config, task, &env).await?;
            }
        } else {
            bail!(
                "Task not found: {}, use `mise tasks ls` to list all tasks",
                self.task
            );
        }

        Ok(())
    }

    async fn display(&self, config: &Arc<Config>, task: &Task, env: &EnvMap) -> Result<()> {
        info::inline_section("Task", &task.display_name)?;
        if !task.aliases.is_empty() {
            info::inline_section("Aliases", task.aliases.join(", "))?;
        }
        info::inline_section("Description", &task.description)?;
        info::inline_section("Source", display_path(&task.config_source))?;
        let mut properties = vec![];
        if task.hide {
            properties.push("hide");
        }
        if task.raw {
            properties.push("raw");
        }
        if !properties.is_empty() {
            info::inline_section("Properties", properties.join(", "))?;
        }
        if !task.depends.is_empty() {
            info::inline_section("Depends on", task.depends.iter().join(", "))?;
        }
        if !task.depends_post.is_empty() {
            info::inline_section("Depends post", task.depends_post.iter().join(", "))?;
        }
        if let Some(dir) = &task.dir {
            info::inline_section("Directory", display_path(dir))?;
        }
        if !task.sources.is_empty() {
            info::inline_section("Sources", task.sources.join(", "))?;
        }
        let outputs = task.outputs.paths(task);
        if !outputs.is_empty() {
            info::inline_section("Outputs", outputs.join(", "))?;
        }
        if let Some(file) = &task.file {
            info::inline_section("File", display_path(file))?;
        }
        if !task.run().is_empty() {
            info::section("Run", task.run().join("\n"))?;
        }
        if !task.env.is_empty() {
            info::section("Environment Variables", toml::to_string_pretty(&task.env)?)?;
        }
        let (spec, _) = task.parse_usage_spec(config, None, env).await?;
        if !spec.is_empty() {
            info::section("Usage Spec", &spec)?;
        }
        Ok(())
    }

    async fn display_json(&self, config: &Arc<Config>, task: &Task, env: &EnvMap) -> Result<()> {
        let (spec, _) = task.parse_usage_spec(config, None, env).await?;
        let o = json!({
            "name": task.display_name,
            "aliases": task.aliases,
            "description": task.description,
            "source": task.config_source,
            "depends": task.depends,
            "depends_post": task.depends_post,
            "wait_for": task.wait_for,
            "env": task.env,
            "dir": task.dir,
            "hide": task.hide,
            "raw": task.raw,
            "sources": task.sources,
            "outputs": task.outputs,
            "shell": task.shell,
            "quiet": task.quiet,
            "silent": task.silent,
            "tools": task.tools,
            "run": task.run(),
            "file": task.file,
            "usage_spec": spec,
        });
        miseprintln!("{}", serde_json::to_string_pretty(&o)?);
        Ok(())
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise tasks info</bold>
    Name: test
    Aliases: t
    Description: Test the application
    Source: ~/src/myproj/mise.toml

    $ <bold>mise tasks info test --json</bold>
    {
      "name": "test",
      "aliases": "t",
      "description": "Test the application",
      "source": "~/src/myproj/mise.toml",
      "depends": [],
      "env": {},
      "dir": null,
      "hide": false,
      "raw": false,
      "sources": [],
      "outputs": [],
      "run": [
        "echo \"testing!\""
      ],
      "file": null,
      "usage_spec": {}
    }
"#
);
