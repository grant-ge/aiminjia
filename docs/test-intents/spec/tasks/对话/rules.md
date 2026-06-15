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

---

## 意图-对话-012: 回复引用文件，链接图片可点击

**场景**
用户让 AI 基于本地工作区文件输出一段说明，并要求回复里同时包含普通 Markdown 链接、引用式链接、带 title 的链接、图片语法。期望普通引用来源使用 `[名称](路径或URL)` 渲染为可点击链接；可预览的本地文件点击后打开右侧预览；不可预览文件交给系统默认打开；图片语法指向真实图片时显示缩略图，指向非图片本地文件时不能显示破图，而是退回为可点击文件链接。本意图护栏 assistant Markdown 的 link/image 渲染、相对路径解析、文件预览和外部打开分流。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断当前 scope：`tauri-pilot aijia where --json` 取，记为 `$SCOPE`
3. 准备测试工作区：`mkdir -p /tmp/aijia-md-link-workspace/docs /tmp/aijia-md-link-workspace/assets`
4. 写入测试文件：
   - `/tmp/aijia-md-link-workspace/docs/storage.md`，内容包含 `存储方案`
   - `/tmp/aijia-md-link-workspace/docs/backend.md`，内容包含 `Rust 方案`
   - `/tmp/aijia-md-link-workspace/docs/test.txt`，内容包含 `测试文本`
   - `/tmp/aijia-md-link-workspace/docs/contract.docx`，可用 `touch` 创建占位文件
   - `/tmp/aijia-md-link-workspace/assets/sample.png`，写入一个最小有效 PNG
5. 新建空对话：`tauri-pilot aijia new-task`
6. 授权 `/tmp/aijia-md-link-workspace` 作为当前对话工作区；若弹出权限确认，选择允许并记住到用户级
7. 在对话输入框输入以下 Prompt（一次性粘贴，不要分批）：
   ```
   请基于当前工作区里的 docs/storage.md 和 docs/backend.md 输出一段 Markdown 测试内容。

   要求：
   1. 提到 storage.md 时使用普通链接 `[存储方案](docs/storage.md)`。
   2. 提到 backend.md 时使用引用式链接 `[后端 Rust 方案][backend]`，并在回复末尾写 `[backend]: docs/backend.md "后端说明"`。
   3. 提到 test.txt 时使用带 title 的链接 `[测试文件](docs/test.txt "这是一个测试文本文件")`。
   4. 提到 contract.docx 时使用普通链接 `[合同模板](docs/contract.docx)`。
   5. 输出一张真实图片：`![真实图片](assets/sample.png "真实图片")`。
   6. 输出一张非图片文件路径的图片语法：`![文本占位图](docs/test.txt "非图片路径")`。
   7. 不要使用行内代码展示这些文件路径。
   ```
8. 点击「发送」按钮
9. 持续观察对话界面，等待 AI 完整结束（最长允许等待 3 分钟）
10. 等结束后，找到本轮新建的对话 ID，记为 `$CONV_ID`
11. 读取 `~/.renlijia/users/{scope}/conversations/{conv_id}/messages.jsonl` 中最后一条 assistant 文本
12. 用 tauri-pilot 的 Markdown 链接快照能力读取当前 assistant 气泡内链接和图片状态，记为 `$SNAPSHOT`（CLI 待补：返回每个 link 的 text、href、resolvedPath、openMode，以及每个 image 的 alt、src、broken）
13. 依次点击「存储方案」「测试文件」「合同模板」「真实图片」「文本占位图」

**验收标准**

应该看到：
- 末条 assistant 记录 `content.text` 包含字面值 `[存储方案](docs/storage.md)`
- 末条 assistant 记录 `content.text` 包含字面值 `[后端 Rust 方案][backend]`
- 末条 assistant 记录 `content.text` 包含字面值 `[backend]: docs/backend.md`
- 末条 assistant 记录 `content.text` 包含字面值 `[测试文件](docs/test.txt "这是一个测试文本文件")`
- 末条 assistant 记录 `content.text` 包含字面值 `![真实图片](assets/sample.png "真实图片")`
- `$SNAPSHOT.links` 中存在 `text == "存储方案"` 且 `href == "docs/storage.md"` 且 `openMode == "preview"`
- `$SNAPSHOT.links` 中存在 `text == "后端 Rust 方案"` 且 `href == "docs/backend.md"` 且 `openMode == "preview"`
- `$SNAPSHOT.links` 中存在 `text == "测试文件"` 且 `href == "docs/test.txt"` 且 `openMode == "preview"`
- 点击「存储方案」或「测试文件」后，右侧文件预览面板标题分别包含 `storage.md` 或 `test.txt`
- 点击「合同模板」后系统打开本地文件，UI 不出现「无法打开文件」或「预览失败」错误 toast
- `$SNAPSHOT.images` 中存在 `alt == "真实图片"` 且 `broken != true`
- `$SNAPSHOT.links` 中存在 `text == "文本占位图"` 且 `href == "docs/test.txt"`（非图片路径退回为普通文件链接）

不应该看到：
- assistant 可视正文中这些本地路径只以行内代码形态出现，而没有对应 Markdown 链接
- 相对路径链接被渲染成纯文本，无法点击
- `![文本占位图](docs/test.txt "非图片路径")` 在 UI 中显示为破图图标
- 点击任一相对路径本地链接时出现 `href is not supported`、`无法解析路径`、`Unknown local file` 类错误
## 意图-对话-013: 后台命令完成后，输出可被读取

**场景**
用户让 AI 启动一个耗时命令，但不希望当前对话一直卡在命令执行上。期望 AI 用当前平台的命令工具把任务放到后台，立即拿到后台任务 ID；命令完成后，AI 能用同一个后台任务读取入口拿到输出内容。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断当前 scope：`tauri-pilot aijia where --json` 取，记为 `$SCOPE`
3. 清理可能残留的测试产物文件：`rm -f /tmp/aijia-shell-background-done.txt`
4. 记录现有所有对话 ID（`ls ~/.renlijia/users/{scope}/conversations/`），记为集合 `$S_BEFORE`
5. 点击底部对话输入框
6. 输入以下 Prompt（一次性粘贴，不要分批）：
   ```
   请使用当前平台的命令执行工具启动一个后台命令，不要前台等待。

   要求：
   - macOS/Linux 请使用 Bash 工具
   - Windows 请使用 PowerShell 工具
   - 工具参数必须设置 run_in_background=true
   - 命令先等待 5 秒，然后把 `aijia-bg-finished-013` 写入 `/tmp/aijia-shell-background-done.txt`
   - 命令标准输出打印 `aijia-bg-output-013`
   - 后台任务启动后，请在回复里告诉我 task_id
   ```
