# rules.md — 上下文压缩

本 task 测的产品承诺：**长对话接近上下文上限时，应用会自动整理旧历史，并在界面和会话 transcript 中留下可核验边界；压缩前不能用本地轮数或字符预算先丢弃旧事实；压缩后要保留摘要、最近消息、项目指令和后续对话能力，让用户能继续当前对话且不会误解已排除事项**。

本 task 同时看三层证据：

- **用户可见层**：对话进行中状态「整理上下文中…」、消息列表中的「对话已压缩」边界条、边界条展开后的「压缩前 / 压缩后 / 摘要消息」信息、压缩后的继续回复。
- **会话持久化层**：`messages.jsonl` 中的 `compact_boundary` 系统消息、压缩摘要消息、压缩边界之后的新 user / assistant 消息。
- **深度 schema 层**：`messages.jsonl` 中的 compact boundary 是主证据；`compact_boundaries.jsonl` 如果存在，只作为兼容索引与 transcript metadata 做一致性校验；凡是声明“延续对话能力”的意图，必须同时验证摘要内容和压缩后追问内容，不只验证 boundary 结构。

本 task 不测 cargo / Rust 集成测试，不用 mock LLM，不验证 microcompact / autocompact / collapse 的内部执行顺序，也不把某个固定 token 阈值写成用户承诺。PromptTooLong recovery 属后端异常恢复路径，当前没有可由 `tauri-pilot aijia` 稳定触发的真实 provider 前置；该能力用 Rust 回归测试覆盖，不放进本 AEIT task 的可执行意图。

cc-best 对齐矩阵：

| 能力面 | 本 task 意图 | 证据口径 |
|---|---|---|
| 自动压缩 auto compact | 001、002、003、005、006、007、011、012、013、014 | UI 边界条 + `messages.jsonl` 的 `system/compact_boundary` + compact summary |
| 摘要质量与早期事实保留 | 002、011、012、013 | summary 与压缩后追问都保留关键事实、已排除误判、工具证据和下一步 |
| reload 恢复 | 003 | 重开会话后 boundary 仍来自 transcript，不依赖内存态 |
| tool pairing / tool artifact | 007、013 | assistant toolCall 与 tool result 之间不被 boundary 切断；长工具输出落到 `tool-results/manifest.jsonl` 后，summary 仍能引用 artifact 全量证据 |
| 项目指令重注入 | 004 | compact 后下一轮仍遵守 `AGENTS.md` |
| 原始对话路径注入 | 014 | compact summary 中的完整对话记录路径是当前会话 `messages.jsonl` 的全路径，且文件可打开 |

压力触发口径：本 task 的每条意图都在真实账号目录下运行。每条意图在推断 `$SCOPE` 后，先读取 `~/.renlijia/users/$SCOPE/config.json` 的 `contextWindow` 原值，记为 `$CONTEXT_WINDOW_BEFORE`，再把 `contextWindow` 临时写为字符串 `"64000"`；该值会让 auto-compact 触发线固定到约 124000 chars。意图结束前必须恢复 `$CONTEXT_WINDOW_BEFORE`。压力文本单轮要低于触发线，多轮累计后进入触发线，验证的是“长历史接近当前窗口上限时会自动整理”，不是单条超大消息自触发。

压力文本输入口径：`tauri-pilot aijia type-message` 单次插入超大文本可能触发 WebView eval timeout。每轮压力消息用多次 `type-message` 追加到同一个 composer，每次追加文本控制在 `<= 20 KB`，全部追加完成后再执行一次 `tauri-pilot aijia send`。

UI 读取口径：`tauri-pilot aijia ui-message --json` 读取的是 `chatStore.messages` 原始记录，可用于检查 `system/compact_boundary` 和 `isCompactSummary`；压缩边界的用户可见状态使用 `tauri-pilot aijia compact-boundary-snapshot --json`；展开压缩边界条使用 `tauri-pilot aijia compact-boundary-toggle --index -1 --json`。

配置恢复口径：runner 如果用 shell 脚本执行压力轮，必须用 `trap` 做异常恢复，不能把真实用户配置永久留在 `"64000"`。

transcript 解析口径：`messages.jsonl` 和 `compact_boundaries.jsonl` 的一条记录都以 `\t✓\n` 分隔，解析时先按该分隔符拆记录，再对每条记录取 `\t` 前的 JSON 前缀解析。验收标准里的“记录”都指这个解析后的 JSON 记录，不按字面换行计数。

完整对话记录路径口径：compact summary 中「完整的对话记录在：」后面的路径必须是当前会话 `messages.jsonl` 的系统全路径。runner 先把 `~/.renlijia/users/$SCOPE/conversations/$CONV_ID/messages.jsonl` 解析成系统绝对路径，记为 `$EXPECTED_TRANSCRIPT_PATH`；再从 `$SUMMARY_MSG.content` 提取路径，记为 `$SUMMARY_TRANSCRIPT_PATH`；两者必须完全相等，且 `$SUMMARY_TRANSCRIPT_PATH` 指向的文件存在。

质量验收口径：仅出现 `compact_boundary`、`isCompactSummary` 或「对话已压缩」不代表压缩质量合格。只要意图场景中声明了关键事实、排除项、下一步或项目约束，验收标准必须分别检查 `$SUMMARY_MSG.content` 与 `$FOLLOWUP_ASSISTANT.content` 是否包含对应字面标记，并检查 follow-up 不包含压力文本标记、重新索要前文或把已排除方向当结论的文本。

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

## 意图-上下文压缩-002: 压缩后继续追问，事实仍被引用

