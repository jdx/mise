use crate::Result;
use crate::cmd::{RunningPidGuard, prepare_noninteractive_child};
use crate::config::{Config, Settings};
use clap::Parser;
use rmcp::{
    RoleServer, ServiceExt,
    handler::server::{ServerHandler, tool::ToolRouter, wrapper::Parameters},
    model::{
        CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, ErrorCode,
        ErrorData, Implementation, ListResourcesResult, ListToolsResult, PaginatedRequestParams,
        ProtocolVersion, ReadResourceRequestParams, ReadResourceResponse, ReadResourceResult,
        Resource, ResourceContents, ServerCapabilities, ServerInfo,
    },
    schemars::JsonSchema,
    service::RequestContext,
    tool, tool_router,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;

/// Run Model Context Protocol (MCP) server
///
/// This command starts an MCP server that exposes mise functionality
/// to AI assistants over stdin/stdout using JSON-RPC protocol.
///
/// The MCP server provides access to:
/// - Installed and available tools
/// - Task definitions and execution
/// - Environment variables
/// - Configuration information
/// - Task execution via the run_task tool
///
/// Resources available:
/// - mise://tools - List all tools (use ?include_inactive=true to include inactive tools)
/// - mise://tasks - List all tasks with their configurations
/// - mise://env - List all environment variables
/// - mise://config - Show configuration files and project root
///
/// Tools available:
/// - list_commands - Every mise command, with its declared effect on the world
/// - install_tool - Install a tool with an optional version (not yet implemented)
/// - run_task - Execute a mise task with optional arguments
///
/// Note: This is primarily intended for integration with AI assistants like Claude,
/// Cursor, or other tools that support the Model Context Protocol.
#[derive(Debug, Parser)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Mcp {}

#[derive(Clone)]
struct MiseServer {
    tool_router: ToolRouter<Self>,
    /// mise's own usage spec, built once. Deriving it walks the whole clap
    /// command tree, which is not work to repeat per request.
    spec: Arc<usage::Spec>,
}

/// Parameters for installing a tool
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
struct InstallToolParams {
    /// Tool name (e.g. "node", "python", "go")
    tool: String,
    /// Optional version to install (e.g. "20", "3.12"). Defaults to latest.
    #[serde(default)]
    version: Option<String>,
}

/// Parameters for listing mise's commands
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
struct ListCommandsParams {
    /// Include commands hidden from help. They are still runnable.
    #[serde(default)]
    include_hidden: bool,
}

/// Parameters for running a mise task
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
struct RunTaskParams {
    /// Name of the task to run
    task: String,
    /// Optional arguments to pass to the task
    #[serde(default)]
    args: Vec<String>,
}

/// Structured data as pretty JSON text, which is what the other tools here
/// return and what clients that only read `content` can use.
fn json_result(value: Value) -> std::result::Result<CallToolResult, ErrorData> {
    let text = serde_json::to_string_pretty(&value).map_err(|e| ErrorData {
        code: ErrorCode::INTERNAL_ERROR,
        message: Cow::Owned(format!("Failed to serialize response: {e}")),
        data: None,
    })?;
    Ok(CallToolResult::success(vec![ContentBlock::text(text)]))
}

#[tool_router]
impl MiseServer {
    fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
            spec: Arc::new(crate::cli::usage::spec()),
        }
    }

    /// Every mise command, with what running it does to the world
    #[tool(
        description = "Every mise command, with what running it does: `read` only inspects state, `write` changes it, `destructive` removes something that is work to get back. A command with no effect listed is unclassified — treat it as needing confirmation, not as safe. Call this before running an unfamiliar mise command."
    )]
    async fn list_commands(
        &self,
        Parameters(ListCommandsParams { include_hidden }): Parameters<ListCommandsParams>,
    ) -> std::result::Result<CallToolResult, ErrorData> {
        fn walk(
            cmd: &usage::SpecCommand,
            path: &mut Vec<String>,
            include_hidden: bool,
            out: &mut Vec<Value>,
        ) {
            for (name, sub) in &cmd.subcommands {
                // A hidden command takes its subtree with it: clap does not
                // propagate `hide` to children, so a visible child of a hidden
                // parent is still not a documented path.
                if sub.hide && !include_hidden {
                    continue;
                }
                path.push(name.clone());
                out.push(json!({
                    "command": path.join(" "),
                    "help": sub.help,
                    "effect": sub.effect.map(|e| e.as_str()),
                    "hidden": sub.hide,
                }));
                walk(sub, path, include_hidden, out);
                path.pop();
            }
        }

        let mut commands = vec![];
        walk(&self.spec.cmd, &mut vec![], include_hidden, &mut commands);
        json_result(json!({ "bin": "mise", "commands": commands }))
    }

    /// Install a tool with an optional version
    #[tool(description = "Install a tool with an optional version (e.g. node@20, python@3.12)")]
    async fn install_tool(
        &self,
        Parameters(_params): Parameters<InstallToolParams>,
    ) -> std::result::Result<CallToolResult, ErrorData> {
        Ok(CallToolResult::error(vec![ContentBlock::text(
            "Tool installation not yet implemented",
        )]))
    }

    /// Execute a mise task with optional arguments
    #[tool(description = "Execute a mise task with optional arguments")]
    async fn run_task(
        &self,
        Parameters(RunTaskParams { task, args }): Parameters<RunTaskParams>,
    ) -> std::result::Result<CallToolResult, ErrorData> {
        let exe = std::env::current_exe().map_err(|e| ErrorData {
            code: ErrorCode::INTERNAL_ERROR,
            message: Cow::Owned(format!("Failed to get current exe: {e}")),
            data: None,
        })?;

        let mut cmd_args = vec!["run".to_string(), task.clone()];
        if !args.is_empty() {
            cmd_args.push("--".to_string());
            cmd_args.extend(args);
        }

        let mut command = tokio::process::Command::new(exe);
        command
            .args(&cmd_args)
            .env("NO_COLOR", "1")
            .env("MISE_YES", "1")
            .kill_on_drop(true)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        prepare_noninteractive_child(command.as_std_mut());
        let child = command.spawn().map_err(|e| ErrorData {
            code: ErrorCode::INTERNAL_ERROR,
            message: Cow::Owned(format!("Failed to spawn mise run: {e}")),
            data: None,
        })?;
        let _running_pid = RunningPidGuard::new(child.id());

        let output = match crate::config::Settings::get().task_timeout_duration() {
            Some(timeout) => tokio::time::timeout(timeout, child.wait_with_output())
                .await
                .map_err(|_| ErrorData {
                    code: ErrorCode::INTERNAL_ERROR,
                    message: Cow::Owned(format!("Task '{task}' timed out after {timeout:?}")),
                    data: None,
                })?,
            None => child.wait_with_output().await,
        }
        .map_err(|e| ErrorData {
            code: ErrorCode::INTERNAL_ERROR,
            message: Cow::Owned(format!("Failed to execute mise run: {e}")),
            data: None,
        })?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if output.status.success() {
            let text = match (stderr.is_empty(), stdout.is_empty()) {
                (true, true) => format!("Task '{task}' completed successfully"),
                (true, false) => stdout.into_owned(),
                (false, true) => stderr.into_owned(),
                (false, false) => format!("{stderr}\n{stdout}"),
            };
            Ok(CallToolResult::success(vec![ContentBlock::text(text)]))
        } else {
            let text = match (stderr.is_empty(), stdout.is_empty()) {
                (true, true) => format!("Task '{task}' failed with no output"),
                (false, true) => stderr.into_owned(),
                (true, false) => stdout.into_owned(),
                (false, false) => format!("{stderr}\n{stdout}"),
            };
            Ok(CallToolResult::error(vec![ContentBlock::text(format!(
                "Task '{task}' failed with exit code {}:\n{text}",
                output.status.code().unwrap_or(1),
            ))]))
        }
    }
}

