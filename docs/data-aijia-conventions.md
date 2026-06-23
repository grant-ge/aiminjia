# `data-aijia-*` 命名约定

> 创建：2026-05-17
> 关联：`docs/e2e-org1-chat-mainline.md`（CLI 工具规格）/ `docs/e2e-cli-implementation-plan.md`（实施计划）

---

## 这是什么

`data-aijia-*` 是 lotus-app（AIjia 桌面端）前端组件上挂的一组 DOM 属性。**目的只有一个**：为 e2e 测试工具 `tauri-pilot aijia <subcommand>` 提供稳定的元素定位锚点。

它**不是** `data-testid`、**不是**业务功能、**不参与**任何运行时逻辑。Vite/React 把它当普通 HTML 属性透传到最终 DOM，仅此而已。

---

## 为什么不用 `data-testid`

1. **业务前缀清晰**：跟项目已有的 `data-tauri-drag-region` 风格一致，一眼看出是 aijia 项目专属
2. **避免污染**：未来如果接入通用测试框架（Vitest / Playwright），它们用 `data-testid` 跑组件测试，跟 aijia 端到端测试的钩子是两套关注点
3. **命名空间隔离**：grep `data-aijia-` 立刻找全所有 e2e 钩子；grep `data-testid` 会混进单元测试用的 ID

---

## 谁可以使用这些钩子

**只有 `tauri-pilot aijia` 子命令的 Rust 实现可以读 `data-aijia-*`。**

e2e 脚本作者**不许**直接写：

```bash
# ❌ 禁止
tauri-pilot eval "document.querySelector('[data-aijia-conversation-row]')"
tauri-pilot click '[data-aijia-dialog-action="confirm"]'
```

这违反 e2e 铁则（见 `docs/e2e-org1-chat-mainline.md` 顶部）。脚本作者只能调 `tauri-pilot aijia <subcommand>`，selector 是子命令内部实现细节。

---

## 命名规则

```
data-aijia-{业务名}[-{字段}]="{值}"
```

- 业务名：单数 kebab-case，描述这是什么 UI 元素（`conversation-row` / `message-list` / `ai-bubble` / `streaming-bubble` / `dialog`）
- 字段：可选，当需要额外信息时加（`conversation-id` / `message-id` / `streaming` / `dialog-action`）
- 值：稳定可读字符串，**不要**塞 React state（如 `data-aijia-hovered={hovered}`），那种瞬态信息应该走 DOM 树结构或 aria-* 表达

### 例子

| 属性 | 含义 |
|---|---|
| `data-aijia-conversation-row` | 标记侧栏一行会话 |
| `data-aijia-conversation-id="abc123"` | 该行会话的 ID |
| `data-aijia-message-list` | 标记消息列表外层容器 |
| `data-aijia-streaming="true"` | 当前在流式输出 |
| `data-aijia-ai-bubble` | 标记一条 AI 消息气泡 |
| `data-aijia-message-id="msg-xyz"` | 该气泡对应 message id |
| `data-aijia-streaming-bubble` | 标记流式输出中的临时气泡 |
| `data-aijia-dialog="permission-ask\|ask-user-question\|confirm"` | 标记一个等用户决策的弹窗，值是弹窗类型 |
| `data-aijia-dialog-tool="WebSearch"` | 触发该弹窗的工具名（仅 permission-ask / ask-user-question 有） |
| `data-aijia-dialog-title` / `-description` | 弹窗内的标题 / 描述节点 |
| `data-aijia-dialog-action="allow\|deny\|cancel\|confirm\|option"` | 弹窗内的 action 按钮 |
| `data-aijia-dialog-question-index` / `-option-index` / `-option-label` | 多 question 多 option 的 ask-user-question 用，定位具体 option |

---

## 何时加 / 何时删

### 加的时机

1. tauri-pilot CLI 新增 `aijia <subcommand>` 时，发现需要稳定锚点 → 加到对应组件上
2. 现有命令在 a11y tree / 文字匹配上不稳 → 升级为 `data-aijia-*` 锚点

### 删的时机

**只在确认 `tauri-pilot aijia/` 下没有任何子命令再用之后**才删。删除流程：

1. 在 `~/github/tauri-pilot/crates/tauri-pilot-cli/src/aijia/` 全仓 grep 该属性
2. 若有引用 → 先改 CLI 实现，PR 合到 tauri-pilot 仓库 → 重装 → 验证 e2e 跑通
3. 然后才能从 lotus-app 删属性

**禁止**先删属性再改 CLI，会导致 e2e 跑挂没人发现（CLI 失败信息可能只是 "element not found"，不会指向 lotus-app 的真实改动）。

---

## 生产影响

- DOM 上 ~10 个额外属性，gzipped HTML 增量 < 1KB
- 零运行时开销（不是 React state，不参与重渲）
- 不出现在前端代码语义层（grep `data-aijia-` 全在 JSX 属性里）

**release 包**：属性保留（webview 里实际渲染），但因为 tauri-pilot plugin 已经 `cfg(debug_assertions)` 在 release build 禁用，无任何监听器读取这些属性。可以理解为"沉睡的钩子"，没有副作用。

---

## 当前清单（v1）

| 文件 | 属性 |
|---|---|
| `src/components/sidebar/ConversationRow.tsx` | `data-aijia-conversation-row` / `data-aijia-conversation-id` |
| `src/components/chat/MessageList.tsx` | `data-aijia-message-list` / `data-aijia-streaming` |
| `src/components/chat/AiBubble.tsx` | `data-aijia-ai-bubble` / `data-aijia-message-id` |
| `src/components/chat/StreamingBubble.tsx` | `data-aijia-streaming-bubble` |
| `src/components/common/Modal.tsx` | `data-aijia-dialog` / `data-aijia-dialog-tool`（透传 props） |
| `src/components/common/PermissionAskDialog.tsx` | `data-aijia-dialog="permission-ask"` / `-tool` / `-title` / `-description` / `-action="allow\|deny"` |
| `src/components/common/ConfirmDialog.tsx` | `data-aijia-dialog="confirm"` / `-title` / `-description` / `-action="cancel\|confirm"` |

后续追加请在本表登记一条，并说明对应的 `aijia <subcommand>`。

---

## Dev 模式 store 暴露（配套）

`src/main.tsx` 的 `import.meta.env.DEV` 守卫里挂 `window.__aijia = { chatStore, sessionStore }`。

- 作用：CLI 通过 `pilot.eval("window.__aijia.chatStore.getState()")` 读 Zustand 真实状态，让 `wait-reply` / `where` 等命令毫秒级且零误差
- 生产影响：Vite 编译期 dead-code elimination，release 包零代码

属于 `data-aijia-*` 之外的另一个 e2e 测试钩子，命名空间统一在 `__aijia`，遵循同样的"只 CLI 内部用���不进 e2e 脚本"原则。
