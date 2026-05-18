# AIjia E2E CLI 实施计划

> 创建：2026-05-17
> 状态：等待启动
> 上游文档：`docs/e2e-org1-chat-mainline.md`（CLI 工具规格）

---

## 实施位置

### 主仓库（写大部分代码）

```
/Users/a20250311/github/tauri-pilot/
├── crates/
│   ├── tauri-plugin-pilot/         ← 几乎不动
│   └── tauri-pilot-cli/            ← 主要改这里
│       ├── Cargo.toml              ← 可能加 1-2 个依赖
│       └── src/
│           ├── main.rs             ← 加 aijia subcommand 路由
│           ├── cli.rs              ← 加 aijia 子命令的 clap 定义
│           └── aijia/              ← 新建目录
│               ├── mod.rs          ← 入口 + dispatcher
│               ├── new_task.rs     ← 一个命令一个文件
│               ├── type_message.rs
│               ├── send.rs
│               ├── wait_reply.rs
│               ├── cancel.rs
│               ├── ui_message.rs
│               ├── last_reply.rs
│               ├── list_sessions.rs
│               ├── switch_session.rs
│               ├── archive_session.rs
│               ├── select_workspace.rs
│               ├── restart_app.rs
│               ├── where_state.rs  ← `where` 是 Rust 关键字，用 where_state
│               ├── screenshot.rs
│               ├── health_check.rs
│               └── cleanup.rs
```

### 附属仓库（小改动）

```
/Users/a20250311/.codex/worktrees/d633/lotus-app/   ← lotus-app 主项目
├── docs/
│   └── data-aijia-conventions.md   ← 新建，命名约定文档
├── src/
│   ├── main.tsx                    ← +1 行：expose store
│   └── components/
│       ├── sidebar/ConversationRow.tsx       ← +2 个 data-aijia-*
│       ├── chat/MessageList.tsx              ← +2 个 data-aijia-*
│       ├── chat/AiBubble.tsx                 ← +2 个 data-aijia-*
│       ├── chat/StreamingBubble.tsx          ← +1 个 data-aijia-*
│       └── common/ConfirmDialog.tsx          ← +2 个 data-aijia-*
```

---

## 完整实施步骤（按顺序）

### Phase 0：基础设施（30 分钟）

#### Step 0.1：写命名约定文档（5 分钟）

**位置**：`lotus-app/docs/data-aijia-conventions.md`

**内容**：
- `data-aijia-*` 是什么
- 命名规则（`data-aijia-{业务名}-{类型}`）
- 何时加 / 何时删
- 删除前必须先改 `tauri-pilot aijia/` 下对应命令

#### Step 0.2：lotus-app 前端打 5 处 `data-aijia-*` 钩子（15 分钟）

派 subagent 直接改这 5 个文件，每个文件加 1-2 行属性，不动业务逻辑。

子 agent 任务清单：
1. `ConversationRow.tsx`：根 `<button>` 加 `data-aijia-conversation-row data-aijia-conversation-id={id}`
2. `MessageList.tsx`：外层加 `data-aijia-message-list data-aijia-streaming={isStreaming?'true':'false'}`
3. `AiBubble.tsx`：根加 `data-aijia-ai-bubble data-aijia-message-id={message.id}`
4. `StreamingBubble.tsx`：根加 `data-aijia-streaming-bubble`
5. `ConfirmDialog.tsx`：`AlertDialogContent` 加 `data-aijia-confirm-dialog`，actions 加 `data-aijia-confirm-action="confirm|cancel"`

#### Step 0.3：lotus-app dev 模式 expose store（5 分钟）

在 `src/main.tsx` 顶部加：

```ts
if (import.meta.env.DEV) {
  (window as any).__aijia = {
    chatStore: useChatStore,
    sessionStore: useSessionStore,
  }
}
```

#### Step 0.4：验证前端改动（5 分钟）

```bash
pnpm tauri:dev
```

- TypeScript 编译通过
- app 启动正常
- devtools 看 `window.__aijia` 不是 undefined
- DOM 上能 grep 到 `data-aijia-*`

---

### Phase 1：CLI 基础设施（1 小时）

#### Step 1.1：在 `tauri-pilot-cli/src/cli.rs` 加 `aijia` subcommand 定义

