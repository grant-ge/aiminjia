# AIjia — 代码仓库

企业 AI 工作台 Tauri 2.x 桌面应用（React/TS 前端 + Rust 后端 + 捆绑 Python 3.12 数据分析运行时）。

产品名：**AIjia**（元数据/文件名）/ **AI小家**（UI 面向用户）。标识符：`com.aijia.app`。

## 仓库结构（关键部分）

```
code/
├── CLAUDE.md                  # 本文件
├── package.json               # 前端 + version
├── src/                       # React/TS 前端
├── src-tauri/
│   ├── Cargo.toml             # Rust crate + version
│   ├── tauri.conf.json        # Tauri 配置 + version
│   ├── Cargo.lock             # 锁文件 + aijia version
│   ├── python-runtime ->      # 软链（不入库），指向 target/<arch>/release/python-runtime/
│   ├── target/aarch64-apple-darwin/release/python-runtime/   # ARM runtime（不入库）
│   └── target/x86_64-apple-darwin/release/python-runtime/    # Intel runtime（不入库）
├── scripts/
│   ├── setup-python.sh/.ps1       # 下载 python-build-standalone + 装依赖
│   ├── setup-playwright.sh/.ps1   # Playwright 浏览器 sidecar
│   ├── ci-upload-windows.py       # CI 用：Windows runner 传 OSS
│   ├── ci-upload-macos.py         # CI 用：macOS runner 传 OSS（接 arch 参数）
│   ├── ci-finalize.py             # CI 用：Ubuntu runner 生成 update.json
│   ├── bump-homebrew.py           # 本地：发版后更新 Homebrew cask
│   └── upload-x64.py              # 本地可选：Intel 本地构建 + 传 OSS
└── .github/workflows/
    └── build-desktop.yml          # tag push → CI 构建 + 直传 OSS + update.json
```

设计文档在姊妹仓库 `../docs/`（Gitee: `inkess/team-docs`）。架构权威在 `docs/agent-architecture.md`。

## 开发命令

```bash
pnpm install                 # 首次或依赖变更
bash scripts/setup-python.sh # 首次：装本机 arch 的 bundled Python（⚠️ 见下）
pnpm tauri dev               # 本地 dev
pnpm tauri build             # 本地打当前架构包（发版通常不用 — CI 全自动）
pnpm lint                    # ESLint
pnpm build                   # 仅前端 bundle
```

## 发布流程（权威 · 自 v0.4.14 起）

推 tag → CI 全自动 → 本地跑一行 Homebrew bump。就这样。

### 1. 改版本号（4 处，都要改）

- `package.json` → `version`
- `src-tauri/tauri.conf.json` → `version`
- `src-tauri/Cargo.toml` → `version`
- `src-tauri/Cargo.lock` → `[package] name = "aijia"` 下的 `version`

### 2. Commit + tag + push

# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## 项目概述

Lotus-App（品牌名 AIjia）是一个 Tauri v2 桌面应用，运行时为 `WebView(React/TS) + Tauri Host(Rust) + 子进程(Python/Playwright)`，主要提供 AI 驱动的数据分析和工作助手功能。

## 常用命令

### 开发

```bash
# 启动 Tauri 开发模式（前端 + 后端热重载）
pnpm tauri:dev
git commit -am "release: vX.Y.Z"
git tag vX.Y.Z
git push codeup main && git push codeup vX.Y.Z
git push origin main && git push origin vX.Y.Z   # 这步触发 CI
```

### 3. CI 自动做（`.github/workflows/build-desktop.yml`）

# 仅启动前端 Vite 开发服务器
pnpm dev
```
| Job | Runner | 脚本 | 产出 |
|-----|--------|------|------|
| `build (windows-latest)` | Windows | `ci-upload-windows.py` | `.exe` + `.sig` → OSS |
| `build (macos-14)` | macOS arm64 | `ci-upload-macos.py` | `.dmg` + `.app.tar.gz` + `.sig` → OSS |
| `finalize` | Ubuntu | `ci-finalize.py` | `update.json` → OSS |

### 构建
`build` 阶段并行；`finalize` 依赖两个 `build` 都成功。全程约 22-25 分钟。

Windows OSS 上传的 PowerShell 步骤开启 `PYTHONUTF8=1` + 所有脚本 ASCII 打印 + 每步 `$LASTEXITCODE` 检查，避免历史上 charmap codec 崩溃。

### 4. 等 CI 绿（可监控）

```bash
# 构建生产包（TypeScript 检查 + Vite build + Tauri bundle）
pnpm tauri:build

# 仅构建前端
pnpm build
gh run watch $(gh run list -R grant-ge/aiminjia -w "Build Desktop Apps" -L 1 --json databaseId --jq '.[0].databaseId') \
  -R grant-ge/aiminjia --exit-status
