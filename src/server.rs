// Project: theses-mcp
// File: src/server.rs
// Description: MCP over stdio: lists the six tools and routes calls to the blocking implementations.
// Author: Jan Alexandr Kopřiva
// License: MIT

use crate::Theses;
use crate::dispatch::{Reply, call_blocking, login_params};
use crate::tools::definitions;
use rmcp::{
    ErrorData as McpError, RoleServer, ServerHandler, ServiceExt,
    model::{
        CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
        ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool,
    },
    service::RequestContext,
    transport::stdio,
};
use serde_json::{Map, Value};
use std::sync::Arc;

#[derive(Clone)]
pub struct Server {
    app: Arc<Theses>,
}

impl Server {
    pub fn new(app: Arc<Theses>) -> Self {
        Server { app }
    }
}

fn respond(reply: Reply) -> CallToolResponse {
    let blocks = vec![ContentBlock::text(reply.text)];
    if reply.is_error {
        CallToolResult::error(blocks)
    } else {
        CallToolResult::success(blocks)
    }
    .into()
}

/// Run one tool. Most go to a worker thread; `login` drives a browser and stays async.
pub async fn call(app: &Arc<Theses>, name: &str, args: Map<String, Value>) -> Reply {
    if name == "login" {
        return match login_params(&args) {
            Err(e) => Reply::failed(name, format!("invalid arguments: {e}")),
            Ok(p) => match crate::login::login(app.clone(), p).await {
                Ok(v) => Reply::ok(&v),
                Err(e) => Reply::failed(name, e),
            },
        };
    }
    let (app, name) = (app.clone(), name.to_string());
    tokio::task::spawn_blocking(move || call_blocking(&app, &name, &args))
        .await
        .unwrap_or_else(|e| Reply::failed("tool", e))
}

impl ServerHandler for Server {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("theses", env!("CARGO_PKG_VERSION")))
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        Ok(ListToolsResult {
            tools: tools(),
            ..Default::default()
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        let reply = call(
            &self.app,
            &request.name,
            request.arguments.unwrap_or_default(),
        )
        .await;
        Ok(respond(reply))
    }
}

pub fn tools() -> Vec<Tool> {
    definitions()
        .into_iter()
        .map(|(name, description, schema)| Tool::new(name, description, schema))
        .collect()
}

pub async fn serve() -> Result<(), Box<dyn std::error::Error>> {
    // the blocking HTTP client must be built off the async threads, and it cannot be dropped
    // on them either, so one reference is leaked on purpose: the process is about to exit
    let app = Arc::new(tokio::task::spawn_blocking(Theses::from_env).await?);
    std::mem::forget(app.clone());
    let running = Server::new(app).serve(stdio()).await?;
    running.waiting().await?;
    Ok(())
}
