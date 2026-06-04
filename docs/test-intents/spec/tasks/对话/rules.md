# rules.md — 对话

本 task 测的产品承诺：**用户能向 AI 发起任意复杂度的对话，无论模型生成耗时多长（分钟级长输出），最终都能完整收到结果，不会因网关侧 / 客户端任何中间层超时而被切断并退化成"模型未能生成回复"的兜底文案**。

UI 文案对应：应用启动后的默认对话界面、底部对话输入框、底部「发送」按钮。

---

## 意图-对话-001: 发起长生成对话，对话能完整收尾

**场景**
用户在主对话界面让 AI 跑一个会持续 2-5 分钟生成的复杂任务（例如让模型写一篇深度长文）。期望流式光标一直滚动直到模型自然结束，最终拿到完整内容，而不是在 ~2 分钟时突然终止并显示"模型未能生成回复"。本意图护栏对应 lotus 网关 chatClient timeout（120s→600s）回归——任何中间层把流式响应卡死在分钟级以下都会让这条意图 FAIL。

前提：当前登录账号的 tenant 默认 chat 模型是 sonnet 4-5 / 4-6 等长输出系列（AIjia 客户端不直接暴露模型切换；模型由后端路由）。如默认是 deepseek-v3 之类短响应模型，本意图在该环境无效，需要先在租户后台改默认模型。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断当前 scope：从 `tauri-pilot aijia where --json` 取，记为 `$SCOPE`
3. 记录当前时间 `T0`
4. 记录现有所有对话 ID（`ls ~/.renlijia/users/$SCOPE/conversations/`），记为集合 `$S_BEFORE`
5. 点击底部对话输入框
6. 输入以下 Prompt（一次性粘贴，不要分批）：
   ```
   请你作为主 AI 助手亲自直接在这条 assistant message 里输出下面文章的完整正文。

   🚫 严格禁止：
   - 调用 Agent 工具 / 委派任务给任何子 agent
   - 调用任何其它工具（Bash、Read、Write、Skill、TaskCreate、Grep、Glob 等）
   - 把生成任务拆轮转交给后续 turn

   ✅ 必须做：
   - 在你这一条 assistant message 的 content.text 里直接写出所有文章内容
   - 一次写完，不要分多轮

   文章要求：
   主题：分布式系统中的一致性与共识，全文 8000 字以上。

   1. 完整解释 CAP 定理与反例
   2. Paxos 协议原理 + 完整伪代码
   3. Raft 协议原理、leader 选举、日志复制 + 完整伪代码
   4. 实践案例：etcd、TiKV、Zookeeper 的实现差异
   5. 分布式事务：2PC / 3PC / TCC / Saga，逐一对比
   6. 每节至少给出 200 行可运行的 Go 代码示例
   7. 文末汇总对比表格

   不要省略任何细节，每节至少 1500 字。再次强调：必须由你本人直接写，不许调用任何工具。
   ```
7. 点击「发送」按钮
8. 持续观察对话界面，等待 AI 完整结束（最长允许等待 8 分钟）
9. 等结束后，找到本轮新建的对话 ID（在 `~/.renlijia/users/$SCOPE/conversations/` 下取 `mtime` 最新且不在 `$S_BEFORE` 里的子目录名），记为 `$CONV_ID`

**验收标准**

- 发送后界面立即出现一条 assistant 流式气泡（光标动效）
- 等待期间气泡内文本持续增加（每 30 秒能观察到字数变多）
- 模型自然结束（流式光标消失），结束时刻在 `T0 + 2 分钟` 到 `T0 + 8 分钟` 之间
- 文件 `~/.renlijia/users/$SCOPE/conversations/$CONV_ID/messages.jsonl` 存在
- 该文件**恰好 2 条**记录（user + assistant；任何额外 tool / 多轮 assistant 都说明委派/分轮跑偏）
- 该文件末条 JSON 记录 `role == "assistant"`
- 该末条记录 `content.text length >= 4000`
- 该末条记录 `content.text` 包含字面值 `CAP`
- 该末条记录 `content.text` 包含字面值 `Raft`
- 该末条记录 `content.text` 包含字面值 `Paxos`
- 该末条记录 `toolCalls` 字段不存在或为空数组（主 LLM 自己处理、没委派给任何工具）