7. 点击「发送」按钮
8. 等待本轮 assistant 回复完成（最长允许等待 1 分钟）
9. 等待 8 秒，让后台命令自然结束
10. 在同一个对话输入框输入以下 Prompt：
    ```
    请使用 TaskOutput 读取刚才后台任务的输出，并告诉我读到了什么。
    ```
11. 点击「发送」按钮
12. 等待本轮 assistant 回复完成（最长允许等待 1 分钟）
13. 找到本轮新建的对话 ID（在 `~/.renlijia/users/{scope}/conversations/` 下取 `mtime` 最新且不在 `$S_BEFORE` 里的子目录名），记为 `$CONV_ID`
14. 从 `~/.renlijia/users/{scope}/conversations/$CONV_ID/messages.jsonl` 中读取第一次后台命令工具结果里的 `task_id`，记为 `$TASK_ID`

**验收标准**

应该看到：
- 第一次 assistant 回复在点击发送后 10 秒内自然结束（流式光标消失）
- 文件 `/tmp/aijia-shell-background-done.txt` 存在
- 文件 `/tmp/aijia-shell-background-done.txt` 内容包含字面值 `aijia-bg-finished-013`
- 文件 `~/.renlijia/users/{scope}/conversations/$CONV_ID/messages.jsonl` 存在
- `messages.jsonl` 中至少一条 assistant 记录的 `toolCalls` 数组里有一个元素 `name == "Bash"` 或 `name == "PowerShell"`
- 该 `toolCalls` 元素的 `arguments.run_in_background == true`
- 第一次后台命令工具结果包含字段 `task_id == "$TASK_ID"`
- 第一次后台命令工具结果包含字段 `task_type == "local_bash"`
- `$TASK_ID` 以字母 `b` 开头
- 第二轮 assistant 记录的 `toolCalls` 数组里有一个元素 `name == "TaskOutput"`
- 该 `TaskOutput` 调用的 `arguments.task_id == "$TASK_ID"`
- 第二轮末条 assistant 记录 `content.text` 包含字面值 `aijia-bg-output-013`

不应该看到：
- 第一次 assistant 回复持续超过 10 秒仍未结束（说明命令没有进入后台）
- 第一次后台命令工具结果包含字段 `task_type == "local_agent"`
- 第二轮末条 assistant 记录 `content.text` 包含字面值 `No task found`
- 第二轮末条 assistant 记录 `content.text` 包含字面值 `missing required field`
- `messages.jsonl` 中没有任何 `name == "TaskOutput"` 的 tool call

---

## 意图-对话-014: 后台命令停止后，不再继续输出

**场景**
用户启动了一个持续运行的后台命令，后来发现不需要了。期望 AI 能用后台任务停止入口终止该命令；停止结果明确指向 shell 后台任务，且停止后命令不再继续往测试文件追加新输出。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断当前 scope：`tauri-pilot aijia where --json` 取，记为 `$SCOPE`
3. 清理可能残留的测试产物文件：`rm -f /tmp/aijia-shell-background-stop.txt`
4. 记录现有所有对话 ID（`ls ~/.renlijia/users/{scope}/conversations/`），记为集合 `$S_BEFORE`
5. 点击底部对话输入框
6. 输入以下 Prompt（一次性粘贴，不要分批）：
   ```
   请使用当前平台的命令执行工具启动一个后台命令，不要前台等待。

   要求：
   - macOS/Linux 请使用 Bash 工具
   - Windows 请使用 PowerShell 工具
   - 工具参数必须设置 run_in_background=true
   - 命令每 1 秒向 `/tmp/aijia-shell-background-stop.txt` 追加一行 `tick-014`
   - 命令持续 120 秒
   - 后台任务启动后，请在回复里告诉我 task_id
   ```
7. 点击「发送」按钮
8. 等待本轮 assistant 回复完成（最长允许等待 1 分钟）
9. 等待 4 秒，记录 `/tmp/aijia-shell-background-stop.txt` 当前行数，记为 `$LINES_BEFORE_STOP`
10. 在同一个对话输入框输入以下 Prompt：
    ```
    请使用 TaskStop 停止刚才的后台任务。
    ```
11. 点击「发送」按钮
12. 等待本轮 assistant 回复完成（最长允许等待 1 分钟）
13. 等待 4 秒，记录 `/tmp/aijia-shell-background-stop.txt` 当前行数，记为 `$LINES_AFTER_STOP`
14. 再等待 4 秒，重新记录 `/tmp/aijia-shell-background-stop.txt` 当前行数，记为 `$LINES_RECHECK`
15. 找到本轮新建的对话 ID（在 `~/.renlijia/users/{scope}/conversations/` 下取 `mtime` 最新且不在 `$S_BEFORE` 里的子目录名），记为 `$CONV_ID`
16. 从 `~/.renlijia/users/{scope}/conversations/$CONV_ID/messages.jsonl` 中读取第一次后台命令工具结果里的 `task_id`，记为 `$TASK_ID`

**验收标准**

应该看到：
- 第一次 assistant 回复在点击发送后 10 秒内自然结束（流式光标消失）
- 文件 `/tmp/aijia-shell-background-stop.txt` 存在
- `$LINES_BEFORE_STOP >= 1`
- `$LINES_AFTER_STOP >= $LINES_BEFORE_STOP`
- `$LINES_RECHECK == $LINES_AFTER_STOP`
- 文件 `~/.renlijia/users/{scope}/conversations/$CONV_ID/messages.jsonl` 存在
- `messages.jsonl` 中至少一条 assistant 记录的 `toolCalls` 数组里有一个元素 `name == "Bash"` 或 `name == "PowerShell"`
- 该 `toolCalls` 元素的 `arguments.run_in_background == true`
- 第一次后台命令工具结果包含字段 `task_id == "$TASK_ID"`
- 第一次后台命令工具结果包含字段 `task_type == "local_bash"`
- 第二轮 assistant 记录的 `toolCalls` 数组里有一个元素 `name == "TaskStop"`
- 该 `TaskStop` 调用的 `arguments.task_id == "$TASK_ID"`
- `TaskStop` 工具结果包含字段 `task_type == "local_bash"`

