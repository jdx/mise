use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use eyre::{Result, WrapErr, bail, eyre};
use indoc::formatdoc;
use mise_interactive_config::{
    BackendInfo, BackendProvider, ConfigResult, InteractiveConfig, ToolInfo, ToolProvider,
    VersionProvider,
};

use strum::IntoEnumIterator;

use crate::args::BackendArg;
use crate::backend::backend_type::BackendType;
use crate::config::config_file;
use crate::config::{Config, Settings, global_config_path};
use crate::file::display_path;
use crate::plugins::PluginType;
use crate::registry::REGISTRY;
use crate::toolset::{install_state, validate_installable_version};
use crate::ui::progress_report::{ProgressIcon, SingleReport};
use crate::version::VERSION_PLAIN;
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
struct MiseVersionProvider {
    /// The file being edited. The editor reads it itself, not through the config loader, so
    /// its `[tools]` keys arrive here without a trust check.
    path: PathBuf,
}

#[async_trait]
impl VersionProvider for MiseVersionProvider {
    async fn latest_version(&self, tool: &str) -> Option<String> {
        // A key with inline options (`tool[api_url=...]`) can point the backend at another
        // host, which then gets the user's forge token. The config loader applies such a key
        // only from a trusted config, and so does the editor: without trust it looks nothing
        // up and the version is typed in by hand.
        if config_file::tool_key_requires_trust(tool) && !config_file::is_path_trusted(&self.path) {
            return None;
        }

        // Create BackendArg from tool name
        let ba = Arc::new(BackendArg::from(tool));

        // Get the backend
        let backend = ba.backend().ok()?;

        // Get config
        let config = Config::get().await.ok()?;

        let latest = backend.latest_version(&config, None, None).await.ok()??;
        offerable_version(latest)
    }
}

/// Keeps a version a backend reported only if the editor can save it into `[tools]` as written.
///
/// The editor offers the version, and the prefixes it cuts from it, as choices and writes the
/// chosen one with its own writer, which applies neither the check `ToolVersion::resolve` makes
/// on a resolved version nor the one `mise use --pin` makes before it writes. The text comes
/// from a version list or release API, not from the user: a template in it would render, and
/// could run commands with exec(), every time the saved config loads, and control characters
/// would reach the terminal the choices are drawn on. Nor is a selector that needs trust, such
/// as `path:`, offered: the editor may save into global config, which is trusted, and the
/// version was not the user's choice. Without a version the editor asks for one in its text
/// field.
fn offerable_version(version: String) -> Option<String> {
    if crate::tera::contains_template_syntax(&version)
        || config_file::version_requires_trust(&version)
    {
        return None;
    }
    validate_installable_version(&version).ok()?;
    Some(version)
}

