# rules.md — settings（设置分层）意图测试规格

## 测试范围

覆盖设置的三层优先级（user / workspace / global）与变更生效时序：workspace 级覆盖 global 级、user 级在多账号间隔离、settings 变更后下一个 turn 立即读到新值（不需要重启）。不包含具体设置面板的 UI 控件行为。

## 待覆盖的主要场景

- 场景 1：workspace 设置某字段，global 同一字段被覆盖，当前 workspace 下该字段读取到 workspace 值
- 场景 2：workspace 没有覆盖某字段时，回退读 global 值
- 场景 3：账号 A 与账号 B 的 user 级设置互不可见，切换账号后读到自己的值
- 场景 4：在 turn 进行中修改 settings，当前 turn 仍用旧值（避免半路换配置），下一个 turn 立即读到新值
- 场景 5：settings 文件被外部破坏（JSON 解析失败）时，应用回退到默认值并给出可恢复提示，不崩
- 场景 6：删除 workspace 级设置（恢复默认）后，立刻回退读 global 值

---

## 意图 1：修改全局 API key 后，下一次发消息使用新的 key

**场景**
用户在某个 LLM provider 的 API key 过期后，在设置里换上新 key。系统不需要重启，下一条发出去的消息直接用新 key 鉴权——���对话此前因为旧 key 失效报错，换 key 后再发立刻成功。

**前提**
- 应用已启动并登录
- 选定一个用户自配的 LLM provider（例如 custom provider 或一个明确填写了 API key 的渠道）
- 在「设置 → 模型 → API key」中保存的当前 key 已经过期 / 被吊销（手工设为一段一定会被服务端 401 的字符串，例如 `"sk-this-key-is-invalid-xyz"`）
- 新建一个空对话，记录 conv_id

**操作**
1. 在输入框输入 `"你好"`，点击发送
2. 等待 AI 完整回复或错误结束
3. 不重启应用，回到「设置 → 模型 → API key」，把字段值改为一个真实可用的 key，点击保存
4. 回到对话页（仍是同一个 conv_id），在输入框输入 `"再来一次"`，点击发送
5. 等待 AI 完整回复

**验收标准**
- 第 1 轮发送后，UI 出现错误提示，提示内容包含 `401` / `认证` / `key` / `unauthorized` 中的至少一项
- 第 1 轮结束后，`~/.renlijia/users/{scope}/conversations/{conv_id}/messages.jsonl` 共 2 条记录，第 1 条 `role` 为 `"user"`、`content.text` 为 `"你好"`，第 2 条为 assistant 错误占位或 turn 失败记录（具体取决于现有错误持久化策略，但本条存在）
- 第 4 轮（改 key 后）发送，UI 不再出现 401 / 认证错误，AI 气泡正常流式输出
- 第 4 轮结束后，`messages.jsonl` 至少新增 2 条记录（user `"再来一次"` + assistant 非空回复）
- 「设置 → 模型 → API key」字段值显示为新 key（或其脱敏形式，前几位 / 后几位明文与刚才输入的新 key 一致）

---

## 意图 2：workspace 级 settings 覆盖 global 同名字段，在该 workspace 对话中生效

**场景**
用户希望某个项目用一个特殊的模型 / 不同的 temperature / 不同的工具白名单，但全局默认不变。在 workspace 的 `settings.json` 中覆盖一个具体字段后，只有这个 workspace 下的对话用 workspace 值，切到没有覆盖该字段的别的 workspace 则继续用 global 值。

**前提**
- 应用已启动并登录
- 在「设置 → 全局」中将 temperature 字段保存为 `0.2`（V_global）
- 准备两个授权过的 workspace：A（路径 `~/Projects/proj-a`）和 B（路径 `~/Projects/proj-b`）
- A 已经在路径下激活过；A 的 `~/Projects/proj-a/.aijia/settings.json` 文件内容为 `{ "temperature": 0.9 }`（V_workspace）
- B 没有 `.aijia/settings.json` 文件
- 关闭应用后重启并登录（确保 workspace 级 settings 已被读取）

**操作**
1. 切换到 workspace A，新建对话 conv_A，发送 `"你好 A"`，等待 AI 回复完成
2. 切换到 workspace B，新建对话 conv_B，发送 `"你好 B"`，等待 AI 回复完成

**验收标准**
- conv_A 目录下任一记录 turn temperature 的字段（在 `conv.json` 或 turn 元数据中），值为 `0.9`
- conv_B 目录下同一字段值为 `0.2`
- `~/Projects/proj-a/.aijia/settings.json` 在两轮操作完成后内容仍为 `{ "temperature": 0.9 }`（应用没把 workspace 设置改回去）
- 「设置 → 全局」UI 中显示的 temperature 字段仍为 `0.2`（workspace 覆盖不污染 global）
- 两个对话的 `messages.jsonl` 各自共 2 条记录，assistant 记录 `content.text` 均非空（说明覆盖不影响 turn 完成）

---

## 意图 3：settings 文件 JSON 非法时，应用启动不崩溃，使用默认值，设置页面可正常访问

**场景**
用户的全局 settings 文件因为某种原因（手工编辑出错、磁盘坏块、上一次崩溃写一半）变成了非法 JSON。应用下次启动不应当卡在白屏 / 崩溃，应当退回默认值继续运行，并且用户能进设置页把它改回去。

**前提**
- 应用已关闭
- 找到全局 settings 文件路径 `~/.renlijia/settings.json`（如果项目用不同路径，按实际路径调整，下同）
- 备份原文件：`cp ~/.renlijia/settings.json /tmp/settings_bak.json`
- 用文本编辑器把 `~/.renlijia/settings.json` 的内容改为非法 JSON，例如：`{ "temperature": 0.5, "broken": [ }`（保存）
- 准备好：测试结束后恢复 `cp /tmp/settings_bak.json ~/.renlijia/settings.json`

**操作**
1. 启动应用，等待主界面加载完成
2. 登录到任一已存在账号
3. 新建一个对话，发送 `"你好"`，等待 AI 完整回复
4. 打开「设置」页面，浏览各分组（模型 / 工具 / 隐私 / 工作区 等）
5. 在「设置 → 全局 → temperature」中把字段改为 `0.3`，点击保存
6. 关闭应用，恢复 `~/.renlijia/settings.json` 为备份内容

**验收标准**
- 第 1 步：应用主窗口能正常显示，未出现原生崩溃对话框 / 白屏超过 10 秒 / 无法点击
- 第 1 步：应用日志（`~/.renlijia/logs/` 下最新日志文件）中出现至少一行包含 `settings` 且包含 `parse` / `invalid` / `corrupt` / `fallback` / `default` 中至少一个关键字的 warn / error 行
- 第 3 步：AI 能正常回复，`messages.jsonl` 共 2 条记录，第 2 条 `role` 为 `"assistant"`、`content.text` 非空
- 第 4 步：「设置」页面所有分组都能打开，未出现 React error boundary 报错横幅 / "Something went wrong" 提示
- 第 4 步：「设置 → 全局 → temperature」字段显示为某个默认值（具体值取决于代码默认，但不为空、不为 NaN）
- 第 5 步：保存后 `~/.renlijia/settings.json` 重新变为合法 JSON（可用 `python -m json.tool ~/.renlijia/settings.json` 验证），且其中 `temperature` 字段值为 `0.3`