不应该看到：
- 第一次 assistant 回复持续超过 10 秒仍未结束（说明命令没有进入后台）
- 第一次后台命令工具结果包含字段 `task_type == "local_agent"`
- `TaskStop` 工具结果包含字段 `task_type == "local_agent"`
- 第二轮末条 assistant 记录 `content.text` 包含字面值 `No task found`
- `$LINES_RECHECK > $LINES_AFTER_STOP`（说明停止后命令仍在继续追加输出）

---

## 意图-对话-015: 前台长命令运行中，回复提示转后台

**场景**
用户让 AI 直接运行一个会持续几十秒的脚本任务，但没有主动要求后台运行。期望系统在前台命令超过阻塞预算后自动把任务转到后台，让当前对话恢复响应；AI 能感知到任务已转后台，回复里给出后台任务 ID，并且后续能用后台任务读取入口拿到脚本输出。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断当前 scope：`tauri-pilot aijia where --json` 取，记为 `$SCOPE`
3. 清理可能残留的测试产物文件：`rm -f /tmp/aijia-shell-auto-background.txt`
4. 记录现有所有对话 ID（`ls ~/.renlijia/users/{scope}/conversations/`），记为集合 `$S_BEFORE`
5. 点击底部对话输入框
6. 输入以下 Prompt（一次性粘贴，不要分批）：
   ```
   请使用当前平台的命令执行工具运行一个前台脚本任务，不要主动把它放到后台。

   要求：
   - macOS/Linux 请使用 Bash 工具
   - Windows 请使用 PowerShell 工具
   - 工具参数不要包含 run_in_background，或把 run_in_background 设置为 false
   - 命令每 5 秒向标准输出打印一行 `aijia-auto-bg-output-015`
   - 命令持续 45 秒
   - 命令结束前不要再调用其它工具
   - 如果系统提示这个命令已自动转到后台，请在回复里告诉我 task_id，并说明任务已在后台继续运行
   ```
7. 点击「发送」按钮
8. 等待本轮 assistant 回复完成（最长允许等待 30 秒）
9. 等待 12 秒，让后台任务继续产生输出
10. 在同一个对话输入框输入以下 Prompt：
    ```
    请使用 TaskOutput 读取刚才自动转后台任务的输出，并告诉我读到了什么。
    ```
11. 点击「发送」按钮
12. 等待本轮 assistant 回复完成（最长允许等待 1 分钟）
13. 找到本轮新建的对话 ID（在 `~/.renlijia/users/{scope}/conversations/` 下取 `mtime` 最新且不在 `$S_BEFORE` 里的子目录名），记为 `$CONV_ID`
14. 从 `~/.renlijia/users/{scope}/conversations/$CONV_ID/messages.jsonl` 中读取第一次命令工具结果里的 `task_id`，记为 `$TASK_ID`

**验收标准**

应该看到：
- 第一次 assistant 回复在点击发送后 30 秒内自然结束（流式光标消失）
- 文件 `~/.renlijia/users/{scope}/conversations/$CONV_ID/messages.jsonl` 存在
- `messages.jsonl` 中至少一条 assistant 记录的 `toolCalls` 数组里有一个元素 `name == "Bash"` 或 `name == "PowerShell"`
- 该 `toolCalls` 元素的 `arguments.run_in_background` 字段不存在，或 `arguments.run_in_background == false`
- 第一次命令工具结果包含字段 `task_id == "$TASK_ID"`
- 第一次命令工具结果包含字段 `task_type == "local_bash"`
- 第一次命令工具结果包含字段 `assistant_auto_backgrounded == true`
- `$TASK_ID` 以字母 `b` 开头
- 第一次 assistant 回复 `content.text` 包含字面值 `$TASK_ID`
- 第一次 assistant 回复 `content.text` 包含字面值 `后台`
- 第二轮 assistant 记录的 `toolCalls` 数组里有一个元素 `name == "TaskOutput"`
- 该 `TaskOutput` 调用的 `arguments.task_id == "$TASK_ID"`
- 第二轮末条 assistant 记录 `content.text` 包含字面值 `aijia-auto-bg-output-015`

不应该看到：
- 第一次 assistant 回复持续超过 30 秒仍未结束（说明前台长命令阻塞了对话）
- 第一次命令工具调用的 `arguments.run_in_background == true`（这会变成显式后台，不是自动转后台）
- 第一次命令工具结果包含字段 `task_type == "local_agent"`
- 第一次命令工具结果不含 `task_id` 字段
- 第一次 assistant 回复 `content.text` 不含 `$TASK_ID`
- 第二轮末条 assistant 记录 `content.text` 包含字面值 `No task found`
- 第二轮末条 assistant 记录 `content.text` 包含字面值 `missing required field`

---

## 意图-对话-016: 自动后台完成后，下轮收到通知

**场景**
前台长命令被系统自动转到后台后，用户没有主动轮询。期望后台命令完成时，下一轮对话能注入完成通知；AI 能基于通知告诉用户该后台任务已经结束，并指出对应 task_id。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断当前 scope：`tauri-pilot aijia where --json` 取，记为 `$SCOPE`
3. 清理可能残留的测试产物文件：`rm -f /tmp/aijia-shell-auto-background-notify.txt`
4. 记录现有所有对话 ID（`ls ~/.renlijia/users/{scope}/conversations/`），记为集合 `$S_BEFORE`
5. 点击底部对话输入框
6. 输入以下 Prompt（一次性粘贴，不要分批）：
   ```
   请使用当前平台的命令执行工具运行一个前台脚本任务，不要主动把它放到后台。

   要求：
   - macOS/Linux 请使用 Bash 工具
   - Windows 请使用 PowerShell 工具
   - 工具参数不要包含 run_in_background
   - 命令等待 25 秒，然后把 `aijia-auto-bg-notify-016` 写入 `/tmp/aijia-shell-auto-background-notify.txt`
   - 命令标准输出打印 `aijia-auto-bg-output-016`
   - 如果系统提示这个命令已自动转到后台，请在回复里告诉我 task_id
   ```
7. 点击「发送」按钮
8. 等待本轮 assistant 回复完成（最长允许等待 30 秒）
9. 等待 35 秒，让后台任务自然完成
10. 在同一个对话输入框输入以下 Prompt：
    ```
    请告诉我刚才自动转后台的任务现在有没有完成。不要重新运行命令。
    ```
11. 点击「发送」按钮
12. 等待本轮 assistant 回复完成（最长允许等待 1 分钟）
13. 找到本轮新建的对话 ID（在 `~/.renlijia/users/{scope}/conversations/` 下取 `mtime` 最新且不在 `$S_BEFORE` 里的子目录名），记为 `$CONV_ID`
14. 从 `~/.renlijia/users/{scope}/conversations/$CONV_ID/messages.jsonl` 中读取第一次命令工具结果里的 `task_id`，记为 `$TASK_ID`

