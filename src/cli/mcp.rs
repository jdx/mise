use crate::Result;
use crate::config::Config;
use clap::Parser;
use rmcp::{
    RoleServer, ServiceExt,
    handler::server::ServerHandler,
    model::{
        AnnotateAble, CallToolRequestParam, CallToolResult, Content, ErrorCode, ErrorData,
        ListResourcesResult, ListToolsResult, PaginatedRequestParam, RawResource,
        ReadResourceRequestParam, ReadResourceResult, ResourceContents,
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
#[derive(Debug, Parser)]
#[clap(verbatim_doc_comment)]
pub struct Mcp {}

#[derive(Clone)]
struct MiseServer {}

impl MiseServer {
    fn new() -> Self {
        Self {}
    }
}

impl ServerHandler for MiseServer {
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
        match params.uri.as_str() {
            "mise://tools" => {
                let _config = Config::get().await.map_err(|e| ErrorData {
                    code: ErrorCode(500),
                    message: Cow::Owned(format!("Failed to load config: {e}")),
                    data: None,
                })?;

                let tool_list: Vec<Value> = vec![];
                let text = serde_json::to_string_pretty(&tool_list).unwrap();
                let contents = vec![ResourceContents::text(text, params.uri.clone())];

                Ok(ReadResourceResult { contents })
            }
            "mise://tasks" => {
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
                        "depends": task.depends.iter().map(|d| d.task.clone()).collect::<Vec<_>>(),
                    })
                }).collect();

                let text = serde_json::to_string_pretty(&task_list).unwrap();
                let contents = vec![ResourceContents::text(text, params.uri.clone())];

                Ok(ReadResourceResult { contents })
            }
            "mise://env" => {
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
                let contents = vec![ResourceContents::text(text, params.uri.clone())];

                Ok(ReadResourceResult { contents })
            }
            "mise://config" => {
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
                let contents = vec![ResourceContents::text(text, params.uri.clone())];

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
