# AI小家 — 组织专家，工作助手

Tauri 2.x 桌面 AI Agent 应用，面向 HR 专业人员的薪酬公平性分析工具。Agent 通过 6 步分析流程（方向确认 → 数据清洗 → 岗位归一化 → 职级推断 → 公平性诊断 → 行动方案）帮助用户完成薪酬分析，同时支持日常 HR 咨询。

## 项目结构

```
├── CLAUDE.md                          # 本文件（项目开发指南）
├── docs/
│   ├── agent-design.md                # 产品设计文档（Agent 行为、6步流程、知识体系）
│   ├── agent-architecture.md          # 技术架构文档（模块设计、数据流、安全策略）
│   ├── visual-standard.md             # 视觉设计标准（Design Tokens、组件规范）
│   └── extension-guide.md             # 插件扩展指南（新增 Tool / Skill 必读）
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
    │   ├── stores/                    # Zustand stores
    │   ├── hooks/                     # useChat, useStreaming, useFileUpload, useTauriEvent
    │   ├── lib/                       # tauri.ts (IPC wrappers), markdown.ts, format.ts
    │   ├── types/                     # TypeScript 类型定义
    │   └── styles/                    # globals.css (Design Tokens)
    └── src-tauri/                     # Rust 后端（Tauri 2.x）
        ├── Cargo.toml
        ├── plugins/                   # 声明式插件（TOML + Markdown）
        └── src/
            ├── lib.rs                 # 应用启动 + 状态注册 + graceful shutdown
            ├── commands/              # IPC 命令（chat, file, settings）
            ├── plugin/                # 插件系统（Tool + Skill 注册式架构）
            ├── llm/                   # LLM 网关 + Agent 编排 + 工具执行器
            ├── storage/               # 文件存储（JSON/JSONL）+ 加密 + 工作目录
            ├── python/                # Python 子进程（沙箱执行 + REPL 会话）
            └── models/                # 数据模型
```

## 架构概览

```
Frontend (React + TypeScript + TailwindCSS 4)
    ↓ Tauri IPC
Rust Backend
    ├── commands/    — IPC command handlers
    ├── plugin/      — Tool + Skill plugin system (registry, traits, builtin, Python bridge)
    ├── llm/         — LLM gateway, streaming, agent orchestration, tool executors
    ├── storage/     — File-based storage (JSON/JSONL), AES-256-GCM encryption
    ├── python/      — Sandboxed Python subprocess (one-shot + persistent REPL session)
    └── search/      — Web search (SearXNG free + Bocha/Tavily paid fallback)
```

**Key entry points:**
- `src-tauri/src/lib.rs` — App setup, state registration
- `src-tauri/src/commands/chat.rs` — Main agent loop (`send_message`)
- `src/App.tsx` — Frontend root component

## 开发命令

```bash
# 环境准备（首次）
cd code && pnpm install
cd code && bash scripts/setup-python.sh   # 下载 Python 运行时 + pip 依赖

# 开发
cd code && pnpm tauri:dev    # 完整应用（前端 + Rust 后端）
cd code && pnpm dev          # 仅前端（WebView）

# 检查
cd code/src-tauri && cargo check          # Rust 类型检查
cd code/src-tauri && cargo test           # Rust 测试
cd code && npx tsc --noEmit               # TypeScript 类型检查
cd code && pnpm lint                      # ESLint

# 构建
cd code && pnpm tauri:build               # 生产包
```

Build output: `code/src-tauri/target/release/bundle/`（macOS: `.app` + `.dmg`，Windows: `.exe` + `.msi`）

**Prerequisites:** Node.js v18+, pnpm v9+, Rust stable

## 扩展优先原则

增加功能时，**必须优先通过插件扩展**，避免修改核心引擎。优先级：

1. **声明式 Skill**（TOML + Markdown）— 新垂直场景，零 Rust 代码
2. **Python Tool**（handler.py）— 数据处理类工具
3. **Rust Tool**（ToolPlugin trait）— 需要系统 API 时
4. **Rust Skill**（Skill trait）— 需要复杂流转逻辑时
5. **修改核心引擎** — 最后手段

详见 `docs/extension-guide.md`

## 常见开发场景

**新增 Tool：** 优先 Python 插件 `src-tauri/plugins/{id}/plugin.toml + handler.py`，或 Rust 实现 `plugin/builtin/tools/` 下新建文件 + 注册

**新增 Skill：** 优先声明式 `src-tauri/plugins/{id}/plugin.toml + workflow.toml + prompts/`，或 Rust 实现 `plugin/builtin/skills/` 下新建文件 + 注册

**新增 LLM Provider：** `llm/providers/` 新建 → `router.rs` 注册 → `gateway.rs` 注册

**新增 IPC 命令：** `commands/*.rs` 定义 → `lib.rs` invoke_handler 注册 → `src/lib/tauri.ts` 添加 TS wrapper

**新增前端组件：** 遵循 `docs/visual-standard.md` Design Tokens，使用 `src/styles/globals.css` 中的 CSS 变量

**修改分析流程：** 编辑 `plugins/comp-analysis/` 下的 workflow.toml + prompts，步骤流转由 `orchestrator.rs` 管理

## 数据存储

| 数据 | 位置 |
|------|------|
| 应用数据根目录 | Tauri `app_data_dir()`（macOS: `~/Library/Application Support/com.aijia.app/`） |
| 会话 + 消息 | `{base_dir}/conversations/{id}/`（conv.json + messages.N.jsonl） |
| 应用设置 | `{base_dir}/config.json` |
| API Key | `{base_dir}/config.json`（AES-256-GCM 加密，密钥存 OS Keychain） |
| 用户工作目录 | `~/.renlijia/`（uploads / reports / charts / exports / analysis / temp / logs） |
| 插件 | `{resource_dir}/plugins/`（随应用打包） |

## 命名约定

| 类型 | 风格 | 示例 |
|------|------|------|
| Rust 模块 | snake_case | `file_manager.rs` |
| React 组件 | PascalCase | `AiBubble.tsx` |
| Zustand Store | camelCase + Store | `chatStore.ts` |
| Tauri IPC 命令 | snake_case | `send_message` |
| 前端 IPC wrapper | camelCase | `sendMessage()` |
| JSON 字段 | camelCase | `#[serde(rename_all = "camelCase")]` |
| CSS 变量 | `--color-*`, `--spacing-*`, `--radius-*` | `var(--color-primary)` |
| 存储格式 | JSON / JSONL | JSON 结构化，JSONL 追加（消息、审计） |