**验收标准**

应该看到：
- 第一次 assistant 回复在点击发送后 30 秒内自然结束（流式光标消失）
- 文件 `/tmp/aijia-shell-auto-background-notify.txt` 存在
- 文件 `/tmp/aijia-shell-auto-background-notify.txt` 内容包含字面值 `aijia-auto-bg-notify-016`
- 文件 `~/.renlijia/users/{scope}/conversations/$CONV_ID/messages.jsonl` 存在
- 第一次命令工具调用的 `arguments` JSON 中不含 `run_in_background` 字段
- 第一次命令工具结果包含字段 `task_id == "$TASK_ID"`
- 第一次命令工具结果包含字段 `task_type == "local_bash"`
- 第一次命令工具结果包含字段 `assistant_auto_backgrounded == true`
- 第二轮末条 assistant 记录 `content.text` 包含字面值 `$TASK_ID`
- 第二轮末条 assistant 记录 `content.text` 包含字面值 `完成`
- 第二轮末条 assistant 记录 `content.text` 包含字面值 `aijia-auto-bg-output-016`

不应该看到：
- 第一次命令工具调用的 `arguments.run_in_background == true`
- 第一次命令工具结果包含字段 `task_type == "local_agent"`
- 第二轮 assistant 记录的 `toolCalls` 数组里有命令执行工具调用（说明 AI 重新运行了命令）
- 第二轮末条 assistant 记录 `content.text` 包含字面值 `No task found`
- 第二轮末条 assistant 记录 `content.text` 包含字面值 `没有找到`

---

## 意图-对话-017: 自动后台任务停止后，不再追加输出

**场景**
前台长命令被系统自动转到后台后，用户决定终止它。期望 AI 用 TaskStop 停止这个自动后台任务；停止结果仍然标识为 shell 后台任务，且停止后测试文件行数不再增长。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断当前 scope：`tauri-pilot aijia where --json` 取，记为 `$SCOPE`
3. 清理可能残留的测试产物文件：`rm -f /tmp/aijia-shell-auto-background-stop.txt`
4. 记录现有所有对话 ID（`ls ~/.renlijia/users/{scope}/conversations/`），记为集合 `$S_BEFORE`
5. 点击底部对话输入框
6. 输入以下 Prompt（一次性粘贴，不要分批）：
   ```
   请使用当前平台的命令执行工具运行一个前台脚本任务，不要主动把它放到后台。

   要求：
   - macOS/Linux 请使用 Bash 工具
   - Windows 请使用 PowerShell 工具
   - 工具参数不要包含 run_in_background
   - 命令每 1 秒向 `/tmp/aijia-shell-auto-background-stop.txt` 追加一行 `tick-017`
   - 命令持续 120 秒
   - 如果系统提示这个命令已自动转到后台，请在回复里告诉我 task_id
   ```
7. 点击「发送」按钮
8. 等待本轮 assistant 回复完成（最长允许等待 30 秒）
9. 等待 4 秒，记录 `/tmp/aijia-shell-auto-background-stop.txt` 当前行数，记为 `$LINES_BEFORE_STOP`
10. 在同一个对话输入框输入以下 Prompt：
    ```
    请使用 TaskStop 停止刚才自动转后台的任务。
    ```
11. 点击「发送」按钮
12. 等待本轮 assistant 回复完成（最长允许等待 1 分钟）
13. 等待 4 秒，记录 `/tmp/aijia-shell-auto-background-stop.txt` 当前行数，记为 `$LINES_AFTER_STOP`
14. 再等待 4 秒，重新记录 `/tmp/aijia-shell-auto-background-stop.txt` 当前行数，记为 `$LINES_RECHECK`
15. 找到本轮新建的对话 ID（在 `~/.renlijia/users/{scope}/conversations/` 下取 `mtime` 最新且不在 `$S_BEFORE` 里的子目录名），记为 `$CONV_ID`
16. 从 `~/.renlijia/users/{scope}/conversations/$CONV_ID/messages.jsonl` 中读取第一次命令工具结果里的 `task_id`，记为 `$TASK_ID`

**验收标准**

应该看到：
- 第一次 assistant 回复在点击发送后 30 秒内自然结束（流式光标消失）
- 文件 `/tmp/aijia-shell-auto-background-stop.txt` 存在
- `$LINES_BEFORE_STOP >= 1`
- `$LINES_AFTER_STOP >= $LINES_BEFORE_STOP`
- `$LINES_RECHECK == $LINES_AFTER_STOP`
- 文件 `~/.renlijia/users/{scope}/conversations/$CONV_ID/messages.jsonl` 存在
- 第一次命令工具调用的 `arguments` JSON 中不含 `run_in_background` 字段
- 第一次命令工具结果包含字段 `task_id == "$TASK_ID"`
- 第一次命令工具结果包含字段 `task_type == "local_bash"`
- 第一次命令工具结果包含字段 `assistant_auto_backgrounded == true`
- 第二轮 assistant 记录的 `toolCalls` 数组里有一个元素 `name == "TaskStop"`
- 该 `TaskStop` 调用的 `arguments.task_id == "$TASK_ID"`
- `TaskStop` 工具结果包含字段 `task_type == "local_bash"`

不应该看到：
- 第一次命令工具调用的 `arguments.run_in_background == true`
- 第一次命令工具结果包含字段 `task_type == "local_agent"`
- `TaskStop` 工具结果包含字段 `task_type == "local_agent"`
- 第二轮末条 assistant 记录 `content.text` 包含字面值 `No task found`
- `$LINES_RECHECK > $LINES_AFTER_STOP`（说明停止后命令仍在继续追加输出）

---

## 意图-对话-018: 短前台命令结束后，不转后台

**场景**
用户让 AI 运行一个很短的前台命令。期望系统不要误把短命令注册成后台任务；AI 直接拿到命令输出并完成回复，工具结果里没有后台 task_id。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断当前 scope：`tauri-pilot aijia where --json` 取，记为 `$SCOPE`
3. 记录现有所有对话 ID（`ls ~/.renlijia/users/{scope}/conversations/`），记为集合 `$S_BEFORE`
4. 点击底部对话输入框
5. 输入以下 Prompt（一次性粘贴，不要分批）：
   ```
   请使用当前平台的命令执行工具运行一个短前台命令。

   要求：
   - macOS/Linux 请使用 Bash 工具
   - Windows 请使用 PowerShell 工具
   - 工具参数不要包含 run_in_background
   - 命令只打印一行 `aijia-short-foreground-018`
   - 不要使用 TaskOutput 或 TaskStop
   ```
