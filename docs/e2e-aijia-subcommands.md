# tauri-pilot `aijia` 子命令清单（以实现为准）

> 最后更新：2026-05-18（按真实跑出的命令行为对齐）
> 实现位置：`~/IdeaProjects/tauri-pilot/crates/tauri-pilot-cli/src/aijia.rs` + `cli.rs`
> 安装：`cargo install --path crates/tauri-pilot-cli`

---

## 设计原则

### 原则 1：只暴露原子能力，编排留给调用者

不打包 `aijia chat` 这类业务流程命令——业务流程会变，CLI 不应该跟着变。

```bash
# 标准用法（Unix 哲学，用 && 串）
aijia new-task && \
aijia type-message "你好" && \
aijia send && \
aijia wait-reply && \
aijia last-reply
```

### 原则 2：选择器靠稳定锚点

实现内部按以下优先级找元素（脚本作者不用关心）：

1. **store 直读**：`window.__aijia.chatStore.getState()`（最稳，无 DOM 依赖）
2. **`data-aijia-*` 属性**：Phase 0 在前端补的稳定钩子
3. **`.ProseMirror` / `button[aria-label="发送"]`** 等 DOM CSS selector
4. **可见中文文本**（兜底）：`button:textContent === '新任务'`

### 原则 3：失败要明确

每个命令要么返回 `{ok: true, ...}` 要么返回 `{ok: false, reason: "..."}`，**不静默无操作**。

### 原则 4：默认行为只看"有用信号"

`ui-message` 默认过滤掉 `text==""` 且无 `tool_calls` 的空气泡（cancel 后的流式占位残影）。要保留这些噪音用 `--include-empty`。

---

## 🔒 铁则

**所有 aijia 业务行为必须经过 `tauri-pilot aijia <subcommand>`，禁止在 e2e 脚本里直接调通用命令**（click / fill / eval / snapshot / screenshot）。

通用命令只能出现在 `aijia` 子命令的 Rust 实现内部。脚本里 grep 到任一 = 违反铁则。

---

## 命令清单（16 个 + 5 个新选项）

### 组 1：会话流（chat mainline，6 个 P0 命令）

#### A1. `aijia new-task [--wait-fresh]`

点侧栏"新任务"，路由到 `/`。

**实现**：扫 `button` 找 `textContent==='新任务'` → click。

**选项**：
- `--wait-fresh`（默认 false）：等到 `chatStore.activeConversationId === null && hasEditor`（最多 2s），表示已落到新任务页且编辑器就绪。不加这个选项时 click 立即返回，`where.sessionId` / `messageCount` 可能仍是上次会话的残值。
  - 注意 1：不是等 `messages` 清空 —— store 的 `messages` 是按 activeId 投影的宽数组，切换会话时不会主动清。
  - 注意 2：新会话是 lazy 创建，`activeConversationId=null` 只是"已离开旧会话进入新任务页"，真正的新 `conv_id` 要等 `send` 后才生成。

**返回**：`{ok: true}` 或 `{ok: false, reason: "new-task button not found"}`

---

#### A2. `aijia type-message <text>`

在 `.ProseMirror` 中执行 `document.execCommand('insertText', false, text)`。

**注**：Tiptap 拒绝合成 `input` 事件，`execCommand` 是 PoC 阶段确认的唯一可靠路径。

**返回**：`{ok: true, text: "<编辑器实际 textContent>"}` 或 `{ok: false, reason: "ProseMirror editor not found"}`

---

#### A3. `aijia send`

点击 `button[aria-label="发送"]`。

**返回**：`{ok: true}` / `{ok: false, reason: "send button disabled"}`

---

#### A4. `aijia cancel [--wait]`

点击 `button[aria-label="停止"]`（流式中才有这个按钮）。

**选项**：
- `--wait`（默认 false）：等到 `chatStore.isStreaming === false`（最多 5s）。

**返回**：`{ok: true}` / `{ok: false, reason: "stop button not found (not streaming?)"}`

---

#### A5. `aijia wait-reply [--timeout 30]`

阻塞等流式回复完成。

**实现**：每 300ms 探测 `chatStore.isStreaming === false && !stopBtnVisible`，**3 连续 ready tick** 才认定收口（防止 tool calls 之间的瞬时 false 误判）。

**返回**：`{ok: true, probe: {...}, stableTicks: 3}` / `{ok: false, reason: "timeout", timeoutSec: N, lastProbe: {...}}`

---

#### A6. `aijia ui-message [--last N] [--role user|assistant|tool_call] [--include-tools] [--include-empty]`

dump 当前会话的所有消息（从 `chatStore.messages`）。**返回顶层 array**。

**默认行为**（2026-05-18 起）：过滤 `text==""` 且 `tool_calls` 为空的空气泡——这种气泡是 cancel 后的流式占位残影，对断言无意义。

**选项**：
- `--last N`：只返回最后 N 条
- `--role user|assistant|tool_call`：按 role 过滤
- `--include-tools`：是否包含 assistant 消息上的 tool_calls 字段（默认 true）
- `--include-empty`：保留空气泡（默认 false）

