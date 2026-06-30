# Decision Index

## Current Decision Sources

| 文档 | 作用 |
|---|---|
| `docs/architecture-blueprint.md` | Runtime-first 总蓝图，解释从 Tauri command God function 迁到 Session/Query Runtime 的方向 |
| `docs/decisions/runtime-decisions.md` | Runtime、LLM gateway、token/cost、managed runtime、auth 等稳定决策 |
| `docs/decisions/ui-platform-decisions.md` | 租户换肤、标题栏、拖拽上传、剪贴板、旧 WebKit、Windows 子进程等 UI/platform 决策 |
| `docs/decisions/employee-system-decisions.md` | 数字员工模板、SKILL bundle、remote catalog、snapshot-first 和派活前置规则 |
| `docs/release-playbook.md` | 发布流程权威文档 |
| `docs/test-intents/README.md` | AEIT 意图测试入口 |
| `docs/test-intents/cli-gap.md` | tauri-pilot `aijia` 子命令缺口与规则漂移记录 |
| `docs/runtime-manager.md` | RuntimeManager 目录、resolver、安装和诊断说明 |

## Decision-To-Code Map

| 决策域 | 关键代码 |
|---|---|
| Runtime Session/Query/Turn | `src-tauri/src/runtime/session_runtime.rs`, `src-tauri/src/runtime/query_engine.rs`, `src-tauri/src/runtime/chat/chat_turn_driver.rs` |
| Runtime events | `src-tauri/src/runtime/events.rs`, `src-tauri/src/transport/tauri_event_adapter.rs`, `src-tauri/tests/review_tauri_event_adapter_test.rs` |
| Tool runtime | `src-tauri/src/runtime/tools/catalog.rs`, `src-tauri/src/runtime/tools/dispatcher.rs`, `src-tauri/src/runtime/tools/permission.rs` |
| MCP | `src-tauri/src/runtime/mcp/*`, `src-tauri/src/plugin/registry.rs` |
| LLM gateway | `src-tauri/src/llm/gateway.rs`, `src-tauri/src/llm/router.rs`, `src-tauri/src/llm/providers/*`, `src-tauri/src/llm/streaming.rs` |
| Workspace/path auth | `src-tauri/src/storage/workspace.rs`, `src-tauri/src/runtime/path_auth/decide.rs`, `src-tauri/src/commands/file.rs`, `src-tauri/src/commands/workspace.rs` |
| Managed runtime | `src-tauri/src/runtime/dependencies/*`, `src/components/settings/panels/RuntimePanel.tsx` |
| Employee system | `src-tauri/src/runtime/employee/*`, `src-tauri/src/commands/employees.rs`, `src/features/employees/*` |
| Skill/pending UI | `src/features/skill-center/*`, `src/stores/skillStore.ts`, `src/hooks/usePendingEventListener.ts`, `src/stores/pendingStore.ts` |

## Archive Rule

`docs/archive/**`、dated gap analyses、handoffs、old plans 和 run reports 只能作为历史背景。除非当前文档或源码仍明确引用，否则不要把它们写成当前架构事实。
