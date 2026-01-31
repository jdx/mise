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

const MAX_DESCRIPTION_LEN: usize = 60;

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
    min_mise_version: Option<String>,
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
            // Show current config preview
            self.show_config_preview(&state)?;

            let choice = self.show_main_menu(&theme, &state)?;

            match choice.as_str() {
                "add_tools" => self.add_tools(&theme, &mut state)?,
                "edit_tools" => self.edit_tools(&theme, &mut state)?,
                "env" => self.add_env_vars(&theme, &mut state)?,
                "dotenv" => {
                    state.load_dotenv = !state.load_dotenv;
                }
                "lockfile" => {
                    state.create_lockfile = !state.create_lockfile;
                }
                "min_version" => self.set_min_version(&theme, &mut state)?,
                "done" => break,
                _ => {}
            }
        }

        Ok(self.generate_config(&state))
    }

    fn show_config_preview(&self, state: &WizardState) -> Result<()> {
        let config = self.generate_config(state);
        if config != self.default() {
            miseprintln!("\n--- Current mise.toml preview ---");
            miseprintln!("{}", config.trim());
            miseprintln!("---------------------------------\n");
        }
        Ok(())
    }

    fn show_main_menu(&self, theme: &demand::Theme, state: &WizardState) -> Result<String> {
        let tools_label = format!("Add tools ({})", state.tools.len());
        let edit_tools_label = "Edit/remove tools";
        let env_label = format!("Add environment variables ({})", state.env_vars.len());
        let dotenv_status = if state.load_dotenv { "✓" } else { " " };
        let dotenv_label = format!("[{}] Load .env file", dotenv_status);
        let lockfile_status = if state.create_lockfile { "✓" } else { " " };
        let lockfile_label = format!("[{}] Enable mise.lock", lockfile_status);
        let min_version_label = match &state.min_mise_version {
            Some(v) => format!("Set min mise version ({})", v),
            None => "Set min mise version".to_string(),
        };

        let mut select = Select::new("Configure mise.toml")
            .description("Select an option")
            .theme(theme);

        select = select.option(
            DemandOption::new("add_tools")
                .label(&tools_label)
                .description("Search and add tools from the registry"),
        );

        if !state.tools.is_empty() {
            select = select.option(
                DemandOption::new("edit_tools")
                    .label(edit_tools_label)
                    .description("Change versions or remove tools"),
            );
        }

        select = select.option(
            DemandOption::new("env")
                .label(&env_label)
                .description("Set environment variables (FOO=bar format)"),
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
            DemandOption::new("min_version")
                .label(&min_version_label)
                .description("Require a minimum mise version"),
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
            .filter(|(name, _)| !state.tools.contains_key(*name))
            .sorted_by_key(|(name, _)| *name)
            .collect();

        if tools.is_empty() {
            miseprintln!("All registry tools have already been added.");
            return Ok(());
        }

        let mut ms = MultiSelect::new("Select tools to add")
            .description("Type to filter, Space to select, Enter to confirm")
            .filterable(true)
            .theme(theme);

        for (name, tool) in tools {
            let description = truncate_description(tool.description.unwrap_or_default());
            ms = ms.option(
                DemandOption::new(name)
                    .label(name)
                    .description(&description),
            );
        }

        let selected: Vec<String> = match ms.run() {
            Ok(s) => s.into_iter().map(|s| s.to_string()).collect(),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => return Ok(()),
            Err(e) => return Err(eyre!(e)),
        };

        // Prompt for version for each newly selected tool
        for tool_name in selected {
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

        Ok(())
    }

    fn edit_tools(&self, theme: &demand::Theme, state: &mut WizardState) -> Result<()> {
        if state.tools.is_empty() {
            return Ok(());
        }

        let tool_names: Vec<String> = state.tools.keys().cloned().collect();

        let mut select = Select::new("Select tool to edit")
            .description("Choose a tool to modify or remove")
            .filterable(true)
            .theme(theme);

        for name in &tool_names {
            let version = state.tools.get(name).unwrap();
            let label = format!("{} = \"{}\"", name, version);
            select = select.option(DemandOption::new(name.as_str()).label(&label));
        }

        select = select.option(DemandOption::new("__back__").label("← Back to menu"));

        let selected = match select.run() {
            Ok(s) => s.to_string(),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => return Ok(()),
            Err(e) => return Err(eyre!(e)),
        };

        if selected == "__back__" {
            return Ok(());
        }

        // Show edit options for the selected tool
        let current_version = state.tools.get(&selected).cloned().unwrap_or_default();
        let edit_label = format!("Change version (current: {})", current_version);

        let action_select = Select::new(format!("Edit {}", selected))
            .theme(theme)
            .option(DemandOption::new("edit").label(&edit_label))
            .option(DemandOption::new("remove").label("Remove tool"))
            .option(DemandOption::new("back").label("← Back"));

        let action = match action_select.run() {
            Ok(a) => a.to_string(),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => return Ok(()),
            Err(e) => return Err(eyre!(e)),
        };

        match action.as_str() {
            "edit" => {
                let new_version = Input::new(format!("New version for {}", selected))
                    .placeholder(&current_version)
                    .theme(theme)
                    .run()
                    .map_err(handle_interrupt)?;
                if !new_version.is_empty() {
                    state.tools.insert(selected, new_version);
                }
            }
            "remove" => {
                state.tools.remove(&selected);
            }
            _ => {}
        }

        Ok(())
    }

    fn add_env_vars(&self, theme: &demand::Theme, state: &mut WizardState) -> Result<()> {
        // Show current env vars
        if !state.env_vars.is_empty() {
            miseprintln!("\nCurrent environment variables:");
            for (k, v) in &state.env_vars {
                miseprintln!("  {k}={v}");
            }
            miseprintln!();
        }

        loop {
            let input = Input::new("Add environment variable")
                .description("Format: KEY=value (empty to finish, KEY= to remove)")
                .theme(theme)
                .run()
                .map_err(handle_interrupt)?;

            if input.is_empty() {
                break;
            }

            if let Some((key, value)) = input.split_once('=') {
                let key = key.trim().to_string();
                let value = value.trim().to_string();

                if key.is_empty() {
                    miseprintln!("Invalid format. Use KEY=value");
                    continue;
                }

                if value.is_empty() {
                    // Remove the variable
                    if state.env_vars.remove(&key).is_some() {
                        miseprintln!("Removed {key}");
                    }
                } else {
                    state.env_vars.insert(key.clone(), value.clone());
                    miseprintln!("Set {key}={value}");
                }
            } else {
                miseprintln!("Invalid format. Use KEY=value");
            }
        }

        Ok(())
    }

    fn set_min_version(&self, theme: &demand::Theme, state: &mut WizardState) -> Result<()> {
        let current = state.min_mise_version.as_deref().unwrap_or("not set");
        let desc = format!("Current: {} (empty to clear)", current);
        let version = Input::new("Minimum mise version")
            .description(&desc)
            .placeholder("2024.0.0")
            .theme(theme)
            .run()
            .map_err(handle_interrupt)?;

        state.min_mise_version = if version.is_empty() {
            None
        } else {
            Some(version)
        };

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

        // Add min_version if set
        if let Some(ref version) = state.min_mise_version {
            config.push_str(&format!("min_version = \"{}\"\n\n", version));
        }

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

fn truncate_description(desc: &str) -> String {
    if desc.len() <= MAX_DESCRIPTION_LEN {
        desc.to_string()
    } else {
        format!("{}…", &desc[..MAX_DESCRIPTION_LEN - 1])
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
