# AI小家 — 组织专家，工作助手

Tauri 2.x 桌面 AI Agent 应用，面向 HR 专业人员的薪酬公平性分析工具。Agent 通过 6 步分析流程（Step 0 方向确认 → 数据清洗 → 岗位归一化 → 职级推断 → 公平性诊断 → 行动方案）帮助用户完成薪酬分析，同时支持日常 HR 咨询。

## 项目结构

```
analysis/
├── CLAUDE.md                          # 本文件
├── docs/
│   ├── agent-design.md                # 产品设计文档（Agent 行为、5步流程、知识体系）
│   ├── tech-architecture.md           # 技术架构文档（模块设计、数据流、安全策略）
│   ├── visual-standard.md             # 视觉设计标准（Design Tokens、组件规范）
│   ├── visual-prototype.html          # 英文视觉原型
│   ├── visual-prototype-zh.html       # 中文视觉原型
│   └── plans/
│       └── 2026-02-27-implementation-plan.md  # 实施计划（Phase 1-8，已全部完成）
└── code/                              # 应用源码
    ├── package.json                   # pnpm + Vite + React 19
    ├── scripts/                       # 构建辅助脚本
    │   ├── setup-python.sh            # macOS/Linux: 下载 Python standalone + 安装 pip 依赖
    │   └── setup-python.ps1           # Windows: 同上 PowerShell 版
    ├── src/                           # 前端（React + TypeScript + TailwindCSS 4）
    │   ├── components/                # UI 组件
    │   │   ├── layout/                # Sidebar, TopBar, ChatArea, InputBar
    │   │   ├── chat/                  # MessageList, UserBubble, AiBubble
    │   │   ├── rich-content/          # CodeBlock, DataTable, MetricCards, ConfirmBlock...
    │   │   ├── settings/              # SettingsModal
    │   │   └── common/                # Button, Badge, Avatar, Modal
    │   ├── stores/                    # Zustand stores (chatStore, settingsStore, analysisStore, notificationStore)
    │   ├── hooks/                     # useChat, useStreaming, useFileUpload, useTauriEvent
    │   ├── lib/                       # tauri.ts (IPC wrappers), markdown.ts, format.ts
    │   ├── types/                     # message.ts, analysis.ts, settings.ts
    │   └── styles/                    # globals.css (CSS variables from visual-standard.md)
    └── src-tauri/                     # Rust 后端（Tauri 2.x）
        ├── Cargo.toml
        └── src/
            ├── main.rs / lib.rs       # 启动 + 状态注册 + 崩溃恢复（清理孤儿任务）
            ├── commands/              # IPC 命令
            │   ├── chat.rs            # send_message（编排检测 + Agent Loop）, stop_streaming(conversation_id), is_agent_busy→Vec<String>, get_messages
            │   ├── file.rs            # upload_file, open/reveal/preview/delete_file (均需 conversation_id)
            │   └── settings.rs        # get/update settings, per-provider key CRUD, provider switching
            ├── llm/                   # LLM 网关 + Agent 编排
            │   ├── gateway.rs         # 流式请求（HashMap 多会话并发，最多 3 个同时运行）
            │   ├── router.rs          # 模型路由（分析任务强制默认模型+工具）
            │   ├── providers/         # DeepSeek V3/R1, Volcano, OpenAI, Claude, Qwen
            │   ├── tools.rs           # 10 个 Tool 定义 + 按步骤过滤
            │   ├── prompts.rs         # System Prompt 库（BASE: 组织咨询+工作助手角色 + DAILY: 4大类场景 + STEP0~5: 薪酬分析）
            │   ├── orchestrator.rs    # 6 步分析编排器（Step 0 方向确认 + Step 1~5 分析 + 确认卡点）
            │   ├── masking.rs         # PII 脱敏（mask_text/unmask，3 级别）
            │   ├── streaming.rs       # SSE 解析
            │   └── tool_executor.rs   # Tool 执行分发（10 个 handler）
            ├── search/                # 搜索模块
            │   ├── tavily.rs          # Tavily 付费搜索（优先）
            │   └── searxng.rs         # SearXNG 免费搜索（降级）
            ├── storage/               # 存储模块
            │   ├── file_store/        # 文件存储（JSON/JSONL，完全替代 SQLite）
            │   │   ├── mod.rs         # AppStorage（写锁 + 公共 API）
            │   │   ├── conversations.rs # 会话 CRUD + 全局索引
            │   │   ├── messages.rs    # 分片 JSONL 消息（追加更新 + 去重）
            │   │   ├── files.rs       # 文件索引（上传 + 生成）
            │   │   ├── analysis.rs    # 分析状态
            │   │   ├── config.rs      # 设置（key-value JSON）
            │   │   ├── notes.rs       # 企业记忆（JSONL last-writer-wins）
            │   │   ├── audit.rs       # 审计日志（自动分片 2MB）
            │   │   ├── cache.rs       # 搜索缓存（TTL）
            │   │   ├── io.rs          # 原子写入 / JSONL 读写 / 文件锁
            │   │   ├── types.rs       # 数据结构定义
            │   │   ├── id.rs          # Base36 ID 生成
            │   │   └── error.rs       # 错误类型
            │   ├── crypto.rs          # AES-256-GCM 加密（OS Keychain 派生密钥）
            │   ├── workspace.rs       # 工作目录管理
            │   └── file_manager.rs    # 文件生命周期（上传/存储/清理）
            ├── python/                # Python 子进程
            │   ├── runner.rs          # 代码执行（沙箱 + 超时 + UTF-8 强制）
            │   ├── parser.rs          # 文件解析分发
            │   └── sandbox.rs         # 沙箱配置（禁止模块、preamble 注入）
            └── models/                # 数据模型
```

