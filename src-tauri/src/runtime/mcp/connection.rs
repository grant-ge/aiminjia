use std::collections::HashMap;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::Mutex;

use crate::runtime::dependencies::{
    ManagedRuntimePreference, ManagedRuntimeProcessEnv, ManagedRuntimeResolver,
    WorkspaceDependencies,
};
use crate::runtime::mcp::types::{McpServerConfig, McpToolDefinition};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpError {
    ConnectionFailed(String),
    ServerNotResponding,
    ToolNotFound(String),
    ToolExecutionFailed(String),
    InvalidResponse(String),
    AlreadyConnected,
    NotConnected,
    UnsupportedTransport(String),
}

impl std::fmt::Display for McpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ConnectionFailed(msg) => write!(f, "MCP connection failed: {msg}"),
            Self::ServerNotResponding => write!(f, "MCP server not responding"),
            Self::ToolNotFound(name) => write!(f, "MCP tool not found: {name}"),
            Self::ToolExecutionFailed(msg) => write!(f, "MCP tool execution failed: {msg}"),
            Self::InvalidResponse(msg) => write!(f, "Invalid MCP response: {msg}"),
            Self::AlreadyConnected => write!(f, "MCP server already connected"),
            Self::NotConnected => write!(f, "MCP server not connected"),
            Self::UnsupportedTransport(kind) => write!(f, "Unsupported transport: {kind}"),
        }
    }
}

impl std::error::Error for McpError {}

pub type McpResult<T> = Result<T, McpError>;

#[async_trait]
pub trait McpConnection: Send + Sync {
    async fn connect(&self) -> McpResult<()>;
    async fn disconnect(&self) -> McpResult<()>;
    fn is_connected(&self) -> bool;
    fn server_name(&self) -> &str;
    async fn list_tools(&self) -> McpResult<Vec<McpToolDefinition>>;
    async fn call_tool(&self, tool_name: &str, arguments: Value) -> McpResult<Value>;
    fn config(&self) -> &McpServerConfig;

    /// Best-effort cleanup called when the in-flight tool call was cancelled.
    /// Default impl just calls `disconnect()`. Concrete connections may
    /// override to e.g. cancel a pending request id without tearing the
    /// whole subprocess down.
    async fn disconnect_on_cancel(&self) -> McpResult<()> {
        self.disconnect().await
    }
}

pub type SharedMcpConnection = Arc<dyn McpConnection>;

struct StdioProcessIo {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

pub struct StdioMcpConnection {
    config: McpServerConfig,
    runtime_resolver: Option<ManagedRuntimeResolver>,
    managed_runtime_preference: Arc<ManagedRuntimePreference>,
    connected: AtomicBool,
    next_request_id: AtomicU64,
    io: Mutex<Option<StdioProcessIo>>,
    cached_tools: Mutex<Option<Vec<McpToolDefinition>>>,
}

impl StdioMcpConnection {
    pub fn new(config: McpServerConfig, runtime_resolver: Option<ManagedRuntimeResolver>) -> Self {
        Self::new_with_preference(
            config,
            runtime_resolver,
            Arc::new(ManagedRuntimePreference::default()),
        )
    }

    pub fn new_with_preference(
        config: McpServerConfig,
        runtime_resolver: Option<ManagedRuntimeResolver>,
        managed_runtime_preference: Arc<ManagedRuntimePreference>,
    ) -> Self {
        Self {
            config,
            runtime_resolver,
            managed_runtime_preference,
            connected: AtomicBool::new(false),
            next_request_id: AtomicU64::new(1),
            io: Mutex::new(None),
            cached_tools: Mutex::new(None),
        }
    }

    async fn write_message(&self, message: &Value) -> McpResult<()> {
        let payload = serde_json::to_string(message)
            .map_err(|err| McpError::InvalidResponse(err.to_string()))?;
        let mut guard = self.io.lock().await;
        let io = guard.as_mut().ok_or(McpError::NotConnected)?;

        io.stdin
            .write_all(payload.as_bytes())
            .await
            .map_err(|err| McpError::ConnectionFailed(err.to_string()))?;
        io.stdin
            .write_all(b"\n")
            .await
            .map_err(|err| McpError::ConnectionFailed(err.to_string()))?;
        io.stdin
            .flush()
            .await
            .map_err(|err| McpError::ConnectionFailed(err.to_string()))?;
        Ok(())
    }

