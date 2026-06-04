# rules.md — 上下文压缩

本 task 测的产品承诺：**长对话接近上下文上限时，应用会自动整理旧历史；用户主动执行 `/compact` 时，应用会立即整理当前历史。两条用户可触达路径都要在界面和会话 transcript 中留下可核验边界，并保留摘要、最近消息、项目指令和后续对话能力，让用户能继续当前对话**。

本 task 同时看三层证据：

- **用户可见层**：对话进行中状态「整理上下文中…」、消息列表中的「对话已压缩」边界条、边界条展开后的「压缩前 / 压缩后 / 摘要消息」信息、压缩后的继续回复。
- **会话持久化层**：`messages.jsonl` 中的 `compact_boundary` 系统消息、压缩摘要消息、压缩边界之后的新 user / assistant 消息。
- **深度 schema 层**：`messages.jsonl` 中的 compact boundary 是主证据；`compact_boundaries.jsonl` 如果存在，只作为兼容索引与 transcript metadata 做一致性校验。

本 task 不测 cargo / Rust 集成测试，不用 mock LLM，不验证 microcompact / autocompact / collapse 的内部执行顺序，也不把某个固定 token 阈值写成用户承诺。PromptTooLong recovery 属后端异常恢复路径，当前没有可由 `tauri-pilot aijia` 稳定触发的真实 provider 前置；该能力用 Rust 回归测试覆盖，不放进本 AEIT task 的可执行意图。

cc-best 对齐矩阵：

| 能力面 | 本 task 意图 | 证据口径 |
|---|---|---|
| 自动压缩 auto compact | 001、002、003、005、006、007 | UI 边界条 + `messages.jsonl` 的 `system/compact_boundary` + compact summary |
| 手动 `/compact` | 008 | `/compact` 不作为普通 user 消息落盘；boundary metadata `trigger == "manual"` |
| reload 恢复 | 003 | 重开会话后 boundary 仍来自 transcript，不依赖内存态 |
| tool pairing | 007 | assistant toolCall 与 tool result 之间不被 boundary 切断 |
| 项目指令重注入 | 004 | compact 后下一轮仍遵守 `AGENTS.md` |

压力触发口径：本 task 的每条意图都在真实账号目录下运行。每条意图在推断 `$SCOPE` 后，先读取 `~/.renlijia/users/$SCOPE/config.json` 的 `contextWindow` 原值，记为 `$CONTEXT_WINDOW_BEFORE`，再把 `contextWindow` 临时写为字符串 `"64000"`；该值会让 auto-compact 触发线固定到约 124000 chars。意图结束前必须恢复 `$CONTEXT_WINDOW_BEFORE`。压力文本单轮要低于触发线，多轮累计后进入触发线，验证的是“长历史接近当前窗口上限时会自动整理”，不是单条超大消息自触发。

压力文本输入口径：`tauri-pilot aijia type-message` 单次插入超大文本可能触发 WebView eval timeout。每轮压力消息用多次 `type-message` 追加到同一个 composer，每次追加文本控制在 `<= 20 KB`，全部追加完成后再执行一次 `tauri-pilot aijia send`。

UI 读取口径：`tauri-pilot aijia ui-message --json` 读取的是 `chatStore.messages` 原始记录，可用于检查 `system/compact_boundary` 和 `isCompactSummary`；压缩边界的用户可见状态使用 `tauri-pilot aijia compact-boundary-snapshot --json`；展开压缩边界条使用 `tauri-pilot aijia compact-boundary-toggle --index -1 --json`。

配置恢复口径：runner 如果用 shell 脚本执行压力轮，必须用 `trap` 做异常恢复，不能把真实用户配置永久留在 `"64000"`。

transcript 解析口径：`messages.jsonl` 和 `compact_boundaries.jsonl` 的一条记录都以 `\t✓\n` 分隔，解析时先按该分隔符拆记录，再对每条记录取 `\t` 前的 JSON 前缀解析。验收标准里的“记录”都指这个解析后的 JSON 记录，不按字面换行计数。

工作区口径：本 task 不假设 `tauri-pilot aijia select-workspace` 已实现。涉及临时工作区的意图必须先确认 `tauri-pilot aijia --help` 中存在对应 workspace 子命令；如果缺失，该意图按环境前置不满足记录为 `SKIPPED`。

工具链等待口径：涉及文件读取或工具调用的意图，`wait-reply` 返回后还要继续轮询 `tauri-pilot aijia where --json` 的 `messageCount` 与 `messages.jsonl` mtime，直到 30 秒内不再增长，并且 transcript 中已有最终 assistant 文本。这样避免在工具调用间隙把 turn 误判为完成。

---

## 意图-上下文压缩-001: 长历史触发整理，界面显示反馈

**场景**
用户在同一个对话里连续发送多轮大段文本，历史进入上下文压力区。应用主动整理旧历史，并在界面呈现整理中的状态和整理完成后的压缩边界。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断当前 scope：从 `tauri-pilot aijia where --json` 取当前登录用户 scope，记为 `$SCOPE`
3. 记录现有所有对话 ID：读取 `~/.renlijia/users/$SCOPE/conversations/` 下已有子目录名，记为集合 `$S_BEFORE`
4. 新建空对话：`tauri-pilot aijia new-task`
5. 准备压力文本 `/tmp/aijia-context-compact-block.txt`：写入至少 500 行固定句子，每行包含字面值 `CTX-COMPACT-PRESSURE` 和行号，文件大小在 `45 KB` 到 `55 KB` 之间
6. 连续执行 4 轮发送；第 N 轮先把下面这段文字输入到输入框，再将 `/tmp/aijia-context-compact-block.txt` 的完整内容按 `<= 20 KB` 分块追加到同一个输入框，全部追加完成后点击发送：
   ```
   这是上下文压缩压力轮 N。请只回复：已收到 CTX-COMPACT-ROUND-N。不要调用工具，不要总结压力文本。
   ```
7. 每轮发送后等待 AI 完整回复：`tauri-pilot aijia wait-reply --timeout 600`
8. 找到本轮新建的对话 ID：在 `~/.renlijia/users/$SCOPE/conversations/` 下取 `mtime` 最新且不在 `$S_BEFORE` 里的子目录名，记为 `$CONV_ID`
9. 如果当前 UI 仍未出现「整理上下文中…」或「对话已压缩」，最多追加 2 轮与第 6 步相同格式的压力消息；每轮后等待 `tauri-pilot aijia wait-reply --timeout 600`
10. 查看当前对话 UI 消息列表：`tauri-pilot aijia ui-message --json`
11. 查看压缩边界状态：`tauri-pilot aijia compact-boundary-snapshot --json`；如果存在边界，执行 `tauri-pilot aijia compact-boundary-toggle --index -1 --json`
12. 打开 `~/.renlijia/users/$SCOPE/conversations/$CONV_ID/messages.jsonl`

**验收标准**

- 压力轮次结束时，当前渲染消息列表存在 compact 用户反馈：过程中采样到「整理上下文中…」，或最终渲染消息列表包含「对话已压缩」
- 当前渲染消息列表包含「对话已压缩」
- 「对话已压缩」边界条展开后，当前渲染消息列表文本包含「压缩前」「压缩后」「摘要消息」
- 文件 `~/.renlijia/users/$SCOPE/conversations/$CONV_ID/messages.jsonl` 存在
- `messages.jsonl` 每条记录都是可解析 JSON
- `messages.jsonl` 中存在记录 `$MSG_BOUNDARY`：`role == "system"` 且 `subtype == "compact_boundary"`
- `$MSG_BOUNDARY.content` 字段 JSON 序列化字符串包含字面值 `Conversation compacted`
- `$MSG_BOUNDARY.compactMetadata.trigger == "auto"`
- `$MSG_BOUNDARY.compactMetadata.preTokens > 0`
- `$MSG_BOUNDARY.compactMetadata.messagesSummarized >= 1`
- 当前渲染消息列表文本不包含字面值 `context length exceeded`、`too many tokens`、`PromptTooLong`、`上下文过长，请压缩后重试`

