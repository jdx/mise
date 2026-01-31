use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use clap::ValueHint;
use demand::{Confirm, DemandOption, Input, MultiSelect, Select};
use eyre::{Result, eyre};
use indoc::formatdoc;
use itertools::Itertools;

use crate::config::Settings;
use crate::config::config_file;
use crate::file::display_path;
use crate::registry::{REGISTRY, RegistryTool};
use crate::ui::ctrlc::show_cursor_after_ctrl_c;
use crate::ui::theme::get_theme;
use crate::{env, file};

/// Generate a mise.toml file
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "g", verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct ConfigGenerate {
    /// Output to file instead of stdout
    #[clap(long, short, verbatim_doc_comment, value_hint = ValueHint::FilePath)]
    output: Option<PathBuf>,
    /// Path to a .tool-versions file to import tools from
    #[clap(long, short, verbatim_doc_comment, value_hint = ValueHint::FilePath)]
    tool_versions: Option<PathBuf>,
    /// Show what would be generated without writing to file
    #[clap(long, short = 'n')]
    dry_run: bool,
}

/// A detected tool with its source and suggested version
#[derive(Debug, Clone)]
struct DetectedTool {
    name: String,
    version: Option<String>,
    source: String,
}

impl std::fmt::Display for DetectedTool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.version {
            Some(v) => write!(f, "{} {} (from {})", self.name, v, self.source),
            None => write!(f, "{} (from {})", self.name, self.source),
        }
    }
}

/// State for the interactive wizard
#[derive(Default)]
struct WizardState {
    tools: BTreeMap<String, String>,
    env_vars: BTreeMap<String, String>,
    load_dotenv: bool,
    create_lockfile: bool,
}

impl ConfigGenerate {
    pub async fn run(self) -> Result<()> {
        let doc = if let Some(tool_versions) = &self.tool_versions {
            self.tool_versions(tool_versions).await?
        } else if self.should_run_interactive() {
            self.interactive().await?
        } else {
            self.default()
        };

        let output = self
            .output
            .clone()
            .unwrap_or_else(|| PathBuf::from(&*env::MISE_DEFAULT_CONFIG_FILENAME));

        if self.dry_run {
            info!("would write to {}", display_path(&output));
            miseprintln!("{doc}");
        } else {
            info!("writing to {}", display_path(&output));
            file::write(&output, doc)?;
        }

        Ok(())
    }

    fn should_run_interactive(&self) -> bool {
        !Settings::get().yes && console::user_attended_stderr()
    }

    async fn interactive(&self) -> Result<String> {
        show_cursor_after_ctrl_c();
        let theme = get_theme();
        let mut state = WizardState::default();

        // First, check for detected tools and offer to add them
        let detected = self.detect_tools();
        if !detected.is_empty() {
            miseprintln!("Detected tools from project files:\n");
            for tool in &detected {
                miseprintln!("  • {tool}");
            }
            miseprintln!();

            let use_detected = Confirm::new("Use detected tools?")
                .theme(&theme)
                .run()
                .map_err(handle_interrupt)?;

            if use_detected {
                for tool in &detected {
                    let version = tool.version.clone().unwrap_or_else(|| "latest".to_string());
                    state.tools.insert(tool.name.clone(), version);
                }
            }
        }

        // Main menu loop
        loop {
            let choice = self.show_main_menu(&theme, &state)?;

            match choice.as_str() {
                "tools" => self.add_tools(&theme, &mut state)?,
                "env" => self.add_env_vars(&theme, &mut state)?,
                "dotenv" => {
                    state.load_dotenv = !state.load_dotenv;
                }
                "lockfile" => {
                    state.create_lockfile = !state.create_lockfile;
                }
                "done" => break,
                _ => {}
            }
        }

        Ok(self.generate_config(&state))
    }

