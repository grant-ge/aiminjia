# AIjia E2E 测试方案（敲定版）

> 创建：2026-05-16
> 状态：方案已敲定，进入实施
> 当前进度：阶段 1 PoC 已跑通（手工 12 步），准备进入阶段 2 固化

---

## 一句话总结

**用 tauri-pilot（fork 维护、本地源码 + 云效托管）做 AIjia 的端到端 UI 测试，加 `aijia` 子命令组沉淀业务原子命令，让 LLM 一行命令跑回归。**

---

## 关键决策（讨论过程中敲定的）

### 决策 0：铁则 — e2e 脚本只走 `aijia` 子命令组（2026-05-17 敲定）

**所有操作 aijia 项目的 e2e 脚本入口，必须经过 `tauri-pilot aijia <subcommand>` 子命令组。禁止直接使用通用命令（click / fill / eval / snapshot / screenshot 等）操作 aijia。**

通用命令仍然是底层原语，但**只能在 `aijia` 子命令的 Rust 实现内部使用**——对脚本作者完全隐藏。

理由：
1. 封装性 / 稳定性 / 可读性 / Token 效率 / 测试代码可移植性
2. 前端 DOM 改动只需要修一处 `aijia` 子命令实现，不必动 100 个 e2e 脚本
3. 详见 `docs/e2e-org1-chat-mainline.md` 顶部铁则段

执行：CI 应加 lint 拒绝 `tests/e2e/*.sh` 里 grep 到通用命令。

---

### 决策 1：工具选型 = tauri-pilot

| 候选 | 结论 | 理由 |
|---|---|---|
| WebdriverIO + tauri-driver | ❌ macOS 死路 | 官方明文不支持，safaridriver 控不了 WKWebView |
| TestDriver.ai | ❌ 商业 + 付费 + 上传录像 | 不符合"完全自控"需求 |
| 自研 test bridge | ❌ 性价比低 | bridge.js 1214 行护城河，自研要 80-120h |
| Rust 集成测试 + 真 LLM | ❌ 跳过 UI | 不符合"替代手工点"需求 |
| osascript / a11y API | ❌ 控不动 WKWebView DOM | macOS a11y 树到 webview 是扁平 AXGroup |
| **tauri-pilot** | ✅ **选定** | macOS 唯一可行（v0.5.2 刚支持）、MIT、Rust CLI、可改 |

### 决策 2：测试范围 = 主功能

只测 webview 内的业务行为：
- 新建会话 / 切换会话
- 发消息 / 收流式响应
- 工具调用展示
- skill / employee 派活
- 设置面板核心配置

**不测**：换肤、标题栏拖拽、剪贴板粘贴、拖拽上传等原生壳行为（tauri-pilot 本来也测不了，需求范围恰好对得上）。

### 决策 3：部署形式 = 本地路径 + 云效托管

不走 git 公开依赖、不走 vendor 进仓库。

**当前形态**：
```toml
# src-tauri/Cargo.toml
tauri-plugin-pilot = { path = "/Users/a20250311/github/tauri-pilot/crates/tauri-plugin-pilot" }
```

**长期形态**（推到云效后）：
```toml
# 默认走云效（CI / 同事友好）
tauri-plugin-pilot = { git = "git@codeup.aliyun.com:renlijia/lotus/tauri-pilot.git", tag = "v0.5.2" }

# 本地 override（gitignore 掉 .cargo/config.toml，只对个人机器生效）
[patch."git@codeup.aliyun.com:renlijia/lotus/tauri-pilot.git"]
tauri-plugin-pilot = { path = "/Users/a20250311/github/tauri-pilot/crates/tauri-plugin-pilot" }
```

### 决策 4：release 包零影响

**双重隔离**：
1. `lib.rs` 的 plugin 注册用 `#[cfg(debug_assertions)]` 包住
2. tauri-pilot plugin 自己也 `cfg(not(debug_assertions))` 走 no-op

**结论**：`pnpm tauri:build` 生产包**绝不包含 tauri-pilot 任何代码**，不影响签名 / 公证 / 自动更新。

### 决策 5：不改名 tauri-pilot

**保留原名**，理由（按重要性）：
1. 改名工作量大（1-2 天纯机械替换），零业务价值
2. 上游 v0.5.2 是 2026-05-14 发的、作者活跃，**保留挑选合并上游 fix 的能力**
3. 没有混淆风险（tauri-pilot 不在 crates.io，谁装都是看 git URL）
4. 改名解决的只是"心理感觉"，不解决技术问题