- 该末条记录 `content.text` 包含字面值 `模型未能生成回复`
- 该末条记录 `content.text` 包含字面值 `请尝试换一种方式提问`
- `messages.jsonl` 中**任何一条**记录 `role == "tool"`（出现 tool 记录 = 主 LLM 委派给某个工具，本意图要求"自己处理"）
- `messages.jsonl` 中**任何一条** assistant 记录的 `toolCalls.length >= 1`（同上，委派信号）
- 流式气泡在 `T0 + 1 分 50 秒` 到 `T0 + 2 分 10 秒` 之间突然停止（典型 120s timeout 切断特征）
- `messages.jsonl` 中出现两条相邻 `role == "assistant"` 记录，且其中一条 `content.text length < 200`（半截流被切后又起新轮兜底）
- 等待 8 分钟流式气泡仍在滚动（说明根本没结束，不在本意图覆盖范围）

---

## 意图-对话-002: AI 生成文件后，回复下方出现文件卡片

**场景**
用户让 AI 创建一个本地文件（如 markdown 笔记）。期望 AI 用工具生成该文件后，在回复末尾用 `![artifact](文件绝对路径)` 标记声明产物；前端识别该标记后，把标记从渲染文本中剥离，并在 assistant 气泡下方渲染一张文件卡片，显示文件名 + 扩展名标识 + 操作按钮。本意图同时护栏系统提示词中「产物标记」段是否被 LLM 遵守，以及前端 `useTurnRenderModel` 对该标记的解析渲染逻辑——任一环节失效都会让这条意图 FAIL。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断当前 scope：从 `tauri-pilot aijia where --json` 取，记为 `$SCOPE`
3. 清理可能残留的测试产物文件：`rm -f /tmp/aijia-test-artifact-001.md`
4. 记录现有所有对话 ID（`ls ~/.renlijia/users/$SCOPE/conversations/`），记为集合 `$S_BEFORE`
5. 点击底部对话输入框
6. 输入以下 Prompt（一次性粘贴，不要分批）：
   ```
   请帮我在本地创建一个 markdown 笔记，文件路径是 `/tmp/aijia-test-artifact-001.md`，文件内容写：

   # 测试笔记

   这是一段用于验证产物展示功能的测试笔记。

   - 条目一
   - 条目二
   - 条目三

   创建完文件后，按 AIjia 系统的产物声明规则在你这条回复里标记该产物。
   ```
7. 点击「发送」按钮
8. 持续观察对话界面，等待 AI 完整结束（最长允许等待 3 分钟）
9. 等结束后，找到本轮新建的对话 ID（在 `~/.renlijia/users/$SCOPE/conversations/` 下取 `mtime` 最新且不在 `$S_BEFORE` 里的子目录名），记为 `$CONV_ID`

**验收标准**

- 模型自然结束（流式光标消失）
- 文件 `/tmp/aijia-test-artifact-001.md` 存在
- 文件 `/tmp/aijia-test-artifact-001.md` 内容包含字面值 `测试笔记`
- 本轮 assistant 气泡下方出现一个文件卡片（`data-testid == "generated-file-card"`）
- 该卡片标题包含字面值 `aijia-test-artifact-001.md`
- 渲染中的 assistant 气泡正文**不含**字面值 `![artifact](`（标记已被剥离）
- 文件 `~/.renlijia/users/$SCOPE/conversations/$CONV_ID/messages.jsonl` 存在
- `messages.jsonl` 末条 JSON 记录 `role == "assistant"`
- 该末条记录 `content.text` 包含字面值 `![artifact](/tmp/aijia-test-artifact-001.md)`（原始标记保留在存储里）

- assistant 气泡正文中直接渲染出字面值 `![artifact]`
- 本轮 assistant 气泡下方文件卡片数量 `>= 2`（本意图只让生成 1 个产物）
- 任一文件卡片的标题显示为字面值 `artifact`（说明把标记名当成了文件名）
- `content.text` 含字面值 `![artifact](` 但页面上**无**任何文件卡片渲染（标记保留却未被解析）

---

## 意图-对话-003: AI 一次生成多个产物时，回复下方按顺序出现多张卡片

