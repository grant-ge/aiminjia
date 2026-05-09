// TODO(coordinator-not-implemented): 这是一个占位 stub，未实现。
//
// Claude Code 的 coordinatorMode 需要：
//   1. 多 worker 并发调度入口（dispatch_workers）
//   2. 综合层 prompt（汇总 worker 输出 → 给主对话一个最终答案）
//   3. worker prompt 自包含上下文（worker 不能依赖 coordinator 的对话历史）
//
// 落地路径见:
//   - docs/superpowers/specs/2026-04-20-subagent-alignment-design.md
//   - claude-code-best/src/coordinator/coordinatorMode.ts
//
// 暂不实施原因：当前没有真实业务在等多 worker 调度。
// 何时实施：当出现"一个任务需要多个独立子代理并行 + 综合"的真实需求时。

use crate::runtime::ids::AgentId;

#[derive(Clone, Debug)]
pub struct TeamContext {
    pub team_id: String,
    pub agent_ids: Vec<AgentId>,
}
