# AIjia — 代码仓库

企业 AI 工作台 Tauri 2.x 桌面应用（React/TS 前端 + Rust 后端 + RuntimeManager 管理的本地运行环境）。

产品名：**AIjia**（元数据/文件名）/ **AI小家**（UI 面向用户）。标识符：`com.aijia.app`。

设计文档在姊妹仓库 `../docs/`（Gitee: `inkess/team-docs`）。架构权威在 `docs/agent-architecture.md`。

## 仓库结构（关键部分）

```
├── CLAUDE.md
├── package.json               # 前端 + version
├── src/                       # React/TS 前端
├── src-tauri/
│   ├── Cargo.toml             # Rust crate + version
│   ├── tauri.conf.json        # Tauri 配置 + version
│   ├── Cargo.lock             # 锁文件 + aijia version
│   ├── src/                   # Rust 源码
│   └── tests/                 # 集成测试
├── scripts/
│   ├── release.py               # 发布入口（交互式，跨平台，强制顺序）
│   ├── bump-version.py          # 同步版本号到 3 个配置文件（跨平台）
│   ├── bump-version.sh          # 同步版本号（macOS 快捷方式）
│   ├── ci-upload-dev.py         # CI: 每次构建上传未签名 dev 包到 OSS
│   ├── ci-generate-download-page.py  # CI: 生成 downloads.html 下载页
│   ├── ci-upload-macos.py       # 本地签名后: macOS release → OSS
│   ├── ci-upload-macos-beta.py  # 本地签名后: macOS beta → OSS
│   ├── ci-upload-windows.py     # 本地签名后: Windows release/beta → OSS
│   ├── ci-finalize.py           # 生成 update.json
│   ├── sign-and-upload-macos.sh    # 本地 macOS 签名+公证+上传全流程
│   ├── sign-and-upload-windows.ps1 # 本地 Windows 签名验证+tauri signer+上传
│   ├── setup-runner-macos.sh    # macOS 签名机环境检查
│   ├── setup-runner-windows.ps1 # Windows 签名机环境检查
│   ├── setup-playwright.sh/.ps1
│   └── bump-homebrew.py
└── .github/workflows/
    └── build-desktop.yml      # GitHub-hosted 构建 + dev 包上传 OSS + 下载页生成
```

## 常用命令

### 开发

```bash
pnpm install                 # 首次或依赖变更
pnpm tauri:dev               # 启动 Tauri 开发模式（前端 + 后端热重载）
pnpm dev                     # 仅启动前端 Vite 开发服务器
```

### 构建

```bash
pnpm tauri:build             # 构建生产包（TypeScript 检查 + Vite build + Tauri bundle）
pnpm build                   # 仅构建前端
pnpm lint                    # ESLint
```

### 测试

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
storage/                        ← L6: Infra Adapter（文件持久化、workspace 管理）
plugin/                         ← 遗留工具插件系统（正在向 RuntimeTool 迁移）
```

**核心约束：`src-tauri/src/runtime/` 下的模块禁止 `use tauri::*`，通过 `RuntimeHost` trait 注入宿主能力。**

### 消息主链路

```
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

### ID 模型

系统内流转的核心标识：`SessionId` > `RunId` > `AgentId` / `ToolCallId`。新增运行态逻辑必须优先使用这套 ID，不再用裸 `conversation_id` 字符串。

### 工具系统（双轨）

- **RuntimeTool**（新）：在 `runtime/tools/dispatcher.rs` 注册，通过 `ToolExecutionContext` + `CapabilityContext` 获取能力，是长期目标路径
- **LegacyToolAdapter**（旧）：将 `plugin/tool_trait.rs` 的 `ToolPlugin` 适配为 `RuntimeTool`，桥接层，不应新增
- 工具实现主体在 `llm/tool_executor/`（upload/dingtalk/search 等）和 `plugin/builtin/tools/`（echo_runtime 等）
- **MCP 工具**（新）：位于 `runtime/mcp/`，通过 `McpConnection -> McpRuntimeTool -> ToolRegistry` 动态注册；对外工具名必须是 `mcp__<server>__<tool>`，disconnect / refresh 时必须同步清理 runtime tool pool 与 `TOOL_CATALOG`

### 事件协议

后端内部发 `RuntimeEvent`，通过 `transport/tauri_event_adapter.rs` 映射为前端 legacy events：

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

### MCP 集成

- `src-tauri/src/runtime/mcp/types.rs`：MCP server 配置、tool definition、fully-qualified 命名规则
- `src-tauri/src/runtime/mcp/connection.rs`：MCP 连接抽象，测试和真实传输都走这一层
- `src-tauri/src/runtime/mcp/runtime_tool.rs`：把远端 MCP tool 包装成 `RuntimeTool`
- `src-tauri/src/runtime/mcp/manager.rs`：管理 server 注册 / connect / refresh / disconnect / unregister 生命周期
- Tauri 启动时会在 `src-tauri/src/lib.rs` 中 `app.manage(Arc<McpServerManager>)`
- 当前仓库已具备 runtime 层 MCP 支持，但**还没有** end-user 配置加载器和前端管理面板

