use std::path::{Path, PathBuf};

use console::style;
use eyre::{Result, WrapErr};
use itertools::Itertools;

use crate::cli::args::{BackendArg, ToolArg};
use crate::config::config_file::ConfigFile;
use crate::config::{config_file, is_global_config, Config, LOCAL_CONFIG_FILENAMES, SETTINGS};
use crate::env::{
    MISE_DEFAULT_CONFIG_FILENAME, MISE_DEFAULT_TOOL_VERSIONS_FILENAME, MISE_GLOBAL_CONFIG_FILE,
};
use crate::file::display_path;
use crate::toolset::{InstallOptions, ToolRequest, ToolSource, ToolVersion, ToolsetBuilder};
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::{env, file, lockfile};

/// Installs a tool and adds the version it to mise.toml.
///
/// This will install the tool version if it is not already installed.
/// By default, this will use a `mise.toml` file in the current directory.
///
/// Use the `--global` flag to use the global config file instead.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, visible_alias = "u", after_long_help = AFTER_LONG_HELP)]
pub struct Use {
    /// Tool(s) to add to config file
    ///
    /// e.g.: node@20, cargo:ripgrep@latest npm:prettier@3
    /// If no version is specified, it will default to @latest
    #[clap(
        value_name = "TOOL@VERSION",
        verbatim_doc_comment,
        required_unless_present = "remove"
    )]
    tool: Vec<ToolArg>,

    /// Force reinstall even if already installed
    #[clap(long, short, requires = "tool")]
    force: bool,

    /// Save fuzzy version to config file
    ///
    /// e.g.: `mise use --fuzzy node@20` will save 20 as the version
    /// this is the default behavior unless `MISE_PIN=1` or `MISE_ASDF_COMPAT=1`
    #[clap(long, verbatim_doc_comment, overrides_with = "pin")]
    fuzzy: bool,

    /// Use the global config file (`~/.config/mise/config.toml`) instead of the local one
    #[clap(short, long, overrides_with_all = & ["path", "env"])]
    global: bool,

    /// Modify an environment-specific config file like .mise.<env>.toml
    #[clap(long, short, overrides_with_all = & ["global", "path"])]
    env: Option<String>,

    /// Number of jobs to run in parallel
    /// [default: 4]
    #[clap(long, short, env = "MISE_JOBS", verbatim_doc_comment)]
    jobs: Option<usize>,

    /// Directly pipe stdin/stdout/stderr from plugin to user
    /// Sets `--jobs=1`
    #[clap(long, overrides_with = "jobs")]
    raw: bool,

    /// Remove the plugin(s) from config file
    #[clap(long, value_name = "PLUGIN", aliases = ["rm", "unset"])]
    remove: Vec<BackendArg>,

    /// Specify a path to a config file or directory
    ///
    /// If a directory is specified, it will look for `mise.toml` (default) or `.tool-versions` if
    /// `MISE_ASDF_COMPAT=1`
    #[clap(short, long, overrides_with_all = & ["global", "env"], value_hint = clap::ValueHint::FilePath
    )]
    path: Option<PathBuf>,

    /// Save exact version to config file
    /// e.g.: `mise use --pin node@20` will save 20.0.0 as the version
    /// Set `MISE_PIN=1` or `MISE_ASDF_COMPAT=1` to make this the default behavior
    #[clap(long, verbatim_doc_comment, overrides_with = "fuzzy")]
    pin: bool,
}