impl ServerHandler for MiseServer {
    fn get_info(&self) -> ServerInfo {
        let capabilities = ServerCapabilities::builder()
            .enable_resources()
            .enable_tools()
            .build();
        ServerInfo::new(capabilities)
            .with_protocol_version(ProtocolVersion::V_2025_03_26)
            .with_server_info(Implementation::new("mise", env!("CARGO_PKG_VERSION")))
            .with_instructions(
                "Mise MCP server provides access to tools, tasks, environment variables, and \
                 configuration. Call list_commands before running an unfamiliar mise command: \
                 every command declares its effect on the world (`read`, `write`, \
                 `destructive`), and a command with no effect listed is unclassified rather \
                 than safe.",
            )
    }

    async fn list_resources(
        &self,
        _pagination: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> std::result::Result<ListResourcesResult, ErrorData> {
        let resources = vec![
            Resource::new("mise://tools", "Installed Tools"),
            Resource::new("mise://tasks", "Available Tasks"),
            Resource::new("mise://env", "Environment Variables"),
            Resource::new("mise://config", "Configuration"),
        ];

        Ok(ListResourcesResult::with_all_items(resources))
    }

    async fn read_resource(
        &self,
        params: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> std::result::Result<ReadResourceResponse, ErrorData> {
        // Parse URI to extract query parameters
        // Example: mise://tools?include_inactive=true
        let url = url::Url::parse(&params.uri).map_err(|e| ErrorData {
            code: ErrorCode::INVALID_REQUEST,
            message: Cow::Owned(format!("Invalid URI: {e}")),
            data: None,
        })?;

        // Parse query parameters
        // include_inactive=true will show all installed tools, not just active ones
        let include_inactive = url
            .query_pairs()
            .any(|(key, value)| key == "include_inactive" && value == "true");

        match (url.scheme(), url.host_str()) {
            ("mise", Some("tools")) => {
                // Return tool information
                // By default only shows active tools (those in current .mise.toml)
                // With ?include_inactive=true, shows all installed tools
                let config = Config::get().await.map_err(|e| ErrorData {
                    code: ErrorCode::INTERNAL_ERROR,
                    message: Cow::Owned(format!("Failed to load config: {e}")),
                    data: None,
                })?;

                // Get tool request set and resolve toolset
                let trs = config
                    .get_tool_request_set()
                    .await
                    .map_err(|e| ErrorData {
                        code: ErrorCode::INTERNAL_ERROR,
                        message: Cow::Owned(format!("Failed to get tool request set: {e}")),
                        data: None,
                    })?
                    .clone();

                let mut ts = crate::toolset::Toolset::from(trs);
                ts.resolve(&config).await.map_err(|e| ErrorData {
                    code: ErrorCode::INTERNAL_ERROR,
                    message: Cow::Owned(format!("Failed to resolve toolset: {e}")),
                    data: None,
                })?;

                // Get current versions to determine which are active
                let current_versions = ts.list_current_versions();
                let active_versions: std::collections::HashSet<String> = current_versions
                    .iter()
                    .map(|(backend, tv)| format!("{}@{}", backend.id(), tv.version))
                    .collect();

                // Determine which versions to include
                let versions = if include_inactive {
                    // Include all versions (active + installed)
                    ts.list_all_versions(&config).await.map_err(|e| ErrorData {
                        code: ErrorCode::INTERNAL_ERROR,
                        message: Cow::Owned(format!("Failed to list tool versions: {e}")),
                        data: None,
                    })?
                } else {
                    // Only include active versions (current)
                    current_versions
                };

                // Group by tool and create JSON output
                // Output format: { "node": [{"version": "20.11.0", "active": true, ...}], ... }
                let mut tools_map: std::collections::HashMap<String, Vec<Value>> =
                    std::collections::HashMap::new();

                for (backend, tv) in versions {
                    let tool_name = backend.id().to_string();
                    let install_path = tv.install_path();
                    let installed = install_path.exists();
                    let version_key = format!("{}@{}", backend.id(), tv.version);
                    let version_info = json!({
                        "version": tv.version.clone(),
                        "requested_version": tv.request.version(),
                        "install_path": install_path.to_string_lossy(),
                        "installed": installed,
                        "active": active_versions.contains(&version_key),
                        "source": tv.request.source().as_json(),
                    });
                    tools_map.entry(tool_name).or_default().push(version_info);
                }

                let text = serde_json::to_string_pretty(&tools_map).unwrap();
                let contents = vec![ResourceContents::TextResourceContents {
                    uri: params.uri.clone(),
                    mime_type: Some("application/json".to_string()),
                    text,
                    meta: None,
                }];

                Ok(ReadResourceResult::new(contents).into())
            }
            ("mise", Some("tasks")) => {
                let config = Config::get().await.map_err(|e| ErrorData {
                    code: ErrorCode::INTERNAL_ERROR,
                    message: Cow::Owned(format!("Failed to load config: {e}")),
                    data: None,
                })?;

                let remote_no_cache = Settings::get().task.remote_no_cache.unwrap_or(false);
                let _artifacts =
                    remote_no_cache.then(crate::task::task_fetcher::RemoteTaskArtifactsGuard::new);
                let task_config = if remote_no_cache {
                    config.with_config_files(config.config_files.clone())
                } else {
                    config
                };
                let tasks = task_config
                    .tasks_with_context_no_cache(None, remote_no_cache)
                    .await
                    .map_err(|e| ErrorData {
                        code: ErrorCode::INTERNAL_ERROR,
                        message: Cow::Owned(format!("Failed to load tasks: {e}")),
                        data: None,
                    })?;

                let task_list: Vec<_> = tasks.iter().map(|(name, task)| {
                    json!({
                        "name": name,
                        "description": task.description.clone(),
                        "aliases": task.aliases,
                        "source": task.config_source.to_string_lossy(),
                        "config_sources": task
                            .config_sources()
                            .iter()
                            .map(|source| source.to_string_lossy())
                            .collect::<Vec<_>>(),
                        "depends": task.depends.iter().map(|d| d.task.clone()).collect::<Vec<_>>(),
                        "depends_post": task.depends_post.iter().map(|d| d.task.clone()).collect::<Vec<_>>(),
                        "wait_for": task.wait_for.iter().map(|d| d.task.clone()).collect::<Vec<_>>(),
                        "env": json!({}), // EnvList is not directly iterable, keeping empty for now
                        "dir": task.dir.clone(),
                        "hide": task.hide,
                        "raw": task.raw,
                        "interactive": task.interactive,
                        "sources": task.sources.clone(),
                        "outputs": task.outputs.clone(),
                        "shell": task.shell.clone(),
                        "quiet": task.quiet,
                        "silent": task.silent,
                        "tools": task.tools.clone(),
                        "run": task.run(),
                        "usage": task.usage.clone(),
                    })
                }).collect();

                let text = serde_json::to_string_pretty(&task_list).unwrap();
                let contents = vec![ResourceContents::TextResourceContents {
                    uri: params.uri.clone(),
                    mime_type: Some("application/json".to_string()),
                    text,
                    meta: None,
                }];

                Ok(ReadResourceResult::new(contents).into())
            }
            ("mise", Some("env")) => {
                let config = Config::get().await.map_err(|e| ErrorData {
                    code: ErrorCode::INTERNAL_ERROR,
                    message: Cow::Owned(format!("Failed to load config: {e}")),
                    data: None,
                })?;

                let env_template = config.env().await.map_err(|e| ErrorData {
                    code: ErrorCode::INTERNAL_ERROR,
                    message: Cow::Owned(format!("Failed to load env: {e}")),
                    data: None,
                })?;

                let mut env_map = HashMap::new();
                for (k, v) in env_template.iter() {
                    env_map.insert(k.clone(), v.clone());
                }

                let text = serde_json::to_string_pretty(&env_map).unwrap();
                let contents = vec![ResourceContents::TextResourceContents {
                    uri: params.uri.clone(),
                    mime_type: Some("application/json".to_string()),
                    text,
                    meta: None,
                }];

                Ok(ReadResourceResult::new(contents).into())
            }
            ("mise", Some("config")) => {
                let config = Config::get().await.map_err(|e| ErrorData {
                    code: ErrorCode::INTERNAL_ERROR,
                    message: Cow::Owned(format!("Failed to load config: {e}")),
                    data: None,
                })?;

                let config_info = json!({
                    "config_files": config.config_files.keys().collect::<Vec<_>>(),
                    "project_root": config.project_root.as_ref().map(|p| p.to_string_lossy()),
                });

                let text = serde_json::to_string_pretty(&config_info).unwrap();
                let contents = vec![ResourceContents::TextResourceContents {
                    uri: params.uri.clone(),
                    mime_type: Some("application/json".to_string()),
                    text,
                    meta: None,
                }];

                Ok(ReadResourceResult::new(contents).into())
            }
            _ => Err(ErrorData {
                code: ErrorCode::RESOURCE_NOT_FOUND,
                message: Cow::Owned(format!("Unknown resource URI: {}", params.uri)),
                data: None,
            }),
        }
    }

    async fn list_tools(
        &self,
        _pagination: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> std::result::Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult::with_all_items(self.tool_router.list_all()))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> std::result::Result<CallToolResponse, ErrorData> {
        let tool_call_context =
            rmcp::handler::server::tool::ToolCallContext::new(self, request, context);
        self.tool_router.call(tool_call_context).await
    }
}

impl Mcp {
    pub async fn run(self) -> Result<()> {
        eprintln!("Starting mise MCP server...");

        let server = MiseServer::new();

        // Create stdio transport and serve
        let service = server
            .serve(rmcp::transport::stdio())
            .await
            .map_err(|e| eyre::eyre!("Failed to create service: {}", e))?;

        // Wait for the service to complete
        service
            .waiting()
            .await
            .map_err(|e| eyre::eyre!("Service error: {}", e))?;

        Ok(())
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    # Start the MCP server (typically used by AI assistant tools)
    $ <bold>mise mcp</bold>

    # Example integration with Claude Desktop (add to claude_desktop_config.json):
    {
      "mcpServers": {
        "mise": {
          "command": "mise",
          "args": ["mcp"],
          "env": {}
        }
      }
    }

    # Interactive testing with JSON-RPC commands:
    $ <bold>echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"1.0"}}}' | mise mcp</bold>

