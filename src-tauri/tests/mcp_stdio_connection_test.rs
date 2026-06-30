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
    let root = std::env::temp_dir().join("renlijia-mcp-runtime");
    Arc::new(StaticRuntimeResolver::new(
        root.join("python").join("bin").join("python3"),
        root.join("node").join("bin").join("node"),
        root.join("node").join("bin").join("npm"),
        root.join("node").join("bin").join("npx"),
        root.join("uv").join("bin").join("uv"),
        root.join("uv").join("bin").join("uvx"),
        root.join("node").join("lib").join("node_modules"),
        root.join("python").join("site-packages"),
    ))
}

fn managed_runtime_root() -> std::path::PathBuf {
    std::env::temp_dir().join("renlijia-mcp-runtime")
}

#[tokio::test]
async fn stdio_connection_performs_initialize_list_and_call() {
    let script_path = write_fixture_server();
    let python_cmd = if cfg!(windows) { "python" } else { "python3" };
    let script_arg = script_path.to_string_lossy().replace('\\', "/");
    let connection = Arc::new(StdioMcpConnection::new(
        McpServerConfig {
            name: "fixture".to_string(),
            transport_type: "stdio".to_string(),
            endpoint: format!(r#"{python_cmd} "{script_arg}""#),
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

    let root = managed_runtime_root();
    assert_eq!(
        program,
        root.join("node")
            .join("bin")
            .join("node")
            .to_string_lossy()
            .into_owned()
    );
    assert_eq!(
        args,
        vec![
            "server.js".to_string(),
            "--runner".to_string(),
            root.join("node")
                .join("bin")
                .join("npx")
                .to_string_lossy()
                .into_owned(),
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

    let root = managed_runtime_root();
    assert_eq!(
        program,
        root.join("node")
            .join("bin")
            .join("node")
            .to_string_lossy()
            .into_owned()
    );
    assert_eq!(
        args,
        vec![
            "server path.js".to_string(),
            format!(
                "--runner={}",
                root.join("node").join("bin").join("npx").to_string_lossy()
            ),
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
