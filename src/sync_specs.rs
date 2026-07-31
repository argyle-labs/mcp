//! `mcp.sync_specs` — pull OpenAPI specs from a registered MCP server into
//! orca's spec registry.
//!
//! Connects to `server`, calls its `{prefix}_spec_list` / `{prefix}_spec_schema`
//! tools, and upserts every advertised repo into `db::openapi_specs`. This is
//! federation functionality (it drives a registered MCP server via `McpPool`),
//! so it lives with the rest of the MCP client surface rather than in core
//! `spec`. Formerly `spec.sync-mcp`.
//!
//! MCP tool responses are arbitrary upstream JSON — opaque payload escape hatch.
#![allow(clippy::disallowed_types)]

use plugin_toolkit::anyhow::{self, Context, anyhow};
use plugin_toolkit::contract;
use plugin_toolkit::core_tables;
use plugin_toolkit::prelude::*;
use plugin_toolkit::serde_json::{Value, json};

use crate::tools::make_mcp_pool;

#[orca_struct(args)]
pub struct McpSyncSpecsArgs {
    /// Registered MCP server to pull specs from.
    pub server: String,
}

/// Result of an `mcp.sync_specs` run. Local mirror of the former db-crate
/// `openapi_specs_registry::SyncMcpSpecsResult`, which lived behind the heavy
/// `db-incore` path and is gone on the light profile.
#[orca_struct]
pub struct SyncMcpSpecsResult {
    /// The server the specs were pulled from.
    pub server: String,
    /// Number of specs successfully cached.
    pub synced: u32,
    /// Per-repo error messages for specs that failed to sync.
    pub errors: Vec<String>,
}

/// [MUTATES STATE] Connect to `server` (a registered MCP server), call its
/// `{prefix}_spec_list` and `{prefix}_spec_schema` tools, and upsert every
/// advertised repo into orca.db.
#[orca_tool(domain = "mcp", verb = "sync_specs")]
async fn mcp_sync_specs(
    args: McpSyncSpecsArgs,
    _ctx: &contract::ToolCtx,
) -> anyhow::Result<SyncMcpSpecsResult> {
    sync_specs(&args.server).await
}

async fn sync_specs(server: &str) -> anyhow::Result<SyncMcpSpecsResult> {
    let pool = make_mcp_pool();
    let prefix = server.split('-').next().unwrap_or(server).to_string();
    let list_tool = format!("{prefix}_spec_list");
    let client = pool
        .get_or_connect(server)
        .await
        .with_context(|| format!("connect MCP server '{server}'"))?;

    let list_result = client
        .call_tool(&list_tool, json!({}), "mcp.sync_specs")
        .await
        .with_context(|| format!("{list_tool} failed"))?;

    let text = list_result["content"]
        .as_array()
        .and_then(|arr| {
            arr.iter()
                .find_map(|c| c["text"].as_str().map(str::to_string))
        })
        .unwrap_or_default();

    let repos: Vec<String> =
        if let Ok(arr) = plugin_toolkit::serde_json::from_str::<Vec<Value>>(&text) {
            arr.into_iter()
                .filter_map(|v| {
                    v["repo"]
                        .as_str()
                        .or_else(|| v["name"].as_str())
                        .or_else(|| v.as_str())
                        .map(str::to_string)
                })
                .collect()
        } else {
            text.lines()
                .map(|l| {
                    l.trim()
                        .trim_start_matches("• ")
                        .split_whitespace()
                        .next()
                        .unwrap_or("")
                        .to_string()
                })
                .filter(|s| !s.is_empty() && !s.contains(':'))
                .collect()
        };

    if repos.is_empty() {
        return Err(anyhow!("MCP spec list returned no repos"));
    }

    let schema_tool = format!("{prefix}_spec_schema");
    let mut synced = 0u32;
    let mut errors: Vec<String> = Vec::new();

    for repo in &repos {
        if repo.is_empty() {
            continue;
        }
        match client
            .call_tool(&schema_tool, json!({ "repo": repo }), "mcp.sync_specs")
            .await
        {
            Err(e) => errors.push(format!("{repo}: {e}")),
            Ok(r) => {
                let spec_text = r["content"].as_array().and_then(|arr| {
                    arr.iter()
                        .find_map(|c| c["text"].as_str().map(str::to_string))
                });
                let Some(spec_text) = spec_text else {
                    errors.push(format!("{repo}: empty schema response"));
                    continue;
                };
                if plugin_toolkit::serde_json::from_str::<Value>(&spec_text).is_err() {
                    errors.push(format!("{repo}: non-JSON schema"));
                    continue;
                }
                let row = core_tables::openapi_specs::OpenApiSpecRow {
                    name: repo.clone(),
                    url: None,
                    source_mcp: Some(prefix.clone()),
                    spec_json: Some(spec_text),
                    cached_at: Some(plugin_toolkit::time::now().to_rfc3339()),
                    enabled: true,
                };
                match core_tables::openapi_specs::upsert(&row) {
                    Ok(_) => synced += 1,
                    Err(e) => errors.push(format!("{repo}: db error: {e}")),
                }
            }
        }
    }

    Ok(SyncMcpSpecsResult {
        server: server.to_string(),
        synced,
        errors,
    })
}
