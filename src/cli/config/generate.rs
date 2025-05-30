use std::path::{Path, PathBuf};

use clap::ValueHint;
use eyre::Result;

use crate::config::{Settings, config_file};
use crate::file;
use crate::file::display_path;

/// [experimental] Generate a mise.toml file
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "g", verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct ConfigGenerate {
    /// Path to a .tool-versions file to import tools from
    #[clap(long, short, verbatim_doc_comment, value_hint = ValueHint::FilePath)]
    tool_versions: Option<PathBuf>,
    /// Output to file instead of stdout
    #[clap(long, short, verbatim_doc_comment, value_hint = ValueHint::FilePath)]
    output: Option<PathBuf>,
}

impl ConfigGenerate {
    pub fn run(self) -> Result<()> {
        Settings::get().ensure_experimental("`mise config generate`")?;
        let doc = if let Some(tool_versions) = &self.tool_versions {
            self.tool_versions(tool_versions)?
        } else {
            self.default()
        };
        if let Some(output) = &self.output {
            info!("writing to {}", display_path(output));
            file::write(output, doc)?;
        } else {
            miseprintln!("{doc}");
        }

        Ok(())
    }

    fn tool_versions(&self, tool_versions: &Path) -> Result<String> {
        let to = config_file::parse_or_init(&PathBuf::from("mise.toml"))?;
        let from = config_file::parse(tool_versions)?;
        let tools = from.to_tool_request_set()?.tools;
        for (ba, tools) in tools {
            to.replace_versions(&ba, tools)?;
        }
        to.dump()
    }

    fn default(&self) -> String {
        r#"
# # mise config files are hierarchical. mise will find all of the config files
# # in all parent directories and merge them together.
# # You might have a structure like:
#
# * ~/work/project/mise.toml   # a config file for a specific work project
# * ~/work/mise.toml           # a config file for projects related to work
# * ~/.config/mise/config.toml # the global config file
# * /etc/mise/config.toml      # the system config file
#
# # This setup allows you to define default versions and configuration across
# # all projects but override them for specific projects.
#
# # set arbitrary env vars to be used whenever in this project or subprojects
# [env]
# NODE_ENV = "development"
# NPM_CONFIG_PREFIX = "~/.npm-global"
# EDITOR = "code --wait"
#
# mise.file = ".env"                # load vars from a dotenv file
# mise.path = "./node_modules/.bin" # add a directory to PATH
#
# [tools]
# terraform = '1.0.0'       # specify a single version
# erlang = '26'             # specify a major version only
# node = 'ref:master'       # build from a git ref
# node = 'path:~/.nodes/14' # BYO – specify a non-mise managed installation
#
# # newest with this prefix (typically exact matches don't use the prefix)
# go = 'prefix:1.16'
#
# # multiple versions will all go into PATH in the order specified
# # this is helpful for making `python311` and `python310` available
# # even when `python` and `python3` point to a different version
# python = ['3.12', '3.11', '3.10']
#
# # some plugins can take options like python's virtualenv activation
# # with these, mise will automatically setup and activate vevs when entering
# # the project directory
# python = {version='3.12', virtualenv='.venv'}
# poetry = {version='1.7.1', pyproject='pyproject.toml'}
#
# [plugins]
# # specify a custom repo url so you can install with `mise plugin add <name>`
# # note this will only be used if the plugin is not already installed
# python = 'https://github.com/asdf-community/asdf-python'
#
# [alias.node.versions]
# # setup a custom alias so you can run `mise use -g node@work` for node-16.x
# work = '16'
"#
        .to_string()
    }
}

// TODO: fill this out
pub static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise cf generate > mise.toml</bold>
    $ <bold>mise cf generate --output=mise.toml</bold>
"#
);