### Skill 系统（新）

Skill 系统采用无状态架构，仅加载 `~/.renlijia/users/{scope}/skills/` 和 `~/.renlijia/skills/` 下的 `SKILL.md` 格式 skill 目录。Runtime 通过 `load_skill` 工具无状态加载 SKILL.md body；不再存在 `switch_skill` / `SkillSessionStore` / workflow pipeline / `plugin.toml` / `workflow.toml` 等遗留概念。

## 前端架构（React/TypeScript）

### 关键模块

- `src/lib/tauri.ts` — 所有 Tauri IPC 的类型化封装（invoke + listen），是前后端接口的唯一真相源
- `src/stores/` — Zustand store（chatStore 是核心，管理会话消息、流式状态、工具执行状态）
- `src/hooks/useStreaming.ts` — 订阅 `streaming:delta`/`streaming:done` 事件并更新 chatStore
- `src/hooks/useTauriEvent.ts` — 通用 Tauri 事件订阅 hook

前端通过 `src/lib/tauri.ts` 中的 `TAURI_EVENTS` 常量订阅事件，不直接使用字符串字面量。

### UI 编写规范（强约束）

写前端 UI 时遵守以下两条硬规则，违反会导致主题不一致 / 深色模式失效 / 设计系统失效：

**1. 颜色必须使用主题变量，禁止硬编码具体颜色**

- ✅ 用语义变量：`bg-background` / `bg-muted` / `bg-primary` / `bg-secondary` / `bg-destructive` / `text-foreground` / `text-muted-foreground` / `text-primary` / `text-primary-foreground` / `border-border` / `border-input` / `ring-ring`
- ❌ 禁止 `bg-white` / `bg-black` / `text-white` / `text-black` / `bg-[#xxx]` / `text-[#xxx]` / `border-white` / `border-black`
- ❌ 用 `border-b` / `border` 等不指定颜色的写法等价于硬编码默认色，必须显式带 `border-border` 或其他语义边框
- 例外：内部纯结构性占位（如 QR code dot pattern、严格指定的品牌色 logo）可以保留具体色值，但要写注释说明为何不能用变量

**2. 复用现成组件，不要手搓已存在的公共能力**

写 UI 之前先 grep `src/components/` 看有没有现成组件，能复用就复用：

- 按钮 → `@/components/ui/button` 的 `<Button>`，靠 `variant` 切色（`default` / `secondary` / `ghost` / `destructive` / `outline` / `link`），不要再叠 `bg-black text-white hover:bg-black/85` 把默认 variant 覆盖掉
- 顶栏 → `@/components/shell/ChatTopBar`（聊天页 / 频道页）或 `@/components/shell/PageTopBar`（普通页），不要手写 `<div data-tauri-drag-region className="flex h-10 ...">`
- 对话框 → `@/components/ui/dialog` 的 `Dialog` / `DialogContent`，外层圆角想生效就给 `DialogContent` 加 `overflow-hidden`
- 下拉 → `@/components/common/AppDropdown`
- 确认弹窗 → `requestConfirm`（`@/components/common/ConfirmDialogHost`）
- Toast → `useNotificationStore.push({ context: 'toast' })`

**Code review 自查清单**：diff 里看到 `bg-black` / `text-white` / `text-[#` / `bg-[#` / `border-white` / 没带颜色的裸 `border-b`、看到自己手写顶栏 / 按钮样式而不是用组件——立刻换成变量 / 公共组件。

## 存储结构

所有运行时数据持久化到 `~/.renlijia/`（`AiJiaHome::from_home()`），不再使用 Tauri app data dir（后者仅用于启动时一次性迁移）：

```
~/.renlijia/
├── conversations/{id}/
│   ├── conv.json                  # 对话元数据
│   ├── messages.N.jsonl           # 消息分片（100 条/片）
│   ├── _current                   # 分片指针 "shard_num:next_seq"
│   ├── compact_boundaries.jsonl   # 压缩边界记录
│   └── file_index.json            # 文件索引
├── subagent_transcripts/          # 子代理完整转录（JSON 数组）
├── skills/                        # 本地 skill 文件
├── crypto/                        # 加密主密钥
├── screenshots/
├── site-profiles/
├── mcp_servers.json
├── permissions.json
└── agent_invocations.json
```

workspace 目录（用户可自定义，默认也是 `~/.renlijia/`）下存放生成物：

- `uploads/` — 用户上传文件副本
- `reports/` / `charts/` / `analysis/` — 生成物
- `logs/` — 运行日志

## 重要架构决策与约束

