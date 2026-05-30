# AIjia — 桌面端代码仓库

> 最后梳理：2026-05-29
> 当前仓库：`/Users/gezhigang/work-codeup/aijia/code`
> 服务端仓库：`/Users/gezhigang/lotus`

企业 AI 工作台 Tauri 2.x 桌面应用（React 19/TypeScript 前端 + Rust Runtime + RuntimeManager 管理的本地运行环境）。

产品名：**AIjia**（元数据/文件名）/ **AI小家**（UI 面向用户）。标识符：`com.aijia.app`。

日常编码约束以本文件为准；当前架构参考以 `docs/architecture-blueprint.md` 和 `docs/decisions/*.md` 为准。`~/lotus/docs/desktop/` 保留跨仓长期参考和服务端仓库需要维护的桌面端文档，其中存储规范 `~/lotus/docs/desktop/storage-conventions.md` 仍是写盘路径权威。

## 仓库结构（关键部分）

```
├── CLAUDE.md                  # 本文件，日常编码必读约束
├── AGENTS.md                  # agent 执行约束
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
├── docs/
│   ├── architecture-blueprint.md
│   ├── decisions/
│   └── release-playbook.md
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
- 对话框 → `@/components/ui/dialog` 的 `Dialog` / `DialogContent`，外层圆角想生效就给 `DialogContent` 加 `overflow-hidden`。**`DialogContent` 自带右上角关闭 X**（`DialogPrimitive.Close` + lucide `X`，主题色），不要再在弹窗里手搓关闭按钮；个别需要强流程不可关的弹窗传 `hideClose` 关掉内置 X
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

**6. macOS 10.15 (WebKit 605 / Safari 13) CSS 兼容性约束**

`vite.config.ts` 的 `build.target: 'safari13'` **只降级 JS 语法，对 CSS 完全无效**。以下 CSS 特性在 Safari 13 上不可用，禁止在源码中使用：

- ❌ `color-mix()` — Safari 16.2+。替代方案：在 `:root` 定义 `--foo-rgb: R, G, B` 变量，用 `rgba(var(--foo-rgb), 0.X)` 替代；需要混合两个实色时预计算静态 hex 值
- ❌ Tailwind v4 渐变工具类（`bg-gradient-to-*` / `from-*` / `to-*`）— v4 生成带 `in oklch` 色彩空间的 CSS，Safari 15.4+ 才支持。替代方案：内联 style 手写标准 `linear-gradient(to top, var(--primary), transparent)`
- ❌ `color-mix()` 嵌套在 `linear-gradient` / `radial-gradient` 中 — 整条渐变声明失效
- ❌ Tailwind 任意值 `drop-shadow-[..._color-mix(...)]` — 整条 filter 失效，改用内联 style `filter: drop-shadow(...)`

项目已有 `--primary-rgb` / `--foreground-rgb` / `--color-bg-base-rgb` / `--muted-foreground-rgb` / `--card-rgb` 等 RGB 分量变量（`globals.css :root`），新增颜色混合需求优先复用。`src/lib/themeUtils.ts` 中已有 `mixColors(hex1, hex2, weight)` 可用于运行时计算两个 hex 色的混合值。

## 存储结构

**权威规范在 `~/lotus/docs/desktop/storage-conventions.md`**。本节只给一句话指引 + 入口；任何写盘改动以规范文档为准。

### 五个域

| 域 | 路径 | 例子 | 入口 |
|---|---|---|---|
| L0 全局元数据 | `~/.renlijia/global/` | `config.json` / `state.json` / `auth/` | `AiJiaHome::global_*` |
| L0 系统单点 | `~/.renlijia/` 根级（白名单） | `crypto/` / `device_id` / `employee-templates-cache/` / `tmp/` | `AiJiaHome::*` |
| L1 用户私有 | `~/.renlijia/users/{scope}/` | `conversations/` / `employees/` / `skills/` / `permissions.json` / `turn_stages/` | `UserScopedPaths::*` |
| L2 临时可重生成 | `~/.renlijia/tmp/` | 剪贴板贴图、IM 附件下载 | `AiJiaHome::tmp_*_dir()` |
| L3 用户工作区 | `<workspacePath>/`（默认 `defaultFolder/`） | 报表 / 图表 / 上传副本 | 工具上下文 `workspace_path()` |

### 关键约束

- **L1 用户私有数据是缺省**：不知道往哪写就用 `UserScopedPaths`。
- **runtime/ 模块不得直接构造 `AiJiaHome`**，必须通过 `UserScopedPathResolver` trait 注入（参考 `chat_turn_driver::turn_stage_path_resolver`、`RuntimeHost::resolve_*` 一族）。
- **L1 未登录态必须 no-op**，不得回退到 root（参考 `transport/tauri_commands/turn_stage.rs::get_active_turn_stage`）。
- **新增 root 级条目必须更新规范 §5 白名单 + `migration_root_cleanup::ARCHIVE_ITEMS`**，否则会被下一轮 cleanup 误归档。
- **消息存储是单文件 `messages.jsonl`**（append-only，按 `id` LWW + `_rev` 增量），旧 shard 模型（`messages.N.jsonl` / `_current` / `compact_boundaries.jsonl`）已通过 `migrate_shards_to_single_file` 自动合并，新代码不要再用 shard 路径。

### 迁移历史 / 进行中

- `users/{scope}/` 已是主写入路径；老 root-level 数据由 `migration_user_scope` 一次性 copy 后，`migration_root_cleanup` 归档到 `.archived-legacy-{ts}/`，30 天自动 GC。
- `turn_stages/` 已 user-scope 化（commit 4108f8e）。`subagent_transcripts/` 已迁到 `conversations/{conv_id}/subagents/`，旧路径标 `#[deprecated]`。
- 仍在做：`playwright-profile/` 用户化、`tmpImage/` 收编到 `tmp/clipboard/`、IM `*_downloads/` 加 GC、`runtimes_dir()` 从 `~/Library/Caches/` 收编回根。完整清单见规范 §11。

