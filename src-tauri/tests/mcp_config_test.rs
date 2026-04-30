use std::collections::HashMap;

use app_lib::runtime::mcp::McpServerConfig;
use tempfile::TempDir;

#[test]
fn mcp_config_store_add_load_remove_roundtrip() {
    let temp_dir = TempDir::new().expect("tempdir");
    let store = app_lib::storage::mcp_config_store::McpConfigStore::new(
        temp_dir.path().join("mcp_servers.json"),
    );

    let first = McpServerConfig {
        name: "demo".to_string(),
        transport_type: "stdio".to_string(),
        endpoint: "/usr/local/bin/demo".to_string(),
        env_vars: Some(HashMap::from([(
            "API_KEY".to_string(),
            "secret".to_string(),
        )])),
    };
    let second = McpServerConfig {
        name: "metrics".to_string(),
        transport_type: "http".to_string(),
        endpoint: "http://localhost:3000/mcp".to_string(),
        env_vars: None,
    };

    store.add(first.clone()).expect("add first config");
    store.add(second.clone()).expect("add second config");

    let loaded = store.load().expect("load configs");
    assert_eq!(loaded, vec![first.clone(), second.clone()]);

    store.remove("demo").expect("remove config");
    let loaded = store.load().expect("reload configs");
    assert_eq!(loaded, vec![second]);
}

#[test]
fn mcp_config_store_rejects_duplicate_names() {
    let temp_dir = TempDir::new().expect("tempdir");
    let store = app_lib::storage::mcp_config_store::McpConfigStore::new(
        temp_dir.path().join("mcp_servers.json"),
    );

    let config = McpServerConfig {
        name: "demo".to_string(),
        transport_type: "stdio".to_string(),
        endpoint: "/usr/local/bin/demo".to_string(),
        env_vars: None,
    };

    store.add(config.clone()).expect("initial add");
    let err = store.add(config).expect_err("duplicate add should fail");
    assert!(
        err.contains("already exists"),
        "expected duplicate name error, got: {err}"
    );
}

#[test]
fn mcp_config_store_roundtrips_env_var_shapes() {
    let temp_dir = TempDir::new().expect("tempdir");
    let store = app_lib::storage::mcp_config_store::McpConfigStore::new(
        temp_dir.path().join("mcp_servers.json"),
    );

    store
        .save(&[
            McpServerConfig {
                name: "none-env".to_string(),
                transport_type: "sse".to_string(),
                endpoint: "http://localhost:4000/sse".to_string(),
                env_vars: None,
            },
            McpServerConfig {
                name: "with-env".to_string(),
                transport_type: "stdio".to_string(),
                endpoint: "/bin/echo".to_string(),
                env_vars: Some(HashMap::from([
                    ("FOO".to_string(), "bar".to_string()),
                    ("COMPLEX".to_string(), "a=b=c".to_string()),
                ])),
            },
        ])
        .expect("save configs");

    let loaded = store.load().expect("load configs");
    assert_eq!(loaded.len(), 2);
    assert!(loaded[0].env_vars.is_none());
    assert_eq!(
        loaded[1]
            .env_vars
            .as_ref()
            .and_then(|vars: &HashMap<String, String>| vars.get("COMPLEX")),
        Some(&"a=b=c".to_string())
    );
}
