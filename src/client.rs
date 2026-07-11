#![allow(clippy::disallowed_types)] // MCP JSON-RPC protocol — opaque tool args/results
use std::collections::HashMap;
use std::sync::Arc;

use plugin_toolkit::core_tables;

/// Resolve a bare command name to an absolute path.
///
/// Launchd and other minimal environments strip PATH down to system directories,
/// so `node`, `npx`, etc. won't be found even when they're installed. Try `which`
/// first (works in interactive shells), then probe well-known install locations.
/// Build a PATH that includes all well-known tool install directories so that
/// processes spawned by orca (MCP servers and their children) can find CLIs
/// like `node`, `npx`, etc. even in minimal daemon environments.
fn augmented_path() -> String {
    let current = std::env::var("PATH").unwrap_or_default();
    let home = std::env::var("HOME").unwrap_or_default();

    let mut extra: Vec<String> = vec![
        format!("{home}/.local/bin"),
        format!("{home}/.volta/bin"),
        format!("{home}/.fnm/current/bin"),
        "/opt/homebrew/bin".to_string(),
        "/opt/homebrew/sbin".to_string(),
        "/usr/local/bin".to_string(),
    ];

    // Add bin dirs for ALL installed nvm node versions. This avoids having to
    // resolve the alias chain (e.g. "24" → "v24.15.0") which nvm handles lazily.
    let nvm_versions = format!("{home}/.nvm/versions/node");
    if let Ok(entries) = std::fs::read_dir(&nvm_versions) {
        for entry in entries.flatten() {
            let bin = entry.path().join("bin");
            if bin.is_dir() {
                extra.push(bin.to_string_lossy().into_owned());
            }
        }
    }

    let mut parts: Vec<&str> = current.split(':').filter(|s| !s.is_empty()).collect();
    for dir in extra.iter().rev() {
        if !parts.contains(&dir.as_str()) {
            parts.insert(0, dir);
        }
    }
    parts.join(":")
}

fn resolve_command(command: &str) -> String {
    if command.starts_with('/') {
        return command.to_string();
    }
    // which works when PATH is rich (interactive shell, dev mode)
    if let Some(resolved) = plugin_toolkit::path::which(command)
        && std::path::Path::new(&resolved).exists()
    {
        return resolved;
    }
    // Probe known install paths — covers launchd/systemd daemon environments
    let mut candidates: Vec<String> = vec![
        format!("/opt/homebrew/bin/{command}"), // Apple Silicon Homebrew
        format!("/usr/local/bin/{command}"),    // Intel Homebrew + manual installs
        format!("/usr/bin/{command}"),
        format!("/bin/{command}"),
    ];
    if let Ok(home) = std::env::var("HOME") {
        // nvm: read the default alias to find the active version
        let nvm_default = format!("{home}/.nvm/alias/default");
        if let Ok(ver) = std::fs::read_to_string(&nvm_default) {
            let ver = ver.trim().to_string();
            candidates.push(format!("{home}/.nvm/versions/node/{ver}/bin/{command}"));
            if !ver.starts_with('v') {
                candidates.push(format!("{home}/.nvm/versions/node/v{ver}/bin/{command}"));
            }
        }
        candidates.push(format!("{home}/.local/bin/{command}"));
        candidates.push(format!("{home}/.volta/bin/{command}")); // Volta
        candidates.push(format!("{home}/.fnm/current/bin/{command}")); // fnm
    }
    for path in &candidates {
        if std::path::Path::new(path).exists() {
            return path.clone();
        }
    }
    tracing::warn!(
        "could not resolve '{command}' to an absolute path; using as-is (may fail in daemon mode)"
    );
    command.to_string()
}

use std::sync::Mutex as StdMutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use plugin_toolkit::anyhow::{self, Context, Result};
use plugin_toolkit::client::{Client, Request};
use plugin_toolkit::process;
use plugin_toolkit::serde_json::{self, Value, json};
use plugin_toolkit::time;
use plugin_toolkit::tracing;