**场景**
用户的长对话已经被自动整理。压缩不是只让后续消息接在边界之后；用户继续追问时，摘要必须支撑模型引用压缩前的关键事实，并明确排除压力文本不是用户需求。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断当前 scope：从 `tauri-pilot aijia where --json` 取当前登录用户 scope，记为 `$SCOPE`
3. 记录现有所有对话 ID：读取 `~/.renlijia/users/$SCOPE/conversations/` 下已有子目录名，记为集合 `$S_BEFORE`
4. 新建空对话：`tauri-pilot aijia new-task`
5. 准备压力文本 `/tmp/aijia-context-compact-followup-block.txt`：写入至少 500 行固定句子，每行包含字面值 `CTX-COMPACT-FOLLOWUP-PRESSURE` 和行号，文件大小在 `45 KB` 到 `55 KB` 之间
6. 连续执行 4 轮发送；第 N 轮先把下面这段文字输入到输入框，再将 `/tmp/aijia-context-compact-followup-block.txt` 的完整内容按 `<= 20 KB` 分块追加到同一个输入框，全部追加完成后点击发送：
   ```
   这是上下文压缩后续追问压力轮 N。
   关键事实=客户名称是星河试点，标记 FOLLOWUP-COMPACT-FACT-ALPHA。
   已排除误判=压力文本不是用户需求，标记 FOLLOWUP-COMPACT-EXCLUDE-PRESSURE。
   请只回复：已收到 FOLLOWUP-COMPACT-ROUND-N。不要调用工具，不要总结压力文本。
   ```
7. 每轮发送后等待 AI 完整回复：`tauri-pilot aijia wait-reply --timeout 600`
8. 找到本轮新建的对话 ID：在 `~/.renlijia/users/$SCOPE/conversations/` 下取 `mtime` 最新且不在 `$S_BEFORE` 里的子目录名，记为 `$CONV_ID`
9. 如果 `~/.renlijia/users/$SCOPE/conversations/$CONV_ID/messages.jsonl` 中还没有 `subtype == "compact_boundary"` 的记录，最多追加 2 轮与第 6 步相同格式的压力消息；每轮后等待 `tauri-pilot aijia wait-reply --timeout 600`
10. 发送追问消息：`请根据前文只回复三行：客户=；已排除=；继续标记=FOLLOWUP-COMPACT-ANSWER-OK`
11. 等待 AI 完整回复：`tauri-pilot aijia wait-reply --timeout 600`
12. 打开 `~/.renlijia/users/$SCOPE/conversations/$CONV_ID/messages.jsonl`
13. 在 `messages.jsonl` 中定位最新一条 `subtype == "compact_boundary"` 的记录，记为 `$MSG_BOUNDARY`
14. 在 `$MSG_BOUNDARY` 之后定位第一条 `isCompactSummary == true` 的 user 记录，记为 `$SUMMARY_MSG`
15. 在 `messages.jsonl` 中定位第 10 步追问对应的 user 记录，记为 `$FOLLOWUP_USER`
16. 在 `$FOLLOWUP_USER` 之后定位第一条 assistant 记录，记为 `$FOLLOWUP_ASSISTANT`
17. 查看当前对话 UI 消息列表

**验收标准**

- 文件 `~/.renlijia/users/$SCOPE/conversations/$CONV_ID/messages.jsonl` 存在
- `messages.jsonl` 每条记录都是可解析 JSON
- `$MSG_BOUNDARY.role == "system"`
- `$MSG_BOUNDARY.subtype == "compact_boundary"`
- `$MSG_BOUNDARY.compactMetadata.trigger == "auto"`
- `$SUMMARY_MSG.role == "user"`
- `$SUMMARY_MSG.isCompactSummary == true`
- `$SUMMARY_MSG.content` 字段 JSON 序列化字符串包含字面值 `<context>`
- `$SUMMARY_MSG.content` 字段 JSON 序列化字符串包含字面值 `FOLLOWUP-COMPACT-FACT-ALPHA`
- `$SUMMARY_MSG.content` 字段 JSON 序列化字符串包含字面值 `FOLLOWUP-COMPACT-EXCLUDE-PRESSURE`
- `$FOLLOWUP_USER` 位于 `$MSG_BOUNDARY` 之后
- `$FOLLOWUP_USER.content` 字段 JSON 序列化字符串包含字面值 `FOLLOWUP-COMPACT-ANSWER-OK`
- `$FOLLOWUP_ASSISTANT.role == "assistant"`
- `$FOLLOWUP_ASSISTANT` 位于 `$FOLLOWUP_USER` 之后
- `$FOLLOWUP_ASSISTANT.content` 字段 JSON 序列化字符串包含字面值 `FOLLOWUP-COMPACT-ANSWER-OK`
- `$FOLLOWUP_ASSISTANT.content` 字段 JSON 序列化字符串包含字面值 `客户=星河试点`
- `$FOLLOWUP_ASSISTANT.content` 字段 JSON 序列化字符串包含字面值 `FOLLOWUP-COMPACT-FACT-ALPHA`
- `$FOLLOWUP_ASSISTANT.content` 字段 JSON 序列化字符串包含字面值 `FOLLOWUP-COMPACT-EXCLUDE-PRESSURE`
- `$MSG_BOUNDARY` 之后至少存在 2 条 `isCompactSummary != true` 的普通消息
- `$FOLLOWUP_ASSISTANT.content` 字段 JSON 序列化字符串不包含字面值 `CTX-COMPACT-FOLLOWUP-PRESSURE`
- `$FOLLOWUP_ASSISTANT.content` 字段 JSON 序列化字符串不包含字面值 `压力文本是用户需求`
- `$FOLLOWUP_ASSISTANT.content` 字段 JSON 序列化字符串不包含字面值 `请重新提供前文`
- `$FOLLOWUP_ASSISTANT.content` 字段 JSON 序列化字符串不包含字面值 `我不记得之前的内容`
- `$FOLLOWUP_ASSISTANT.content` 字段 JSON 序列化字符串不包含字面值 `context length exceeded`
- `$FOLLOWUP_ASSISTANT.content` 字段 JSON 序列化字符串不包含字面值 `too many tokens`
- `$FOLLOWUP_ASSISTANT.content` 字段 JSON 序列化字符串不包含字面值 `PromptTooLong`
- `$FOLLOWUP_ASSISTANT.content` 字段 JSON 序列化字符串不包含字面值 `上下文过长，请压缩后重试`

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