    async fn send_request(&self, method: &str, params: Value) -> McpResult<Value> {
        if !self.is_connected() && method != "initialize" {
            return Err(McpError::NotConnected);
        }

        let request_id = self.next_request_id.fetch_add(1, Ordering::Relaxed);
        let request = json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "method": method,
            "params": params,
        });

        self.write_message(&request).await?;

        let mut line = String::new();
        let mut guard = self.io.lock().await;
        let io = guard.as_mut().ok_or(McpError::NotConnected)?;
        io.stdout
            .read_line(&mut line)
            .await
            .map_err(|err| McpError::ConnectionFailed(err.to_string()))?;

        if line.trim().is_empty() {
            return Err(McpError::ServerNotResponding);
        }

        let response: Value = serde_json::from_str(&line)
            .map_err(|err| McpError::InvalidResponse(err.to_string()))?;

        if let Some(error) = response.get("error") {
            let message = error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("unknown MCP error");
            return Err(McpError::ToolExecutionFailed(message.to_string()));
        }

        response
            .get("result")
            .cloned()
            .ok_or_else(|| McpError::InvalidResponse("missing result field".to_string()))
    }

    async fn send_notification(&self, method: &str, params: Value) -> McpResult<()> {
        let notification = if params.is_null() || params == json!({}) {
            json!({
                "jsonrpc": "2.0",
                "method": method,
            })
        } else {
            json!({
                "jsonrpc": "2.0",
                "method": method,
                "params": params,
            })
        };
        self.write_message(&notification).await
    }

    fn parse_stdio_command(
        endpoint: &str,
        runtime_resolver: Option<&ManagedRuntimeResolver>,
    ) -> McpResult<(String, Vec<String>)> {
        let raw_parts = split_stdio_command(endpoint)?;
        let mut parts = Vec::with_capacity(raw_parts.len());
        for part in raw_parts {
            parts.push(Self::resolve_stdio_placeholders(&part, runtime_resolver)?);
        }

        if parts.is_empty() {
            return Err(McpError::ConnectionFailed(
                "stdio endpoint command is empty".to_string(),
            ));
        }

        Ok((parts[0].clone(), parts.into_iter().skip(1).collect()))
    }

    fn resolve_stdio_placeholders(
        token: &str,
        runtime_resolver: Option<&ManagedRuntimeResolver>,
    ) -> McpResult<String> {
        let mut resolved = String::with_capacity(token.len());
        let mut rest = token;

        while let Some(start) = rest.find("${renlijia.") {
            resolved.push_str(&rest[..start]);
            let placeholder_start = &rest[start..];
            let Some(end) = placeholder_start.find('}') else {
                return Err(McpError::ConnectionFailed(format!(
                    "invalid MCP stdio runtime placeholder: {placeholder_start}"
                )));
            };
            let placeholder = &placeholder_start[..=end];
            resolved.push_str(&Self::resolve_stdio_placeholder(
                placeholder,
                runtime_resolver,
            )?);
            rest = &placeholder_start[end + 1..];
        }

        resolved.push_str(rest);
        Ok(resolved)
    }

    fn resolve_stdio_placeholder(
        token: &str,
        runtime_resolver: Option<&ManagedRuntimeResolver>,
    ) -> McpResult<String> {
        let Some(name) = token
            .strip_prefix("${renlijia.")
            .and_then(|rest| rest.strip_suffix('}'))
        else {
            return Err(McpError::ConnectionFailed(format!(
                "invalid MCP stdio runtime placeholder: {token}"
            )));
        };

        let resolver = runtime_resolver.ok_or_else(|| {
            McpError::ConnectionFailed(format!(
                "MCP stdio runtime placeholder {token} requires a RuntimeResolver"
            ))
        })?;
        let deps = resolver
            .workspace_dependencies()
            .map_err(|err| McpError::ConnectionFailed(err.to_string()))?;

        Self::dependency_path(name, &deps).ok_or_else(|| {
            McpError::ConnectionFailed(format!("unknown MCP stdio runtime placeholder: {token}"))
        })
    }

    fn dependency_path(name: &str, deps: &WorkspaceDependencies) -> Option<String> {
        let path = match name {
            "python" => &deps.python,
            "node" => &deps.node,
            "npm" => &deps.npm,
            "npx" => &deps.npx,
            "uv" => &deps.uv,
            "uvx" => &deps.uvx,
            _ => return None,
        };
        Some(path.to_string_lossy().into_owned())
    }

    fn env_map(&self) -> HashMap<String, String> {
        self.config.env_vars.clone().unwrap_or_default()
    }

    #[doc(hidden)]
    pub fn resolved_stdio_command_for_test(&self) -> McpResult<(String, Vec<String>)> {
        Self::parse_stdio_command(&self.config.endpoint, self.runtime_resolver.as_ref())
    }
}