**用子命令前缀 `aijia` 区分通用 vs 业务**：
```bash
# 通用 DOM 命令（保留 30+ 个原命令不变）
tauri-pilot click @e5
tauri-pilot snapshot

# AIjia 业务命令（新加）
tauri-pilot aijia new-task
tauri-pilot aijia chat "你好"
```

### 决策 6：tauri-pilot 当 AIjia 专属工具维护

- **不**计划提 PR 回上游
- **不**走通用工具路线
- 改造心态：当成 aijia 项目的一部分
- 只在我们觉得有价值时挑着合上游 fix

### 决策 7：固化策略 = 业务命令直接进 tauri-pilot CLI

不写单独 wrapper 项目（避免多一层维护）。

业务命令分两层：

**Layer A：业务原子命令**（6 个）
```bash
tauri-pilot aijia new-task              # 点侧栏"新任务"
tauri-pilot aijia type-message "你好"   # Tiptap focus + execCommand insertText
tauri-pilot aijia send                  # 点发送按钮
tauri-pilot aijia wait-reply [--timeout 30s]  # 阻塞到流式结束
tauri-pilot aijia last-reply            # 取最后一条 assistant 消息
tauri-pilot aijia list-sessions         # 列侧栏会话名
```

**Layer B：场景级组合命令**（1 个）
```bash
tauri-pilot aijia chat "你好"
# 内部 = new-task + type-message + send + wait-reply + last-reply
# 一行完成完整对话回合
```

---

## 已完成的事

### 系统层

- ✅ Rust 1.95.0 工具链已装（`~/.rustup/toolchains/1.95.0-aarch64-apple-darwin`）
- ✅ tauri-pilot CLI 已装（`~/.cargo/bin/tauri-pilot`，v0.5.2）
- ✅ jq 安装尝试中（aijia bundle 脚本依赖）
- ✅ tauri-pilot 完整源码本地（`/Users/a20250311/github/tauri-pilot/`）
- ✅ lotus-app pnpm install 完成

### 仓库改动（`try/tauri-pilot-poc` 分支，未 commit）

| 文件 | 改动 |
|---|---|
| `rust-toolchain.toml`（新） | pin rust 1.95.0 |
| `src-tauri/Cargo.toml` | +3 行 `tauri-plugin-pilot = { path = "..." }` |
| `src-tauri/Cargo.lock` | 自动升 `tauri-plugin` 子依赖到 2.6.1 |
| `src-tauri/capabilities/default.json` | +1 行 `"pilot:default"` |
| `src-tauri/src/lib.rs` | +8 行 `cfg(debug_assertions)` 注册 plugin |
| managed runtime cache | dev 模式按需从 OSS manifest 下载，不依赖安装包内置 runtime 资源 |

### 验证结果

完整 e2e 链路跑通：

| 步骤 | 命令 | 结果 |
|---|---|---|
| 1 | `tauri-pilot ping` | ✓ ok |
| 2 | `tauri-pilot windows` | main http://127.0.0.1:5173/ |
| 3 | `tauri-pilot screenshot ...` | 175KB 真实截图 |
| 4 | `tauri-pilot snapshot --save ...` | a11y tree 2713 行 |
| 5 | `tauri-pilot click @e5` (新任务) | 进入新任务页 |
| 6 | `tauri-pilot eval` + execCommand | Tiptap 输入成功 |
| 7 | `tauri-pilot click @e414` (发送) | 真后端 + 真 LLM 真回复 |

证据截图：`/tmp/aijia-poc-{1..5}-*.png`

### 关键技术发现

**Tiptap 富文本编辑器适配点**：
- ❌ `tauri-pilot fill` 不工作（Tiptap 不监听原生 input 事件）
- ❌ `tauri-pilot type` 不工作（同上）
- ❌ `dispatchEvent(new InputEvent('beforeinput', ...))` 不工作
- ✅ `document.execCommand('insertText', false, text)` 工作

**结论**：必须封装成 `tauri-pilot aijia type-message` 命令，内部用 execCommand。

---

## 实施路径

### Step 1：tauri-pilot CLI 加业务命令（约 4-6h）

**位置**：`/Users/a20250311/github/tauri-pilot/crates/tauri-pilot-cli/`

**改动**：

