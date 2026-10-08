use eyre::Result;

use crate::args::ToolArg;
use crate::config::Config;
use crate::errors::Error;
use crate::toolset::ToolsetBuilder;

/// Show the install directory of a tool version
///
/// Fails if no matching version is installed.
#[derive(Debug, usage_rs::Args)]
#[usage(
    verbatim_doc_comment,
    example(
        r###"mise where node@20
~/.local/share/mise/installs/node/20.0.0"###,
        help = r###"Show the newest installed node 20.x"###
    ),
    example(
        r###"mise where node
~/.local/share/mise/installs/node/20.0.0"###,
        help = r###"Show the active node, or the newest installed node if no config requests one"###
    )
)]
pub(crate) struct Where {
    /// Tool to look up, such as ruby@3
    ///
    /// With a version prefix, shows the newest installed version that matches it.
    /// Otherwise, shows the active installed version.
    #[usage(value_name = "TOOL@VERSION")]
    tool: ToolArg,

    /// the version prefix to use when querying the latest version
    /// same as the first argument after the "@"
    /// used for asdf compatibility
    #[usage(hide = true, verbatim_doc_comment)]
    asdf_version: Option<String>,
}

impl Where {
    pub(crate) async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        // A version named on the command line carries no install options.
        let named_here = self.tool.tvr.is_some() || self.asdf_version.is_some();
        // The options a configuration sets for the tool apply to a version named
        // here too (as `tool@version` or `tool version`), as they do for
        // `mise x tool@version`.
        let named = match (self.tool.tvr.clone(), &self.asdf_version) {
            (Some(tvr), _) => Some(tvr),
            (None, Some(version)) => self.tool.clone().with_version(version).tvr,
            (None, None) => None,
        };
        let tvr = match named {
            Some(tvr) => crate::toolset::apply_config_options_to_runtime_arg(
                config.get_tool_request_set().await?,
                tvr,
            ),
            None => {
                let ts = ToolsetBuilder::new().build(&config).await?;
                match ts.versions.get(self.tool.ba.as_ref()) {
                    Some(tvl) => tvl.os_supported_requests().next().cloned().ok_or_else(|| {
                        eyre::eyre!("{} does not have an active version", self.tool.ba)
                    })?,
                    None => self.tool.with_version("latest").tvr.unwrap(),
                }
            }
        };

        let tv = tvr.resolve(&config, &Default::default()).await?;

        if tv.backend()?.is_version_installed(&config, &tv, true) {
            miseprintln!("{}", tv.install_path().to_string_lossy());
            Ok(())
        } else if let Some(dir) = (named_here && !tv.resolved_from_lockfile())
            .then(|| installed_variant(&tv))
            .transpose()?
            .flatten()
        {
            // The version was installed with options a configuration sets; the one
            // named here carries none.
            miseprintln!("{}", dir.to_string_lossy());
            Ok(())
        } else {
            Err(Error::VersionNotInstalled(
                Box::new(tv.ba().clone()),
                tv.version,
            ))?
        }
    }
}

/// The installation of `tv`'s tool and version made with install options, when the
/// version as named installs nothing of its own and there is exactly one. With
/// several, which one is meant depends on options the command line did not give,
/// so it is an error that lists them rather than a guess (the version link names
/// only the most recent one).
fn installed_variant(tv: &crate::toolset::ToolVersion) -> Result<Option<std::path::PathBuf>> {
    let mut variants = crate::install_layout::resolver::variants_of(tv);
    if variants.len() > 1 {
        let list = variants
            .iter()
            .map(|dir| format!("  {}", crate::file::display_path(dir)))
            .collect::<Vec<_>>()
            .join("\n");
        eyre::bail!(
            "{} is installed with several sets of options:\n{list}\n\
             Run this where the configuration that sets them applies, or name them, \
             for example `{}[option=value]@{}`",
            tv.style(),
            tv.ba().short,
            tv.version
        );
    }
    Ok(variants.pop())
}
