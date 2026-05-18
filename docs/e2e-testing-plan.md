# E2E 测试方案与路线图

> 创建日期：2026-05-16
> 当前状态：方案确认，待执行 PoC

## 一句话方案

**用 tauri-pilot 做 E2E UI 自动化**，源码本地化（脱钩外部仓库），按 PoC → 云效托管 → 集成 rules.md → MCP 自动化 四步走。

---

## 核心决策

| 决策点 | 选择 | 理由 |
|---|---|---|
| 工具选型 | tauri-pilot（v0.5.2，MIT） | macOS 唯一可行；CLI 形态匹配需求；MIT 可改 |
| 部署形式 | 本地路径依赖 → 云效托管 | 摆脱 GitHub 外部依赖、国内拉得快、组织自主 |
| 启用方式 | `cfg(debug_assertions)` 双重隔离 | release 包零代码、零开销 |
| 测试范围 | 主功能（不测换肤/拖拽/标题栏等原生层） | webview 内 DOM 行为 100% 覆盖范围 |
| 测试驱动 | rules.md 自然语言断言 + LLM 翻译 | 复用已有 21 个 feature 的 rules.md |

---

## 阶段路线图

### 阶段 0：当前状态（已完成）

- ✅ rust 1.95.0 工具链已装（`~/.rustup/toolchains/1.95.0-aarch64-apple-darwin`）
- ✅ tauri-pilot CLI 已装（`~/.cargo/bin/tauri-pilot`，v0.5.2）
- ✅ tauri-pilot 源码本地（`/Users/a20250311/github/tauri-pilot/`，untouched）
- ✅ 集成代码已写入分支 `try/tauri-pilot-poc`（未 commit）：
  - `rust-toolchain.toml`（新建，pin 1.95.0）
  - `src-tauri/Cargo.toml`（+4 行 tauri-pilot 依赖）
  - `src-tauri/src/lib.rs`（+8 行 cfg(debug_assertions) 注册）
  - `src-tauri/capabilities/default.json`（+1 行 `pilot:default`）
  - `src-tauri/Cargo.lock`（自动更新 tauri-plugin 子依赖到 2.6.1）

### 阶段 1：本地路径 PoC 跑通（30 分钟，下一步）

**目标**：在 lotus-app dev 模式下，能用 `tauri-pilot` CLI 控制真 AIjia app 截图、点按钮。

**改动**：把 `Cargo.toml` 里的 git 依赖改成本地路径

```toml
# src-tauri/Cargo.toml
tauri-plugin-pilot = { path = "/Users/a20250311/github/tauri-pilot/crates/tauri-plugin-pilot" }
```

**步骤**：

```bash
cd /Users/a20250311/.codex/worktrees/d633/lotus-app
# 0. 准备 bundled runtime（aijia 自己的构建依赖，~85MB 下载）
bash scripts/prepare-bundled-runtime.sh

# 1. 改 Cargo.toml 为本地路径（见上）

# 2. 启动 dev
pnpm tauri:dev

# 3. 另开终端验证
tauri-pilot ping                       # 应该返回 OK
tauri-pilot windows                    # 列出 AIjia 窗口
tauri-pilot state                      # url / title / ready
tauri-pilot screenshot poc.png         # 截图
tauri-pilot snapshot -i > snap.json    # a11y tree
tauri-pilot eval "document.title"      # 求值

# 4. 验证业务流
tauri-pilot click '...'                # 点新建会话按钮
tauri-pilot fill 'textarea' '你好'      # 填消息
tauri-pilot press Enter                # 发送
tauri-pilot wait '.streaming-done'     # 等流式结束
tauri-pilot text '...'                 # 取消息内容
```

**通过标准**：以上任意 3 个命令返回非空结果且 app 状态变化符合预期。

### 阶段 2：本地源码二次开发（按需）

**改造方向**（按优先级）：

| 优先级 | 改造点 | 工时 |
|---|---|---|
| P0 | 加 `wait-streaming-done` 命令（aijia 特定） | 2h |
| P1 | 加 `get-current-session-id` 等业务级查询 | 2h |
| P1 | 改 socket 命名空间避免和其他 Tauri app 冲突 | 1h |
| P2 | 砍 Windows 实现（如果只测 macOS） | 4h |
| P2 | 砍 press / recorder 等不需要的命令 | 2h |
| P3 | 修上游 bug（React 19 / Tiptap 3 兼容性，发现再改） | 视情况 |

**位置**：在 `/Users/a20250311/github/tauri-pilot/` 直接改，**lotus-app 通过 path 依赖自动用上新代码**。

### 阶段 3：云效托管（脱钩外部依赖）

**目标**：把本地源码推到云效，lotus-app 切换到云效 URL。

**步骤**：

```bash
# 1. 本地 tauri-pilot 加云效 remote
cd /Users/a20250311/github/tauri-pilot
git remote add codeup git@codeup.aliyun.com:renlijia/lotus/tauri-pilot.git

# 2. 把代码 + tag 推过去
git push codeup master
git push codeup --tags

# 3. lotus-app 切换依赖到云效（用 [patch] 段保留本地 override）
# 见下文「Cargo.toml 最终形态」
```

