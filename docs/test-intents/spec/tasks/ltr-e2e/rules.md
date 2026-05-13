# LTR 端到端冒烟意图（P2.11）

> 三条场景对齐 v4 §1.5 验收。**手动测试** —— 需启动 `pnpm tauri:dev` + 配置真实 LLM endpoint + 准备 EmployeeRecord。
>
> rules.md 只描述产品视角的意图与可观察断言。具体 fixture 准备、操作步骤、TDD 实现由人类测试者按 `context/how-to-test.md` 规范执行,记录到 `test-progress.md`。

---

## 场景 A — 单 Teammate 帮 Lead 调研

### 意图
用户在主对话提问"调研下 claude-code-best 的 Agent Teams 机制"。Lead 自己不应该阻塞做调研,而是建团队、派一个 Teammate(默认名"小研")去做,等 Teammate 汇报完再向用户汇总。

### 触发步骤(产品视角)
1. 用户在主对话发送上述提问。
2. Lead 自动调 `TeamCreate(team_name="research-team")`。
3. Lead 调 `Agent(employee_id="researcher", team_name="research-team", name="小研", prompt="...")` 派出 1 个 Teammate。
4. Teammate 接到首 turn(团队上下文 attachment + dispatch prompt)→ 调 `TaskClaim` 拿任务 → 自己 `WebSearch` 等工具调研 → `TaskUpdate(completed)` → `SendMessage(to="team-lead", message={type:"text",content:"..."})`。
5. Lead 收到 SendMessage → idle loop 续 turn → 汇总文字回复用户。

### 必须满足的断言
- A1. `~/.renlijia/users/{scope}/conversations/{conv_id}/team.json` 存在,内容含 1 名 Lead(name=team-lead) + 1 名 Teammate(name=小研)。
- A2. `conversations/{conv_id}/teammates/agent-{id}.jsonl` 存在,首条 user 消息含 `<system-reminder>` 与 "team-lead" 字样(team_context attachment 注入)。
- A3. teammates 目录的 `.meta.json` 存在,`kind=teammate` / `boot_system_prompt` 含 `Teammate 身份` 字样(addendum 已拼到 system prompt)。
- A4. teammate transcript 内可见 `SendMessage` 调用且 `to=team-lead`,内容是 text variant。
- A5. Lead 主对话最终文本汇总里包含 Teammate 调研产出的关键字(证明 Lead 确实读到了 SendMessage 内容)。
- A6. 全程 Lead 不阻塞用户对话,Teammate 与 Lead 是异步并发(非串行 await)。

### 不该发生
- A-N1. Lead 自己亲自调 `WebSearch` 做调研(应该交给 Teammate)。
- A-N2. Teammate 主动 `Ask` 用户(`is_async=true` 应自动 deny)。
- A-N3. Teammate 用 `SendMessage` 报告任务进度(应���用 `TaskUpdate`)。

---

## 场景 B — 多 Teammate Swarm 协作

### 意图
用户给一个跨 domain 的复合任务(例:"调研竞品并分析其定价模型,给我一张对比表"),Lead 切成 3 个独立子 Task,派 3 个 Teammate 并行,Teammate 之间需要时通过 SendMessage 交换中间结果,Lead 最终汇总。

### 触发步骤(产品视角)
1. 用户提问。
2. Lead `TeamCreate` → `TaskCreate` 三条(竞品调研 / 定价采集 / 表格汇编)。
3. Lead 三次 `Agent(employee_id=..., team_name=..., name="researcher"|"analyzer"|"writer")`。
4. 三个 Teammate 各自 `TaskClaim` 一条任务并行执行。
5. 至少一次 Teammate↔Teammate 直接 `SendMessage`(例如 writer 给 researcher 说"少了 X 项,补一下")。
6. 三个 Teammate 完成 → `TaskUpdate(completed)` → `SendMessage(to=team-lead, ...)`。
7. Lead 汇总。

### 必须满足的断言
- B1. `team.json` teammates 长度 == 3。
- B2. 三份独立 transcript JSONL,每份至少 1 条 user message + 1 条 assistant 回复。
- B3. 至少 1 条 transcript 含 `MessageSource::Teammate(<某个名字>)` 来源的 ChatMessage(证明 Teammate↔Teammate 通信发生)。
- B4. Lead 收到 task-notification XML(`<task-notification ... action="claimed">` / `action="updated">`),证明 P2.5 emitter 工作。
- B5. Lead 最终汇总含 3 个 sub-task 的成果。