6. 点击「发送」按钮
7. 等待本轮 assistant 回复完成（最长允许等待 30 秒）
8. 找到本轮新建的对话 ID（在 `~/.renlijia/users/{scope}/conversations/` 下取 `mtime` 最新且不在 `$S_BEFORE` 里的子目录名），记为 `$CONV_ID`

**验收标准**

应该看到：
- assistant 回复在点击发送后 30 秒内自然结束（流式光标消失）
- 文件 `~/.renlijia/users/{scope}/conversations/$CONV_ID/messages.jsonl` 存在
- 命令工具调用的 `arguments` JSON 中不含 `run_in_background` 字段
- 命令工具结果的输出包含字面值 `aijia-short-foreground-018`
- 末条 assistant 记录 `content.text` 包含字面值 `aijia-short-foreground-018`

不应该看到：
- 命令工具结果包含字段 `task_id`
- 命令工具结果包含字段 `assistant_auto_backgrounded == true`
- 末条 assistant 记录 `content.text` 包含字面值 `后台`
- `messages.jsonl` 中出现 `name == "TaskOutput"` 的 tool call
- `messages.jsonl` 中出现 `name == "TaskStop"` 的 tool call

---

## 意图-对话-019: 转后台后，同轮继续执行后续动作

**场景**
前台长命令被自动转到后台后，AI 不应该停在“命令还在跑”的状态。期望 AI 感知到任务已经转后台，并在同一轮继续执行用户要求的下一步动作，证明 agentic loop 没被长脚本卡死。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断当前 scope：`tauri-pilot aijia where --json` 取，记为 `$SCOPE`
3. 清理可能残留的测试产物文件：`rm -f /tmp/aijia-shell-auto-background-continue.txt /tmp/aijia-shell-auto-background-running-019.txt`
4. 记录现有所有对话 ID（`ls ~/.renlijia/users/{scope}/conversations/`），记为集合 `$S_BEFORE`
5. 点击底部对话输入框
6. 输入以下 Prompt（一次性粘贴，不要分批）：
   ```
   请按顺序完成两件事：

   第一件事：使用当前平台的命令执行工具运行一个前台脚本任务，不要主动把它放到后台。
   - macOS/Linux 请使用 Bash 工具
   - Windows 请使用 PowerShell 工具
   - 工具参数不要包含 run_in_background
   - 命令每 5 秒向标准输出打印一行 `aijia-auto-bg-running-019`
   - 命令持续 45 秒

   第二件事：如果系统提示第一件事已经自动转到后台，请立刻创建文件 `/tmp/aijia-shell-auto-background-continue.txt`，文件内容写 `aijia-continue-after-bg-019`，然后在回复里告诉我第一件事的 task_id。
   ```
7. 点击「发送」按钮
8. 等待本轮 assistant 回复完成（最长允许等待 45 秒）
9. 找到本轮新建的对话 ID（在 `~/.renlijia/users/{scope}/conversations/` 下取 `mtime` 最新且不在 `$S_BEFORE` 里的子目录名），记为 `$CONV_ID`
10. 从 `~/.renlijia/users/{scope}/conversations/$CONV_ID/messages.jsonl` 中读取第一次命令工具结果里的 `task_id`，记为 `$TASK_ID`

**验收标准**

应该看到：
- assistant 回复在点击发送后 45 秒内自然结束（流式光标消失）
- 文件 `/tmp/aijia-shell-auto-background-continue.txt` 存在
- 文件 `/tmp/aijia-shell-auto-background-continue.txt` 内容包含字面值 `aijia-continue-after-bg-019`
- 文件 `~/.renlijia/users/{scope}/conversations/$CONV_ID/messages.jsonl` 存在
- 第一次命令工具调用的 `arguments` JSON 中不含 `run_in_background` 字段
- 第一次命令工具结果包含字段 `task_id == "$TASK_ID"`
- 第一次命令工具结果包含字段 `task_type == "local_bash"`
- 第一次命令工具结果包含字段 `assistant_auto_backgrounded == true`
- `$TASK_ID` 以字母 `b` 开头
- 本轮 assistant 记录的 `toolCalls` 数组里至少有两个元素
- 第一个命令工具调用之后，后续 tool call 创建了 `/tmp/aijia-shell-auto-background-continue.txt`
- 末条 assistant 记录 `content.text` 包含字面值 `$TASK_ID`

不应该看到：
- assistant 回复持续超过 45 秒仍未结束（说明 agentic loop 被前台长脚本卡住）
- 第一次命令工具调用的 `arguments.run_in_background == true`
- 第一次命令工具结果包含字段 `task_type == "local_agent"`
- 文件 `/tmp/aijia-shell-auto-background-continue.txt` 不存在
- 末条 assistant 记录 `content.text` 不含 `$TASK_ID`

## 意图-对话-020: 前台任务输出错误流，后台读取包含错误

### 场景

用户让 agent 运行一个前台长脚本，脚本同时写 stdout 和 stderr。任务自动转后台后，用户继续读取任务输出，错误流不能因为后台切换而丢失。

### 操作步骤

1. 应用探活：`tauri-pilot aijia health-check`
2. 开启一条新对话：`tauri-pilot aijia new-task`。
3. 通过 `tauri-pilot aijia where --json` 记录 `{scope}` 和 `{conversationId}`。
4. 发送消息：请使用当前平台的命令执行工具运行一个前台脚本任务，不要主动放到后台；macOS/Linux 使用 Bash，Windows 使用 PowerShell；命令持续 25 秒，每 5 秒向 stdout 输出一行 `aijia-stdout-020`，每 5 秒向 stderr 输出一行 `aijia-stderr-020`；工具参数不要包含 `run_in_background`，或设置为 `false`。
5. 等 agent 回复任务已自动转后台。
6. 从对话消息文件 `~/.renlijia/users/{scope}/conversations/{conversationId}/messages.jsonl` 中记录自动后台任务的 `{taskId}` 和 `{outputFile}`。
7. 发送消息：请使用 TaskOutput 读取刚才自动转后台任务的输出，并告诉我读到了哪些 stdout 和 stderr 内容。
8. 等 agent 回复。

### 验收标准

应该看到：

