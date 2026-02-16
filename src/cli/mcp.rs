use crate::Result;
use crate::config::Config;
use clap::Parser;
use rmcp::{
    RoleServer, ServiceExt,
    handler::server::{
        ServerHandler,
        tool::{Parameters, ToolRouter},
    },
    model::{
        AnnotateAble, CallToolRequestParam, CallToolResult, Content, ErrorCode, ErrorData,
        Implementation, ListResourcesResult, ListToolsResult, PaginatedRequestParam,
        ProtocolVersion, RawResource, ReadResourceRequestParam, ReadResourceResult,
        ResourceContents, ServerCapabilities, ServerInfo,
    },
    schemars::JsonSchema,
    service::RequestContext,
    tool, tool_router,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::borrow::Cow;
use std::collections::HashMap;

/// [experimental] Run Model Context Protocol (MCP) server
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

#[tool_router]
impl MiseServer {
    fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }

    /// Install a tool with an optional version
    #[tool(description = "Install a tool with an optional version (e.g. node@20, python@3.12)")]
    async fn install_tool(
        &self,
        Parameters(_params): Parameters<InstallToolParams>,
    ) -> std::result::Result<CallToolResult, ErrorData> {
        Ok(CallToolResult::error(vec![Content::text(
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

        let child = tokio::process::Command::new(exe)
            .args(&cmd_args)
            .env("NO_COLOR", "1")
            .env("MISE_YES", "1")
            .kill_on_drop(true)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| ErrorData {
                code: ErrorCode::INTERNAL_ERROR,
                message: Cow::Owned(format!("Failed to spawn mise run: {e}")),
                data: None,
            })?;

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
            Ok(CallToolResult::success(vec![Content::text(text)]))
        } else {
            let text = match (stderr.is_empty(), stdout.is_empty()) {
                (true, true) => format!("Task '{task}' failed with no output"),
                (false, true) => stderr.into_owned(),
                (true, false) => stdout.into_owned(),
                (false, false) => format!("{stderr}\n{stdout}"),
            };
            Ok(CallToolResult::error(vec![Content::text(format!(
                "Task '{task}' failed with exit code {}:\n{text}",
                output.status.code().unwrap_or(1),
            ))]))
        }
    }
}

impl ServerHandler for MiseServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2025_03_26,
            capabilities: ServerCapabilities::builder()
                .enable_resources()
                .enable_tools()
                .build(),
            server_info: Implementation::from_build_env(),
            instructions: Some("Mise MCP server provides access to tools, tasks, environment variables, and configuration".to_string()),
        }
    }

    async fn list_resources(
        &self,
        _pagination: Option<PaginatedRequestParam>,
        _context: RequestContext<RoleServer>,
    ) -> std::result::Result<ListResourcesResult, ErrorData> {
        let resources = vec![
            RawResource::new("mise://tools", "Installed Tools".to_string()).no_annotation(),
            RawResource::new("mise://tasks", "Available Tasks".to_string()).no_annotation(),
            RawResource::new("mise://env", "Environment Variables".to_string()).no_annotation(),
            RawResource::new("mise://config", "Configuration".to_string()).no_annotation(),
        ];

        Ok(ListResourcesResult {
            resources,
            next_cursor: None,
        })
    }

    async fn read_resource(
        &self,
        params: ReadResourceRequestParam,
        _context: RequestContext<RoleServer>,
    ) -> std::result::Result<ReadResourceResult, ErrorData> {
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
                }];

                Ok(ReadResourceResult { contents })
            }
            ("mise", Some("tasks")) => {
                let config = Config::get().await.map_err(|e| ErrorData {
                    code: ErrorCode::INTERNAL_ERROR,
                    message: Cow::Owned(format!("Failed to load config: {e}")),
                    data: None,
                })?;

                let tasks = config.tasks().await.map_err(|e| ErrorData {
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
                        "depends": task.depends.iter().map(|d| d.task.clone()).collect::<Vec<_>>(),
                        "depends_post": task.depends_post.iter().map(|d| d.task.clone()).collect::<Vec<_>>(),
                        "wait_for": task.wait_for.iter().map(|d| d.task.clone()).collect::<Vec<_>>(),
                        "env": json!({}), // EnvList is not directly iterable, keeping empty for now
                        "dir": task.dir.clone(),
                        "hide": task.hide,
                        "raw": task.raw,
                        "sources": task.sources.clone(),
                        "outputs": task.outputs.clone(),
                        "shell": task.shell.clone(),
                        "quiet": task.quiet,
                        "silent": task.silent,
                        "tools": task.tools.clone(),
                        "run": task.run_script_strings(),
                        "usage": task.usage.clone(),
                    })
                }).collect();

                let text = serde_json::to_string_pretty(&task_list).unwrap();
                let contents = vec![ResourceContents::TextResourceContents {
                    uri: params.uri.clone(),
                    mime_type: Some("application/json".to_string()),
                    text,
                }];

                Ok(ReadResourceResult { contents })
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
                }];

                Ok(ReadResourceResult { contents })
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
                }];

                Ok(ReadResourceResult { contents })
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
        _pagination: Option<PaginatedRequestParam>,
        _context: RequestContext<RoleServer>,
    ) -> std::result::Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult {
            tools: self.tool_router.list_all(),
            next_cursor: None,
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParam,
        context: RequestContext<RoleServer>,
    ) -> std::result::Result<CallToolResult, ErrorData> {
        let tool_call_context =
            rmcp::handler::server::tool::ToolCallContext::new(self, request, context);
        self.tool_router.call(tool_call_context).await
    }
}

impl Mcp {
    pub async fn run(self) -> Result<()> {
        let settings = crate::config::Settings::get();
        settings.ensure_experimental("mcp")?;

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
          "env": {
            "MISE_EXPERIMENTAL": "1"
          }
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
    - <bold>install_tool</bold> - Install a tool (not yet implemented)
    - <bold>run_task</bold> - Execute a mise task with optional arguments
      Example: {"task": "build", "args": ["--verbose"]}
"#
);
