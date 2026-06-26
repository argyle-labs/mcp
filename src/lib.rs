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
//!
//! The long-lived `McpPool` (`client`) owns the JSON-RPC clients and is shared
//! by every tool. Registry rows live in orca's own state DB, read through the
//! toolkit's re-exported `db` surface.
//!
//! Every import flows through `plugin_toolkit::*` / its prelude — the toolkit is
//! the single gateway. The only non-orca crates this plugin names are
//! `abi_stable` (the cdylib FFI boundary) and `dirs` (OS home-dir lookup).

mod abi_export;

pub mod client;
pub mod context7;
pub mod lifecycle;
pub mod sync;
pub mod tools;
pub mod types;