## 意图-上下文压缩-011: 压缩后追问，关键事实保留

**场景**
用户在一次真实排障交接型长对话中，先把已确认事实、已排除误判、风险和下一步混在多轮上下文里交给 AI。长历史触发自动整理后，用户继续追问验收报告；合格标准不是只出现压缩边界，而是摘要能支撑后续回答继续引用关键事实，并且不会把已排除方向当成结论。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断当前 scope：从 `tauri-pilot aijia where --json` 取当前登录用户 scope，记为 `$SCOPE`
3. 记录现有所有对话 ID：读取 `~/.renlijia/users/$SCOPE/conversations/` 下已有子目录名，记为集合 `$S_BEFORE`
4. 读取 `~/.renlijia/users/$SCOPE/config.json` 的 `contextWindow` 原值；如果字段存在，记 `$CONFIG_CONTEXT_WINDOW_WAS_PRESENT == true` 且原值为 `$CONTEXT_WINDOW_BEFORE`；如果字段不存在，记 `$CONFIG_CONTEXT_WINDOW_WAS_PRESENT == false`
5. 临时把 `~/.renlijia/users/$SCOPE/config.json` 的 `contextWindow` 写为字符串 `"64000"`
6. 新建空对话：`tauri-pilot aijia new-task`
7. 准备压力文本 `/tmp/aijia-context-compact-quality-block.txt`：写入至少 500 行固定句子，每行包含字面值 `CTX-COMPACT-QUALITY-PRESSURE` 和行号，文件大小在 `45 KB` 到 `55 KB` 之间
8. 在同一对话中连续执行 4 轮发送；第 N 轮先把下面这段排障交接材料输入到输入框，再将 `/tmp/aijia-context-compact-quality-block.txt` 的完整内容按 `<= 20 KB` 分块追加到同一个输入框，全部追加完成后点击发送：
   ```
   这是自动压缩质量验收排障交接轮 N。
   用户目标=自动压缩质量验证，标记 QUALITY-COMPACT-GOAL-AUTO-ONLY。
   已确认事实=v2 网关已经出现非流式 summary 调用日志，标记 QUALITY-COMPACT-EVIDENCE-V2-NONSTREAM。
   已确认事实=当前分支已合并 main，合并提交标记 QUALITY-COMPACT-MERGE-80F32CAE。
   已排除误判=手动压缩路径不能代表自动压缩质量，标记 QUALITY-COMPACT-EXCLUDE-MANUAL-PATH。
   下一步=优先验证自动压缩触发后的追问是否保留关键事实，标记 QUALITY-COMPACT-NEXT-TRIGGER-FIRST。
   请只回复：已收到 QUALITY-COMPACT-ROUND-N。不要调用工具，不要总结压力文本。
   ```
9. 每轮发送后等待 AI 完整回复：`tauri-pilot aijia wait-reply --timeout 600`
10. 找到本轮新建的对话 ID：在 `~/.renlijia/users/$SCOPE/conversations/` 下取 `mtime` 最新且不在 `$S_BEFORE` 里的子目录名，记为 `$CONV_ID`
11. 如果 `~/.renlijia/users/$SCOPE/conversations/$CONV_ID/messages.jsonl` 中还没有 `subtype == "compact_boundary"` 的记录，最多追加 2 轮与第 8 步相同格式的压力消息；每轮后等待 `tauri-pilot aijia wait-reply --timeout 600`
12. 发送追问消息：`请根据前文输出压缩质量验收报告，只回复以下字段：目标=；已确认事实=；已排除误判=；下一步=；质量判断=QUALITY-COMPACT-FOLLOWUP-OK`
13. 等待 AI 完整回复：`tauri-pilot aijia wait-reply --timeout 600`
14. 打开 `~/.renlijia/users/$SCOPE/conversations/$CONV_ID/messages.jsonl`
15. 在 `messages.jsonl` 中定位最新一条 `subtype == "compact_boundary"` 的记录，记为 `$MSG_BOUNDARY`
16. 在 `$MSG_BOUNDARY` 之后定位第一条 `isCompactSummary == true` 的 user 记录，记为 `$SUMMARY_MSG`
17. 在 `messages.jsonl` 中定位第 12 步追问对应的 user 记录，记为 `$FOLLOWUP_USER`
18. 在 `$FOLLOWUP_USER` 之后定位第一条 assistant 记录，记为 `$FOLLOWUP_ASSISTANT`
19. 恢复 `~/.renlijia/users/$SCOPE/config.json`：如果 `$CONFIG_CONTEXT_WINDOW_WAS_PRESENT == true`，把 `contextWindow` 恢复为 `$CONTEXT_WINDOW_BEFORE`；如果 `$CONFIG_CONTEXT_WINDOW_WAS_PRESENT == false`，移除 `contextWindow` 字段

**验收标准**