```rust
#[derive(Subcommand)]
pub enum Commands {
    // ... 现有命令
    
    /// AIjia-specific business commands
    Aijia {
        #[command(subcommand)]
        command: AijiaCommand,
    },
}

#[derive(Subcommand)]
pub enum AijiaCommand {
    NewTask,
    TypeMessage { text: String },
    Send,
    WaitReply { #[arg(long, default_value = "30")] timeout: u64 },
    Cancel,
    UiMessage {
        #[arg(long)] last: Option<usize>,
        #[arg(long)] role: Option<String>,
        #[arg(long)] since: Option<String>,
        #[arg(long, default_value_t = true)] include_tools: bool,
    },
    LastReply { #[arg(long)] format: Option<String> },
    ListSessions,
    SwitchSession { id_or_index: String },
    ArchiveSession { id_or_index: String },
    SelectWorkspace { name: String },
    RestartApp,
    Where,
    Screenshot { #[arg(long)] label: String },
    HealthCheck,
    CleanupTestSessions,
}
```

#### Step 1.2：在 `main.rs` 加 dispatcher

```rust
match args.command {
    // ... 现有匹配
    Commands::Aijia { command } => aijia::dispatch(client, command).await?,
}
```

#### Step 1.3：新建 `aijia/mod.rs`，dispatcher 和共享类型

```rust
pub mod new_task;
pub mod type_message;
// ... 等等

pub async fn dispatch(client: &Client, cmd: AijiaCommand) -> Result<()> {
    match cmd {
        AijiaCommand::NewTask => new_task::run(client).await,
        AijiaCommand::TypeMessage { text } => type_message::run(client, &text).await,
        // ... 等等
    }
}
```

#### Step 1.4：编译通过（空实现也行）

每个命令文件先写 `todo!()`，能编译过即可。

---

### Phase 2：实施 16 个命令（5-7 小时）

按依赖关系顺序实施。每完成一个 **立即重装 CLI 测一下**：

```bash
cargo install --path /Users/a20250311/github/tauri-pilot/crates/tauri-pilot-cli --force
```

#### Round A：基础设施类（30 分钟）

1. **`aijia health-check`**：检查 pilot socket 通、app ready
   - 实现：调 `ping` + `state`，验证 readyState = complete

2. **`aijia where`**：返回当前状态
   - 实现：`pilot.eval` 读 `window.__aijia.chatStore.getState()`，再读 DOM 几个属性
   - 输出：JSON（结构见 spec）

**验证**：跑 `tauri-pilot aijia health-check` 返回 `{ok: true}`，`aijia where` 返回完整 JSON

#### Round B：发消息核心（1 小时）

3. **`aijia new-task`**：点新任务
   - 实现：`pilot.snapshot()` 找 a11y "新任务" button → click
   - 或者：用 `data-aijia-conversation-row` 不存在 → 走 a11y role+name

4. **`aijia type-message`**：Tiptap 输入
   - 实现：`pilot.eval("document.querySelector('.ProseMirror').focus(); document.execCommand('insertText', false, '<text>')")`
   - 输入 escape 处理

5. **`aijia send`**：点发送
   - 实现：找 `button[aria-label="发送"]` → click

6. **`aijia last-reply`**：取最后 AI 回复
   - 实现：`pilot.eval` querySelector `[data-aijia-ai-bubble]:last-child` → textContent

**验证**：手工跑 4 步组合
```bash
aijia new-task && aijia type-message "你好" && aijia send && sleep 5 && aijia last-reply
```
能看到 AI 真实回复。

#### Round C：等流式 + 取消息（1.5 小时）

7. **`aijia wait-reply`**（**最难，三策略 fallback**）
   - 策略 1：`pilot.eval` 读 `window.__aijia.chatStore.getState().streamStates[id].isStreaming` 轮询
   - 策略 2：`pilot.eval` 看是否有 `button[aria-label="停止"]`
   - 策略 3：textContent 5 秒不变兜底
   - 超时返回 timeout

8. **`aijia ui-message`**（**核心查询**）
   - 实现：`pilot.eval` 遍历 `[data-aijia-message-list] > *` 取所有消息
   - 区分 user / assistant / tool_call（用 `data-aijia-*` 标签）
   - 支持 `--last N`、`--role X` 过滤
   - 输出：JSON 数组

**验证**：
```bash
aijia new-task && aijia type-message "你好" && aijia send && aijia wait-reply
aijia ui-message
# 应该看到 user + assistant 两条
```

#### Round D：会话管理（1.5 小时）

