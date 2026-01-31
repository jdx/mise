use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use clap::ValueHint;
use eyre::{Result, eyre};
use indoc::formatdoc;
use mise_interactive_config::{
    BackendInfo, BackendProvider, ConfigResult, InteractiveConfig, ToolInfo, ToolProvider,
    VersionProvider,
};

use strum::IntoEnumIterator;

use crate::backend::backend_type::BackendType;
use crate::cli::args::BackendArg;
use crate::cli::version::VERSION_PLAIN;
use crate::config::config_file;
use crate::config::{Config, Settings};
use crate::file::display_path;
use crate::plugins::PluginType;
use crate::registry::REGISTRY;
use crate::toolset::install_state;
use crate::ui::progress_report::{ProgressIcon, SingleReport};
use crate::{env, file};

/// Tool provider that lists tools from the mise REGISTRY
struct MiseToolProvider;

impl ToolProvider for MiseToolProvider {
    fn list_tools(&self) -> Vec<ToolInfo> {
        REGISTRY
            .iter()
            .map(|(name, rt)| ToolInfo {
                name: name.to_string(),
                description: rt.description.map(|s| s.to_string()),
                aliases: rt.aliases.iter().map(|s| s.to_string()).collect(),
            })
            .collect()
    }
}

/// Version provider that fetches latest versions from backends
struct MiseVersionProvider;

#[async_trait]
impl VersionProvider for MiseVersionProvider {
    async fn latest_version(&self, tool: &str) -> Option<String> {
        // Create BackendArg from tool name
        let ba = Arc::new(BackendArg::from(tool));

        // Get the backend
        let backend = ba.backend().ok()?;

        // Get config
        let config = Config::get().await.ok()?;

        // Get the latest version
        backend.latest_version(&config, None).await.ok()?
    }
}

/// Backend provider that lists available backends
struct MiseBackendProvider;

impl BackendProvider for MiseBackendProvider {
    fn list_backends(&self) -> Vec<BackendInfo> {
        let mut backends = Vec::new();

        // Add built-in backend types (skip Core, Unknown, and Vfox/VfoxBackend which are for plugins)
        for backend_type in BackendType::iter() {
            let (name, description) = match backend_type {
                BackendType::Aqua => ("aqua", Some("Install tools from aquaproj registry")),
                BackendType::Asdf => ("asdf", Some("Install tools via asdf plugins")),
                BackendType::Cargo => ("cargo", Some("Install Rust packages from crates.io")),
                BackendType::Conda => ("conda", Some("Install packages from conda-forge")),
                BackendType::Dotnet => ("dotnet", Some("Install .NET tools")),
                BackendType::Forgejo => ("forgejo", Some("Install from Forgejo releases")),
                BackendType::Gem => ("gem", Some("Install Ruby gems")),
                BackendType::Github => ("github", Some("Install from GitHub releases")),
                BackendType::Gitlab => ("gitlab", Some("Install from GitLab releases")),
                BackendType::Go => ("go", Some("Install Go modules")),
                BackendType::Npm => ("npm", Some("Install npm packages globally")),
                BackendType::Pipx => ("pipx", Some("Install Python CLI tools")),
                BackendType::Spm => ("spm", Some("Install Swift packages")),
                BackendType::Http => ("http", Some("Download files from HTTP URLs")),
                BackendType::S3 => ("s3", Some("Download from S3 buckets")),
                BackendType::Ubi => ("ubi", Some("Universal Binary Installer")),
                // Skip internal/meta types
                BackendType::Core
                | BackendType::Vfox
                | BackendType::VfoxBackend(_)
                | BackendType::Unknown => continue,
            };

            // Skip experimental backends unless experimental mode is enabled
            if backend_type.is_experimental() && !Settings::get().experimental {
                continue;
            }

            backends.push(BackendInfo {
                name: name.to_string(),
                description: description.map(|s| s.to_string()),
            });
        }

        // Add plugin-provided backends (vfox-backend plugins)
        for (plugin_name, plugin_type) in install_state::list_plugins().iter() {
            if *plugin_type == PluginType::VfoxBackend {
                backends.push(BackendInfo {
                    name: plugin_name.clone(),
                    description: Some(format!("Plugin-provided backend: {}", plugin_name)),
                });
            }
        }

        backends
    }
}

