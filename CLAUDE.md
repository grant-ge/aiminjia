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
│   ├── sign-windows.ps1         # Windows 本地签名（签名机用）
│   ├── ci-upload-windows-unsigned.py  # CI: 未签名 exe → staging
│   ├── ci-upload-macos.py       # CI: macOS release → OSS
│   ├── ci-upload-macos-beta.py  # CI: macOS beta → OSS
│   ├── ci-finalize.py           # CI: 生成 update.json
│   ├── setup-playwright.sh/.ps1
│   ├── bump-homebrew.py
│   └── upload-x64.py            # 本地 Intel 构建上传
└── .github/workflows/
    └── build-desktop.yml      # tag push → CI 构建 + 直传 OSS + update.json
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

## 发布流程（权威 · 自 v0.5.22 起）

三阶段：**Beta 测试（已签名）→ 测试验证 → 正式发布（已签名）**。Windows 代码签名必须在本地签名机完成（证书绑定物理机），macOS 签名在 CI 自动完成。

### 流程总览

```
bump 版本号 → beta tag → CI 构建 → Windows 本地签名 → 测试
                                                        ↓
                                            release tag → CI 构建 → Windows 本地签名 → finalize → 自动更新生效
```

### 快速发布（推荐）

使用交互式发布管理器，自动引导并阻止跳步：

```bash
python scripts/release.py             # 交互式菜单（macOS / Windows 均可）
python scripts/release.py status      # 查看当前发布进度
python scripts/release.py start       # 开始新版本
python scripts/release.py beta        # 触发 beta 构建
python scripts/release.py sign-beta   # Windows 签名机执行
python scripts/release.py test-passed # 确认测试通过
python scripts/release.py release     # 触发正式构建
python scripts/release.py sign-release # Windows 签名机执行
python scripts/release.py finalize    # 生成 update.json
```

发布状态存储在 `.release-state.json`（已 gitignore），任何人 clone 后从头开始。

### Step 0: 改版本号

```bash
python scripts/bump-version.py 0.5.22   # 跨平台，同步 package.json + Cargo.toml + tauri.conf.json
# 手动更新 src-tauri/Cargo.lock（cargo build 自动更新，或 cargo update -p aijia）
git commit -am "chore: bump to 0.5.22"
git push codeup main && git push origin main
```

版本号 4 处必须一致：
- `package.json` → `version`
- `src-tauri/tauri.conf.json` → `version`
- `src-tauri/Cargo.toml` → `version`
- `src-tauri/Cargo.lock` → `[package] name = "aijia"` 下的 `version`

### Step 1: Beta 构建（已签名，用于测试）

```bash
python scripts/release.py beta        # 交互式，自动创建 tag 并 push
# 或手动：git tag beta-v0.5.22 && git push origin beta-v0.5.22
```

**CI 自动做**：
| 平台 | 产出 | 去向 |
|------|------|------|
| macOS arm64 | `.dmg` + `.app.tar.gz` + `.sig`（Apple 签名） | `aijia/beta/v0.5.22/` |
| Windows x64 | `.exe`（未签名） | `aijia/staging/beta/v0.5.22/` |

**Windows 本地签名**（CI 完成后，在签名机执行）：

```powershell
# 方式 1：交互式（推荐，有步骤检查）
python scripts/release.py sign-beta

# 方式 2：直接调用
.\scripts\sign-windows.ps1 -Version 0.5.22 -ReleaseType beta
```

脚本自动：下载未签名 exe → Authenticode 签名 → 重新生成 Tauri updater `.sig` → 上传到 `aijia/beta/v0.5.22/`

### Step 2: 测试验证

测试清单：
- [ ] Windows 安装无安全警告（签名验证通过）
- [ ] macOS 安装无安全警告
- [ ] 核心功能冒烟测试
- [ ] 版本号显示正确
- [ ] 新功能 / 修复验证

Beta 下载地址：
- macOS: `https://lotus.renlijia.com/aijia/beta/v0.5.22/AIjia_0.5.22_aarch64.dmg`
- Windows: `https://lotus.renlijia.com/aijia/beta/v0.5.22/AIjia_0.5.22_x64-setup.exe`