    # Resources you can query:
    - <bold>mise://tools</bold> - List active tools
    - <bold>mise://tools?include_inactive=true</bold> - List all installed tools
    - <bold>mise://tasks</bold> - List all tasks
    - <bold>mise://env</bold> - List environment variables
    - <bold>mise://config</bold> - Show configuration info

    # Tools available:
    - <bold>list_commands</bold> - Every mise command and what running it does
      Example: {"include_hidden": false}
    - <bold>install_tool</bold> - Install a tool (not yet implemented)
    - <bold>run_task</bold> - Execute a mise task with optional arguments
      Example: {"task": "build", "args": ["--verbose"]}
"#
);

#[cfg(test)]
mod tests {
    use super::*;

    /// The text of every command row `list_commands` returns.
    async fn commands(include_hidden: bool) -> Vec<Value> {
        let res = MiseServer::new()
            .list_commands(Parameters(ListCommandsParams { include_hidden }))
            .await
            .unwrap();
        let ContentBlock::Text(text) = &res.content[0] else {
            panic!("expected text content");
        };
        serde_json::from_str::<Value>(&text.text).unwrap()["commands"]
            .as_array()
            .unwrap()
            .clone()
    }

    fn find<'a>(commands: &'a [Value], path: &str) -> &'a Value {
        commands
            .iter()
            .find(|c| c["command"] == path)
            .unwrap_or_else(|| panic!("no command {path:?}"))
    }

    #[tokio::test]
    async fn list_commands_carries_the_declared_effects() {
        // The point of the tool. If these come back null the classification in
        // command_effects is not reaching the agent that needs it.
        let commands = commands(false).await;
        assert_eq!(find(&commands, "ls")["effect"], "read");
        assert_eq!(find(&commands, "install")["effect"], "write");
        assert_eq!(find(&commands, "prune")["effect"], "destructive");
    }

    #[tokio::test]
    async fn list_commands_reaches_nested_paths() {
        let commands = commands(false).await;
        assert_eq!(find(&commands, "tasks ls")["effect"], "read");
        assert!(find(&commands, "tasks ls")["help"].is_string());
    }

    #[tokio::test]
    async fn hidden_subtrees_are_excluded_by_default() {
        // clap does not propagate `hide`, so `bootstrap launchd` is hidden
        // while its children are not; the children must not leak either.
        let shown = commands(false).await;
        assert!(!shown.iter().any(|c| c["command"] == "bootstrap launchd"));
        assert!(
            !shown
                .iter()
                .any(|c| c["command"] == "bootstrap launchd apply")
        );

        let all = commands(true).await;
        assert!(all.iter().any(|c| c["command"] == "bootstrap launchd"));
        assert!(
            all.iter()
                .any(|c| c["command"] == "bootstrap launchd apply")
        );
    }

    #[tokio::test]
    async fn nothing_is_unclassified_by_accident() {
        // command_effects tests its own table, but nothing there proves the
        // classification survives into what an agent is actually served. A
        // missing effect reads as "unknown, ask" — fine for `mise run`, whose
        // effect really is whatever the task does, and a silent gap for
        // anything else.
        let deliberate: std::collections::HashSet<_> = crate::cli::command_effects::UNCLASSIFIED
            .iter()
            .map(|(cmd, _)| *cmd)
            .collect();
        let accidental: Vec<_> = commands(false)
            .await
            .iter()
            .filter(|c| c["effect"].is_null())
            .map(|c| c["command"].as_str().unwrap().to_string())
            .filter(|cmd| !deliberate.contains(cmd.as_str()))
            .collect();
        assert!(
            accidental.is_empty(),
            "unclassified with no entry in command_effects::UNCLASSIFIED: {accidental:?}"
        );
    }

    #[test]
    fn server_info_identifies_mise() {
        // rmcp defaults server_info to its own crate name/version, which would
        // make every MCP client see the server as "rmcp".
        let info = MiseServer::new().get_info();
        assert_eq!(info.server_info.name, "mise");
        assert_eq!(info.server_info.version, env!("CARGO_PKG_VERSION"));
    }
}
