#!/usr/bin/env bash
# Pull the latest source and rebuild + reinstall the plugin cdylib.
# Usage: scripts/update.sh [ORCA_PLUGIN_DIR]
set -euo pipefail

git pull --ff-only
exec "$(dirname "$0")/install.sh" "$@"