```

### 测试
### 5. Homebrew bump（本地唯一手动步骤）

```bash
# 前端单测（Vitest）
pnpm test

# 前端关键集成测试（事件联调回归）
pnpm exec vitest run src/lib/tauri.events.test.ts src/hooks/useStreaming.integration.test.tsx src/stores/chatStore.test.ts

# Rust 全部测试
cd src-tauri && cargo test

# Rust 单个测试文件（集成测试在 tests/ 目录）
cd src-tauri && cargo test --test tauri_event_adapter_test -- --nocapture

# Rust review_ 系列回归测试（验证架构约束）
cd src-tauri && cargo test review_ --tests --no-fail-fast

# Rust 按名称过滤单测
cd src-tauri && cargo test <test_name> -- --nocapture
python3 scripts/bump-homebrew.py X.Y.Z
```

### 6. 验证
### 代码检查

```bash
curl -sI https://lotus.renlijia.com/aijia/vX.Y.Z/AIjia_X.Y.Z_aarch64.dmg        # 200
curl -sI https://lotus.renlijia.com/aijia/vX.Y.Z/AIjia_X.Y.Z_x64-setup.exe      # 200
curl -s  https://lotus.renlijia.com/aijia/update.json | jq '.version, .platforms | keys'
# 前端 ESLint
pnpm lint
```

## 后端架构（Rust）

### 分层结构（从上到下）

```
transport/tauri_commands/       ← L1: Transport Adapter（Tauri IPC 入口，禁止包含业务逻辑）
runtime/                        ← L2: Session/Query Runtime（核心编排层）
  session_runtime.rs            ← 驱动一次完整 agentic turn
  query_engine.rs               ← 会话级编排，transport-neutral
  tools/                        ← L3: Tool Runtime（工具注册、路由、权限、执行）
  agent/                        ← L4: Task/Agent Runtime（子代理、任务生命周期）
  store/                        ← L5: State Store（repository trait + file-based 实现）
llm/                            ← L6: Infra Adapter（LLM provider、tool_executor）
python/                         ← L6: Infra Adapter（Python 沙箱执行）
storage/                        ← L6: Infra Adapter（文件持久化、workspace 管理）
plugin/                         ← 遗留工具插件系统（正在向 RuntimeTool 迁移）
```

## 三平台完整发版时序（Windows + macOS arm + macOS Intel）
**核心约束：`src-tauri/src/runtime/` 下的模块禁止 `use tauri::*`，通过 `RuntimeHost` trait 注入宿主能力。**

CI 管 Windows + macOS arm，本地补 Intel。两条线并行跑，总耗时 ~25min：
### 消息主链路

```
t=0    git tag vX.Y.Z && git push origin vX.Y.Z
       │
       ├── [CI 线] (25min)                         [本地 Intel 线] (~15min)
       │   ├ build windows  ─┐                    ├ 切软链到 Intel runtime
       │   ├ build macos-arm ─┤→ finalize          ├ pnpm tauri build --target x86_64
       │   │                  │  (update.json)     ├ hdiutil create DMG
       │   │                  ▼                    │
       │   └ update.json in OSS ✓ (~25min)         └ DMG + .app.tar.gz 就绪 ✓ (~15min)
       │                                           │
       ▼                                           ▼
t≈25   两条线必须都就绪 ─────────────────────────────┘
       │
       ▼
       python3 scripts/upload-x64.py X.Y.Z        # 读 OSS 现有 update.json → merge darwin-x86_64 → 写回
       切软链回 ARM
       python3 scripts/bump-homebrew.py X.Y.Z
       curl update.json 验证 3 平台全在
invoke('send_message')
  → transport/tauri_commands/chat.rs::TauriChatCommandAdapter::send_message()
    → SessionRuntime::run_chat_request()
      → RuntimeChatTurnDriver::run_chat_turn()
        → QueryEngine / ToolDispatcher
          → RuntimeTool / LegacyToolAdapter
      → RuntimeEventBus
        → TauriEventAdapter → app.emit() 发 legacy events 给前端
      → runtime/store/ 持久化