---

## 意图-上下文压缩-002: 压缩后继续提问，仍能延续对话

**场景**
用户的长对话已经被自动整理。压缩不是终止当前会话；用户继续发送短问题后，应用接住后续提问，并把新的 user / assistant 消息接在压缩边界之后。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断当前 scope：从 `tauri-pilot aijia where --json` 取当前登录用户 scope，记为 `$SCOPE`
3. 记录现有所有对话 ID：读取 `~/.renlijia/users/$SCOPE/conversations/` 下已有子目录名，记为集合 `$S_BEFORE`
4. 新建空对话：`tauri-pilot aijia new-task`
5. 准备压力文本 `/tmp/aijia-context-compact-followup-block.txt`：写入至少 500 行固定句子，每行包含字面值 `CTX-COMPACT-FOLLOWUP-PRESSURE` 和行号，文件大小在 `45 KB` 到 `55 KB` 之间
6. 连续执行 4 轮发送；第 N 轮先把下面这段文字输入到输入框，再将 `/tmp/aijia-context-compact-followup-block.txt` 的完整内容按 `<= 20 KB` 分块追加到同一个输入框，全部追加完成后点击发送：
   ```
   这是上下文压缩后续提问压力轮 N。请只回复：已收到 FOLLOWUP-COMPACT-ROUND-N。不要调用工具，不要总结压力文本。
   ```
7. 每轮发送后等待 AI 完整回复：`tauri-pilot aijia wait-reply --timeout 600`
8. 找到本轮新建的对话 ID：在 `~/.renlijia/users/$SCOPE/conversations/` 下取 `mtime` 最新且不在 `$S_BEFORE` 里的子目录名，记为 `$CONV_ID`
9. 如果 `~/.renlijia/users/$SCOPE/conversations/$CONV_ID/messages.jsonl` 中还没有 `subtype == "compact_boundary"` 的记录，最多追加 2 轮与第 6 步相同格式的压力消息；每轮后等待 `tauri-pilot aijia wait-reply --timeout 600`
10. 发送短消息：`请只回复：压缩后仍可继续对话`
11. 等待 AI 完整回复：`tauri-pilot aijia wait-reply --timeout 600`
12. 打开 `~/.renlijia/users/$SCOPE/conversations/$CONV_ID/messages.jsonl`
13. 查看当前对话 UI 消息列表

**验收标准**

- `messages.jsonl` 中存在记录 `$MSG_BOUNDARY`：`subtype == "compact_boundary"`
- `$MSG_BOUNDARY` 之后存在 1 条 `role == "user"` 且 `isCompactSummary == true` 的摘要消息
- `$MSG_BOUNDARY` 之后存在 1 条 user 记录，其 `content` 字段 JSON 序列化字符串包含字面值 `压缩后仍可继续对话`
- 上述 follow-up user 记录之后存在 1 条 assistant 记录
- 上述 assistant 记录的 `content` 字段 JSON 序列化字符串包含字面值 `压缩后仍可继续对话`
- 对话消息列表最后一条 assistant 回复包含字面值 `压缩后仍可继续对话`
- `$MSG_BOUNDARY` 之后至少存在 2 条 `isCompactSummary != true` 的普通消息
- follow-up assistant 回复不包含压力文本标记 `CTX-COMPACT-FOLLOWUP-PRESSURE`
- follow-up assistant 回复不包含字面值 `请重新提供前文`、`我不记得之前的内容`、`context length exceeded`、`too many tokens`、`PromptTooLong`、`上下文过长，请压缩后重试`

---

## 意图-上下文压缩-003: 重新打开会话，压缩边界仍然可见

**场景**
用户的长对话发生过自动压缩。用户切走再重新打开该对话时，压缩边界作为会话历史的一部分恢复显示，而不是只在当前内存态里短暂出现。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断当前 scope：从 `tauri-pilot aijia where --json` 取当前登录用户 scope，记为 `$SCOPE`
3. 记录现有所有对话 ID：读取 `~/.renlijia/users/$SCOPE/conversations/` 下已有子目录名，记为集合 `$S_BEFORE`
4. 新建空对话：`tauri-pilot aijia new-task`
5. 准备压力文本 `/tmp/aijia-context-compact-reopen-block.txt`：写入至少 500 行固定句子，每行包含字面值 `CTX-COMPACT-REOPEN-PRESSURE` 和行号，文件大小在 `45 KB` 到 `55 KB` 之间
6. 连续执行 4 轮发送；第 N 轮先把下面这段文字输入到输入框，再将 `/tmp/aijia-context-compact-reopen-block.txt` 的完整内容按 `<= 20 KB` 分块追加到同一个输入框，全部追加完成后点击发送：
   ```
   这是上下文压缩重开恢复压力轮 N。请只回复：已收到 REOPEN-COMPACT-ROUND-N。不要调用工具，不要总结压力文本。
   ```
7. 每轮发送后等待 AI 完整回复：`tauri-pilot aijia wait-reply --timeout 600`
8. 找到本轮新建的对话 ID：在 `~/.renlijia/users/$SCOPE/conversations/` 下取 `mtime` 最新且不在 `$S_BEFORE` 里的子目录名，记为 `$CONV_ID`
9. 如果当前 UI 仍未出现「对话已压缩」，最多追加 2 轮与第 6 步相同格式的压力消息；每轮后等待 `tauri-pilot aijia wait-reply --timeout 600`
10. 执行 `tauri-pilot aijia compact-boundary-snapshot --json` 确认当前渲染消息列表出现「对话已压缩」，再执行 `tauri-pilot aijia compact-boundary-toggle --index -1 --json`
11. 打开 `~/.renlijia/users/$SCOPE/conversations/$CONV_ID/messages.jsonl`，记录最新 `subtype == "compact_boundary"` 记录的 `createdAt` 与 `compactMetadata.preTokens`
12. 切换到另一个对话，或回到对话列表后重新打开 `$CONV_ID`
13. 再次执行 `tauri-pilot aijia compact-boundary-snapshot --json`，并通过 `tauri-pilot aijia compact-boundary-toggle --index -1 --json` 展开「对话已压缩」边界条
14. 再次打开 `~/.renlijia/users/$SCOPE/conversations/$CONV_ID/messages.jsonl`

**验收标准**

- 重开前，当前渲染消息列表包含「对话已压缩」
- 重开前，「对话已压缩」边界条展开后包含「压缩前」「压缩后」「摘要消息」
- 重开后，当前渲染消息列表仍包含「对话已压缩」
- 重开后，「对话已压缩」边界条展开后仍包含「压缩前」「压缩后」「摘要消息」
- 重开后，`messages.jsonl` 中仍存在 `subtype == "compact_boundary"` 的记录
- 重开后最新 `subtype == "compact_boundary"` 记录的 `createdAt` 与重开前记录的 `createdAt` 一致
- 重开后最新 `subtype == "compact_boundary"` 记录的 `compactMetadata.preTokens` 与重开前记录的 `compactMetadata.preTokens` 一致
- 重开后当前渲染消息列表文本不包含普通裸露 system 文本 `Conversation compacted`
- 重开后消息列表中至少存在 1 条压缩边界之后的普通 user 或 assistant 消息

