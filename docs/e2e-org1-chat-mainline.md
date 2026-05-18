# AIjia E2E 测试 CLI 工具规格

> 创建：2026-05-16
> 修订：2026-05-17（剥离场景细节，CLI 工具与意图测试责任分离）
> 状态：CLI 命令清单已敲定，等实施

---

## 这份文档讲什么

**只讲 CLI 工具本身——能做什么、命令长什么样**。

**不讲**：
- 哪个场景要测、测什么、断言什么
- prompt 怎么写、fixture 怎么选
- PR gate 跑不跑、用真 LLM 还是 mock

那些**归意图测试 / 测试用例作者管**。CLI 工具的责任仅限于"提供操作 aijia 的稳定原子能力"。

---

## 🔒 铁则（不可违反）

**所有操作 aijia 项目的入口必须经过 `tauri-pilot aijia <subcommand>` 子命令组。**

### 禁止（在 e2e 脚本里直接调用通用命令）

```bash
❌ tauri-pilot click @e5
❌ tauri-pilot fill '...' '...'
❌ tauri-pilot eval "..."
❌ tauri-pilot snapshot
❌ tauri-pilot screenshot file.png
```

### 必须（所有 aijia 业务行为走子命令）

```bash
✅ tauri-pilot aijia new-task
✅ tauri-pilot aijia type-message "..."
✅ tauri-pilot aijia send
✅ tauri-pilot aijia wait-reply
✅ tauri-pilot aijia ui-message
```

### 为什么

1. **封装性**：脚本不需要知道按钮 selector。前端改 selector，100 个脚本不动
2. **稳定性**：所有"找元素"的逻辑集中在 `aijia` 子命令的实现里
3. **可读性**：业务语义命令一眼看懂
4. **测试代码可移植**：换底层自动化协议（未来可能从 tauri-pilot 换成别的），上层脚本零修改

### 例外（仅限 tauri-pilot CLI 内部）

通用命令（click / eval / snapshot 等）**只能在 `aijia` 子命令的 Rust 实现内部使用**。e2e 脚本作者看不到、用不到。

```
e2e 脚本 / 意图测试
  ↓ 只调用
tauri-pilot aijia <subcommand>
  ↓ 内部实现（脚本作者看不到这一层及以下）
tauri-pilot click / fill / eval / snapshot
  ↓
tauri-plugin-pilot bridge.js
  ↓
AIjia webview DOM
```

**违反铁则的判定**：e2e 脚本里 grep 到 `tauri-pilot click|eval|fill|snapshot|screenshot` 任一 = 违反。

---

## 用法模式

两种用法，**不要**打包业务流程命令：

### 用法 1：原子命令独立调用

```bash
aijia new-task
aijia type-message "你好"
aijia send
```

### 用法 2：用 `&&` 串成流程（编排留给调用者）

```bash
aijia new-task && \
aijia type-message "你好" && \
aijia send && \
aijia wait-reply

reply=$(aijia last-reply | jq -r '.content')
```

**设计原则**（Unix 哲学）：CLI 只提供原子能力，编排留给调用者。**不打包 `aijia chat` 这类业务流程命令**——业务流程会变，CLI 不应该跟着变。

---

## CLI 命令清单

### Layer A：原子命令（13 个）

| 命令 | 用途 |
|---|---|
| `aijia new-task` | 点侧栏"新任务"按钮（仅路由，不创会话） |
| `aijia type-message <text>` | Tiptap 输入框填文本（execCommand insertText） |
| `aijia send` | 点发送按钮 |
| `aijia wait-reply [--timeout 30]` | 等流式结束（读 store / 按钮翻转 / textContent 稳定 三策略 fallback） |
| `aijia cancel` | 流式中点停止按钮 |
| `aijia ui-message [--last N\|--role X\|--since T]` | **抓 UI 上所有可见消息**（按显示顺序，从 DOM 读，不读 jsonl） |
| `aijia last-reply [--format json]` | 取最后一条 AI 回复（= `ui-message --last 1 --role assistant` 便捷别名） |
| `aijia list-sessions` | 列侧栏会话 |
| `aijia switch-session <id\|index>` | 切会话 |
| `aijia archive-session <id\|index>` | 归档（hover row → … → 归档 → 确认弹窗） |
| `aijia select-workspace <name>` | 切工作目录 |
| `aijia restart-app` | 重启 app + 等 ready |
| `aijia where` | 报告当前状态（不含消息内容，看消息用 `ui-message`） |

#### `aijia ui-message` 详解（核心命令）

