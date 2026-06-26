#!/usr/bin/env bash
# Build and install the MCP orca plugin cdylib.
#
# This plugin is NOT a standalone service — it is an orca plugin loaded by the
# orca daemon's plugin-loader. "Installing" means building the cdylib and
# dropping it where orca discovers plugins. There is no container or systemd
# unit of its own; the orca daemon hosts it.
#
# Usage: scripts/install.sh [ORCA_PLUGIN_DIR]
#   ORCA_PLUGIN_DIR defaults to ~/.orca/plugins
set -euo pipefail

PLUGIN_DIR="${1:-${ORCA_PLUGIN_DIR:-$HOME/.orca/plugins}}"

cargo build --release

# cdylib name is platform-specific: libmcp.so (Linux), libmcp.dylib (macOS).
case "$(uname -s)" in
  Darwin) LIB="libmcp.dylib" ;;
  *)      LIB="libmcp.so" ;;
esac

mkdir -p "$PLUGIN_DIR"
install -m 0644 "target/release/${LIB}" "${PLUGIN_DIR}/${LIB}"
echo "installed ${LIB} -> ${PLUGIN_DIR}/${LIB}"
echo "restart the orca daemon (or run 'orca plugin reload') to load it."