#[derive(Clone, plugin_toolkit::serde::Deserialize)]
#[serde(crate = "plugin_toolkit::serde")]
pub struct McpServerConfig {
    pub command: String,
    pub args: Vec<String>,
    #[serde(default)]
    pub env: std::collections::HashMap<String, String>,
    /// Bearer token for HTTP/SSE transport (resolved from token_env at config load time).
    pub token: Option<String>,
    /// Additional SSE URLs tried in order if `command` is an http URL that fails.
    /// Priority: command (index 0) → fallback_urls[0] → fallback_urls[1] → ...
    #[serde(default)]
    pub fallback_urls: Vec<String>,
}

// ── Transport backends ────────────────────────────────────────────────────────

enum Transport {
    /// Persistent stdio child spoken to line-by-line over JSON-RPC. The toolkit
    /// `process::Child` owns its own internal stdio lock and mints + correlates
    /// JSON-RPC ids inside `request`, so the transport needs no lock of its own.
    /// The seam kills the child on drop. Boxed so the stdio child (the larger
    /// variant) does not bloat every `Sse` instance.
    Stdio { child: Box<process::Child> },
    /// HTTP/SSE transport (MCP over Server-Sent Events).
    /// Each request opens a fresh /sse connection, gets a session endpoint, POSTs
    /// the JSON-RPC message, then reads the response from that same SSE stream.
    /// This is stateless per-request and matches the MCP /sse + /message model.
    Sse {
        base_url: String,
        token: Option<String>,
        http: Client,
    },
}

pub struct McpClient {
    transport: Transport,
    // Monotonic JSON-RPC id counter, used only for SSE correlation (the stdio
    // seam mints its own ids). Plain atomic — no async runtime named.
    next_id: AtomicU64,
    pub tools: Vec<McpTool>,
}

#[derive(Clone, plugin_toolkit::serde::Serialize, plugin_toolkit::serde::Deserialize)]
#[serde(crate = "plugin_toolkit::serde")]
pub struct McpTool {
    pub name: String,
    pub description: String,
    #[serde(rename = "inputSchema")]
    pub input_schema: plugin_toolkit::json_schema::JsonSchemaNode,
}

impl McpClient {
    pub async fn connect(cfg: &McpServerConfig) -> Result<Self> {
        if cfg.command.starts_with("http://") || cfg.command.starts_with("https://") {
            // Try each URL in priority order, returning the first that succeeds.
            let all_urls = std::iter::once(cfg.command.as_str())
                .chain(cfg.fallback_urls.iter().map(|s| s.as_str()));
            let mut last_err = anyhow::anyhow!("no URLs configured");
            for url in all_urls {
                let mut candidate = cfg.clone();
                candidate.command = url.to_string();
                candidate.fallback_urls = vec![];
                match Self::connect_sse(&candidate).await {
                    Ok(client) => return Ok(client),
                    Err(e) => {
                        tracing::debug!("MCP SSE failed for {url}: {e}");
                        last_err = e;
                    }
                }
            }
            Err(last_err)
        } else {
            Self::connect_stdio(cfg).await
        }
    }

    async fn connect_stdio(cfg: &McpServerConfig) -> Result<Self> {
        let resolved = resolve_command(&cfg.command);

        // The persistent-child seam pipes stdin/stdout, inherits stderr, and
        // kills the child on drop — so a dropped `McpClient` never leaks the
        // federated subprocess.
        let mut cmd = process::Command::new(&resolved).args(&cfg.args);

        // Augment PATH so MCP server subprocesses can find tools (node, npx, etc.)
        // that live in nvm/volta/fnm/homebrew paths stripped by launchd/systemd daemons.
        cmd = cmd.env("PATH", augmented_path());

        // DOCKER_HOST is exposed by the docker plugin through the subprocess-env
        // seam (no docker_runtimes table); forward it to the federated child so
        // docker-backed MCP servers reach the daemon.
        if let Some((_, host)) = plugin_toolkit::contract::subprocess_env::collect()
            .into_iter()
            .find(|(k, _)| k == "DOCKER_HOST")
        {
            cmd = cmd.env("DOCKER_HOST", host);
        }
        for (k, v) in &cfg.env {
            cmd = cmd.env(k, v);
        }

        let child = cmd.spawn().context("failed to spawn MCP stdio child")?;

        let mut client = McpClient {
            transport: Transport::Stdio {
                child: Box::new(child),
            },
            next_id: AtomicU64::new(0),
            tools: vec![],
        };

        client.handshake().await?;
        Ok(client)
    }