**返回**（顶层 array）：
```json
[
  {"id": "...", "index": 0, "role": "user", "text": "你好", "tool_calls": []},
  {"id": "...", "index": 1, "role": "assistant", "text": "你好啊", "tool_calls": []}
]
```

**注意**：消息内容字段是 `text` 不是 `content`；磁盘 `messages.jsonl` 里 `content` 是 `{text: "..."}` 嵌套对象。

---

#### A7. `aijia last-reply`

`ui-message --last 1 --role assistant` 的快捷别名，但返回**单个对象**（不是数组），方便管道。

**返回**：`{id, index, role: "assistant", text, tool_calls}` 或 `null`

---

### 组 2：会话管理（4 个 P1 命令）

#### A8. `aijia list-sessions`

列出侧栏所有会话。**返回顶层 array**。

**返回**：
```json
[
  {"index": 0, "id": "abc-123", "title": "新对话", "active": true, "archived": false},
  {"index": 1, "id": "def-456", "title": "调研笔记", "active": false, "archived": false}
]
```

**字段名注意**：实际字段是 `active` / `archived` / `title`（不是 `isActive` / `isArchived` / `name`）。`archived` 字段同时兼容后端的 `isArchived` / `lifecycle: 'Archived'`。

---

#### A9. `aijia switch-session <id|index>`

切换会话。id 优先（按全 UUID 匹配），全数字按 index（0=最新）。

**实现**：从 store 解析 id → click 侧栏 `[data-aijia-conversation-id="<id>"]` row（走 UI 路径，不直接改 store）。

**返回**：`{ok: true, id, title}` / `{ok: false, reason: "conversation not found"}`

---

#### A10. `aijia archive-session <id|index> [--wait]`

归档会话。走 Tauri IPC `archive_conversation`，**不走 UI hover**（pointer model 不稳）。Tauri IPC arg 是 camelCase：`{conversationId: "..."}`。

**选项**：
- `--wait`（默认 false）：通过 `get_conversations` IPC **直接查后端**，等到目标 id 的 `isArchived=true` 或从列表中消失（hidden）（最多 3s）。
  - **为什么不查前端 store**：UI 路径走的是 `useChat.archiveConversation` 的乐观更新（filter 出列表 + IPC 调用），但我们 IPC 直调跳过了这条路径，前端 `chatStore.conversations` 永远不会自动刷新。所以 `--wait` 直接走后端 IPC 查真实状态。
  - 返回 `confirmed: true` 表示后端真的归档了；`hiddenFromList: true` 表示该 id 已不在 `get_conversations` 返回里（后端默认隐藏 archived 会话）。

**返回**：`{ok: true, archived: "<id>"}` / `{ok: false, reason: "conversation not found"}`

---

#### A11. `aijia cleanup-test-sessions [--prefix e2e-test-]`

批量归档 title 前缀匹配的会话。

**重要**：**只匹配 title**。首条用户消息不会更新 title——脚本里要先 rename（UI: ⋯ → 重命名聊天）才能让前缀匹配生效。

**返回**：`{ok: bool, prefix, archived: [{id, title}], failed: [...]}`

---

### 组 3：诊断（3 个命令）

#### C1. `aijia where`

当前 UI 状态快照。

**返回**：
```json
{
  "url": "http://127.0.0.1:5173/",
  "route": "/",
  "title": "AI小家 — 你的智能工作助手",
  "sessionId": "abc-123",
  "sessionName": "新对话",
  "isStreaming": false,
  "isSending": false,
  "hasToolCallBlock": false,
  "messageCount": 2,
  "lastError": null,
  "hasEditor": true,
  "workspace": null,
  "model": null
}
```

`workspace` / `model` 字段当前总是 `null`（store 未暴露），显式占位方便日后接入。

---

#### C2. `aijia screenshot --label <label> [--selector <css>]`

截图到 `/tmp/aijia-e2e-<label>-<timestamp>.png`。

**参数名**：是 `--label` 不是 `--name`（设计稿曾误写）。

**选项**：
- `--selector`：CSS 选择器限定截图区域。默认 `[data-aijia-message-list]`（如果存在）否则 `body`。**全文档截图通常会撞 html-to-image 30s 超时**，所以默认带 selector 限制范围。

**注**：当前 plugin 的 `bridge.js` 默认 `skipFonts:true` 才能避免大 DOM 超时；如果 plugin 没用最新 bridge（需 `touch crates/tauri-plugin-pilot/src/lib.rs` 强制重编），`aijia screenshot` 可能 30s 超时——可以用 raw `tauri-pilot screenshot <path>` 兜底，~100ms 完成。

---

#### C3. `aijia health-check`

启动后第一个跑，验证 app ready。

**返回**：`{ok: true, readyState: "complete", hasEditor: bool, activeConversationId: "..."}`

---

### 组 4：明确占位（未实现，不要依赖）

