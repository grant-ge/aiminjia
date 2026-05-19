# rules.md — 数字员工 意图测试规格

## 测试范围

覆盖数字员工的完整生命周期：从模板雇佣、资源/技能配置、派活前置补全（trigger prechecks）、dispatch prompt 生成与执行、cron 定时自动触发、运行态守卫（ActiveRunGuard），到状态机迁移（Active / Paused / Archived / Running）与软删除清理。关注后端 `runtime/employee/` 与 Tauri 命令 `commands/employees.rs`、以及前端 `src/features/employees/` 的雇佣向导和派活入口在端到端链路上的正确性。

## 待覆盖的主要场景

- 场景 1：用户从模板雇佣员工（HireWizard），员工目录写入快照 `template/template.json` 并在 `EmployeeStore` 中可见
- 场景 2：派活前置检查 (`runTriggerPrechecks`) 按模板 `requiresAttachment / resourceConfigKind / requiresDingtalk` 弹出对应配置表单，未配置完整时阻断派活
- 场景 3：派活成功后 `build_dispatch_prompt` 拼出包含资源信息且以"请立即开始按职责执行"结尾的 prompt，并真正驱动一次 chat turn
- 场景 4：cron 表达式到点后 scheduler 自动派活，运行态在 `EmployeeActiveRuns` 中可见并通过事件下发到前端
- 场景 5：用户手动暂停 / 恢复员工（lifecycle Active ↔ Paused），cron tick 在 Paused 状态下不再触发
- 场景 6：用户归档员工（lifecycle Archived），过 7 天后由 `purge_old_archived` 自动物理清理目录
- 场景 7：员工正在 Running 时被取消（用户停止 / panic / 提前 return），`ActiveRunGuard` 在 Drop 时正确清理活跃运行记录，不泄漏
- 场景 8：snapshot-first 读取——派活时 `effective_tool_whitelist / effective_system_prompt_extra / effective_default_skill_id` 优先读 `template/template.json`，缺失时回退到 record 字段

## 待补充

> 具体意图（场景/前提/操作/验收标准）待补全。