- 文件 `/tmp/aijia-context-compact-quality-block.txt` 存在
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
- `$SUMMARY_MSG.content` 字段 JSON 序列化字符串包含字面值 `QUALITY-COMPACT-GOAL-AUTO-ONLY`
- `$SUMMARY_MSG.content` 字段 JSON 序列化字符串包含字面值 `QUALITY-COMPACT-EVIDENCE-V2-NONSTREAM`
- `$SUMMARY_MSG.content` 字段 JSON 序列化字符串包含字面值 `QUALITY-COMPACT-MERGE-80F32CAE`
- `$SUMMARY_MSG.content` 字段 JSON 序列化字符串包含字面值 `QUALITY-COMPACT-EXCLUDE-MANUAL-PATH`
- `$SUMMARY_MSG.content` 字段 JSON 序列化字符串包含字面值 `QUALITY-COMPACT-NEXT-TRIGGER-FIRST`
- `$FOLLOWUP_USER` 位于 `$MSG_BOUNDARY` 之后
- `$FOLLOWUP_ASSISTANT.role == "assistant"`
- `$FOLLOWUP_ASSISTANT` 位于 `$FOLLOWUP_USER` 之后
- `$FOLLOWUP_ASSISTANT.content` 字段 JSON 序列化字符串包含字面值 `QUALITY-COMPACT-FOLLOWUP-OK`
- `$FOLLOWUP_ASSISTANT.content` 字段 JSON 序列化字符串包含字面值 `目标=自动压缩质量验证`
- `$FOLLOWUP_ASSISTANT.content` 字段 JSON 序列化字符串包含字面值 `QUALITY-COMPACT-GOAL-AUTO-ONLY`
- `$FOLLOWUP_ASSISTANT.content` 字段 JSON 序列化字符串包含字面值 `QUALITY-COMPACT-EVIDENCE-V2-NONSTREAM`
- `$FOLLOWUP_ASSISTANT.content` 字段 JSON 序列化字符串包含字面值 `QUALITY-COMPACT-MERGE-80F32CAE`
- `$FOLLOWUP_ASSISTANT.content` 字段 JSON 序列化字符串包含字面值 `QUALITY-COMPACT-EXCLUDE-MANUAL-PATH`
- `$FOLLOWUP_ASSISTANT.content` 字段 JSON 序列化字符串包含字面值 `QUALITY-COMPACT-NEXT-TRIGGER-FIRST`
- `$FOLLOWUP_ASSISTANT.content` 字段 JSON 序列化字符串不包含字面值 `手动压缩失败所以自动压缩失败`
- `$FOLLOWUP_ASSISTANT.content` 字段 JSON 序列化字符串不包含字面值 `我不记得`
- `$FOLLOWUP_ASSISTANT.content` 字段 JSON 序列化字符串不包含字面值 `请重新提供`
- `$FOLLOWUP_ASSISTANT.content` 字段 JSON 序列化字符串不包含字面值 `CTX-COMPACT-QUALITY-PRESSURE`
- 如果 `$CONFIG_CONTEXT_WINDOW_WAS_PRESENT == true`，`~/.renlijia/users/$SCOPE/config.json` 的 `contextWindow == $CONTEXT_WINDOW_BEFORE`
- 如果 `$CONFIG_CONTEXT_WINDOW_WAS_PRESENT == false`，`~/.renlijia/users/$SCOPE/config.json` 中不存在字段 `contextWindow`

---

## 意图-上下文压缩-012: 超过三十轮后，早期事实保留

**场景**
用户在同一对话里先交代一个很早的已排除误判，然后继续聊超过三十轮，最后长历史触发自动整理。压缩不能在整理前先把最早几轮丢掉；压缩摘要和后续追问都必须还能引用第一轮的已排除事实。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断当前 scope：从 `tauri-pilot aijia where --json` 取当前登录用户 scope，记为 `$SCOPE`
3. 记录现有所有对话 ID：读取 `~/.renlijia/users/$SCOPE/conversations/` 下已有子目录名，记为集合 `$S_BEFORE`
4. 读取 `~/.renlijia/users/$SCOPE/config.json` 的 `contextWindow` 原值；如果字段存在，记 `$CONFIG_CONTEXT_WINDOW_WAS_PRESENT == true` 且原值为 `$CONTEXT_WINDOW_BEFORE`；如果字段不存在，记 `$CONFIG_CONTEXT_WINDOW_WAS_PRESENT == false`
5. 临时把 `~/.renlijia/users/$SCOPE/config.json` 的 `contextWindow` 写为字符串 `"64000"`
6. 新建空对话：`tauri-pilot aijia new-task`
7. 发送第 1 轮早期事实消息：
   ```
   这是超过三十轮压缩质量验收的第 1 轮。
   早期事实=登录页白屏已排除，标记 ROUND30-COMPACT-EARLY-EXCLUSION。
   验证对象=只验证自动压缩，不验证手动命令，标记 ROUND30-COMPACT-AUTO-ONLY。
   请只回复：已记录 ROUND30-COMPACT-EARLY-1。不要调用工具。
   ```
8. 等待 AI 完整回复：`tauri-pilot aijia wait-reply --timeout 600`
9. 连续发送第 2 到第 35 轮小消息；第 N 轮消息格式为：`这是超过三十轮压缩质量验收的第 N 轮，保持上下文但不要总结。请只回复：已记录 ROUND30-COMPACT-FILLER-N。不要调用工具。`
10. 每轮发送后等待 AI 完整回复：`tauri-pilot aijia wait-reply --timeout 600`
11. 找到本轮新建的对话 ID：在 `~/.renlijia/users/$SCOPE/conversations/` 下取 `mtime` 最新且不在 `$S_BEFORE` 里的子目录名，记为 `$CONV_ID`
12. 打开 `~/.renlijia/users/$SCOPE/conversations/$CONV_ID/messages.jsonl`，定位包含字面值 `ROUND30-COMPACT-EARLY-EXCLUSION` 的 user 记录，记为 `$EARLY_USER`
13. 准备压力文本 `/tmp/aijia-context-compact-round30-block.txt`：写入至少 500 行固定句子，每行包含字面值 `CTX-COMPACT-ROUND30-PRESSURE` 和行号，文件大小在 `45 KB` 到 `55 KB` 之间
14. 在同一对话中连续执行 4 轮发送；第 N 轮先把下面这段文字输入到输入框，再将 `/tmp/aijia-context-compact-round30-block.txt` 的完整内容按 `<= 20 KB` 分块追加到同一个输入框，全部追加完成后点击发送：
   ```
   这是超过三十轮后的自动压缩压力轮 N。
   请保留早期事实 ROUND30-COMPACT-EARLY-EXCLUSION 和验证对象 ROUND30-COMPACT-AUTO-ONLY。
   请只回复：已收到 ROUND30-COMPACT-PRESSURE-N。不要调用工具，不要总结压力文本。
   ```