### Step 3: 正式发布（已签名）

测试通过后：

```bash
python scripts/release.py release      # 交互式，有确认提示
# 或手动：git tag v0.5.22 && git push origin v0.5.22
```

**CI 自动做**：
| 平台 | 产出 | 去向 |
|------|------|------|
| macOS arm64 | `.dmg` + `.app.tar.gz` + `.sig`（Apple 签名） | `aijia/v0.5.22/` + `latest/` |
| Windows x64 | `.exe`（未签名） | `aijia/staging/release/v0.5.22/` |

**Windows 本地签名**：

```powershell
python scripts/release.py sign-release
# 或直接：.\scripts\sign-windows.ps1 -Version 0.5.22 -ReleaseType release
```

### Step 4: Finalize（生成 update.json，用户收到自动更新）

```bash
python scripts/release.py finalize
# → 触发 GitHub Actions "Finalize Release" workflow
# → 生成 update.json → 已安装用户收到更新推送
```

### Step 5: 收尾

```bash
# macOS Intel（x86_64）补打（可选，视用户群体决定）
# 详见下方 "macOS Intel 补充构建" 章节

# Homebrew 更新
python3 scripts/bump-homebrew.py 0.5.22

# Changelog 更新（lotus 仓库）
cd ../lotus && ./scripts/update-changelog.sh desktop 0.5.22

# 推到国内镜像
git push codeup main && git push codeup v0.5.22
```

### Windows 签名机环境要求

| 项目 | 说明 |
|------|------|
| Windows SDK | 含 `signtool.exe` |
| 代码签名证书 | 已导入本地证书存储 |
| Python + oss2 | `pip install oss2` |
| Node.js + pnpm | 用于 `npx tauri signer sign`（重新生成 .sig） |

**环境变量**（建议写入系统环境变量，持久化）：

```
SIGN_CERT_THUMBPRINT=<证书指纹>
TAURI_SIGNING_PRIVATE_KEY=<Tauri updater 私钥 base64>
TAURI_SIGNING_PRIVATE_KEY_PASSWORD=<私钥密码>
OSS_ACCESS_KEY_ID=<阿里云 OSS AK>
OSS_ACCESS_KEY_SECRET=<阿里云 OSS SK>
```

### OSS 路径规范

```
aijia/
├── staging/                     # CI 上传的未签名 Windows exe（临时）
│   ├── beta/v0.5.22/
│   └── release/v0.5.22/
├── beta/                        # Beta 测试版（已签名，不触发自动更新）
│   └── v0.5.22/
│       ├── AIjia_0.5.22_x64-setup.exe      # Windows（已签名）
│       ├── AIjia_0.5.22_x64-setup.exe.sig  # Tauri updater sig
│       ├── AIjia_0.5.22_aarch64.dmg        # macOS
│       └── AIjia.app.tar.gz + .sig         # macOS updater
├── v0.5.22/                     # 正式版（已签名）
│   ├── AIjia_0.5.22_x64-setup.exe
│   ├── AIjia_0.5.22_x64-setup.exe.sig
│   ├── AIjia_0.5.22_aarch64.dmg
│   └── AIjia.app.tar.gz + .sig
├── latest/                      # 正式版最新下载入口
│   ├── windows-x64
│   └── macos-arm64
└── update.json                  # Tauri 自动更新清单（仅正式版）
```

### CI Workflows

| Workflow | 触发 | 作用 |
|----------|------|------|
| `build-desktop.yml` | `beta-v*` tag / `v*` tag / manual | 构建 + macOS 上传 + Windows 未签名上传到 staging |
| `finalize-release.yml` | manual（输入 version） | Windows 签名完成后，生成 update.json |
| `ci.yml` | push main / PR | Rust + TS 类型检查 + lint |

### macOS Intel（x86_64）— 补充构建

CI matrix 只覆盖 Windows + macOS arm64。**Intel 包需本地补打**：

