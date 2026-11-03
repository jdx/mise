use console::style;
use eyre::{Result, bail};

use crate::args::BackendArg;
use crate::backend::Backend;
use crate::config::Config;
use crate::toolset::{Toolset, ToolsetBuilder};

/// Show the active tool versions
///
/// Similar to `mise ls --current`, but prints only the tool and its versions, which
/// is easier to use in scripts. Versions that are not installed are printed too;
/// when listing every tool, mise also warns about them.
#[derive(Debug, usage_rs::Args)]
#[usage(
    verbatim_doc_comment,
    hide = true,
    example(
        r###"mise current
python 3.13.1 3.12.8
shfmt 3.10.0
node 24.11.0"###,
        help = r###"Print every active tool in `.tool-versions` format"###
    ),
    example(
        r###"mise current node
24.11.0"###,
        help = r###"Print the active node version"###
    ),
    example(
        r###"mise current python
3.13.1 3.12.8"###,
        help = r###"A tool can have several active versions"###
    )
)]
pub(crate) struct Current {
    /// Only show this tool, such as `node` or `npm:prettier`
    #[usage()]
    plugin: Option<BackendArg>,
}

impl Current {
    pub(crate) async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        let ts = ToolsetBuilder::new().build(&config).await?;
        match &self.plugin {
            Some(ba) => {
                if let Some(plugin) = ba.backend()?.plugin()
                    && !plugin.is_installed()
                {
                    bail!("Plugin {ba} is not installed");
                }
                self.one(ts, ba.backend()?.as_ref()).await
            }
            None => self.all(ts).await,
        }
    }

    async fn one(&self, ts: Toolset, tool: &dyn Backend) -> Result<()> {
        if let Some(plugin) = tool.plugin()
            && !plugin.is_installed()
        {
            warn!("Plugin {} is not installed", tool.id());
            return Ok(());
        }
        match ts
            .list_versions_by_plugin()
            .into_iter()
            .find(|(p, _)| p.id() == tool.id())
        {
            Some((_, tvl)) => {
                let versions = tvl.os_supported_versions().collect::<Vec<_>>();
                if versions.is_empty() {
                    warn!(
                        "Plugin {} does not have a version set",
                        style(tool.id()).blue().for_stderr()
                    );
                    return Ok(());
                }
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

    async fn all(&self, ts: Toolset) -> Result<()> {
        let config = Config::get().await?;
        for (plugin, tvl) in ts.list_versions_by_plugin() {
            let versions = tvl.os_supported_versions().collect::<Vec<_>>();
            if versions.is_empty() {
                continue;
            }
            for tv in &versions {
                if !plugin.is_version_installed(&config, tv, true) {
                    let source = ts.versions.get(tv.ba()).unwrap().source.clone();
                    warn!(
                        "{}@{} is specified in {}, but not installed",
                        &tv.ba(),
                        &tv.version,
                        &source
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