/// Keeps a version read from a file such as `.nvmrc` only if it holds no template syntax or
/// control characters and is not a selector that needs trust, such as `path:`. The file belongs
/// to the project in the working directory, which may be one nobody reviewed, and the editor
/// can save the version into global config, where it would render on every load, or put a
/// directory of the project's choosing on PATH. A range such as `>=20` is legitimate here, so
/// this is looser than [`offerable_version`].
fn detected_version(version: String) -> Option<String> {
    if crate::tera::contains_template_syntax(&version)
        || version.chars().any(char::is_control)
        || config_file::version_requires_trust(&version)
    {
        return None;
    }
    Some(version)
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
                BackendType::Packslip => {
                    ("packslip", Some("Install from a vendor's signed packslip"))
                }
                BackendType::Pipx => ("pipx", Some("Install Python CLI tools")),
                BackendType::Spinel => ("spinel", Some("Compile Ruby tools with Spinel")),
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
///
/// Opens a terminal editor for PATH (default: mise.toml in the current directory)
/// where you can add tools from the registry, pick versions, and set environment
/// variables, tasks, and settings. It pre-fills tools it detects from files in the
/// current directory, such as `.node-version`.
///
/// Without an interactive terminal, or with --yes, it writes a commented starter
/// template instead. If PATH already exists it stops with an error rather than
/// replace the file; pass --force to replace it. Use --dry-run to print the result
/// without writing it.
#[derive(Debug, usage_rs::Args)]
#[usage(
    verbatim_doc_comment,
    example("mise edit", help = "Edit mise.toml in the current directory"),
    example("mise edit mise.local.toml", help = "Edit another config file"),
    example("mise edit -g", help = "Edit the global config"),
    example(
        "mise edit -t .tool-versions",
        help = "Copy the tools from .tool-versions into mise.toml"
    ),
    example("mise edit -n", help = "Print the result instead of writing it"),
    example(
        "mise edit -y new.toml",
        help = "Write a commented starter config to a new file without opening the editor"
    ),
    example(
        "mise edit -y --force",
        help = "Replace an existing mise.toml with the starter config"
    )
)]
pub(crate) struct Edit {
    /// Edit the global config file (~/.config/mise/config.toml)
    // Rejected alongside a path rather than resolved in its favour: "edit the global config
    // file, namely ./custom.toml" has no meaning, and resolving it silently is how
    // `mise edit config --global` came to write a file called `config` into the current
    // directory and report success. `mise dot add` states the same collision the
    // same way, with `conflicts_with_all` between its own `--global` and `--path`.
    #[usage(long, short = 'g', conflicts = "path")]
    global: bool,
    /// Print the result instead of writing it to the file
    #[usage(long, short = 'n')]
    dry_run: bool,
    /// Replace an existing file with the starter template when not opening the editor
    #[usage(long, short)]
    force: bool,
    /// Config file to edit or create; defaults to mise.toml
    #[usage(verbatim_doc_comment, value_hint = ValueHint::FilePath)]
    path: Option<PathBuf>,
    /// Copy the tools from this .tool-versions file into the config, without opening the editor
    #[usage(long, short, value_name = "FILE", verbatim_doc_comment, value_hint = ValueHint::FilePath)]
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
    pub(super) fn is_dry_run(&self) -> bool {
        self.dry_run
    }

    pub(crate) fn new(
        global: bool,
        dry_run: bool,
        force: bool,
        path: Option<PathBuf>,
        tool_versions: Option<PathBuf>,
    ) -> Self {
        Self {
            global,
            dry_run,
            force,
            path,
            tool_versions,
        }
    }

    pub(crate) async fn run(self) -> Result<()> {
        let path = if let Some(path) = self.path.clone() {
            path
        } else if self.global {
            global_config_path()
        } else {
            PathBuf::from(&*env::MISE_DEFAULT_CONFIG_FILENAME)
        };
        if !self.dry_run {
            config_file::symlink_check_before_write(&path)?;
        }

        if let Some(tool_versions) = &self.tool_versions {
            // Import from .tool-versions file
            let doc = self.tool_versions(tool_versions, &path).await?;

            if self.dry_run {
                info!("would write to {}", display_path(&path));
                miseprintln!("{doc}");
            } else {
                info!("writing to {}", display_path(&path));
                write_checked_config(&path, doc)?;
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
            } else if self.force {
                info!("writing to {}", display_path(&path));
                write_checked_config(&path, doc)?;
            } else {
                info!("writing to {}", display_path(&path));
                write_new_config(&path, &doc)?;
            }
        }

        Ok(())
    }

    fn should_run_interactive(&self) -> bool {
        !Settings::get().yes && console::user_attended_stderr()
    }

    async fn interactive(&self, path: &Path) -> Result<()> {
        use crate::ui::progress_report::ProgressReport;

        let title = format!("mise {} by @jdx", *VERSION_PLAIN);

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
            // The file can become a link while the editor is open, so where it leads is
            // checked again when it is saved.
            .before_save(|path| {
                config_file::checked_write_destination(path)
                    .map_err(|e| std::io::Error::other(e.to_string()))
            })
            .with_tool_provider(Box::new(MiseToolProvider))
            .with_version_provider(Box::new(MiseVersionProvider {
                path: env::current_dir().unwrap_or_default().join(path),
            }))
            .with_backend_provider(Box::new(MiseBackendProvider));

        // Auto-detect tools and add them
        pr.set_message("Detecting tools...".into());
        let detected = detect_tools();
        for tool in detected {
            let version = tool.version.unwrap_or_else(|| "latest".to_string());
            editor.add_tool(&tool.name, &version);
        }

        // Auto-detect deps providers if experimental is enabled
        if Settings::get().experimental {
            pr.set_message("Detecting deps providers...".into());
            let cwd = env::current_dir().unwrap_or_default();
            let deps_providers = crate::deps::detect_applicable_providers(&cwd);
            for provider in deps_providers {
                editor.add_deps(&provider);
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
                return Err(crate::request_exit(130));
            }
            Err(e) => return Err(eyre!(e)),
        }

        Ok(())
    }

