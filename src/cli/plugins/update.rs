use console::style;
use eyre::{Report, Result};
use rayon::prelude::*;
use std::sync::Mutex;

use crate::config::Settings;
use crate::plugins;
use crate::ui::multi_progress_report::MultiProgressReport;

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
    pub fn run(self) -> Result<()> {
        let plugins: Vec<_> = match self.plugin {
            Some(plugins) => plugins
                .into_iter()
                .map(|p| {
                    let (p, ref_) = match p.split_once('#') {
                        Some((p, ref_)) => (p, Some(ref_.to_string())),
                        None => (p.as_str(), None),
                    };
                    let plugin = plugins::get(p);
                    Ok((plugin.clone(), ref_))
                })
                .collect::<Result<_>>()?,
            None => plugins::list_external()
                .into_iter()
                .map(|p| (p, None))
                .collect::<Vec<_>>(),
        };

        // let queue = Mutex::new(plugins);
        let settings = Settings::try_get()?;
        let mpr = MultiProgressReport::get();

        rayon::ThreadPoolBuilder::new()
            .num_threads(self.jobs.unwrap_or(settings.jobs))
            .build()?
            .install(|| {
                let results = Mutex::new(Vec::new());
                plugins.into_par_iter().for_each(|(plugin, ref_)| {
                    let prefix = format!("plugin:{}", style(plugin.id()).blue().for_stderr());
                    let pr = mpr.add(&prefix);
                    let mut results = results.lock().unwrap();
                    let result = plugin.update(pr.as_ref(), ref_);
                    results.push(result)
                });

                let locked_results = results.lock().unwrap();
                if locked_results.iter().all(|r| r.is_ok()) {
                    return Ok(());
                }

                let errors: Vec<&Report> = locked_results
                    .iter()
                    .filter_map(|r| r.as_ref().err())
                    .collect();

                let report = errors
                    .into_iter()
                    .fold(eyre!("encountered errors during update"), |report, e| {
                        report.wrap_err(e)
                    });

                Err(report)
            })
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise plugins update</bold>            # update all plugins
    $ <bold>mise plugins update node</bold>       # update only node
    $ <bold>mise plugins update node#beta</bold>  # specify a ref
"#
);

#[cfg(test)]
mod tests {
    #[test]
    fn test_plugin_update() {
        assert_cli!(
            "plugin",
            "install",
            "tiny",
            "https://github.com/mise-plugins/rtx-tiny.git"
        );
        // assert_cli!("p", "update"); tested in e2e
        assert_cli!("plugins", "update", "tiny");
    }
}
