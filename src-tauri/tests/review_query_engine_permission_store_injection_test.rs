//! PermissionStore 的 owner 已迁到 SessionRuntime；
//! 该回归测试只负责守住当前注入入口不被误删。

use std::sync::Arc;

use app_lib::runtime::event_bus::RuntimeEventBus;
use app_lib::runtime::query_engine::QueryEngine;
use app_lib::runtime::session_runtime::SessionRuntime;
use app_lib::runtime::store::PermissionStore;

#[test]
fn review_session_runtime_accepts_permission_store() {
    let store = Arc::new(PermissionStore::in_memory());
    let _runtime = SessionRuntime::new(QueryEngine::new(), RuntimeEventBus::new())
        .with_permission_store(store);
}
