use std::path::PathBuf;

use app_lib::runtime::chat::context_builder::build_env_info;

#[tokio::test]
async fn build_env_info_is_async_and_preserves_output_contract() {
    let workspace_path = PathBuf::from("/tmp/test-workspace");
    let result = build_env_info(&workspace_path, None, None, true).await;

    assert!(result.contains("[当前环境]"));
    assert!(result.contains("工作目录: /tmp/test-workspace"));
    assert!(result.contains("Platform:"));
    assert!(result.contains("系统环境检测（未注入 AIjia 托管运行时）"));
}