---

## 意图-上下文压缩-004: 压缩后继续提问，仍遵守项目指令

**场景**
用户在带项目指令的工作区里进行长对话，历史被自动整理后继续提问。应用把项目指令重新放回模型上下文，让 AI 在压缩后的下一轮仍然遵守该项目规则。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断当前 scope：从 `tauri-pilot aijia where --json` 取当前登录用户 scope，记为 `$SCOPE`
3. 准备测试工作区目录 `/tmp/aijia-context-compact-project`
4. 在该目录写入 `AGENTS.md`，内容包含以下规则：
   ```
   # 测试项目指令

   在本项目内回答用户时，每条最终回复末尾必须单独写一行：
   [PROJECT-COMPACT-RULE-OK]
   ```
5. 读取 `tauri-pilot aijia --help`，确认存在可选择本地工作区的 workspace 子命令
6. 通过 CLI 选中测试工作区：`tauri-pilot aijia workspace-queue-path /tmp/aijia-context-compact-project`，再执行 `tauri-pilot aijia workspace-open-picker` 和 `tauri-pilot aijia workspace-pick --variant other`
7. 记录现有所有对话 ID：读取 `~/.renlijia/users/$SCOPE/conversations/` 下已有子目录名，记为集合 `$S_BEFORE`
8. 新建空对话：`tauri-pilot aijia new-task`
9. 发送校准消息：`请用一句话确认你已读取当前项目指令`
10. 等待 AI 完整回复：`tauri-pilot aijia wait-reply --timeout 600`
11. 确认校准回复包含字面值 `[PROJECT-COMPACT-RULE-OK]`
12. 准备压力文本 `/tmp/aijia-context-compact-project-block.txt`：写入至少 500 行固定句子，每行包含字面值 `CTX-COMPACT-PROJECT-PRESSURE` 和行号，文件大小在 `45 KB` 到 `55 KB` 之间
13. 在同一对话中连续执行 4 轮发送；第 N 轮先把下面这段文字输入到输入框，再将 `/tmp/aijia-context-compact-project-block.txt` 的完整内容按 `<= 20 KB` 分块追加到同一个输入框，全部追加完成后点击发送：
   ```
   这是项目指令重注入压力轮 N。请只回复：已收到 PROJECT-COMPACT-ROUND-N。不要调用工具，不要总结压力文本。
   ```
14. 每轮发送后等待 AI 完整回复：`tauri-pilot aijia wait-reply --timeout 600`
15. 找到本轮新建的对话 ID：在 `~/.renlijia/users/$SCOPE/conversations/` 下取 `mtime` 最新且不在 `$S_BEFORE` 里的子目录名，记为 `$CONV_ID`
16. 如果 `~/.renlijia/users/$SCOPE/conversations/$CONV_ID/messages.jsonl` 中还没有 `subtype == "compact_boundary"` 的记录，最多追加 2 轮与第 13 步相同格式的压力消息；每轮后等待 `tauri-pilot aijia wait-reply --timeout 600`
17. 发送短消息：`请再次确认项目指令是否仍然有效`
18. 等待 AI 完整回复：`tauri-pilot aijia wait-reply --timeout 600`
19. 打开 `~/.renlijia/users/$SCOPE/conversations/$CONV_ID/messages.jsonl`
20. 查看当前对话 UI 消息列表

**验收标准**

- 校准回复包含字面值 `[PROJECT-COMPACT-RULE-OK]`
- `messages.jsonl` 中存在记录 `$MSG_BOUNDARY`：`subtype == "compact_boundary"`
- `$MSG_BOUNDARY` 之后存在 1 条 user 记录，其 `content` 字段 JSON 序列化字符串包含字面值 `请再次确认项目指令是否仍然有效`
- 上述 user 记录之后存在 1 条 assistant 记录
- `messages.jsonl` 末条记录 `role == "assistant"`
- `messages.jsonl` 末条记录 `content` 字段 JSON 序列化字符串包含字面值 `[PROJECT-COMPACT-RULE-OK]`
- 对话消息列表最后一条 assistant 回复包含字面值 `[PROJECT-COMPACT-RULE-OK]`
- `$MSG_BOUNDARY` 之后存在 1 条 `role == "user"` 且 `isCompactSummary == true` 的摘要消息
- compact 后 assistant 回复不包含字面值 `我无法读取项目指令`、`没有看到项目指令`、`我找不到 AGENTS.md`、`项目指令已失效`
- compact 后 assistant 回复不包含字面值 `context length exceeded`、`too many tokens`、`PromptTooLong`

---

## 意图-上下文压缩-005: 压缩落盘后，边界结构一致

**场景**
长对话触发自动压缩后，应用不仅要让用户继续对话，还必须把压缩边界、摘要、tail 起点和 boundary metadata 写成一致的数据结构。这个意图专门校验 transcript schema；兼容索引文件存在时再做一致性比对。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断当前 scope：从 `tauri-pilot aijia where --json` 取当前登录用户 scope，记为 `$SCOPE`
3. 记录现有所有对话 ID：读取 `~/.renlijia/users/$SCOPE/conversations/` 下已有子目录名，记为集合 `$S_BEFORE`
4. 新建空对话：`tauri-pilot aijia new-task`
5. 准备压力文本 `/tmp/aijia-context-compact-schema-block.txt`：写入至少 500 行固定句子，每行包含字面值 `CTX-COMPACT-SCHEMA-PRESSURE` 和行号，文件大小在 `45 KB` 到 `55 KB` 之间
6. 连续执行 4 轮发送；第 N 轮先把下面这段文字输入到输入框，再将 `/tmp/aijia-context-compact-schema-block.txt` 的完整内容按 `<= 20 KB` 分块追加到同一个输入框，全部追加完成后点击发送：
   ```
   这是上下文压缩 schema 校验压力轮 N。请只回复：已收到 SCHEMA-COMPACT-ROUND-N。不要调用工具，不要总结压力文本。
   ```
7. 每轮发送后等待 AI 完整回复：`tauri-pilot aijia wait-reply --timeout 600`
8. 找到本轮新建的对话 ID：在 `~/.renlijia/users/$SCOPE/conversations/` 下取 `mtime` 最新且不在 `$S_BEFORE` 里的子目录名，记为 `$CONV_ID`
9. 如果 `~/.renlijia/users/$SCOPE/conversations/$CONV_ID/messages.jsonl` 中还没有 `subtype == "compact_boundary"` 的记录，最多追加 2 轮与第 6 步相同格式的压力消息；每轮后等待 `tauri-pilot aijia wait-reply --timeout 600`
10. 发送短消息：`请只回复：schema 校验后仍可继续`
11. 等待 AI 完整回复：`tauri-pilot aijia wait-reply --timeout 600`
12. 打开 `~/.renlijia/users/$SCOPE/conversations/$CONV_ID/messages.jsonl`
13. 如果存在 `~/.renlijia/users/$SCOPE/conversations/$CONV_ID/compact_boundaries.jsonl`，打开该文件
14. 在 `messages.jsonl` 中定位最新一条 `subtype == "compact_boundary"` 的记录，记为 `$MSG_BOUNDARY`
15. 读取 `$MSG_BOUNDARY.compactMetadata`，记为 `$BOUNDARY_META`
16. 在 `messages.jsonl` 中定位 `$MSG_BOUNDARY` 之后第一条 `isCompactSummary == true` 的 user 记录，记为 `$SUMMARY_MSG`
17. 在 `messages.jsonl` 中定位 `id == $BOUNDARY_META.tailMessageId` 的记录，记为 `$TAIL_ANCHOR_MSG`
18. 在 `messages.jsonl` 中定位 `$MSG_BOUNDARY` 之后第一条 `content` 字段 JSON 序列化字符串包含字面值 `schema 校验后仍可继续` 的 user 记录，记为 `$FOLLOWUP_USER`
19. 在 `messages.jsonl` 中定位 `$FOLLOWUP_USER` 之后第一条 assistant 记录，记为 `$FOLLOWUP_ASSISTANT`
20. 如果 `compact_boundaries.jsonl` 存在，在其中定位最后一条记录，记为 `$SIDECAR_RECORD`