**场景**
用户让 AI 在一次回复里同时生成 2 个产物文件。期望 AI 在回复末尾按生成顺序写出 2 条 `![artifact](路径)` 标记；前端解析后渲染 2 张文件卡片，**DOM 顺序与标记顺序一致**，每张卡片标题对应各自文件名。本意图护栏「多 artifact 标记并存时各自独立解析」和「卡片渲染顺序不乱」——同一条意图同时验证 `useTurnRenderModel.parseArtifactMarks` 的 `for...of matchAll` 循环没漏标记、顺序没倒置。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断当前 scope：从 `tauri-pilot aijia where --json` 取，记为 `$SCOPE`
3. 清理可能残留的测试产物文件：`rm -f /tmp/aijia-test-artifact-multi-*.md`
4. 记录现有所有对话 ID（`ls ~/.renlijia/users/$SCOPE/conversations/`），记为集合 `$S_BEFORE`
5. 点击底部对话输入框
6. 输入以下 Prompt（一次性粘贴，不要分批）：
   ```
   请帮我在本地创建两个 markdown 笔记，每个文件内容写一行短句即可：
   - 文件 1 路径 `/tmp/aijia-test-artifact-multi-1.md`，内容写 `# 笔记 A`
   - 文件 2 路径 `/tmp/aijia-test-artifact-multi-2.md`，内容写 `# 笔记 B`

   两个文件都创建完后，按 AIjia 系统的产物声明规则在回复末尾按顺序标记这两个产物（先标记 1 再标记 2）。
   ```
7. 点击「发送」按钮
8. 持续观察对话界面，等待 AI 完整结束（最长允许等待 3 分钟）
9. 等结束后，找到本轮新建的对话 ID，记为 `$CONV_ID`

**验收标准**

- 模型自然结束（流式光标消失）
- 文件 `/tmp/aijia-test-artifact-multi-1.md` 存在
- 文件 `/tmp/aijia-test-artifact-multi-2.md` 存在
- 本轮 assistant 气泡下方 `data-testid == "generated-file-card"` 的元素数量 `== 2`
- 按 DOM 顺序，第 1 个卡片标题包含字面值 `aijia-test-artifact-multi-1.md`
- 按 DOM 顺序，第 2 个卡片标题包含字面值 `aijia-test-artifact-multi-2.md`
- 渲染中的 assistant 气泡正文**不含**字面值 `![artifact](`
- 文件 `~/.renlijia/users/$SCOPE/conversations/$CONV_ID/messages.jsonl` 末条 JSON 记录 `role == "assistant"`
- 该末条记录 `content.text` 包含字面值 `![artifact](/tmp/aijia-test-artifact-multi-1.md)`
- 该末条记录 `content.text` 包含字面值 `![artifact](/tmp/aijia-test-artifact-multi-2.md)`

- 本轮 assistant 气泡下方文件卡片数量 `== 1`（说明 2 条 artifact 标记里只解析出 1 条）
- 本轮 assistant 气泡下方文件卡片数量 `>= 3`（多解析出意外卡片）
- 第 1 个卡片标题反而包含 `aijia-test-artifact-multi-2.md`（顺序倒置）
- assistant 气泡正文中直接渲染出字面值 `![artifact]`

---

## 意图-对话-004: AI 用 Bash 命令生成产物时，回复同样带 artifact 标记

**场景**
用户明确要求 AI 用 Bash 工具（而非 Write 工具）生成本地文件。期望 LLM 不只在 Write 路径下记得加标记——通过 Bash / shell 命令落盘的产物，回复中同样要用 `![artifact](路径)` 声明、并被前端渲染成卡片。本意图护栏系统提示词中「Write工具、Bash、脚本、MCP工具等」并列约束 LLM 跨工具路径都遵守标记，避免出现「只 Write 加标记、Bash 不加」的协议漂移。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断当前 scope：从 `tauri-pilot aijia where --json` 取，记为 `$SCOPE`
3. 清理可能残留的测试产物文件：`rm -f /tmp/aijia-test-artifact-bash.txt`
4. 记录现有所有对话 ID（`ls ~/.renlijia/users/$SCOPE/conversations/`），记为集合 `$S_BEFORE`
5. 点击底部对话输入框
6. 输入以下 Prompt（一次性粘贴，不要分批）：
   ```
   请用 Bash 工具把字符串 `测试笔记 Bash 版` 写到本地文件 `/tmp/aijia-test-artifact-bash.txt`。命令示例：
   `echo "测试笔记 Bash 版" > /tmp/aijia-test-artifact-bash.txt`

   要求：必须调用 Bash 工具完成，不要使用 Write 工具。完成后按 AIjia 系统的产物声明规则在回复中标记该文件。
   ```
7. 点击「发送」按钮
8. 持续观察对话界面，等待 AI 完整结束（最长允许等待 3 分钟）
9. 等结束后，找到本轮新建的对话 ID，记为 `$CONV_ID`

**验收标准**

- 模型自然结束（流式光标消失）
- 文件 `/tmp/aijia-test-artifact-bash.txt` 存在
- 文件 `/tmp/aijia-test-artifact-bash.txt` 内容包含字面值 `测试笔记 Bash 版`
- `~/.renlijia/users/$SCOPE/conversations/$CONV_ID/messages.jsonl` 中至少一条 assistant 记录的 `toolCalls` 数组里有一个元素 `name == "Bash"`
- 该 `toolCalls` 元素的 `arguments` JSON 序列化字符串包含字面值 `aijia-test-artifact-bash.txt`
- 末条 assistant 记录 `content.text` 包含字面值 `![artifact](/tmp/aijia-test-artifact-bash.txt)`
- 本轮 assistant 气泡下方 `data-testid == "generated-file-card"` 的元素数量 `>= 1`
- 至少一张卡片标题包含字面值 `aijia-test-artifact-bash.txt`
- 渲染中的 assistant 气泡正文**不含**字面值 `![artifact](`

- 文件 `/tmp/aijia-test-artifact-bash.txt` 存在但末条 assistant `content.text` 不含 `![artifact](` 字面值（Bash 路径下漏加标记）
- `messages.jsonl` 中所有 assistant 记录的 `toolCalls` 全部为空数组（根本没调工具）
- `messages.jsonl` 中所有 assistant 记录的 `toolCalls` 元素 `name` 全是 `"Write"`（LLM 没按要求走 Bash 路径，本意图在该环境不构成有效验证，需复跑）
- 本轮 assistant 气泡下方文件卡片数量 `== 0`

---

## 意图-对话-005: AI 生成 xlsx 文件时，卡片显示 XLS 类型标签

**场景**
用户让 AI 生成一个 `.xlsx` 扩展名的产物文件。期望前端 `useTurnRenderModel` 把扩展名 `xlsx` 经 `ARTIFACT_EXT_TO_TYPE` 映射为 `excel` 类型，再经 `normalizeFileLabel` 把 `XLSX` 规范化为 `XLS` 显示在卡片左侧文件图标上。本意图护栏扩展名映射表 + label 规范化两段逻辑——直接关掉任一段都会让卡片显示成 `XLSX` / `EXCEL` / `FILE`，意图就 FAIL。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断当前 scope：从 `tauri-pilot aijia where --json` 取，记为 `$SCOPE`
3. 清理可能残留的测试产物文件：`rm -f /tmp/aijia-test-artifact.xlsx`
4. 记录现有所有对话 ID（`ls ~/.renlijia/users/$SCOPE/conversations/`），记为集合 `$S_BEFORE`
5. 点击底部对话输入框
6. 输入以下 Prompt（一次性粘贴，不要分批）：
   ```
   请用 Bash 工具创建一个空文件 `/tmp/aijia-test-artifact.xlsx`，命令用 `touch /tmp/aijia-test-artifact.xlsx` 即可，**不要**往里写任何内容（占位文件即可，本次测试不关心 xlsx 内容）。

   完成后按 AIjia 系统的产物声明规则在回复中标记该文件。
   ```
7. 点击「发送」按钮
8. 持续观察对话界面，等待 AI 完整结束（最长允许等待 3 分钟）
9. 等结束后，找到本轮新建的对话 ID，记为 `$CONV_ID`

**验收标准**

- 模型自然结束（流式光标消失）
- 文件 `/tmp/aijia-test-artifact.xlsx` 存在
- 末条 assistant 记录 `content.text` 包含字面值 `![artifact](/tmp/aijia-test-artifact.xlsx)`
- 本轮 assistant 气泡下方 `data-testid == "generated-file-card"` 的元素数量 `>= 1`
- 至少一张卡片标题包含字面值 `aijia-test-artifact.xlsx`
- 至少一张卡片的可视文本内容包含字面值 `XLS`（左侧文件图标 label 区域）
- 渲染中的 assistant 气泡正文**不含**字面值 `![artifact](`

- 卡片可视文本中出现字面值 `XLSX`（normalizeFileLabel 未生效，原扩展名直接透出）
- 卡片可视文本中出现字面值 `EXCEL`（normalizeFileLabel 未生效，alias 直接透出）
- 卡片可视文本中出现字面值 `FILE`（扩展名识别失败、走兜底）
- 本轮 assistant 气泡下方文件卡片数量 `== 0`

---

## 意图-对话-006: 点击产物卡片「预览」按钮，侧边栏显示文件内容

**场景**
意图-002 ~ 005 验证了 artifact 标记 → 卡片渲染。本意图验证 artifact 卡片主按钮「预览」（artifact 路径下 `primaryAction === 'preview'`，按钮文案被改写为「预览」）：点击后右侧 RightPanel 切到 `FilePreviewPane`，渲染文件正文。本意图护栏「artifact 合成 id 走 `localPath` 旁路调 `getLocalFilePreview`」——若主按钮 handler 仍走后端 `getFilePreview(id, convId)` 用合成 id `artifact-{msgId}-{fileName}` 查 db 必失败，弹 toast `无法预览此文件`。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断当前 scope：`tauri-pilot aijia where --json` 取，记为 `$SCOPE`
3. 清理可能残留的测试产物文件：`rm -f /tmp/aijia-test-preview.md`
4. 记录现有所有对话 ID（`ls ~/.renlijia/users/{scope}/conversations/`），记为集合 `$S_BEFORE`
5. 点击底部对话输入框
6. 输入以下 Prompt（一次性粘贴，不要分批）：
   ```
   请帮我在本地创建一个 markdown 笔记，文件路径是 `/tmp/aijia-test-preview.md`，文件内容写：

   # 预览测试

   这是一段用于验证侧边栏预览的测试笔记。

   - 条目甲

   创建完文件后，按 AIjia 系统的产物声明规则在你这条回复里标记该产物。
   ```
7. 点击「发送」按钮
8. 持续观察对话界面，等待 AI 完整结束（最长允许等待 3 分钟）
9. 等结束后，找到本轮新建的对话 ID（在 `~/.renlijia/users/{scope}/conversations/` 下取 `mtime` 最新且不在 `$S_BEFORE` 里的子目录名），记为 `$CONV_ID`
10. 等本轮 assistant 气泡下方 `[data-testid="generated-file-card"]` 数量 `>= 1`
11. 触发卡片预览：`tauri-pilot aijia file-card-click --action preview --turn last`
12. 等 2 秒让 `FilePreviewPane` 异步加载

**验收标准**

- 模型自然结束（流式光标消失）
- 文件 `/tmp/aijia-test-preview.md` 存在
- 本轮 assistant 气泡下方 `[data-testid="generated-file-card"]` 数量 `>= 1`
- 步骤 11 命令返回 JSON 中 `ok == true`
- 步骤 12 后 `[data-aijia-file-preview-header]` 出现
- `[data-aijia-file-preview-header]` 内 `textContent` 包含字面值 `aijia-test-preview.md`
- `[data-aijia-file-preview-body]` 出现
- `[data-aijia-file-preview-body]` 内 `textContent` 包含字面值 `预览测试`
- `[data-aijia-file-preview-body]` 内 `textContent` 包含字面值 `条目甲`

- 应用通知区出现 toast 标题字面值 `无法预览此文件`
- `[data-aijia-file-preview-body]` 内出现字面值 `加载预览失败`
- 步骤 11 命令返回 JSON 中 `reason == "card_not_found"` 或 `reason == "menuitem_disabled"`
- `/tmp/aijia-test-preview.md` 文件被删除或修改（mtime 与步骤 9 后一致）

---

## 意图-对话-007: 点击产物卡片「用默认应用打开」，无错误提示

**场景**
意图-006 验证主按钮「预览」。本意图验证 chevron dropdown 的「用默认应用打开」menuitem：点击后系统默认应用打开产物文件、前端不弹错误 toast。本意图护栏「artifact 合成 id 走 `@tauri-apps/plugin-shell.open(filePath)` 旁路」——若 handler 仍调后端 `openGeneratedFile(合成 id, convId)`，db 查不到必弹 toast `无法打开文件`。系统应用是否真打开是 macOS OS 级行为，e2e 不可机器验证；本意图只验「不弹错误 toast」这条可机器观察的护栏。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断当前 scope：`tauri-pilot aijia where --json` 取，记为 `$SCOPE`
3. 清理可能残留的测试产物文件：`rm -f /tmp/aijia-test-open.md`
4. 记录现有所有对话 ID（`ls ~/.renlijia/users/{scope}/conversations/`），记为集合 `$S_BEFORE`
5. 点击底部对话输入框
6. 输入以下 Prompt（一次性粘贴，不要分批）：
   ```
   请帮我在本地创建一个 markdown 笔记，文件路径是 `/tmp/aijia-test-open.md`，文件内容写：

   # 打开测试

   这是一段用于验证用默认应用打开的测试笔记。

   创建完文件后，按 AIjia 系统的产物声明规则在你这条回复里标记该产物。
   ```
7. 点击「发送」按钮
8. 持续观察对话界面，等待 AI 完整结束（最长允许等待 3 分钟）
9. 等结束后，找到本轮新建的对话 ID，记为 `$CONV_ID`
10. 等本轮 assistant 气泡下方 `[data-testid="generated-file-card"]` 数量 `>= 1`
11. 记录 `/tmp/aijia-test-open.md` 当前 `mtime`，记为 `M_BEFORE`
12. 触发 dropdown 操作：`tauri-pilot aijia file-card-click --action open --turn last`
13. 等 2 秒让前端 handler 跑完 + toast 可能弹出的窗口

**验收标准**

- 模型自然结束（流式光标消失）
- 文件 `/tmp/aijia-test-open.md` 存在
- 本轮 assistant 气泡下方 `[data-testid="generated-file-card"]` 数量 `>= 1`
- 步骤 12 命令返回 JSON 中 `ok == true`
- 步骤 12 命令返回 JSON 中无 `reason` 字段
- `/tmp/aijia-test-open.md` 的 `mtime` 与 `M_BEFORE` 一致（点击只读不改文件）

- 应用通知区出现 toast 标题字面值 `无法打开文件`
- 应用通知区出现 toast 字面值 `打开生成文件失败`（旧错误字面，防回归）
- 步骤 12 命令返回 JSON 中 `reason == "card_not_found"` 或 `reason == "menuitem_disabled"`
- dev server 日志中出现字面值 `Failed to resolve stored path`（说明 handler 走错后端 IPC、没走 artifact 旁路）

---

## 意图-对话-008: 点击产物卡片「在文件夹中显示」，无错误提示

**场景**
本意图验证 chevron dropdown 的「在文件夹中显示」menuitem：点击后 Finder 打开产物所在父目录、前端不弹错误 toast。本意图护栏「artifact 合成 id 走 `@tauri-apps/plugin-shell.open(parentDir)` 旁路」——若 handler 仍调后端 `revealFileInFolder(合成 id, convId)`，db 查不到必弹 toast `无法在文件夹中显示`。Finder 是否真打开是 macOS OS 级行为，e2e 不可机器验证；本意图只验「不弹错误 toast」这条可机器观察的护栏。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断当前 scope：`tauri-pilot aijia where --json` 取，记为 `$SCOPE`
3. 清理可能残留的测试产物文件：`rm -f /tmp/aijia-test-reveal.md`
4. 记录现有所有对话 ID（`ls ~/.renlijia/users/{scope}/conversations/`），记为集合 `$S_BEFORE`
5. 点击底部对话输入框
6. 输入以下 Prompt（一次性粘贴，不要分批）：
   ```
   请帮我在本地创建一个 markdown 笔记，文件路径是 `/tmp/aijia-test-reveal.md`，文件内容写：

   # 在文件夹中显示测试

   这是一段用于验证文件夹定位的测试笔记。

   创建完文件后，按 AIjia 系统的产物声明规则在你这条回复里标记该产物。
   ```
7. 点击「发送」按钮
8. 持续观察对话界面，等待 AI 完整结束（最长允许等待 3 分钟）
9. 等结束后，找到本轮新建的对话 ID，记为 `$CONV_ID`
10. 等本轮 assistant 气泡下方 `[data-testid="generated-file-card"]` 数量 `>= 1`
11. 记录 `/tmp/aijia-test-reveal.md` 当前 `mtime`，记为 `M_BEFORE`
12. 触发 dropdown 操作：`tauri-pilot aijia file-card-click --action reveal --turn last`
13. 等 2 秒让前端 handler 跑完 + toast 可能弹出的窗口

**验收标准**

- 模型自然结束（流式光标消失）
- 文件 `/tmp/aijia-test-reveal.md` 存在
- 本轮 assistant 气泡下方 `[data-testid="generated-file-card"]` 数量 `>= 1`
- 步骤 12 命令返回 JSON 中 `ok == true`
- 步骤 12 命令返回 JSON 中无 `reason` 字段
- `/tmp/aijia-test-reveal.md` 的 `mtime` 与 `M_BEFORE` 一致（点击只读不改文件）

- 应用通知区出现 toast 标题字面值 `无法在文件夹中显示`
- 应用通知区出现 toast 字面值 `定位生成文件失败`（旧错误字面，防回归）
- 步骤 12 命令返回 JSON 中 `reason == "card_not_found"` 或 `reason == "menuitem_disabled"`
- dev server 日志中出现字面值 `Failed to resolve stored path`

---

## 意图-对话-009: PDF 卡片主按钮显示「打开」，不进入预览面板

**场景**
PDF 不在前端 `generatedFileActions.PREVIEWABLE_FILE_TYPES` 白名单（同后端 `normalize_preview_kind` 对齐：只支持 md/html/txt/json/csv/图片）。期望前端 `useTurnRenderModel` 据此判 `canPreview = false` → `primaryAction = 'open'`，卡片主按钮文案变成「打开」（不是「预览」）；用户清楚 PDF 不能内联预览、要走默认应用。本意图护栏「PDF 不能误进预览路径」——若某次改动把 pdf 加进 PREVIEWABLE_FILE_TYPES 或 useTurnRenderModel 路由错，主按钮会变成「预览」、点击后调到 `get_local_file_preview` 拉文件字节，违背产品「PDF 走外部应用打开」的承诺。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断当前 scope：`tauri-pilot aijia where --json` 取，记为 `$SCOPE`
3. 清理可能残留的测试产物文件：`rm -f /tmp/aijia-test-artifact.pdf`
4. 记录现有所有对话 ID（`ls ~/.renlijia/users/{scope}/conversations/`），记为集合 `$S_BEFORE`
5. 点击底部对话输入框
6. 输入以下 Prompt（一次性粘贴，不要分批）：
   ```
   请用 Bash 工具创建一个空文件 `/tmp/aijia-test-artifact.pdf`，命令用 `touch /tmp/aijia-test-artifact.pdf` 即可，**不要**往里写任何内容（占位文件即可，本次测试不关心 pdf 内容）。

   完成后按 AIjia 系统的产物声明规则在回复中标记该文件。
   ```
7. 点击「发送」按钮
8. 持续观察对话界面，等待 AI 完整结束（最长允许等待 3 分钟）
9. 等结束后，找到本轮新建的对话 ID，记为 `$CONV_ID`
10. 等本轮 assistant 气泡下方 `[data-testid="generated-file-card"]` 数量 `>= 1`
11. 执行 `tauri-pilot aijia file-card-snapshot --json`

**验收标准**

- 模型自然结束（流式光标消失）
- 文件 `/tmp/aijia-test-artifact.pdf` 存在
- 末条 assistant 记录 `content.text` 包含字面值 `![artifact](/tmp/aijia-test-artifact.pdf)`
- 步骤 11 命令返回 JSON 中 `count >= 1`
- 至少一张卡片 `title` 包含字面值 `aijia-test-artifact.pdf`
- 至少一张卡片 `filePath == "/tmp/aijia-test-artifact.pdf"`
- 至少一张卡片 `appName == "打开"`（主按钮文案）
- 至少一张卡片的可视文本内容包含字面值 `PDF`（左侧 TiltedFileIcon 的 label 区域）

- 任一卡片 `appName == "预览"`（若 PDF 被加进 PREVIEWABLE_FILE_TYPES 或路由错，主按钮会变预览——回归信号）
- 卡片可视文本中出现字面值 `FILE`（扩展名识别失败、走兜底）
- 步骤 11 命令返回 JSON 中 `count == 0`

---

## 意图-对话-010: AI 生成 pptx 文件时，卡片显示 PPT 类型标签

**场景**
用户让 AI 生成一个 `.pptx` 扩展名的产物文件。期望前端 `useTurnRenderModel.ARTIFACT_EXT_TO_TYPE` 把 `pptx` 映射为 `ppt` 类型，再经 `GeneratedFileCard.normalizeFileLabel` 把 `PPTX`/`POWERPOINT` 规范化为 `PPT` 显示在卡片左侧文件图标上。本意图护栏扩展名映射表 + label 规范化两段逻辑——直接关掉任一段都会让卡片显示成 `PPTX`/`POWERPOINT`/`FILE`，意图就 FAIL。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断当前 scope：`tauri-pilot aijia where --json` 取，记为 `$SCOPE`
3. 清理可能残留的测试产物文件：`rm -f /tmp/aijia-test-artifact.pptx`
4. 记录现有所有对话 ID（`ls ~/.renlijia/users/{scope}/conversations/`），记为集合 `$S_BEFORE`
5. 点击底部对话输入框
6. 输入以下 Prompt（一次性粘贴，不要分批）：
   ```
   请用 Bash 工具创建一个空文件 `/tmp/aijia-test-artifact.pptx`，命令用 `touch /tmp/aijia-test-artifact.pptx` 即可，**不要**往里写任何内容（占位文件即可，本次测试不关心 pptx 内容）。

   完成后按 AIjia 系统的产物声明规则在回复中标记该文件。
   ```
7. 点击「发送」按钮
8. 持续观察对话界面，等待 AI 完整结束（最长允许等待 3 分钟）
9. 等结束后，找到本轮新建的对话 ID，记为 `$CONV_ID`

**验收标准**

- 模型自然结束（流式光标消失）
- 文件 `/tmp/aijia-test-artifact.pptx` 存在
- 末条 assistant 记录 `content.text` 包含字面值 `![artifact](/tmp/aijia-test-artifact.pptx)`
- 本轮 assistant 气泡下方 `[data-testid="generated-file-card"]` 数量 `>= 1`
- 至少一张卡片标题包含字面值 `aijia-test-artifact.pptx`
- 至少一张卡片的可视文本内容包含字面值 `PPT`（左侧文件图标 label 区域）

- 卡片可视文本中出现字面值 `PPTX`（normalizeFileLabel 未生效，原扩展名直接透出）
- 卡片可视文本中出现字面值 `POWERPOINT`（normalizeFileLabel 未生效，alias 直接透出）
- 卡片可视文本中出现字面值 `FILE`（扩展名识别失败、走兜底）
- 本轮 assistant 气泡下方文件卡片数量 `== 0`

---

## 意图-对话-011: AI 生成 docx 文件时，卡片显示 DOC 类型标签

**场景**
用户让 AI 生成一个 `.docx` 扩展名的产物文件。期望前端 `useTurnRenderModel.ARTIFACT_EXT_TO_TYPE` 把 `docx` 映射为对应类型，再经 `GeneratedFileCard.normalizeFileLabel` 把 `DOCX`/`WORD` 规范化为 `DOC` 显示在卡片左侧文件图标上。本意图护栏 office 系列扩展名标签统一化——若 normalize 漏了 docx 分支，卡片会显示成 `DOCX`/`WORD`，跟 XLS/PPT 系列不对称。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断当前 scope：`tauri-pilot aijia where --json` 取，记为 `$SCOPE`
3. 清理可能残留的测试产物文件：`rm -f /tmp/aijia-test-artifact.docx`
4. 记录现有所有对话 ID（`ls ~/.renlijia/users/{scope}/conversations/`），记为集合 `$S_BEFORE`
5. 点击底部对话输入框
6. 输入以下 Prompt（一次性粘贴，不要分批）：
   ```
   请用 Bash 工具创建一个空文件 `/tmp/aijia-test-artifact.docx`，命令用 `touch /tmp/aijia-test-artifact.docx` 即可，**不要**往里写任何内容（占位文件即可，本次测试不关心 docx 内容）。

   完成后按 AIjia 系统的产物声明规则在回复中标记该文件。
   ```
7. 点击「发送」按钮
8. 持续观察对话界面，等待 AI 完整结束（最长允许等待 3 分钟）
9. 等结束后，找到本轮新建的对话 ID，记为 `$CONV_ID`

**验收标准**

- 模型自然结束（流式光标消失）
- 文件 `/tmp/aijia-test-artifact.docx` 存在
- 末条 assistant 记录 `content.text` 包含字面值 `![artifact](/tmp/aijia-test-artifact.docx)`
- 本轮 assistant 气泡下方 `[data-testid="generated-file-card"]` 数量 `>= 1`
- 至少一张卡片标题包含字面值 `aijia-test-artifact.docx`
- 至少一张卡片的可视文本内容包含字面值 `DOC`（左侧文件图标 label 区域）

- 卡片可视文本中出现字面值 `DOCX`（normalizeFileLabel 未生效，原扩展名直接透出）
- 卡片可视文本中出现字面值 `WORD`（normalizeFileLabel 未生效，alias 直接透出）
- 卡片可视文本中出现字面值 `FILE`（扩展名识别失败、走兜底）
- 本轮 assistant 气泡下方文件卡片数量 `== 0`