    async fn connect_sse(cfg: &McpServerConfig) -> Result<Self> {
        let base_url = cfg.command.trim_end_matches('/').to_string();
        let token = cfg.token.clone().filter(|t| !t.is_empty());
        let http = Client::new();

        // Probe with a health check before attempting handshake.
        let mut health_req = Request::new("GET", format!("{base_url}/health")).timeout_ms(60_000);
        if let Some(token) = &token {
            health_req = health_req.header("Authorization", format!("Bearer {token}"));
        }
        let health = http.send(health_req)?;
        if !health.is_success() {
            anyhow::bail!("SSE server health check failed: HTTP {}", health.status);
        }

        let mut client = McpClient {
            transport: Transport::Sse {
                base_url,
                token,
                http,
            },
            next_id: AtomicU64::new(0),
            tools: vec![],
        };

        client.handshake().await?;
        Ok(client)
    }

    async fn handshake(&mut self) -> Result<()> {
        let init_resp = self
            .request(
                "initialize",
                json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": {},
                    "clientInfo": { "name": "orca", "version": "1.0" }
                }),
            )
            .await?;
        drop(init_resp);

        self.notify("notifications/initialized", json!({})).await?;

        let tools_resp = self.request("tools/list", json!({})).await?;
        let tools: Vec<McpTool> = tools_resp["result"]["tools"]
            .as_array()
            .unwrap_or(&vec![])
            .iter()
            .map(|t| McpTool {
                name: t["name"].as_str().unwrap_or("").to_string(),
                description: t["description"].as_str().unwrap_or("").to_string(),
                input_schema: serde_json::from_value(t["inputSchema"].clone()).unwrap_or_default(),
            })
            .collect();
        self.tools = tools;
        Ok(())
    }

    fn next_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    async fn request(&self, method: &str, params: Value) -> Result<Value> {
        self.request_timeout(method, params, 30).await
    }

    async fn request_timeout(
        &self,
        method: &str,
        params: Value,
        timeout_secs: u64,
    ) -> Result<Value> {
        // No shared request lock: the stdio seam serializes concurrent `request`
        // calls behind its own internal stdio lock and correlates by injected id,
        // and each SSE request opens its own isolated session, so responses can
        // never cross.
        match &self.transport {
            Transport::Stdio { child } => {
                // The seam mints + correlates the JSON-RPC id itself, so `id` is a
                // placeholder overwritten by `request`. It performs the interleaved
                // write + correlated read in one round trip.
                let msg = json!({ "jsonrpc": "2.0", "id": 0, "method": method, "params": params });
                let line = serde_json::to_string(&msg)?;
                let resp = child
                    .request(&line, Duration::from_secs(timeout_secs))
                    .await?;
                Ok(serde_json::from_str(resp.trim())?)
            }

            Transport::Sse {
                base_url,
                token,
                http,
            } => {
                // Per-request SSE: open /sse, get session endpoint, POST request,
                // read the correlated response off that same stream. Each request
                // gets its own isolated session, and JSON-RPC id matching keeps
                // responses from crossing. This id correlation is MCP-domain and
                // stays in the plugin; the SSE parse belongs to the toolkit stream.
                let id = self.next_id();
                let msg = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });

                let mut open_req = Request::new("GET", format!("{base_url}/sse"))
                    .header("Accept", "text/event-stream")
                    .timeout_ms(timeout_secs.saturating_mul(1000));
                if let Some(token) = token {
                    open_req = open_req.header("Authorization", format!("Bearer {token}"));
                }
                let mut events = http.events(open_req)?;
                if !(200..300).contains(&events.status()) {
                    anyhow::bail!("SSE open failed: HTTP {}", events.status());
                }

                // First event carries the `/message?sessionId=…` endpoint.
                let session_post = loop {
                    match events.next() {
                        Some(evt) if !evt.data.trim().is_empty() => {
                            break evt.data.trim().to_string();
                        }
                        Some(_) => continue,
                        None => anyhow::bail!("SSE closed before endpoint event"),
                    }
                };

                let post_url = if session_post.starts_with("http") {
                    session_post
                } else {
                    format!("{base_url}{session_post}")
                };

                let mut post_req = Request::new("POST", &post_url)
                    .json(&msg)?
                    .timeout_ms(timeout_secs.saturating_mul(1000));
                if let Some(token) = token {
                    post_req = post_req.header("Authorization", format!("Bearer {token}"));
                }
                http.send(post_req)?;

                while let Some(evt) = events.next() {
                    let data = evt.data.trim();
                    if data.is_empty() {
                        continue;
                    }
                    let resp: Value = serde_json::from_str(data)?;
                    if resp["id"] == id {
                        return Ok(resp);
                    }
                }
                anyhow::bail!("SSE stream closed before response")
            }
        }
    }

    async fn notify(&self, method: &str, params: Value) -> Result<()> {
        let msg = json!({ "jsonrpc": "2.0", "method": method, "params": params });

        match &self.transport {
            Transport::Stdio { child } => {
                let line = serde_json::to_string(&msg)?;
                child.notify(&line).await?;
            }
            Transport::Sse {
                base_url,
                token,
                http,
            } => {
                // Notifications via SSE: open a session, POST the notification.
                // The peer ignores notifications that aren't JSON-RPC requests
                // (no `id` field means no response expected). Fire and forget.
                let mut open_req = Request::new("GET", format!("{base_url}/sse"))
                    .header("Accept", "text/event-stream")
                    .timeout_ms(5_000);
                if let Some(token) = token {
                    open_req = open_req.header("Authorization", format!("Bearer {token}"));
                }
                if let Ok(mut events) = http.events(open_req)
                    && (200..300).contains(&events.status())
                {
                    let mut session_post = String::new();
                    while let Some(evt) = events.next() {
                        if !evt.data.trim().is_empty() {
                            session_post = evt.data.trim().to_string();
                            break;
                        }
                    }
                    if !session_post.is_empty() {
                        let post_url = if session_post.starts_with("http") {
                            session_post
                        } else {
                            format!("{base_url}{session_post}")
                        };
                        let mut post_req = Request::new("POST", &post_url).timeout_ms(5_000);
                        if let Ok(req) = post_req.json(&msg) {
                            post_req = req;
                            if let Some(token) = token {
                                post_req =
                                    post_req.header("Authorization", format!("Bearer {token}"));
                            }
                            _ = http.send(post_req);
                        }
                    }
                }
            }
        }
        Ok(())
    }

    pub async fn call_tool(
        &self,
        name: &str,
        arguments: Value,
        correlation_id: &str,
    ) -> Result<Value> {
        tracing::trace!(
            correlation_id = %correlation_id,
            tool = %name,
            arguments = %arguments,
            "→ mcp call"
        );

        let resp = self
            .request_timeout(
                "tools/call",
                json!({ "name": name, "arguments": arguments }),
                300, // 5 minutes — agent runs can take much longer than 30s
            )
            .await?;

        if let Some(err) = resp.get("error") {
            tracing::trace!(
                correlation_id = %correlation_id,
                tool = %name,
                error = %err,
                "← mcp error"
            );
            anyhow::bail!("MCP error: {err}");
        }

        let result = resp["result"].clone();
        tracing::trace!(
            correlation_id = %correlation_id,
            tool = %name,
            result = %result,
            "← mcp result"
        );

        Ok(result)
    }
}