1. **Tauri command 层只做参数接收 → 转发 Runtime**，不含业务逻辑（见 `docs/architecture-blueprint.md`）
2. **不接受只改 prompt（base.md/daily.md）来修复能力问题**；能力边界应由 runtime/tool/capability/sandbox 保证
3. **新工具应实现 `RuntimeTool` trait**，不应新增 `ToolPlugin` 实现
4. **`CapabilityContext`（`runtime/tools/capability.rs`）是工具获取系统能力的窄接口**，不应扩大它来传入 `LlmGateway`、`AuthManager` 等编排层对象
5. **LLM 协议：生产路径走 Anthropic**（`lotus.rs` → `claude.rs`，直通 `/anthropic/v1/messages`）。`openai.rs` 仅供 `custom.rs::send_openai_compat` 复用 OpenAI 兼容协议；新增 LLM 能力默认按 Anthropic 协议设计。Prompt renderer 用协议中性命名 `ChatPromptRenderer` / `system_message`，**禁止再加 `openai_` 前缀**。System prompt 多段缓存通过 `SystemPromptSegment` + `stream_message_with_segments` 入口透传；system 侧 `cache_control: ephemeral` 上限 3 块（总额度 4，预留 1 给 tools）。详见 `docs/superpowers/specs/2026-05-10-anthropic-protocol-migration-cleanup.md`
6. **Token / Cost 统计必须包含 Anthropic cache 字段**：`TokenUsage` / `TotalTokenUsage` 有 `cache_creation_input_tokens` / `cache_read_input_tokens`；`estimated_cost_usd` 按 1.25× / 0.1× 加权。新增涉及 token 的事件 / 日志字段，必须同步透传到 `TurnCompleted` 与前端 TS 类型，并保留 `#[serde(default)]` 兼容老会话

## 进行中的架构专项

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

## 意图测试框架（test-intents）

规范文档在 `docs/test-intents/`：
- `context/context.md` — 业务规则（settings 优先级、masking 链路、skill 加载语义等）
- `context/capabilities.md` — 测试工具箱（TempDir、MockLlmExecutor、ProbeExecutor 代码片段）
- `context/how-to-test.md` — 操作规范（命名、执行顺序、漂移判断）
- `context/how-to-write-rules.md` — **rules.md 写作规范**（产品视角、具体断言、可复现 fixture、常见陷阱）
- `spec/tasks/<feature>/rules.md` — 该功能的意图列表与断言
- `spec/tasks/<feature>/test-progress.md` — 执行记录（通过/失败/坑）

**继续做某个功能的意图测试：先读对应 `rules.md` + `test-progress.md`，再看 `context/` 四文件，然后按 `how-to-test.md` 规范执行。**
**新建一个功能的 rules.md：先读 `context/how-to-write-rules.md`，再按产品视角逐条写，写完用快速自查清单过一遍。**

## 发布流程（权威 · 自 v0.5.22 起，GitHub-hosted 架构）

**架构：GitHub-hosted runners 构建未签名包 → 本地下载 → 本地签名 → 上传 OSS**。

所有构建在 GitHub-hosted runners 上完成（`macos-14`、`windows-latest`），不再使用 self-hosted runners。签名在本地手动执行：macOS 需 Developer ID 证书 + Apple 公证，Windows 需 SimpleSign (EV 硬件 token)。

### 三种包

| 类型 | 签名 | 来源 | 用途 |
|------|------|------|------|
| **Dev** | 未签名 | CI 每次构建自动上传到 `aijia/dev/` | 开发者快速验证，macOS 需 `xattr -cr`，Windows 有 SmartScreen 警告 |
| **Beta** | 已签名 | 本地签名后上传到 `aijia/beta/v{x}/` | 内测验证，不触发自动更新 |
| **Release** | 已签名 | 本地签名后上传到 `aijia/v{x}/` + `latest/` | 正式版，触发自动更新 |

**下载页**：https://lotus.renlijia.com/aijia/downloads.html（CI 构建后自动生成）

### 流程总览

```
bump 版本号 → push / tag → GitHub-hosted CI 构建（3 平台）→ 自动上传 dev 包到 OSS
                                                              ↓
                              下载 unsigned artifacts → 本地签名（macOS + Windows）→ 上传 beta/release 到 OSS
                                                                                       ↓
                                                                               finalize → update.json → 自动更新
```

### 快速发布（推荐）

```bash
python scripts/release.py             # 交互式菜单
python scripts/release.py status      # 查看当前发布进度
python scripts/release.py start       # 开始新版本
python scripts/release.py beta        # 触发 beta 构建
python scripts/release.py test-passed # 确认测试通过
python scripts/release.py release     # 触发正式构建
python scripts/release.py finalize    # 生成 update.json
```

### Step 0: 改版本号

```bash
python scripts/bump-version.py 0.5.22   # 跨平台，同步 package.json + Cargo.toml + tauri.conf.json
git commit -am "chore: bump to 0.5.22"
git push origin main
```

版本号 4 处必须一致：`package.json` / `tauri.conf.json` / `Cargo.toml` / `Cargo.lock`

### Step 1: CI 构建（GitHub-hosted）

Push tag 触发 CI，3 个 job 并行：

| 平台 | Runner | 产出 |
|------|--------|------|
| macOS ARM64 | `macos-14` | unsigned `.dmg` + `.app.tar.gz` + `.sig` |
| macOS Intel | `macos-14` (cross-compile x86_64) | unsigned `.dmg` + `.app.tar.gz` + `.sig` |
| Windows x64 | `windows-latest` | unsigned `.exe` + `.sig` |

CI 自动：
1. 构建 → 上传 GitHub Artifacts（30 天保留）
2. 上传 dev 包到 OSS `aijia/dev/`（latest + versioned）
3. 生成 downloads.html 上传到 OSS