```

**⚠️ 关键时序**：`upload-x64.py` 必须在 **CI 的 finalize job 完成后**才跑，否则 CI 的 update.json（只含 2 平台）会覆盖掉 Intel 的条目。
### ID 模型

系统内流转的核心标识：`SessionId` > `RunId` > `AgentId` / `ToolCallId`。新增运行态逻辑必须优先使用这套 ID，不再用裸 `conversation_id` 字符串。

## 可选：macOS Intel 本地补包
### 工具系统（双轨）

CI 不打 Intel（GitHub 已下线 native Intel runner）。Intel 用户少，仅需要时在本机补：
- **RuntimeTool**（新）：在 `runtime/tools/dispatcher.rs` 注册，通过 `ToolExecutionContext` + `CapabilityContext` 获取能力，是长期目标路径
- **LegacyToolAdapter**（旧）：将 `plugin/tool_trait.rs` 的 `ToolPlugin` 适配为 `RuntimeTool`，桥接层，不应新增
- 工具实现主体在 `llm/tool_executor/`（upload/load/execute_python/report/chart 等）和 `plugin/builtin/tools/`（browse/extract 等）
- **MCP 工具**（新）：位于 `runtime/mcp/`，通过 `McpConnection -> McpRuntimeTool -> ToolRegistry` 动态注册；对外工具名必须是 `mcp__<server>__<tool>`，disconnect / refresh 时必须同步清理 runtime tool pool 与 `TOOL_CATALOG`

```bash
# 1) 软链切到 Intel runtime（target/x86_64-apple-darwin/release/python-runtime 必须已就位）
rm src-tauri/python-runtime
(cd src-tauri && ln -sfn target/x86_64-apple-darwin/release/python-runtime python-runtime)
file src-tauri/python-runtime/bin/python3.12   # 验证 x86_64
### 事件协议

# 2) 签名 env
export TAURI_SIGNING_PRIVATE_KEY=$(cat ~/.tauri/aijia.key)
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD=$(security find-generic-password -s aijia-tauri-signer -a password -w)
后端内部发 `RuntimeEvent`，通过 `transport/tauri_event_adapter.rs` 映射为前端 legacy events：

# 3) Cross-compile（Rosetta 下 pip 也走 x86_64）
pnpm tauri build --target x86_64-apple-darwin --bundles app
hdiutil create -volname "AIjia" \
  -srcfolder src-tauri/target/x86_64-apple-darwin/release/bundle/macos/AIjia.app \
  -ov -format UDZO \
  src-tauri/target/x86_64-apple-darwin/release/bundle/dmg/AIjia_X.Y.Z_x64.dmg
| RuntimeEventKind | 前端 Tauri Event |
|---|---|
| StreamDelta | `streaming:delta` |
| StreamDone | `streaming:done` |
| ToolCallExecuting | `tool:executing` |
| ToolCallCompleted | `tool:completed` |
| PermissionAskRequired | `permission:ask` |
| MessagePersisted | `message:updated` |
| AgentIdle | `agent:idle` |
| TaskStatusChanged | `task:status-changed` |

# 4) 上传 + 自动 merge 到 update.json
python3 scripts/upload-x64.py X.Y.Z
### MCP 集成

# 5) 切回 ARM（重要！本地 pnpm tauri dev 用 ARM）
rm src-tauri/python-runtime
(cd src-tauri && ln -sfn target/aarch64-apple-darwin/release/python-runtime python-runtime)
```
- `src-tauri/src/runtime/mcp/types.rs`：MCP server 配置、tool definition、fully-qualified 命名规则
- `src-tauri/src/runtime/mcp/connection.rs`：MCP 连接抽象，测试和真实传输都走这一层
- `src-tauri/src/runtime/mcp/runtime_tool.rs`：把远端 MCP tool 包装成 `RuntimeTool`
- `src-tauri/src/runtime/mcp/manager.rs`：管理 server 注册 / connect / refresh / disconnect / unregister 生命周期
- Tauri 启动时会在 `src-tauri/src/lib.rs` 中 `app.manage(Arc<McpServerManager>)`
- 当前仓库已具备 runtime 层 MCP 支持，但**还没有** end-user 配置加载器和前端管理面板；若要接真实 server，需要先实现 `McpConnection`，再由宿主层把连接注册到 `McpServerManager`

## Python Runtime 双架构软链规范
### Python 沙箱

`src-tauri/python-runtime` 是**软链**，指向 `target/<arch>-apple-darwin/release/python-runtime/` 里的真实目录。两份 runtime 常驻 `target/<arch>/`，切换只改软链 —— 切架构零下载。
- 配置入口：`python/sandbox.rs` — `SandboxConfig::for_workspace()` 设置允许路径（写死为 workspace 的 7 个子目录）
- 执行入口：`python/runner.rs` — `PythonRunner`
- 沙箱通过 `_safe_open` 限制写路径，通过 `validate_code()` 静态检查危险模式

**首次设置**（两份都要建一次）：
## 前端架构（React/TypeScript）

```bash
# ARM
rm -rf src-tauri/python-runtime
PIP_INDEX_URL=https://pypi.tuna.tsinghua.edu.cn/simple \
PIP_TRUSTED_HOST=pypi.tuna.tsinghua.edu.cn \
bash scripts/setup-python.sh
mkdir -p src-tauri/target/aarch64-apple-darwin/release
mv src-tauri/python-runtime src-tauri/target/aarch64-apple-darwin/release/python-runtime
### 关键模块