15. 每轮发送后等待 AI 完整回复：`tauri-pilot aijia wait-reply --timeout 600`
16. 如果 `~/.renlijia/users/$SCOPE/conversations/$CONV_ID/messages.jsonl` 中还没有 `subtype == "compact_boundary"` 的记录，最多追加 2 轮与第 14 步相同格式的压力消息；每轮后等待 `tauri-pilot aijia wait-reply --timeout 600`
17. 发送追问消息：`请根据前文只回复四行：早期事实=；验证对象=；已排除误判=；质量判断=ROUND30-COMPACT-FOLLOWUP-OK`
18. 等待 AI 完整回复：`tauri-pilot aijia wait-reply --timeout 600`
19. 打开 `~/.renlijia/users/$SCOPE/conversations/$CONV_ID/messages.jsonl`
20. 在 `messages.jsonl` 中定位最新一条 `subtype == "compact_boundary"` 的记录，记为 `$MSG_BOUNDARY`
21. 在 `$MSG_BOUNDARY` 之后定位第一条 `isCompactSummary == true` 的 user 记录，记为 `$SUMMARY_MSG`
22. 在 `messages.jsonl` 中定位第 17 步追问对应的 user 记录，记为 `$FOLLOWUP_USER`
23. 在 `$FOLLOWUP_USER` 之后定位第一条 assistant 记录，记为 `$FOLLOWUP_ASSISTANT`
24. 恢复 `~/.renlijia/users/$SCOPE/config.json`：如果 `$CONFIG_CONTEXT_WINDOW_WAS_PRESENT == true`，把 `contextWindow` 恢复为 `$CONTEXT_WINDOW_BEFORE`；如果 `$CONFIG_CONTEXT_WINDOW_WAS_PRESENT == false`，移除 `contextWindow` 字段

**验收标准**

- 文件 `/tmp/aijia-context-compact-round30-block.txt` 存在
- 文件 `~/.renlijia/users/$SCOPE/conversations/$CONV_ID/messages.jsonl` 存在
- `messages.jsonl` 每条记录都是可解析 JSON
- `$EARLY_USER.role == "user"`
- `$EARLY_USER.content` 字段 JSON 序列化字符串包含字面值 `ROUND30-COMPACT-EARLY-EXCLUSION`
- `$EARLY_USER` 位于 `$MSG_BOUNDARY` 之前
- `$MSG_BOUNDARY.role == "system"`
- `$MSG_BOUNDARY.subtype == "compact_boundary"`
- `$MSG_BOUNDARY.compactMetadata.trigger == "auto"`
- `$MSG_BOUNDARY.compactMetadata.messagesSummarized >= 1`
- `$MSG_BOUNDARY.compactMetadata.preTokens > $MSG_BOUNDARY.compactMetadata.postTokens`
- `$SUMMARY_MSG.role == "user"`
- `$SUMMARY_MSG.isCompactSummary == true`
- `$SUMMARY_MSG.content` 字段 JSON 序列化字符串包含字面值 `<context>`
- `$SUMMARY_MSG.content` 字段 JSON 序列化字符串包含字面值 `ROUND30-COMPACT-EARLY-EXCLUSION`
- `$SUMMARY_MSG.content` 字段 JSON 序列化字符串包含字面值 `ROUND30-COMPACT-AUTO-ONLY`
- `$FOLLOWUP_USER` 位于 `$MSG_BOUNDARY` 之后
- `$FOLLOWUP_ASSISTANT.role == "assistant"`
- `$FOLLOWUP_ASSISTANT` 位于 `$FOLLOWUP_USER` 之后
- `$FOLLOWUP_ASSISTANT.content` 字段 JSON 序列化字符串包含字面值 `ROUND30-COMPACT-FOLLOWUP-OK`
- `$FOLLOWUP_ASSISTANT.content` 字段 JSON 序列化字符串包含字面值 `登录页白屏已排除`
- `$FOLLOWUP_ASSISTANT.content` 字段 JSON 序列化字符串包含字面值 `ROUND30-COMPACT-EARLY-EXCLUSION`
- `$FOLLOWUP_ASSISTANT.content` 字段 JSON 序列化字符串包含字面值 `ROUND30-COMPACT-AUTO-ONLY`
- `$FOLLOWUP_ASSISTANT.content` 字段 JSON 序列化字符串不包含字面值 `登录页白屏仍是原因`
- `$FOLLOWUP_ASSISTANT.content` 字段 JSON 序列化字符串不包含字面值 `请重新提供`
- `$FOLLOWUP_ASSISTANT.content` 字段 JSON 序列化字符串不包含字面值 `我不记得`
- `$FOLLOWUP_ASSISTANT.content` 字段 JSON 序列化字符串不包含字面值 `CTX-COMPACT-ROUND30-PRESSURE`
- 如果 `$CONFIG_CONTEXT_WINDOW_WAS_PRESENT == true`，`~/.renlijia/users/$SCOPE/config.json` 的 `contextWindow == $CONTEXT_WINDOW_BEFORE`
- 如果 `$CONFIG_CONTEXT_WINDOW_WAS_PRESENT == false`，`~/.renlijia/users/$SCOPE/config.json` 中不存在字段 `contextWindow`

---

## 意图-上下文压缩-013: 长工具输出 artifact 化后，摘要和追问保留工具证据