#### A12. `aijia select-workspace <name>` / A13. `aijia restart-app`

当前返回 `{ok: false, reason: "not implemented"}`，rules.md 不要依赖这两个命令。

---

## 用法示例

### 一回合对话 + 验证

```bash
tauri-pilot aijia new-task --wait-fresh
tauri-pilot aijia type-message "你好"
tauri-pilot aijia send
tauri-pilot aijia wait-reply --timeout 60
reply=$(tauri-pilot aijia last-reply --json | python3 -c "import json,sys; print(json.load(sys.stdin)['text'])")
[ -n "$reply" ] && echo "PASS" || echo "FAIL: empty reply"
```

### Cancel 流式

```bash
tauri-pilot aijia new-task --wait-fresh
tauri-pilot aijia type-message "请写一篇 500 字散文"
tauri-pilot aijia send
sleep 1.5  # 等进入流式
tauri-pilot aijia cancel --wait
# 此时 isStreaming=false, 编辑器解封
```

### 跨会话操作

```bash
# 创建一个会话留作 fixture
tauri-pilot aijia new-task --wait-fresh
tauri-pilot aijia type-message "fixture"
tauri-pilot aijia send
tauri-pilot aijia wait-reply
conv_id=$(tauri-pilot aijia where --json | python3 -c "import json,sys; print(json.load(sys.stdin)['sessionId'])")

# 跑别的测试，最后切回来再归档
tauri-pilot aijia switch-session "$conv_id"
tauri-pilot aijia archive-session "$conv_id" --wait
```

### 失败现场存档

```bash
tauri-pilot aijia where --json > /tmp/where-before.json
tauri-pilot aijia screenshot --label fail-${TEST_NAME}
```

---

## 与 rules.md 对接示例

`docs/test-intents/spec/tasks/session-runtime/rules.md` 意图 1（产品视角变体）：

```bash
MARKER="intent1-$(date +%s)"
tauri-pilot aijia new-task --wait-fresh
tauri-pilot aijia type-message "回复 marker=$MARKER"
tauri-pilot aijia send
tauri-pilot aijia wait-reply --timeout 60
conv_id=$(tauri-pilot aijia where --json | python3 -c "import json,sys; print(json.load(sys.stdin)['sessionId'])")

# UI 断言
ui_reply=$(tauri-pilot aijia last-reply --json | python3 -c "import json,sys; print(json.load(sys.stdin)['text'])")
[[ "$ui_reply" == *"$MARKER"* ]] || { echo "FAIL: marker missing in UI"; exit 1; }

# 磁盘断言（注意：messages.jsonl 单文件 ndjson，每行末尾有 \t✓ 校验位）
test -f ~/.renlijia/users/t_28__u_54/conversations/$conv_id/messages.jsonl
grep -c "$MARKER" ~/.renlijia/users/t_28__u_54/conversations/$conv_id/messages.jsonl
```

---

## 字段速查（坑点）

| 命令 | 字段 | 说明 |
|---|---|---|
| `ui-message` 消息项 | `text`（不是 `content`） | UI 看到的是 text 字段 |
| `last-reply` 返回 | `text`（不是 `content`） | 同上 |
| 磁盘 `messages.jsonl` | `content.text`（嵌套对象） | 持久化用嵌套 |
| `list-sessions` | `active / archived / title`（不是 `isActive / isArchived / name`） | 实现以 store 为准 |
| `where` | `sessionId` 在 `new-task` 后**可能是上次的值** | lazy 创建，要 `send` 后才更新 |
| `where` | `messageCount` 在 `new-task` 后可能是上次会话残值 | 同上，加 `--wait-fresh` 修复 |
| `archive-session` | 后端 IPC 写盘成功后，前端 `chatStore.conversations` **不会自动刷新** | 要么 sleep，要么用 `--wait`（直接查后端 IPC，不查 store） |
| `ui-message` 默认 | 过滤空气泡 | cancel 残影；用 `--include-empty` 取消过滤 |
| `screenshot` | `--label` 不是 `--name` | 设计稿历史错误 |

---

## 不做的命令（明确排除）

| 不做 | 原因 |
|---|---|
| `aijia chat <message>` | 业务流程多变，违反"CLI 只给原子能力"原则，用 `&&` 串 |
| `aijia continue-chat` / `send-and-cancel` | 同上 |
| `aijia login` | 登录态用持久化的，不在每次测试里跑 |
| `aijia upload-file` / `paste-image` | 拖拽/粘贴依赖 Tauri 原生事件，tauri-pilot 控不了 |
| `aijia change-theme` | 用户明确说不测换肤 |
| `aijia hire-employee` | 等真有 employee 测试场景再加 |

---

## 命名约定

- **动词在前**：`new-task` / `type-message` / `switch-session`
- **kebab-case**：跟 tauri-pilot 原命令一致
- **避免缩写**：`type-message` 不是 `tm`
- **字段以实现为准**：实现用 `text` 就别再写 `content`
