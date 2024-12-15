use std::path::{Path, PathBuf};

use console::{style, Term};
use eyre::{eyre, Result};
use itertools::Itertools;
use path_absolutize::Absolutize;

use crate::cli::args::{BackendArg, ToolArg};
use crate::config::config_file::ConfigFile;
use crate::config::{config_file, is_global_config, Config, SETTINGS};
use crate::env::{
    MISE_DEFAULT_CONFIG_FILENAME, MISE_DEFAULT_TOOL_VERSIONS_FILENAME, MISE_GLOBAL_CONFIG_FILE,
};
use crate::file::display_path;
use crate::registry::REGISTRY;
use crate::toolset::{
    InstallOptions, ResolveOptions, ToolRequest, ToolSource, ToolVersion, ToolsetBuilder,
};
use crate::ui::ctrlc;
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::{config, env, file};

/// Installs a tool and adds the version to mise.toml.
///
/// This will install the tool version if it is not already installed.
/// By default, this will use a `mise.toml` file in the current directory.
///
/// In the following order:
///   - If `MISE_DEFAULT_CONFIG_FILENAME` is set, it will use that instead.
///   - If `MISE_OVERRIDE_CONFIG_FILENAMES` is set, it will the first from that list.
///   - If `MISE_ENV` is set, it will use a `mise.<env>.toml` instead.
///   - Otherwise just "mise.toml"
///
/// Use the `--global` flag to use the global config file instead.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, visible_alias = "u", after_long_help = AFTER_LONG_HELP)]
pub struct Use {
    /// Tool(s) to add to config file
    ///
    /// e.g.: node@20, cargo:ripgrep@latest npm:prettier@3
    /// If no version is specified, it will default to @latest
    ///
    /// Tool options can be set with this syntax:
    ///
    ///     mise use ubi:BurntSushi/ripgrep[exe=rg]
    #[clap(value_name = "TOOL@VERSION", verbatim_doc_comment)]
    tool: Vec<ToolArg>,

    /// Force reinstall even if already installed
    #[clap(long, short, requires = "tool")]
    force: bool,

    /// Save fuzzy version to config file
    ///
    /// e.g.: `mise use --fuzzy node@20` will save 20 as the version
    /// this is the default behavior unless `MISE_PIN=1`
    #[clap(long, verbatim_doc_comment, overrides_with = "pin")]
    fuzzy: bool,

    /// Use the global config file (`~/.config/mise/config.toml`) instead of the local one
    #[clap(short, long, overrides_with_all = & ["path", "env"])]
    global: bool,

    /// Create/modify an environment-specific config file like .mise.<env>.toml
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
    /// If a directory is specified, it will look for a config file in that directory following
    /// the rules above.
    #[clap(short, long, overrides_with_all = & ["global", "env"], value_hint = clap::ValueHint::FilePath)]
    path: Option<PathBuf>,

    /// Save exact version to config file
    /// e.g.: `mise use --pin node@20` will save 20.0.0 as the version
    /// Set `MISE_PIN=1` to make this the default behavior
    ///
    /// Consider using mise.lock as a better alternative to pinning in mise.toml:
    /// https://mise.jdx.dev/configuration/settings.html#lockfile
    #[clap(long, verbatim_doc_comment, overrides_with = "fuzzy")]
    pin: bool,
}

