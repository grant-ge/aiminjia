# AIjia — 代码仓库

企业 AI 工作台 Tauri 2.x 桌面应用（React/TS 前端 + Rust 后端 + RuntimeManager 管理的本地运行环境）。

产品名：**AIjia**（元数据/文件名）/ **AI小家**（UI 面向用户）。标识符：`com.aijia.app`。

设计文档统一在 `~/lotus/docs/desktop/`。架构权威在 `~/lotus/docs/desktop/agent-architecture.md`。

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
│   ├── release.py                  # 发布入口（交互式，跨平台，强制顺序）
│   ├── bump-version.py             # 同步版本号到 3 个配置文件（跨平台）
│   ├── bump-version.sh             # 同步版本号（macOS 快捷方式）
│   ├── build-and-sign-macos.sh     # mac: arm64 + x86_64 串行 build+sign+notarize+upload
│   ├── sign-and-upload-macos.sh    # mac: 单架构 sign+notarize+upload（被上面调用，幂等可重跑）
│   ├── release-windows.ps1         # win: 一键 staging下载→signtool→tauri sig→OSS 上传→清理
│   ├── ci-upload-windows.mjs       # win: Node + ali-oss 上传（被 release-windows.ps1 调用）
│   ├── ci-cleanup-staging.mjs      # win: 发布后清理 OSS staging
│   ├── verify-release.sh           # mac: 发版后全量验证（OSS + 签名 + 公证 + spctl）
│   ├── sign-and-upload-windows.ps1 # 旧版 Windows 流程（保留兼容，新流程用 release-windows.ps1）
│   ├── ci-upload-macos.py          # macOS release → OSS
│   ├── ci-upload-macos-beta.py     # macOS beta → OSS
│   ├── ci-generate-download-page.py  # CI: 生成 downloads.html 下载页
│   ├── ci-finalize.py              # 生成 update.json
│   ├── setup-runner-macos.sh       # macOS 签名机环境检查
│   ├── setup-runner-windows.ps1    # Windows 签名机环境检查
│   └── bump-homebrew.py
└── .github/workflows/
    └── build-desktop.yml      # Windows-only CI：构建未签名 exe → 上传 OSS staging + 下载页生成
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
- 工具实现主体在 `llm/tool_executor/`（upload/dingtalk/search 等）
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

**3. 图标统一走图标库 `lucide-react`，禁止往 `public/` 新增 SVG 资产**

仓库已经依赖 `lucide-react`，覆盖 90% 以上常见图标场景。**新做图标不要画 SVG**——理由有三：① 自造 SVG 的 stroke / fill / opacity 不跟主题变量，dark mode 切换、租户换肤、状态色都得手 patch；② Tauri webview 对 SVG 比 Chrome 严格（XML 注释 `--` 会被拒、缺 `width`/`height` intrinsic size 不渲染），每加一个 svg 资产就是一个潜在踩坑点；③ 静态资源会进 bundle，安装包/启动时间持续膨胀。

