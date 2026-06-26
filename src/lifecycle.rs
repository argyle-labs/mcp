//! MCP plugin lifecycle / health surface.
//!
//! Unlike a media-server plugin, the MCP plugin does **not** deploy a service
//! of its own — it is an in-process orca cdylib that speaks the MCP wire
//! protocol to whatever servers the operator has registered (via `mcp.update`,
//! `~/.claude.json`, or an enabled plugin's manifest). There is therefore no
//! container/LXC to provision, no image to bump, and no service volume to back
//! up: the only durable state is the registry rows in orca's own state DB,
//! which orca already snapshots.
//!
//! What this module *does* own is the lifecycle action that matters for an MCP
//! client: proving a registered server is actually reachable and handshakes
//! cleanly. `mcp.health` connects to one (or every) registered server, runs the
//! real `initialize` + `tools/list` handshake, and reports which came up. This
//! is a live I/O probe, not a config-table check.
//!
//! Imports flow through `plugin_toolkit::prelude::*` only — the toolkit is the
//! single gateway.
#![allow(clippy::disallowed_types)]

use std::sync::Arc;

use plugin_toolkit::contract;
use plugin_toolkit::prelude::*;

use crate::client::McpPool;

#[plugin_struct(args)]
pub struct McpHealthArgs {
    /// Server name to probe. Omit to probe every registered server.
    #[arg(long)]
    pub name: Option<String>,
}

#[plugin_struct]
pub struct McpHealthServer {
    pub server: String,
    /// True if the `initialize` + `tools/list` handshake succeeded.
    pub reachable: bool,
    /// Number of tools the server advertised (0 when unreachable).
    pub tools: u32,
    /// Connection error, when the handshake failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[plugin_struct]
pub struct McpHealthOutput {
    pub servers: Vec<McpHealthServer>,
}

fn make_mcp_pool() -> McpPool {
    use plugin_toolkit::contract::config::{APP_DB_FILE, APP_STATE_DIR};
    if let Ok(path) = std::env::var("ORCA_DB_PATH") {
        return McpPool::new_with_db(std::path::PathBuf::from(path));
    }
    if let Some(home) = dirs::home_dir() {
        return McpPool::new_with_db(home.join(APP_STATE_DIR).join(APP_DB_FILE));
    }
    McpPool::new()
}

/// Probe registered MCP server(s) with the real connect + handshake sequence
/// and report reachability. The lifecycle "is it alive" check for an MCP
/// client — does live I/O, never a table lookup.
#[orca_tool(domain = "mcp", verb = "health")]
async fn mcp_health(
    args: McpHealthArgs,
    _ctx: &contract::ToolCtx,
) -> Result<McpHealthOutput> {
    let pool = Arc::new(make_mcp_pool());
    let configs = pool.read_configs();

    let names: Vec<String> = match args.name {
        Some(n) => {
            if !configs.contains_key(&n) {
                bail!("MCP server '{n}' is not registered");
            }
            vec![n]
        }
        None => configs.keys().cloned().collect(),
    };

    let mut servers = Vec::with_capacity(names.len());
    for name in names {
        match pool.get_or_connect(&name).await {
            Ok(client) => servers.push(McpHealthServer {
                server: name,
                reachable: true,
                tools: client.tools.len() as u32,
                error: None,
            }),
            Err(e) => {
                // Drop any half-open client so a later probe reconnects cleanly.
                pool.evict(&name).await;
                servers.push(McpHealthServer {
                    server: name,
                    reachable: false,
                    tools: 0,
                    error: Some(e.to_string()),
                });
            }
        }
    }

    Ok(McpHealthOutput { servers })
}