## 开发命令

```bash
# 前端开发（仅 WebView）
cd code && pnpm dev

# Tauri 完整开发（前端 + Rust 后端）
cd code && pnpm tauri:dev

# 构建生产包
cd code && pnpm tauri:build

# Rust 类型检查
cd code/src-tauri && cargo check

# TypeScript 类型检查
cd code && npx tsc --noEmit

# Rust 测试
cd code/src-tauri && cargo test

# Lint
cd code && pnpm lint

# 下载打包 Python 运行时（构建前需执行一次）
cd code && bash scripts/setup-python.sh
```

## 开发指南

### 环境准备 (Setup)

**Prerequisites:**
- Node.js (v18+)
- pnpm (v9+)
- Rust toolchain (rustup, stable channel)

**First-time setup:**
```bash
cd code && pnpm install
cd code && bash scripts/setup-python.sh   # Download Python runtime + pip dependencies
```

**Development:**
```bash
cd code && pnpm tauri:dev    # Full app (frontend + Rust backend)
cd code && pnpm dev          # Frontend only (WebView, no Rust backend)
```

### 项目理解 (Understanding the Project)

**Reference docs:**
- `docs/agent-design.md` — Product features, agent behavior, 6-step analysis flow
- `docs/tech-architecture.md` — Module design, data flow, security strategy
- `docs/visual-standard.md` — UI design tokens, component specs, color system

**Key entry points:**
- `src-tauri/src/lib.rs` — App setup, state registration, crash recovery
- `src-tauri/src/commands/chat.rs` — Main agent loop (`send_message`), streaming, orchestration
- `src/App.tsx` — Frontend root component

**Architecture overview:**
```
Frontend (React + TypeScript + TailwindCSS 4)
    ↓ Tauri IPC
Rust Backend
    ├── commands/    — IPC command handlers
    ├── llm/         — LLM gateway, agent orchestrator, tools, prompts
    ├── storage/     — File-based storage (JSON/JSONL, replaces SQLite)
    ├── python/      — Sandboxed Python subprocess execution
    └── search/      — Web search (Tavily + SearXNG fallback)
```

### 常见开发场景 (Common Development Tasks)

**Adding a new tool:**
1. Define the tool schema in `src-tauri/src/llm/tools.rs`
2. Implement the handler in `src-tauri/src/llm/tool_executor.rs`
3. Add the tool to the appropriate step filter in `tools.rs` (which steps can use it)

