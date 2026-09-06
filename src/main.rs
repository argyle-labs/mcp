//! Dynamic (subprocess) entrypoint for the mcp plugin.
//!
//! Pure tool-surface plugin built on the typed [`Plugin`] builder. The plugin is
//! a `[[bin]]`, owns no runtime, and reaches orca only through the socket.
//!
//! `use mcp as _;` force-links this plugin's own lib crate so its `#[orca_tool]`
//! inventory survives linking — without it the `[[bin]]` references nothing in the
//! rlib and the linker drops every tool (this is what the macro's `link:` did).
plugin_toolkit::instrument::bootstrap!();
use mcp as _;
use plugin_toolkit::plugin::Plugin;

fn main() -> plugin_toolkit::anyhow::Result<()> {
    Plugin::named("mcp")
        .version(env!("CARGO_PKG_VERSION"))
        .tools(["mcp."])
        .serve()
}
