use std::path::PathBuf;

use crate::cli::args::ToolArg;
use crate::cli::prune::prune;
use crate::config::config_file::ConfigFile;
use crate::config::{Config, config_file};
use crate::env::MISE_GLOBAL_CONFIG_FILE;
use crate::file::display_path;
use crate::{config, env};
use eyre::Result;
use itertools::Itertools;
use path_absolutize::Absolutize;

/// Removes installed tool versions from mise.toml
///
/// By default, this will use the `mise.toml` file that has the tool defined.
///
/// In the following order:
///   - If `--global` is set, it will use the global config file.
///   - If `--path` is set, it will use the config file at the given path.
///   - If `--env` is set, it will use `mise.<env>.toml`.
///   - If `MISE_DEFAULT_CONFIG_FILENAME` is set, it will use that instead.
///   - If `MISE_OVERRIDE_CONFIG_FILENAMES` is set, it will the first from that list.
///   - Otherwise just "mise.toml" or global config if cwd is home directory.
///
/// Will also prune the installed version if no other configurations are using it.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, visible_aliases = ["rm", "remove"], after_long_help = AFTER_LONG_HELP)]
pub struct Unuse {
    /// Tool(s) to remove
    #[clap(value_name = "INSTALLED_TOOL@VERSION", required = true)]
    installed_tool: Vec<ToolArg>,

    /// Use the global config file (`~/.config/mise/config.toml`) instead of the local one
    #[clap(short, long, overrides_with_all = & ["path", "env"])]
    global: bool,

    /// Create/modify an environment-specific config file like .mise.<env>.toml
    #[clap(long, short, overrides_with_all = & ["global", "path"])]
    env: Option<String>,

    /// Specify a path to a config file or directory
    ///
    /// If a directory is specified, it will look for a config file in that directory following
    /// the rules above.
    #[clap(short, long, overrides_with_all = & ["global", "env"], value_hint = clap::ValueHint::FilePath)]
    path: Option<PathBuf>,

    /// Do not also prune the installed version
    #[clap(long)]
    no_prune: bool,
}

impl Unuse {
    pub fn run(self) -> Result<()> {
        let mut cf = self.get_config_file()?;
        let tools = cf.to_tool_request_set()?.tools;
        let mut removed: Vec<&ToolArg> = vec![];
        for ta in &self.installed_tool {
            if let Some(tool_requests) = tools.get(&ta.ba) {
                let should_remove = if let Some(v) = &ta.version {
                    tool_requests.iter().any(|tv| &tv.version() == v)
                } else {
                    true
                };
                // TODO: this won't work properly for unusing a specific version in of multiple in a config
                if should_remove {
                    removed.push(ta);
                    cf.remove_tool(&ta.ba)?;
                }
            }
        }
        if removed.is_empty() {
            debug!("no tools to remove");
        } else {
            cf.save()?;
            let removals = removed.iter().join(", ");
            info!("removed: {removals} from {}", display_path(cf.get_path()));
        }

        if !self.no_prune {
            prune(self.installed_tool.iter().map(|ta| &ta.ba).collect(), false)?;
            config::rebuild_shims_and_runtime_symlinks(&[])?;
        }

        Ok(())
    }

    fn get_config_file(&self) -> Result<Box<dyn ConfigFile>> {
        let cwd = env::current_dir()?;
        let path = if self.global {
            MISE_GLOBAL_CONFIG_FILE.clone()
        } else if let Some(p) = &self.path {
            let from_dir = config::config_file_from_dir(p).absolutize()?.to_path_buf();
            if from_dir.starts_with(&cwd) {
                from_dir
            } else {
                p.clone()
            }
        } else if let Some(env) = &self.env {
            let p = cwd.join(format!(".mise.{env}.toml"));
            if p.exists() {
                p
            } else {
                cwd.join(format!("mise.{env}.toml"))
            }
        } else if env::in_home_dir() {
            MISE_GLOBAL_CONFIG_FILE.clone()
        } else {
            let config = Config::get();
            for cf in config.config_files.values() {
                if cf
                    .to_tool_request_set()?
                    .tools
                    .keys()
                    .any(|ba| self.installed_tool.iter().any(|ta| ta.ba == *ba))
                {
                    return config_file::parse(cf.get_path());
                }
            }
            config::local_toml_config_path()
        };
        config_file::parse_or_init(&path)
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    # will uninstall specific version
    $ <bold>mise unuse node@18.0.0</bold>

    # will uninstall specific version from global config
    $ <bold>mise unuse -g node@18.0.0</bold>

    # will uninstall specific version from .mise.local.toml
    $ <bold>mise unuse --env local node@20</bold>

    # will uninstall specific version from .mise.staging.toml
    $ <bold>mise unuse --env staging node@20</bold>
"#
);