pub struct McpPool {
    clients: StdMutex<HashMap<String, Arc<McpClient>>>,
    db_path: Option<std::path::PathBuf>,
}

impl Default for McpPool {
    fn default() -> Self {
        Self::new()
    }
}

impl McpPool {
    pub fn new() -> Self {
        McpPool {
            clients: StdMutex::new(HashMap::new()),
            db_path: None,
        }
    }

    pub fn new_with_db(db_path: std::path::PathBuf) -> Self {
        McpPool {
            clients: StdMutex::new(HashMap::new()),
            db_path: Some(db_path),
        }
    }

    pub fn read_configs(&self) -> HashMap<String, McpServerConfig> {
        let mut configs = Self::read_claude_configs();

        // DB servers take precedence over ~/.claude.json. The core-table helpers
        // route over the capability sink to the ambient orca db, so `db_path` is
        // now only an advisory gate: read DB-backed servers when one is set.
        if self.db_path.is_some() {
            if let Ok(rows) = core_tables::mcp_servers::list() {
                for row in rows {
                    configs.insert(
                        row.name.clone(),
                        McpServerConfig {
                            command: row.command,
                            args: row.args,
                            env: row.env,
                            token: None,
                            fallback_urls: vec![],
                        },
                    );
                }
            }
            // Enabled plugins that declare an MCP server are auto-federated.
            // Plugin entries take precedence over ~/.claude.json but not over explicit mcp_servers rows.
            if let Ok(plugins) = core_tables::plugins::list() {
                for p in plugins {
                    if !p.enabled {
                        continue;
                    }

                    // Transport lives in the manifest, not the row — re-parse on demand.
                    let Ok((manifest, _)) = crate::manifest::parse_path(&p.manifest_path) else {
                        continue;
                    };
                    let Some(mcp) = manifest.plugin.mcp else {
                        continue;
                    };
                    // urls (priority-ordered list) override stdio command.
                    // All URLs are passed; connect() tries them in order.
                    let urls = mcp.urls();
                    let (cmd, fallback_urls) = if !urls.is_empty() {
                        let mut it = urls.into_iter();
                        let primary = it.next().unwrap();
                        (primary, it.collect::<Vec<_>>())
                    } else if let Some(c) = mcp.command_nonempty() {
                        (c.to_string(), vec![])
                    } else {
                        continue;
                    };
                    // Merge stored credentials (orca creds set) into env so the subprocess
                    // receives them without requiring the caller to export them manually.
                    let mut env = mcp.env;
                    let mut token: Option<String> = None;
                    if let Ok(creds) = core_tables::plugin_creds::list(&p.id) {
                        for c in creds {
                            // If this credential matches token_env, use it as Bearer token.
                            if mcp.token_env.as_deref() == Some(c.key.as_str()) {
                                token = Some(c.value.clone());
                            }
                            env.insert(c.key, c.value);
                        }
                    }
                    configs.entry(p.id).or_insert(McpServerConfig {
                        command: cmd,
                        args: mcp.args,
                        env,
                        token,
                        fallback_urls,
                    });
                }
            }
        }

        configs
    }