**Adding a new LLM Provider:**
1. Create a new file in `src-tauri/src/llm/providers/`
2. Register the provider in `src-tauri/src/llm/router.rs`
3. Register in `src-tauri/src/llm/gateway.rs`

**Modifying system prompts:**
- Edit prompt constants in `src-tauri/src/llm/prompts.rs`
- BASE prompt defines the core agent persona
- DAILY prompt covers daily HR consultation scenarios
- STEP0~STEP5 prompts control each analysis step's behavior

**Modifying analysis steps:**
- Edit `src-tauri/src/llm/orchestrator.rs` for flow logic and step transitions
- Edit step-specific prompts in `src-tauri/src/llm/prompts.rs`

**Adding frontend components:**
- Follow `docs/visual-standard.md` design tokens
- Use semantic CSS variables from `src/styles/globals.css`
- Place components in the appropriate subdirectory under `src/components/`

**Adding a new IPC command:**
1. Define the command function in `src-tauri/src/commands/*.rs`
2. Register it in `src-tauri/src/lib.rs` `invoke_handler`
3. Add a TypeScript wrapper in `src/lib/tauri.ts`

### 测试 (Testing)

```bash
# Rust tests
cd code/src-tauri && cargo test

# TypeScript type checking
cd code && npx tsc --noEmit

# ESLint
cd code && pnpm lint

# Manual testing with full app
cd code && pnpm tauri:dev
```

### 构建发布 (Building)

```bash
# Download Python runtime (first time only)
cd code && bash scripts/setup-python.sh

# Build production bundle
cd code && pnpm tauri:build
```

Build output location: `code/src-tauri/target/release/bundle/`
- macOS: `.app` bundle and `.dmg` installer
- Windows: `.exe` and `.msi` installer

## 数据存储位置

| 数据 | 位置 | 说明 |
|------|------|------|
| 文件存储根目录 | Tauri app_data_dir | JSON/JSONL 文件，会话/消息/设置/文件索引/企业记忆 |
| 全局会话索引 | {base_dir}/index.json | 所有会话的轻量索引 |
| 会话数据 | {base_dir}/conversations/{id}/ | conv.json + messages.N.jsonl + file_index.json + analysis.json |
| 设置 | {base_dir}/config.json | key-value 键值对 |
| 企业记忆 | {base_dir}/shared/memory/memory.jsonl | last-writer-wins 语义 |
| 审计日志 | {base_dir}/audit/audit.jsonl | 自动分片（2MB） |
| 搜索缓存 | {base_dir}/shared/cache/{hash}.json | TTL 7 天 |
| 用户上传文件 | workspace/uploads/ | 物理文件，file_index.json 记录绑定 conversation_id |
| 生成的报告 | workspace/reports/ | HTML/Excel 报告 |
| 生成的图表 | workspace/charts/ | PNG 图表 |
| 导出数据 | workspace/exports/ | CSV/Excel/JSON 导出 |
| 临时文件 | workspace/temp/ | Python 脚本执行临时文件 |
| API Key 加密 | OS Keychain | macOS Keychain / Windows Credential Manager |
| 各 Provider API Key | config.json | `apiKey:{provider}` 键，AES-256-GCM 加密存储 |

## 命名约定

- **Rust 模块**：snake_case（`tool_executor.rs`, `file_manager.rs`）
- **React 组件**：PascalCase（`AiBubble.tsx`, `SettingsModal.tsx`）
- **Zustand Store**：camelCase + Store 后缀（`chatStore.ts`, `settingsStore.ts`）
- **Tauri IPC 命令**：snake_case（`send_message`, `upload_file`, `switch_provider`）
- **前端 IPC 包装**：camelCase（`sendMessage()`, `uploadFile()`, `switchProvider()`）
- **文件存储格式**：JSON（结构化数据）/ JSONL（追加数据：消息、记忆、审计日志）
- **JSON 字段名**：camelCase（`#[serde(rename_all = "camelCase")]`）
- **CSS 变量**：`--color-*`, `--font-*`, `--spacing-*`, `--radius-*`