**验收标准**

- 文件 `~/.renlijia/users/$SCOPE/conversations/$CONV_ID/messages.jsonl` 存在
- `messages.jsonl` 每条记录都是可解析 JSON
- `$MSG_BOUNDARY.role == "system"`
- `$MSG_BOUNDARY.subtype == "compact_boundary"`
- `$MSG_BOUNDARY.content` 字段 JSON 序列化字符串包含字面值 `Conversation compacted`
- `$MSG_BOUNDARY.compactMetadata` 存在
- `$BOUNDARY_META.trigger == "auto"`
- `$BOUNDARY_META.preTokens > 0`
- `$BOUNDARY_META.postTokens > 0`
- `$BOUNDARY_META.postTokens < $BOUNDARY_META.preTokens`
- `$BOUNDARY_META.tokensSaved == $BOUNDARY_META.preTokens - $BOUNDARY_META.postTokens`
- `$BOUNDARY_META.messagesSummarized >= 1`
- `$BOUNDARY_META.tailMessageId` 是非空字符串
- `$SUMMARY_MSG.role == "user"`
- `$SUMMARY_MSG.isCompactSummary == true`
- `$SUMMARY_MSG.id` 是非空字符串
- `$SUMMARY_MSG.content` 字段 JSON 序列化字符串包含字面值 `<context>`
- `$TAIL_ANCHOR_MSG.id == $BOUNDARY_META.tailMessageId`
- `$TAIL_ANCHOR_MSG.isCompactSummary != true`
- 自动整理当前 user 的主路径中，`$TAIL_ANCHOR_MSG` 是 `$MSG_BOUNDARY` 之前最近一条非摘要 user 记录
- `$FOLLOWUP_USER.role == "user"`
- `$FOLLOWUP_USER.isCompactSummary != true`
- `$FOLLOWUP_USER.content` 字段 JSON 序列化字符串包含字面值 `schema 校验后仍可继续`
- `$FOLLOWUP_ASSISTANT.role == "assistant"`
- `$FOLLOWUP_ASSISTANT.content` 字段 JSON 序列化字符串包含字面值 `schema 校验后仍可继续`
- `$SUMMARY_MSG` 不是 `messages.jsonl` 最后一条记录
- `$FOLLOWUP_USER` 位于 `$MSG_BOUNDARY` 之后
- `$FOLLOWUP_ASSISTANT` 位于 `$FOLLOWUP_USER` 之后
- `$SUMMARY_MSG.content` 字段 JSON 序列化字符串不是空字符串
- `$BOUNDARY_META.preservedSegment` 存在时，`preservedSegment.headUuid` 或 `preservedSegment.firstPreservedMessageId` 是非空字符串
- `$BOUNDARY_META.preservedSegment` 存在时，`preservedSegment.anchorUuid` 或 `preservedSegment.anchorMessageId` 等于 `$SUMMARY_MSG.id`
- `$BOUNDARY_META.preservedSegment` 存在时，`preservedSegment.tailUuid` 或 `preservedSegment.tailMessageId` 等于 `$BOUNDARY_META.tailMessageId`
- `$BOUNDARY_META.preservedSegment` 存在时，`preservedSegment.preservedTokenCount > 0`
- 如果 `$SIDECAR_RECORD` 存在，`$SIDECAR_RECORD.conversation_id == "$CONV_ID"`
- 如果 `$SIDECAR_RECORD` 存在，`$SIDECAR_RECORD.pre_tokens == $BOUNDARY_META.preTokens`
- 如果 `$SIDECAR_RECORD` 存在，`$SIDECAR_RECORD.post_tokens == $BOUNDARY_META.postTokens`
- 如果 `$SIDECAR_RECORD` 存在，`$SIDECAR_RECORD.messages_summarized == $BOUNDARY_META.messagesSummarized`
- 如果 `$SIDECAR_RECORD` 存在，`$SIDECAR_RECORD.tail_message_id == $BOUNDARY_META.tailMessageId`

---

## 意图-上下文压缩-006: 压缩后再次长聊，再次追加边界

**场景**
用户的对话已经产生过一次自动压缩。用户继续在同一会话里发送长内容并再次进入上下文压力区时，应用追加新的压缩边界，而不是复写旧边界或把旧摘要当成普通对话重新暴露给用户。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断当前 scope：从 `tauri-pilot aijia where --json` 取当前登录用户 scope，记为 `$SCOPE`
3. 记录现有所有对话 ID：读取 `~/.renlijia/users/$SCOPE/conversations/` 下已有子目录名，记为集合 `$S_BEFORE`
4. 新建空对话：`tauri-pilot aijia new-task`
5. 准备第一段压力文本 `/tmp/aijia-context-compact-twice-a.txt`：写入至少 500 行固定句子，每行包含字面值 `CTX-COMPACT-TWICE-A` 和行号，文件大小在 `45 KB` 到 `55 KB` 之间
6. 连续执行 4 轮发送；第 N 轮先把下面这段文字输入到输入框，再将 `/tmp/aijia-context-compact-twice-a.txt` 的完整内容按 `<= 20 KB` 分块追加到同一个输入框，全部追加完成后点击发送：
   ```
   这是第一次上下文压缩压力轮 N。请只回复：已收到 TWICE-A-ROUND-N。不要调用工具，不要总结压力文本。
   ```
7. 每轮发送后等待 AI 完整回复：`tauri-pilot aijia wait-reply --timeout 600`
8. 找到本轮新建的对话 ID：在 `~/.renlijia/users/$SCOPE/conversations/` 下取 `mtime` 最新且不在 `$S_BEFORE` 里的子目录名，记为 `$CONV_ID`
9. 如果 `~/.renlijia/users/$SCOPE/conversations/$CONV_ID/messages.jsonl` 中还没有 `subtype == "compact_boundary"` 的记录，最多追加 2 轮与第 6 步相同格式的压力消息；每轮后等待 `tauri-pilot aijia wait-reply --timeout 600`
10. 发送短消息：`请只回复：第一次压缩后继续`
11. 等待 AI 完整回复：`tauri-pilot aijia wait-reply --timeout 600`
12. 准备第二段压力文本 `/tmp/aijia-context-compact-twice-b.txt`：写入至少 500 行固定句子，每行包含字面值 `CTX-COMPACT-TWICE-B` 和行号，文件大小在 `45 KB` 到 `55 KB` 之间
13. 在同一对话中连续执行 4 轮发送；第 N 轮先把下面这段文字输入到输入框，再将 `/tmp/aijia-context-compact-twice-b.txt` 的完整内容按 `<= 20 KB` 分块追加到同一个输入框，全部追加完成后点击发送：
   ```
   这是第二次上下文压缩压力轮 N。请只回复：已收到 TWICE-B-ROUND-N。不要调用工具，不要总结压力文本。
   ```