    async fn tool_versions(&self, tool_versions: &Path, path: &Path) -> Result<String> {
        // A dry run writes nothing, so it previews a dangling link's new config instead of
        // refusing it the way a write does.
        let to = if self.dry_run && path.is_symlink() && !path.exists() {
            config_file::init(path).await?
        } else {
            config_file::parse_or_init(path).await?
        };
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
            # _.file = ".env"                # load vars from a dotenv file
            # _.path = "./node_modules/.bin" # add a directory to PATH

            # [tools]
            # node = "22"
            # python = "3.12"
            # go = "latest"
        "#}
    }
}

/// Write a generated config, creating its directory if it is not there yet.
///
/// `--global` resolves to `~/.config/mise/config.toml`, and nothing creates that directory
/// ahead of time. Every other writer of that same file goes through `MiseToml::save`, which
/// has always created the parent first — these two commands reached `file::write` directly, so
/// on a fresh install, where `mise generate config --global` is the first thing to touch the
/// path, they failed with a bare "no such file or directory" and wrote nothing.
///
/// A bare relative name (`mise edit foo.toml`) gives an empty parent, which `create_dir_all`
/// treats as a no-op.
fn write_config(path: &Path, doc: String) -> Result<()> {
    if let Some(parent) = path.parent() {
        file::create_dir_all(parent)?;
    }
    file::write(path, doc)
}

/// [`write_config`] to the file `path` leads to, checked again just before the write: building
/// the document awaits (an import parses both files), and a link can change in the meantime.
/// Writing to the destination the check approved, rather than to `path`, is what the editor's
/// save does too.
fn write_checked_config(path: &Path, doc: String) -> Result<()> {
    let destination = config_file::checked_write_destination(path)?;
    write_config(&destination, doc)
}

/// Write the default template to a config that must not exist yet.
///
/// The template is a fresh file, not an edit of the one already there, so writing it over an
/// existing config would throw that config away. The interactive editor opens an existing file
/// and `--tool-versions` merges into it; only the template branch replaces it, and it is the
/// one reached by `-y` or by running without a terminal (a script, CI, an editor's task
/// runner), where nobody is watching to notice. `create_new` makes the "does it exist" check
/// and the creation one step, so a config another process writes in between is not replaced.
fn write_new_config(path: &Path, doc: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        file::create_dir_all(parent)?;
    }
    // Create the directory of the file a dangling link names, too: the link may point into a
    // directory that does not exist yet.
    let target = symlink_target(path);
    if let Some(parent) = target.parent() {
        file::create_dir_all(parent)?;
    }
    let mut f = match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&target)
    {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => bail!(
            "{} already exists; to edit it, run this command in a terminal without --yes, \
             or pass --force to replace it with the starter template",
            display_path(path)
        ),
        Err(e) => {
            return Err(e).wrap_err_with(|| format!("failed create: {}", display_path(path)));
        }
    };
    f.write_all(doc.as_bytes())
        .wrap_err_with(|| format!("failed write: {}", display_path(path)))
}

