#!/usr/bin/env bash
# Restore an orca state DB snapshot taken by backup.sh.
# Usage: scripts/restore.sh SNAPSHOT.db
set -euo pipefail

SNAP="${1:?usage: restore.sh SNAPSHOT.db}"
DB="${ORCA_DB_PATH:-$HOME/.orca/orca.db}"

if [[ ! -f "$SNAP" ]]; then
  echo "snapshot not found: $SNAP" >&2
  exit 1
fi

echo "stop the orca daemon before restoring, then press enter…"
read -r _
cp "$SNAP" "$DB"
echo "restored $SNAP -> $DB"