**含义**：把对话窗口里**用户肉眼看到的所有消息**按显示顺序读出来。**只从 DOM 抓**（不读后台 jsonl、不分析截图）。

**输出**（JSON 数组）：

```json
[
  { "index": 0, "role": "user", "text": "你好" },
  { "index": 1, "role": "assistant", "text": "你好 pzcaaaa！收到..." },
  { "index": 2, "role": "tool_call", "name": "read_file",
    "params": { "path": "README.md" },
    "result": "# AIjia ..." },
  { "index": 3, "role": "assistant", "text": "根据 README..." }
]
```

**选项**：

| 参数 | 用途 |
|---|---|
| `--last N` | 只取最后 N 条 |
| `--role user\|assistant\|tool_call` | 按角色过滤 |
| `--since 2m` | 最近 N 分钟（基于 DOM 渲染时间戳） |
| `--include-tools` | 是否包含 tool_call（默认 true） |

#### `aijia where` 返回结构

```json
{
  "url": "http://127.0.0.1:5173/",
  "route": "/",
  "title": "AI小家",
  "sessionId": "abc123",
  "sessionName": "新对话",
  "workspace": "lotus-app",
  "isStreaming": false,
  "isSending": false,
  "hasEditor": true,
  "hasToolCallBlock": false,
  "messageCount": 3,
  "model": "claude-sonnet-4-6",
  "lastError": null
}
```

业务级 UI 状态查询全集中在这一个命令，**避免脚本各自 eval querySelector**。如需新查询字段，加到 `where` 返回里。

### Layer B：诊断（1 个）

| 命令 | 用途 |
|---|---|
| `aijia screenshot --label <name> [--selector <css>]` | 截图到 `/tmp/aijia-e2e-{label}-{ts}.png`。默认 selector 是 `[data-aijia-message-list]`（聚焦聊天主区，速度也最快）。底层走 `tauri-pilot screenshot` RPC，bridge 已默认 `skipFonts:true`，在 lotus-app 这种多 @font-face 的项目上从 30s timeout 缩到 ~100ms。需要嵌入字体的精确还原可传 `--selector` 显式控制范围。 |

### Layer C：health-check + teardown（2 个）

| 命令 | 用途 |
|---|---|
| `aijia health-check` | 确认 app 起来、有默认模型、IPC 通——任一失败 abort 整批 |
| `aijia cleanup-test-sessions [--prefix <str>]` | 归档所有 title 以 `--prefix`（默认 `e2e-test-`）开头的会话。**注意**：只匹配 conversation `title` 字段——发消息内容以 prefix 开头不会自动改 title，测试作者需先通过 UI 重命名（⋯ → 重命名聊天）才能让 cleanup 命中。 |

**合计 16 个命令。**

---

## 前端配套改动（实施前置）

CLI 子命令实现需要前端提供两个稳定钩子：

### ❶ 前端补 5 处 `data-aijia-*` 钩子（约 10 行 diff）

**命名约定**：用 `data-aijia-*` 业务前缀（跟项目已有 `data-tauri-drag-region` 风格一致），**不用** `data-testid`。

**注意**：这些钩子**不暴露给 e2e 脚本**（铁则禁止脚本直接用 selector）。它们是 `aijia` 子命令**内部实现**用的稳定钩子。

| 文件 | 改动 | aijia 子命令哪里用 |
|---|---|---|
| `src/components/sidebar/ConversationRow.tsx:64` | 根 `<button>` 加 `data-aijia-conversation-row data-aijia-conversation-id={id}` | `list-sessions` / `switch-session` / `archive-session` |
| `src/components/chat/MessageList.tsx:150` | 外层加 `data-aijia-message-list data-aijia-streaming={...}` | `wait-reply` / `where` |
| `src/components/chat/AiBubble.tsx` | 根加 `data-aijia-ai-bubble data-aijia-message-id={message.id}` | `last-reply` / `ui-message` |
| `src/components/chat/StreamingBubble.tsx:42` | 根加 `data-aijia-streaming-bubble` | `cancel` 后兜底 |
| `src/components/common/ConfirmDialog.tsx:41` | `AlertDialogContent` 加 `data-aijia-confirm-dialog`，actions 加 `data-aijia-confirm-action="confirm\|cancel"` | `archive-session` |

**配套**：写 `docs/data-aijia-conventions.md` 命名约定文档。

**生产影响**：DOM 多 ~10 个属性、~几 KB 体��、零运行时开销。