1. **`src/cli.rs`**：新增 `Aijia` enum + 6 个 subcommand
   ```rust
   #[derive(Subcommand)]
   pub enum AijiaCommand {
       NewTask,
       TypeMessage { text: String },
       Send,
       WaitReply { #[arg(long, default_value = "30")] timeout: u64 },
       LastReply,
       ListSessions,
       Chat { message: String },
   }
   ```

2. **`src/main.rs`**：在主 dispatcher 加 `Commands::Aijia(cmd) => run_aijia_command(client, cmd)`

3. **新建 `src/aijia.rs`**：业务命令实现
   - `cmd_new_task`：snapshot → 找 button name="新任务" → click
   - `cmd_type_message`：eval execCommand
   - `cmd_send`：snapshot → 找 button name="发送" → click
   - `cmd_wait_reply`：watch `.streaming-done` 或轮询 textContent 稳定
   - `cmd_last_reply`：eval `[...document.querySelectorAll('.message-assistant')].pop()?.textContent`
   - `cmd_chat`：组合上面所有

4. **`Cargo.toml`**：业务命令需要的辅助依赖（应该都已经有了）

5. **`cargo install --path /Users/a20250311/github/tauri-pilot/crates/tauri-pilot-cli --force`**：装新版

### Step 2：验证新命令（30min）

```bash
# 用新命令一行完成 PoC（替代之前 12 步）
tauri-pilot aijia chat "你好"
```

期望输出：JSON 含 `{reply: "...", session_id: "..."}` 或类似。

### Step 3：推到云效（30min）

```bash
cd /Users/a20250311/github/tauri-pilot
# 加云效 remote
git remote add codeup git@codeup.aliyun.com:renlijia/lotus/tauri-pilot.git
git push codeup master --tags

# lotus-app 改 Cargo.toml 走云效 + 本地 patch
```

### Step 4：写第一个意图测试 e2e（持续）

挑一个简单的 rules.md（如 `session-runtime`），按"产品视角断言"翻译成 `tauri-pilot aijia chat ...` 命令序列。

---

## 风险与对策

| 风险 | 等级 | 对策 |
|---|---|---|
| tauri-pilot pre-1.0 breaking | 中 | 锁本地源码 commit，自主控制升级 |
| 上游作者跑路 | 低 | 源码已本地 + 云效托管 |
| Tiptap 版本升级破坏 execCommand | 中 | 业务命令封装隔离，改一处即可 |
| `wait-reply` 判定流式结束不准 | 中 | 多策略：DOM 选择器 + textContent 稳定 + 事件监听 |
| rust 1.95 影响其他构建 | 低 | rust-toolchain.toml 只 pin 本仓库 |
| release 包带 e2e 代码 | **无** | cfg(debug_assertions) + plugin no-op 双保险 |

---

## 关键文件位置

| 文件 | 用途 |
|---|---|
| `/Users/a20250311/.codex/worktrees/d633/lotus-app/docs/e2e-testing-decisions.md` | **本文件**（最终决策） |
| `/Users/a20250311/.codex/worktrees/d633/lotus-app/docs/e2e-testing-plan.md` | 初版方案（已被本文件取代，保留参考） |
| `/Users/a20250311/.codex/worktrees/d633/lotus-app/E2E_POC_REPORT.md` | PoC 跑通报告 |
| `/Users/a20250311/github/tauri-pilot/` | tauri-pilot 源码（本地维护） |
| `/Users/a20250311/github/tauri-pilot/crates/tauri-pilot-cli/src/` | CLI 源码（业务命令加在这里） |
| `~/.cargo/bin/tauri-pilot` | CLI binary（重装后更新） |
| `/tmp/aijia-poc-*.png` | PoC 验证截图 |
| `docs/test-intents/spec/tasks/*/rules.md` | 21 个 feature 的产品视角断言（e2e 用例来源） |

---

## 反对意见 / 重新讨论的触发条件

如果以下任一情况发生，**回头重新评估方案**：

1. tauri-pilot 在 macOS 上的 socket 通信不稳定（v0.5.2 才支持 macOS，存在风险）
2. Tiptap 升级 / 换富文本编辑器导致 execCommand 路径死掉
3. 业务命令封装不下来（发现每个 rules.md 都需要独特操作，无法复用）
4. 上游 react 19 / Tauri 2.x 上游有 breaking，跟我们 fork 偏差大
5. 团队成员 / CI 跑不起来（rust 1.95 / 云效访问问题）

**当前认为以上风险都可控**，按方案推进。
