use crate::Result;
use crate::config::Config;
use clap::Parser;
use rmcp::{
    RoleServer, ServiceExt,
    handler::server::ServerHandler,
    model::{
        AnnotateAble, CallToolRequestParam, CallToolResult, Content, ErrorCode, ErrorData,
        Implementation, ListResourcesResult, ListToolsResult, PaginatedRequestParam,
        ProtocolVersion, RawResource, ReadResourceRequestParam, ReadResourceResult,
        ResourceContents, ServerCapabilities, ServerInfo,
    },
    service::RequestContext,
};
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
///
/// Resources available:
/// - mise://tools - List all tools (use ?include_inactive=true to include inactive tools)
/// - mise://tasks - List all tasks with their configurations
/// - mise://env - List all environment variables
/// - mise://config - Show configuration files and project root
///
/// Note: This is primarily intended for integration with AI assistants like Claude,
/// Cursor, or other tools that support the Model Context Protocol.
#[derive(Debug, Parser)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Mcp {}

#[derive(Clone)]
struct MiseServer {}

impl MiseServer {
    fn new() -> Self {
        Self {}
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
            code: ErrorCode(400),
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
                    code: ErrorCode(500),
                    message: Cow::Owned(format!("Failed to load config: {e}")),
                    data: None,
                })?;

                // Get tool request set and resolve toolset
                let trs = config
                    .get_tool_request_set()
                    .await
                    .map_err(|e| ErrorData {
                        code: ErrorCode(500),
                        message: Cow::Owned(format!("Failed to get tool request set: {e}")),
                        data: None,
                    })?
                    .clone();

                let mut ts = crate::toolset::Toolset::from(trs);
                ts.resolve(&config).await.map_err(|e| ErrorData {
                    code: ErrorCode(500),
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
                        code: ErrorCode(500),
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
                    code: ErrorCode(500),
                    message: Cow::Owned(format!("Failed to load config: {e}")),
                    data: None,
                })?;

                let tasks = config.tasks().await.map_err(|e| ErrorData {
                    code: ErrorCode(500),
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
                        "run": task.run.clone(),
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
                    code: ErrorCode(500),
                    message: Cow::Owned(format!("Failed to load config: {e}")),
                    data: None,
                })?;

                let env_template = config.env().await.map_err(|e| ErrorData {
                    code: ErrorCode(500),
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
                    code: ErrorCode(500),
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
                code: ErrorCode(404),
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
        // For now, return empty tools list
        Ok(ListToolsResult {
            tools: vec![],
            next_cursor: None,
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParam,
        _context: RequestContext<RoleServer>,
    ) -> std::result::Result<CallToolResult, ErrorData> {
        match &request.name[..] {
            "install_tool" => Ok(CallToolResult::success(vec![Content::text(
                "Tool installation not yet implemented".to_string(),
            )])),
            "run_task" => Ok(CallToolResult::success(vec![Content::text(
                "Task execution not yet implemented".to_string(),
            )])),
            _ => Err(ErrorData {
                code: ErrorCode(404),
                message: Cow::Owned(format!("Unknown tool: {}", request.name)),
                data: None,
            }),
        }
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
          "args": ["mcp"]
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
"#
);