**决策**：✅ 已批准（2026-05-17）

### ❷ Dev 模式 expose Zustand store（1 行）

```ts
// src/main.tsx 或 App.tsx 顶部
if (import.meta.env.DEV) {
  (window as any).__aijia = {
    chatStore: useChatStore,
    sessionStore: useSessionStore,
  }
}
```

`aijia` 子命令内部通过 `pilot.eval("window.__aijia.chatStore.getState()")` 读 Zustand 真实状态，让 `wait-reply` / `where` 等命令**毫秒级、零误差**。

**生产影响**：`import.meta.env.DEV` 在 release build 为 false，Vite 编译时整段剥离，**release 包零代码**。

**决策**：✅ 已批准方案 A（2026-05-17）—— 直接 expose `window.__aijia.chatStore`，1 行代码，仅 dev build 生效

---

## 附件类延后

**坑**：CLAUDE.md 明文："Tauri 2 webview 拦截 HTML5 drop 事件，React `onDrop` 永不触发。"

意味着 tauri-pilot 在 webview 里 `dispatchEvent` 控不了拖拽、剪贴板。**CLI 暂不提供** `aijia attach-file` 等命令。

未来思路：绕过前端 drop，走后端 IPC 直接注入附件。**核心 13 个原子命令稳定后再做**。

---

## 实施清单

### Round 0：前置改动

- [x] ❶ 前端补 5 处 `data-aijia-*` 钩子（已批准）
- [x] ❷ Dev 模式 expose store 方案 A（已批准）
- [ ] 写 `docs/data-aijia-conventions.md` 命名约定文档

### Round 1：tauri-pilot CLI 实现 16 个命令

实施位置：`/Users/a20250311/github/tauri-pilot/crates/tauri-pilot-cli/src/aijia.rs`（新建）

实施顺序：

1. `health-check` + `where`（基础设施）
2. `new-task` + `type-message` + `send` + `last-reply`
3. `wait-reply`（优先读 store，回退按钮翻转，兜底 textContent 稳定）
4. `ui-message`（核心查询命令）
5. `list-sessions` + `switch-session`
6. `select-workspace`
7. `cancel` + `archive-session`
8. `restart-app` + `cleanup-test-sessions`
9. `screenshot`

每个命令完成后**重装 CLI 验证一次**（不要写完 16 个再一起测）。

### Round 2：交给意图测试 / 测试用例

CLI 工具准备好后，**写测试是意图测试 / 测试用例作者的事**：

- 场景列表
- prompt 写法
- 断言粒度
- fixture 选择
- 真 LLM 还是 mock
- 怎么 retry / 处理 flaky

CLI 工具不参与这些决策。

---

## 风险与对策

| 风险 | 等级 | 对策 |
|---|---|---|
| Tiptap execCommand 在 React 19 失效 | 中 | PoC 已验证可用；备选 expose Tiptap editor 实例 |
| `wait-reply` 三策略都失败 | 低 | 已有 fallback 链 |
| 前端 store 重命名 | 低 | expose 处编译报错，1 行修复 |
| Tauri 升级破坏 tauri-pilot | 低 | 锁定 v0.5.2，本地维护，必要时 fork |

---

## 文档结构

| 文件 | 内容 |
|---|---|
| `docs/e2e-testing-decisions.md` | 选型决策（tauri-pilot / 不改名 / 云效托管 / 铁则） |
| **`docs/e2e-org1-chat-mainline.md`**（本文件） | **CLI 工具规格**（命令清单 + 前端配套） |
| `docs/data-aijia-conventions.md`（待写） | `data-aijia-*` 命名约定 |
| `E2E_POC_REPORT.md` | PoC 首次跑通报告 |
| `~/github/tauri-pilot/crates/tauri-pilot-cli/src/aijia.rs` | CLI 实现（待写） |
| **意图测试 rules.md** | 测试场景与断言（**本文件不涉及**） |

---

## 责任分界

| 责任 | 归谁 |
|---|---|
| CLI 工具能做什么 / 命令长什么样 | 本文件 |
| 前端 `data-aijia-*` 钩子 | 本文件 + lotus-app 前端 |
| Dev 模式 expose store | 本文件 + lotus-app 前端 |
| 测什么场景 / 测什么观察点 / 怎么断言 | **意图测试 / 测试用例作者**（本文件不涉及） |
| prompt 写法 / fixture / mock 策略 | **意图测试 / 测试用例作者** |
| CI / PR gate / 跑频率 | **未来 DevOps**（本文件不涉及） |