- ✅ 用 lucide：`import { User, Search, X } from 'lucide-react'`，外层包圆 `bg-primary/12 rounded-full text-primary p-[15%]` 就能达到"中性头像 + 跟主题色"的视觉，跟着 `--primary` 自动换肤
- ✅ lucide 没有的图标 → 先在 lucide 全集 (https://lucide.dev/icons) 搜近似语义；再不行用 `@radix-ui/react-icons` 的字符 / 系统图标作为补充
- ❌ 禁止往 `public/` 加新的 SVG / PNG / 图标资产仅为了"做一个图"
- ❌ 禁止自己写 `<svg viewBox=... ><path .../></svg>` inline 图形（除非是项目主 logo / 启动闪屏这种**唯一身份性**资产）
- 例外：纯结构性图形（QR code pattern、charts / diagrams、严格指定的品牌 logo）保留 svg/图形代码即可，但要写注释说明为何不能用图标库
- **图标颜色必须由 `currentColor` + 父级 `text-*` 变量驱动**，禁止给 lucide 图标传 `color="#xxx"` 或 `stroke="#xxx"`。常见错误：图标变全黑（默认 `text-foreground`）——99% 是缺一层 `text-primary` / `text-muted-foreground` / `text-destructive` 父容器，**不是**要给图标加硬编码 fill

**4. 阴影必须使用全局变量，禁止硬编码 `boxShadow` 字面量**

- ✅ 用 `src/styles/globals.css` 中定义的语义变量：`--shadow-input` / `--shadow-md` / `--shadow-popover` / `--shadow-modal` / `--shadow-card`
- ✅ Tailwind 任意值语法：`shadow-[var(--shadow-popover)]`；内联 style：`boxShadow: 'var(--shadow-popover)'`
- ❌ 禁止 `style={{ boxShadow: '0 4px 12px -4px rgba(0,0,0,0.08)' }}` 之类硬编码字面量
- 选择依据：input 边框光晕 → `input`；卡片/Toast → `card` / `md`；下拉/popover/气泡/浮层 → `popover`；Modal/Dialog → `modal`
- 新增一类浮层需要新阴影 → 先在 `globals.css` 里加 `--shadow-*` 变量再用，**不要先硬编码再"以后改"**

**5. Store / 消息更新必须 immutable，禁止原地 mutate（保持 React.memo 命中）**

`AiBubble` 套了 `React.memo` 避免长对话历史消息重渲（`src/components/chat/AiBubble.tsx`），memo 默认浅比较 props。**只要任何代码原地 mutate `message` 或 `message.content`，UI 就会停止刷新**（memo 拦下重渲）。

- ✅ 走 `chatStore` / `sessionStore` 的 mutate 方法：`updateMessage` / `upsertMessage` / `setMessages`，它们用 `{...m, ...updates}` 和 `[...arr]` 生成新引用
- ✅ 自己组装新对象：`store.upsertMessage({ ...oldMsg, content: { ...oldMsg.content, text: newText } })`
- ❌ 禁止 `msg.content.text += delta`、`msg.content.tables.push(...)`、`messages[i] = ...`、`Object.assign(msg, ...)` 之类原地写
- ❌ 禁止把 store state 中的 message 对象传出去再 mutate（即便后续 `setMessages([...])` 也已经被 memo 跳过）
- 流式增量内容**不要**写进 `messages` 数组的 message 对象——专门走独立的 `streamingContent` state（由 `StreamingBubble` 渲染，未套 memo），stream 完成时再 `upsertMessage` 一次落到 messages
- 出现"bubble 不刷新"现象时，第一反应去查 store 路径是否有人开始原地改 content，而不是怀疑 memo
- 测试契约：`src/components/chat/__tests__/AiBubble.memo.test.tsx` 钉死 4 条不变式（同引用跳、新引用渲、isStreaming 变化渲、原地 mutate 跳），添加新的 message mutate 路径时同步加测试

**Code review 自查清单**：diff 里看到 `bg-black` / `text-white` / `text-[#` / `bg-[#` / `border-white` / 没带颜色的裸 `border-b`、看到自己手写顶栏 / 按钮样式而不是用组件、看到 `public/*.svg` 新增图标资产或 inline 自写 `<svg>` 图形（除主 logo / 结构性图形外）、看到给 lucide 图标硬编码 `color="#"` / `stroke="#"` 或图标视觉全黑没绑 `text-*` 变量、看到 `boxShadow: '0 ... rgba(...)'` 硬编码字面量、看到对 store 中的 message / content 原地赋值或 push / Object.assign / `msg.foo = ...`——立刻换成变量 / 公共组件 / 图标库 / immutable 写法。

## 存储结构

所有运行时数据持久化到 `~/.renlijia/`（`AiJiaHome::from_home()`），不再使用 Tauri app data dir（后者仅用于启动时一次性迁移）：

```
~/.renlijia/
├── users/{scope}/conversations/{id}/  # per-team 布局（scope = t_{tenantId}__u_{userId}）
│   ├── conv.json                  # 对话元数据（含 isArchived / title / messageCount）
│   ├── conv.json.bak              # 原子写时的备份
│   ├── messages.jsonl             # 消息流水（单文件 ndjson，每行末尾带 `\t✓` 校验位）
│   ├── compact_boundaries.jsonl   # 压缩边界记录
│   ├── file_index.json            # 文件索引
│   ├── generated/                 # LLM 生成产物（报告、图表等）
│   ├── notes/                     # 长文笔记
│   └── uploads/                   # 上传文件副本
├── conversations/{id}/            # 老布局（迁移前会话；新会话不再落到这里）
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

- **数字员工配置表单 = 软校验（2026-05-14）**：所有内置员工的 hand-tuned 配置表单（`SalesTableConfigForm` / `CustomerSupportConfigForm` / `TechSupportConfigForm` / `MonitoringUrlsForm`）保存按钮**不再因必填项空缺而禁用**。设计原则：配置表单是"hint"而不是"gate"——空值或部分值都允许保存，员工在派活后的第一次对话中通过 dws 列字段、问用户子表名、列群关键字等方式补全。`SalesTableConfigForm.parseDingtalkAitableUrl` 现在 baseId 解析到就接受（sheetId 缺失时 tableId 留空），UI 提示"未包含 sheetId，小销会在对话中确认"。仅保留**格式错误**校验（如 fieldMapping JSON 解析失败），不再阻断保存空值。`MonitoringUrlsForm` 删除 URL 格式校验和"至少一行非空 name"校验；保存时过滤掉完全空的行。`SchemaForm` 的 schema-driven 校验**保留**，因为它是给未来自定义模板用的，校验由模板作者通过 JSON Schema 控制，不是产品默认行为。

1. **Tauri command 层只做参数接收 → 转发 Runtime**，不含业务逻辑（见 `docs/architecture-blueprint.md`）
2. **不接受只改 prompt（base.md/daily.md）来修复能力问题**；能力边界应由 runtime/tool/capability/sandbox 保证
3. **新工具应实现 `RuntimeTool` trait**，不应新增 `ToolPlugin` 实现
4. **`CapabilityContext`（`runtime/tools/capability.rs`）是工具获取系统能力的窄接口**，不应扩大它来传入 `LlmGateway`、`AuthManager` 等编排层对象
5. **LLM 协议：生产路径走 Anthropic**（`lotus.rs` → `claude.rs`，直通 `/anthropic/v1/messages`）。`openai.rs` 仅供 `custom.rs::send_openai_compat` 复用 OpenAI 兼容协议；新增 LLM 能力默认按 Anthropic 协议设计。Prompt renderer 用协议中性命名 `ChatPromptRenderer` / `system_message`，**禁止再加 `openai_` 前缀**。System prompt 多段缓存通过 `SystemPromptSegment` + `stream_message_with_segments` 入口透传；system 侧 `cache_control: ephemeral` 上限 3 块（总额度 4，预留 1 给 tools）。详见 `docs/superpowers/specs/2026-05-10-anthropic-protocol-migration-cleanup.md`
6. **Token / Cost 统计必须包含 Anthropic cache 字段**：`TokenUsage` / `TotalTokenUsage` 有 `cache_creation_input_tokens` / `cache_read_input_tokens`；`estimated_cost_usd` 按 1.25× / 0.1× 加权。新增涉及 token 的事件 / 日志字段，必须同步透传到 `TurnCompleted` 与前端 TS 类型，并保留 `#[serde(default)]` 兼容老会话

## 进行中的架构专项

1. **Workspace-First 文件能力模型**
2. **Atomic Tool 工具体系**
3. **Prompt Slimming 提示词职责回收**
4. **Skill 本地导入/打包导入模型统一**

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

## 发布流程（权威 · 自 v0.5.23 起，本地构建 + 一键 Windows）

**架构：macOS 本地全流程 + Windows GitHub-hosted 构建未签名包 + 本地一键签名上传**。

- **macOS arm64 + x86_64**：在你的 Mac 上 `build-and-sign-macos.sh` 串行 build → codesign → notarize → staple → tauri signer → 上传 OSS。串行的原因是 `setup-dws.sh` 切换 runtime symlink，并发会冲突。
- **Windows x64**：tag 触发 `windows-latest` runner 跑 unsigned 构建 → 产物上传到 OSS staging（`aijia/staging/unsigned/v{ver}/`，公开 CDN URL）→ 在你的 Windows 机器上跑 `release-windows.ps1` 一键完成：拉 staging exe → signtool 签名（带 timestamp）→ tauri signer 生成 `.sig` → Node + `ali-oss` 上传 → 清理 staging。

### 内置运行时（自 0.5.24 起）

发版前**必须**先跑 `scripts/prepare-bundled-runtime.{sh,ps1}`，把 Node 20.18 / Python 3.12.7 / uv 0.4.27 打入 `src-tauri/resources/runtime/<platform>/`。Tauri build 把目录复制进安装包（~85MB 增量），用户首启完全离线可用。

- **入口脚本**：mac 走 `scripts/build-and-sign-macos.sh` 内部自动按 arch 调 `prepare-bundled-runtime.sh PLATFORM=darwin-{arm64,x64}`；Windows CI `.github/workflows/build-desktop.yml` 的 build job 自动跑 `prepare-bundled-runtime.ps1`。
- **上游源**：`scripts/runtime-sources.json` pin 了 Node（nodejs.org）、Python（mac 用 python-build-standalone install_only，win 用官方 embeddable zip）、uv（astral-sh release）的 URL + sha256。升级运行时改 `bundleVersion` + 各组件 version + 9 个 sha256。
- **缓存**：`.runtime-cache/`（已 gitignore）按文件名缓存下载产物，CI 也 cache 这个目录（`actions/cache@v4` key on `hashFiles('scripts/runtime-sources.json')`）。
- **解析链**：启动期 `BundledRuntimeResolver`（reads `app.path().resource_dir()/runtime/<platform>/`）→ `InstalledRuntimeResolver`（`~/.renlijia/runtimes/renlijia-primary-runtime/current`，OSS 升级路径）→ on-demand OSS download（兜底）。前两个任一成功即跳过 OSS。详见 `src-tauri/src/runtime/dependencies/{bundled_resolver,chain_resolver,manager}.rs`。
- **mac 签名**：现有 inside-out `find -type f` + `file ... Mach-O` 自动覆盖 `Contents/Resources/runtime/` 下所有二进制（node/python3/uv/uvx/libpython3.12.dylib + lib-dynload `.so`）。`sign-and-upload-macos.sh` 在签名后**审计** runtime/ 下每个 Mach-O 是否带 `flags=...runtime`（hardened），漏一个则 fail，防止 notarization 拒签。
- **Windows 签名**：`release-windows.ps1` 只签外层 NSIS 安装包，**不**单独签 `resources\runtime\` 内嵌的 `node.exe / python.exe / uv.exe / uvx.exe`。SmartScreen 只校验外层签名，nested PE 不展示在用户首次启动的警告里——因此当前是有意 trade-off（每个 nested exe 单独走 signtool + timestamp 会增加 ~30s 发版时间）。如果将来用户报告 "Unknown publisher" 提示在 PowerShell 直接执行内嵌 exe 时出现，再补 signtool 遍历。
- **诊断**：Settings → 运行时 显示 `activeResolver`、内置版本号、`node/python/uv --version` 实时输出 + 一键重检（`runtime_diagnostics` Tauri 命令 → `src/components/settings/panels/RuntimePanel.tsx`）。
- **Spec/Plan**：`docs/superpowers/plans/2026-05-13-bundle-runtime-into-installer.md`。

### 三种包

| 类型 | 签名 | 来源 | 用途 |
|------|------|------|------|
| **Beta** | 已签名 | 本地签名后上传到 `aijia/beta/v{x}/` | 内测验证，不触发自动更新 |
| **Release** | 已签名 | 本地签名后上传到 `aijia/v{x}/` + `latest/` | 正式版，触发自动更新 |
| ~~Dev~~ | ~~未签名~~ | ~~已弃用~~ | ~~CI 已不再产出 dev 包~~ |

**下载页**：https://lotus.renlijia.com/aijia/downloads.html

### 工作流（一次性命令）

```bash
# 1. 启动版本（在 mac 上）
python3 scripts/release.py start              # bump base 版本号（X.Y.Z）
python3 scripts/release.py beta               # bump 到 X.Y.Z-beta.N + 推 tag → 触发 Windows CI

# 2. macOS（本地，串行 arm64 → x86_64，~35min）
bash scripts/build-and-sign-macos.sh X.Y.Z-beta.N beta
# 单架构重跑（其中一边失败时）：ARCH=x86_64 bash scripts/build-and-sign-macos.sh ...

# 3. Windows（在 Windows 机器上，一条命令，首次问 4 个值之后全自动）
.\scripts\release-windows.ps1 -Version X.Y.Z-beta.N -Type beta

# 4. 验证（mac 上跑，~30s）
bash scripts/verify-release.sh X.Y.Z-beta.N beta

# 5. 测试通过后正式发布（同上，beta 改为 release）
python3 scripts/release.py test-passed
python3 scripts/release.py release            # bump 回 X.Y.Z + 推 release tag
bash scripts/build-and-sign-macos.sh X.Y.Z release
.\scripts\release-windows.ps1 -Version X.Y.Z -Type release
bash scripts/verify-release.sh X.Y.Z release
python3 scripts/release.py finalize           # 生成 update.json → 自动更新生效
```

### 关键脚本

| 脚本 | 平台 | 职责 |
|------|------|------|
| `scripts/release.py` | mac | 交互式流程守卫：bump 版本 / 推 tag / 强制顺序 / finalize |
| `scripts/build-and-sign-macos.sh` | mac | macOS 串行 arm64 → x86_64：build + sign + notarize + upload，幂等可重跑 |
| `scripts/sign-and-upload-macos.sh` | mac | 单架构 sign + notarize + upload（被上面脚本调用，也可单独跑） |
| `scripts/release-windows.ps1` | win | Windows 一键流程：staging 下载 → signtool 签 → tauri sig → ali-oss 上传 → 清理 |
| `scripts/ci-upload-windows.mjs` | win | Node + ali-oss 上传（替代旧 Python 版） |
| `scripts/ci-cleanup-staging.mjs` | win | 发布后删除 OSS staging 文件 |
| `scripts/verify-release.sh` | mac | 发版后全量验证（OSS 可达性 + 签名 + 公证 + spctl） |

### macOS 签名脚本的关键点

1. **inside-out 逐文件 codesign**：`--deep` 在 macOS 11+ 不可靠，会漏签 `Contents/Resources/dws` 等嵌套二进制。脚本对每个 Mach-O 二进制独立签，每个都带 `--timestamp --options runtime`。
2. **DMG 必须由脚本构造（自 v0.5.25 起）**：tauri.conf.json `bundle.targets` 已去掉 `dmg`，只保留 `["nsis", "app"]`。理由：tauri 2.x 的 `bundle_dmg.sh` 调用约定与系统装的 Homebrew `create-dmg` 1.2.3 不匹配，每次发版都失败。`sign-and-upload-macos.sh` 的 Step 1b 用 `hdiutil create -volname "AIjia $VERSION" -fs HFS+ -format UDZO` 从签好的 .app + `/Applications` symlink 构造 DMG，再 codesign 一次。三个参数都必须显式：① volname 带版本号避冲突；② **`-fs HFS+`** —— APFS DMG 头部会浪费 ~30MB 元数据（v0.5.24 因为漏了这个参数，DMG 从 92MB 涨到 122MB）；③ `-format UDZO` zlib 压缩。脚本入口前置自动清理 `/Volumes/AIjia*` 残留挂载和 `hdiutil info` 里的 aijia images，防止"Operation not permitted"。
3. **并行 notarize（自 v0.5.25 起）**：DMG 和 .app 同时提交 Apple notary，每个独立 60min timeout，submission id + rc 写盘后台监控；任一失败保留 tmp dir 调试。串行版（v0.5.24）总耗时被 Apple 端排队叠加；并行版接近单次延迟。**自重试（自 v0.5.25-beta.3 起）**：notarize 函数包了 3 次重试循环，仅当 log 含 `abortedUpload|connectTimeout|HTTPClient|connection.*reset|EOF` 这类瞬时网络错误时才重试，Apple 业务拒绝（`Invalid`/`Rejected`）立即 fail。两次重试间隔 30s。背景：Apple notary 把 zip/dmg 上传到 S3 `notary-submissions-prod` bucket 偶尔会 abort，重试基本就好。
3. **幂等检测**：每步前先 probe（`codesign -dv` 看 `flags=runtime` + `Authority=Developer ID Application`；`xcrun stapler validate` 看是否已 stapled）。重跑只跑没完成的步骤。
4. **签名前 .app 版本预检（v0.5.24+）**：codesign 前用 `PlistBuddy` 读 `CFBundleShortVersionString` 和传入 `$VERSION` 比对（兼容 `-beta.N` 后缀去除），不一致 fail 并提示 `CLEAN_BUILD=1 bash scripts/build-and-sign-macos.sh ...`。同时挂载 DMG 抽查内副本版本，不一致只 warn（Step 2 会重建 DMG，非致命）。防止上一次失败的 build 在 `target/` 留下旧 .app + 幂等 probe 跳过签名 → 把上版本当新版本签上去发出去。`build-and-sign-macos.sh` 提供 `CLEAN_BUILD=1` 环境变量入口跑 `cargo clean`。
5. **手动重启 sign-and-upload 注意事项**：通过 `build-and-sign-macos.sh` 串行调用是最可靠路径；如果需要在中间步骤之后单独重跑 `sign-and-upload-macos.sh`（例如 notarize 中断后接力 staple），**必须前台跑或用 `caffeinate -is bash scripts/sign-and-upload-macos.sh ...`**。`nohup ... &` + `disown` 在 macOS 下父 bash 退出仍可能 SIGHUP 杀子进程���曾在 v0.5.24 发版时导致 .app notarize 完成后脚本死掉、需手动接力 staple/sig/upload。

### Windows 一键脚本的关键点

1. **零 Python 依赖**：用 Node + `ali-oss` SDK 上传 OSS。避开了 Windows 上 Microsoft Store python.exe stub 的常见坑（exit 9009）。
2. **凭据持久化**：4 个值（cert thumbprint / OSS key id / OSS secret / tauri key 密码）首次输入后存进 Windows Credential Manager（cmdkey + Win32 CredRead），之后只需要传 `-Version` + `-Type`。`-Reconfigure` 重新输入。
3. **signtool 自动化**：脚本自动拼对的命令 `signtool sign /v /fd sha256 /sha1 <thumbprint> /tr <timestamp-url> /td sha256 <exe>`。不会再漏 `/tr` 导致无 timestamp 签名。
4. **Tauri key 走文件路径**：`tauri signer sign -k <file>`，不走环境变量（避免 PowerShell 传 base64 给子进程时引入空白字符）。
5. **公开 staging URL**：CI 上传到 `aijia/staging/unsigned/v{ver}/`，CDN 公开可下载（不需要 GitHub token / gh CLI），任何 Windows 机器只要 `git pull` 就能跑发版。

### 签名机环境要求

**macOS**（检查：`bash scripts/setup-runner-macos.sh`）：
- Developer ID Application 证书已导入 login keychain
- 环境变量：`APPLE_ID`, `APPLE_PASSWORD`（App-Specific Password）, `APPLE_TEAM_ID`
- 环境变量：`TAURI_SIGNING_PRIVATE_KEY`, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`
- 环境变量：`OSS_ACCESS_KEY_ID`, `OSS_ACCESS_KEY_SECRET`
- 推荐：把上面 7 个变量集中写到 `.env.local.aijia`（已加 .gitignore，chmod 600），构建脚本 `source` 一下

**Windows**（检查：`.\scripts\setup-runner-windows.ps1`）：
- SimpleSign + EV 硬件 token（GUI 签名工具）或 signtool.exe（Windows SDK）
- Node.js（项目本来就要）
- `$HOME\.tauri\aijia.key`（从 macOS 机器拷过来）
- 首次跑 `release-windows.ps1` 时交互式存入 Credential Manager

### GitHub Secrets（CI 构建用，仅 Windows CI 需要）

| Secret | 用途 |
|--------|------|
| `TAURI_SIGNING_PRIVATE_KEY` | Tauri updater Ed25519 密钥 |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | Ed25519 密钥密码 |
| `OSS_ACCESS_KEY_ID` | 阿里云 OSS（staging 上传 + 下载页生成） |
| `OSS_ACCESS_KEY_SECRET` | 阿里云 OSS |

注：Apple 签名 / Windows 签名相关 secrets 不再需要（签名都在本地做）。

### OSS 路径规范

```
aijia/
├── staging/unsigned/v{ver}/     # CI 上传的 Windows 未签名包（CDN 公开，签完会被清理）
├── beta/v{ver}/                 # Beta 测试版（已签名）
│   ├── AIjia_{ver}_aarch64.dmg
│   ├── AIjia_{ver}_x64.dmg
│   ├── AIjia.app.tar.gz + .sig          # ARM64 updater
│   ├── AIjia_x64.app.tar.gz + .sig      # Intel updater
│   ├── AIjia_{ver}_x64-setup.exe + .sig # Windows
├── v{ver}/                      # 正式版（已签名）
├── latest/                      # 正式版最新下载入口
├── downloads.html               # 下载页（CI 自动生成）
└── update.json                  # Tauri 自动更新清单（仅正式版）
```

### CI Workflows

| Workflow | 触发 | 作用 |
|----------|------|------|
| `build-desktop.yml` | `beta-v*` / `v*` tag / manual | **仅 Windows**：GitHub-hosted 构建未签名 exe → 上传到 OSS staging + 生成下载页 |
| `finalize-release.yml` | manual（输入 version） | 生成 update.json |
| `ci.yml` | push main / PR | Rust + TS 类型检查 + lint |

## Git Remotes

- `origin` → `github.com:grant-ge/aiminjia.git`（公开，CI 运行于此）
- `codeup` → `codeup.aliyun.com:renlijia/lotus/lotus-app.git`（国内镜像）

两个都要推。tag 只触发 `origin` 的 CI。

## 关键设计决策

- Tauri 2.x：Ed25519 signed auto-updater（密钥 `~/.tauri/aijia.key`，GitHub Secrets `TAURI_SIGNING_PRIVATE_KEY(_PASSWORD)`）
- 数据存储：文件持久化（JSON/JSONL）+ AES-256-GCM（敏感字段）
- i18n：react-i18next，`src/i18n/{zh-CN,en-US}.json`，默认 zh-CN
- OSS：阿里云 `lotus-releases` bucket，前缀 `aijia/`，CDN `https://lotus.renlijia.com`
- Homebrew：`grant-ge/homebrew-tap` 下 `Casks/aijia.rb`，`on_arm` / `on_intel` 分架构 URL
- GitHub Secrets（4 个，签名在本地做不需要更多）：`TAURI_SIGNING_PRIVATE_KEY`, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`, `OSS_ACCESS_KEY_ID`, `OSS_ACCESS_KEY_SECRET`
- 签名密钥（本地）：`~/.tauri/aijia.key` + Keychain `aijia-tauri-signer`
- OSS 凭证（本地）：Keychain `aijia-oss`，或环境变量 `OSS_ACCESS_KEY_ID` / `OSS_ACCESS_KEY_SECRET`
- **数字员工 SKILL bundle + ResourceConfig + 派活前置补全（2026-05）**：5 个内置员工各带 `defaultSkillId / requiresAttachment / resourceConfigKind / requiresDingtalk` 元数据（`src/features/employees/templates.ts`）。派活前 `runTriggerPrechecks` 决定是否弹文件 picker / 资源 form / 钉钉 alert，因此**派活的唯一入口是 EmployeeDrawer 底部按钮**（卡片点击只打开 Drawer，不再有 inline 派活按钮）。SKILL 内容（`competitive-intelligence`, `sales-followup-rules`）以 managed global skills bundle 分发到 `~/.renlijia/skills/`。dispatch prompt 强制末尾"请立即开始按职责执行"以避免 LLM 等用户指示。详见 `~/lotus/docs/desktop/superpowers/plans/2026-05-05-employee-skills-and-resources-plan.md`。
- **数字员工模板服务化（2026-05-10）**：模板从 `src/features/employees/templates.ts` 硬编码常量演进为 OPS 后台管理的版本化资源。新模块 `runtime/employee/template_store.rs` 持有 `TemplateRef` / `TemplateSnapshot` / `TemplateManifest` 类型和 `ensure_instance_snapshot()` 写盘逻辑（原子写 + sha256 校验 + 幂等）。11 个内置模板的 v1.0.0 JSON 通过 `include_str!("templates_bootstrap.json")` 编译进 binary（`OnceLock` 缓存解析结果）做 bootstrap fallback，离线雇佣可用。每个员工实例 `users/{scope}/employees/{id}/` 下新增 `template/{template.json, manifest.json}` 子目录存放雇佣时冷冻的模板快照——这是后续运行时合并视图的来源。`EmployeeRecord` 加 `template_ref: Option<TemplateRef>`（`#[serde(default)]`，零破坏），`EmployeeStore::create / get / list_unlocked` 自动调用 `stamp_snapshot_for_record`：雇佣时按 `template_id` 匹配 bootstrap → 拷快照、老 record 下次读取时自动补全 `template_ref` + 写出 snapshot（一次性原地迁移，无需启动迁移 hook）。**架构位移已完成**：模板成为 source of truth，record 旧字段（`tool_whitelist / system_prompt_extra / default_skill_id / role / description / avatar`）从"独立配置源"变成"雇佣时从 snapshot 派生的冷冻副本"——但**字段未物理删除**，dispatch / runner / chat 仍读 record 字段。Spec：`lotus/docs/superpowers/specs/2026-05-10-employee-templates-as-a-service.md` §12。
- **数字员工模板 HTTP loader（2026-05-10 / PR3）**：桌面端可从 lotus ops-portal 公共端点拉取已发布模板版本。`template_store::fetch_catalog` / `fetch_manifest` / `download_snapshot` / `ensure_cached` 走 `reqwest` 异步客户端，URL base 默认 `https://ai-ops.renlijia.com`，可通过 `LOTUS_OPS_BASE_URL` env var 覆盖（本地 dev 指 `http://localhost:8082`）。下载产物校验 sha256 后写入全局 cache `~/.renlijia/employee-templates-cache/{tid}/{version}.json`（跨用户共享，因为模板是不可变只读资源，详见 spec §5）。`AiJiaHome::employee_templates_cache_dir()` 暴露路径。新 Tauri 命令：`employee_template_refresh` 拉公共 catalog 后逐个 `ensure_cached`，单个模板失败只 log warn 不中断（partial refresh > hard fail）；`employee_template_catalog` 改为返回 `merge_catalog(bootstrap, cache)` —— 同 `template_id` 时版本字符串高者胜（cache > bootstrap if newer，反之亦然）。`stamp_snapshot_for_record` 也升级为先查 bootstrap、miss 再扫 cache，所以即便老 record 引用的是自定义 org 模板（不在 bootstrap），只要先跑过 `employee_template_refresh` 就能正确回填 `template_ref`。9 个单测覆盖：bootstrap parse / lookup / 快照写读 + 幂等 / cache 读写 / merge 优先级（cache 更新 / bootstrap 更新 / 只有 cache / cache dir 不存在）/ hex 编码。HTTP 路径**未加单测**（要 mock server，PR4 之后再补），但可以从 dev 环境手测：`LOTUS_OPS_BASE_URL=http://localhost:8082 pnpm tauri:dev` → 控制台 invoke `employee_template_refresh`。
- **HireWizard 接入后端 catalog（2026-05-10 / PR4）**：`HireWizard.tsx` 第 1 步的模板网格不再渲染前端硬编码 `BUILTIN_TEMPLATES`。`useEffect` on `open` 触发：先 fire-and-forget `employeeTemplateRefresh()`（拉最新版本到 cache，失败只 warn），再 `employeeTemplateCatalog()` 取合并后的列表（bootstrap ∪ cache），用 `snapshotToTemplate()` 映射为前端 `EmployeeTemplate` 类型后填到 `catalog` state。任何步骤失败都回退到 `BUILTIN_TEMPLATES`，保证离线或服务挂掉时雇佣流不挂。`snapshotToTemplate()` 在 `templates.ts` 新增，做三件事：① 同 `templateId` 已在 `BUILTIN_TEMPLATES` 里时**直接返回硬编码副本**（保证 v1.0.0 期间桌面端 UX 完全等价于 PR4 之前，避免微妙差异如 emoji 渲染）② 不认识的 id 按字段映射（`cron === ''` → `null`，`defaultSkillId === ''` → `null`，等）③ `resourceConfigKind` 来自 `RESOURCE_CONFIG_KIND_BY_ID` 硬编码 map，自定义 org 模板默认 `'none'`（PR6 会用 `resourceConfigSchema` JSON Schema 替代这个闭合枚举）。`findTemplate()` / `EmployeeCard` / `EmployeeDrawer` / `triggerPrechecks` 仍读 `BUILTIN_TEMPLATES`——它们查的是**老员工 record 引用的模板**，按 id 反查是稳定的（PR5 会改成读员工目录里的 `template/template.json` 快照，PR6 删 `RESOURCE_CONFIG_KIND_BY_ID`）。3 个 vitest 单测：`builtin:` 同 id 走 BUILTIN_TEMPLATES verbatim / 不认识的 id 按字段映射 / 空串 → null 的字段归一。
- **运行时 snapshot-first 读取（2026-05-10 / PR5）**：dispatch / chat 派活路径改为优先读员工目录下 `template/template.json` 快照，record 字段降级为兜底。`template_store` 新增 3 个 helper：`effective_tool_whitelist(employees_root, id, fallback)` / `effective_system_prompt_extra(...)` / `effective_default_skill_id(...)`，统一规则是"snapshot wins, falls back to record"，snapshot 文件读取失败只 log warn 不报错。`build_dispatch_prompt` 签名新增 `employees_root: Option<&Path>` 参数：`Some` 时启用 snapshot 查找（生产路径），`None` 时纯走 record 字段（13 个单测保持纯函数行为）。`chat.rs` 派活点把 `employees_dir` 解析提前到 `build_dispatch_prompt` 之前，并把 `OverrideGuard::install` 的 `tool_whitelist` 也走 `effective_tool_whitelist(employees_dir_async)`——这是 LLM 工具过滤的实际生效点。**字段未删**（PR6 才做物理删除）：仅把读取优先级换了，意味着 record 字段从"独立配置源"正式变为"snapshot 缺失时的应急兜底"。新增 4 个单测：snapshot 优先级 / 无 snapshot 回退 / snapshot 空串视为 None（不回退）/ dispatch_prompt 端到端验证 snapshot 覆盖 record。`runtime::employee::` 48 passed（+4），全仓 `cargo test --lib` 837 passed（+4，9 个 pre-existing failures 与本 PR 无关）。
- **Schema-driven 表单（2026-05-10 / PR6）**：自定义 org / private 模板的 instance config 现在可以用 JSON Schema 驱动渲染，不再依赖闭合枚举 `ResourceConfigKind` 和对应的硬编码 React 表单。新组件 `src/features/employees/forms/SchemaForm.tsx`（~340 行）支持 7 种 widget：string text / textarea (`ui:widget=textarea`) / enum select / number/integer / boolean / array-of-string (tag input) / array-of-enum (checkbox group)，附带 URL/email format / minLength/maxLength / minimum/maximum / minItems/maxItems / required 校验，错误信息在 `touched[field]` 后显示。HireWizard step 3 的路由规则：① 模板有 `resourceConfigKind !== 'none'` → 走老的 5 个 hand-tuned 表单（11 个内置员工保持原样，UX 完全等价）② 否则若 `resourceConfigSchema.properties` 非空 → 走 SchemaForm ③ 都没有 → 跳过 step 3 直接雇佣。`hasSchemaForm(template)` 是分诊函数，控制 step indicator / Next 按钮 label / 实际渲染。`EmployeeTemplate.resourceConfigSchema?: Record<string, unknown> | null` 可选字段，`snapshotToTemplate()` 从后端 snapshot 透传过来。**11 个内置模板没有 schema**（PR1 灌库时留空），所以这条路径只对未来 OPS portal 上发布的自定义模板生效——零回归风险。10 个 vitest 单测覆盖所有 widget 类型 + 校验路径 + 数字归一 + cancel 行为。Build：`tsc --noEmit` 0 errors；vitest `src/features/employees/` 42 passed（+10）。**有意未删 5 个 hardcoded form**：它们对 11 个内置员工是最优 UX（多行表格、URL 校验、知识源上传等），SchemaForm 是为新场景准备的、不替换它们。spec §10 PR7 计划之后再做最终 cleanup。
- **数字员工模板生产部署 + 集成测试（2026-05-10）**：lotus 侧上线 5 个修复后桌面端到 OSS 的下载链路真正打通。① **OSS 签名 URL**：bucket 私有，`PublicManifest` 改为返回 `bucket.SignURL` 1h 签名 URL，避免 403 ② **签名 URL host 重写**：OSS client 配置的是 `oss-cn-hangzhou-internal.aliyuncs.com`（VPC 内部），`SignURL` 继承导致桌面端走公网拉不到——`rewritePublicHost` 把签名 URL 的 scheme+host 改写为 `OSS.PublicURL` 的公网 https，path + query 保留 ③ **Publish 输出 camelCase**：Go `model.EmployeeTemplate` 用 `json:"template_id"` snake_case，Rust `TemplateSnapshot` 用 `#[serde(rename_all="camelCase")]`，桌面端反序列化静默失败（所有字段变 default）；新增 `desktopTemplateSnapshot` view-model 仅用于 publish 时渲染 OSS JSON，DB 行不动 ④ **`shared/pkg/version`** 统一两位 `MAJOR.MINOR` 版本号校验，3 个 publish handler（ops 模板 + ops skill + tenant skill）全部接入，根治"一会两位一会三位"的历史 drift ⑤ 11 个 v1.0 模板已重新发布到生产 OSS。集成测试 `src-tauri/tests/employee_template_lifecycle_test.rs` 含 10 个 case：8 个 hermetic（场景 1 第一次加载 / 场景 2 老用户迁移 / 场景 3 升级 path）+ 2 个 live `--ignored`（真打 `ai-ops.renlijia.com`，验证 manifest → 签名 URL → 下载 → camelCase 解码 + non-empty toolWhitelist 等字段，证明上面 5 个修复端到端可用）。所有 10 个 test 通过 ✅。详见 lotus spec §13。
- **数字员工状态机（v0.5.7）**：`EmployeeRecord.enabled: bool` 拆为 `lifecycle: EmployeeLifecycle`（Active / Paused / Archived）+ `cron_enabled: bool` 二维独立字段。旧文件通过 `#[serde(alias = "enabled")]` 自动迁移到 `cron_enabled`。运行态来自后端 `EmployeeActiveRuns` Mutex<HashMap>（不再用前端"10 分钟内 Running"启发式），由 `ActiveRunGuard` RAII Drop 守卫保证 panic / cancel / 提前 return 都不会泄漏。删除 = 软删除（lifecycle=Archived），调度器每 60s tick 自动 `purge_old_archived(7d)`，per-record 检查走 race-safe `purge_if_archived_older_than`（同一把锁内 re-check lifecycle + age，防止"用户刚恢复就被 purge"）。新增 Tauri 命令：`employee_active_run` / `employee_stop_run` / `employee_restore` / `employee_purge`。前端 deriveStatus 7 状态优先级：archived > running > has-report > paused > needs-setup > scheduled > idle。详见 git history（原 `docs/plans/archived/2026-05-06-employee-state-machine-IMPLEMENTATION.md`，已实现后归档删除）。
- **Windows 兼容性约定（v0.5.7）**：所有 git 子进程必须传 `-c core.quotepath=false`（中文文件名展示）；用户可编辑 JSON 文件（mcp_config / global_config）的读路径走 `storage::text_io::read_to_string_strip_bom`（剥 Win10 Notepad BOM）；外部 CLI 输出（dws / where.exe / tasklist）解码走 `storage::console_decode::decode_console_bytes`（Windows GBK fallback，靠 `encoding_rs`）；MCP 子进程 spawn 时强制 `PYTHONIOENCODING=utf-8` / `PYTHONUTF8=1` / `LANG=en_US.UTF-8`；hooks runner + skill `!cmd` 替换在 Windows 走 `powershell.exe -NoProfile -Command`（不能裸 `sh -c`）；用户/LLM 提供的文件名走 `storage::safe_filename::ensure_safe_filename`（CON/PRN/COM*/LPT* 保留名 + 禁字符 `<>:"\|?*` + 尾部 `.`/空格 + 长度 ≤ 200）；任何写到磁盘的状态文件优先 tmp + rename 原子写（参考 `runtime::employee::store::write_atomic`），目录删除走 `remove_dir_all_retry` 3×150–300ms backoff。
- **Windows 子进程黑窗抑制（v0.5.8）**：所有 `Command::spawn` / `.output()` 必须先调 `.no_window()`（`storage::process_ext::NoWindowExt` trait extension），它在 Windows 上注入 `CREATE_NO_WINDOW = 0x08000000` 创建标志，其它平台是 no-op。漏一个就在 Windows 上看到 cmd.exe / conhost.exe 一闪而过。
- **max_tokens 按模型自动选（v0.5.8）**：默认输出预算从写死的 4096/100000 改成 `llm::max_tokens::default_max_tokens_for_model(name)` 启发式查表（DeepSeek V4 = 384k、GPT-5 = 128k、GLM-4.5/4.6 = 96k、Claude Sonnet/Opus + Gemini-2 = 64k、Qwen3 / qwen-max-2025 = 16k、Qwen-max-longcontext = 30k、其他兜底 8192）。新增上限按模型加进表里。从 `chat_turn_driver` 调（`llm_settings.primary_model` 在 scope 内）；想覆盖的���用方传 `Some(value)` 即可，否则传 `None` 让 driver 走 per-model 默认。
- **跨平台拖拽上传（v0.5.9）**：Tauri 2 webview 拦截 HTML5 drop 事件，React `onDrop` 永不触发；唯一靠谱的入口是 `getCurrentWebview().onDragDropEvent`。`useDragDropListener`（在 `App` 顶层 mount 一次）订阅 native 事件，把 resolved `PendingAttachment[]` push 进 `useDropInbox`（zustand pull queue），HomeTaskComposerCard / ChatBottomArea 各自 useEffect drain。新增附件路径校验（`useChatAttachments::isAcceptablePastedPath` / `makePendingAttachment` / `resolvePastedPaths`）必须支持 `[\\/]` 双分隔符 + Windows `C:\` 卷根 + 系统目录前缀拒绝。
- **剪贴板图片粘贴**：`useComposerPaste.handlePaste` 支持截图/复制图片粘贴。流程：同步提取 `clipboardData.items` 中的 image blob（异步后 clipboardData 会被浏览器清空）→ 先尝试 native file paths（Finder 复制文件）→ 路径为空时 fallback 到 image blob → `saveClipboardImage()` 写入 `tmpImage/` → 添加为 `PendingAttachment`。`saveClipboardImage` 从 `useChatAttachments` 传入（Tauri IPC → Rust `save_clipboard_image_to_tmp`）。ChatBottomArea + HomeTaskComposerCard 两个 composer 均已接入。
- **租户运行时换肤（C 路径，2026-05-09 重做）**：tenant 4 色（`accentColor` / `primaryColor` / `bgColor` / `sidebarBgColor`）+ `productName` + `logoUrl` + `fontFamily` 由 lotus 后台（`tenant-portal/web/src/pages/settings/branding.tsx`，10 个 PRESET_THEMES）下发；后端 `auth/state.rs::TenantInfo` 透传，前端 `src/stores/brandingStore.ts::applyBranding` 派生 ~40 个 CSS 变量（design.pen + legacy `--color-*` 双套命名空间）。派生算法在 `src/lib/themeUtils.ts`：accent 用 lighten/darken/rgba 派生 hover/subtle/-50/-100/...；primary 决定 `--foreground` 和文字色；bg/sidebar 用 `mixColors(bg, fg, ratio)` 派生 muted/border/popover。`isDarkColor` 决定 `--*-foreground` 用 `#FFFFFF` 还是 `#1A1A1A`，所以 dark mode 不需要 `.dark` selector，直接 4 色填深色即可。`authStore` 在 `restoreFromStorage / login` 后调 `applyTenantBranding(info)`，`logout` 时 `useBrandingStore.reset()`。**前端不再提供本地 accent 选择器**（之前 `GeneralPanel` 的强调色 swatch 已删除）——皮肤完全由 lotus 后台定，避免"用户本地选择 vs 租户下发"的覆盖冲突。`localStorage` 里历史残留的 `aijia-accent-color` 和后端 settings 里的 `accentColor` 字段都不再被读取。
- **标题栏 = accent 色（2026-05-09）**：mac Overlay 区 + Windows 自绘标题栏统一用 `bg-primary text-primary-foreground` 显示租户 accent，是用户感知最强的换肤点。`App.tsx` 顶层 wrapper 用 `bg-background`（不再是 `bg-sidebar`），让租户 bgColor 一改 wrapper 就跟着变。Windows 标题栏拖拽踩坑：① `flex-1` 占位 div 必须有 `data-tauri-drag-region`，吃掉中间空白区域的拖拽；② `WindowControls` 外层 `onMouseDown stopPropagation`，否则点击关闭按钮会被父级拖拽吞；③ `UpdateAvailableLink` 也必须包 `stopPropagation` 容器，否则点击更新链接没反应（0.3.x 历史 bug）。mac Overlay 拖拽由系统提供，**不要**额外绑 `onMouseDown=startDragging` 否则跟系统冲突。Windows 标题栏底部用 `border-b border-primary-foreground/15` 做半透明分隔线，避免 accent 与 background 色差小时糊在一起。

