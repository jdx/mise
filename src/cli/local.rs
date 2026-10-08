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

/// Set or show local tool versions (deprecated; use `mise use`)
///
/// Writes `mise.toml` in the current directory, or an existing `.tool-versions`
/// there when there is no `mise.toml`. Set MISE_USE_TOML=0 to create
/// `.tool-versions` instead. Use `mise global` for the global config.
#[derive(Debug, usage_rs::Args)]
#[usage(
    verbatim_doc_comment,
    hide = true,
    alias = "l",
    example(
        r###"mise local --pin node@24"###,
        help = r###"Save the exact version, such as `node = "24.11.0"`, in the current directory"###
    ),
    example(
        r###"mise local -p node@24"###,
        help = r###"Save node 24 in the nearest `mise.toml` in this or a parent directory"###
    ),
    example(
        r###"mise local --fuzzy node@24"###,
        help = r###"Save node 24 as written, such as `node = "24"`, in the current directory"###
    ),
    example(
        r###"mise local --remove=node"###,
        help = r###"Remove node from the local config"###
    ),
    example(
        r###"mise local node
24.11.0"###,
        help = r###"Show the local node version"###
    )
)]
pub(crate) struct Local {
    /// Tools to add to `mise.toml` or `.tool-versions`, such as `node@24`
    ///
    /// With a single tool and no version, prints that tool's local version.
    #[usage(value_name = "TOOL@VERSION", verbatim_doc_comment)]
    tool: Vec<ToolArg>,

    /// Use the nearest `mise.toml` in this or a parent directory
    ///
    /// By default, only the current directory is used. With MISE_USE_TOML=0, a
    /// `.tool-versions` file also counts.
    #[usage(short, long, verbatim_doc_comment)]
    parent: bool,

    /// Save the version as written, such as `24`
    ///
    /// This is the default unless MISE_ASDF_COMPAT=1.
    #[usage(long, overrides = "pin")]
    fuzzy: bool,

    /// Print the path of the config file
    #[usage(long)]
    path: bool,

    /// Save the exact version, such as `24.11.0`
    #[usage(long, verbatim_doc_comment, overrides = "fuzzy")]
    pin: bool,

    /// Remove these tools from the local config
    #[usage(long, value_name = "TOOL", aliases = ["rm", "unset"])]
    remove: Option<Vec<BackendArg>>,
}

impl Local {
    pub(crate) async fn run(self) -> Result<()> {
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

pub(super) fn get_parent_path() -> Result<PathBuf> {
    let mut filenames = vec![MISE_DEFAULT_CONFIG_FILENAME.as_str()];
    if !*env::MISE_USE_TOML {
        filenames.push(MISE_DEFAULT_TOOL_VERSIONS_FILENAME.as_str());
    }
    file::find_up(&env::current_dir()?, &filenames)
        .wrap_err_with(|| eyre!("no {} file found", filenames.join(" or "),))
}

pub(super) async fn local(
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
    let cf = config_file::parse_or_init(path).await?;
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
