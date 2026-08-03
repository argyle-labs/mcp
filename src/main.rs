//! Dynamic (subprocess) entrypoint for the mcp plugin.
//!
//! The toolkit's `serve_tool_plugin!` emits `fn main`, serving this plugin over the orca
//! socket. Dynamic replacement for the retired cdylib export — the plugin is a
//! `[[bin]]`, owns no runtime, and reaches orca only through the socket.
plugin_toolkit::serve_tool_plugin! {
    name: "mcp",
    target_compat: "2024-11-05",
    link: mcp,
}
