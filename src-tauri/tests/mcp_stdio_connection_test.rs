use std::fs;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

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

#[tokio::test]
async fn stdio_connection_performs_initialize_list_and_call() {
    let script_path = write_fixture_server();
    let connection = Arc::new(StdioMcpConnection::new(McpServerConfig {
        name: "fixture".to_string(),
        transport_type: "stdio".to_string(),
        endpoint: format!("python3 {}", script_path.display()),
        env_vars: None,
    }));

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
    let connection = build_mcp_connection(&McpServerConfig {
        name: "unsupported".to_string(),
        transport_type: "http".to_string(),
        endpoint: "http://localhost:3000/mcp".to_string(),
        env_vars: None,
    })
    .expect("factory should still build an unsupported placeholder connection");

    let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
    let err = runtime
        .block_on(async { connection.connect().await })
        .expect_err("unsupported transport should fail on connect");

    assert!(matches!(err, McpError::UnsupportedTransport(kind) if kind == "http"));
}