### Step 2: 本地签名 + 上传

**下载 CI artifacts**：
```bash
gh run download <run-id> -n macos-arm64-unsigned -D build/
gh run download <run-id> -n macos-x64-unsigned -D build/
gh run download <run-id> -n windows-unsigned -D build/
```

**macOS 签名**（codesign → notarize → staple → tauri signer → upload）：
```bash
bash scripts/sign-and-upload-macos.sh 0.5.22 beta            # ARM64
bash scripts/sign-and-upload-macos.sh 0.5.22 beta x86_64     # Intel
bash scripts/sign-and-upload-macos.sh 0.5.22 release          # 正式版
```

**Windows 签名**（SimpleSign 签名 → tauri signer → upload）：
1. 用 SimpleSign 对 `.exe` 进行 Authenticode 签名
2. 运行上传脚本：
```powershell
.\scripts\sign-and-upload-windows.ps1 -Version 0.5.22 -Type beta
.\scripts\sign-and-upload-windows.ps1 -Version 0.5.22 -Type release
```

### Step 3: 测试验证（Beta）

**直接下载**（macOS 已签名公证，双击即可打开）：
- macOS: `https://lotus.renlijia.com/aijia/beta/v0.5.22/AIjia_0.5.22-beta_aarch64.dmg`
- Windows: `https://lotus.renlijia.com/aijia/beta/v0.5.22/AIjia_0.5.22-beta_x64-setup.exe`
- 或从下载页：https://lotus.renlijia.com/aijia/downloads.html

**Homebrew**：`brew install --cask grant-ge/tap/aijia-beta`

测试清单：
- [ ] macOS 双击 DMG 直接打开，无安全警告
- [ ] Windows 安装无安全警告
- [ ] 核心功能冒烟测试 + 版本号正确 + 新功能验证

### Step 4: Finalize（生成 update.json）

```bash
python scripts/release.py finalize
```

### 签名机环境要求

**macOS**（检查：`bash scripts/setup-runner-macos.sh`）：
- Developer ID Application 证书已导入 login keychain
- 环境变量：`APPLE_ID`, `APPLE_PASSWORD`（App-Specific）, `APPLE_TEAM_ID`
- 环境变量：`TAURI_SIGNING_PRIVATE_KEY`, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`
- 环境变量：`OSS_ACCESS_KEY_ID`, `OSS_ACCESS_KEY_SECRET`
- Python + oss2

**Windows**（检查：`.\scripts\setup-runner-windows.ps1`）：
- SimpleSign 签名工具 + EV 硬件 token
- Node.js（用于 `npx @tauri-apps/cli signer sign` 或全局 `tauri` CLI）
- Python + oss2
- 环境变量：`TAURI_SIGNING_PRIVATE_KEY`, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`
- 环境变量：`OSS_ACCESS_KEY_ID`, `OSS_ACCESS_KEY_SECRET`

### GitHub Secrets（CI 构建用）

| Secret | 用途 |
|--------|------|
| `TAURI_SIGNING_PRIVATE_KEY` | Tauri updater Ed25519 密钥 |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | Ed25519 密钥密码 |
| `OSS_ACCESS_KEY_ID` | 阿里云 OSS（dev 包上传 + 下载页生成） |
| `OSS_ACCESS_KEY_SECRET` | 阿里云 OSS |

注：Apple 签名 / Windows 签名相关 secrets 不再需要（签名在本地做）。

### OSS 路径规范

```
aijia/
├── dev/                         # CI 自动上传的未签名 dev 包
│   ├── AIjia_latest_aarch64.dmg    # 最新 macOS ARM64（每次构建覆盖）
│   ├── AIjia_latest_x64.dmg       # 最新 macOS Intel
│   ├── AIjia_latest_x64-setup.exe # 最新 Windows
│   └── v0.5.22/                   # 按版本归档
├── beta/                        # Beta 测试版（已签名）
│   └── v0.5.22/
│       ├── AIjia_0.5.22_x64-setup.exe + .sig
│       ├── AIjia_0.5.22_aarch64.dmg
│       └── AIjia.app.tar.gz + .sig
├── v0.5.22/                     # 正式版（已签名）
│   ├── AIjia_0.5.22_x64-setup.exe + .sig
│   ├── AIjia_0.5.22_aarch64.dmg
│   └── AIjia.app.tar.gz + .sig
├── latest/                      # 正式版最新下载入口
├── downloads.html               # 下载页（CI 自动生成）
└── update.json                  # Tauri 自动更新清单（仅正式版）
```

### CI Workflows

| Workflow | 触发 | 作用 |
|----------|------|------|
| `build-desktop.yml` | `beta-v*` / `v*` tag / manual | GitHub-hosted 构建 3 平台 + dev 包上传 OSS + 下载页生成 |
| `finalize-release.yml` | manual（输入 version） | 生成 update.json |
| `ci.yml` | push main / PR | Rust + TS 类型检查 + lint |

## Git Remotes