    fn show_main_menu(&self, theme: &demand::Theme, state: &WizardState) -> Result<String> {
        let tools_label = format!("Add/edit tools ({})", state.tools.len());
        let env_label = format!("Add/edit environment variables ({})", state.env_vars.len());
        let dotenv_status = if state.load_dotenv { "✓" } else { " " };
        let dotenv_label = format!("[{}] Load .env file", dotenv_status);
        let lockfile_status = if state.create_lockfile { "✓" } else { " " };
        let lockfile_label = format!("[{}] Enable mise.lock", lockfile_status);

        let mut select = Select::new("Configure mise.toml")
            .description("Select an option")
            .theme(theme);

        select = select.option(
            DemandOption::new("tools")
                .label(&tools_label)
                .description("Select tools from the registry"),
        );
        select = select.option(
            DemandOption::new("env")
                .label(&env_label)
                .description("Set environment variables"),
        );

        if Path::new(".env").exists() {
            select = select.option(
                DemandOption::new("dotenv")
                    .label(&dotenv_label)
                    .description("Include mise.file = \".env\" in config"),
            );
        }

        select = select.option(
            DemandOption::new("lockfile")
                .label(&lockfile_label)
                .description("Create lockfile for reproducible installs"),
        );

        select = select.option(
            DemandOption::new("done")
                .label("Done - generate config")
                .description("Write the configuration file"),
        );

        match select.run() {
            Ok(choice) => Ok(choice.to_string()),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {
                std::process::exit(130);
            }
            Err(e) => Err(eyre!(e)),
        }
    }

    fn add_tools(&self, theme: &demand::Theme, state: &mut WizardState) -> Result<()> {
        let tools: Vec<(&str, &RegistryTool)> = REGISTRY
            .iter()
            .map(|(name, tool)| (*name, tool))
            .sorted_by_key(|(name, _)| *name)
            .collect();

        let mut ms = MultiSelect::new("Select tools")
            .description("Space to select, Enter to confirm")
            .filterable(true)
            .theme(theme);

        for (name, tool) in tools {
            let description = tool.description.unwrap_or_default();
            let mut opt = DemandOption::new(name).label(name).description(description);
            if state.tools.contains_key(name) {
                opt = opt.selected(true);
            }
            ms = ms.option(opt);
        }

        let selected: Vec<String> = match ms.run() {
            Ok(s) => s.into_iter().map(|s| s.to_string()).collect(),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => return Ok(()),
            Err(e) => return Err(eyre!(e)),
        };

        // Remove tools that were deselected
        state.tools.retain(|k, _| selected.contains(k));

        // Add newly selected tools (prompt for version)
        for tool_name in selected {
            if !state.tools.contains_key(&tool_name) {
                let version = Input::new(format!("Version for {tool_name}"))
                    .description("e.g., '20', '3.12', 'latest'")
                    .placeholder("latest")
                    .theme(theme)
                    .run()
                    .map_err(handle_interrupt)?;
                let version = if version.is_empty() {
                    "latest".to_string()
                } else {
                    version
                };
                state.tools.insert(tool_name, version);
            }
        }

        Ok(())
    }

    fn add_env_vars(&self, theme: &demand::Theme, state: &mut WizardState) -> Result<()> {
        loop {
            // Show current env vars
            if !state.env_vars.is_empty() {
                miseprintln!("\nCurrent environment variables:");
                for (k, v) in &state.env_vars {
                    miseprintln!("  {k} = {v}");
                }
                miseprintln!();
            }

            let key = Input::new("Environment variable name")
                .description("Leave empty to finish")
                .theme(theme)
                .run()
                .map_err(handle_interrupt)?;

            if key.is_empty() {
                break;
            }

            let default_value = state.env_vars.get(&key).cloned().unwrap_or_default();
            let mut input = Input::new(format!("Value for {key}")).theme(theme);
            if !default_value.is_empty() {
                input = input.placeholder(&default_value);
            }

            let value = input.run().map_err(handle_interrupt)?;

            if value.is_empty() && !default_value.is_empty() {
                // Keep existing value
            } else {
                state.env_vars.insert(key, value);
            }
        }

        Ok(())
    }

    /// Detect tools by scanning for files listed in registry detect fields
    fn detect_tools(&self) -> Vec<DetectedTool> {
        let cwd = env::current_dir().unwrap_or_default();
        let mut detected = Vec::new();
        let mut seen_tools = std::collections::HashSet::new();

        // Scan registry for tools with detect files
        for (name, tool) in REGISTRY.iter() {
            if tool.detect.is_empty() {
                continue;
            }

            for detect_file in tool.detect.iter() {
                let path = cwd.join(detect_file);
                if path.exists() && !seen_tools.contains(*name) {
                    let version = self.extract_version(*name, &path);
                    detected.push(DetectedTool {
                        name: name.to_string(),
                        version,
                        source: detect_file.to_string(),
                    });
                    seen_tools.insert(*name);
                    break; // Only detect once per tool
                }
            }
        }

        // Sort by tool name for consistent output
        detected.sort_by(|a, b| a.name.cmp(&b.name));
        detected
    }