14. 每轮发送后等待 AI 完整回复：`tauri-pilot aijia wait-reply --timeout 600`
15. 如果 `~/.renlijia/users/$SCOPE/conversations/$CONV_ID/messages.jsonl` 中 `subtype == "compact_boundary"` 的记录数小于 2，最多追加 2 轮与第 13 步相同格式的压力消息；每轮后等待 `tauri-pilot aijia wait-reply --timeout 600`
16. 发送短消息：`请只回复：第二次压缩后继续`
17. 等待 AI 完整回复：`tauri-pilot aijia wait-reply --timeout 600`
18. 打开 `~/.renlijia/users/$SCOPE/conversations/$CONV_ID/messages.jsonl`
19. 在 `messages.jsonl` 中按出现顺序定位所有 `subtype == "compact_boundary"` 的记录，记为 `$BOUNDARIES`
20. 取 `$BOUNDARIES` 倒数第二条为 `$BOUNDARY_1`，最后一条为 `$BOUNDARY_2`
21. 在 `$BOUNDARY_1` 之后定位第一条 `isCompactSummary == true` 的 user 记录，记为 `$SUMMARY_1`
22. 在 `$BOUNDARY_2` 之后定位第一条 `isCompactSummary == true` 的 user 记录，记为 `$SUMMARY_2`
23. 在 `messages.jsonl` 中定位 `id == $BOUNDARY_2.compactMetadata.tailMessageId` 的记录，记为 `$TAIL_2`
24. 查看当前对话 UI 消息列表

**验收标准**

- 文件 `~/.renlijia/users/$SCOPE/conversations/$CONV_ID/messages.jsonl` 存在
- `messages.jsonl` 每条记录都是可解析 JSON
- `$BOUNDARIES.length >= 2`
- `$BOUNDARY_2` 位于 `$BOUNDARY_1` 之后
- `$BOUNDARY_1.compactMetadata.trigger == "auto"`
- `$BOUNDARY_2.compactMetadata.trigger == "auto"`
- `$BOUNDARY_2.compactMetadata.preTokens > 0`
- `$BOUNDARY_2.compactMetadata.postTokens > 0`
- `$BOUNDARY_2.compactMetadata.postTokens < $BOUNDARY_2.compactMetadata.preTokens`
- `$SUMMARY_1.role == "user"`
- `$SUMMARY_1.isCompactSummary == true`
- `$SUMMARY_2.role == "user"`
- `$SUMMARY_2.isCompactSummary == true`
- `$SUMMARY_2` 位于 `$BOUNDARY_2` 之后
- `$SUMMARY_2.id` 是非空字符串
- `$BOUNDARY_2.compactMetadata.tailMessageId` 是非空字符串
- `$TAIL_2.id == $BOUNDARY_2.compactMetadata.tailMessageId`
- `$TAIL_2.isCompactSummary != true`
- `$TAIL_2` 位于 `$BOUNDARY_1` 之后
- `messages.jsonl` 中存在 1 条 user 记录，其 `content` 字段 JSON 序列化字符串包含字面值 `第二次压缩后继续`
- 上述 user 记录之后存在 1 条 assistant 记录
- 上述 assistant 记录的 `content` 字段 JSON 序列化字符串包含字面值 `第二次压缩后继续`
- 当前渲染消息列表包含「对话已压缩」
- 当前渲染消息列表文本不包含字面值 `<context>`
- 当前渲染消息列表文本不包含字面值 `context length exceeded`、`too many tokens`、`PromptTooLong`、`上下文过长，请压缩后重试`

---

## 意图-上下文压缩-007: 含文件读取历史，消息保持配对

**场景**
用户在工作区里让 AI 读取文件后继续长聊，随后触发上下文压缩。压缩边界不能切断 assistant 发起的文件读取请求和对应的工具结果，压缩后继续提问仍能得到回复。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断当前 scope：从 `tauri-pilot aijia where --json` 取当前登录用户 scope，记为 `$SCOPE`
3. 准备测试工作区目录 `/tmp/aijia-context-compact-tool-pair`
4. 在该目录写入文件 `compact-tool-pair-source.txt`，内容第一行是 `TOOL-COMPACT-PAIR-MARKER`，后续写入至少 500 行带行号的固定文本
5. 读取 `tauri-pilot aijia --help`，确认存在可选择本地工作区的 workspace 子命令
6. 通过 CLI 选中测试工作区：`tauri-pilot aijia workspace-queue-path /tmp/aijia-context-compact-tool-pair`，再执行 `tauri-pilot aijia workspace-open-picker` 和 `tauri-pilot aijia workspace-pick --variant other`
7. 记录现有所有对话 ID：读取 `~/.renlijia/users/$SCOPE/conversations/` 下已有子目录名，记为集合 `$S_BEFORE`
8. 新建空对话：`tauri-pilot aijia new-task`
9. 发送文件读取请求：`请读取当前工作区的 compact-tool-pair-source.txt，并只回复第一行标记`
10. 等待 AI 完整回复：`tauri-pilot aijia wait-reply --timeout 600`
11. 找到本轮新建的对话 ID：在 `~/.renlijia/users/$SCOPE/conversations/` 下取 `mtime` 最新且不在 `$S_BEFORE` 里的子目录名，记为 `$CONV_ID`
12. 打开 `~/.renlijia/users/$SCOPE/conversations/$CONV_ID/messages.jsonl`；如果其中不存在带 `toolCalls` 的 assistant 记录，最多重新发送 2 次第 9 步文件读取请求，每次后等待 AI 完整回复
13. 准备压力文本 `/tmp/aijia-context-compact-tool-pair-block.txt`：写入至少 500 行固定句子，每行包含字面值 `CTX-COMPACT-TOOL-PAIR-PRESSURE` 和行号，文件大小在 `45 KB` 到 `55 KB` 之间
14. 在同一对话中连续执行 4 轮发送；第 N 轮先把下面这段文字输入到输入框，再将 `/tmp/aijia-context-compact-tool-pair-block.txt` 的完整内容按 `<= 20 KB` 分块追加到同一个输入框，全部追加完成后点击发送：
   ```
   这是文件读取历史压缩压力轮 N。请只回复：已收到 TOOL-PAIR-COMPACT-ROUND-N。不要调用工具，不要总结压力文本。
   ```
15. 每轮发送后等待 AI 完整回复：`tauri-pilot aijia wait-reply --timeout 600`
16. 如果 `~/.renlijia/users/$SCOPE/conversations/$CONV_ID/messages.jsonl` 中还没有 `subtype == "compact_boundary"` 的记录，最多追加 2 轮与第 14 步相同格式的压力消息；每轮后等待 `tauri-pilot aijia wait-reply --timeout 600`
17. 发送短消息：`请只回复：文件读取历史压缩后继续`
18. 等待 AI 完整回复：`tauri-pilot aijia wait-reply --timeout 600`
19. 打开 `~/.renlijia/users/$SCOPE/conversations/$CONV_ID/messages.jsonl`
20. 收集所有 assistant 记录里的 `toolCalls[].id`，记为集合 `$TOOL_CALL_IDS`
21. 收集所有 tool 记录里的 `toolCallId` 或 `tool_call_id`，记为集合 `$TOOL_RESULT_IDS`
22. 对每个 `$TOOL_CALL_IDS` 中的 id，定位发起该 id 的 assistant 记录和匹配该 id 的 tool 记录
23. 查看当前对话 UI 消息列表

**验收标准**