9. **`aijia list-sessions`**
   - 实现：`pilot.eval` 遍历 `[data-aijia-conversation-row]` 取 `data-aijia-conversation-id` 和文本

10. **`aijia switch-session`**
    - 实现：参数支持 id 或 index；找对应 `[data-aijia-conversation-id="..."]` → click

11. **`aijia archive-session`**
    - 实现：hover row（pilot 模拟）→ 点 `[data-aijia-conversation-row] [aria-label="聊天更多操作"]` → 点 "归档" 菜单项 → 确认弹窗 `[data-aijia-confirm-action="confirm"]` click

**验证**：创建会话、列、切换、归档全链路

#### Round E：剩余命令（1 小时）

12. **`aijia select-workspace`**：点工作目录选择器 → 选项
13. **`aijia cancel`**：找 `button[aria-label="停止"]` click
14. **`aijia restart-app`**：通过 Tauri command 重启 app + 等 ready
15. **`aijia screenshot`**：包装通用 screenshot + 加 label 前缀
16. **`aijia cleanup-test-sessions`**：循环 list-sessions → archive-session（名字含 "e2e-test-" 的）

**验证**：每个命令独立测一遍

---

### Phase 3：跑通完整 PoC 重放（30 分钟）

用之前手工跑的"新建会话 → 发消息 → 看回复"流程，**这次只用 aijia 子命令**：

```bash
aijia health-check && \
aijia new-task && \
aijia type-message "你好，这是 e2e 测试" && \
aijia send && \
aijia wait-reply && \
aijia ui-message --last 1 --role assistant
```

如果能一行跑通、输出真实 AI 回复 = Phase 1+2 全 OK。

---

### Phase 4：推到云效（30 分钟）

```bash
cd /Users/a20250311/github/tauri-pilot
git add -A
git commit -m "feat(aijia): add 16 aijia subcommands for AIjia e2e testing"
git remote add codeup git@codeup.aliyun.com:renlijia/lotus/tauri-pilot.git
git push codeup master --tags
```

然后在 lotus-app 的 `src-tauri/Cargo.toml` 把 path 依赖切换成 git 依赖：

```toml
# 默认走云效
tauri-plugin-pilot = { git = "git@codeup.aliyun.com:renlijia/lotus/tauri-pilot.git", tag = "v0.5.2-aijia" }

# 本地 override（gitignore 掉 .cargo/config.toml）
[patch."git@codeup.aliyun.com:renlijia/lotus/tauri-pilot.git"]
tauri-plugin-pilot = { path = "/Users/a20250311/github/tauri-pilot/crates/tauri-plugin-pilot" }
```

---

## 总时间预估

| Phase | 内容 | 时间 |
|---|---|---|
| 0 | 前端钩子 + expose store + 命名约定 | **30 min** |
| 1 | CLI 基础设施（clap + dispatcher） | **1 hr** |
| 2 | 实施 16 个命令 | **5-7 hr** |
| 3 | 跑通完整 PoC | **30 min** |
| 4 | 推云效 + 切依赖 | **30 min** |
| **合计** | | **7.5-9.5 小时** |

实际可能更短（很多命令实现相似，写第一个最难、后面是抄）。

---

## 风险点与对策

| 风险 | 对策 |
|---|---|
| `data-aijia-*` 加到组件后某个组件渲染挂 | 一次只改一个文件，加完跑 `pnpm tauri:dev` 看 app 还能起 |
| `wait-reply` 三策略都不稳 | 先实现策略 1（读 store），不够再加策略 2/3 |
| 编译错误（rust 1.95 / edition 2024） | 用 `rust-toolchain.toml` pin 已配好 |
| 命令多互相影响 | 一个一个写、一个一个测，不要批量写完一起测 |
| Phase 2 写到一半发现命令设计有误 | 立刻停，回 spec 文档讨论，不要"先实现再说" |

---

## 不在本计划内的事

明确**不做**：

- ❌ 写具体测试场景（C-01/C-02/... 的 shell 脚本）—— 归意图测试
- ❌ mock LLM —— 决策已撤销
- ❌ PR gate CI workflow —— 决策已撤销
- ❌ scenario TOML —— 暂不做
- ❌ 接 MCP server —— 已有 tauri-pilot mcp，不需要重写
- ❌ 改 tauri-pilot 包名 —— 决策已敲定不改名
- ❌ 改 tauri-pilot 核心实现（plugin/server/eval）—— 只加 aijia 子命令
