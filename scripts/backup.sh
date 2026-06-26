#!/usr/bin/env bash
# Back up the MCP registry state.
#
# This plugin keeps NO state of its own: registered MCP servers + tool mappings
# live in orca's own state DB (default ~/.orca/orca.db, or $ORCA_DB_PATH), which
# orca already snapshots. This script copies that DB out as a convenience.
# Usage: scripts/backup.sh [OUT_DIR]
set -euo pipefail

OUT_DIR="${1:-./backup}"
DB="${ORCA_DB_PATH:-$HOME/.orca/orca.db}"

if [[ ! -f "$DB" ]]; then
  echo "orca state DB not found at $DB (set ORCA_DB_PATH)" >&2
  exit 1
fi

mkdir -p "$OUT_DIR"
STAMP="$(date +%Y%m%d-%H%M%S)"
cp "$DB" "${OUT_DIR}/orca-${STAMP}.db"
echo "backed up $DB -> ${OUT_DIR}/orca-${STAMP}.db"