- `origin` → `github.com:grant-ge/aiminjia.git`（公开，CI 运行于此）
- `codeup` → `codeup.aliyun.com:renlijia/lotus/lotus-app.git`（国内镜像）

两个都要推。tag 只触发 `origin` 的 CI。

## 关键设计决策

- Tauri 2.x：Ed25519 signed auto-updater（密钥 `~/.tauri/aijia.key`，GitHub Secrets `TAURI_SIGNING_PRIVATE_KEY(_PASSWORD)`）
- 数据存储：SQLite + AES-256-GCM（敏感字段）
- i18n：react-i18next，`src/i18n/{zh-CN,en-US}.json`，默认 zh-CN
- OSS：阿里云 `lotus-releases` bucket，前缀 `aijia/`，CDN `https://lotus.renlijia.com`
- Homebrew：`grant-ge/homebrew-tap` 下 `Casks/aijia.rb`，`on_arm` / `on_intel` 分架构 URL
- GitHub Secrets（4 个，签名在本地做不需要更多）：`TAURI_SIGNING_PRIVATE_KEY`, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`, `OSS_ACCESS_KEY_ID`, `OSS_ACCESS_KEY_SECRET`
- 签名密钥（本地）：`~/.tauri/aijia.key` + Keychain `aijia-tauri-signer`
- OSS 凭证（本地）：Keychain `aijia-oss`，或环境变量 `OSS_ACCESS_KEY_ID` / `OSS_ACCESS_KEY_SECRET`
- **数字员工 SKILL bundle + ResourceConfig + 派活前置补全（2026-05）**：5 个内置员工各带 `defaultSkillId / requiresAttachment / resourceConfigKind / requiresDingtalk` 元数据（`src/features/employees/templates.ts`）。派活前 `runTriggerPrechecks` 决定是否弹文件 picker / 资源 form / 钉钉 alert，因此**派活的唯一入口是 EmployeeDrawer 底部按钮**（卡片点击只打开 Drawer，不再有 inline 派活按钮）。SKILL 内容（`competitive-intelligence`, `sales-followup-rules`）以 managed global skills bundle 分发到 `~/.renlijia/skills/`。dispatch prompt 强制末尾"请立即开始按职责执行"以避免 LLM 等用户指示。详见 `../docs/plans/2026-05-05-employee-skills-and-resources-design.md`。
- **数字员工模板服务化（2026-05-10）**：模板从 `src/features/employees/templates.ts` 硬编码常量演进为 OPS 后台管理的版本化资源。新模块 `runtime/employee/template_store.rs` 持有 `TemplateRef` / `TemplateSnapshot` / `TemplateManifest` 类型和 `ensure_instance_snapshot()` 写盘逻辑（原子写 + sha256 校验 + 幂等）。11 个内置模板的 v1.0.0 JSON 通过 `include_str!("templates_bootstrap.json")` 编译进 binary（`OnceLock` 缓存解析结果）做 bootstrap fallback，离线雇佣可用。每个员工实例 `users/{scope}/employees/{id}/` 下新增 `template/{template.json, manifest.json}` 子目录存放雇佣时冷冻的模板快照——这是后续运行时合并视图的来源。`EmployeeRecord` 加 `template_ref: Option<TemplateRef>`（`#[serde(default)]`，零破坏），`EmployeeStore::create / get / list_unlocked` 自动调用 `stamp_snapshot_for_record`：雇佣时按 `template_id` 匹配 bootstrap → 拷快照、老 record 下次读取时自动补全 `template_ref` + 写出 snapshot（一次性原地迁移，无需启动迁移 hook）。**架构位移已完成**：模板成为 source of truth，record 旧字段（`tool_whitelist / system_prompt_extra / default_skill_id / role / description / avatar`）从"独立配置源"变成"雇佣时从 snapshot 派生的冷冻副本"——但**字段未物理删除**，dispatch / runner / chat 仍读 record 字段。Spec：`lotus/docs/superpowers/specs/2026-05-10-employee-templates-as-a-service.md` §12。
- **数字员工模板 HTTP loader（2026-05-10 / PR3）**：桌面端可从 lotus ops-portal 公共端点拉取已发布模板版本。`template_store::fetch_catalog` / `fetch_manifest` / `download_snapshot` / `ensure_cached` 走 `reqwest` 异步客户端，URL base 默认 `https://ai-ops.renlijia.com`，可通过 `LOTUS_OPS_BASE_URL` env var 覆盖（本地 dev 指 `http://localhost:8082`）。下载产物校验 sha256 后写入全局 cache `~/.renlijia/employee-templates-cache/{tid}/{version}.json`（跨用户共享，因为模板是不可变只读资源，详见 spec §5）。`AiJiaHome::employee_templates_cache_dir()` 暴露路径。新 Tauri 命令：`employee_template_refresh` 拉公共 catalog 后逐个 `ensure_cached`，单个模板失败只 log warn 不中断（partial refresh > hard fail）；`employee_template_catalog` 改为返回 `merge_catalog(bootstrap, cache)` —— 同 `template_id` 时版本字符串高者胜（cache > bootstrap if newer，反之亦然）。`stamp_snapshot_for_record` 也升级为先查 bootstrap、miss 再扫 cache，所以即便老 record 引用的是自定义 org 模板（不在 bootstrap），只要先跑过 `employee_template_refresh` 就能正确回填 `template_ref`。9 个单测覆盖：bootstrap parse / lookup / 快照写读 + 幂等 / cache 读写 / merge 优先级（cache 更新 / bootstrap 更新 / 只有 cache / cache dir 不存在）/ hex 编码。HTTP 路径**未加单测**（要 mock server，PR4 之后再补），但可以从 dev 环境手测：`LOTUS_OPS_BASE_URL=http://localhost:8082 pnpm tauri:dev` → 控制台 invoke `employee_template_refresh`。
- **HireWizard 接入后端 catalog（2026-05-10 / PR4）**：`HireWizard.tsx` 第 1 步的模板网格不再渲染前端硬编码 `BUILTIN_TEMPLATES`。`useEffect` on `open` 触发：先 fire-and-forget `employeeTemplateRefresh()`（拉最新版本到 cache，失败只 warn），再 `employeeTemplateCatalog()` 取合并后的列表（bootstrap ∪ cache），用 `snapshotToTemplate()` 映射为前端 `EmployeeTemplate` 类型后填到 `catalog` state。任何步骤失败都回退到 `BUILTIN_TEMPLATES`，保证离线或服务挂掉时雇佣流不挂。`snapshotToTemplate()` 在 `templates.ts` 新增，做三件事：① 同 `templateId` 已在 `BUILTIN_TEMPLATES` 里时**直接返回硬编码副本**（保证 v1.0.0 期间桌面端 UX 完全等价于 PR4 之前，避免微妙差异如 emoji 渲染）② 不认识的 id 按字段映射（`cron === ''` → `null`，`defaultSkillId === ''` → `null`，等）③ `resourceConfigKind` 来自 `RESOURCE_CONFIG_KIND_BY_ID` 硬编码 map，自定义 org 模板默认 `'none'`（PR6 会用 `resourceConfigSchema` JSON Schema 替代这个闭合枚举）。`findTemplate()` / `EmployeeCard` / `EmployeeDrawer` / `triggerPrechecks` 仍读 `BUILTIN_TEMPLATES`——它们查的是**老员工 record 引用的模板**，按 id 反查是稳定的（PR5 会改成读员工目录里的 `template/template.json` 快照，PR6 删 `RESOURCE_CONFIG_KIND_BY_ID`）。3 个 vitest 单测：`builtin:` 同 id 走 BUILTIN_TEMPLATES verbatim / 不认识的 id 按字段映射 / 空串 → null 的字段归一。
- **运行时 snapshot-first 读取（2026-05-10 / PR5）**：dispatch / chat 派活路径改为优先读员工目录下 `template/template.json` 快照，record 字段降级为兜底。`template_store` 新增 3 个 helper：`effective_tool_whitelist(employees_root, id, fallback)` / `effective_system_prompt_extra(...)` / `effective_default_skill_id(...)`，统一规则是"snapshot wins, falls back to record"，snapshot 文件读取失败只 log warn 不报错。`build_dispatch_prompt` 签名新增 `employees_root: Option<&Path>` 参数：`Some` 时启用 snapshot 查找（生产路径），`None` 时纯走 record 字段（13 个单测保持纯函数行为）。`chat.rs` 派活点把 `employees_dir` 解析提前到 `build_dispatch_prompt` 之前，并把 `OverrideGuard::install` 的 `tool_whitelist` 也走 `effective_tool_whitelist(employees_dir_async)`——这是 LLM 工具过滤的实际生效点。**字段未删**（PR6 才做物理删除）：仅把读取优先级换了，意味着 record 字段从"独立配置源"正式变为"snapshot 缺失时的应急兜底"。新增 4 个单测：snapshot 优先级 / 无 snapshot 回退 / snapshot 空串视为 None（不回退）/ dispatch_prompt 端到端验证 snapshot 覆盖 record。`runtime::employee::` 48 passed（+4），全仓 `cargo test --lib` 837 passed（+4，9 个 pre-existing failures 与本 PR 无关）。
- **Schema-driven 表单（2026-05-10 / PR6）**：自定义 org / private 模板的 instance config 现在可以用 JSON Schema 驱动渲染，不再依赖闭合枚举 `ResourceConfigKind` 和对应的硬编码 React 表单。新组件 `src/features/employees/forms/SchemaForm.tsx`（~340 行）支持 7 种 widget：string text / textarea (`ui:widget=textarea`) / enum select / number/integer / boolean / array-of-string (tag input) / array-of-enum (checkbox group)，附带 URL/email format / minLength/maxLength / minimum/maximum / minItems/maxItems / required 校验，错误信息在 `touched[field]` 后显示。HireWizard step 3 的路由规则：① 模板有 `resourceConfigKind !== 'none'` → 走老的 5 个 hand-tuned 表单（11 个内置员工保持原样，UX 完全等价）② 否则若 `resourceConfigSchema.properties` 非空 → 走 SchemaForm ③ 都没有 → 跳过 step 3 直接雇佣。`hasSchemaForm(template)` 是分诊函数，控制 step indicator / Next 按钮 label / 实际渲染。`EmployeeTemplate.resourceConfigSchema?: Record<string, unknown> | null` 可选字段，`snapshotToTemplate()` 从后端 snapshot 透传过来。**11 个内置模板没有 schema**（PR1 灌库时留空），所以这条路径只对未来 OPS portal 上发布的自定义模板生效——零回归风险。10 个 vitest 单测覆盖所有 widget 类型 + 校验路径 + 数字归一 + cancel 行为。Build：`tsc --noEmit` 0 errors；vitest `src/features/employees/` 42 passed（+10）。**有意未删 5 个 hardcoded form**：它们对 11 个内置员工是最优 UX（多行表格、URL 校验、知识源上传等），SchemaForm 是为新场景准备的、不替换它们。spec §10 PR7 计划之后再做最终 cleanup。
- **数字员工模板生产部署 + 集成测试（2026-05-10）**：lotus 侧上线 5 个修复后桌面端到 OSS 的下载链路真正打通。① **OSS 签名 URL**：bucket 私有，`PublicManifest` 改为返回 `bucket.SignURL` 1h 签名 URL，避免 403 ② **签名 URL host 重写**：OSS client 配置的是 `oss-cn-hangzhou-internal.aliyuncs.com`（VPC 内部），`SignURL` 继承导致桌面端走公网拉不到——`rewritePublicHost` 把签名 URL 的 scheme+host 改写为 `OSS.PublicURL` 的公网 https，path + query 保留 ③ **Publish 输出 camelCase**：Go `model.EmployeeTemplate` 用 `json:"template_id"` snake_case，Rust `TemplateSnapshot` 用 `#[serde(rename_all="camelCase")]`，桌面端反序列化静默失败（所有字段变 default）；新增 `desktopTemplateSnapshot` view-model 仅用于 publish 时渲染 OSS JSON，DB 行不动 ④ **`shared/pkg/version`** 统一两位 `MAJOR.MINOR` 版本号校验，3 个 publish handler（ops 模板 + ops skill + tenant skill）全部接入，根治"一会两位一会三位"的历史 drift ⑤ 11 个 v1.0 模板已重新发布到生产 OSS。集成测试 `src-tauri/tests/employee_template_lifecycle_test.rs` 含 10 个 case：8 个 hermetic（场景 1 第一次加载 / 场景 2 老用户迁移 / 场景 3 升级 path）+ 2 个 live `--ignored`（真打 `ai-ops.renlijia.com`，验证 manifest → 签名 URL → 下载 → camelCase 解码 + non-empty toolWhitelist 等字段，证明上面 5 个修复端到端可用）。所有 10 个 test 通过 ✅。详见 lotus spec §13。
- **数字员工状态机（v0.5.7）**：`EmployeeRecord.enabled: bool` 拆为 `lifecycle: EmployeeLifecycle`（Active / Paused / Archived）+ `cron_enabled: bool` 二维独立字段。旧文件通过 `#[serde(alias = "enabled")]` 自动迁移到 `cron_enabled`。运行态来自后端 `EmployeeActiveRuns` Mutex<HashMap>（不再用前端"10 分钟内 Running"启发式），由 `ActiveRunGuard` RAII Drop 守卫保证 panic / cancel / 提前 return 都不会泄漏。删除 = 软删除（lifecycle=Archived），调度器每 60s tick 自动 `purge_old_archived(7d)`，per-record 检查走 race-safe `purge_if_archived_older_than`（同一把锁内 re-check lifecycle + age，防止"用户刚恢复就被 purge"）。新增 Tauri 命令：`employee_active_run` / `employee_stop_run` / `employee_restore` / `employee_purge`。前端 deriveStatus 7 状态优先级：archived > running > has-report > paused > needs-setup > scheduled > idle。详见 `../docs/plans/2026-05-06-employee-state-machine-IMPLEMENTATION.md`。
- **Windows 兼容性约定（v0.5.7）**：所有 git 子进程必须传 `-c core.quotepath=false`（中文文件名展示）；用户可编辑 JSON 文件（mcp_config / global_config）的读路径走 `storage::text_io::read_to_string_strip_bom`（剥 Win10 Notepad BOM）；外部 CLI 输出（dws / where.exe / tasklist）解码走 `storage::console_decode::decode_console_bytes`（Windows GBK fallback，靠 `encoding_rs`）；MCP 子进程 spawn 时强制 `PYTHONIOENCODING=utf-8` / `PYTHONUTF8=1` / `LANG=en_US.UTF-8`；hooks runner + skill `!cmd` 替换在 Windows 走 `powershell.exe -NoProfile -Command`（不能裸 `sh -c`）；用户/LLM 提供的文件名走 `storage::safe_filename::ensure_safe_filename`（CON/PRN/COM*/LPT* 保留名 + 禁字符 `<>:"\|?*` + 尾部 `.`/空格 + 长度 ≤ 200）；任何写到磁盘的状态文件优先 tmp + rename 原子写（参考 `runtime::employee::store::write_atomic`），目录删除走 `remove_dir_all_retry` 3×150–300ms backoff。
- **Windows 子进程黑窗抑制（v0.5.8）**：所有 `Command::spawn` / `.output()` 必须先调 `.no_window()`（`storage::process_ext::NoWindowExt` trait extension），它在 Windows 上注入 `CREATE_NO_WINDOW = 0x08000000` 创建标志，其它平台是 no-op。漏一个就在 Windows 上看到 cmd.exe / conhost.exe 一闪而过。
- **max_tokens 按模型自动选（v0.5.8）**：默认输出预算从写死的 4096/100000 改成 `llm::max_tokens::default_max_tokens_for_model(name)` 启发式查表（DeepSeek V4 = 384k、GPT-5 = 128k、GLM-4.5/4.6 = 96k、Claude Sonnet/Opus + Gemini-2 = 64k、Qwen3 / qwen-max-2025 = 16k、Qwen-max-longcontext = 30k、其他兜底 8192）。新增上限按模型加进表里。从 `chat_turn_driver` 调（`llm_settings.primary_model` 在 scope 内）；想覆盖的���用方传 `Some(value)` 即可，否则传 `None` 让 driver 走 per-model 默认。
- **跨平台拖拽上传（v0.5.9）**：Tauri 2 webview 拦截 HTML5 drop 事件，React `onDrop` 永不触发；唯一靠谱的入口是 `getCurrentWebview().onDragDropEvent`。`useDragDropListener`（在 `App` 顶层 mount 一次）订阅 native 事件，把 resolved `PendingAttachment[]` push 进 `useDropInbox`（zustand pull queue），HomeTaskComposerCard / ChatBottomArea 各自 useEffect drain。新增附件路径校验（`useChatAttachments::isAcceptablePastedPath` / `makePendingAttachment` / `resolvePastedPaths`）必须支持 `[\\/]` 双分隔符 + Windows `C:\` 卷根 + 系统目录前缀拒绝。
- **剪贴板图片粘贴**：`useComposerPaste.handlePaste` 支持截图/复制图片粘贴。流程：同步提取 `clipboardData.items` 中的 image blob（异步后 clipboardData 会被浏览器清空）→ 先尝试 native file paths（Finder 复制文件）→ 路径为空时 fallback 到 image blob → `saveClipboardImage()` 写入 `tmpImage/` → 添加为 `PendingAttachment`。`saveClipboardImage` 从 `useChatAttachments` 传入（Tauri IPC → Rust `save_clipboard_image_to_tmp`）。ChatBottomArea + HomeTaskComposerCard 两个 composer 均已接入。
- **租户运行时换肤（C 路径，2026-05-09 重做）**：tenant 4 色（`accentColor` / `primaryColor` / `bgColor` / `sidebarBgColor`）+ `productName` + `logoUrl` + `fontFamily` 由 lotus 后台（`tenant-portal/web/src/pages/settings/branding.tsx`，10 个 PRESET_THEMES）下发；后端 `auth/state.rs::TenantInfo` 透传，前端 `src/stores/brandingStore.ts::applyBranding` 派生 ~40 个 CSS 变量（design.pen + legacy `--color-*` 双套命名空间）。派生算法在 `src/lib/themeUtils.ts`：accent 用 lighten/darken/rgba 派生 hover/subtle/-50/-100/...；primary 决定 `--foreground` 和文字色；bg/sidebar 用 `mixColors(bg, fg, ratio)` 派生 muted/border/popover。`isDarkColor` 决定 `--*-foreground` 用 `#FFFFFF` 还是 `#1A1A1A`，所以 dark mode 不需要 `.dark` selector，直接 4 色填深色即可。`authStore` 在 `restoreFromStorage / login` 后调 `applyTenantBranding(info)`，`logout` 时 `useBrandingStore.reset()`。**前端不再提供本地 accent 选择器**（之前 `GeneralPanel` 的强调色 swatch 已删除）——皮肤完全由 lotus 后台定，避免"用户本地选择 vs 租户下发"的覆盖冲突。`localStorage` 里历史残留的 `aijia-accent-color` 和后端 settings 里的 `accentColor` 字段都不再被读取。
- **标题栏 = accent 色（2026-05-09）**：mac Overlay 区 + Windows 自绘标题栏统一用 `bg-primary text-primary-foreground` 显示租户 accent，是用户感知最强的换肤点。`App.tsx` 顶层 wrapper 用 `bg-background`（不再是 `bg-sidebar`），让租户 bgColor 一改 wrapper 就跟着变。Windows 标题栏拖拽踩坑：① `flex-1` 占位 div 必须有 `data-tauri-drag-region`，吃掉中间空白区域的拖拽；② `WindowControls` 外层 `onMouseDown stopPropagation`，否则点击关闭按钮会被父级拖拽吞；③ `UpdateAvailableLink` 也必须包 `stopPropagation` 容器，否则点击更新链接没反应（0.3.x 历史 bug）。mac Overlay 拖拽由系统提供，**不要**额外绑 `onMouseDown=startDragging` 否则跟系统冲突。Windows 标题栏底部用 `border-b border-primary-foreground/15` 做半透明分隔线，避免 accent 与 background 色差小时糊在一起。