/// Follow `path` through any symlinks to the file they finally name, which need not exist.
///
/// `create_new` does not follow a symlink in the last component: a link whose target is not
/// there yet, as a dotfile manager lays down before it writes the file, counts as existing.
/// Opening the target instead keeps such a link writable, while a link to a real config still
/// fails because its target exists. A chain longer than the kernel's own limit is left as is,
/// and `create_new` then reports it as existing.
fn symlink_target(path: &Path) -> PathBuf {
    let mut target = path.to_path_buf();
    for _ in 0..40 {
        let Ok(link) = std::fs::read_link(&target) else {
            break;
        };
        target = match target.parent() {
            Some(dir) => dir.join(link),
            None => link,
        };
    }
    target
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
            if path.exists() && !seen_tools.contains(name) {
                let version = extract_version(name, &path).and_then(detected_version);
                detected.push(DetectedTool {
                    name: name.to_string(),
                    version,
                    source: detect_file.to_string(),
                });
                seen_tools.insert(name);
                break; // Only detect once per tool
            }
        }
    }

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offerable_version_keeps_a_plain_version() {
        for v in ["1.2.3", "2024.01.15", "1.2.3+build", "21.0.2-tem"] {
            assert_eq!(offerable_version(v.to_string()).as_deref(), Some(v));
        }
    }

    #[test]
    fn offerable_version_drops_a_version_the_config_would_render_or_could_not_install() {
        for v in [
            "1.2.3{{exec(command='id')}}",
            "1.2.3{{env.HOME}}",
            "1.2.3{%if true%}{%endif%}",
            "1.2.3{#\x1b[42D\x1b[K#}",
            "1.2.3\x1b[2K",
            "1.2.3$(id)",
            "1.2.3 ; id",
            "../1.2.3",
            "",
            // a selector that takes effect only in a trusted config
            "path:bin",
            "ref-main",
            "rev:1a2b3c",
        ] {
            assert_eq!(offerable_version(v.to_string()), None, "{v:?}");
        }
    }

    #[test]
    fn detected_version_keeps_ranges_and_drops_templates() {
        for v in ["20", ">=3.11", "^1.2 || ^2", "lts/iron"] {
            assert_eq!(detected_version(v.to_string()).as_deref(), Some(v));
        }
        for v in [
            "20{{exec(command='id')}}",
            "20{# x #}",
            "20\x1b[2K",
            "path:./bin",
        ] {
            assert_eq!(detected_version(v.to_string()), None, "{v:?}");
        }
    }

    #[test]
    fn write_new_config_keeps_a_file_created_after_the_command_started() {
        // There is no existence check before the write to race with: whatever is at the path
        // when the file is opened, even a config another process has just written, is kept.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mise.toml");
        std::fs::write(&path, "[tools]\nnode = \"22\"\n").unwrap();

        let err = write_new_config(&path, "# template\n").unwrap_err();
        assert!(err.to_string().contains("already exists"), "{err}");
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "[tools]\nnode = \"22\"\n"
        );
    }

    #[cfg(unix)]
    #[test]
    fn an_import_checks_its_destination_again_when_it_saves() {
        // The config was a regular file when the command checked it, and a link out of the
        // project by the time the import has its document ready. Tests trust every project, so
        // the link leads to a file that does not exist yet, which is refused whatever the trust.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let project = root.join("swapped");
        let outside = root.join("outside.toml");
        std::fs::create_dir_all(&project).unwrap();
        let path = project.join("mise.toml");
        std::fs::write(&path, "").unwrap();
        config_file::symlink_check_before_write(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        std::os::unix::fs::symlink(&outside, &path).unwrap();

        let err = write_checked_config(&path, "[tools]\n".to_string()).unwrap_err();
        assert!(err.to_string().contains("refusing to write"), "{err}");
        assert!(!outside.exists());
    }

    #[cfg(unix)]
    #[test]
    fn write_new_config_writes_through_a_dangling_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("dotfiles/mise.toml");
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        let path = dir.path().join("mise.toml");
        std::os::unix::fs::symlink("dotfiles/mise.toml", &path).unwrap();

        write_new_config(&path, "# template\n").unwrap();
        assert!(std::fs::symlink_metadata(&path).unwrap().is_symlink());
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "# template\n");
    }

    #[cfg(unix)]
    #[test]
    fn write_new_config_creates_the_directory_a_dangling_symlink_points_into() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("dotfiles/mise.toml");
        let path = dir.path().join("mise.toml");
        std::os::unix::fs::symlink("dotfiles/mise.toml", &path).unwrap();

        write_new_config(&path, "# template\n").unwrap();
        assert!(std::fs::symlink_metadata(&path).unwrap().is_symlink());
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "# template\n");
    }

    #[cfg(unix)]
    #[test]
    fn write_new_config_keeps_the_target_of_a_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("real.toml");
        std::fs::write(&target, "[tools]\nnode = \"22\"\n").unwrap();
        let path = dir.path().join("mise.toml");
        std::os::unix::fs::symlink(&target, &path).unwrap();

        let err = write_new_config(&path, "# template\n").unwrap_err();
        assert!(err.to_string().contains("already exists"), "{err}");
        assert_eq!(
            std::fs::read_to_string(&target).unwrap(),
            "[tools]\nnode = \"22\"\n"
        );
    }

    #[test]
    fn write_new_config_creates_a_missing_file_and_its_directory() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fresh/mise/config.toml");

        write_new_config(&path, "# template\n").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "# template\n");
    }
}
