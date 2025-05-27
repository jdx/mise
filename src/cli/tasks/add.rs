use crate::config::config_file;
use crate::task::Task;
use crate::{config, file};
use eyre::Result;
use std::path::MAIN_SEPARATOR_STR;
use toml_edit::Item;

/// Create a new task
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct TasksAdd {
    /// Tasks name to add
    #[clap()]
    task: String,

    /// Description of the task
    #[clap(long)]
    description: Option<String>,
    /// Other names for the task
    #[clap(long, short)]
    alias: Vec<String>,
    /// Dependencies to run after the task runs
    #[clap(long)]
    depends_post: Vec<String>,
    /// Wait for these tasks to complete if they are to run
    #[clap(long, short)]
    wait_for: Vec<String>,
    /// Run the task in a specific directory
    #[clap(long, short = 'D')]
    dir: Option<String>,
    /// Hide the task from `mise task` and completions
    #[clap(long, short = 'H')]
    hide: bool,
    /// Directly connect stdin/stdout/stderr
    #[clap(long, short)]
    raw: bool,
    /// Glob patterns of files this task uses as input
    #[clap(long, short)]
    sources: Vec<String>,
    /// Glob patterns of files this task creates, to skip if they are not modified
    #[clap(long)]
    outputs: Vec<String>,
    /// Run the task in a specific shell
    #[clap(long)]
    shell: Option<String>,
    /// Do not print the command before running
    #[clap(long, short)]
    quiet: bool,
    /// Do not print the command or its output
    #[clap(long)]
    silent: bool,
    // TODO
    // env: Vec<String>,
    // tools: Vec<String>,
    /// Add dependencies to the task
    #[clap(long, short)]
    depends: Vec<String>,
    /// Command to run on windows
    #[clap(long)]
    run_windows: Option<String>,

    /// Create a file task instead of a toml task
    #[clap(long, short)]
    file: bool,

    #[clap(last = true)]
    run: Vec<String>,
}

impl TasksAdd {
    pub async fn run(self) -> Result<()> {
        if self.file {
            let mut path = Task::task_dir()
                .await
                .join(self.task.replace(':', MAIN_SEPARATOR_STR));
            if path.is_dir() {
                path = path.join("_default");
            }
            let mut lines = vec![format!(
                "#!/usr/bin/env {}",
                self.shell.clone().unwrap_or("bash".into())
            )];
            if !self.depends.is_empty() {
                lines.push("#MISE depends=[\"".to_string() + &self.depends.join("\", \"") + "\"]");
            }
            if !self.depends_post.is_empty() {
                lines.push(
                    "#MISE depends_post=[\"".to_string()
                        + &self.depends_post.join("\", \"")
                        + "\"]",
                );
            }
            if !self.wait_for.is_empty() {
                lines
                    .push("#MISE wait_for=[\"".to_string() + &self.wait_for.join("\", \"") + "\"]");
            }
            if !self.alias.is_empty() {
                lines.push("#MISE alias=[\"".to_string() + &self.alias.join("\", \"") + "\"]");
            }
            if let Some(description) = &self.description {
                lines.push("#MISE description=\"".to_string() + description + "\"");
            }
            if self.dir.is_some() {
                lines.push("#MISE dir=".to_string() + &self.dir.unwrap());
            }
            if self.hide {
                lines.push("#MISE hide=true".to_string());
            }
            if self.raw {
                lines.push("#MISE raw=true".to_string());
            }
            if !self.sources.is_empty() {
                lines.push("#MISE sources=[\"".to_string() + &self.sources.join("\", \"") + "\"]");
            }
            if !self.outputs.is_empty() {
                lines.push("#MISE outputs=[\"".to_string() + &self.outputs.join("\", \"") + "\"]");
            }
            if self.quiet {
                lines.push("#MISE quiet=true".to_string());
            }
            if self.silent {
                lines.push("#MISE silent=true".to_string());
            }
            lines.push("set -euxo pipefail".into());
            lines.push("".into());
            if !self.run.is_empty() {
                lines.push(self.run.join(" "));
                lines.push("".into());
            }
            file::create_dir_all(path.parent().unwrap())?;
            file::write(&path, lines.join("\n"))?;
            file::make_executable(&path)?;
        } else {
            let path = config::local_toml_config_path();
            let mut doc: toml_edit::DocumentMut =
                file::read_to_string(&path).unwrap_or_default().parse()?;
            let tasks = doc
                .entry("tasks")
                .or_insert_with(|| {
                    let mut table = toml_edit::Table::new();
                    table.set_implicit(true);
                    Item::Table(table)
                })
                .as_table_mut()
                .unwrap();
            let mut task = toml_edit::Table::new();
            if !self.depends.is_empty() {
                let mut depends = toml_edit::Array::new();
                for dep in &self.depends {
                    depends.push(dep);
                }
                task.insert("depends", Item::Value(depends.into()));
            }
            if !self.depends_post.is_empty() {
                let mut depends_post = toml_edit::Array::new();
                for dep in &self.depends_post {
                    depends_post.push(dep);
                }
                task.insert("depends_post", Item::Value(depends_post.into()));
            }
            if !self.wait_for.is_empty() {
                let mut wait_for = toml_edit::Array::new();
                for dep in &self.wait_for {
                    wait_for.push(dep);
                }
                task.insert("wait_for", Item::Value(wait_for.into()));
            }
            if self.description.is_some() {
                task.insert("description", self.description.unwrap().into());
            }
            if !self.alias.is_empty() {
                let mut alias = toml_edit::Array::new();
                for a in &self.alias {
                    alias.push(a);
                }
                task.insert("alias", Item::Value(alias.into()));
            }
            if self.dir.is_some() {
                task.insert("dir", self.dir.unwrap().into());
            }
            if self.hide {
                task.insert("hide", true.into());
            }
            if self.raw {
                task.insert("raw", true.into());
            }
            if !self.sources.is_empty() {
                let mut sources = toml_edit::Array::new();
                for source in &self.sources {
                    sources.push(source);
                }
                task.insert("sources", Item::Value(sources.into()));
            }
            if !self.outputs.is_empty() {
                let mut outputs = toml_edit::Array::new();
                for output in &self.outputs {
                    outputs.push(output);
                }
                task.insert("outputs", Item::Value(outputs.into()));
            }
            if self.shell.is_some() {
                task.insert("shell", self.shell.unwrap().into());
            }
            if self.quiet {
                task.insert("quiet", true.into());
            }
            if self.silent {
                task.insert("silent", true.into());
            }
            if !self.run.is_empty() {
                task.insert("run", shell_words::join(&self.run).into());
            }
            tasks.insert(&self.task, Item::Table(task));
            file::write(&path, doc.to_string())?;
            config_file::trust(&config_file::config_trust_root(&path))?;
        }

        Ok(())
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise task add pre-commit --depends "test" --depends "render" -- echo pre-commit</bold>
"#
);
