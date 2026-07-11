//! Minimal `orca-plugin.toml` reader — the slice of the manifest the MCP
//! federation loader needs.
//!
//! Replaces the dropped db-crate `plugin_manifest::parse_path`. Only the
//! `[plugin.mcp]` transport section is modelled, matching exactly what
//! `client::read_configs` reads: the command/args/env, the SSE `url`/`urls`
//! priority list, and the `token_env` Bearer key.
//!
//! `toml` is named directly here because the toolkit re-exports no TOML parser;
//! it is the sole external crate this file needs (see the migration report).

use plugin_toolkit::serde::Deserialize;
use std::collections::HashMap;

#[derive(Deserialize)]
#[serde(crate = "plugin_toolkit::serde")]
pub struct Manifest {
    pub plugin: PluginSection,
}

#[derive(Deserialize)]
#[serde(crate = "plugin_toolkit::serde")]
pub struct PluginSection {
    #[serde(default)]
    pub mcp: Option<McpSection>,
}

#[derive(Deserialize, Default)]
#[serde(crate = "plugin_toolkit::serde")]
pub struct McpSection {
    #[serde(default)]
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    pub token_env: Option<String>,
    pub url: Option<String>,
    #[serde(default)]
    pub urls: Vec<String>,
}

impl McpSection {
    /// SSE URLs in priority order: the explicit `urls` list if present,
    /// otherwise the single `url`, otherwise empty (stdio transport).
    pub fn urls(&self) -> Vec<String> {
        if !self.urls.is_empty() {
            self.urls.clone()
        } else if let Some(u) = &self.url {
            vec![u.clone()]
        } else {
            vec![]
        }
    }

    /// The stdio command, or `None` when it is unset (empty string).
    pub fn command_nonempty(&self) -> Option<&str> {
        if self.command.is_empty() {
            None
        } else {
            Some(&self.command)
        }
    }
}

/// Parse an `orca-plugin.toml` from disk. Returns the manifest plus the
/// canonicalized absolute path string.
pub fn parse_path(path: &str) -> plugin_toolkit::anyhow::Result<(Manifest, String)> {
    use plugin_toolkit::anyhow::Context;

    let resolved = plugin_toolkit::path::expand_tilde(path);
    let abs = std::fs::canonicalize(&resolved)
        .with_context(|| format!("manifest not found: {resolved}"))?;
    let text = std::fs::read_to_string(&abs)
        .with_context(|| format!("failed to read {}", abs.display()))?;
    let manifest: Manifest = toml::from_str(&text)
        .with_context(|| format!("invalid orca-plugin.toml at {}", abs.display()))?;
    Ok((manifest, abs.to_string_lossy().into_owned()))
}
