<p align="center">
  <img src="assets/icon-256.png" width="120" alt="mcp" />
</p>

# mcp

An [orca](https://github.com/argyle-labs/orca) plugin that turns orca into an
**MCP (Model Context Protocol) client**. It federates registered MCP servers —
over stdio (subprocess JSON-RPC) and HTTP/SSE — into orca's tool surface, keeps a
registry of servers + per-server tool mappings, and proxies `tools/call`.

This is a **client**, not a server: it does not host an MCP service of its own.
It speaks the MCP wire protocol to whatever servers the operator registers.

## What it is (and isn't)

The plugin is an orca **cdylib** the orca daemon `dlopen`s at runtime. There is
no container image, systemd unit, or LXC for this plugin — the orca daemon is
its host process. "Deploying" it means building the cdylib and dropping it in
orca's plugin directory; see [Install](#install).

The only durable state is the registry (registered servers + tool mappings),
which lives in orca's own state DB — not in any file this plugin owns.

## Tool surface

One flat surface; an MCP server is the resource and its tool mappings nest into
the row.

| Tool | Purpose |
|---|---|
| `mcp.list` | Every registered MCP server with its tool mappings. |
| `mcp.detail` | One server (+ mappings) and the live tool advertisement; omit `name` for the full federated catalogue. |
| `mcp.update` | Register/update a server, map/unmap a tool, or run a tool sync — args select the sub-op. |
| `mcp.delete` | Remove a server (cascades its mappings). |
| `mcp.run` | Invoke a tool on a registered server (the `tools/call` envelope). |
| `mcp.health` | Live connect + `initialize`/`tools/list` handshake probe of registered server(s) — real I/O, not a table check. See [`src/lifecycle.rs`](src/lifecycle.rs). |

Servers are discovered from three sources (highest precedence first): explicit
registry rows, an enabled orca plugin's manifest MCP block, and the
`mcpServers` map in `~/.claude.json`.

## Transports

- **stdio** — `command` is a binary (e.g. `npx`). The plugin spawns it,
  augments `PATH` so daemon environments can still find `node`/`npx`/etc., and
  reaps the child on drop.
- **HTTP/SSE** — `command` is an `http(s)://` URL. Each request opens `/sse`,
  reads the session endpoint, POSTs the JSON-RPC message, and reads the reply
  off the same stream. Optional bearer token + priority-ordered fallback URLs.

## Configure / register servers

Via the plugin's own tools (preferred):

```sh
# stdio server
orca tool mcp.update --name context7 --command npx --args -y --args @upstash/context7-mcp
# HTTP/SSE server on localhost
orca tool mcp.update --name local-sse --command http://127.0.0.1:8765
# probe reachability
orca tool mcp.health
```

Or via `~/.claude.json` — see [`examples/claude.json`](examples/claude.json) and
[`examples/register.sh`](examples/register.sh). Examples use `127.0.0.1` only.

## Install

```sh
# Builds the cdylib and installs it into ~/.orca/plugins (override with arg 1
# or $ORCA_PLUGIN_DIR), then restart the orca daemon to load it.
scripts/install.sh
```

`scripts/update.sh` pulls + rebuilds + reinstalls. `scripts/backup.sh` /
`scripts/restore.sh` snapshot the orca state DB (where the registry lives).
`scripts/entrypoint.sh` deliberately fails — this plugin has no standalone
runtime.

## Build

```sh
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
```

With an orca checkout at `../orca`, the committed
[`.cargo/config.toml`](.cargo/config.toml) `[patch]` resolves `plugin-toolkit`
locally; otherwise it resolves from the pinned rc tag in `Cargo.toml`.

---

## The two-dependency rule

A compliant orca plugin's `[dependencies]` is essentially **one crate** —
`plugin-toolkit` — plus the unavoidable `abi_stable`. This plugin carries one
additional, **justified** niche dep:

| Dep | Why it is allowed |
|---|---|
| `plugin-toolkit` | The single orca gateway. Every other crate the plugin would reach for — serde, serde_json, schemars, clap, anyhow, async-trait, tokio, tracing, chrono, reqwest, the `futures_util` streaming combinators, and the `db` / `dispatch` / `contract` / `derive` / `utils::{http,json_schema,path}` surface — is reached through `plugin_toolkit::*` / its prelude or injected by the `#[plugin_struct]` / `#[orca_tool]` macros. The plugin source names **no** other orca-side crate. |
| `abi_stable` | **The one genuine non-toolkit framework dependency, and it cannot be removed.** See below. |
| `dirs` | **A justified niche dep.** `dirs::home_dir()` resolves the OS-specific home directory to locate the orca state DB when `ORCA_DB_PATH` is unset. It is a tiny cross-platform path helper the toolkit does not re-export; like `abi_stable` it is a genuinely-external crate, not an orca-side capability. If the toolkit grows a home-dir helper, this dep goes away. |

Everything under `[dev-dependencies]` (`tokio` for the async `kill_on_drop`
test, `plugin-toolkit` again) is outside the rule: dev-deps never ship in the
cdylib.

> `cargo tree -e normal --depth 1` for this crate shows exactly:
> `plugin-toolkit`, `abi_stable`, `dirs`.

### Why `abi_stable` is the unavoidable exception

orca loads external plugins as **cdylibs it `dlopen`s at runtime** — not as
statically linked crates. That crossing is a C-ABI FFI boundary, and the data
that crosses it (the root module, the version header, the layout hashes the
loader checks before it trusts the `.so`/`.dylib`) must have a **guaranteed,
stable memory layout**. Rust's native `repr(Rust)` gives no such guarantee
across independent compilations, so the boundary types come from `abi_stable`
(`RString`, `RStr`, `RResult`, `PrefixTypeTrait`, …).

The decisive detail: `#[export_root_module]` — the attribute that emits the
single symbol orca's loader looks up — **expands to bare `::abi_stable::*` paths
in this crate's own root.** There is no source path for the toolkit to redirect
and no `crate =` attribute to retarget; the macro hard-codes the crate name into
generated code that lives *in the plugin*. So unlike serde/reqwest (whose paths
route through `::plugin_toolkit::*`), `abi_stable` genuinely must be a direct dep.

It is pinned to **the same `abi_stable` version the toolkit uses** (`0.11`) so the
layout hash baked into the cdylib matches what orca's `plugin-loader` validates at
load time. A version skew here is not a compile error — it is a load-time
rejection. Keep it in lockstep with the toolkit.

The whole abi boundary is isolated to one file,
[`src/abi_export.rs`](src/abi_export.rs): the only place `abi_stable` is named,
the only place the JSON dispatch payload type is aliased, and the only place the
`disallowed_types` lint is suppressed.

### Authoring a fresh plugin from this template

To start a new `<name>` plugin modelled on this one:

1. **Scaffold the crate.** Copy `Cargo.toml`, `.cargo/config.toml`,
   `src/abi_export.rs`, and a `src/` tree (`lib.rs`, `tools.rs`, plus
   `lifecycle.rs` as the surface needs). Keep
   `[lib] crate-type = ["cdylib", "rlib"]` — `cdylib` is the artifact orca loads;
   `rlib` keeps the in-crate test harness. Use `edition = "2024"` if you use
   `if let … && …` let-chains.

2. **Set `[dependencies]` to `plugin-toolkit` + `abi_stable = "0.11"`** — and
   nothing else *unless* a crate is genuinely unreachable through the toolkit
   (e.g. `dirs` here). Justify every such dep in a Cargo.toml comment. Put test
   tooling under `[dev-dependencies]`.

3. **Write the surface against the toolkit only.** `use plugin_toolkit::prelude::*;`
   for the common surface; reach `plugin_toolkit::http`,
   `plugin_toolkit::serde_json`, `plugin_toolkit::json_schema`,
   `plugin_toolkit::futures_util`, etc. explicitly where the prelude doesn't
   cover it. Derive on hand-written types via `#[plugin_struct]` (or the explicit
   `#[derive(plugin_toolkit::serde::Serialize, …)] #[serde(crate =
   "plugin_toolkit::serde")]` form).

4. **Update `abi_export.rs` metadata** — change `target_software`,
   `target_compat`, `orca_compat`, and `TOOL_PREFIX` to your `<name>.` namespace.
   Leave the rest of the FFI plumbing as-is.

5. **Prove the rule holds** before committing:
   ```sh
   cargo build && cargo clippy --all-targets -- -D warnings && cargo test
   cargo tree -e normal --depth 1   # only plugin-toolkit + abi_stable (+ justified niche deps)
   ```
   Any unjustified third crate under `[dependencies]` is a toolkit gap — file it
   against `plugin-toolkit` and route through it rather than adding the dep here.