- Bash 或 PowerShell 工具结果 JSON 中 `assistant_auto_backgrounded == true`
- Bash 或 PowerShell 工具结果 JSON 中 `status == "backgrounded"`
- Bash 或 PowerShell 工具结果 JSON 中 `task_id == "{taskId}"`
- 文件 `{outputFile}` 存在
- TaskOutput 工具结果中包含字符串 `aijia-stdout-020`
- TaskOutput 工具结果中包含字符串 `aijia-stderr-020`

不应该看到：

- TaskOutput 工具结果中不含 `No task found`
- TaskOutput 工具结果中不含 `missing required field`
- TaskOutput 工具结果中不含只有 `aijia-stdout-020` 而没有 `aijia-stderr-020` 的读取结果

## 意图-对话-021: 后台任务失败退出，通知包含失败状态

### 场景

用户运行一个前台长脚本，脚本自动转后台后以非零退出码结束。agent 下一轮必须能从系统任务通知感知失败状态和退出码，而不是误报任务完成。

### 操作步骤

1. 应用探活：`tauri-pilot aijia health-check`
2. 开启一条新对话：`tauri-pilot aijia new-task`。
3. 通过 `tauri-pilot aijia where --json` 记录 `{scope}` 和 `{conversationId}`。
4. 发送消息：请使用当前平台的命令执行工具运行一个前台脚本任务，不要主动放到后台；macOS/Linux 使用 Bash，Windows 使用 PowerShell；命令先输出 `aijia-fail-before-021`，等待 12 秒，再输出 `aijia-fail-after-021`，然后以退出码 42 结束；工具参数不要包含 `run_in_background`，或设置为 `false`。
5. 等 agent 回复任务已自动转后台。
6. 从对话消息文件 `~/.renlijia/users/{scope}/conversations/{conversationId}/messages.jsonl` 中记录自动后台任务的 `{taskId}`。
7. 等待 8 秒。
8. 发送消息：刚才自动转后台的任务现在应该已经失败退出了。请不要调用 TaskOutput，直接根据系统任务通知告诉我任务状态、退出码和 task_id。
9. 等 agent 回复。

### 验收标准

应该看到：

- Bash 或 PowerShell 工具结果 JSON 中 `assistant_auto_backgrounded == true`
- Bash 或 PowerShell 工具结果 JSON 中 `task_id == "{taskId}"`
- 对话消息文件中出现 `<task-id>{taskId}</task-id>`
- 对话消息文件中出现 `<status>failed</status>`
- 对话消息文件中出现 `exit code 42`
- assistant 最终回复中包含 `{taskId}`
- assistant 最终回复中包含 `failed` 或 `失败`
- assistant 最终回复中包含 `42`

不应该看到：

- 对话消息文件中不含 `<status>completed</status>` 作为 `{taskId}` 的最终状态
- assistant 最终回复中不含 `completed` 或 `完成` 作为 `{taskId}` 的最终状态
- assistant 最终回复中不含 `TaskOutput` 工具调用结果摘要

## 意图-对话-022: 后台切换前已有输出，读取不丢行

### 场景

前台长脚本在自动转后台之前已经输出多行内容。后台切换后，TaskOutput 读取到的 transcript 必须包含切换前和切换后的输出。

### 操作步骤

1. 应用探活：`tauri-pilot aijia health-check`
2. 开启一条新对话：`tauri-pilot aijia new-task`。
3. 通过 `tauri-pilot aijia where --json` 记录 `{scope}` 和 `{conversationId}`。
4. 发送消息：请使用当前平台的命令执行工具运行一个前台脚本任务，不要主动放到后台；macOS/Linux 使用 Bash，Windows 使用 PowerShell；命令每 1 秒输出一行，内容依次为 `aijia-pre-bg-022-1` 到 `aijia-pre-bg-022-15`，总持续约 15 秒；工具参数不要包含 `run_in_background`，或设置为 `false`。
5. 等 agent 回复任务已自动转后台。
6. 从对话消息文件 `~/.renlijia/users/{scope}/conversations/{conversationId}/messages.jsonl` 中记录自动后台任务的 `{taskId}`。
7. 等待 8 秒。
8. 发送消息：请使用 TaskOutput 从 offset 0 读取刚才自动转后台任务的全部输出，并告诉我第一行和最后一行分别是什么。
9. 等 agent 回复。

### 验收标准

应该看到：

- Bash 或 PowerShell 工具结果 JSON 中 `assistant_auto_backgrounded == true`
- TaskOutput 工具结果中包含 `aijia-pre-bg-022-1`
- TaskOutput 工具结果中包含 `aijia-pre-bg-022-2`
- TaskOutput 工具结果中包含 `aijia-pre-bg-022-10`
- TaskOutput 工具结果中包含 `aijia-pre-bg-022-15`
- assistant 最终回复中包含 `aijia-pre-bg-022-1`
- assistant 最终回复中包含 `aijia-pre-bg-022-15`

不应该看到：

- TaskOutput 工具结果中不含缺失 `aijia-pre-bg-022-1` 的读取结果
- TaskOutput 工具结果中不含缺失 `aijia-pre-bg-022-10` 的读取结果
- TaskOutput 工具结果中不含 `No task found`

## 意图-对话-023: 连续读取后台输出，偏移不重复

### 场景

用户分两次读取同一个自动后台任务的输出。第二次读取使用第一次返回的 offset，结果不能重复返回第一批行，也不能漏掉新产生的行。

### 操作步骤

1. 应用探活：`tauri-pilot aijia health-check`
2. 开启一条新对话：`tauri-pilot aijia new-task`。
3. 通过 `tauri-pilot aijia where --json` 记录 `{scope}` 和 `{conversationId}`。
4. 发送消息：请使用当前平台的命令执行工具运行一个前台脚本任务，不要主动放到后台；macOS/Linux 使用 Bash，Windows 使用 PowerShell；命令每 2 秒输出一行，内容依次为 `aijia-offset-023-1` 到 `aijia-offset-023-30`；工具参数不要包含 `run_in_background`，或设置为 `false`。
5. 等 agent 回复任务已自动转后台。
6. 从对话消息文件 `~/.renlijia/users/{scope}/conversations/{conversationId}/messages.jsonl` 中记录自动后台任务的 `{taskId}`。
7. 发送消息：请使用 TaskOutput 从 offset 0 读取刚才自动转后台任务的输出，并告诉我返回的 new_offset。
8. 等 agent 回复，并记录 `{offset1}`。
9. 等待 6 秒。
10. 发送消息：请使用 TaskOutput 从 offset `{offset1}` 继续读取同一个任务的输出，并告诉我这次新增了哪些行。
11. 等 agent 回复。

### 验收标准

应该看到：

