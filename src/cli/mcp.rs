use crate::Result;
use crate::cli::run::render_execution_plan_explain;
use crate::config::{Config, Settings};
use crate::task::task_descriptor::{TaskDescriptorOptions, task_descriptor_json};
use crate::task::task_execution_plan::ExecutionPlan;
use crate::task::task_plan_analysis::{ChangeImpact, ContentionAnalysis, cycle_path_label};
use crate::task::task_plan_bundle::{
    PlanBuildRequest, build_execution_plan_bundle, join_task_specs_for_cli,
};
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
/// - mise://plan - Build static execution plan (?tasks=build,test&changed=src/main.ts)
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
            RawResource::new("mise://plan", "Static Execution Plan".to_string()).no_annotation(),
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

                let task_list: Vec<_> = tasks
                    .iter()
                    .map(|(name, task)| {
                        let options = TaskDescriptorOptions {
                            include_usage: true,
                            use_run_script_strings: true,
                            ..Default::default()
                        };
                        let mut descriptor = task_descriptor_json(task, &options);
                        if let Value::Object(ref mut map) = descriptor {
                            map.insert("name".to_string(), json!(name));
                        }
                        descriptor
                    })
                    .collect();

                let text = serde_json::to_string_pretty(&task_list).unwrap();
                let contents = vec![ResourceContents::TextResourceContents {
                    uri: params.uri.clone(),
                    mime_type: Some("application/json".to_string()),
                    text,
                }];

                Ok(ReadResourceResult { contents })
            }
            ("mise", Some("plan")) => {
                let config = Config::get().await.map_err(|e| ErrorData {
                    code: ErrorCode::INTERNAL_ERROR,
                    message: Cow::Owned(format!("Failed to load config: {e}")),
                    data: None,
                })?;

                let task_specs = parse_list_query(&url, &["tasks", "task"]);
                let changed = parse_list_query(&url, &["changed"]);
                let format = parse_plan_output_format(&url);

                let args = if task_specs.is_empty() {
                    vec![]
                } else {
                    join_task_specs_for_cli(&task_specs)
                };

                let jobs = Settings::get().jobs;
                let bundle = build_execution_plan_bundle(
                    &config,
                    PlanBuildRequest {
                        requested_task_specs: task_specs.clone(),
                        cli_args: args.clone(),
                        changed_files: changed.clone(),
                        jobs,
                        task_list_with_context: false,
                        task_list_skip_deps: false,
                        deps_skip_deps: false,
                        fetch_remote: true,
                        no_cache: false,
                        ..Default::default()
                    },
                )
                .await
                .map_err(|e| ErrorData {
                    code: ErrorCode::INTERNAL_ERROR,
                    message: Cow::Owned(format!("Failed to build execution plan bundle: {e}")),
                    data: None,
                })?;
                if let Some(cycle) = &bundle.cycle {
                    return Err(ErrorData {
                        code: ErrorCode::INTERNAL_ERROR,
                        message: Cow::Owned(format!(
                            "Failed to build execution plan: circular dependency detected in static DAG: {}",
                            cycle_path_label(cycle)
                        )),
                        data: None,
                    });
                }
                let plan = bundle.plan.ok_or_else(|| ErrorData {
                    code: ErrorCode::INTERNAL_ERROR,
                    message: Cow::Owned("Failed to build execution plan".to_string()),
                    data: None,
                })?;
                let config_files = bundle.config_files.clone();
                let plan_hash = bundle
                    .plan_hash
                    .clone()
                    .unwrap_or_else(|| "unknown".to_string());
                let change_impact = bundle.change_impact.clone();
                let contention = bundle.contention.clone().unwrap_or_default();

                let plan_payload = json!({
                    "requested_task_specs": bundle.requested_task_specs,
                    "resolved_cli_args": bundle.resolved_cli_args,
                    "changed_files": changed,
                    "jobs": jobs,
                    "plan_hash": plan_hash,
                    "config_files": config_files,
                    "plan": plan,
                    "change_impact": change_impact,
                    "contention": contention,
                });

                let (text, mime_type) = match format {
                    PlanOutputFormat::Json => (
                        serde_json::to_string_pretty(&plan_payload).unwrap(),
                        "application/json".to_string(),
                    ),
                    PlanOutputFormat::Explain => (
                        render_plan_explain(
                            &task_specs,
                            &args,
                            jobs,
                            &plan_hash,
                            &config_files,
                            &plan,
                            &change_impact,
                            &contention,
                        ),
                        "text/plain".to_string(),
                    ),
                };
                let contents = vec![ResourceContents::TextResourceContents {
                    uri: params.uri.clone(),
                    mime_type: Some(mime_type),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlanOutputFormat {
    Json,
    Explain,
}

fn parse_list_query(url: &url::Url, keys: &[&str]) -> Vec<String> {
    let mut values = Vec::new();
    for (key, value) in url.query_pairs() {
        if !keys.iter().any(|k| *k == key) {
            continue;
        }
        values.extend(
            value
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(ToString::to_string),
        );
    }
    values
}

fn parse_plan_output_format(url: &url::Url) -> PlanOutputFormat {
    let value = url
        .query_pairs()
        .find(|(k, _)| k == "format")
        .map(|(_, v)| v.to_ascii_lowercase())
        .unwrap_or_else(|| "json".to_string());
    match value.as_str() {
        "explain" => PlanOutputFormat::Explain,
        _ => PlanOutputFormat::Json,
    }
}

#[allow(clippy::too_many_arguments)]
fn render_plan_explain(
    requested_task_specs: &[String],
    resolved_cli_args: &[String],
    jobs: usize,
    plan_hash: &str,
    config_files: &[String],
    plan: &ExecutionPlan,
    change_impact: &ChangeImpact,
    contention: &ContentionAnalysis,
) -> String {
    let mut lines = vec![
        "Execution plan (MCP explain)".to_string(),
        format!(
            "Requested task specs: {}",
            if requested_task_specs.is_empty() {
                "<default>".to_string()
            } else {
                requested_task_specs.join(", ")
            }
        ),
        format!(
            "Resolved CLI args: {}",
            if resolved_cli_args.is_empty() {
                "<none>".to_string()
            } else {
                resolved_cli_args.join(" ")
            }
        ),
        format!("Jobs: {jobs}"),
        format!("Plan hash: {plan_hash}"),
    ];

    lines.push(String::new());
    if !config_files.is_empty() {
        lines.push(format!("Config files ({})", config_files.len()));
        for cf in config_files {
            lines.push(format!("  - {cf}"));
        }
    } else {
        lines.push("Config files (0)".to_string());
        lines.push("  (none)".to_string());
    }

    lines.push(String::new());
    lines.push("Detailed explain (shared run renderer):".to_string());
    match render_execution_plan_explain(plan, change_impact, contention) {
        Ok(explain) => lines.push(explain),
        Err(err) => lines.push(format!("failed to render plan explain output: {err}")),
    }
    lines.join("\n")
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;
    use crate::task::task_execution_plan::{ExecutionStage, PlannedTask, TaskDeclarationRef};
    use crate::task::task_identity::TaskIdentity;

    #[test]
    fn test_parse_plan_output_format_defaults_to_json() {
        let url = url::Url::parse("mise://plan").unwrap();
        assert_eq!(parse_plan_output_format(&url), PlanOutputFormat::Json);
    }

    #[test]
    fn test_parse_plan_output_format_explain() {
        let url = url::Url::parse("mise://plan?format=explain").unwrap();
        assert_eq!(parse_plan_output_format(&url), PlanOutputFormat::Explain);
    }

    #[test]
    fn test_render_plan_explain_uses_shared_run_renderer() {
        let plan = ExecutionPlan {
            stages: vec![ExecutionStage::parallel(vec![PlannedTask {
                identity: TaskIdentity {
                    name: "build".to_string(),
                    args: vec![],
                    env: vec![],
                },
                runtime: true,
                interactive: false,
                declaration: TaskDeclarationRef {
                    source: "/tmp/mise.toml".to_string(),
                    line: Some(3),
                },
            }])],
        };

        let explain = render_plan_explain(
            &["build".to_string()],
            &["build".to_string()],
            4,
            "sha256:test",
            &["/tmp/mise.toml".to_string()],
            &plan,
            &ChangeImpact::default(),
            &ContentionAnalysis::default(),
        );

        assert!(explain.contains("Execution plan (MCP explain)"));
        assert!(explain.contains("Plan hash: sha256:test"));
        assert!(explain.contains("Detailed explain (shared run renderer):"));
        assert!(explain.contains("Plan:"));
        assert!(explain.contains("Stage 1"));
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