## 重要架构决策与约束

1. **Tauri command 层只做参数接收 → 转发 Runtime**，不含业务逻辑（见 `docs/architecture-blueprint.md`）
2. **不接受只改 prompt（base.md/daily.md）来修复能力问题**；能力边界应由 runtime/tool/capability/sandbox 保证
3. **新工具应实现 `RuntimeTool` trait**，不应新增 `ToolPlugin` 实现
4. **`CapabilityContext`（`runtime/tools/capability.rs`）是工具获取系统能力的窄接口**，不应扩大它来传入 `LlmGateway`、`AuthManager` 等编排层对象
5. **LLM 协议：生产路径走 Anthropic**（`lotus.rs` → `claude.rs`，直通 `/anthropic/v1/messages`）。`openai.rs` 仅供 `custom.rs::send_openai_compat` 复用 OpenAI 兼容协议；新增 LLM 能力默认按 Anthropic 协议设计。Prompt renderer 用协议中性命名 `ChatPromptRenderer` / `system_message`，**禁止再加 `openai_` 前缀**。System prompt 多段缓存通过 `SystemPromptSegment` + `stream_message_with_segments` 入口透传；system 侧 `cache_control: ephemeral` 上限 3 块（总额度 4，预留 1 给 tools）。详见 `docs/superpowers/specs/2026-05-10-anthropic-protocol-migration-cleanup.md`
6. **Token / Cost 统计必须包含 Anthropic cache 字段**：`TokenUsage` / `TotalTokenUsage` 有 `cache_creation_input_tokens` / `cache_read_input_tokens`；`estimated_cost_usd` 按 1.25× / 0.1× 加权。新增涉及 token 的事件 / 日志字段，必须同步透传到 `TurnCompleted` 与前端 TS 类型，并保留 `#[serde(default)]` 兼容老会话
7. **Windows 兼容性**：git 子进程 `-c core.quotepath=false`；JSON 读取 `strip_bom`；CLI 输出 `decode_console_bytes`（GBK）；文件名 `ensure_safe_filename`；原子写 tmp+rename；目录删除 `remove_dir_all_retry`。详见 [`docs/decisions/ui-platform-decisions.md`](docs/decisions/ui-platform-decisions.md)
8. **Windows 子进程黑窗**：所有 `Command::spawn/.output()` 必须先调 `.no_window()`（`storage::process_ext::NoWindowExt`）。详见同上
9. **旧 macOS 白屏防护**：禁止引入含正则 lookbehind `(?<=`/`(?<!` 的依赖；`vite.config.ts` build.target 已设 safari13。详见同上
10. **Shell 工具 PATH**：BashTool/PowerShellTool spawn 前必须 `inject_bundled_runtime_path`；命令收尾走 `emit_shell_failure_diagnostic`。详见 [`docs/decisions/runtime-decisions.md`](docs/decisions/runtime-decisions.md)
11. **云端唯一 + auth 401 按错误码判定**：所有 LLM 路由恒走 lotus 网关（已移除 `use_cloud`/本地模型/`tavily`/`bocha` 配置）；判定可刷新的 auth 401 用 HTTP 401 + 错误码/类型（`authentication_error`/`auth_error`），**禁止再用消息文案子串匹配**。详见 [`docs/decisions/runtime-decisions.md`](docs/decisions/runtime-decisions.md)
12. **`/auth/refresh` 必须单飞 + 先落盘后改内存**（`auth/mod.rs`）：服务器对 refresh_token 是单用即吊销，客户端必须把"读 refresh_token → 调 server → persist 磁盘 → 改 in-memory state"整段串行化（用 `AuthManager.refresh_lock: Mutex<()>`），并且 **持久化失败一律拒绝把新 token 提交到内存**（`persist_auth` 现在返回 `Result<()>`，调用方 `?` 传出）。两条铁规：① `refresh_auth_info` 和 `get_session_key` 不许各自独立发 refresh；② disk = source of truth，下次启动 `restore()` 看到的必须是服务器认可的最新 token。判定服务器拒签用 `client::is_auth_unauthorized(err)`（typed `AuthApiError.status==401`），网络/5xx 不清状态、留给下次重试。背景：SLS 实锤 `user_id=87` 服务器持有 `ed8f044d` / 客户端硬盘还是 `c104df42` 的 token 错位，根因就是 race + persist 静默失败。