### 不该发生
- B-N1. Lead 自己接管某个 Teammate 的任务(应让 Teammate 完成)。
- B-N2. 同一个 task 被两个 Teammate `TaskClaim` 成功(P1.5 唯一性约束)。
- B-N3. 某个 Teammate `Ask` 用户。

---

## 场景 C — shutdown 握手 + plan_approval

### 意图
Lead 让 Teammate 做一个有不可逆副作用的操作(例:"把 ~/Downloads 里 30 天前的图片移到归档文件夹")。Teammate 应在执行前发 `plan_approval_request` 让 Lead 看一眼;Lead approve=true 后才动手;完成后 Lead 走 shutdown handshake 让 Teammate 优雅退出。

### 触发步骤(产品视角)
1. 用户给 Lead 上述任务。
2. Lead `TeamCreate` + spawn 1 个 Teammate(`worker`)。
3. Teammate 起草执行方案 → `SendMessage(to=team-lead, message={type:"plan_approval_request", request_id:"pa-1", plan:"..."})`。
4. Lead 收到 → idle 唤起 → 决定 approve → `SendMessage(to=worker, message={type:"plan_approval_response", request_id:"pa-1", approve:true})`。
5. Teammate 收到 response → 执行实际动作 → `TaskUpdate(completed)` → `SendMessage(to=team-lead, text="完成")`。
6. Lead `SendMessage(to=worker, message={type:"shutdown_request", reason:"任务完成"})`。
7. Teammate 收到 shutdown_request → **不自杀**,而是 `SendMessage(to=team-lead, message={type:"shutdown_response", request_id:"...", approve:true, reason:"已无未保存状态"})`。
8. Lead 看到 approve=true 的 response → `TeammateStop(agent_name="worker")` 强制清理 / 或 `TeamDelete()` 整体收尾。

### 必须满足的断言
- C1. Teammate transcript 出现 `<plan-approval-request id="pa-1">` 包装的 user message(P2.9 渲染)。
- C2. Lead 与 Teammate transcript 中 request_id "pa-1" 双方都出现且匹配。
- C3. Teammate 收到 `shutdown_request` 后**没有立即退出**(transcript 显示它继续跑了一个 turn 才被 cancel)。
- C4. Teammate transcript 出现 `<shutdown-request reason="...">` user message(P2.6 渲染)。
- C5. 最终 `TeammateStop` 或 `TeamDelete` 触发 cleanup,`team.json` 中无残留 worker。
- C6. cancellation_registry / inbox_registry / agent_names 中均无 worker 残留(可通过 server 端日志或开发者面板观察)。

### 不该发生
- C-N1. Teammate 收到 `shutdown_request` 后立即 cleanup 退出(P1.6 旧行为,应已被 P2.6 移除)。
- C-N2. plan_approval 走 permission Ask 流程(应是纯结构化消息,不阻塞)。
- C-N3. `TeammateStop` 取消时把整个 Lead 也带挂(child token 隔离失败)。

---

## 通用观察项(三个场景共用)

- O1. `cancel_session` / app-close 触发后,registries(team / name / inbox / cancellation)全部清空(P1.8 + P2.7 hook)。
- O2. 所有 SendMessage 调用都能在前端 dev tools 的事件流里看到对应 `tool:executing` / `tool:completed` 事件(transport 链路完整)。
- O3. Teammate idle loop 60s heartbeat 在长跑任务中可见 `last_active_at` 更新(team.json mtime 推进 / 内存态可观察)。

---

## 已知限制(P2 阶段不做)

- L1. Lead 自动续 turn(Path A + Path C 的实际"spawn continuation")暂未接入 chat_turn_driver,因此目前 Lead "唤起"实际上要靠用户在前端手动发新消息触发。supervisor 的 enqueue/state CAS 已正确,只欠 driver 这一截。
- L2. team_context attachment 当前依赖 `TeammateWorkerCtx::conv_dir = Some(...)` 才注入,生产路径中 conv_dir 由 spawn_subagent 设为 `None`(留 P3 接 paths resolver),因此 Step 2 中 A2 / C1 等 transcript 路径断言**目前不能从生产路径产出**;手测需要在 spawn_subagent.rs 中临时把 conv_dir 注入,或者在 P3 paths wiring 完成后再跑。
- L3. SendMessage 末尾的 Lead wake 路径目前只是 `log::info!` —— Lead 不会在用户没主动发新消息的情况下被叫醒处理 inbox(P2.4 注释里说明)。

完成手测后请把上述 3 条限制对应的"绕过方法"写到 test-progress.md。
