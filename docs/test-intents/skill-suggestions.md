# Skill 沉淀候选清单（待审核）

> 来源：跑测过程中发现的经验候选项；agent 不会自动加载本文件
> 审核流程：你看完 → 标记 ✅/❌ → 通过的迁到 `.claude/skills/test-intents-runner/SKILL.md` §5
> 路径：本文档**不在** `.claude/skills/` 下，仅作为人工审核入口

## 评审维度

| 维度 | 含义 |
|---|---|
| **稳定性** | 是产品长期行为，还是 CLI 短期 quirk？quirk 该写 cli-gap 而非 skill |
| **复发率** | 跨多个 task 都会踩，还是仅特定 task 单次出现？低频不沉 |
| **替代方案** | 已有更明确的文档（rules / cli-gap）吗？能从那里查到就不沉 skill |
| **agent 行为修正度** | 不沉，agent 下次会怎么错？错得严重才沉 |

---

## 候选 1：应用重启后会自动恢复最后会话

**经验**：在已登录账号下完整退出 `target/debug/aijia` 后通过 `pnpm dev:with-pilot` 重启，应用启动完成（`readyState: complete`）即直接落到关闭前最后访问的会话——`aijia where --json` 立即返回 `sessionId / messageCount` 等于关闭前数值。**不需要**先点对话列表里的某条会话才能"恢复"。

**影响 agent 行为**：写"重启 → 登录 → 点 conv_id 对话进入"这种步骤是冗余的，可能多出的点击反而切到别的会话；agent 重启完应直接读 `where` 看 sessionId 是否匹配预期。

**实战出处**：搜索 task 意图 3，2026-05-20 跑测。意图 3 第 3 步原文是「在对话列表中点击 conv_id 对应的对话」，实际重启完已经在那里了。

**评审建议**：✅ 推荐沉淀。是稳定产品行为而非 quirk，影响多个跨 turn / 跨重启的意图（崩溃恢复 task 同样会用到）。

**建议归位**：runner skill §5.13（编号待定，目前�� §5.12）

---

## 候选 2：`ui-message --include-tools` 默认过滤空文本占位

**经验**：tool 消息在 jsonl 中存的是 `{ role: "tool", content: "...", toolCallId: "toolu_..." }`，但前端 `ui-message` 默认会 drop 掉 text 字段为空的消息行（被视为 streaming-bubble 残渣）。重启后回放历史时，jsonl 里 4 条消息健全，但 `ui-message --include-tools` 只返 user + 最终 assistant 两条；必须加 `--include-empty` 才能看到全部 4 条，但此时 tool 消息的 `tool_calls` 字段又是空 array。

**影响 agent 行为**：判定"UI 是否显示工具气泡"只看 `ui-message` 会判错；得加 `--include-empty`、且要在 jsonl 里反查 `toolCallId` 才能跟 assistant.toolCalls[].id 配对。

**实战出处**：搜索 task 意图 3，2026-05-20 跑测。

**评审建议**：⚠️ 倾向不沉淀。这是 CLI 当前实现细节而非产品行为；已经记录在 `cli-gap.md` 作为 `aijia tool-bubble` 子命令的实战出处；CLI 修复后这条经验直接过期。

**建议归位**：保持留在 cli-gap.md，**不**进 skill。

---

## 候选 3：杀 `target/debug/aijia` 进程会一并把 `cargo run` 父进程也杀掉

**经验**：`pkill -f 'target/debug/aijia'` 之后 `pgrep` 不仅看不到 aijia bin，连父级 `cargo run --no-default-features --features e2e` 也消失了。tauri dev 父子进程绑定，没有"重启子进程而保留父进程 watcher"的能力，必须重新 `pnpm dev:with-pilot` 启全套。

**影响 agent 行为**：跑「崩溃恢复」类意图时不能假设 watcher 还活、只重启 webview；必须等 ~30s 完整重启（含 build.rs / vite / cargo dev runner / tauri build）。

**实战出处**：搜索 task 意图 3，2026-05-20 跑测。

**评审建议**：✅ 推荐沉淀。涉及"重启"动作的所有意图都会踩，agent 不知道这个会以为快速 kill+respawn 是几秒的事。

**建议归位**：runner skill §5.14 或合并进候选 1 写成「重启相关」一条

---

## 候选 5：`session_id` 和 `conversation_id` 在 rules / 实现 / CLI 三处语义不齐

**经验**：rules.md 多个 task 同时使用 `session_id` 和 `conv_id` 两个名字，意图作者可能把它们当成两种不同概念（session = 跨多 turn 的会话状态、conv = 持久化消息流）。实际 CLI `aijia where --json` 返回里只有 `sessionId` 字段，而它实际给的是**conversation id**（`~/.renlijia/users/$SCOPE/conversations/{该值}/messages.jsonl` 真存在）。意图作者再分别让 agent「记录 session_id」和「记录 conv_id」时，agent 只能两个变量都填同一个值。