# Intel
PIP_INDEX_URL=https://pypi.tuna.tsinghua.edu.cn/simple \
PIP_TRUSTED_HOST=pypi.tuna.tsinghua.edu.cn \
arch -x86_64 bash scripts/setup-python.sh
mkdir -p src-tauri/target/x86_64-apple-darwin/release
mv src-tauri/python-runtime src-tauri/target/x86_64-apple-darwin/release/python-runtime
- `src/lib/tauri.ts` — 所有 Tauri IPC 的类型化封装（invoke + listen），是前后端接口的唯一真相源
- `src/stores/` — Zustand store（chatStore 是核心，管理会话消息、流式状态、工具执行状态）
- `src/hooks/useStreaming.ts` — 订阅 `streaming:delta`/`streaming:done` 事件并更新 chatStore
- `src/hooks/useTauriEvent.ts` — 通用 Tauri 事件订阅 hook

# 默认软链到 ARM
(cd src-tauri && ln -sfn target/aarch64-apple-darwin/release/python-runtime python-runtime)
```
### 事件订阅原则

设清华镜像环境变量是因为默认 PyPI 在国内走代理常超时。
前端通过 `src/lib/tauri.ts` 中的 `TAURI_EVENTS` 常量订阅事件，不直接使用字符串字面量。

## 关键设计决策
## 存储结构

- Tauri 2.x：Ed25519 signed auto-updater（密钥 `~/.tauri/aijia.key`，GitHub Secrets `TAURI_SIGNING_PRIVATE_KEY(_PASSWORD)`）
- 数据存储：SQLite + AES-256-GCM（敏感字段）
- i18n：react-i18next，`src/i18n/{zh-CN,en-US}.json`，默认 zh-CN
- OSS：阿里云 `lotus-releases` bucket，前缀 `aijia/`，CDN `https://lotus.renlijia.com`
- Homebrew：`grant-ge/homebrew-tap` 下 `Casks/aijia.rb`，`on_arm` / `on_intel` 分架构 URL
- GitHub Secrets（4 个，都已就位）：`TAURI_SIGNING_PRIVATE_KEY`, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`, `OSS_ACCESS_KEY_ID`, `OSS_ACCESS_KEY_SECRET`
所有运行时数据持久化到 workspace 目录（`AppStorage`，基于 JSON 文件）：

## 数据存储位置
- `conversations/{id}/` — 对话数据（`conv.json`、`messages.*.jsonl`、`file_index.json`）
- `workspace/uploads/` — 用户上传文件的副本
- `workspace/exports/` / `reports/` / `charts/` / `analysis/` — 生成物
- `shared/memory/` — 跨对话记忆

- 本地数据：macOS `~/Library/Application Support/com.aijia.app/`，Windows `%APPDATA%\com.aijia.app\`
- 签名密钥（本地开发/Intel 构建）：`~/.tauri/aijia.key` + Keychain `aijia-tauri-signer`
- OSS 凭证（本地 `upload-x64.py`/`bump-homebrew.py` 用）：Keychain `aijia-oss`，或环境变量 `OSS_ACCESS_KEY_ID` / `OSS_ACCESS_KEY_SECRET`
## 重要架构决策与约束

## Git Remotes
1. **Tauri command 层只做参数接收 → 转发 Runtime**，不含业务逻辑（见 `docs/architecture-blueprint.md`）
2. **不接受只改 prompt（base.md/daily.md）来修复能力问题**；能力边界应由 runtime/tool/capability/sandbox 保证
3. **新工具应实现 `RuntimeTool` trait**，不应新增 `ToolPlugin` 实现
4. **`CapabilityContext`（`runtime/tools/capability.rs`）是工具获取系统能力的窄接口**，不应扩大它来传入 `LlmGateway`、`AuthManager` 等编排层对象

- `origin` → `github.com:grant-ge/aiminjia.git`（公开，CI 运行于此）
- `codeup` → `codeup.aliyun.com:renlijia/lotus/lotus-app.git`（国内镜像）
## 进行中的架构专项

两个都要推。tag 只触发 `origin` 的 CI。
当前有 4 个进行中的架构专项，定义在 `docs/2026-04-12-runtime-gap-problem-statement.md`：

1. **Workspace-First 文件能力模型**（计划：`docs/superpowers/plans/2026-04-12-workspace-first-file-runtime-plan.md`）
2. **Atomic Tool 工具体系**
3. **Prompt Slimming 提示词职责回收**
4. **Skill 本地导入/打包导入模型统一**

架构总蓝图：`docs/architecture-blueprint.md`；分期计划索引：`docs/superpowers/plans/README.md`

## 集成测试文件命名惯例

`src-tauri/tests/` 下：
- `review_*.rs` — 架构约束回归测试，验证各期实施后约束不被破坏
- `*_integration_test.rs` — 跨模块集成测试
- `*_test.rs` — 针对单个功能的集成测试
