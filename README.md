<p align="center">
  <img src="assets/icon-256.png" width="120" alt="mcp" />
</p>

# mcp

Turns orca into an MCP client — it federates existing MCP servers (stdio + HTTP/SSE) into orca's tool surface.

A first-party [orca](https://github.com/argyle-labs/orca) plugin (MCP client).

This is a **backend/adapter** — it has no service of its own; it wires an existing system into orca.

---

## Run it without orca

There's nothing to deploy: this plugin drives software you already run (upstream: <https://modelcontextprotocol.io/>). Install/configure that directly, then register it with orca.


## With orca

orca drives this plugin through its generic surface — rich, mcp-specific data comes back in the typed `service.status` payload, never bespoke tools.

## Layout

- `src/` — the plugin (pure Rust): the `ServiceBackend` descriptor + `configure` / `status`.
- `scripts/` — provisioning / lifecycle helpers.
- `assets/` — plugin icon.
