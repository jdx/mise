use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use color_eyre::eyre::{ContextCompat, Result, eyre};
use console::style;
use itertools::Itertools;

use crate::config::{Settings, config_file};
use crate::env::{MISE_DEFAULT_CONFIG_FILENAME, MISE_DEFAULT_TOOL_VERSIONS_FILENAME};
use crate::file::display_path;
use crate::{
    cli::args::{BackendArg, ToolArg},
    config::Config,
};
use crate::{env, file};

/// Sets/gets tool version in local .tool-versions or mise.toml
///
/// Use this to set a tool's version when within a directory
/// Use `mise global` to set a tool version globally
/// This uses `.tool-version` by default unless there is a `mise.toml` file or if `MISE_USE_TOML`
/// is set. A future v2 release of mise will default to using `mise.toml`.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, hide = true, alias = "l", after_long_help = AFTER_LONG_HELP)]
pub struct Local {
    /// Tool(s) to add to .tool-versions/mise.toml
    /// e.g.: node@20
    /// if this is a single tool with no version,
    /// the current value of .tool-versions/mise.toml will be displayed
    #[clap(value_name = "TOOL@VERSION", verbatim_doc_comment)]
    tool: Vec<ToolArg>,

    /// Recurse up to find a .tool-versions file rather than using the current directory only
    /// by default this command will only set the tool in the current directory ("$PWD/.tool-versions")
    #[clap(short, long, verbatim_doc_comment)]
    parent: bool,

    /// Save exact version to `.tool-versions`
    /// e.g.: `mise local --pin node@20` will save `node 20.0.0` to .tool-versions
    #[clap(long, verbatim_doc_comment, overrides_with = "fuzzy")]
    pin: bool,

    /// Save fuzzy version to `.tool-versions`
    /// e.g.: `mise local --fuzzy node@20` will save `node 20` to .tool-versions
    /// This is the default behavior unless MISE_ASDF_COMPAT=1
    #[clap(long, overrides_with = "pin")]
    fuzzy: bool,

    /// Remove the plugin(s) from .tool-versions
    #[clap(long, value_name = "PLUGIN", aliases = ["rm", "unset"])]
    remove: Option<Vec<BackendArg>>,

    /// Get the path of the config file
    #[clap(long)]
    path: bool,
}

impl Local {
    pub async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        let path = if self.parent {
            get_parent_path()?
        } else {
            get_path()?
        };
        local(
            &config,
            &path,
            self.tool,
            self.remove,
            self.pin,
            self.fuzzy,
            self.path,
        )
        .await
    }
}

fn get_path() -> Result<PathBuf> {
    let cwd = env::current_dir()?;
    let mise_toml = cwd.join(MISE_DEFAULT_CONFIG_FILENAME.as_str());
    let tool_versions = cwd.join(MISE_DEFAULT_TOOL_VERSIONS_FILENAME.as_str());
    if mise_toml.exists() {
        Ok(mise_toml)
    } else if tool_versions.exists() {
        Ok(tool_versions)
    } else if *env::MISE_USE_TOML {
        Ok(mise_toml)
    } else {
        Ok(tool_versions)
    }
}

pub fn get_parent_path() -> Result<PathBuf> {
    let mut filenames = vec![MISE_DEFAULT_CONFIG_FILENAME.as_str()];
    if !*env::MISE_USE_TOML {
        filenames.push(MISE_DEFAULT_TOOL_VERSIONS_FILENAME.as_str());
    }
    file::find_up(&env::current_dir()?, &filenames)
        .wrap_err_with(|| eyre!("no {} file found", filenames.join(" or "),))
}

#[allow(clippy::too_many_arguments)]
pub async fn local(
    config: &Arc<Config>,
    path: &Path,
    runtime: Vec<ToolArg>,
    remove: Option<Vec<BackendArg>>,
    pin: bool,
    fuzzy: bool,
    show_path: bool,
) -> Result<()> {
    deprecated!(
        "local",
        "mise local/global are deprecated. Use `mise use` instead."
    );
    let settings = Settings::try_get()?;
    let cf = config_file::parse_or_init(path)?;
    if show_path {
        miseprintln!("{}", path.display());
        return Ok(());
    }

    if let Some(plugins) = &remove {
        for plugin in plugins {
            cf.remove_tool(plugin)?;
        }
        let tools = plugins
            .iter()
            .map(|r| style(&r.short).blue().for_stderr().to_string())
            .join(" ");
        miseprintln!("{} {} {tools}", style("mise").dim(), display_path(path));
    }

    if !runtime.is_empty() {
        let runtimes = ToolArg::double_tool_condition(&runtime)?;
        if cf.display_runtime(&runtimes)? {
            return Ok(());
        }
        let pin = pin || (settings.asdf_compat && !fuzzy);
        cf.add_runtimes(config, &runtimes, pin).await?;
        let tools = runtimes.iter().map(|t| t.style()).join(" ");
        miseprintln!("{} {} {tools}", style("mise").dim(), display_path(path));
    }

    if !runtime.is_empty() || remove.is_some() {
        trace!("saving config file {}", display_path(path));
        cf.save()?;
    } else {
        miseprint!("{}", cf.dump()?)?;
    }

    Ok(())
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>
    # set the current version of node to 20.x for the current directory
    # will use a precise version (e.g.: 20.0.0) in .tool-versions file
    $ <bold>mise local node@20</bold>

    # set node to 20.x for the current project (recurses up to find .tool-versions)
    $ <bold>mise local -p node@20</bold>

    # set the current version of node to 20.x for the current directory
    # will use a fuzzy version (e.g.: 20) in .tool-versions file
    $ <bold>mise local --fuzzy node@20</bold>

    # removes node from .tool-versions
    $ <bold>mise local --remove=node</bold>

    # show the current version of node in .tool-versions
    $ <bold>mise local node</bold>
    20.0.0
"#
);
