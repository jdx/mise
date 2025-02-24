use crate::cli::args::ToolArg;
use crate::cli::prune::prune;
use crate::config;
use crate::config::config_file::ConfigFile;
use crate::config::{config_file, Config};
use crate::file::display_path;
use crate::toolset::ToolRequest;
use eyre::Result;
use itertools::Itertools;

/// Removes installed tool versions from mise.toml
///
/// Will also prune the installed version if no other configurations are using it.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, visible_aliases = ["rm", "remove"], after_long_help = AFTER_LONG_HELP)]
pub struct Unuse {
    /// Tool(s) to remove
    #[clap(value_name = "INSTALLED_TOOL@VERSION", required = true)]
    installed_tool: Vec<ToolArg>,

    /// Do not also prune the installed version
    #[clap(long)]
    no_prune: bool,

    /// Remove tool from global config
    #[clap(short, long)]
    global: bool,
}

impl Unuse {
    pub fn run(self) -> Result<()> {
        let config = Config::get();
        let mut cf = self.get_config_file(&config)?;
        let tools = cf.to_tool_request_set()?.tools;
        let mut removed: Vec<&ToolArg> = vec![];
        for ta in &self.installed_tool {
            if let Some(tool_requests) = tools.get(&ta.ba) {
                let tools_to_remove: Vec<&ToolRequest> = tool_requests
                    .iter()
                    .filter(|tv| {
                        tv.version() == ta.version.as_ref().map_or("latest", |v| v.as_str())
                    })
                    .collect();
                for _tool in tools_to_remove {
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

    pub fn get_config_file(&self, config: &Config) -> Result<Box<dyn ConfigFile>> {
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
        config_file::parse_or_init(&config::local_toml_config_path())
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    # will uninstall specific version
    $ <bold>mise unuse node@18.0.0</bold>

    # will uninstall specific version from global config
    $ <bold>mise unuse -g node@18.0.0</bold>
"#
);
