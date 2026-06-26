//! Wire types for the `system.mcp.*` and `system.mcp.federation.*` tools.
//!
//! `serde_json::Value` appears here only inside [`mcp_fed`] for MCP
//! protocol-level opaque blobs (input_schema, args, resource,
//! structured_content) whose shapes are defined by upstream MCP servers,
//! not by orca.
#![allow(clippy::disallowed_types)]

use plugin_toolkit::prelude::*;
use std::collections::HashMap;

// ── Registry CRUD types ─────────────────────────────────────────────────────

#[plugin_struct]
pub struct McpServerEntry {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
    pub enabled: bool,
}

#[plugin_struct]
pub struct MappingEntry {
    pub orca_tool: String,
    pub mcp_name: String,
    pub external_tool: String,
    pub match_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f64>,
    pub enabled: bool,
}

#[plugin_struct]
pub struct SyncToolsServerEntry {
    pub server: String,
    pub added: u32,
    pub skipped: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[plugin_struct(args)]
pub struct ListMcpServersArgs {}

#[plugin_struct]
pub struct ListMcpServersOutput {
    pub servers: Vec<McpServerEntry>,
}

#[plugin_struct(args)]
pub struct AddMcpServerArgs {
    pub name: String,
    pub command: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[arg(skip)]
    pub env: Option<HashMap<String, String>>,
}

#[plugin_struct]
pub struct McpServerMutationResult {
    pub name: String,
    pub changed: bool,
}

#[plugin_struct(args)]
pub struct RemoveMcpServerArgs {
    pub name: String,
}

#[plugin_struct(args)]
pub struct MapToolArgs {
    pub name: String,
    pub orca_tool: String,
    pub external_tool: String,
}

#[plugin_struct]
pub struct MapToolResult {
    pub orca_tool: String,
    pub mcp_name: String,
    pub external_tool: String,
}

#[plugin_struct(args)]
pub struct UnmapToolArgs {
    pub orca_tool: String,
}

#[plugin_struct]
pub struct UnmapToolResult {
    pub orca_tool: String,
    pub changed: bool,
}

#[plugin_struct(args)]
pub struct SyncToolsArgs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub all: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub threshold: Option<f64>,
}

#[plugin_struct]
pub struct SyncToolsOutput {
    pub results: Vec<SyncToolsServerEntry>,
}

#[plugin_struct(args)]
pub struct ListToolMappingsArgs {
    /// Filter by server name (omit for all).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[plugin_struct]
pub struct ListToolMappingsOutput {
    pub mappings: Vec<MappingEntry>,
}

// ── MCP federation types (opaque MCP protocol blobs) ────────────────────────

mod mcp_fed {
    use super::*;
    use plugin_toolkit::json_schema::JsonSchemaNode;
    use plugin_toolkit::serde_json as sj;
    use sj::Value;

    #[plugin_struct]
    #[serde(rename_all = "camelCase")]
    pub struct McpToolEntry {
        pub server: String,
        pub name: String,
        pub description: String,
        pub input_schema: JsonSchemaNode,
    }

    #[plugin_struct(args)]
    pub struct ListMcpToolsArgs {}

    #[plugin_struct]
    pub struct ListMcpToolsOutput {
        pub tools: Vec<McpToolEntry>,
    }

    #[plugin_struct]
    pub struct RunMcpToolArgs {
        pub server: String,
        pub tool: String,
        #[serde(default)]
        pub args: Option<sj::Map<String, Value>>,
    }

    #[plugin_struct]
    #[serde(rename_all = "camelCase")]
    pub struct McpContent {
        #[serde(rename = "type")]
        pub kind: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub text: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub data: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub mime_type: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub resource: Option<Value>,
    }

    #[plugin_struct]
    #[serde(rename_all = "camelCase")]
    pub struct RunMcpToolOutput {
        pub content: Vec<McpContent>,
        pub is_error: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub structured_content: Option<Value>,
    }
}

pub use mcp_fed::{
    ListMcpToolsArgs, ListMcpToolsOutput, McpContent, McpToolEntry, RunMcpToolArgs,
    RunMcpToolOutput,
};
