#!/usr/bin/env bash
# No standalone runtime: the MCP plugin runs inside the orca daemon, which is
# its entrypoint. This script exists only to document that and to fail loudly if
# someone tries to run the plugin as a service.
echo "the mcp plugin is an orca cdylib, not a standalone service." >&2
echo "build + install it with scripts/install.sh, then run the orca daemon." >&2
exit 1
