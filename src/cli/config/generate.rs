use std::path::PathBuf;

use crate::config::Settings;
use crate::file;
use crate::file::display_path;
use clap::ValueHint;
use eyre::Result;

/// [experimental] Generate an .rtx.toml file
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "g", verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct ConfigGenerate {
    /// Output to file instead of stdout
    #[clap(long, short, verbatim_doc_comment, value_hint = ValueHint::FilePath)]
    output: Option<PathBuf>,
}

impl ConfigGenerate {
    pub fn run(self) -> Result<()> {
        let settings = Settings::try_get()?;
        settings.ensure_experimental()?;
        let doc = r#"
# # rtx config files are hierarchical. rtx will find all of the config files
# # in all parent directories and merge them together.
# # You might have a structure like:
#
# * ~/work/project/.rtx.toml  # a config file for a specific work project
# * ~/work/.rtx.toml          # a config file for projects related to work
# * ~/.config/rtx/config.toml # the global config file
# * /etc/rtx/config.toml      # the system config file
#
# # This setup allows you to define default versions and configuration across
# # all projects but override them for specific projects.
#
# # add extra directories to PATH
# env_path = [
#   "~/bin", # absolute path
#   "./node_modules/.bin", # relative path to this file, not $PWD
# ]
#
# # set arbitrary env vars to be used whenever in this project or subprojects
# [env]
# NODE_ENV = "development"
# NPM_CONFIG_PREFIX = "~/.npm-global"
# EDITOR = "code --wait"
#
# [env_remove]
# load a dotenv file
# env_file = ".env"
#
# [tools]
# terraform = '1.0.0'       # specify a single version
# erlang = '26'             # specify a major version only
# node = 'ref:master'       # build from a git ref
# node = 'path:~/.nodes/14' # BYO – specify a non-rtx managed installation
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
# # with these, rtx will automatically setup and activate vevs when entering
# # the project directory
# python = {version='3.12', virtualenv='.venv'}
# poetry = {version='1.7.1', pyproject='pyproject.toml'}
#
# [plugins]
# # specify a custom repo url so you can install with `rtx plugin add <name>`
# # note this will only be used if the plugin is not already installed
# python = 'https://github.com/asdf-community/asdf-python'
#
# [alias.node]
# # setup a custom alias so you can run `rtx use -g node@work` for node-16.x
# work = '16'
"#
        .trim();
        if let Some(output) = &self.output {
            rtxstatusln!("writing to {}", display_path(output));
            file::write(output, doc)?;
        } else {
            rtxprintln!("{doc}");
        }

        Ok(())
    }
}

// TODO: fill this out
static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>
  $ <bold>rtx cf generate > .rtx.toml</bold>
  $ <bold>rtx cf generate --output=.rtx.toml</bold>
"#
);

#[cfg(test)]
mod tests {
    use std::env;

    #[test]
    fn test_generate() {
        with_settings!({
            let out = assert_cli!("config", "generate");
            for line in out.lines() {
                assert!(line.len() < 80);
            }
            assert_cli_snapshot!("cfg", "generate");
        });
    }
}