    fn read_claude_configs() -> HashMap<String, McpServerConfig> {
        let home = std::env::var("HOME").unwrap_or_default();
        let path = format!("{home}/.claude.json");
        let Ok(raw) = std::fs::read_to_string(&path) else {
            return HashMap::new();
        };
        let Ok(json): Result<Value, _> = serde_json::from_str(&raw) else {
            return HashMap::new();
        };
        let Some(servers) = json["mcpServers"].as_object() else {
            return HashMap::new();
        };
        servers
            .iter()
            .filter_map(|(k, v)| {
                let command = v["command"].as_str()?.to_string();
                let args = v["args"]
                    .as_array()
                    .unwrap_or(&vec![])
                    .iter()
                    .filter_map(|a| a.as_str().map(|s| s.to_string()))
                    .collect();
                let env = v["env"]
                    .as_object()
                    .map(|m| {
                        m.iter()
                            .filter_map(|(ek, ev)| ev.as_str().map(|s| (ek.clone(), s.to_string())))
                            .collect()
                    })
                    .unwrap_or_default();
                Some((
                    k.clone(),
                    McpServerConfig {
                        command,
                        args,
                        env,
                        token: None,
                        fallback_urls: vec![],
                    },
                ))
            })
            .collect()
    }

    pub async fn get_or_connect(&self, server_name: &str) -> Result<Arc<McpClient>> {
        // Cache hit — return without connecting. The lock is not held across the
        // connect await (it is a std mutex); a concurrent connect for the same
        // server is a harmless race, last insert wins.
        if let Some(c) = self
            .clients
            .lock()
            .expect("clients lock poisoned")
            .get(server_name)
        {
            return Ok(c.clone());
        }
        let configs = self.read_configs();
        let cfg = configs
            .get(server_name)
            .ok_or_else(|| anyhow::anyhow!("unknown MCP server: {server_name}"))?;
        let client = Arc::new(McpClient::connect(cfg).await?);
        self.clients
            .lock()
            .expect("clients lock poisoned")
            .insert(server_name.to_string(), client.clone());
        Ok(client)
    }

