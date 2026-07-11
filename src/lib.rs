//! MCP (Model Context Protocol) client plugin for orca.
//!
//! Federates registered MCP servers — over stdio (subprocess JSON-RPC) and
//! HTTP/SSE — into orca's tool surface, and exposes registry CRUD:
//!
//! - `mcp.{list, detail, update, delete}` — registered-server registry +
//!   per-server tool mappings.
//! - `mcp.run` — invoke a tool on a registered server (the `tools/call`
//!   envelope).
//! - `mcp.health` — live connect + handshake probe of registered server(s)
//!   (see `lifecycle`).
//! - `mcp.sync_specs` — pull OpenAPI specs from a registered server's
//!   `{prefix}_spec_list`/`{prefix}_spec_schema` tools into orca's spec
//!   registry (see `sync_specs`; formerly core `spec.sync-mcp`).
//!
//! The long-lived `McpPool` (`client`) owns the JSON-RPC clients and is shared
//! by every tool. Registry rows live in orca's own state DB, read through the
//! toolkit's re-exported `db` surface.
//!
//! Every import flows through `plugin_toolkit::*` / its prelude — the toolkit is
//! the single gateway. The only non-orca crate this plugin names is `dirs` (OS
//! home-dir lookup).

pub mod client;
pub mod context7;
pub mod lifecycle;
pub mod manifest;
pub mod sync;
pub mod sync_specs;
pub mod tools;
pub mod types;
