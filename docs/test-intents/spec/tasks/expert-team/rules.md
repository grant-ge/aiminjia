# rules.md — 专家团队 意图测试规格

## 测试范围

覆盖专家团队（Team）这一多 agent 协作单元的完整链路：从团队创建、Teammate 注册（markdown loader 加载子 agent 定义）、Lead idle loop 调度，到 Lead 通过 `task_notification` 分发任务给 Teammate、Teammate 完成后回写 envelope、团队级取消传播（用户停止 Lead 时所有 Teammate 同步停止）、以及团队对话历史/transcript 的持久化与隔离。关注 `runtime/agent/team*`、`tools/builtin/teammate_stop.rs`、`lead_idle.rs`、`cancellation_registry.rs` 在多 agent 场景下的正确性。

## 待覆盖的主���场景

- 场景 1：用户创建专家团队并添加 Teammate（markdown 文件落到 team 目录），Lead 启动后 `name_registry` 中可见所有 Teammate
- 场景 2：Lead 接到任务后通过 `task_notification_lead` 给 Teammate 派单，Teammate idle loop 唤醒并独立执行
- 场景 3：Teammate 完成任务后通过 `subagent_result_envelope` 回写结果，Lead 在下一轮 turn 看到 Teammate 的输出
- 场景 4：用户在团队对话中点"停止"，`cancellation_registry` 触发，Lead 和所有正在 Running 的 Teammate 同时收到取消并退出，不出现僵尸子 agent
- 场景 5：Teammate 主动调用 `teammate_stop` 工具自行下线，Lead 的 idle loop 感知到 Teammate 状态变化
- 场景 6：团队对话历史按 team scope 隔离持久化（`team_paths`），跨团队不串消息；`subagent_transcript_store` 保留每个 Teammate 的完整转录
- 场景 7：多 Teammate 并发执行时 `tool_round_concurrency` 控制范围内不互相阻塞，且事件 run_id 各自独立

## 待补充

> 具体意图（场景/前提/操作/验收标准）待补全。