    pub async fn evict(&self, server_name: &str) {
        self.clients
            .lock()
            .expect("clients lock poisoned")
            .remove(server_name);
    }

    pub async fn all_tools(&self) -> Vec<Value> {
        let configs = self.read_configs();
        let mut result = Vec::new();
        for server_name in configs.keys() {
            if let Ok(client) = self.get_or_connect(server_name).await {
                for tool in &client.tools {
                    result.push(json!({
                        "server": server_name,
                        "name": tool.name,
                        "description": tool.description,
                        "inputSchema": tool.input_schema,
                    }));
                }
            }
        }
        result
    }

    /// Like `all_tools` but skips named servers entirely — avoids connecting to them.
    ///
    /// Naming logic per tool (in priority order):
    /// 1. Explicit override in plugin's `command_map` (universal → internal).
    /// 2. Auto-strip: if tool name starts with `{plugin_id}_`, strip that prefix.
    /// 3. Pass-through: expose tool under its original name.
    ///
    /// The `alias` field carries the internal tool name when a rename occurred,
    /// used by the federation router to call the right name on the remote server.
    pub async fn all_tools_filtered(&self, skip: &[&str]) -> Vec<Value> {
        // Per plugin: inverse command_map (internal_name → universal_name) + id prefix
        struct PluginMeta {
            prefix: String,                   // "{id}_" — stripped from tool names automatically
            inverse: HashMap<String, String>, // internal_name → explicit universal_name
        }

        let plugin_meta: HashMap<String, PluginMeta> = core_tables::plugins::list()
            .unwrap_or_default()
            .into_iter()
            .filter(|p| p.enabled)
            .map(|p| {
                let prefix = format!("{}_", p.id);
                let inverse = p.command_map.into_iter().map(|(u, t)| (t, u)).collect();
                (p.id, PluginMeta { prefix, inverse })
            })
            .collect();

        let configs = self.read_configs();

        // Federate with a per-server hard deadline so that a single unreachable
        // server (e.g. an off-LAN homelab plugin) cannot block the entire
        // tools/list call. Servers that error or time out are silently dropped —
        // they simply don't appear in the federation set this call.
        let mut connected: Vec<(String, Arc<McpClient>)> = Vec::new();
        for name in configs
            .keys()
            .filter(|n| !skip.contains(&n.as_str()))
            .cloned()
        {
            if let Some(Ok(client)) =
                time::timeout(Duration::from_secs(3), self.get_or_connect(&name)).await
            {
                connected.push((name, client));
            }
        }

        let mut result = Vec::new();
        for (server_name, client) in &connected {
            let meta = plugin_meta.get(server_name.as_str());
            {
                for tool in &client.tools {
                    let universal = if let Some(m) = meta {
                        if let Some(explicit) = m.inverse.get(&tool.name) {
                            // Explicit override wins
                            explicit.clone()
                        } else if let Some(stripped) = tool.name.strip_prefix(&m.prefix) {
                            // Auto-strip plugin id prefix
                            stripped.to_string()
                        } else {
                            // No prefix match — pass through as-is
                            tool.name.clone()
                        }
                    } else {
                        tool.name.clone()
                    };

                    if universal == tool.name {
                        result.push(json!({
                            "server": server_name,
                            "name": universal,
                            "description": tool.description,
                            "inputSchema": tool.input_schema,
                        }));
                    } else {
                        result.push(json!({
                            "server": server_name,
                            "name": universal,
                            "alias": tool.name,
                            "description": tool.description,
                            "inputSchema": tool.input_schema,
                        }));
                    }
                }
            }
        }
        result
    }

