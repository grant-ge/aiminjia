use std::fs;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use app_lib::runtime::dependencies::StaticRuntimeResolver;
use app_lib::runtime::mcp::{
    build_mcp_connection, McpConnection, McpError, McpServerConfig, StdioMcpConnection,
};
use serde_json::json;

fn unique_temp_path(prefix: &str, ext: &str) -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time before unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("{prefix}-{nanos}.{ext}"))
}

fn write_fixture_server() -> std::path::PathBuf {
    let script_path = unique_temp_path("lotus-mcp-fixture", "py");
    let script = r#"
import json
import sys

for raw in sys.stdin:
    raw = raw.strip()
    if not raw:
        continue
    message = json.loads(raw)
    method = message.get("method")
    if method == "initialize":
        response = {
            "jsonrpc": "2.0",
            "id": message["id"],
            "result": {
                "protocolVersion": "2024-11-05",
                "capabilities": {"tools": {"listChanged": False}},
                "serverInfo": {"name": "fixture", "version": "1.0.0"},
            },
        }
    elif method == "notifications/initialized":
        continue
    elif method == "tools/list":
        response = {
            "jsonrpc": "2.0",
            "id": message["id"],
            "result": {
                "tools": [
                    {
                        "name": "echo",
                        "description": "Echo input",
                        "inputSchema": {
                            "type": "object",
                            "properties": {"message": {"type": "string"}},
                            "required": ["message"],
                        },
                    }
                ]
            },
        }
    elif method == "tools/call":
        response = {
            "jsonrpc": "2.0",
            "id": message["id"],
            "result": {
                "content": [
                    {
                        "type": "text",
                        "text": "echo:" + message["params"]["arguments"]["message"],
                    }
                ],
                "isError": False,
            },
        }
    else:
        response = {
            "jsonrpc": "2.0",
            "id": message.get("id"),
            "error": {"code": -32601, "message": "method not found"},
        }

    sys.stdout.write(json.dumps(response) + "\n")
    sys.stdout.flush()
"#;
    fs::write(&script_path, script).expect("write MCP fixture server");
    script_path
}

fn managed_runtime_resolver() -> Arc<dyn app_lib::runtime::dependencies::RuntimeResolver> {
    Arc::new(StaticRuntimeResolver::new(
        "/tmp/renlijia/python/bin/python3".into(),
        "/tmp/renlijia/node/bin/node".into(),
        "/tmp/renlijia/node/bin/npm".into(),
        "/tmp/renlijia/node/bin/npx".into(),
        "/tmp/renlijia/uv/bin/uv".into(),
        "/tmp/renlijia/uv/bin/uvx".into(),
        "/tmp/renlijia/node/node_modules".into(),
        "/tmp/renlijia/python/site-packages".into(),
    ))
}

#[tokio::test]
async fn stdio_connection_performs_initialize_list_and_call() {
    let script_path = write_fixture_server();
    let connection = Arc::new(StdioMcpConnection::new(
        McpServerConfig {
            name: "fixture".to_string(),
            transport_type: "stdio".to_string(),
            endpoint: format!("python3 {}", script_path.display()),
            env_vars: None,
        },
        None,
    ));

    connection.connect().await.unwrap();
    assert!(connection.is_connected());

    let tools = connection.list_tools().await.unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].qualified_name(), "mcp__fixture__echo");

    let result = connection
        .call_tool("echo", json!({ "message": "hello" }))
        .await
        .unwrap();

    assert_eq!(result["content"][0]["text"], "echo:hello");

    connection.disconnect().await.unwrap();
    assert!(!connection.is_connected());

    let _ = fs::remove_file(script_path);
}

#[test]
fn connection_factory_rejects_unsupported_transport() {
    let connection = build_mcp_connection(
        &McpServerConfig {
            name: "unsupported".to_string(),
            transport_type: "http".to_string(),
            endpoint: "http://localhost:3000/mcp".to_string(),
            env_vars: None,
        },
        None,
    )
    .expect("factory should still build an unsupported placeholder connection");

    let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
    let err = runtime
        .block_on(async { connection.connect().await })
        .expect_err("unsupported transport should fail on connect");

    assert!(matches!(err, McpError::UnsupportedTransport(kind) if kind == "http"));
}

#[test]
fn stdio_connection_resolves_renlijia_runtime_placeholders_before_spawn() {
    let connection = StdioMcpConnection::new(
        McpServerConfig {
            name: "node-server".to_string(),
            transport_type: "stdio".to_string(),
            endpoint: "${renlijia.node} server.js --runner ${renlijia.npx}".to_string(),
            env_vars: None,
        },
        Some(managed_runtime_resolver()),
    );

    let (program, args) = connection
        .resolved_stdio_command_for_test()
        .expect("placeholder command should resolve");

    assert_eq!(program, "/tmp/renlijia/node/bin/node");
    assert_eq!(
        args,
        vec![
            "server.js".to_string(),
            "--runner".to_string(),
            "/tmp/renlijia/node/bin/npx".to_string(),
        ]
    );
}

#[test]
fn stdio_connection_preserves_quoted_arguments_and_embedded_placeholders() {
    let connection = StdioMcpConnection::new(
        McpServerConfig {
            name: "quoted-node-server".to_string(),
            transport_type: "stdio".to_string(),
            endpoint: r#"${renlijia.node} "server path.js" --runner=${renlijia.npx} '--json={"mode":"safe"}'"#.to_string(),
            env_vars: None,
        },
        Some(managed_runtime_resolver()),
    );

    let (program, args) = connection
        .resolved_stdio_command_for_test()
        .expect("quoted command should parse and resolve");

    assert_eq!(program, "/tmp/renlijia/node/bin/node");
    assert_eq!(
        args,
        vec![
            "server path.js".to_string(),
            "--runner=/tmp/renlijia/node/bin/npx".to_string(),
            "--json={\"mode\":\"safe\"}".to_string(),
        ]
    );
}

#[test]
fn stdio_connection_rejects_runtime_placeholder_without_resolver() {
    let connection = StdioMcpConnection::new(
        McpServerConfig {
            name: "node-server".to_string(),
            transport_type: "stdio".to_string(),
            endpoint: "${renlijia.node} server.js".to_string(),
            env_vars: None,
        },
        None,
    );

    let message = connection
        .resolved_stdio_command_for_test()
        .expect_err("placeholder without resolver should fail")
        .to_string();

    assert!(message.contains("requires a RuntimeResolver"));
}

#[test]
fn stdio_connection_rejects_unknown_runtime_placeholder() {
    let connection = StdioMcpConnection::new(
        McpServerConfig {
            name: "ruby-server".to_string(),
            transport_type: "stdio".to_string(),
            endpoint: "${renlijia.ruby} server.rb".to_string(),
            env_vars: None,
        },
        Some(managed_runtime_resolver()),
    );

    let message = connection
        .resolved_stdio_command_for_test()
        .expect_err("unknown placeholder should fail")
        .to_string();

    assert!(message.contains("unknown MCP stdio runtime placeholder"));
}