fn split_stdio_command(endpoint: &str) -> McpResult<Vec<String>> {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Quote {
        None,
        Single,
        Double,
    }

    let mut parts = Vec::new();
    let mut current = String::new();
    let mut quote = Quote::None;
    let mut chars = endpoint.chars().peekable();

    while let Some(ch) = chars.next() {
        match (quote, ch) {
            (Quote::None, c) if c.is_whitespace() => {
                if !current.is_empty() {
                    parts.push(std::mem::take(&mut current));
                }
            }
            (Quote::None, '\'') => quote = Quote::Single,
            (Quote::None, '"') => quote = Quote::Double,
            (Quote::Single, '\'') => quote = Quote::None,
            (Quote::Double, '"') => quote = Quote::None,
            (Quote::None | Quote::Double, '\\') => {
                if let Some(next) = chars.next() {
                    current.push(next);
                } else {
                    current.push(ch);
                }
            }
            (_, c) => current.push(c),
        }
    }

    if quote != Quote::None {
        return Err(McpError::ConnectionFailed(
            "stdio endpoint command has an unterminated quote".to_string(),
        ));
    }

    if !current.is_empty() {
        parts.push(current);
    }

    Ok(parts)
}

#[async_trait]
impl McpConnection for StdioMcpConnection {
    async fn connect(&self) -> McpResult<()> {
        if self.is_connected() {
            return Ok(());
        }

        let (program, args) =
            Self::parse_stdio_command(&self.config.endpoint, self.runtime_resolver.as_ref())?;
        let mut command = Command::new(&program);
        command
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        crate::storage::process_ext::NoWindowExt::no_window(&mut command);

        if self.managed_runtime_preference.is_enabled() {
            if let Some(resolver) = self.runtime_resolver.as_ref() {
                let env = ManagedRuntimeProcessEnv::from_resolver(resolver.as_ref())
                    .map_err(|err| McpError::ConnectionFailed(err.to_string()))?;
                log::debug!(
                    "[mcp] injecting managed runtime env for MCP server command: {program}"
                );
                env.apply_to_tokio_command(&mut command);
            }
        }

        // Force UTF-8 stdio for MCP children. Windows zh-CN consoles default to
        // CP936; many MCP servers (Python, Node) honor these env vars and emit
        // UTF-8 to their piped stdout, which our line-framed JSON-RPC reader
        // requires. User env_map below can still override if needed.
        command.env("PYTHONIOENCODING", "utf-8");
        command.env("PYTHONUTF8", "1");
        if std::env::var_os("LANG").is_none() {
            command.env("LANG", "en_US.UTF-8");
        }

        for (key, value) in self.env_map() {
            command.env(key, value);
        }

        let mut child = command
            .spawn()
            .map_err(|err| McpError::ConnectionFailed(err.to_string()))?;

        let stdin = child.stdin.take().ok_or_else(|| {
            McpError::ConnectionFailed("failed to capture MCP server stdin".to_string())
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            McpError::ConnectionFailed("failed to capture MCP server stdout".to_string())
        })?;

        {
            let mut guard = self.io.lock().await;
            *guard = Some(StdioProcessIo {
                child,
                stdin,
                stdout: BufReader::new(stdout),
            });
        }

        let initialize_result = self
            .send_request(
                "initialize",
                json!({
                    "protocolVersion": "2025-11-25",
                    "capabilities": {},
                    "clientInfo": {
                        "name": "aijia",
                        "version": "0.4.1"
                    }
                }),
            )
            .await?;

        if initialize_result.get("serverInfo").is_none() {
            return Err(McpError::InvalidResponse(
                "initialize result missing serverInfo".to_string(),
            ));
        }

        self.connected.store(true, Ordering::Relaxed);

        self.send_notification("notifications/initialized", Value::Null)
            .await?;

        Ok(())
    }