    pub async fn find_ctx7_server(&self) -> Option<String> {
        let configs = self.read_configs();
        for server_name in configs.keys() {
            if let Ok(client) = self.get_or_connect(server_name).await
                && client.tools.iter().any(|t| t.name == "resolve-library-id")
            {
                return Some(server_name.clone());
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── resolve_command ───────────────────────────────────────────────────────

    #[test]
    fn resolve_command_absolute_path_returned_unchanged() {
        // Absolute paths bypass all resolution logic.
        assert_eq!(resolve_command("/usr/bin/env"), "/usr/bin/env");
        assert_eq!(resolve_command("/bin/bash"), "/bin/bash");
    }

    #[test]
    fn resolve_command_known_binary_returns_nonempty() {
        // "bash" exists on every CI/dev machine — we just need it to resolve to something.
        let resolved = resolve_command("bash");
        assert!(
            !resolved.is_empty(),
            "resolve_command('bash') should return non-empty"
        );
        // Should be an absolute path or the bare name unchanged
        assert!(
            resolved == "bash" || resolved.starts_with('/'),
            "got: {resolved}"
        );
    }

    #[test]
    fn resolve_command_unknown_returns_input_unchanged() {
        // A completely made-up command falls through all probes and returns as-is.
        let result = resolve_command("zzz_no_such_binary_xyz_999");
        assert_eq!(result, "zzz_no_such_binary_xyz_999");
    }

    // ── augmented_path ────────────────────────────────────────────────────────

    #[test]
    fn augmented_path_contains_homebrew_bin() {
        let path = augmented_path();
        // On macOS the output should include at least one of the standard dirs
        assert!(
            path.contains("/opt/homebrew/bin")
                || path.contains("/usr/local/bin")
                || path.contains("/usr/bin"),
            "augmented_path missing expected dirs: {path}",
        );
    }

    #[test]
    fn augmented_path_has_no_empty_segments() {
        let path = augmented_path();
        for segment in path.split(':') {
            assert!(!segment.is_empty(), "empty segment in PATH: {path}");
        }
    }

    #[test]
    fn augmented_path_does_not_add_duplicate_extra_dirs() {
        // The extras we inject should not appear twice.
        let path = augmented_path();
        let mut seen = std::collections::HashSet::new();
        for candidate in ["/opt/homebrew/bin", "/opt/homebrew/sbin", "/usr/local/bin"] {
            if path.contains(candidate) {
                assert!(
                    seen.insert(candidate),
                    "extra dir appears more than once: {candidate}"
                );
            }
        }
    }

    // ── stdio transport drives the persistent child ──────────────────────────

    /// The `Transport::Stdio` variant, built over the persistent-child seam,
    /// drives a JSON-RPC peer: `request` writes a request line to the child's
    /// stdin, mints + injects an id, and reads the correlated reply back. `cat`
    /// echoes each line verbatim (injected id included), so it is the minimal
    /// such peer. The seam owns kill-on-drop (verified in the toolkit's own
    /// `process` tests), so this test covers the transport wiring the plugin
    /// owns: that a request reaches the child and its reply comes back.
    ///
    /// Async is driven by the toolkit's shared reactor (`reactor::block_on`) —
    /// the plugin names no runtime of its own.
    #[test]
    fn stdio_transport_round_trips_a_request() {
        plugin_toolkit::reactor::block_on(async {
            let child = process::Command::new("cat")
                .spawn()
                .expect("spawn cat via seam");

            let client = McpClient {
                transport: Transport::Stdio {
                    child: Box::new(child),
                },
                next_id: AtomicU64::new(0),
                tools: vec![],
            };

            let Transport::Stdio { child } = &client.transport else {
                unreachable!("constructed as stdio");
            };
            let reply = child
                .request(
                    r#"{"jsonrpc":"2.0","method":"ping"}"#,
                    Duration::from_secs(5),
                )
                .await
                .expect("request round-trip");
            let v: Value = serde_json::from_str(reply.trim()).expect("reply is JSON");
            assert_eq!(v["method"], "ping");
        });
    }
}