**收益**：
- GitHub 不可达不影响构建
- 国内 CI / 同事拉源码快
- 组织自主控制版本
- 后续我们的改动有自己的 commit 历史

### 阶段 4：rules.md 自动化（长期目标）

**目标**：让 Claude 读 `docs/test-intents/spec/tasks/*/rules.md`，自动调 tauri-pilot 跑回归。

**两条路径**：

#### 路径 A：CLI 脚本驱动

写一个 `scripts/run-rules.sh <rules-path>`：

1. Claude / agent 读 rules.md（自然语言）
2. 翻译成 tauri-pilot 命令序列
3. 执行命令、抓 snapshot / screenshot
4. LLM 判定结果

#### 路径 B：MCP 直接驱动（推荐）

tauri-pilot 内置 MCP server（`mcp.rs` 1843 行）：

```bash
tauri-pilot mcp        # 启动 stdio MCP server
```

把 30+ subcommand 暴露为 MCP tools，Claude Code / Cursor 配置 MCP 后直接调用：

```jsonc
// ~/.claude.json 加 MCP server
{
  "mcpServers": {
    "tauri-pilot": {
      "command": "tauri-pilot",
      "args": ["mcp"]
    }
  }
}
```

之后让 Claude 跑测试：

```
用户: 跑一遍 agent-teams 的回归
Claude: [读 docs/test-intents/spec/tasks/agent-teams/rules.md]
        [调用 mcp__tauri-pilot__click / fill / snapshot ...]
        [按 rules.md 自然语言断言判定]
        [报告 pass / fail + 失败现场截图]
```

---

## Cargo.toml 最终形态（推荐）

```toml
# src-tauri/Cargo.toml

[dependencies]
# 默认走云效（CI / 同事友好）
tauri-plugin-pilot = { 
    git = "https://codeup.aliyun.com/renlijia/lotus/tauri-pilot.git", 
    tag = "v0.5.2" 
}

# 本地 override（开发时改源码用）
[patch."https://codeup.aliyun.com/renlijia/lotus/tauri-pilot.git"]
tauri-plugin-pilot = { 
    path = "/Users/a20250311/github/tauri-pilot/crates/tauri-plugin-pilot" 
}
```

或者把 `[patch]` 段单独放到 gitignored 的 `.cargo/config.toml`，只对你的机器生效。

---

## 风险与对策

| 风险 | 等级 | 对策 |
|---|---|---|
| tauri-pilot pre-1.0 breaking change | 中 | 锁 tag `v0.5.2`，自主控制升级时机 |
| 上游作者跑路 | 低 | 源码已本地 + 云效托管，零依赖外部 |
| rust 1.95 升级影响其他构建 | 低 | `rust-toolchain.toml` 只 pin 本仓库 |
| Cargo.lock 大量改动 | 中 | 已通过 `cargo update tauri-plugin --precise` 最小化影响 |
| release 包含 e2e 代码 | **无** | `cfg(debug_assertions)` 已保证 |
| CI macOS 跑不通 | 低 | tauri-pilot CI 在 macos-latest 持续绿（v0.5.2） |
| aijia 自己的 `resources/runtime` 缺失 | 中 | 跑 `prepare-bundled-runtime.sh` 即解决 |

---

## 不会发生的影响（release 安全）

✅ release 包零增量代码：`#[cfg(debug_assertions)]` 屏蔽 plugin 注册
✅ release 包零代码：tauri-pilot 自身在 `cfg(not(debug_assertions))` 是 no-op
✅ 不影响签名 / 公证 / 上传 OSS / Tauri updater
✅ 不影响 bundled runtime（Node / Python / uv）
✅ 不修改 main 分支（所有改动在 `try/tauri-pilot-poc`）

---

## 当前任务清单（按顺序）

- [ ] 跑 `bash scripts/prepare-bundled-runtime.sh` 准备 aijia 内置运行时
- [ ] `Cargo.toml` 改为本地路径依赖（脱钩 git）
- [ ] `cargo check` / `pnpm tauri:dev` 验证编译
- [ ] `tauri-pilot ping` + `screenshot` 验证全链路
- [ ] 写一个简单 e2e 脚本走 "新建会话 → 发消息 → 截图" 完整流程
- [ ] 决策是否进 P0 改造（加 aijia 特定命令）
- [ ] 推 tauri-pilot 到云效
- [ ] 改 Cargo.toml 为 `git = 云效URL` + 本地 `[patch]` override
- [ ] 配 MCP server 让 Claude 自动跑回归
- [ ] 选一个 rules.md 跑一遍验证 MCP 流程

---

## 关键文件位置

- 本方案：`/Users/a20250311/.codex/worktrees/d633/lotus-app/docs/e2e-testing-plan.md`
- PoC 改动：`try/tauri-pilot-poc` 分支
- PoC 报告：`/Users/a20250311/.codex/worktrees/d633/lotus-app/E2E_POC_REPORT.md`
- tauri-pilot 源码：`/Users/a20250311/github/tauri-pilot/`
- tauri-pilot CLI：`~/.cargo/bin/tauri-pilot`
- rules.md 集合：`docs/test-intents/spec/tasks/*/rules.md`（21 个 feature）
