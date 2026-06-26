#!/usr/bin/env bash
# Register an MCP server with the orca registry.
#
# Thin wrapper over `mcp.update` (the plugin's own tool). Prefer calling the
# tool directly via the orca CLI/API; this exists for shell-driven bootstrap.
# Usage: scripts/configure.sh NAME COMMAND [ARGS...]
set -euo pipefail

NAME="${1:?usage: configure.sh NAME COMMAND [ARGS...]}"
COMMAND="${2:?usage: configure.sh NAME COMMAND [ARGS...]}"
shift 2

orca tool mcp.update --name "$NAME" --command "$COMMAND" ${@:+--args "$@"}
echo "registered MCP server '$NAME'"
