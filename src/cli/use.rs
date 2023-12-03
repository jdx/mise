use std::path::{Path, PathBuf};

use color_eyre::eyre::Result;
use console::style;
use itertools::Itertools;

use crate::cli::args::tool::{ToolArg, ToolArgParser};
use crate::config::{config_file, Config};
use crate::env::{RTX_DEFAULT_CONFIG_FILENAME, RTX_DEFAULT_TOOL_VERSIONS_FILENAME};
use crate::file::display_path;
use crate::output::Output;
use crate::plugins::PluginName;
use crate::toolset::ToolsetBuilder;
use crate::{dirs, env, file};

/// Change the active version of a tool locally or globally.
///
/// This will install the tool if it is not already installed.
/// By default, this will use an `.rtx.toml` file in the current directory.
/// Use the --global flag to use the global config file instead.
/// This replaces asdf's `local` and `global` commands, however those are still available in rtx.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, visible_alias = "u", after_long_help = AFTER_LONG_HELP)]
pub struct Use {
    /// Tool(s) to add to config file
    /// e.g.: node@20
    /// If no version is specified, it will default to @latest
    #[clap(value_name = "TOOL@VERSION", value_parser = ToolArgParser, verbatim_doc_comment, required_unless_present = "remove")]
    tool: Vec<ToolArg>,

    /// Save exact version to config file
    /// e.g.: `rtx use --pin node@20` will save `node 20.0.0` to ~/.tool-versions
    #[clap(long, verbatim_doc_comment, overrides_with = "fuzzy")]
    pin: bool,

    /// Save fuzzy version to config file
    /// e.g.: `rtx use --fuzzy node@20` will save `node 20` to ~/.tool-versions
    /// this is the default behavior unless RTX_ASDF_COMPAT=1
    #[clap(long, verbatim_doc_comment, overrides_with = "pin")]
    fuzzy: bool,

    /// Remove the tool(s) from config file
    #[clap(long, value_name = "TOOL", aliases = ["rm", "unset"])]
    remove: Option<Vec<PluginName>>,

    /// Use the global config file (~/.config/rtx/config.toml) instead of the local one
    #[clap(short, long, overrides_with = "path")]
    global: bool,

    /// Specify a path to a config file or directory
    #[clap(short, long, overrides_with = "global", value_hint = clap::ValueHint::FilePath)]
    path: Option<PathBuf>,
}

impl Use {
    pub fn run(self, mut config: Config, out: &mut Output) -> Result<()> {
        let mut ts = ToolsetBuilder::new()
            .with_args(&self.tool)
            .with_install_missing()
            .build(&mut config)?;
        ts.versions
            .retain(|_, tvl| self.tool.iter().any(|t| t.plugin == tvl.plugin_name));

        let path = match (self.global, self.path) {
            (true, _) => global_file(),
            (false, Some(p)) => config_file_from_dir(&p),
            (false, None) => config_file_from_dir(&dirs::CURRENT),
        };
        let is_trusted = config_file::is_trusted(&config.settings, &path);
        let mut cf = match path.exists() {
            true => config_file::parse(&path, is_trusted)?,
            false => config_file::init(&path, is_trusted),
        };

        let pin = self.pin || (config.settings.asdf_compat && !self.fuzzy);

        for (plugin_name, tvl) in ts.versions {
            let versions: Vec<String> = tvl
                .versions
                .into_iter()
                .map(|tv| {
                    if pin {
                        tv.version
                    } else {
                        tv.request.version()
                    }
                })
                .collect();
            cf.replace_versions(&plugin_name, &versions);
        }

        for plugin_name in self.remove.unwrap_or_default() {
            cf.remove_plugin(&plugin_name);
        }
        cf.save()?;
        let tools = self.tool.iter().map(|t| t.to_string()).join(" ");
        rtxprintln!(
            out,
            "{} {} {}",
            style("rtx").dim(),
            display_path(&path),
            style(tools).cyan()
        );
        Ok(())
    }
}

fn global_file() -> PathBuf {
    env::RTX_CONFIG_FILE
        .clone()
        .unwrap_or_else(|| dirs::CONFIG.join("config.toml"))
}

fn config_file_from_dir(p: &Path) -> PathBuf {
    if !p.is_dir() {
        return p.to_path_buf();
    }
    let rtx_toml = p.join(&*RTX_DEFAULT_CONFIG_FILENAME);
    let tool_versions = p.join(&*RTX_DEFAULT_TOOL_VERSIONS_FILENAME);
    if rtx_toml.exists() {
        return rtx_toml;
    } else if tool_versions.exists() {
        return tool_versions;
    }
    let filenames = vec![RTX_DEFAULT_CONFIG_FILENAME.as_str()];
    if let Some(p) = file::find_up(p, &filenames) {
        return p;
    }
    rtx_toml
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>
  # set the current version of node to 20.x in .rtx.toml of current directory
  # will write the fuzzy version (e.g.: 20)
  $ <bold>rtx use node@20</bold>

  # set the current version of node to 20.x in ~/.config/rtx/config.toml
  # will write the precise version (e.g.: 20.0.0)
  $ <bold>rtx use -g --pin node@20</bold>
"#
);

#[cfg(test)]
mod tests {
    use insta::assert_snapshot;

    use crate::{assert_cli_snapshot, dirs, file};

    #[test]
    fn test_use_local() {
        let cf_path = dirs::CURRENT.join(".test.rtx.toml");
        file::write(&cf_path, "").unwrap();

        assert_cli_snapshot!("use", "tiny@2");
        assert_snapshot!(file::read_to_string(&cf_path).unwrap());

        assert_cli_snapshot!("use", "--pin", "tiny");
        assert_snapshot!(file::read_to_string(&cf_path).unwrap());

        assert_cli_snapshot!("use", "--fuzzy", "tiny@2");
        assert_snapshot!(file::read_to_string(&cf_path).unwrap());

        let p = cf_path.to_string_lossy().to_string();
        assert_cli_snapshot!("use", "--rm", "tiny", "--path", &p);
        assert_snapshot!(file::read_to_string(&cf_path).unwrap());

        let _ = file::remove_file(&cf_path);
    }

    #[test]
    fn test_use_local_tool_versions() {
        let cf_path = dirs::CURRENT.join(".test-tool-versions");
        file::write(&cf_path, "").unwrap();

        assert_cli_snapshot!("use", "tiny@3");
        assert_snapshot!(file::read_to_string(&cf_path).unwrap());
    }

    #[test]
    fn test_use_global() {
        let cf_path = dirs::CONFIG.join("config.toml");
        let orig = file::read_to_string(&cf_path).unwrap();
        let _ = file::remove_file(&cf_path);

        assert_cli_snapshot!("use", "-g", "tiny@2");
        assert_snapshot!(file::read_to_string(&cf_path).unwrap());

        file::write(&cf_path, orig).unwrap();
    }
}