**场景**
用户让 AI 读取一个很长的本地文件，关键事实位于工具输出预览之外。合格行为不是把旧工具结果替换成不可恢复的 `[budget-trimmed]` / `[microcompacted]`，也不是只保留 toolCall 配对；应用必须把完整工具输出保存到 `tool-results` artifact，并在自动压缩 summary 调用时读回 artifact 证据，使压缩摘要和后续追问都能引用预览之外的关键事实。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断当前 scope：从 `tauri-pilot aijia where --json` 取当前登录用户 scope，记为 `$SCOPE`
3. 记录现有所有对话 ID：读取 `~/.renlijia/users/$SCOPE/conversations/` 下已有子目录名，记为集合 `$S_BEFORE`
4. 读取 `~/.renlijia/users/$SCOPE/config.json` 的 `contextWindow` 原值；如果字段存在，记 `$CONFIG_CONTEXT_WINDOW_WAS_PRESENT == true` 且原值为 `$CONTEXT_WINDOW_BEFORE`；如果字段不存在，记 `$CONFIG_CONTEXT_WINDOW_WAS_PRESENT == false`
5. 临时把 `~/.renlijia/users/$SCOPE/config.json` 的 `contextWindow` 写为字符串 `"64000"`
6. 准备测试工作区目录 `/tmp/aijia-context-compact-tool-artifact`
7. 在该目录写入文件 `compact-tool-artifact-source.txt`：第一行包含字面值 `TOOL-ARTIFACT-CASE-ID=TA-2026-0604`，中间写入至少 1500 行固定填充文本，最后一行包含字面值 `TOOL-ARTIFACT-TAIL-DECISION=KEEP-REMOTE-LOGS`；文件大小至少 `90 KB`
8. 读取 `tauri-pilot aijia --help`，确认存在可选择本地工作区的 workspace 子命令；如果缺失，按环境前置不满足记录 `SKIPPED`
9. 通过 CLI 选中测试工作区：`tauri-pilot aijia workspace-queue-path /tmp/aijia-context-compact-tool-artifact`，再执行 `tauri-pilot aijia workspace-open-picker` 和 `tauri-pilot aijia workspace-pick --variant other`
10. 新建空对话：`tauri-pilot aijia new-task`
11. 发送文件读取请求：`请读取当前工作区的 compact-tool-artifact-source.txt，确认第一行案例 ID 和最后一行尾部决定。只回复：TOOL-ARTIFACT-READ-OK 案例=；尾部决定=`
12. 等待 AI 完整回复：`tauri-pilot aijia wait-reply --timeout 600`
13. 找到本轮新建的对话 ID：在 `~/.renlijia/users/$SCOPE/conversations/` 下取 `mtime` 最新且不在 `$S_BEFORE` 里的子目录名，记为 `$CONV_ID`
14. 等待工具链稳定：持续轮询 `tauri-pilot aijia where --json` 的 `messageCount` 与 `~/.renlijia/users/$SCOPE/conversations/$CONV_ID/messages.jsonl` 的 mtime，直到 30 秒内不再增长
15. 打开 `~/.renlijia/users/$SCOPE/conversations/$CONV_ID/messages.jsonl`，定位包含 `TOOL-ARTIFACT-READ-OK` 的 assistant 记录，记为 `$READ_ASSISTANT`
16. 定位读取文件产生的 tool 记录，记为 `$TOOL_MSG`；如果存在多条 tool 记录，选择 `content` 包含 `<persisted-tool-result` 或 `compact-tool-artifact-source.txt` 的那一条
17. 打开 `~/.renlijia/users/$SCOPE/conversations/$CONV_ID/tool-results/manifest.jsonl`，定位与 `$TOOL_MSG.toolCallId` 相同的 manifest 记录，记为 `$ARTIFACT_RECORD`
18. 打开 `$ARTIFACT_RECORD.path` 指向的 artifact 文件，记为 `$ARTIFACT_CONTENT`
19. 准备压力文本 `/tmp/aijia-context-compact-tool-artifact-block.txt`：写入至少 500 行固定句子，每行包含字面值 `CTX-COMPACT-TOOL-ARTIFACT-PRESSURE` 和行号，文件大小在 `45 KB` 到 `55 KB` 之间
20. 在同一对话中连续执行 4 轮发送；第 N 轮先把下面这段文字输入到输入框，再将 `/tmp/aijia-context-compact-tool-artifact-block.txt` 的完整内容按 `<= 20 KB` 分块追加到同一个输入框，全部追加完成后点击发送：
   ```
   这是长工具输出 artifact 自动压缩质量验收压力轮 N。
   工具证据案例=TOOL-ARTIFACT-CASE-ID=TA-2026-0604。
   工具证据尾部决定=TOOL-ARTIFACT-TAIL-DECISION=KEEP-REMOTE-LOGS。
   验证对象=自动压缩 summary 必须能读回 tool-results artifact，而不是只看工具预览。
   请只回复：已收到 TOOL-ARTIFACT-COMPACT-ROUND-N。不要调用工具，不要总结压力文本。
   ```
21. 每轮发送后等待 AI 完整回复：`tauri-pilot aijia wait-reply --timeout 600`
22. 如果 `~/.renlijia/users/$SCOPE/conversations/$CONV_ID/messages.jsonl` 中还没有 `subtype == "compact_boundary"` 的记录，最多追加 2 轮与第 20 步相同格式的压力消息；每轮后等待 `tauri-pilot aijia wait-reply --timeout 600`
23. 发送追问消息：`请根据前文和工具读取证据，只回复三行：案例=；尾部决定=；质量判断=TOOL-ARTIFACT-FOLLOWUP-OK`
24. 等待 AI 完整回复：`tauri-pilot aijia wait-reply --timeout 600`
25. 打开 `~/.renlijia/users/$SCOPE/conversations/$CONV_ID/messages.jsonl`
26. 在 `messages.jsonl` 中定位最新一条 `subtype == "compact_boundary"` 的记录，记为 `$MSG_BOUNDARY`
27. 在 `$MSG_BOUNDARY` 之后定位第一条 `isCompactSummary == true` 的 user 记录，记为 `$SUMMARY_MSG`
28. 在 `messages.jsonl` 中定位第 23 步追问对应的 user 记录，记为 `$FOLLOWUP_USER`
29. 在 `$FOLLOWUP_USER` 之后定位第一条 assistant 记录，记为 `$FOLLOWUP_ASSISTANT`
30. 恢复 `~/.renlijia/users/$SCOPE/config.json`：如果 `$CONFIG_CONTEXT_WINDOW_WAS_PRESENT == true`，把 `contextWindow` 恢复为 `$CONTEXT_WINDOW_BEFORE`；如果 `$CONFIG_CONTEXT_WINDOW_WAS_PRESENT == false`，移除 `contextWindow` 字段

**验收标准**