/// Edit mise.toml interactively
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Edit {
    /// Show what would be generated without writing to file
    #[clap(long, short = 'n')]
    dry_run: bool,
    /// Path to the config file to create
    #[clap(verbatim_doc_comment, value_hint = ValueHint::FilePath)]
    path: Option<PathBuf>,
    /// Path to a .tool-versions file to import tools from
    #[clap(long, short, verbatim_doc_comment, value_hint = ValueHint::FilePath)]
    tool_versions: Option<PathBuf>,
}

/// A detected tool with its source and suggested version
#[derive(Debug, Clone)]
struct DetectedTool {
    name: String,
    version: Option<String>,
    #[allow(dead_code)]
    source: String,
}

impl Edit {
    pub async fn run(self) -> Result<()> {
        let path = self
            .path
            .clone()
            .unwrap_or_else(|| PathBuf::from(&*env::MISE_DEFAULT_CONFIG_FILENAME));

        if let Some(tool_versions) = &self.tool_versions {
            // Import from .tool-versions file
            let doc = self.tool_versions(tool_versions).await?;

            if self.dry_run {
                info!("would write to {}", display_path(&path));
                miseprintln!("{doc}");
            } else {
                info!("writing to {}", display_path(&path));
                file::write(&path, doc)?;
            }
        } else if self.should_run_interactive() {
            // Run interactive TOML editor
            self.interactive(&path).await?;
        } else {
            // Non-interactive: output default template
            let doc = self.default();

            if self.dry_run {
                info!("would write to {}", display_path(&path));
                miseprintln!("{doc}");
            } else {
                info!("writing to {}", display_path(&path));
                file::write(&path, doc)?;
            }
        }

        Ok(())
    }

    fn should_run_interactive(&self) -> bool {
        !Settings::get().yes && console::user_attended_stderr()
    }

    async fn interactive(&self, path: &Path) -> Result<()> {
        use crate::ui::progress_report::ProgressReport;

        let title = format!("mise {} by @jdx", &*VERSION_PLAIN);

        // Show loading spinner while setting up
        let pr = ProgressReport::new("edit".into());
        pr.set_message("Loading...".into());

        // Create the interactive config editor
        let mut editor = if path.exists() {
            pr.set_message("Loading config...".into());
            InteractiveConfig::open(path.to_path_buf()).map_err(|e| eyre!(e))?
        } else {
            InteractiveConfig::new(path.to_path_buf())
        };

        editor = editor
            .title(&title)
            .dry_run(self.dry_run)
            .with_tool_provider(Box::new(MiseToolProvider))
            .with_version_provider(Box::new(MiseVersionProvider))
            .with_backend_provider(Box::new(MiseBackendProvider));

        // Auto-detect tools and add them
        pr.set_message("Detecting tools...".into());
        let detected = detect_tools();
        for tool in detected {
            let version = tool.version.unwrap_or_else(|| "latest".to_string());
            editor.add_tool(&tool.name, &version);
        }

        // Auto-detect prepare providers if experimental is enabled
        if Settings::get().experimental {
            pr.set_message("Detecting prepare providers...".into());
            let cwd = env::current_dir().unwrap_or_default();
            let prepare_providers = crate::prepare::detect_applicable_providers(&cwd);
            for provider in prepare_providers {
                editor.add_prepare(&provider);
            }
        }

        // Clear the loading spinner before starting the TUI
        pr.finish_with_icon("Ready".into(), ProgressIcon::Success);

        // Run the editor (now async)
        match editor.run().await {
            Ok(ConfigResult::Saved(content)) => {
                if self.dry_run {
                    info!("would write to {}", display_path(path));
                    miseprintln!("{content}");
                } else {
                    info!("saved to {}", display_path(path));
                }
            }
            Ok(ConfigResult::Cancelled) => {
                info!("cancelled");
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {
                std::process::exit(130);
            }
            Err(e) => return Err(eyre!(e)),
        }

        Ok(())
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

// ============================================================================
// Tool detection
// ============================================================================

fn detect_tools() -> Vec<DetectedTool> {
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
                let version = extract_version(name, &path);
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

fn extract_version(tool: &str, path: &Path) -> Option<String> {
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

pub static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise edit</bold>             <dim># edit mise.toml interactively</dim>
    $ <bold>mise edit .mise.toml</bold>  <dim># edit a specific file</dim>
    $ <bold>mise edit -y</bold>          <dim># skip interactive editor</dim>
    $ <bold>mise edit -n</bold>          <dim># preview without writing</dim>
"#
);