```bash
rustup target add x86_64-apple-darwin             # 首次

# 跨架构准备 sidecar 资源
rm -rf src-tauri/playwright-runtime src-tauri/resources/dws
TARGET_ARCH=x86_64 bash scripts/setup-playwright.sh
TARGET_ARCH=x86_64 bash scripts/setup-dws.sh

# 签名密钥
export TAURI_SIGNING_PRIVATE_KEY="$(cat ~/.tauri/aijia.key)"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="$(security find-generic-password -s aijia-tauri-signer -a password -w)"

pnpm tauri build --target x86_64-apple-darwin     # 5–10 分钟
python3 scripts/upload-x64.py 0.5.22              # 双传 OSS + GitHub Release
```

**顺序约束**：必须等 `finalize-release.yml` 跑完再跑 `upload-x64.py`，否则 update.json 会丢失 `darwin-x86_64` 条目。

**跨架构 sidecar 注意**：`pnpm tauri build --target` 只重编 Rust 主二进制，sidecar 资源原样拷贝。必须用 `TARGET_ARCH=x86_64` 重新运行 setup 脚本。验证：

```bash
APP=src-tauri/target/x86_64-apple-darwin/release/bundle/macos/AIjia.app
file "$APP/Contents/MacOS/aijia"                                   # x86_64 ✅
file "$APP/Contents/Resources/dws"                                 # x86_64 ✅
file "$APP/Contents/Resources/playwright-runtime/node/bin/node"    # x86_64 ✅
```

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
- GitHub Secrets（4 个）：`TAURI_SIGNING_PRIVATE_KEY`, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`, `OSS_ACCESS_KEY_ID`, `OSS_ACCESS_KEY_SECRET`
- 签名密钥（本地）：`~/.tauri/aijia.key` + Keychain `aijia-tauri-signer`
- OSS 凭证（本地）：Keychain `aijia-oss`，或环境变量 `OSS_ACCESS_KEY_ID` / `OSS_ACCESS_KEY_SECRET`
- **数字员工 SKILL bundle + ResourceConfig + 派活前置补全（2026-05）**：5 个内置员工各带 `defaultSkillId / requiresAttachment / resourceConfigKind / requiresDingtalk` 元数据（`src/features/employees/templates.ts`）。派活前 `runTriggerPrechecks` 决定是否弹文件 picker / 资源 form / 钉钉 alert，因此**派活的唯一入口是 EmployeeDrawer 底部按钮**（卡片点击只打开 Drawer，不再有 inline 派活按钮）。SKILL 内容（`competitive-intelligence`, `sales-followup-rules`）以 managed global skills bundle 分发到 `~/.renlijia/skills/`。dispatch prompt 强制末尾"请立即开始按职责执行"以避免 LLM 等用户指示。详见 `../docs/plans/2026-05-05-employee-skills-and-resources-design.md`。
- **数字员工模板服务化（2026-05-10）**：模板从 `src/features/employees/templates.ts` 硬编码常量演进为 OPS 后台管理的版本化资源。新模块 `runtime/employee/template_store.rs` 持有 `TemplateRef` / `TemplateSnapshot` / `TemplateManifest` 类型和 `ensure_instance_snapshot()` 写盘逻辑（原子写 + sha256 校验 + 幂等）。11 个内置模板的 v1.0.0 JSON 通过 `include_str!("templates_bootstrap.json")` 编译进 binary（`OnceLock` 缓存解析结果）做 bootstrap fallback，离线雇佣可用。每个员工实例 `users/{scope}/employees/{id}/` 下新增 `template/{template.json, manifest.json}` 子目录存放雇佣时冷冻的模板快照——这是后续运行时合并视图的来源。`EmployeeRecord` 加 `template_ref: Option<TemplateRef>`（`#[serde(default)]`，零破坏），`EmployeeStore::create / get / list_unlocked` 自动调用 `stamp_snapshot_for_record`：雇佣时按 `template_id` 匹配 bootstrap 拷快照、老 record 下次读取时自动补全 `template_ref` + 写出 snapshot（一次性原地迁移，无需启动迁移 hook）。新 Tauri 命令 `employee_template_catalog` 返回 bootstrap 列表（前端 `EmployeeTemplateSnapshot` 类型 + `employeeTemplateCatalog()` API）。**架构位移已完成**：模板成为 source of truth，record 旧字段（`tool_whitelist / system_prompt_extra / default_skill_id / role / description / avatar`）从"独立配置源"变成"雇佣时从 snapshot 派生的冷冻副本"——但**字段未物理删除**，dispatch / runner / chat 仍读 record 字段。后续 PR：HTTP loader（拉 lotus `/api/public/employee-templates/:tid/manifest` + sha256 校验 + `~/.renlijia/employee-templates-cache/` LRU）、HireWizard 切换到 catalog API、运行时改读 snapshot + 删除 record 冗余字段、schema-driven form 替换 `ResourceConfigKind` 闭合枚举。Spec：`lotus/docs/superpowers/specs/2026-05-10-employee-templates-as-a-service.md` §12。
- **数字员工状态机（v0.5.7）**：`EmployeeRecord.enabled: bool` 拆为 `lifecycle: EmployeeLifecycle`（Active / Paused / Archived）+ `cron_enabled: bool` 二维独立字段。旧文件通过 `#[serde(alias = "enabled")]` 自动迁移到 `cron_enabled`。运行态来自后端 `EmployeeActiveRuns` Mutex<HashMap>（不再用前端"10 分钟内 Running"启发式），由 `ActiveRunGuard` RAII Drop 守卫保证 panic / cancel / 提前 return 都不会泄漏。删除 = 软删除（lifecycle=Archived），调度器每 60s tick 自动 `purge_old_archived(7d)`，per-record 检查走 race-safe `purge_if_archived_older_than`（同一把锁内 re-check lifecycle + age，防止"用户刚恢复就被 purge"）。新增 Tauri 命令：`employee_active_run` / `employee_stop_run` / `employee_restore` / `employee_purge`。前端 deriveStatus 7 状态优先级：archived > running > has-report > paused > needs-setup > scheduled > idle。详见 `../docs/plans/2026-05-06-employee-state-machine-IMPLEMENTATION.md`。
- **Windows 兼容性约定（v0.5.7）**：所有 git 子进程必须传 `-c core.quotepath=false`（中文文件名展示）；用户可编辑 JSON 文件（mcp_config / global_config）的读路径走 `storage::text_io::read_to_string_strip_bom`（剥 Win10 Notepad BOM）；外部 CLI 输出（dws / where.exe / tasklist）解码走 `storage::console_decode::decode_console_bytes`（Windows GBK fallback，靠 `encoding_rs`）；MCP 子进程 spawn 时强制 `PYTHONIOENCODING=utf-8` / `PYTHONUTF8=1` / `LANG=en_US.UTF-8`；hooks runner + skill `!cmd` 替换在 Windows 走 `powershell.exe -NoProfile -Command`（不能裸 `sh -c`）；用户/LLM 提供的文件名走 `storage::safe_filename::ensure_safe_filename`（CON/PRN/COM*/LPT* 保留名 + 禁字符 `<>:"\|?*` + 尾部 `.`/空格 + 长度 ≤ 200）；任何写到磁盘的状态文件优先 tmp + rename 原子写（参考 `runtime::employee::store::write_atomic`），目录删除走 `remove_dir_all_retry` 3×150–300ms backoff。
- **Windows 子进程黑窗抑制（v0.5.8）**：所有 `Command::spawn` / `.output()` 必须先调 `.no_window()`（`storage::process_ext::NoWindowExt` trait extension），它在 Windows 上注入 `CREATE_NO_WINDOW = 0x08000000` 创建标志，其它平台是 no-op。漏一个就在 Windows 上看到 cmd.exe / conhost.exe 一闪而过。
- **max_tokens 按模型自动选（v0.5.8）**：默认输出预算从写死的 4096/100000 改成 `llm::max_tokens::default_max_tokens_for_model(name)` 启发式查表（DeepSeek V4 = 384k、GPT-5 = 128k、GLM-4.5/4.6 = 96k、Claude Sonnet/Opus + Gemini-2 = 64k、Qwen3 / qwen-max-2025 = 16k、Qwen-max-longcontext = 30k、其他兜底 8192）。新增上限按模型加进表里。从 `chat_turn_driver` 调（`llm_settings.primary_model` 在 scope 内）；想覆盖的���用方传 `Some(value)` 即可，否则传 `None` 让 driver 走 per-model 默认。
- **跨平台拖拽上传（v0.5.9）**：Tauri 2 webview 拦截 HTML5 drop 事件，React `onDrop` 永不触发；唯一靠谱的入口是 `getCurrentWebview().onDragDropEvent`。`useDragDropListener`（在 `App` 顶层 mount 一次）订阅 native 事件，把 resolved `PendingAttachment[]` push 进 `useDropInbox`（zustand pull queue），HomeTaskComposerCard / ChatBottomArea 各自 useEffect drain。新增附件路径校验（`useChatAttachments::isAcceptablePastedPath` / `makePendingAttachment` / `resolvePastedPaths`）必须支持 `[\\/]` 双分隔符 + Windows `C:\` 卷根 + 系统目录前缀拒绝。
- **剪贴板图片粘贴**：`useComposerPaste.handlePaste` 支持截图/复制图片粘贴。流程：同步提取 `clipboardData.items` 中的 image blob（异步后 clipboardData 会被浏览器清空）→ 先尝试 native file paths（Finder 复制文件）→ 路径为空时 fallback 到 image blob → `saveClipboardImage()` 写入 `tmpImage/` → 添加为 `PendingAttachment`。`saveClipboardImage` 从 `useChatAttachments` 传入（Tauri IPC → Rust `save_clipboard_image_to_tmp`）。ChatBottomArea + HomeTaskComposerCard 两个 composer 均已接入。
- **租户运行时换肤（C 路径，2026-05-09 重做）**：tenant 4 色（`accentColor` / `primaryColor` / `bgColor` / `sidebarBgColor`）+ `productName` + `logoUrl` + `fontFamily` 由 lotus 后台（`tenant-portal/web/src/pages/settings/branding.tsx`，10 个 PRESET_THEMES）下发；后端 `auth/state.rs::TenantInfo` 透传，前端 `src/stores/brandingStore.ts::applyBranding` 派生 ~40 个 CSS 变量（design.pen + legacy `--color-*` 双套命名空间）。派生算法在 `src/lib/themeUtils.ts`：accent 用 lighten/darken/rgba 派生 hover/subtle/-50/-100/...；primary 决定 `--foreground` 和文字色；bg/sidebar 用 `mixColors(bg, fg, ratio)` 派生 muted/border/popover。`isDarkColor` 决定 `--*-foreground` 用 `#FFFFFF` 还是 `#1A1A1A`，所以 dark mode 不需要 `.dark` selector，直接 4 色填深色即可。`authStore` 在 `restoreFromStorage / login` 后调 `applyTenantBranding(info)`，`logout` 时 `useBrandingStore.reset()`。**前端不再提供本地 accent 选择器**（之前 `GeneralPanel` 的强调色 swatch 已删除）——皮肤完全由 lotus 后台定，避免"用户本地选择 vs 租户下发"的覆盖冲突。`localStorage` 里历史残留的 `aijia-accent-color` 和后端 settings 里的 `accentColor` 字段都不再被读取。
- **标题栏 = accent 色（2026-05-09）**：mac Overlay 区 + Windows 自绘标题栏统一用 `bg-primary text-primary-foreground` 显示租户 accent，是用户感知最强的换肤点。`App.tsx` 顶层 wrapper 用 `bg-background`（不再是 `bg-sidebar`），让租户 bgColor 一改 wrapper 就跟着变。Windows 标题栏拖拽踩坑：① `flex-1` 占位 div 必须有 `data-tauri-drag-region`，吃掉中间空白区域的拖拽；② `WindowControls` 外层 `onMouseDown stopPropagation`，否则点击关闭按钮会被父级拖拽吞；③ `UpdateAvailableLink` 也必须包 `stopPropagation` 容器，否则点击更新链接没反应（0.3.x 历史 bug）。mac Overlay 拖拽由系统提供，**不要**额外绑 `onMouseDown=startDragging` 否则跟系统冲突。Windows 标题栏底部用 `border-b border-primary-foreground/15` 做半透明分隔线，避免 accent 与 background 色差小时糊在一起。

