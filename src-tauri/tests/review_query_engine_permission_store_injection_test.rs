//! QueryEngine 必须能接受并向 ToolExecutionContext 注入 PermissionStore。

use std::sync::Arc;

use app_lib::runtime::query_engine::QueryEngine;
use app_lib::runtime::store::PermissionStore;

#[test]
fn review_query_engine_accepts_permission_store() {
    let store = Arc::new(PermissionStore::in_memory());
    // 如果 QueryEngine 没有 with_permission_store 方法，编译失败
    let _engine = QueryEngine::new().with_permission_store(store);
}