    /// Try to extract version from a detected file
    fn extract_version(&self, tool: &str, path: &Path) -> Option<String> {
        let filename = path.file_name()?.to_str()?;
        let content = file::read_to_string(path).ok()?;

        match (tool, filename) {
            // Node.js version from package.json engines
            ("node", "package.json") => {
                let json: serde_json::Value = serde_json::from_str(&content).ok()?;
                json.get("engines")
                    .and_then(|e| e.get("node"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            }
            // Python version from pyproject.toml
            ("python", "pyproject.toml") => {
                let doc: toml::Value = toml::from_str(&content).ok()?;
                doc.get("project")
                    .and_then(|p| p.get("requires-python"))
                    .and_then(|v| v.as_str())
                    .map(|s| {
                        s.trim_start_matches(|c: char| !c.is_ascii_digit())
                            .to_string()
                    })
                    .filter(|s| !s.is_empty())
            }
            // Go version from go.mod
            ("go", "go.mod") => content
                .lines()
                .find(|line| line.starts_with("go "))
                .map(|line| line.trim_start_matches("go ").trim().to_string()),
            // Version files (simple text content)
            (_, f) if f.starts_with('.') && f.ends_with("-version") => {
                let v = content.trim().to_string();
                if v.is_empty() { None } else { Some(v) }
            }
            (_, ".nvmrc") => {
                let v = content.trim().to_string();
                if v.is_empty() { None } else { Some(v) }
            }
            _ => None,
        }
    }

    fn generate_config(&self, state: &WizardState) -> String {
        let mut config = String::new();

        // Add lockfile setting if requested
        if state.create_lockfile {
            config.push_str("[settings]\nlockfile = true\n\n");
        }

        // Add env section if needed
        if !state.env_vars.is_empty() || state.load_dotenv {
            config.push_str("[env]\n");
            if state.load_dotenv {
                config.push_str("mise.file = \".env\"\n");
            }
            for (key, value) in &state.env_vars {
                config.push_str(&format!("{key} = {}\n", quote_toml_value(value)));
            }
            config.push('\n');
        }

        // Add tools section
        if !state.tools.is_empty() {
            config.push_str("[tools]\n");
            for (name, version) in &state.tools {
                config.push_str(&format!("{name} = {}\n", quote_toml_value(version)));
            }
        }

        if config.is_empty() {
            // Return the default template if nothing was configured
            self.default()
        } else {
            config
        }
    }

    async fn tool_versions(&self, tool_versions: &Path) -> Result<String> {
        let to =
            config_file::parse_or_init(&PathBuf::from(&*env::MISE_DEFAULT_CONFIG_FILENAME)).await?;
        let from = config_file::parse(tool_versions).await?;
        let tools = from.to_tool_request_set()?.tools;
        for (ba, tools) in tools {
            to.replace_versions(&ba, tools)?;
        }
        to.dump()
    }

    fn default(&self) -> String {
        formatdoc! {r#"
            # mise config files are hierarchical. mise will find all of the config files
            # in all parent directories and merge them together.
            # You might have a structure like:
            #
            # * ~/work/project/mise.toml   # a config file for a specific work project
            # * ~/work/mise.toml           # a config file for projects related to work
            # * ~/.config/mise/config.toml # the global config file
            # * /etc/mise/config.toml      # the system config file
            #
            # This setup allows you to define default versions and configuration across
            # all projects but override them for specific projects.

            # [env]
            # NODE_ENV = "development"
            # mise.file = ".env"                # load vars from a dotenv file
            # mise.path = "./node_modules/.bin" # add a directory to PATH

            # [tools]
            # node = "22"
            # python = "3.12"
            # go = "latest"
        "#}
    }
}

fn quote_toml_value(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

fn handle_interrupt(e: std::io::Error) -> eyre::Error {
    if e.kind() == std::io::ErrorKind::Interrupted {
        std::process::exit(130);
    }
    eyre!(e)
}

pub static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise config generate</bold>         <dim># creates mise.toml</dim>
    $ <bold>mise config generate -o .mise.toml</bold>
    $ <bold>mise config generate -y</bold>      <dim># skip interactive wizard</dim>
    $ <bold>mise config generate -n</bold>      <dim># preview without writing</dim>
"#
);