- 第一次 TaskOutput 工具调用参数中 `task_id == "{taskId}"`
- 第一次 TaskOutput 工具调用参数中 `offset == 0`
- 第一次 TaskOutput 工具结果中 `new_offset == {offset1}`
- 第二次 TaskOutput 工具调用参数中 `task_id == "{taskId}"`
- 第二次 TaskOutput 工具调用参数中 `offset == {offset1}`
- 第二次 TaskOutput 工具结果中至少包含一行 `aijia-offset-023-` 前缀的新增输出

不应该看到：

- 第二次 TaskOutput 工具结果中不含第一次 TaskOutput 已经返回过的完整行
- 第二次 TaskOutput 工具调用参数中不含 `offset == 0`
- 第二次 TaskOutput 工具结果中不含 `No task found`

## 意图-对话-024: 多个长任务转后台，输出互不串线

### 场景

同一轮对话中连续启动两个前台长脚本。两个脚本都自动转后台后，用户分别读取两个 task_id 的输出，输出内容必须按 task_id 隔离。

### 操作步骤

1. 应用探活：`tauri-pilot aijia health-check`
2. 开启一条新对话：`tauri-pilot aijia new-task`。
3. 通过 `tauri-pilot aijia where --json` 记录 `{scope}` 和 `{conversationId}`。
4. 发送消息：请在同一轮中连续完成两步。第一步使用当前平台的命令执行工具运行前台长脚本 A，不要主动放到后台，命令每 5 秒输出一行 `aijia-multi-024-A`，持续 45 秒。系统提示 A 自动转后台后，不要等待 A 结束，继续运行前台长脚本 B，不要主动放到后台，命令每 5 秒输出一行 `aijia-multi-024-B`，持续 45 秒。最终回复请告诉我两个自动后台任务的 task_id。
5. 等 agent 回复。
6. 从对话消息文件 `~/.renlijia/users/{scope}/conversations/{conversationId}/messages.jsonl` 中记录任务 A 的 `{taskIdA}` 和任务 B 的 `{taskIdB}`。
7. 等待 12 秒。
8. 发送消息：请分别使用 TaskOutput 读取 task A 和 task B 的输出，并告诉我每个 task 里读到的标记。
9. 等 agent 回复。

### 验收标准

应该看到：

- 对话消息文件中出现两个不同的 Bash 或 PowerShell 自动后台工具结果
- 第一个自动后台工具结果中 `task_id == "{taskIdA}"`
- 第二个自动后台工具结果中 `task_id == "{taskIdB}"`
- `{taskIdA} != {taskIdB}`
- `{taskIdA}` 的 TaskOutput 工具结果中包含 `aijia-multi-024-A`
- `{taskIdB}` 的 TaskOutput 工具结果中包含 `aijia-multi-024-B`

不应该看到：

- `{taskIdA}` 的 TaskOutput 工具结果中不含 `aijia-multi-024-B`
- `{taskIdB}` 的 TaskOutput 工具结果中不含 `aijia-multi-024-A`
- 两个自动后台工具结果不使用相同的 `task_id`

## 意图-对话-025: 已完成任务再停止，返回已结束状态

### 场景

自动后台任务已经自然完成后，用户再请求停止它。系统不能把已完成任务误报成刚刚被成功杀掉，必须返回已结束或不可停止的明确状态。

### 操作步骤

1. 应用探活：`tauri-pilot aijia health-check`
2. 开启一条新对话：`tauri-pilot aijia new-task`。
3. 通过 `tauri-pilot aijia where --json` 记录 `{scope}` 和 `{conversationId}`。
4. 发送消息：请使用当前平台的命令执行工具运行一个前台脚本任务，不要主动放到后台；macOS/Linux 使用 Bash，Windows 使用 PowerShell；命令每 5 秒输出一行 `aijia-completed-stop-025`，持续 15 秒；工具参数不要包含 `run_in_background`，或设置为 `false`。
5. 等 agent 回复任务已自动转后台。
6. 从对话消息文件 `~/.renlijia/users/{scope}/conversations/{conversationId}/messages.jsonl` 中记录自动后台任务的 `{taskId}`。
7. 等待 12 秒。
8. 确认对话消息文件中出现 `{taskId}` 对应的 `<status>completed</status>` 通知。
9. 发送消息：请使用 TaskStop 停止刚才已经自然完成的任务，并告诉我停止结果。
10. 等 agent 回复。

### 验收标准

应该看到：

- 对话消息文件中出现 `<task-id>{taskId}</task-id>`
- 对话消息文件中出现 `<status>completed</status>`
- TaskStop 工具调用参数中 `task_id == "{taskId}"`
- TaskStop 工具结果中包含 `completed`、`already finished`、`already completed`、`已完成` 或 `已结束` 中任一明确状态
- assistant 最终回复中包含 `{taskId}`

不应该看到：

- TaskStop 工具结果中不含 `Successfully stopped task` 作为 `{taskId}` 的停止结果
- assistant 最终回复中不含 `成功停止` 作为 `{taskId}` 的停止结果
- 对话消息文件中不含 `{taskId}` 对应的第二个 `<status>completed</status>` 通知

## 意图-对话-026: 九秒前台任务运行，不自动转后台

### 场景

用户运行接近自动后台阈值但未超过阈值的前台命令。该命令应该保持前台完成，不能被错误转为后台任务。

### 操作步骤

1. 应用探活：`tauri-pilot aijia health-check`
2. 开启一条新对话：`tauri-pilot aijia new-task`。
3. 通过 `tauri-pilot aijia where --json` 记录 `{scope}` 和 `{conversationId}`。
4. 发送消息：请使用当前平台的命令执行工具运行一个 9 秒前台脚本任务，不要主动放到后台；macOS/Linux 使用 Bash，Windows 使用 PowerShell；命令先输出 `aijia-nine-sec-start-026`，等待 9 秒，再输出 `aijia-nine-sec-end-026`；工具参数不要包含 `run_in_background`，或设置为 `false`。
5. 等 agent 回复。
6. 检查对话消息文件 `~/.renlijia/users/{scope}/conversations/{conversationId}/messages.jsonl`。

### 验收标准

应该看到：

- Bash 或 PowerShell 工具结果文本中包含 `aijia-nine-sec-start-026`
- Bash 或 PowerShell 工具结果文本中包含 `aijia-nine-sec-end-026`
- assistant 最终回复中包含 `aijia-nine-sec-end-026`

不应该看到：