- 文件 `/tmp/aijia-context-compact-tool-artifact/compact-tool-artifact-source.txt` 存在，且大小至少 `90 KB`
- 文件 `~/.renlijia/users/$SCOPE/conversations/$CONV_ID/messages.jsonl` 存在
- 文件 `~/.renlijia/users/$SCOPE/conversations/$CONV_ID/tool-results/manifest.jsonl` 存在
- `messages.jsonl` 与 `manifest.jsonl` 每条记录都是可解析 JSON
- `$READ_ASSISTANT.role == "assistant"`
- `$READ_ASSISTANT.content` 字段 JSON 序列化字符串包含字面值 `TOOL-ARTIFACT-READ-OK`
- `$READ_ASSISTANT.content` 字段 JSON 序列化字符串包含字面值 `TOOL-ARTIFACT-CASE-ID=TA-2026-0604`
- `$READ_ASSISTANT.content` 字段 JSON 序列化字符串包含字面值 `TOOL-ARTIFACT-TAIL-DECISION=KEEP-REMOTE-LOGS`
- `$TOOL_MSG.role == "tool"`
- `$TOOL_MSG.content` 字段 JSON 序列化字符串包含字面值 `<persisted-tool-result`
- `$TOOL_MSG.content` 字段 JSON 序列化字符串包含字面值 `Full output saved to:`
- `$TOOL_MSG.content` 字段 JSON 序列化字符串包含字面值 `Sha256:`
- `$TOOL_MSG.content` 字段 JSON 序列化字符串不包含字面值 `[Output truncated:`
- `$ARTIFACT_RECORD.toolCallId == $TOOL_MSG.toolCallId`
- `$ARTIFACT_RECORD.path` 是非空字符串，且文件存在
- `$ARTIFACT_RECORD.originalChars >= 90000`
- `$ARTIFACT_CONTENT` 包含字面值 `TOOL-ARTIFACT-CASE-ID=TA-2026-0604`
- `$ARTIFACT_CONTENT` 包含字面值 `TOOL-ARTIFACT-TAIL-DECISION=KEEP-REMOTE-LOGS`
- `$MSG_BOUNDARY.role == "system"`
- `$MSG_BOUNDARY.subtype == "compact_boundary"`
- `$MSG_BOUNDARY.compactMetadata.trigger == "auto"`
- `$MSG_BOUNDARY.compactMetadata.messagesSummarized >= 1`
- `$MSG_BOUNDARY.compactMetadata.preTokens > $MSG_BOUNDARY.compactMetadata.postTokens`
- `$SUMMARY_MSG.role == "user"`
- `$SUMMARY_MSG.isCompactSummary == true`
- `$SUMMARY_MSG.content` 字段 JSON 序列化字符串包含字面值 `<context>`
- `$SUMMARY_MSG.content` 字段 JSON 序列化字符串包含字面值 `TOOL-ARTIFACT-CASE-ID=TA-2026-0604`
- `$SUMMARY_MSG.content` 字段 JSON 序列化字符串包含字面值 `TOOL-ARTIFACT-TAIL-DECISION=KEEP-REMOTE-LOGS`
- `$FOLLOWUP_USER` 位于 `$MSG_BOUNDARY` 之后
- `$FOLLOWUP_ASSISTANT.role == "assistant"`
- `$FOLLOWUP_ASSISTANT` 位于 `$FOLLOWUP_USER` 之后
- `$FOLLOWUP_ASSISTANT.content` 字段 JSON 序列化字符串包含字面值 `TOOL-ARTIFACT-FOLLOWUP-OK`
- `$FOLLOWUP_ASSISTANT.content` 字段 JSON 序列化字符串包含字面值 `TOOL-ARTIFACT-CASE-ID=TA-2026-0604`
- `$FOLLOWUP_ASSISTANT.content` 字段 JSON 序列化字符串包含字面值 `TOOL-ARTIFACT-TAIL-DECISION=KEEP-REMOTE-LOGS`
- `$FOLLOWUP_ASSISTANT.content` 字段 JSON 序列化字符串不包含字面值 `CTX-COMPACT-TOOL-ARTIFACT-PRESSURE`
- `$FOLLOWUP_ASSISTANT.content` 字段 JSON 序列化字符串不包含字面值 `只看到工具预览`
- `$FOLLOWUP_ASSISTANT.content` 字段 JSON 序列化字符串不包含字面值 `请重新提供文件内容`
- `$FOLLOWUP_ASSISTANT.content` 字段 JSON 序列化字符串不包含字面值 `我不记得`
- 如果 `$CONFIG_CONTEXT_WINDOW_WAS_PRESENT == true`，`~/.renlijia/users/$SCOPE/config.json` 的 `contextWindow == $CONTEXT_WINDOW_BEFORE`
- 如果 `$CONFIG_CONTEXT_WINDOW_WAS_PRESENT == false`，`~/.renlijia/users/$SCOPE/config.json` 中不存在字段 `contextWindow`

---

## 意图-上下文压缩-014: 摘要落盘后，原文全路径可查

**场景**
用户的长对话触发自动压缩后，后续模型如果需要核对摘要里的细节，能从 compact summary 里看到当前会话原始 transcript 的完整文件路径。这个路径不是相对路径，也不是目录提示，而是可以直接打开的 `messages.jsonl` 全路径。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断当前 scope：从 `tauri-pilot aijia where --json` 取当前登录用户 scope，记为 `$SCOPE`
3. 记录现有所有对话 ID：读取 `~/.renlijia/users/$SCOPE/conversations/` 下已有子目录名，记为集合 `$S_BEFORE`
4. 读取 `~/.renlijia/users/$SCOPE/config.json` 的 `contextWindow` 原值；如果字段存在，记 `$CONFIG_CONTEXT_WINDOW_WAS_PRESENT == true` 且原值为 `$CONTEXT_WINDOW_BEFORE`；如果字段不存在，记 `$CONFIG_CONTEXT_WINDOW_WAS_PRESENT == false`
5. 临时把 `~/.renlijia/users/$SCOPE/config.json` 的 `contextWindow` 写为字符串 `"64000"`
6. 新建空对话：`tauri-pilot aijia new-task`
7. 准备压力文本 `/tmp/aijia-context-compact-transcript-path-block.txt`：写入至少 500 行固定句子，每行包含字面值 `CTX-COMPACT-TRANSCRIPT-PATH-PRESSURE` 和行号，文件大小在 `45 KB` 到 `55 KB` 之间
8. 连续执行 4 轮发送；第 N 轮先把下面这段文字输入到输入框，再将 `/tmp/aijia-context-compact-transcript-path-block.txt` 的完整内容按 `<= 20 KB` 分块追加到同一个输入框，全部追加完成后点击发送：
   ```
   这是完整对话记录路径验收压力轮 N。
   核验标记=TRANSCRIPT-PATH-COMPACT-MARKER-ALPHA。
   请只回复：已收到 TRANSCRIPT-PATH-COMPACT-ROUND-N。不要调用工具，不要总结压力文本。
   ```
