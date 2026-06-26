#!/usr/bin/env bash
# Register MCP servers with orca via the plugin's own tool surface.
# Registry rows take precedence over ~/.claude.json entries.
set -euo pipefail

# A stdio server (subprocess JSON-RPC).
orca tool mcp.update --name context7 --command npx --args -y --args @upstash/context7-mcp

# An HTTP/SSE server on localhost.
orca tool mcp.update --name local-sse --command http://127.0.0.1:8765

# Probe reachability of everything registered.
orca tool mcp.health
