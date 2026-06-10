use crate::runtime::human_interaction::{
    PermissionDecisionIntent, PermissionGroup, PermissionGroupKey, PermissionGroupResolution,
};
use crate::runtime::ids::{RunId, SessionId, ToolCallId};

#[test]
fn same_run_same_directory_groups_together() {
    let key = PermissionGroupKey::read_path(
        SessionId::new("sess-1"),
        RunId::new("run-1"),
        "/private/tmp/aijia-permission-test/secret1.txt",
    );
    let mut group = PermissionGroup::new(key);

    group.push_request(
        ToolCallId::new("tool-1"),
        "/private/tmp/aijia-permission-test/secret1.txt",
    );
    group.push_request(
        ToolCallId::new("tool-2"),
        "/private/tmp/aijia-permission-test/secret2.txt",
    );

    assert_eq!(group.items().len(), 2);
    assert_eq!(
        group.coverage_scope(),
        Some("/private/tmp/aijia-permission-test".to_string())
    );
}

#[test]
fn allow_always_scope_must_cover_every_item_before_batch_resolve() {
    let key = PermissionGroupKey::read_path(
        SessionId::new("sess-1"),
        RunId::new("run-1"),
        "/private/tmp/aijia-permission-test/secret1.txt",
    );
    let mut group = PermissionGroup::new(key);
    group.push_request(
        ToolCallId::new("tool-1"),
        "/private/tmp/aijia-permission-test/secret1.txt",
    );
    group.push_request(
        ToolCallId::new("tool-2"),
        "/private/tmp/aijia-permission-test/secret2.txt",
    );

    let result = group.resolve(PermissionDecisionIntent::AllowAlways {
        scope: Some("/private/tmp/aijia-permission-test".into()),
    });

    assert_eq!(result, PermissionGroupResolution::ResolveAll);
}

#[test]
fn too_narrow_scope_does_not_batch_resolve() {
    let key = PermissionGroupKey::read_path(
        SessionId::new("sess-1"),
        RunId::new("run-1"),
        "/private/tmp/aijia-permission-test/secret1.txt",
    );
    let mut group = PermissionGroup::new(key);
    group.push_request(
        ToolCallId::new("tool-1"),
        "/private/tmp/aijia-permission-test/secret1.txt",
    );
    group.push_request(
        ToolCallId::new("tool-2"),
        "/private/tmp/aijia-permission-test/secret2.txt",
    );

    let result = group.resolve(PermissionDecisionIntent::AllowAlways {
        scope: Some("/private/tmp/aijia-permission-test/secret1.txt".into()),
    });

    assert_eq!(
        result,
        PermissionGroupResolution::NeedClarification {
            message:
                "授权范围没有覆盖全部待审批请求，请选择仅本次、拒绝，或说明包含全部文件的目录范围。"
                    .into()
        }
    );
}