impl Use {
    pub fn run(mut self) -> Result<()> {
        if self.tool.is_empty() && self.remove.is_empty() {
            self.tool = vec![self.tool_selector()?];
        }
        let config = Config::try_get()?;
        let mut ts = ToolsetBuilder::new()
            .with_global_only(self.global)
            .build(&config)?;
        let mpr = MultiProgressReport::get();
        let mut cf = self.get_config_file()?;
        let mut resolve_options = ResolveOptions {
            latest_versions: false,
            use_locked_version: true,
        };
        let versions: Vec<_> = self
            .tool
            .iter()
            .cloned()
            .map(|t| match t.tvr {
                Some(tvr) => {
                    if tvr.version() == "latest" {
                        // user specified `@latest` so we should resolve the latest version
                        // TODO: this should only happen on this tool, not all of them
                        resolve_options.latest_versions = true;
                        resolve_options.use_locked_version = false;
                    }
                    Ok(tvr)
                }
                None => ToolRequest::new(
                    t.ba,
                    "latest",
                    ToolSource::MiseToml(cf.get_path().to_path_buf()),
                ),
            })
            .collect::<Result<_>>()?;
        let mut versions = ts.install_all_versions(
            versions.clone(),
            &mpr,
            &InstallOptions {
                force: self.force,
                jobs: self.jobs,
                raw: self.raw,
                resolve_options,
                ..Default::default()
            },
        )?;

        let pin = self.pin || !self.fuzzy && (SETTINGS.pin || SETTINGS.asdf_compat);

        for (ba, tvl) in &versions.iter().chunk_by(|tv| tv.ba()) {
            let versions: Vec<_> = tvl
                .into_iter()
                .map(|tv| {
                    let mut request = tv.request.clone();
                    if pin {
                        if let ToolRequest::Version {
                            version: _version,
                            source,
                            os,
                            options,
                            backend,
                        } = request
                        {
                            request = ToolRequest::Version {
                                version: tv.version.clone(),
                                source,
                                os,
                                options,
                                backend,
                            };
                        }
                    }
                    request
                })
                .collect();
            cf.replace_versions(ba, versions)?;
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

        config::rebuild_shims_and_runtime_symlinks(&versions)?;

        self.render_success_message(cf.as_ref(), &versions)?;
        Ok(())
    }

    fn get_config_file(&self) -> Result<Box<dyn ConfigFile>> {
        let cwd = env::current_dir()?;
        let path = if !env::MISE_ENV.is_empty() {
            let env = env::MISE_ENV.last().unwrap();
            config_file_from_dir(&cwd.join(format!("mise.{env}.toml")))
        } else if self.global || env::in_home_dir() {
            MISE_GLOBAL_CONFIG_FILE.clone()
        } else if let Some(env) = &self.env {
            let p = cwd.join(format!(".mise.{env}.toml"));
            if p.exists() {
                p
            } else {
                cwd.join(format!("mise.{env}.toml"))
            }
        } else if let Some(p) = &self.path {
            let from_dir = config_file_from_dir(p).absolutize()?.to_path_buf();
            if from_dir.starts_with(&cwd) {
                from_dir
            } else {
                p.clone()
            }
        } else {
            config_file_from_dir(&cwd)
        };
        config_file::parse_or_init(&path)
    }

    fn warn_if_hidden(&self, config: &Config, global: &Path) {
        let ts = ToolsetBuilder::new().build(config).unwrap_or_default();
        let warn = |targ: &ToolArg, p| {
            let plugin = &targ.ba;
            let p = display_path(p);
            let global = display_path(global);
            warn!("{plugin} is defined in {p} which overrides the global config ({global})");
        };
        for targ in &self.tool {
            if let Some(tv) = ts.versions.get(&targ.ba) {
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

    fn tool_selector(&self) -> Result<ToolArg> {
        let mut s = demand::Select::new("Tools")
            .description("Select a tool to install")
            .filtering(true)
            .filterable(true);
        for rt in REGISTRY.values().unique_by(|r| r.short) {
            if let Some(backend) = rt.backends().first() {
                // TODO: populate registry with descriptions from aqua and other sources
                // TODO: use the backend from the lockfile if available
                let description = rt.description.unwrap_or(backend);
                s = s.option(demand::DemandOption::new(rt).description(description));
            }
        }
        ctrlc::show_cursor_after_ctrl_c();
        match s.run() {
            Ok(rt) => rt.short.parse(),
            Err(err) => {
                Term::stderr().show_cursor()?;
                Err(eyre!(err))
            }
        }
    }
}

fn config_file_from_dir(p: &Path) -> PathBuf {
    if !p.is_dir() {
        return p.to_path_buf();
    }
    for dir in file::all_dirs().unwrap_or_default() {
        if let Some(cf) = config::config_files_in_dir(&dir).last() {
            if !is_global_config(cf) {
                return cf.clone();
            }
        }
    }
    match SETTINGS.asdf_compat {
        true => p.join(&*MISE_DEFAULT_TOOL_VERSIONS_FILENAME),
        false => p.join(&*MISE_DEFAULT_CONFIG_FILENAME),
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>
    
    # run with no arguments to use the interactive selector
    $ <bold>mise use</bold>

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