- Bash 或 PowerShell 工具结果 JSON 中不含 `assistant_auto_backgrounded == true`
- Bash 或 PowerShell 工具结果 JSON 中不含 `status == "backgrounded"`
- 对话消息文件中不含 `task_id` 指向本次 9 秒任务

## 意图-对话-027: 十一秒前台任务运行，自动转后台

### 场景

用户运行超过自动后台阈值的前台命令。命令不需要等到自然结束，系统应自动转后台并让 agent 感知 task_id。

### 操作步骤

1. 应用探活：`tauri-pilot aijia health-check`
2. 开启一条新对话：`tauri-pilot aijia new-task`。
3. 通过 `tauri-pilot aijia where --json` 记录 `{scope}` 和 `{conversationId}`。
4. 发送消息：请使用当前平台的命令执行工具运行一个 11 秒前台脚本任务，不要主动放到后台；macOS/Linux 使用 Bash，Windows 使用 PowerShell；命令先输出 `aijia-eleven-sec-start-027`，等待 11 秒，再输出 `aijia-eleven-sec-end-027`；工具参数不要包含 `run_in_background`，或设置为 `false`。
5. 等 agent 在 30 秒内回复。
6. 从对话消息文件 `~/.renlijia/users/{scope}/conversations/{conversationId}/messages.jsonl` 中记录自动后台任务的 `{taskId}`。

### 验收标准

应该看到：

- Bash 或 PowerShell 工具结果 JSON 中 `assistant_auto_backgrounded == true`
- Bash 或 PowerShell 工具结果 JSON 中 `status == "backgrounded"`
- Bash 或 PowerShell 工具结果 JSON 中 `task_id == "{taskId}"`
- assistant 最终回复中包含 `{taskId}`
- assistant 最终回复中包含 `后台` 或 `background`

不应该看到：

- Bash 或 PowerShell 工具结果不以纯前台文本形式只返回 `aijia-eleven-sec-start-027` 和 `aijia-eleven-sec-end-027`
- assistant 最终回复中不含 `我等命令结束后再继续` 这类等待结束表述
- 对话消息文件中不含 `{taskId}` 对应的 `No task found`

## 意图-对话-028: 短超时前台任务运行，按超时报错

### 场景

用户显式要求前台命令使用短 timeout，而命令运行时间超过 timeout 且短于自动后台阈值。系统应按前台超时处理，不能把它转成后台任务逃避 timeout。

### 操作步骤

1. 应用探活：`tauri-pilot aijia health-check`
2. 开启一条新对话：`tauri-pilot aijia new-task`。
3. 通过 `tauri-pilot aijia where --json` 记录 `{scope}` 和 `{conversationId}`。
4. 发送消息：请使用当前平台的命令执行工具运行一个前台脚本任务，不要主动放到后台；macOS/Linux 使用 Bash，Windows 使用 PowerShell；命令先输出 `aijia-timeout-start-028`，等待 8 秒，再输出 `aijia-timeout-end-028`；请把工具 timeout 设置为 5 秒；工具参数不要包含 `run_in_background`，或设置为 `false`。
5. 等 agent 回复。
6. 检查对话消息文件 `~/.renlijia/users/{scope}/conversations/{conversationId}/messages.jsonl`。

### 验收标准

应该看到：

- Bash 或 PowerShell 工具调用参数中 timeout 为 5 秒或等价毫秒值
- Bash 或 PowerShell 工具结果中包含 `timeout`、`timed out`、`超时` 中任一超时状态
- assistant 最终回复中包含 `timeout`、`timed out`、`超时` 中任一超时描述

不应该看到：

- Bash 或 PowerShell 工具结果 JSON 中不含 `assistant_auto_backgrounded == true`
- Bash 或 PowerShell 工具结果 JSON 中不含 `status == "backgrounded"`
- Bash 或 PowerShell 工具结果中不含 `aijia-timeout-end-028`

## 意图-对话-029: 前台长子任务，自动转后台

### 场景

用户让 AI 通过 Agent 工具处理一个会超过前台等待预算的子任务，但没有显式要求 `run_in_background=true`。期望 Agent 工具先以前台方式启动，超过预算后自动返回 `task_id`，工具结果标出 `assistant_auto_backgrounded=true` 且 `task_type == "local_agent"`；后续再用 TaskOutput 读取同一个子任务的输出，能看到 `aijia-agent-auto-bg-029`。

### 操作步骤

1. 应用探活：`tauri-pilot aijia health-check`
2. 打开新对话：`tauri-pilot aijia new-task`
3. 通过 `tauri-pilot aijia where --json` 记录 `{scope}` 和 `{conversationId}`
4. 发送消息：请使用 Agent 工具运行一个前台子 Agent 任务，不要主动放到后台；工具参数不要包含 `run_in_background`，或设置为 `false`。请让这个子 Agent 只做一件事：先等待超过 20 秒，再返回字符串 `aijia-agent-auto-bg-029`。当系统把子任务自动转后台后，请不要等待子任务自然结束，立刻告诉我 task_id。
5. 等待 assistant 在 45 秒内返回，记录这次自动后台化对应的 `{taskId}`。
6. 等待 25 秒。
7. 发送消息：请使用 TaskOutput 从 offset 0 读取刚才 `{taskId}` 的输出，并告诉我是否读到了 `aijia-agent-auto-bg-029`。
8. 等待 assistant 回复。

### 验收标准

应该看到：

- `messages.jsonl` 中出现 `name == "Agent"` 的 tool call
- 该 `Agent` tool call 的 `arguments` 中不含 `run_in_background`，或其值为 `false`
- Agent 工具结果 JSON 中 `assistant_auto_backgrounded == true`
- Agent 工具结果 JSON 中 `task_type == "local_agent"`
- Agent 工具结果 JSON 中 `task_id == "{taskId}"`
- 第一次 assistant 回复中包含 `{taskId}`
- 第二轮 assistant 记录的 `toolCalls` 数组里有一个元素 `name == "TaskOutput"`
- 该 `TaskOutput` 调用的 `arguments.task_id == "{taskId}"`
- 该 `TaskOutput` 调用的 `arguments.offset == 0`
- `TaskOutput` 工具结果中包含字符串 `aijia-agent-auto-bg-029`
- 第二次 assistant 回复中包含 `aijia-agent-auto-bg-029`

不应该看到：

- `TaskOutput` 工具结果中不含 `No task found`
- Agent 工具结果 JSON 中不含 `task_type == "local_bash"`
- Agent 工具结果 JSON 中不含 `assistant_auto_backgrounded == false`
- 第一次 assistant 回复不应等到 `aijia-agent-auto-bg-029` 出现后才返回