impl Use {
    pub fn run(self) -> Result<()> {
        let config = Config::try_get()?;
        let mut ts = ToolsetBuilder::new().build(&config)?;
        let mpr = MultiProgressReport::get();
        let versions: Vec<_> = self
            .tool
            .iter()
            .cloned()
            .map(|t| match t.tvr {
                Some(tvr) => Ok(tvr),
                None => ToolRequest::new(t.backend, "latest", ToolSource::Argument),
            })
            .collect::<Result<_>>()?;
        let mut versions = ts.install_versions(
            &config,
            versions.clone(),
            &mpr,
            &InstallOptions {
                force: self.force,
                jobs: self.jobs,
                raw: self.raw,
                resolve_options: Default::default(),
            },
        )?;

        let mut cf = self.get_config_file()?;
        let pin = self.pin || !self.fuzzy && (SETTINGS.pin || SETTINGS.asdf_compat);

        for (fa, tvl) in &versions.iter().chunk_by(|tv| &tv.backend) {
            let versions: Vec<String> = tvl
                .into_iter()
                .map(|tv| {
                    if pin {
                        tv.version.clone()
                    } else {
                        tv.request.version()
                    }
                })
                .collect();
            cf.replace_versions(fa, &versions)?;
        }

        if self.global {
            self.warn_if_hidden(&config, cf.get_path());
        }
        for plugin_name in &self.remove {
            cf.remove_plugin(plugin_name)?;
        }
        cf.save()?;

        for tv in &mut versions {
            // update the source so the lockfile is updated correctly
            tv.request.set_source(cf.source());
        }

        lockfile::update_lockfiles(&versions).wrap_err("failed to update lockfiles")?;

        self.render_success_message(cf.as_ref(), &versions)?;
        Ok(())
    }

    fn get_config_file(&self) -> Result<Box<dyn ConfigFile>> {
        let path = if let Some(env) = &*env::MISE_ENV {
            config_file_from_dir(&env::current_dir()?.join(format!(".mise.{}.toml", env)))
        } else if self.global {
            MISE_GLOBAL_CONFIG_FILE.clone()
        } else if let Some(env) = &self.env {
            config_file_from_dir(&env::current_dir()?.join(format!(".mise.{}.toml", env)))
        } else if let Some(p) = &self.path {
            config_file_from_dir(p)
        } else {
            config_file_from_dir(&env::current_dir()?)
        };
        config_file::parse_or_init(&path)
    }

    fn warn_if_hidden(&self, config: &Config, global: &Path) {
        let ts = ToolsetBuilder::new().build(config).unwrap_or_default();
        let warn = |targ: &ToolArg, p| {
            let plugin = &targ.backend;
            let p = display_path(p);
            let global = display_path(global);
            warn!("{plugin} is defined in {p} which overrides the global config ({global})");
        };
        for targ in &self.tool {
            if let Some(tv) = ts.versions.get(&targ.backend) {
                if let ToolSource::MiseToml(p) | ToolSource::ToolVersions(p) = &tv.source {
                    if !file::same_file(p, global) {
                        warn(targ, p);
                    }
                }
            }
        }
    }

    fn render_success_message(&self, cf: &dyn ConfigFile, versions: &[ToolVersion]) -> Result<()> {
        let path = display_path(cf.get_path());
        let tools = versions.iter().map(|t| t.style()).join(", ");
        miseprintln!(
            "{} {} tools: {tools}",
            style("mise").green(),
            style(path).cyan().for_stderr(),
        );
        Ok(())
    }
}

fn config_file_from_dir(p: &Path) -> PathBuf {
    if !p.is_dir() {
        return p.to_path_buf();
    }
    let mise_toml = p.join(&*MISE_DEFAULT_CONFIG_FILENAME);
    let tool_versions = p.join(&*MISE_DEFAULT_TOOL_VERSIONS_FILENAME);
    if mise_toml.exists() {
        return mise_toml;
    } else if tool_versions.exists() {
        return tool_versions;
    }
    let filenames = LOCAL_CONFIG_FILENAMES
        .iter()
        .rev()
        .filter(|f| is_global_config(Path::new(f)))
        .map(|f| f.to_string())
        .collect::<Vec<_>>();
    if let Some(p) = file::find_up(p, &filenames) {
        return p;
    }
    match SETTINGS.asdf_compat {
        true => tool_versions,
        false => mise_toml,
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    # set the current version of node to 20.x in mise.toml of current directory
    # will write the fuzzy version (e.g.: 20)
    $ <bold>mise use node@20</bold>

    # set the current version of node to 20.x in ~/.config/mise/config.toml
    # will write the precise version (e.g.: 20.0.0)
    $ <bold>mise use -g --pin node@20</bold>

    # sets .mise.local.toml (which is intended not to be committed to a project)
    $ <bold>mise use --env local node@20</bold>

    # sets .mise.staging.toml (which is used if MISE_ENV=staging)
    $ <bold>mise use --env staging node@20</bold>
"#
);