- 文件 `~/.renlijia/users/$SCOPE/conversations/$CONV_ID/messages.jsonl` 存在
- `messages.jsonl` 每条记录都是可解析 JSON
- `messages.jsonl` 中存在至少 1 条 assistant 记录，其 `toolCalls.length >= 1`
- `messages.jsonl` 中存在至少 1 条 tool 记录，其 `content` 字段 JSON 序列化字符串包含字面值 `TOOL-COMPACT-PAIR-MARKER`
- `$TOOL_CALL_IDS.length >= 1`
- `$TOOL_RESULT_IDS.length >= 1`
- `$TOOL_CALL_IDS` 中的每个 id 都存在于 `$TOOL_RESULT_IDS`
- `$TOOL_RESULT_IDS` 中的每个 id 都存在于 `$TOOL_CALL_IDS`
- 对每个匹配的 id，assistant 记录位于对应 tool 记录之前
- 对每个匹配的 id，assistant 记录与对应 tool 记录之间不存在 `subtype == "compact_boundary"` 的记录
- `messages.jsonl` 中存在记录 `$MSG_BOUNDARY`：`subtype == "compact_boundary"`
- `$MSG_BOUNDARY.compactMetadata.trigger == "auto"`
- `$MSG_BOUNDARY` 之后存在 1 条 `role == "user"` 且 `isCompactSummary == true` 的摘要消息
- `messages.jsonl` 中存在 1 条 user 记录，其 `content` 字段 JSON 序列化字符串包含字面值 `文件读取历史压缩后继续`
- 上述 user 记录之后存在 1 条 assistant 记录
- 上述 assistant 记录的 `content` 字段 JSON 序列化字符串包含字面值 `文件读取历史压缩后继续`
- 当前渲染消息列表文本不包含字面值 `<context>`
- 当前渲染消息列表文本不包含字面值 `context length exceeded`、`too many tokens`、`PromptTooLong`、`上下文过长，请压缩后重试`

---

## 意图-上下文压缩-008: 手动 `/compact` 整理当前历史

**场景**
用户在一次普通对话后主动输入 `/compact`，应用把它当成上下文控制命令执行压缩，而不是把 `/compact` 当普通聊天内容发给模型。压缩完成后，用户继续提问能接在新的压缩边界之后。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断当前 scope：从 `tauri-pilot aijia where --json` 取当前登录用户 scope，记为 `$SCOPE`
3. 记录现有所有对话 ID：读取 `~/.renlijia/users/$SCOPE/conversations/` 下已有子目录名，记为集合 `$S_BEFORE`
4. 新建空对话：`tauri-pilot aijia new-task`
5. 发送第一条普通消息：`请记住手动压缩标记 MANUAL-COMPACT-MARKER-1，并只回复：已记录 MANUAL-COMPACT-MARKER-1`
6. 等待 AI 完整回复：`tauri-pilot aijia wait-reply --timeout 600`
7. 发送第二条普通消息：`请记住手动压缩标记 MANUAL-COMPACT-MARKER-2，并只回复：已记录 MANUAL-COMPACT-MARKER-2`
8. 等待 AI 完整回复：`tauri-pilot aijia wait-reply --timeout 600`
9. 找到本轮新建的对话 ID：在 `~/.renlijia/users/$SCOPE/conversations/` 下取 `mtime` 最新且不在 `$S_BEFORE` 里的子目录名，记为 `$CONV_ID`
10. 记录 `messages.jsonl` 中当前 `subtype == "compact_boundary"` 的记录数，记为 `$BOUNDARY_COUNT_BEFORE`
11. 输入手动压缩命令并发送：`/compact 保留 MANUAL-COMPACT-MARKER-1 和 MANUAL-COMPACT-MARKER-2 的事实`
12. 不使用 `wait-reply` 等待本步骤；轮询 `~/.renlijia/users/$SCOPE/conversations/$CONV_ID/messages.jsonl`，直到 `subtype == "compact_boundary"` 的记录数大于 `$BOUNDARY_COUNT_BEFORE`，或 180 秒超时
13. 发送短消息：`请只回复：手动压缩后继续 MANUAL-COMPACT-FOLLOWUP`
14. 等待 AI 完整回复：`tauri-pilot aijia wait-reply --timeout 600`
15. 打开 `~/.renlijia/users/$SCOPE/conversations/$CONV_ID/messages.jsonl`
16. 查看当前对话 UI 消息列表：`tauri-pilot aijia ui-message --json`；再执行 `tauri-pilot aijia compact-boundary-toggle --index -1 --json` 展开「对话已压缩」边界条

**验收标准**

- `messages.jsonl` 中存在最新记录 `$MSG_BOUNDARY`：`role == "system"` 且 `subtype == "compact_boundary"`
- `$MSG_BOUNDARY.compactMetadata.trigger == "manual"`
- `$MSG_BOUNDARY.compactMetadata.preTokens > 0`
- `$MSG_BOUNDARY.compactMetadata.postTokens > 0`
- `$MSG_BOUNDARY.compactMetadata.tokensSaved == $MSG_BOUNDARY.compactMetadata.preTokens - $MSG_BOUNDARY.compactMetadata.postTokens`
- `$MSG_BOUNDARY` 之后存在 1 条 `role == "user"` 且 `isCompactSummary == true` 的摘要消息
- `messages.jsonl` 中没有普通 user 记录的 `content` 字段 JSON 序列化字符串等于或包含字面值 `/compact 保留 MANUAL-COMPACT-MARKER-1`
- `messages.jsonl` 中存在 1 条 user 记录，其 `content` 字段 JSON 序列化字符串包含字面值 `手动压缩后继续 MANUAL-COMPACT-FOLLOWUP`
- 上述 follow-up user 记录位于 `$MSG_BOUNDARY` 之后
- 上述 follow-up user 记录之后存在 1 条 assistant 记录
- 上述 assistant 记录的 `content` 字段 JSON 序列化字符串包含字面值 `MANUAL-COMPACT-FOLLOWUP`
- 当前渲染消息列表包含「对话已压缩」
- 「对话已压缩」边界条展开后，当前渲染消息列表文本包含「压缩前」「压缩后」「摘要消息」

---

## 意图-上下文压缩-009: 长任务压缩后，追问延续事实

**场景**
用户让 AIjia 阅读真实项目并连续在聊天里输出 RepoWiki 正文，期间明确要求不要写文件、不要只给计划。长上下文触发自动整理后，压缩摘要要保留任务主题、输出约束、关键文件路径和测试设计事实；用户继续追问时，模型能沿着压缩后的上下文回答，而不是只留下压缩边界和 token 数字。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断当前 scope：从 `tauri-pilot aijia where --json` 取当前登录用户 scope，记为 `$SCOPE`
3. 记录现有所有对话 ID：读取 `~/.renlijia/users/$SCOPE/conversations/` 下已有子目录名，记为集合 `$S_BEFORE`
4. 新建空对话：`tauri-pilot aijia new-task`
5. 确认参考项目存在：路径 `/Users/a20250311/github/claude-code-best/.qoder/repowiki/zh/content/核心概念/AI 对话模式/上下文管理.md` 是文件
6. 发送 RepoWiki 第三部分请求：
   ```
   请阅读 /Users/a20250311/github/claude-code-best 中与 /compact、上下文压缩、message/history、compact boundary、tool result trimming、PromptTooLong retry 相关的真实代码和 wiki。

   请直接在聊天回复里输出 RepoWiki 第三部分正文，不要使用 Write 工具，不要写文件，不要摘要，不要计划。

   主题聚焦 /compact、上下文压缩、message/history、compact boundary、项目指令恢复、tool result trimming、PromptTooLong retry、端到端验证。

   每个小节都要有文件路径引用，来源是 /Users/a20250311/github/claude-code-best。

   从标题“# RepoWiki Part 3：上下文压缩与历史重建”开始，中文正文，尽量详细。
   ```
