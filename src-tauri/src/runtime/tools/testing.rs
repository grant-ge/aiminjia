use std::sync::Arc;

use crate::runtime::tools::{AllowAllPermissionPipeline, LegacyToolAdapter, ToolDispatcher};

pub fn single_legacy_tool_dispatcher(tool_name: &str) -> Arc<ToolDispatcher> {
    let dispatcher = Arc::new(ToolDispatcher::new(Arc::new(AllowAllPermissionPipeline)));
    dispatcher.register(Arc::new(LegacyToolAdapter::for_test(tool_name)));
    dispatcher
}
