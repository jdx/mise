use std::path::{Path, PathBuf};

use console::style;
use eyre::Result;
use itertools::Itertools;

use crate::cli::args::{BackendArg, ToolArg};
use crate::config::config_file::ConfigFile;
use crate::config::{config_file, is_global_config, Config, Settings, LOCAL_CONFIG_FILENAMES};
use crate::env::{
    MISE_DEFAULT_CONFIG_FILENAME, MISE_DEFAULT_TOOL_VERSIONS_FILENAME, MISE_GLOBAL_CONFIG_FILE,
};
use crate::file::display_path;
use crate::toolset::{InstallOptions, ToolRequest, ToolSource, ToolVersion, ToolsetBuilder};
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::{env, file};

/// Install tool version and add it to config
///
/// This will install the tool if it is not already installed.
/// By default, this will use an `.mise.toml` file in the current directory.
/// Use the --global flag to use the global config file instead.
/// This replaces asdf's `local` and `global` commands, however those are still available in mise.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, visible_alias = "u", after_long_help = AFTER_LONG_HELP)]
pub struct Use {
    /// Tool(s) to add to config file
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
    /// e.g.: `mise use --fuzzy node@20` will save 20 as the version
    /// this is the default behavior unless MISE_ASDF_COMPAT=1
    #[clap(long, verbatim_doc_comment, overrides_with = "pin")]
    fuzzy: bool,

    /// Use the global config file (~/.config/mise/config.toml) instead of the local one
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
    /// Sets --jobs=1
    #[clap(long, overrides_with = "jobs")]
    raw: bool,

    /// Remove the plugin(s) from config file
    #[clap(long, value_name = "PLUGIN", aliases = ["rm", "unset"])]
    remove: Vec<BackendArg>,

    /// Specify a path to a config file or directory
    /// If a directory is specified, it will look for .mise.toml (default) or .tool-versions
    #[clap(short, long, overrides_with_all = & ["global", "env"], value_hint = clap::ValueHint::FilePath
    )]
    path: Option<PathBuf>,

    /// Save exact version to config file
    /// e.g.: `mise use --pin node@20` will save 20.0.0 as the version
    /// Set MISE_ASDF_COMPAT=1 to make this the default behavior
    #[clap(long, verbatim_doc_comment, overrides_with = "fuzzy")]
    pin: bool,
}

impl Use {
    pub fn run(self) -> Result<()> {
        hint!(
            "use",
            "install multiple versions simultaneously with",
            "mise use python@3.8 python@3.9"
        );
        let config = Config::try_get()?;
        let mut ts = ToolsetBuilder::new().build(&config)?;
        let mpr = MultiProgressReport::get();
        let versions: Vec<_> = self
            .tool
            .iter()
            .cloned()
            .map(|t| match t.tvr {
                Some(tvr) => Ok(tvr),
                None => ToolRequest::new(t.backend, "latest"),
            })
            .collect::<Result<_>>()?;
        let versions = ts.install_versions(
            &config,
            versions.clone(),
            &mpr,
            &InstallOptions {
                force: self.force,
                jobs: self.jobs,
                raw: self.raw,
                latest_versions: false,
            },
        )?;

        let mut cf = self.get_config_file()?;
        let settings = Settings::try_get()?;
        let pin = self.pin || (settings.asdf_compat && !self.fuzzy);

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
        self.render_success_message(cf.as_ref(), &versions)?;
        Ok(())
    }