7. 等待 AI 完整回复：`tauri-pilot aijia wait-reply --timeout 900`；随后继续轮询 `tauri-pilot aijia where --json` 的 `messageCount` 与 `messages.jsonl` mtime，直到 30 秒内不再增长
8. 找到本轮新建的对话 ID：在 `~/.renlijia/users/$SCOPE/conversations/` 下取 `mtime` 最新且不在 `$S_BEFORE` 里的子目录名，记为 `$CONV_ID`
9. 发送 RepoWiki 第四部分请求：
   ```
   请继续直接在聊天里输出 RepoWiki 第四部分正文，不要使用 Write 工具，不要写文件，不要摘要，不要计划。

   主题：继续展开 claude-code-best 的上下文压缩工程细节，重点写：

   1. autoCompactIfNeeded / compactConversation / streamCompactSummary 的调用链；
   2. message grouping、history rebuild、boundary/pivot 的数据关系；
   3. PromptTooLong 截断重试策略；
   4. 项目指令、文件附件、skills、MCP instructions 在压缩后的恢复策略；
   5. 与 AIjia 当前 compact-fix 方案可以对齐和不能直接照搬的点。

   要求中文正文、尽量详细、继续带本地文件路径引用，从标题“# RepoWiki Part 4：压缩调用链与恢复策略”开始。
   ```
10. 等待 AI 完整回复：`tauri-pilot aijia wait-reply --timeout 900`；随后继续轮询 `tauri-pilot aijia where --json` 的 `messageCount` 与 `messages.jsonl` mtime，直到 30 秒内不再增长
11. 发送 RepoWiki 第五部分请求：
   ```
   请继续直接在聊天里输出 RepoWiki 第五部分正文，不要使用 Write 工具，不要写文件，不要摘要，不要计划。

   主题：claude-code-best 的上下文压缩测试与验收设计，重点写：

   1. /compact 手动命令与自动压缩分别怎么测；
   2. 历史恢复、boundary、project instructions、MCP/skills/file attachments 的验收标准；
   3. PromptTooLong retry、tool result trimming、message grouping 的失败场景；
   4. 对 AIjia 当前上下文压缩意图测试应该如何设计真实 E2E；
   5. 哪些能力当前 AIjia 已经有，哪些不能写进验收。

   要求中文正文，尽量详细，继续带本地文件路径引用，从标题“# RepoWiki Part 5：上下文压缩测试与验收设计”开始。
   ```
12. 等待 AI 完整回复：`tauri-pilot aijia wait-reply --timeout 1200`；随后继续轮询 `tauri-pilot aijia where --json` 的 `messageCount` 与 `messages.jsonl` mtime，直到 30 秒内不再增长
13. 如果 `~/.renlijia/users/$SCOPE/conversations/$CONV_ID/messages.jsonl` 中还没有 `subtype == "compact_boundary"` 的记录，发送一次补充请求：`请继续输出 RepoWiki Part 5 的剩余测试与验收设计正文，仍然不要使用 Write 工具，不要摘要，不要计划。`，然后重复第 12 步等待
14. 发送压缩后追问：
   ```
   请不要调用工具，只根据本对话历史回复五行：
   标题=<第五部分标题>
   调用链=<autoCompactIfNeeded/compactConversation/streamCompactSummary 三者关系>
   恢复范围=<历史恢复、boundary、project instructions、MCP/skills/file attachments>
   失败场景=<PromptTooLong retry、tool result trimming、message grouping>
   AIjia验收=<当前上下文压缩意图测试要覆盖的真实 E2E 重点>
   ```
15. 等待 AI 完整回复：`tauri-pilot aijia wait-reply --timeout 900`
16. 打开 `~/.renlijia/users/$SCOPE/conversations/$CONV_ID/messages.jsonl`
17. 在 `messages.jsonl` 中定位最新一条 `subtype == "compact_boundary"` 的记录，记为 `$MSG_BOUNDARY`
18. 在 `$MSG_BOUNDARY` 之后定位第一条 `isCompactSummary == true` 的 user 记录，记为 `$SUMMARY_MSG`
19. 在 `messages.jsonl` 中定位第 14 步追问对应的 user 记录，记为 `$FOLLOWUP_USER`
20. 在 `$FOLLOWUP_USER` 之后定位第一条 assistant 记录，记为 `$FOLLOWUP_ASSISTANT`
21. 如果存在 `~/.renlijia/users/$SCOPE/conversations/$CONV_ID/compact_boundaries.jsonl`，打开该文件最后一条记录，记为 `$SIDECAR_RECORD`

**验收标准**

- 文件 `~/.renlijia/users/$SCOPE/conversations/$CONV_ID/messages.jsonl` 存在
- `messages.jsonl` 每条记录都是可解析 JSON
- `$MSG_BOUNDARY.role == "system"`
- `$MSG_BOUNDARY.subtype == "compact_boundary"`
- `$MSG_BOUNDARY.compactMetadata.trigger == "auto"`
- `$MSG_BOUNDARY.compactMetadata.messagesSummarized >= 1`
- `$MSG_BOUNDARY.compactMetadata.preTokens > $MSG_BOUNDARY.compactMetadata.postTokens`
- `$SUMMARY_MSG.role == "user"`
- `$SUMMARY_MSG.isCompactSummary == true`
- `$SUMMARY_MSG.content` 字段 JSON 序列化字符串包含字面值 `<context>`
- `$SUMMARY_MSG.content` 字段 JSON 序列化字符串包含字面值 `RepoWiki Part 5`
- `$SUMMARY_MSG.content` 字段 JSON 序列化字符串包含字面值 `/compact`
- `$SUMMARY_MSG.content` 字段 JSON 序列化字符串包含字面值 `PromptTooLong`
- `$SUMMARY_MSG.content` 字段 JSON 序列化字符串包含字面值 `tool result trimming`
- `$SUMMARY_MSG.content` 字段 JSON 序列化字符串包含字面值 `message grouping`
- `$SUMMARY_MSG.content` 字段 JSON 序列化字符串包含字面值 `不要使用 Write 工具`
- `$SUMMARY_MSG.content` 字段 JSON 序列化字符串包含字面值 `/Users/a20250311/github/claude-code-best`
- `$SUMMARY_MSG.content` 字段 JSON 序列化字符串包含字面值 `AIjia 当前上下文压缩意图测试`
- 如果 `$SIDECAR_RECORD` 存在，`$SIDECAR_RECORD.summary_text` 包含字面值 `RepoWiki Part 5`
- 如果 `$SIDECAR_RECORD` 存在，`$SIDECAR_RECORD.summary_text` 包含字面值 `/compact`
- 如果 `$SIDECAR_RECORD` 存在，`$SIDECAR_RECORD.summary_text` 包含字面值 `PromptTooLong`
- 如果 `$SIDECAR_RECORD` 存在，`$SIDECAR_RECORD.summary_text` 包含字面值 `tool result trimming`
- 如果 `$SIDECAR_RECORD` 存在，`$SIDECAR_RECORD.summary_text` 包含字面值 `message grouping`
- 如果 `$SIDECAR_RECORD` 存在，`$SIDECAR_RECORD.summary_text` 包含字面值 `不要使用 Write 工具`
- `$FOLLOWUP_USER` 位于 `$MSG_BOUNDARY` 之后
- `$FOLLOWUP_ASSISTANT.role == "assistant"`
- `$FOLLOWUP_ASSISTANT` 位于 `$FOLLOWUP_USER` 之后
- `$FOLLOWUP_ASSISTANT.content` 字段 JSON 序列化字符串包含字面值 `标题=RepoWiki Part 5：上下文压缩测试与验收设计`
- `$FOLLOWUP_ASSISTANT.content` 字段 JSON 序列化字符串包含字面值 `autoCompactIfNeeded`
- `$FOLLOWUP_ASSISTANT.content` 字段 JSON 序列化字符串包含字面值 `compactConversation`
- `$FOLLOWUP_ASSISTANT.content` 字段 JSON 序列化字符串包含字面值 `streamCompactSummary`
- `$FOLLOWUP_ASSISTANT.content` 字段 JSON 序列化字符串包含字面值 `boundary`
- `$FOLLOWUP_ASSISTANT.content` 字段 JSON 序列化字符串包含字面值 `project instructions`
- `$FOLLOWUP_ASSISTANT.content` 字段 JSON 序列化字符串包含字面值 `MCP`
- `$FOLLOWUP_ASSISTANT.content` 字段 JSON 序列化字符串包含字面值 `skills`
- `$FOLLOWUP_ASSISTANT.content` 字段 JSON 序列化字符串包含字面值 `file attachments`
- `$FOLLOWUP_ASSISTANT.content` 字段 JSON 序列化字符串包含字面值 `PromptTooLong retry`
- `$FOLLOWUP_ASSISTANT.content` 字段 JSON 序列化字符串包含字面值 `tool result trimming`
- `$FOLLOWUP_ASSISTANT.content` 字段 JSON 序列化字符串包含字面值 `message grouping`
- `$FOLLOWUP_ASSISTANT.content` 字段 JSON 序列化字符串不包含字面值 `我不记得`
- `$FOLLOWUP_ASSISTANT.content` 字段 JSON 序列化字符串不包含字面值 `无法根据本对话历史`

