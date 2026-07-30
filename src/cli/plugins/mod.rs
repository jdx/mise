use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;

use clap::Subcommand;
use eyre::{Report, Result, WrapErr, eyre};
use tokio::task::{Id, JoinSet};

use crate::config::Config;

pub(crate) mod install;
mod link;
mod ls;
mod ls_remote;
mod uninstall;
mod update;

type PluginTaskResult = Result<()>;
type PluginTaskNames = HashMap<Id, String>;

fn take_plugin_name(task_names: &mut PluginTaskNames, id: Id) -> String {
    task_names
        .remove(&id)
        .expect("plugin task name should be registered before spawning")
}

fn spawn_plugin_task<F>(
    tasks: &mut JoinSet<PluginTaskResult>,
    task_names: &mut PluginTaskNames,
    plugin: impl Into<String>,
    task: F,
) where
    F: Future<Output = PluginTaskResult> + Send + 'static,
{
    let task = tasks.spawn(task);
    task_names.insert(task.id(), plugin.into());
}

async fn join_plugin_tasks(
    mut tasks: JoinSet<PluginTaskResult>,
    mut task_names: PluginTaskNames,
    operation: &'static str,
) -> Result<()> {
    let mut failures: Vec<(String, Report)> = Vec::new();

    while let Some(result) = tasks.join_next_with_id().await {
        match result {
            Ok((id, Ok(()))) => {
                take_plugin_name(&mut task_names, id);
            }
            Ok((id, Err(err))) => {
                let plugin = take_plugin_name(&mut task_names, id);
                failures.push((plugin, err));
            }
            Err(err) => {
                let plugin = take_plugin_name(&mut task_names, err.id());
                failures.push((plugin, err.into()));
            }
        }
    }

    match failures.len() {
        0 => Ok(()),
        1 => {
            let (plugin, err) = failures.pop().unwrap();
            Err(err).wrap_err_with(|| format!("[{plugin}] plugin {operation}"))
        }
        _ => {
            failures.sort_by(|(a, _), (b, _)| a.cmp(b));
            let names = failures
                .iter()
                .map(|(plugin, _)| plugin.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            let details = failures
                .iter()
                .map(|(plugin, err)| format!("{plugin}: {err:#}"))
                .collect::<Vec<_>>()
                .join("\n\n");
            Err(eyre!("Failed to {operation} plugins: {names}\n\n{details}"))
        }
    }
}

#[derive(Debug, clap::Args)]
#[clap(about = "Manage plugins", visible_alias = "p", aliases = ["plugin", "plugin-list"])]
pub struct Plugins {
    #[clap(subcommand)]
    command: Option<Commands>,

    /// list all available remote plugins
    ///
    /// same as `mise plugins ls-remote`
    #[clap(short, long, hide = true)]
    pub all: bool,

    /// The built-in plugins only
    /// Normally these are not shown
    #[clap(short, long, verbatim_doc_comment, conflicts_with = "all")]
    pub core: bool,

    /// Show the git url for each plugin
    /// e.g.: https://github.com/mise-plugins/vfox-cmake.git
    #[clap(short, long, alias = "url", verbatim_doc_comment)]
    pub urls: bool,

    /// Show the git refs for each plugin
    /// e.g.: main 1234abc
    #[clap(long, hide = true, verbatim_doc_comment)]
    pub refs: bool,

    /// List installed plugins
    ///
    /// This is the default behavior but can be used with --core
    /// to show core and user plugins
    #[clap(long, verbatim_doc_comment, conflicts_with = "all")]
    pub user: bool,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Install(install::PluginsInstall),
    Link(link::PluginsLink),
    Ls(ls::PluginsLs),
    LsRemote(ls_remote::PluginsLsRemote),
    Uninstall(uninstall::PluginsUninstall),
    Update(update::Update),
}

impl Commands {
    pub async fn run(self, config: &Arc<Config>) -> Result<()> {
        match self {
            Self::Install(cmd) => cmd.run(config).await,
            Self::Link(cmd) => cmd.run().await,
            Self::Ls(cmd) => cmd.run(config).await,
            Self::LsRemote(cmd) => cmd.run(config).await,
            Self::Uninstall(cmd) => cmd.run().await,
            Self::Update(cmd) => cmd.run().await,
        }
    }
}

impl Plugins {
    pub async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        let cmd = self.command.unwrap_or(Commands::Ls(ls::PluginsLs {
            all: self.all,
            core: self.core,
            refs: self.refs,
            urls: self.urls,
            user: self.user,
            outdated: false,
        }));

        cmd.run(&config).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    #[tokio::test]
    async fn plugin_tasks_finish_after_an_earlier_failure() {
        let completed = Arc::new(AtomicBool::new(false));
        let mut tasks = JoinSet::new();
        let mut task_names = PluginTaskNames::new();
        spawn_plugin_task(&mut tasks, &mut task_names, "failed", async {
            Err(eyre!("failed immediately"))
        });
        spawn_plugin_task(&mut tasks, &mut task_names, "completed", {
            let completed = completed.clone();
            async move {
                tokio::time::sleep(Duration::from_millis(10)).await;
                completed.store(true, Ordering::SeqCst);
                Ok(())
            }
        });

        let err = join_plugin_tasks(tasks, task_names, "update")
            .await
            .unwrap_err();

        assert!(completed.load(Ordering::SeqCst));
        assert_eq!(
            format!("{err:#}"),
            "[failed] plugin update: failed immediately"
        );
    }

    #[tokio::test]
    async fn plugin_task_failures_are_reported_in_name_order() {
        let mut tasks = JoinSet::new();
        let mut task_names = PluginTaskNames::new();
        spawn_plugin_task(&mut tasks, &mut task_names, "zeta", async {
            Err(eyre!("zeta detail"))
        });
        spawn_plugin_task(&mut tasks, &mut task_names, "alpha", async {
            Err(eyre!("alpha detail"))
        });

        let err = join_plugin_tasks(tasks, task_names, "install")
            .await
            .unwrap_err();

        assert_eq!(
            format!("{err:#}"),
            "Failed to install plugins: alpha, zeta\n\n\
             alpha: alpha detail\n\n\
             zeta: zeta detail"
        );
    }

    #[tokio::test]
    async fn single_plugin_failure_preserves_the_error_chain() {
        let mut tasks = JoinSet::new();
        let mut task_names = PluginTaskNames::new();
        spawn_plugin_task(&mut tasks, &mut task_names, "tiny", async {
            Err(eyre!("inner failure").wrap_err("outer context"))
        });

        let err = join_plugin_tasks(tasks, task_names, "update")
            .await
            .unwrap_err();

        assert_eq!(
            format!("{err:#}"),
            "[tiny] plugin update: outer context: inner failure"
        );
    }

    #[tokio::test]
    async fn panicked_plugin_task_does_not_cancel_other_tasks() {
        async fn panic_plugin_task() -> Result<()> {
            panic!("plugin task panic");
        }

        let completed = Arc::new(AtomicBool::new(false));
        let mut tasks = JoinSet::new();
        let mut task_names = PluginTaskNames::new();
        spawn_plugin_task(
            &mut tasks,
            &mut task_names,
            "panicked-plugin",
            panic_plugin_task(),
        );
        spawn_plugin_task(&mut tasks, &mut task_names, "completed", {
            let completed = completed.clone();
            async move {
                tokio::time::sleep(Duration::from_millis(10)).await;
                completed.store(true, Ordering::SeqCst);
                Ok(())
            }
        });

        let err = join_plugin_tasks(tasks, task_names, "update")
            .await
            .unwrap_err();

        assert!(completed.load(Ordering::SeqCst));
        assert!(format!("{err:#}").contains("[panicked-plugin] plugin update"));
    }
}
