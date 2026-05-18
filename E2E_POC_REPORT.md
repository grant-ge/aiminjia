# E2E 测试方案 PoC 报告（睡前调研结果）

**结论先行**：tauri-pilot 是当前最优方案，集成可行，但卡在 aijia 项目自己的 bundled runtime 资源缺失（与 tauri-pilot 无关）。

---

## 当前所在分支

```
worktree: /Users/a20250311/.codex/worktrees/d633/lotus-app
branch:   try/tauri-pilot-poc
状态:     有未提交改动，未 commit、未推送
```

## 已完成的事

### 1. 派 4 个 subagent 交叉验证

- 网络检索（3 个）：现状、5 个真实 Tauri 项目、非 WebDriver 方案
- 本地源码（3 个）：架构、plugin 实现、CLI 实现、macOS 跑通可行性

**核心结论一致**（独立验证）：

| 维度 | 结论 |
|---|---|
| WebdriverIO + tauri-driver | macOS 死路（官方明文不支持） |
| osascript / a11y API | 控不动 WKWebView DOM |
| tauri-pilot | **可行**，2026-05-14 v0.5.2 刚支持 macOS |
| TestDriver.ai | 商业，要 API key + 上传录像 |
| 自研 test bridge | 最小版 24-40h、对齐 80-120h |

### 2. tauri-pilot 内部结构（已读源码）

- **代码量**：plugin ~4500 行 Rust + 1214 行 JS bridge，CLI ~7300 行
- **协议**：自定义 JSON-RPC over Unix socket（macOS/Linux）或 Named Pipe（Windows），不是 W3C WebDriver
- **macOS 支持**：commit `17ef530` 明确"零代码改动，Unix-socket 分支天然兼容"
- **License**：MIT
- **API 公开性**：没用 `tauri::private::*`，全是稳定 API
- **关键依赖**：tauri = "2"（features = ["unstable"]）+ tokio + enigo（press 命令可选）
- **screenshot 走 DOM-to-canvas**（vendored html-to-image），**不需要 macOS 屏幕录制权限**

### 3. 已经在系统上完成的安装

| 工具 | 版本 | 位置 |
|---|---|---|
| Rust toolchain | **1.95.0** | `~/.rustup/toolchains/1.95.0-aarch64-apple-darwin` |
| `tauri-pilot` CLI | v0.5.2 | `~/.cargo/bin/tauri-pilot` |

验证：
```bash
tauri-pilot --help    # 已能跑，列出 30+ 子命令
```

## PoC 当前进度

### 已做的改动（**全部在 try/tauri-pilot-poc 分支，未 commit**）

#### 1. `rust-toolchain.toml`（新建）

```toml
[toolchain]
channel = "1.95.0"
```

原因：tauri-pilot 要求 edition=2024 / rust 1.95.0。**只针对本仓库**，不影响其他项目。

#### 2. `src-tauri/Cargo.toml`（+4 行）

```toml
tauri-plugin-pilot = { git = "https://github.com/mpiton/tauri-pilot", tag = "v0.5.2" }
```

#### 3. `src-tauri/src/lib.rs`（+8 行）

```rust
let mut builder = tauri::Builder::default()...;
#[cfg(debug_assertions)]
{
    builder = builder.plugin(tauri_plugin_pilot::init());
}
builder.setup(...)
```

**release 构建零影响**：cfg(debug_assertions) 保证只在 dev 启用，且 tauri-pilot 本身在 release 也是 no-op。

#### 4. `src-tauri/capabilities/default.json`（+1 行）

加 `"pilot:default"` 到 permissions 数组。**没这一行会导致 eval timeout**（tauri-pilot README 明确说明）。

#### 5. `src-tauri/Cargo.lock`（自动更新）

`cargo update tauri-plugin --precise 2.6.1` 拉了 tauri-plugin 子依赖到 2.6.1，因为 tauri-pilot 要 `^2.6.1`。其他依赖也跟着升了一批（共 +559/-118 行）。**没动直接依赖版本**，只是 lock 文件。