### 已归档的设计决策（详情外迁，按需查阅）

- 数字员工系统（模板服务化 PR1-PR6 / 状态机 / SKILL bundle / 配置软校验）→ [`docs/decisions/employee-system-decisions.md`](docs/decisions/employee-system-decisions.md)
- UI / 平台决策（换肤 / 拖拽 / 粘贴 / 标题栏 / 登录页关闭按钮）→ [`docs/decisions/ui-platform-decisions.md`](docs/decisions/ui-platform-decisions.md)
- 运行时决策（max_tokens / 个人版账户计费 / Shell bundled PATH / 云端唯一化 + auth 401 按错误码）→ [`docs/decisions/runtime-decisions.md`](docs/decisions/runtime-decisions.md)

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

## 发布流程

详见 [`docs/release-playbook.md`](docs/release-playbook.md)。

## Git Remotes

- `origin` → `github.com:grant-ge/aiminjia.git`（公开，CI 运行于此）
- `codeup` → `codeup.aliyun.com:renlijia/lotus/lotus-app.git`（国内镜像）

两个都要推。tag 只触发 `origin` 的 CI。

## CLAUDE.md 维护规则

本文件只放**日常编码必读的活跃约束**，不放已完工特性的实现记录。新增设计决策时：
- 含活跃编码约束（"必须 X / 禁止 Y"）→ 在「重要架构决策与约束」加 **1 行摘要**，详情写到 `docs/decisions/*.md` 并链接
- 纯实现记录（"PR 做了什么 / 怎么做的"）→ 直接写到 `docs/decisions/*.md`，CLAUDE.md 只在「已归档的设计决策」加一条指针
- 目标：本文件保持 **< 350 行**

## 关键设计决策

- Tauri 2.x：Ed25519 signed auto-updater（密钥 `~/.tauri/aijia.key`，GitHub Secrets `TAURI_SIGNING_PRIVATE_KEY(_PASSWORD)`）
- 数据存储：文件持久化（JSON/JSONL）+ AES-256-GCM（敏感字段）
- i18n：react-i18next，`src/i18n/{zh-CN,en-US}.json`，默认 zh-CN
- OSS：阿里云 `lotus-releases` bucket，前缀 `aijia/`，CDN `https://lotus.renlijia.com`
- Homebrew：`grant-ge/homebrew-tap` 下 `Casks/aijia.rb`，`on_arm` / `on_intel` 分架构 URL
- GitHub Secrets（4 个，签名在本地做不需要更多）：`TAURI_SIGNING_PRIVATE_KEY`, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`, `OSS_ACCESS_KEY_ID`, `OSS_ACCESS_KEY_SECRET`
- 签名密钥（本地）：`~/.tauri/aijia.key` + Keychain `aijia-tauri-signer`
- OSS 凭证（本地）：Keychain `aijia-oss`，或环境变量 `OSS_ACCESS_KEY_ID` / `OSS_ACCESS_KEY_SECRET`
