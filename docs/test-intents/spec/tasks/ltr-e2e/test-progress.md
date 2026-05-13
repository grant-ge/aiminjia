# LTR 端到端冒烟执行进度（P2.11）

> 与 rules.md 配套。每条意图对应一行,记录 PASS / FAIL / BLOCKED + 简短证据。
> 待手测完成后由测试者填充。

---

## 执行环境

- 日期: _待填_
- 测试者: _待填_
- LLM endpoint: _待填(建议 lotus/Doubao 或 deepseek-v4)_
- 分支: `ltr-mvp` @ commit _待填_
- 启动命令: `pnpm tauri:dev`
- EmployeeRecord fixture(小研):
  - tool_whitelist 至少含: `SendMessage`, `TaskList`, `TaskGet`, `TaskClaim`, `TaskUpdate`, `WebSearch`(场景 A 需要)。
  - system_prompt_extra: _待填(简短角色描述,例 "你是研究助理,擅长技术调研")_。

---

## 场景 A — 单 Teammate 调研

| 断言 | 状态 | 证据 / 笔记 |
|---|---|---|
| A1 team.json 含 1 Lead + 1 Teammate | _待测_ | |
| A2 transcript 首条 user 含 system-reminder + team-lead | _待测_ | 见 L2 限制,可能 BLOCKED |
| A3 .meta.json kind=teammate 且 boot_system_prompt 含 "Teammate 身份" | _待测_ | |
| A4 transcript 出现 SendMessage(to=team-lead, text) | _待测_ | |
| A5 Lead 汇总含调研产出关键字 | _待测_ | 见 L1/L3,可能需用户手动发空消息触发 Lead 续 turn |
| A6 Lead/Teammate 异步并发 | _待测_ | |
| A-N1 Lead 不亲自 WebSearch | _待测_ | |
| A-N2 Teammate 无 Ask | _待测_ | |
| A-N3 Teammate 无用 SendMessage 报进度 | _待测_ | |

---

## 场景 B — Multi-Teammate Swarm

| 断言 | 状态 | 证据 |
|---|---|---|
| B1 teammates == 3 | _待测_ | |
| B2 三份 transcript 各非空 | _待测_ | |
| B3 至少 1 条 Teammate↔Teammate ChatMessage | _待测_ | |
| B4 Lead 收到 task-notification XML | _待测_ | |
| B5 Lead 汇总含 3 sub-task | _待测_ | |
| B-N1 Lead 不接管 Teammate task | _待测_ | |
| B-N2 同 task 不重复 Claim | _待测_ | P1.5 应保证 |
| B-N3 无 Ask | _待测_ | |

---

## 场景 C — Plan Approval + Shutdown

| 断言 | 状态 | 证据 |
|---|---|---|
| C1 transcript 出现 `<plan-approval-request id="pa-1">` | _待测_ | |
| C2 双方 transcript request_id 匹配 | _待测_ | |
| C3 收到 shutdown_request 后未立即退出 | _待测_ | P2.6 行为 |
| C4 transcript 出现 `<shutdown-request reason="...">` | _待测_ | |
| C5 cleanup 后 team.json 无 worker 残留 | _待测_ | |
| C6 三 registry 无 worker 残留 | _待测_ | |
| C-N1 不立即 cleanup | _待测_ | |
| C-N2 plan_approval 不走 permission Ask | _待测_ | |
| C-N3 child cancel 不带挂 Lead | _待测_ | |

---

## 通用项

| 断言 | 状态 | 证据 |
|---|---|---|
| O1 cancel_session/app-close 后 registries 清空 | _待测_ | |
| O2 SendMessage 触发前端 tool:executing/completed 事件 | _待测_ | |
| O3 60s heartbeat 更新 last_active_at | _待测_ | |

---

## 已知限制 & 绕过

(rules.md L1 / L2 / L3 的应对)

- L1 Lead 唤起未自动:测试期间手动在前端发空消息(`/`)或类似无害命令触发 Lead 续 turn。
- L2 conv_dir 注入未接通:**手测前**在 `src-tauri/src/runtime/tools/builtin/spawn_subagent.rs` line ~454(`conv_dir: None,` 处)临时改成基于 `aijia_home + scope + session` 的真实路径。手测完后**回滚**(留待 P3 paths wiring)。
- L3 Lead inbox 不主动唤起:同 L1。

---

## 总判定

- [ ] 场景 A 全绿
- [ ] 场景 B 全绿
- [ ] 场景 C 全绿
- [ ] 通用项全绿

完成后回报到 `docs/superpowers/plans/2026-05-11-ltr-implementation-plan.md` 末尾的 P2 完成检查清单。