## 唯一阻塞点

`cargo check` 在 build script 阶段失败：

```
resource path `resources/runtime` doesn't exist
```

这是 aijia 自己的 **bundled runtime** 资源（CLAUDE.md "内置运行时（自 0.5.24 起）" 章节），需要先跑：

```bash
bash scripts/prepare-bundled-runtime.sh
```

**这一步会下载 ~85MB（Node 20.18 / Python 3.12.7 / uv 0.4.27）**。
**用户没授权 → 我没跑** → 所以 PoC 停在这里。

> 跟 tauri-pilot 集成没有任何关系——这是 aijia 自己的构建依赖，原本就需要。

## 起床后的下一步

### 选项 A：继续验证 PoC（推荐，30 分钟）

```bash
# 1. 切到 PoC 分支（如果还没切）
cd /Users/a20250311/.codex/worktrees/d633/lotus-app
# 确认: git branch  应该显示 * try/tauri-pilot-poc

# 2. 准备 bundled runtime（85MB 下载）
bash scripts/prepare-bundled-runtime.sh

# 3. 启动 dev
pnpm tauri:dev

# 4. 另开终端验证 PoC
tauri-pilot ping              # 应返回 OK，证明 socket 通了
tauri-pilot windows           # 列出 AIjia 窗口
tauri-pilot snapshot -i       # 截当前页面 a11y tree（重定向到文件看）
tauri-pilot screenshot poc.png # 截图！
tauri-pilot eval "document.title"  # 求值
```

### 选项 B：放弃 PoC（撤销改动）

```bash
cd /Users/a20250311/.codex/worktrees/d633/lotus-app
git checkout -- src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/capabilities/default.json src-tauri/src/lib.rs
rm rust-toolchain.toml
git branch -D try/tauri-pilot-poc  # 切换到其他分支后才能删
```

### 选项 C：PoC 跑通后正式集成

如果选项 A 成功，下一步：

1. **写 e2e helper**：把 `docs/test-intents/spec/tasks/*/rules.md` 里的产品视角断言翻译成 `tauri-pilot` 命令序列
2. **接 MCP**：`tauri-pilot mcp` 启动 MCP server，让 Claude 直接读 rules.md 自动跑回归
3. **写 scenario YAML**：用 `tauri-pilot run scenario.toml` 跑批，加 `--junit` 出报告

## 我没动的东西

- ❌ 没有 commit
- ❌ 没有 push
- ❌ 没动 main 分支
- ❌ 没运行 `prepare-bundled-runtime.sh`（要下载 85MB）
- ❌ 没动其他 worktree（`/Users/a20250311/IdeaProjects/lotus-app` 是干净的 main）
- ❌ 没修改 `/Users/a20250311/github/tauri-pilot`（你下载的源码原封不动）

## 风险评估

| 风险点 | 我的判断 |
|---|---|
| rust 1.95 升级影响 aijia 自己的构建 | **低**——edition=2021 兼容，仅是工具链版本要求 |
| Cargo.lock 大改动 | **中**——升了 tauri-plugin 等依赖，需要跑一次完整测试才放心 |
| tauri-pilot pre-1.0 breaking | **中**——锁了 `tag = "v0.5.2"`，破坏前不会自动升 |
| 作者跑路 | **低**——MIT license，源码已在本地，必要时 fork |
| macOS 上跑不通 | **低**——CI 在 macos-latest 上跑 release v0.5.2 持续绿 |

---

## 关键文件位置

- 本报告：`/Users/a20250311/.codex/worktrees/d633/lotus-app/E2E_POC_REPORT.md`
- tauri-pilot 源码：`/Users/a20250311/github/tauri-pilot/`
- 关键 README：`/Users/a20250311/github/tauri-pilot/README.md`（集成步骤）
- 关键 docs：`/Users/a20250311/github/tauri-pilot/docs/`
- CLI binary：`~/.cargo/bin/tauri-pilot`
