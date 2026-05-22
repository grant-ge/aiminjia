# rules.md — workspace（工作区）意图测试规格

## 测试范围

覆盖 workspace（工作区）的发现、切换与上下文隔离：用户在已有项目目录下启动对话时 workspace 自动识别、workspace 级别设置对 global 设置的覆盖关系、以及在多个 workspace 间切换时上下文（对话历史 / 记忆 / 生成物路径）严格隔离。不包含 workspace 内部具体文件结构的字段级验证。

## 待覆盖的主要场景

- 场景 1：用户在一个已经存在 workspace 标记的目录下启动对话，workspace 被自动发现并激活，不弹新建向导
- 场景 2：workspace 级别覆盖 global 设置的字段在当前 turn 立即生效，未覆盖的字段继续走 global
- 场景 3：从 workspace A 切到 workspace B 后，B 看不到 A 的对话历史 / 记忆 / uploads / reports
- 场景 4：在没有任何 workspace 标记的临时目录下启动，回退到默认 home workspace，不污染其他 workspace
- 场景 5：workspace 目录被外部删除后重新启动，应用不崩，给出可恢复的错误提示
- 场景 6：同一 workspace 被两个窗口同时打开时，对状态文件的并发写不丢数据

---

## 意图 1：在已存在项目标记的目录下发起对话，workspace 被自动识别

**场景**
用户在 Finder / 文件管理器里找到一个已经存在 `.git/` 或 `CLAUDE.md` 的项目目录，把这个目录授权给应用，新建的对话能在该目录上下文中工作，AI 引用的相对路径都解析到该目录而不是默认 `~/.renlijia/`。

**前提**
- 应用已启动并登录
- 准备一个本地目录 `~/Projects/demo-app`，目录下至少存在 `.git/` 或一个 `CLAUDE.md` 文件
- 该目录之前没有被授权过（执行 `multica` 工具或在「设置 → 工作区」中确认该路径不在已授权列表）

**操作**
1. 新建一个对话，记录 conv_id
2. 在对话页通过「授权本地目录」入口选择 `~/Projects/demo-app`
3. 在输入框输入 `"列出当前工作区根目录下的文件"`，点击发送
4. 等待 AI 完整回复

**验收标准**
- 前端在对话页顶部显示工作区名为 `demo-app`（目录最后一级名）
- AI 回复中至少出现一个 `~/Projects/demo-app` 目录下的真实文件名或子目录名（如 `.git`、`CLAUDE.md`、`README.md` 等真实存在的项）
- AI 回复中不包含 `~/.renlijia` 路径或 `.renlijia` 子目录名
- `~/.renlijia/users/{scope}/conversations/{conv_id}/conv.json` 中存在工作区授权相关字段，其值的根路径字符串包含 `Projects/demo-app`
- 对话授权完成后未弹出任何「新建工作区」或「初始化项目」向导

---

## 意图 2：workspace 级 settings.json 覆盖 global 同名字段，且仅对该 workspace 对话生效

**场景**
用户希望某个项目用一个特殊的模型温度，但其他项目继续用默认。在该项目的 workspace settings.json 中改一个字段后，只有这个 workspace 下的新对话用新值，其他 workspace 不受影响。

**前提**
- 应用已启动并登录
- 已有两个授权过的 workspace：A（路径 `~/Projects/proj-a`）和 B（路径 `~/Projects/proj-b`）
- A 和 B 都已经在该路径下创建过对话（即两个 workspace 都已激活过）
- 在「设置 → 全局」中将「模型 temperature」（或任一可观察的设置项，比如默认 system prompt 附加内容）调整为已知值 V_global，保存
- 关闭应用，编辑 `~/Projects/proj-a/.aijia/settings.json`（若不存在则创建），写入只覆盖该字段的 JSON：`{ "temperature": V_workspace }`（V_workspace ≠ V_global）
- 重新启动应用并登录

**操作**
1. 在 workspace A 下新建对话 conv_A，发送任意一条消息，等待回复完成
2. 切换到 workspace B，新建对话 conv_B，发送任意一条消息，等待回复完成

**验收标准**
- `~/.renlijia/users/{scope}/conversations/{conv_A}/conv.json` 或同一目录下的 turn 元数据中，记录的 temperature 字段值等于 V_workspace
- `~/.renlijia/users/{scope}/conversations/{conv_B}/conv.json` 或同一目录下的 turn 元数据中，记录的 temperature 字段值等于 V_global
- 「设置 → 全局」中显示的 temperature 字段值依旧为 V_global（workspace 设置不污染 global 显示）
- `~/Projects/proj-a/.aijia/settings.json` 文件内容未被应用启动后改动（fs stat 大小 / mtime 与启动前一致）

---

## 意图 3：切换 workspace 后新对话使用新 workspace 设置，旧对话不受影响

**场景**
用户在 workspace A 起了一个对话，期间切到 workspace B 起新对话，应用应当让新对话用 B 的设置，而原来 A 的对话仍按 A 的设置继续工作。切回 A 时 A 的对话历史依然在。

**前提**
- 应用已启动并登录
- workspace A（`~/Projects/proj-a/.aijia/settings.json` 含 `"temperature": 0.2`）
- workspace B（`~/Projects/proj-b/.aijia/settings.json` 含 `"temperature": 0.9`）
- 两个 workspace 都被授权过且都至少激活过一次

**操作**
1. 切换到 workspace A，新建对话 conv_A，发送消息 `"A 的第一条"`，等待回复完成
2. 通过「工作区切换」UI 切换到 workspace B
3. 在 workspace B 下新建对话 conv_B，发送消息 `"B 的第一条"`，等待回复完成
4. 切回 workspace A，打开 conv_A，发送消息 `"A 的第二条"`，等待回复完成

**验收标准**
- conv_A 目录 `~/.renlijia/users/{scope}/conversations/{conv_A}/messages.jsonl` 共 4 条记录（两轮 user + assistant），第 1 条 `content.text` 为 `"A 的第一条"`，第 3 条 `content.text` 为 `"A 的第二条"`
- conv_A 目录下任一记录 turn temperature 的字段，前后两轮值均为 `0.2`
- conv_B 目录 `~/.renlijia/users/{scope}/conversations/{conv_B}/messages.jsonl` 共 2 条记录，第 1 条 `content.text` 为 `"B 的第一条"`
- conv_B 目录下记录 turn temperature 的字段值为 `0.9`
- 工作区侧边栏切换到 A 时显示 conv_A 在对话列表中，切到 B 时 conv_A 不出现在 B 的对话列表里