    async fn disconnect(&self) -> McpResult<()> {
        let mut guard = self.io.lock().await;
        if let Some(mut io) = guard.take() {
            let _ = io.stdin.shutdown().await;
            let _ = io.child.kill().await;
            let _ = io.child.wait().await;
        }
        self.connected.store(false, Ordering::Relaxed);
        *self.cached_tools.lock().await = None;
        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }

    fn server_name(&self) -> &str {
        &self.config.name
    }

    async fn list_tools(&self) -> McpResult<Vec<McpToolDefinition>> {
        if let Some(cached) = self.cached_tools.lock().await.clone() {
            return Ok(cached);
        }

        let result = self.send_request("tools/list", json!({})).await?;
        let tools = result
            .get("tools")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                McpError::InvalidResponse("tools/list missing tools array".to_string())
            })?;

        let parsed_tools: Vec<McpToolDefinition> = tools
            .iter()
            .map(|tool| McpToolDefinition {
                server_name: self.config.name.clone(),
                tool_name: tool
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                description: tool
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                input_schema: tool
                    .get("inputSchema")
                    .cloned()
                    .unwrap_or_else(|| json!({})),
            })
            .collect();

        *self.cached_tools.lock().await = Some(parsed_tools.clone());
        Ok(parsed_tools)
    }

    async fn call_tool(&self, tool_name: &str, arguments: Value) -> McpResult<Value> {
        let result = self
            .send_request(
                "tools/call",
                json!({
                    "name": tool_name,
                    "arguments": arguments,
                }),
            )
            .await?;

        Ok(result)
    }

    fn config(&self) -> &McpServerConfig {
        &self.config
    }
}

pub struct UnsupportedMcpConnection {
    config: McpServerConfig,
}

impl UnsupportedMcpConnection {
    pub fn new(config: McpServerConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl McpConnection for UnsupportedMcpConnection {
    async fn connect(&self) -> McpResult<()> {
        Err(McpError::UnsupportedTransport(
            self.config.transport_type.clone(),
        ))
    }

    async fn disconnect(&self) -> McpResult<()> {
        Ok(())
    }

    fn is_connected(&self) -> bool {
        false
    }

    fn server_name(&self) -> &str {
        &self.config.name
    }

    async fn list_tools(&self) -> McpResult<Vec<McpToolDefinition>> {
        Err(McpError::UnsupportedTransport(
            self.config.transport_type.clone(),
        ))
    }

    async fn call_tool(&self, _tool_name: &str, _arguments: Value) -> McpResult<Value> {
        Err(McpError::UnsupportedTransport(
            self.config.transport_type.clone(),
        ))
    }

    fn config(&self) -> &McpServerConfig {
        &self.config
    }
}

pub fn build_mcp_connection(
    config: &McpServerConfig,
    runtime_resolver: Option<ManagedRuntimeResolver>,
) -> McpResult<SharedMcpConnection> {
    build_mcp_connection_with_preference(
        config,
        runtime_resolver,
        Arc::new(ManagedRuntimePreference::default()),
    )
}

pub fn build_mcp_connection_with_preference(
    config: &McpServerConfig,
    runtime_resolver: Option<ManagedRuntimeResolver>,
    managed_runtime_preference: Arc<ManagedRuntimePreference>,
) -> McpResult<SharedMcpConnection> {
    match config.transport_type.as_str() {
        "stdio" => Ok(Arc::new(StdioMcpConnection::new_with_preference(
            config.clone(),
            runtime_resolver,
            managed_runtime_preference,
        ))),
        _ => Ok(Arc::new(UnsupportedMcpConnection::new(config.clone()))),
    }
}