9. 每轮发送后等待 AI 完整回复：`tauri-pilot aijia wait-reply --timeout 600`
10. 找到本轮新建的对话 ID：在 `~/.renlijia/users/$SCOPE/conversations/` 下取 `mtime` 最新且不在 `$S_BEFORE` 里的子目录名，记为 `$CONV_ID`
11. 如果 `~/.renlijia/users/$SCOPE/conversations/$CONV_ID/messages.jsonl` 中还没有 `subtype == "compact_boundary"` 的记录，最多追加 2 轮与第 8 步相同格式的压力消息；每轮后等待 `tauri-pilot aijia wait-reply --timeout 600`
12. 打开 `~/.renlijia/users/$SCOPE/conversations/$CONV_ID/messages.jsonl`
13. 在 `messages.jsonl` 中定位最新一条 `subtype == "compact_boundary"` 的记录，记为 `$MSG_BOUNDARY`
14. 在 `$MSG_BOUNDARY` 之后定位第一条 `isCompactSummary == true` 的 user 记录，记为 `$SUMMARY_MSG`
15. 把 `~/.renlijia/users/$SCOPE/conversations/$CONV_ID/messages.jsonl` 解析为系统绝对路径，记为 `$EXPECTED_TRANSCRIPT_PATH`
16. 从 `$SUMMARY_MSG.content` 字段中提取「完整的对话记录在：」之后、`</context>` 之前的路径文本，去掉首尾空白后记为 `$SUMMARY_TRANSCRIPT_PATH`
17. 打开 `$SUMMARY_TRANSCRIPT_PATH` 指向的文件，记为 `$SUMMARY_TRANSCRIPT_FILE`
18. 如果 `~/.renlijia/users/$SCOPE/conversations/$CONV_ID/compact_boundaries.jsonl` 存在，打开该文件并定位最新一条 compact boundary 索引记录，记为 `$BOUNDARY_INDEX`
19. 恢复 `~/.renlijia/users/$SCOPE/config.json`：如果 `$CONFIG_CONTEXT_WINDOW_WAS_PRESENT == true`，把 `contextWindow` 恢复为 `$CONTEXT_WINDOW_BEFORE`；如果 `$CONFIG_CONTEXT_WINDOW_WAS_PRESENT == false`，移除 `contextWindow` 字段

**验收标准**

- 文件 `/tmp/aijia-context-compact-transcript-path-block.txt` 存在
- 文件 `~/.renlijia/users/$SCOPE/conversations/$CONV_ID/messages.jsonl` 存在
- `messages.jsonl` 每条记录都是可解析 JSON
- `$MSG_BOUNDARY.role == "system"`
- `$MSG_BOUNDARY.subtype == "compact_boundary"`
- `$MSG_BOUNDARY.compactMetadata.trigger == "auto"`
- `$SUMMARY_MSG.role == "user"`
- `$SUMMARY_MSG.isCompactSummary == true`
- `$SUMMARY_MSG.content` 字段 JSON 序列化字符串包含字面值 `<context>`
- `$SUMMARY_MSG.content` 字段 JSON 序列化字符串包含字面值 `TRANSCRIPT-PATH-COMPACT-MARKER-ALPHA`
- `$SUMMARY_MSG.content` 字段 JSON 序列化字符串包含字面值 `完整的对话记录在：`
- `$SUMMARY_TRANSCRIPT_PATH` 是非空字符串
- `$SUMMARY_TRANSCRIPT_PATH == $EXPECTED_TRANSCRIPT_PATH`
- `$SUMMARY_TRANSCRIPT_PATH` 字符串以当前系统的绝对路径前缀开头：Windows 为盘符加 `:\`，macOS/Linux 为 `/`
- 文件 `$SUMMARY_TRANSCRIPT_PATH` 存在
- `$SUMMARY_TRANSCRIPT_FILE` 每条记录都是可解析 JSON
- `$SUMMARY_TRANSCRIPT_FILE` 中存在记录 `$MSG_BOUNDARY`
- `$SUMMARY_TRANSCRIPT_FILE` 中存在记录 `$SUMMARY_MSG`
- `$SUMMARY_TRANSCRIPT_PATH` 字符串不以 `messages.jsonl` 开头
- `$SUMMARY_TRANSCRIPT_PATH` 字符串不以 `$CONV_ID/messages.jsonl` 开头
- 如果 `$BOUNDARY_INDEX` 存在，`$BOUNDARY_INDEX.summary_text` 字段 JSON 序列化字符串包含字面值 `完整的对话记录在：`
- 如果 `$BOUNDARY_INDEX` 存在，`$BOUNDARY_INDEX.summary_text` 字段 JSON 序列化字符串包含 `$SUMMARY_TRANSCRIPT_PATH`
- 如果 `$CONFIG_CONTEXT_WINDOW_WAS_PRESENT == true`，`~/.renlijia/users/$SCOPE/config.json` 的 `contextWindow == $CONTEXT_WINDOW_BEFORE`
- 如果 `$CONFIG_CONTEXT_WINDOW_WAS_PRESENT == false`，`~/.renlijia/users/$SCOPE/config.json` 中不存在字段 `contextWindow`

---
