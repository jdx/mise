use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use clap::ValueHint;
use demand::{Confirm, DemandOption, Input, MultiSelect};
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

        // Step 1: Detect tools from project files
        let detected = self.detect_tools().await;
        let mut tools: BTreeMap<String, String> = BTreeMap::new();

        // Step 2: If we detected tools, ask user to confirm them
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
                    tools.insert(tool.name.clone(), version);
                }
            }
        }

        // Step 3: Ask if user wants to add more tools
        let add_more = Confirm::new("Add additional tools?")
            .theme(&theme)
            .run()
            .map_err(handle_interrupt)?;

        if add_more {
            let additional = self.select_tools(&theme)?;
            for tool_name in additional {
                if !tools.contains_key(&tool_name) {
                    // Ask for version
                    let version = Input::new(format!("Version for {tool_name}"))
                        .description("Enter version (e.g., '20', '3.12', 'latest')")
                        .placeholder("latest")
                        .theme(&theme)
                        .run()
                        .map_err(handle_interrupt)?;
                    let version = if version.is_empty() {
                        "latest".to_string()
                    } else {
                        version
                    };
                    tools.insert(tool_name, version);
                }
            }
        }

        // Step 4: Ask about environment variables
        let mut env_vars: BTreeMap<String, String> = BTreeMap::new();
        let add_env = Confirm::new("Add environment variables?")
            .theme(&theme)
            .run()
            .map_err(handle_interrupt)?;

        if add_env {
            loop {
                let key = Input::new("Environment variable name")
                    .description("Leave empty to finish")
                    .theme(&theme)
                    .run()
                    .map_err(handle_interrupt)?;

                if key.is_empty() {
                    break;
                }

                let value = Input::new(format!("Value for {key}"))
                    .theme(&theme)
                    .run()
                    .map_err(handle_interrupt)?;

                env_vars.insert(key, value);
            }
        }

        // Step 5: Ask about .env file
        let load_dotenv = if Path::new(".env").exists() {
            Confirm::new("Load variables from .env file?")
                .theme(&theme)
                .run()
                .map_err(handle_interrupt)?
        } else {
            false
        };

        // Generate the config
        Ok(self.generate_config(&tools, &env_vars, load_dotenv))
    }

    async fn detect_tools(&self) -> Vec<DetectedTool> {
        let mut detected = Vec::new();
        let cwd = env::current_dir().unwrap_or_default();

        // Detect Node.js from package.json
        if let Some(tool) = self.detect_node(&cwd).await {
            detected.push(tool);
        }

        // Detect Node.js from .nvmrc or .node-version
        if let Some(tool) = self.detect_node_version_file(&cwd) {
            // Only add if we didn't already detect node
            if !detected.iter().any(|t| t.name == "node") {
                detected.push(tool);
            }
        }

        // Detect Python from pyproject.toml or .python-version
        if let Some(tool) = self.detect_python(&cwd).await {
            detected.push(tool);
        }

        // Detect Go from go.mod
        if let Some(tool) = self.detect_go(&cwd).await {
            detected.push(tool);
        }

        // Detect Ruby from .ruby-version or Gemfile
        if let Some(tool) = self.detect_ruby(&cwd) {
            detected.push(tool);
        }

        // Detect package managers
        detected.extend(self.detect_package_managers(&cwd));

        detected
    }

    async fn detect_node(&self, cwd: &Path) -> Option<DetectedTool> {
        let package_json = cwd.join("package.json");
        if !package_json.exists() {
            return None;
        }

        let content = file::read_to_string(&package_json).ok()?;
        let json: serde_json::Value = serde_json::from_str(&content).ok()?;

        let version = json
            .get("engines")
            .and_then(|e| e.get("node"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        Some(DetectedTool {
            name: "node".to_string(),
            version,
            source: "package.json".to_string(),
        })
    }

    fn detect_node_version_file(&self, cwd: &Path) -> Option<DetectedTool> {
        for (filename, source) in [(".nvmrc", ".nvmrc"), (".node-version", ".node-version")] {
            let path = cwd.join(filename);
            if path.exists() {
                let version = file::read_to_string(&path)
                    .ok()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty());
                return Some(DetectedTool {
                    name: "node".to_string(),
                    version,
                    source: source.to_string(),
                });
            }
        }
        None
    }

    async fn detect_python(&self, cwd: &Path) -> Option<DetectedTool> {
        // Check .python-version first
        let python_version = cwd.join(".python-version");
        if python_version.exists() {
            let version = file::read_to_string(&python_version)
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
            return Some(DetectedTool {
                name: "python".to_string(),
                version,
                source: ".python-version".to_string(),
            });
        }

        // Check pyproject.toml
        let pyproject = cwd.join("pyproject.toml");
        if pyproject.exists() {
            let content = file::read_to_string(&pyproject).ok()?;
            let version = self.parse_python_version_from_pyproject(&content);
            return Some(DetectedTool {
                name: "python".to_string(),
                version,
                source: "pyproject.toml".to_string(),
            });
        }

        None
    }

    fn parse_python_version_from_pyproject(&self, content: &str) -> Option<String> {
        // Try to parse requires-python from pyproject.toml
        let doc: toml::Value = toml::from_str(content).ok()?;
        doc.get("project")
            .and_then(|p| p.get("requires-python"))
            .and_then(|v| v.as_str())
            .map(|s| {
                // Convert ">=3.10" to "3.10", etc.
                s.trim_start_matches(|c: char| !c.is_ascii_digit())
                    .to_string()
            })
            .filter(|s| !s.is_empty())
    }

    async fn detect_go(&self, cwd: &Path) -> Option<DetectedTool> {
        let go_mod = cwd.join("go.mod");
        if !go_mod.exists() {
            return None;
        }

        let content = file::read_to_string(&go_mod).ok()?;
        let version = content
            .lines()
            .find(|line| line.starts_with("go "))
            .map(|line| line.trim_start_matches("go ").trim().to_string());

        Some(DetectedTool {
            name: "go".to_string(),
            version,
            source: "go.mod".to_string(),
        })
    }

    fn detect_ruby(&self, cwd: &Path) -> Option<DetectedTool> {
        // Check .ruby-version
        let ruby_version = cwd.join(".ruby-version");
        if ruby_version.exists() {
            let version = file::read_to_string(&ruby_version)
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
            return Some(DetectedTool {
                name: "ruby".to_string(),
                version,
                source: ".ruby-version".to_string(),
            });
        }

        // Check Gemfile for ruby version
        let gemfile = cwd.join("Gemfile");
        if gemfile.exists() {
            return Some(DetectedTool {
                name: "ruby".to_string(),
                version: None,
                source: "Gemfile".to_string(),
            });
        }

        None
    }

    fn detect_package_managers(&self, cwd: &Path) -> Vec<DetectedTool> {
        let mut detected = Vec::new();

        // Detect pnpm
        if cwd.join("pnpm-lock.yaml").exists() {
            detected.push(DetectedTool {
                name: "pnpm".to_string(),
                version: None,
                source: "pnpm-lock.yaml".to_string(),
            });
        }

        // Detect yarn
        if cwd.join("yarn.lock").exists() {
            detected.push(DetectedTool {
                name: "yarn".to_string(),
                version: None,
                source: "yarn.lock".to_string(),
            });
        }

        // Detect bun
        if cwd.join("bun.lockb").exists() || cwd.join("bun.lock").exists() {
            detected.push(DetectedTool {
                name: "bun".to_string(),
                version: None,
                source: "bun.lock".to_string(),
            });
        }

        // Detect uv
        if cwd.join("uv.lock").exists() {
            detected.push(DetectedTool {
                name: "uv".to_string(),
                version: None,
                source: "uv.lock".to_string(),
            });
        }

        // Detect poetry
        if cwd.join("poetry.lock").exists() {
            detected.push(DetectedTool {
                name: "poetry".to_string(),
                version: None,
                source: "poetry.lock".to_string(),
            });
        }

        detected
    }

    fn select_tools(&self, theme: &demand::Theme) -> Result<Vec<String>> {
        let tools: Vec<(&str, &RegistryTool)> = REGISTRY
            .iter()
            .map(|(name, tool)| (*name, tool))
            .sorted_by_key(|(name, _)| *name)
            .collect();

        let mut ms = MultiSelect::new("Select tools")
            .description("Use arrows to move, space to select, enter to confirm")
            .filterable(true)
            .theme(theme);

        for (name, tool) in tools {
            let description = tool.description.unwrap_or_default();
            ms = ms.option(DemandOption::new(name).label(name).description(description));
        }

        match ms.run() {
            Ok(selected) => Ok(selected.into_iter().map(|s| s.to_string()).collect()),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => Ok(vec![]),
            Err(e) => Err(eyre!(e)),
        }
    }

    fn generate_config(
        &self,
        tools: &BTreeMap<String, String>,
        env_vars: &BTreeMap<String, String>,
        load_dotenv: bool,
    ) -> String {
        let mut config = String::new();

        // Add env section if needed
        if !env_vars.is_empty() || load_dotenv {
            config.push_str("[env]\n");
            if load_dotenv {
                config.push_str("mise.file = \".env\"\n");
            }
            for (key, value) in env_vars {
                config.push_str(&format!("{key} = {}\n", quote_toml_value(value)));
            }
            config.push('\n');
        }

        // Add tools section
        if !tools.is_empty() {
            config.push_str("[tools]\n");
            for (name, version) in tools {
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
