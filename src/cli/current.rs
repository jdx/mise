use console::style;
use eyre::{bail, Result};

use crate::backend;
use crate::backend::Backend;
use crate::cli::args::BackendArg;
use crate::config::Config;
use crate::toolset::{Toolset, ToolsetBuilder};

/// Shows current active and installed runtime versions
///
/// This is similar to `mise ls --current`, but this only shows the runtime
/// and/or version. It's designed to fit into scripts more easily.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, hide = true, after_long_help = AFTER_LONG_HELP)]
pub struct Current {
    /// Plugin to show versions of
    /// e.g.: ruby, node, cargo:eza, npm:prettier, etc.
    #[clap()]
    plugin: Option<BackendArg>,
}

impl Current {
    pub fn run(self) -> Result<()> {
        let config = Config::try_get()?;
        let ts = ToolsetBuilder::new().build(&config)?;
        match &self.plugin {
            Some(fa) => {
                let backend = backend::get(fa);
                if let Some(plugin) = backend.plugin() {
                    if !plugin.is_installed() {
                        bail!("Plugin {fa} is not installed");
                    }
                }
                self.one(ts, backend.as_ref())
            }
            None => self.all(ts),
        }
    }

    fn one(&self, ts: Toolset, tool: &dyn Backend) -> Result<()> {
        if let Some(plugin) = tool.plugin() {
            if !plugin.is_installed() {
                warn!("Plugin {} is not installed", tool.id());
                return Ok(());
            }
        }
        match ts
            .list_versions_by_plugin()
            .into_iter()
            .find(|(p, _)| p.id() == tool.id())
        {
            Some((_, versions)) => {
                miseprintln!(
                    "{}",
                    versions
                        .iter()
                        .map(|v| v.version.to_string())
                        .collect::<Vec<_>>()
                        .join(" ")
                );
            }
            None => {
                warn!(
                    "Plugin {} does not have a version set",
                    style(tool.id()).blue().for_stderr()
                );
            }
        };
        Ok(())
    }

    fn all(&self, ts: Toolset) -> Result<()> {
        for (plugin, versions) in ts.list_versions_by_plugin() {
            if versions.is_empty() {
                continue;
            }
            for tv in versions {
                if !plugin.is_version_installed(tv, true) {
                    let source = ts.versions.get(&tv.backend).unwrap().source.clone();
                    warn!(
                        "{}@{} is specified in {}, but not installed",
                        &tv.backend, &tv.version, &source
                    );
                    hint!(
                        "tools_missing",
                        "install missing tools with",
                        "mise install"
                    );
                }
            }
            miseprintln!(
                "{} {}",
                &plugin.id(),
                versions
                    .iter()
                    .map(|v| v.version.to_string())
                    .collect::<Vec<_>>()
                    .join(" ")
            );
        }
        Ok(())
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>
    
    # outputs `.tool-versions` compatible format
    $ <bold>mise current</bold>
    python 3.11.0 3.10.0
    shfmt 3.6.0
    shellcheck 0.9.0
    node 20.0.0

    $ <bold>mise current node</bold>
    20.0.0

    # can output multiple versions
    $ <bold>mise current python</bold>
    3.11.0 3.10.0
"#
);