    fn get_config_file(&self) -> Result<Box<dyn ConfigFile>> {
        let path = if self.global {
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
                    if p != global {
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
    match is_asdf_compat() {
        true => tool_versions,
        false => mise_toml,
    }
}

fn is_asdf_compat() -> bool {
    Settings::try_get().map_or(false, |s| s.asdf_compat)
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    # set the current version of node to 20.x in .mise.toml of current directory
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

#[cfg(test)]
mod tests {
    use insta::assert_snapshot;

    use crate::test::reset;
    use crate::{dirs, env, file};

    #[test]
    fn test_use_local_reuse() {
        reset();
        let cf_path = env::current_dir().unwrap().join(".test.mise.toml");
        file::write(&cf_path, "").unwrap();

        assert_cli_snapshot!("use", "tiny@2", @r###"
        mise ~/cwd/.test.mise.toml tools: tiny@2.1.0
        mise hint install multiple versions simultaneously with mise use python@3.8 python@3.9
        "###);
        assert_snapshot!(file::read_to_string(&cf_path).unwrap(), @r###"
        [tools]
        tiny = "2"
        "###);

        assert_cli_snapshot!("use", "tiny@1", "tiny@2", "tiny@3", @r###"
        mise ~/cwd/.test.mise.toml tools: tiny@1.0.1, tiny@2.1.0, tiny@3.1.0
        mise hint install multiple versions simultaneously with mise use python@3.8 python@3.9
        "###);
        assert_snapshot!(file::read_to_string(&cf_path).unwrap(), @r###"
        [tools]
        tiny = ["1", "2", "3"]
        "###);

        assert_cli_snapshot!("use", "--pin", "tiny", @r###"
        mise ~/cwd/.test.mise.toml tools: tiny@3.1.0
        mise hint install multiple versions simultaneously with mise use python@3.8 python@3.9
        "###);
        assert_snapshot!(file::read_to_string(&cf_path).unwrap(), @r###"
        [tools]
        tiny = "3.1.0"
        "###);

        assert_cli_snapshot!("use", "--fuzzy", "tiny@2", @r###"
        mise ~/cwd/.test.mise.toml tools: tiny@2.1.0
        mise hint install multiple versions simultaneously with mise use python@3.8 python@3.9
        "###);
        assert_snapshot!(file::read_to_string(&cf_path).unwrap(), @r###"
        [tools]
        tiny = "2"
        "###);

        let p = cf_path.to_string_lossy().to_string();
        assert_cli_snapshot!("use", "--rm", "tiny", "--path", &p, @r###"
        mise ~/cwd/.test.mise.toml tools:
        mise hint install multiple versions simultaneously with mise use python@3.8 python@3.9
        "###);
        assert_snapshot!(file::read_to_string(&cf_path).unwrap(), @"");

        let _ = file::remove_file(&cf_path);
    }

    #[test]
    fn test_use_local_create() {
        reset();
        let _ = file::remove_file(env::current_dir().unwrap().join(".test-tool-versions"));
        let cf_path = env::current_dir().unwrap().join(".test.mise.toml");

        assert_cli_snapshot!("use", "tiny@2", @r###"
        mise ~/cwd/.test.mise.toml tools: tiny@2.1.0
        mise hint install multiple versions simultaneously with mise use python@3.8 python@3.9
        "###);
        assert_snapshot!(file::read_to_string(&cf_path).unwrap(), @r###"
      [tools]
      tiny = "2"
      "###);

        assert_cli_snapshot!("use", "tiny@1", "tiny@2", "tiny@3", @r###"
        mise ~/cwd/.test.mise.toml tools: tiny@1.0.1, tiny@2.1.0, tiny@3.1.0
        mise hint install multiple versions simultaneously with mise use python@3.8 python@3.9
        "###);
        assert_snapshot!(file::read_to_string(&cf_path).unwrap(), @r###"
      [tools]
      tiny = ["1", "2", "3"]
      "###);

        assert_cli_snapshot!("use", "--pin", "tiny", @r###"
        mise ~/cwd/.test.mise.toml tools: tiny@3.1.0
        mise hint install multiple versions simultaneously with mise use python@3.8 python@3.9
        "###);
        assert_snapshot!(file::read_to_string(&cf_path).unwrap(), @r###"
      [tools]
      tiny = "3.1.0"
      "###);

        assert_cli_snapshot!("use", "--fuzzy", "tiny@2", @r###"
        mise ~/cwd/.test.mise.toml tools: tiny@2.1.0
        mise hint install multiple versions simultaneously with mise use python@3.8 python@3.9
        "###);
        assert_snapshot!(file::read_to_string(&cf_path).unwrap(), @r###"
      [tools]
      tiny = "2"
      "###);

        let p = cf_path.to_string_lossy().to_string();
        assert_cli_snapshot!("use", "--rm", "tiny", "--path", &p, @r###"
        mise ~/cwd/.test.mise.toml tools:
        mise hint install multiple versions simultaneously with mise use python@3.8 python@3.9
        "###);
        assert_snapshot!(file::read_to_string(&cf_path).unwrap(), @"");

        let _ = file::remove_file(&cf_path);
    }

    #[test]
    fn test_use_local_tool_versions_reuse() {
        reset();
        let cf_path = env::current_dir().unwrap().join(".test-tool-versions");
        file::write(&cf_path, "").unwrap();

        assert_cli_snapshot!("use", "tiny@3", @r###"
        mise ~/cwd/.test-tool-versions tools: tiny@3.1.0
        mise hint install multiple versions simultaneously with mise use python@3.8 python@3.9
        "###);
        assert_snapshot!(file::read_to_string(&cf_path).unwrap(), @r###"
        tiny 3
        "###);

        let _ = file::remove_file(&cf_path);
    }

    #[test]
    fn test_use_local_tool_versions_create() {
        reset();
        let cf_path = env::current_dir().unwrap().join(".test-tool-versions");

        assert_cli_snapshot!("use", "tiny@3", @r###"
        mise ~/cwd/.test-tool-versions tools: tiny@3.1.0
        mise hint install multiple versions simultaneously with mise use python@3.8 python@3.9
        "###);
        assert_snapshot!(file::read_to_string(&cf_path).unwrap(), @r###"
        tiny 3
        "###);

        let _ = file::remove_file(&cf_path);
    }

    #[test]
    fn test_use_global() {
        reset();
        let cf_path = dirs::CONFIG.join("config.toml");
        let orig = file::read_to_string(&cf_path).unwrap();

        assert_cli_snapshot!("use", "-g", "tiny@2", @r###"
        mise ~/config/config.toml tools: tiny@2.1.0
        mise hint install multiple versions simultaneously with mise use python@3.8 python@3.9
        mise tiny is defined in ~/cwd/.test-tool-versions which overrides the global config (~/config/config.toml)
        "###);
        assert_snapshot!(file::read_to_string(&cf_path).unwrap(), @r###"
        [env]
        TEST_ENV_VAR = 'test-123'

        [alias.tiny]
        "my/alias" = '3.0'

        [tasks.configtask]
        run = 'echo "configtask:"'
        [tasks.lint]
        run = 'echo "linting!"'
        [tasks.test]
        run = 'echo "testing!"'
        [settings]
        always_keep_download= true
        always_keep_install= true
        legacy_version_file= true
        plugin_autoupdate_last_check_duration = "20m"
        jobs = 2

        [tools]
        tiny = "2"
        "###);

        file::write(&cf_path, orig).unwrap();
    }
}
