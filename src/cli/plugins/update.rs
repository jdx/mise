use std::sync::Arc;

use console::style;
use eyre::Result;
use tokio::{sync::Semaphore, task::JoinSet};

use crate::config::Settings;
use crate::plugins;
use crate::toolset::install_state;
use crate::ui::multi_progress_report::MultiProgressReport;

use super::{PluginTaskNames, PluginTaskResult, join_plugin_tasks, spawn_plugin_task};

/// Updates a plugin to the latest version
///
/// note: this updates the plugin itself, not the runtime versions
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, visible_aliases = ["up", "upgrade"], after_long_help = AFTER_LONG_HELP)]
pub struct Update {
    /// Plugin(s) to update
    #[clap()]
    plugin: Option<Vec<String>>,

    /// Number of jobs to run in parallel
    /// Default: 4
    #[clap(long, short, verbatim_doc_comment)]
    jobs: Option<usize>,
}

impl Update {
    pub async fn run(self) -> Result<()> {
        let plugins: Vec<_> = match self.plugin {
            Some(plugins) => plugins
                .into_iter()
                .map(|p| match p.split_once('#') {
                    Some((p, ref_)) => (p.to_string(), Some(ref_.to_string())),
                    None => (p, None),
                })
                .collect(),
            None => install_state::list_plugins()
                .keys()
                .map(|p| (p.clone(), None))
                .collect::<Vec<_>>(),
        };

        let settings = Settings::try_get()?;
        let mut jset: JoinSet<PluginTaskResult> = JoinSet::new();
        let mut task_names = PluginTaskNames::new();
        let semaphore = Arc::new(Semaphore::new(self.jobs.unwrap_or(settings.jobs)));
        for (short, ref_) in plugins {
            let semaphore = semaphore.clone();
            let plugin_name = short.clone();
            spawn_plugin_task(&mut jset, &mut task_names, plugin_name, async move {
                let _permit = semaphore.acquire_owned().await?;
                let plugin = plugins::get(&short)?;
                let prefix = format!("plugin:{}", style(plugin.name()).blue().for_stderr());
                let mpr = MultiProgressReport::get();
                let pr = mpr.add(&prefix);
                plugin.update(pr.as_ref(), ref_).await?;
                Ok(())
            });
        }

        join_plugin_tasks(jset, task_names, "update").await
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise plugins update</bold>              # update all plugins
    $ <bold>mise plugins update cmake</bold>       # update only cmake
    $ <bold>mise plugins update cmake#beta</bold>  # specify a ref
"#
);