---

## 意图-上下文压缩-010: 上传图片后压缩，摘要保留图片标记

**场景**
用户在对话里上传图片并附带文字说明，随后继续长聊触发自动整理。应用在压缩摘要链路中把用户图片内容降级为 `[image]` 文本标记，同时保留用户围绕图片给出的文字说明。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断当前 scope：从 `tauri-pilot aijia where --json` 取当前登录用户 scope，记为 `$SCOPE`
3. 记录现有所有对话 ID：读取 `~/.renlijia/users/$SCOPE/conversations/` 下已有子目录名，记为集合 `$S_BEFORE`
4. 新建空对话：`tauri-pilot aijia new-task`
5. 写入测试图片 `/tmp/aijia-context-compact-image.png`：该文件是一个可打开的 PNG 图片
6. 将测试图片加入 composer 附件队列：`tauri-pilot aijia composer-queue-files --paths /tmp/aijia-context-compact-image.png`
7. 点击 composer 的「+」按钮消费附件队列：`tauri-pilot aijia composer-click-plus`
8. 输入图片说明文字：
   ```
   这张图片用于上下文压缩图片验收，图片标记是 IMAGE-COMPACT-MARKER-ALPHA。请只回复：图片已收到。
   ```
9. 发送消息：`tauri-pilot aijia send`
10. 等待 AI 完整回复：`tauri-pilot aijia wait-reply --timeout 600`
11. 找到本轮新建的对话 ID：在 `~/.renlijia/users/$SCOPE/conversations/` 下取 `mtime` 最新且不在 `$S_BEFORE` 里的子目录名，记为 `$CONV_ID`
12. 打开 `~/.renlijia/users/$SCOPE/conversations/$CONV_ID/messages.jsonl`，定位包含字面值 `IMAGE-COMPACT-MARKER-ALPHA` 的 user 记录，记为 `$IMAGE_USER`
13. 准备压力文本 `/tmp/aijia-context-compact-image-block.txt`：写入至少 500 行固定句子，每行包含字面值 `CTX-COMPACT-IMAGE-PRESSURE` 和行号，文件大小在 `45 KB` 到 `55 KB` 之间
14. 在同一对话中连续执行 4 轮发送；第 N 轮先把下面这段文字输入到输入框，再将 `/tmp/aijia-context-compact-image-block.txt` 的完整内容按 `<= 20 KB` 分块追加到同一个输入框，全部追加完成后点击发送：
   ```
   这是图片历史压缩压力轮 N。请只回复：已收到 IMAGE-COMPACT-ROUND-N。不要调用工具，不要总结压力文本。
   ```
15. 每轮发送后等待 AI 完整回复：`tauri-pilot aijia wait-reply --timeout 600`
16. 如果 `~/.renlijia/users/$SCOPE/conversations/$CONV_ID/messages.jsonl` 中还没有 `subtype == "compact_boundary"` 的记录，最多追加 2 轮与第 14 步相同格式的压力消息；每轮后等待 `tauri-pilot aijia wait-reply --timeout 600`
17. 发送追问消息：`请根据前文只回复：图片标记=IMAGE-COMPACT-MARKER-ALPHA`
18. 等待 AI 完整回复：`tauri-pilot aijia wait-reply --timeout 600`
19. 打开 `~/.renlijia/users/$SCOPE/conversations/$CONV_ID/messages.jsonl`
20. 在 `messages.jsonl` 中定位最新一条 `subtype == "compact_boundary"` 的记录，记为 `$MSG_BOUNDARY`
21. 在 `$MSG_BOUNDARY` 之后定位第一条 `isCompactSummary == true` 的 user 记录，记为 `$SUMMARY_MSG`
22. 在 `messages.jsonl` 中定位第 17 步追问对应的 user 记录，记为 `$FOLLOWUP_USER`
23. 在 `$FOLLOWUP_USER` 之后定位第一条 assistant 记录，记为 `$FOLLOWUP_ASSISTANT`

**验收标准**

- 文件 `/tmp/aijia-context-compact-image.png` 存在
- 文件 `~/.renlijia/users/$SCOPE/conversations/$CONV_ID/messages.jsonl` 存在
- `messages.jsonl` 每条记录都是可解析 JSON
- `$IMAGE_USER.role == "user"`
- `$IMAGE_USER.content` 字段 JSON 序列化字符串包含字面值 `IMAGE-COMPACT-MARKER-ALPHA`
- `$IMAGE_USER.files.length >= 1`
- `$IMAGE_USER.files[0].fileType == "image"`
- `$IMAGE_USER.files[0].kind == "image"`
- `$MSG_BOUNDARY.role == "system"`
- `$MSG_BOUNDARY.subtype == "compact_boundary"`
- `$MSG_BOUNDARY.compactMetadata.trigger == "auto"`
- `$SUMMARY_MSG.role == "user"`
- `$SUMMARY_MSG.isCompactSummary == true`
- `$SUMMARY_MSG.content` 字段 JSON 序列化字符串包含字面值 `<context>`
- `$SUMMARY_MSG.content` 字段 JSON 序列化字符串包含字面值 `[image]`
- `$SUMMARY_MSG.content` 字段 JSON 序列化字符串包含字面值 `IMAGE-COMPACT-MARKER-ALPHA`
- `$SUMMARY_MSG.content` 字段 JSON 序列化字符串不包含字面值 `data:image/`
- `$SUMMARY_MSG.content` 字段 JSON 序列化字符串不包含字面值 `base64,`
- `$FOLLOWUP_USER` 位于 `$MSG_BOUNDARY` 之后
- `$FOLLOWUP_ASSISTANT.role == "assistant"`
- `$FOLLOWUP_ASSISTANT` 位于 `$FOLLOWUP_USER` 之后
- `$FOLLOWUP_ASSISTANT.content` 字段 JSON 序列化字符串包含字面值 `图片标记=IMAGE-COMPACT-MARKER-ALPHA`
- `$FOLLOWUP_ASSISTANT.content` 字段 JSON 序列化字符串不包含字面值 `CTX-COMPACT-IMAGE-PRESSURE`

---
