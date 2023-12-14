use color_eyre::eyre::Result;
use console::style;
use rayon::prelude::*;

use crate::config::{Config, Settings};

use crate::plugins::{unalias_plugin, PluginName};
use crate::ui::multi_progress_report::MultiProgressReport;

/// Updates a plugin to the latest version
///
/// note: this updates the plugin itself, not the runtime versions
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, alias = "upgrade", after_long_help = AFTER_LONG_HELP)]
pub struct Update {
    /// Plugin(s) to update
    #[clap()]
    plugin: Option<Vec<PluginName>>,

    /// Number of jobs to run in parallel
    /// Default: 4
    #[clap(long, short, verbatim_doc_comment)]
    jobs: Option<usize>,
}

impl Update {
    pub fn run(self, config: &Config) -> Result<()> {
        let plugins: Vec<_> = match self.plugin {
            Some(plugins) => plugins
                .into_iter()
                .map(|p| {
                    let (p, ref_) = match p.split_once('#') {
                        Some((p, ref_)) => (p, Some(ref_.to_string())),
                        None => (p.as_str(), None),
                    };
                    let p = unalias_plugin(p);
                    let plugin = config.get_or_create_plugin(p);
                    Ok((plugin.clone(), ref_))
                })
                .collect::<Result<_>>()?,
            None => config
                .external_plugins()
                .into_iter()
                .map(|(_, p)| (p, None))
                .collect::<Vec<_>>(),
        };

        // let queue = Mutex::new(plugins);
        let settings = Settings::try_get()?;
        let mpr = MultiProgressReport::new();
        rayon::ThreadPoolBuilder::new()
            .num_threads(self.jobs.unwrap_or(settings.jobs))
            .build()?
            .install(|| {
                plugins.into_par_iter().for_each(|(plugin, ref_)| {
                    let prefix = format!("plugin:{}", style(plugin.name()).blue().for_stderr());
                    let pr = mpr.add(&prefix);
                    plugin.update(pr.as_ref(), ref_).unwrap();
                });
                Ok(())
            })
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>
  $ <bold>rtx plugins update</bold>            # update all plugins
  $ <bold>rtx plugins update node</bold>       # update only node
  $ <bold>rtx plugins update node#beta</bold>  # specify a ref
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
            "https://github.com/rtx-plugins/rtx-tiny.git"
        );
        // assert_cli!("p", "update"); tested in e2e
        assert_cli!("plugins", "update", "tiny");
    }
}