**典型表现**：
- 待办队列 rules 写「调用 `pending_snapshot_for_session(sessionId)`」，agent 只能传 conv_id 当 sessionId，IPC 返 `[]` 也无法判定是真空队列还是 id 不对
- 项目记忆 / 搜索 rules 写「记录 conv_id」，没问题，因为存储路径就是按这个 id 走

**影响 agent 行为**：意图断言失败时，agent 难以判断是「产品 bug / pending 没入队」还是「id 类型不对 / IPC 调错」，会反复重测浪费时间。

**实战出处**：待办队列 task 意图 1，2026-05-20 跑测。`pending_snapshot_for_session` 用 where 给的 sessionId 调返空数组，无法分辨是因为没入队还是因为 sessionId 实际是 conv_id。

**评审建议**：⚠️ 这不是 skill 经验，是**产品 / spec / CLI 三方的命名对齐工作**。沉到 skill 反而把混乱合理化。

**建议归位**：
1. 推到 `cli-gap.md` 当一个独立的"对齐工作"条目（不是新 CLI，是改 `aijia where` 返回字段命名 + 拆出 `sessionId / conversationId` 两个字段）
2. 推到一个**待办**层面的事：让 author skill 下次写 rules 时确认 session vs conv 的语义，把术语统一

**不**沉 runner skill。

---

## 候选 6：长回答 LLM 经常自我中断反问澄清，"慢问题"prompt 不稳定

（暂记录，待评审）

**经验**：意图 1 / 3 需要 AI 流式输出至少 5-10 秒才能在期间插入 pending 消息。rules 例句"请用中文写一段大约 500 字的产品介绍稿"在实际跑测中，AI **3 秒内**就反问「请告诉我：1. 产品名称 — 叫...」截断了流式输出，让 pending 测试窗口不存在。

**影响 agent 行为**：agent 按 rules 示例 prompt 跑会反复 FAIL，误以为 pending 机制坏了，实际只是 prompt 没有触发足够长的流式。

**实战出处**：待办队列 task 意图 1，2026-05-20 跑测。改用"逐字写 1000 字诗，每句展开"才让 AI 不反问、真流式输出几十秒。

**评审建议**：✅ 推荐沉淀（短）。LLM 自反问的频率因 model / system prompt 而异，给 agent 一个"判定 AI 是否在真流式"的姿势比给死的 prompt 模板靠谱。

**建议归位**：runner skill §5.x「测 pending / cancel / 流式相关意图前，先确认 AI 真的在流式：`aijia where --json` 看 `isStreaming` 持续 3-5 秒不变为 false 再做下一步」

---

## 候选 4：长 turn 默认要先怀疑授权弹窗，时间戳是判 timeout 真伪的关键

（已沉为 §5.12，**不**重复审核；备注：实战出处是项目记忆 task 意图 2 卡 3 分 37 秒）

---

## 候选 7：意图测试报告里"占位账号 ↔ 现场账号"的 mapping 每条意图都写一边

**经验**：rules.md 里出现占位账号（如 `acct@example.com` / `Pwd-Valid-001`）、占位租户（如 DemoCorp）、占位资源 ID 等，实测时换成现场账号 / 租户 / id 后，**报告每条意图正文都重复写一行 mapping**，不要图省事只在报告头部 metadata 里列一次。

格式建议（每条意图开头加两行）：
```
- rules 写的账号: acct@example.com / Pwd-Valid-001
- 实测账号: 18267316753@pzctest / 18267316753
```

**影响 agent 行为**：报告每条意图独立可读，对账成本低；只在头部列一次会让单条意图脱离上下文后无法判断验收用的是什么账号、是否符合 rules 原意。

**实战出处**：登录 task，2026-05-20 跑测。用户明确选择"每条意图都写一边 mapping"。

**评审建议**：✅ 推荐沉淀（短，写进报告格式约定）。是稳定的报告写作约定而非临时 quirk，所有有占位字段的 task（登录 / 数字员工 / 工作空间 / 租户）都会用到。

**建议归位**：runner skill §3「报告输出格式」段，在示例报告里加一条 mapping 行示范，并在正文加一句「rules.md 出现占位账号/租户/id，实测时换值的，每条意图正文都标 mapping」

---

## 工作流

1. 你在每条「评审建议」下回复 ✅ 通过 / ❌ 否决 / 📝 修改建议
2. 通过的我用 Edit 工具 append 到 `.claude/skills/test-intents-runner/SKILL.md` §5 末尾
3. 否决的从本文件移除（保留在 git 历史里随时可查）
4. 修改的我按你的措辞重写后再请你二次确认

候选项后续累积新条目时统一在本文件追加 `## 候选 N：...`，agent 不会自动看到。
