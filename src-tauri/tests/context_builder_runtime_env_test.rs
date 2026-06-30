use app_lib::runtime::chat::context_builder::{build_env_info, ManagedRuntimeEnvInfo};

#[tokio::test]
async fn env_info_teaches_managed_runtime_default_and_system_escape() {
    let workspace_path = std::path::PathBuf::from("/tmp/test-workspace");
    let runtime_info = ManagedRuntimeEnvInfo {
        runtime_root: "/cache/aijia/current".into(),
        python_path: "/cache/aijia/python/bin/python3".into(),
        node_path: "/cache/aijia/node/bin/node".into(),
        npm_path: "/cache/aijia/node/bin/npm".into(),
        npx_path: "/cache/aijia/node/bin/npx".into(),
        uv_path: "/cache/aijia/uv/bin/uv".into(),
        uvx_path: "/cache/aijia/uv/bin/uvx".into(),
    };

    let result = build_env_info(&workspace_path, None, Some(&runtime_info), true).await;

    assert!(result.contains("系统环境检测（未注入 AIjia 自带 Runtime）"));
    assert!(result.contains("AIjia 自带运行环境：已开启（默认优先）"));
    assert!(result.contains("直接使用裸 `node`"));
    assert!(result.contains("工具没有 `runtime_env` 参数"));
    assert!(result.contains("使用上方“系统环境检测”里的系统绝对路径"));
    assert!(result.contains("npm install -g <包名> --silent"));
    assert!(result.contains(r#"node -e "require('<包名>'); console.log('ok')""#));
    assert!(result.contains("禁止使用 --system"));
    assert!(!result.contains("默认使用上面列出的绝对路径"));
    assert!(!result.contains("必须带 NODE_PATH"));
}

#[tokio::test]
async fn env_info_when_switch_off_uses_system_environment_wording() {
    let workspace_path = std::path::PathBuf::from("/tmp/test-workspace");
    let runtime_info = ManagedRuntimeEnvInfo {
        runtime_root: "/cache/aijia/current".into(),
        python_path: "/cache/aijia/python/bin/python3".into(),
        node_path: "/cache/aijia/node/bin/node".into(),
        npm_path: "/cache/aijia/node/bin/npm".into(),
        npx_path: "/cache/aijia/node/bin/npx".into(),
        uv_path: "/cache/aijia/uv/bin/uv".into(),
        uvx_path: "/cache/aijia/uv/bin/uvx".into(),
    };

    let result = build_env_info(&workspace_path, None, Some(&runtime_info), false).await;

    assert!(result.contains("系统环境检测（未注入 AIjia 自带 Runtime）"));
    assert!(result.contains("AIjia 自带运行环境：已关闭（默认使用系统环境）"));
    assert!(result.contains("不要假设 AIjia 自带环境已注入"));
    assert!(result.contains("工具没有 `runtime_env` 参数"));
    assert!(!result.contains("Runtime 当前目录: /cache/aijia/current"));
    assert!(!result.contains("AIjia 自带运行环境：已开启"));
}
