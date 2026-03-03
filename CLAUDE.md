# AI小家 — 组织专家，工作助手

Tauri 2.x 桌面 AI Agent 应用，面向 HR 专业人员的薪酬公平性分析工具。Agent 通过 6 步分析流程（方向确认 → 数据清洗 → 岗位归一化 → 职级推断 → 公平性诊断 → 行动方案）帮助用户完成薪酬分析，同时支持日常 HR 咨询。

## 项目结构

```
├── CLAUDE.md                          # 本文件（项目开发指南）
├── docs/
│   ├── agent-design.md                # 产品设计（能力概述、6步流程、日常场景、安全）
│   ├── agent-architecture.md          # 技术架构（状态机、Agent Loop、记忆、工具、脱敏）
│   ├── visual-standard.md             # 视觉标准（Design Tokens、色彩、字体、间距、组件模式）
│   └── extension-guide.md             # 插件扩展（Python Tool / 声明式 Skill / Rust 扩展）
└── code/                              # 应用源码
    ├── package.json                   # pnpm + Vite + React 19
    ├── scripts/                       # 构建辅助脚本
    │   ├── setup-python.sh            # macOS/Linux: 下载 Python standalone + 安装 pip 依赖
    │   ├── setup-python.ps1           # Windows: 同上 PowerShell 版
    │   └── upload-to-oss.py           # 发布：上传安装包到阿里云 OSS
    ├── src/                           # 前端（React + TypeScript + TailwindCSS 4）
    │   ├── components/                # UI 组件
    │   │   ├── layout/                # Sidebar, TopBar, ChatArea, InputBar
    │   │   ├── chat/                  # MessageList, UserBubble, AiBubble
    │   │   ├── rich-content/          # CodeBlock, DataTable, MetricCards, ConfirmBlock...
    │   │   ├── settings/              # SettingsModal, LoginSection
    │   │   └── common/                # Button, Badge, Avatar, Modal
    │   ├── stores/                    # Zustand stores (chatStore, settingsStore, authStore)
    │   ├── hooks/                     # useChat, useStreaming, useFileUpload, useTauriEvent
    │   ├── lib/                       # tauri.ts (IPC wrappers), markdown.ts, format.ts
    │   ├── types/                     # TypeScript 类型定义
    │   └── styles/                    # globals.css (Design Tokens)
    └── src-tauri/                     # Rust 后端（Tauri 2.x）
        ├── Cargo.toml
        ├── plugins/                   # 声明式插件（TOML + Markdown）
        └── src/
            ├── lib.rs                 # 应用启动 + 状态注册 + graceful shutdown
            ├── commands/              # IPC 命令（chat, file, settings, auth）
            ├── plugin/                # 插件系统（Tool + Skill 注册式架构）
            ├── llm/                   # LLM 网关 + Agent 编排 + 工具执行器
            ├── auth/                  # 云端认证（登录、Token 续期、状态持久化）
            ├── storage/               # 文件存储（JSON/JSONL）+ 加密 + 工作目录
            ├── python/                # Python 子进程（沙箱执行 + REPL 会话）
            └── models/                # 数据模型
```

## 架构概览

```
Frontend (React + TypeScript + TailwindCSS 4)
    ↓ Tauri IPC
Rust Backend
    ├── commands/    — IPC command handlers (chat, file, settings, auth)
    ├── plugin/      — Tool + Skill plugin system (registry, traits, builtin, Python bridge)
    ├── llm/         — LLM gateway, streaming, agent orchestration, tool executors
    ├── auth/        — Cloud auth (login, token refresh, session key, encrypted persistence)
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

## 发布流程

发布新版本按以下 5 步顺序执行：

### Step 1: 升级版本号

修改以下 3 个文件中的 `version` 字段（保持一致）：
- `code/package.json`
- `code/src-tauri/Cargo.toml`
- `code/src-tauri/tauri.conf.json`

### Step 2: 提交 + 打 Tag → 触发 GitHub Actions

```bash
git add -A && git commit -m "bump to vX.Y.Z"
git tag vX.Y.Z
git push origin main --tags
```

CI（`.github/workflows/build-desktop.yml`）自动构建：
- macOS ARM `.dmg`（macos-14 runner）
- Windows `.exe`（windows-latest runner）
- 创建 GitHub Release（Draft）

### Step 3: 本地交叉编译 macOS Intel DMG

```bash
cd code && pnpm tauri build --target x86_64-apple-darwin
```

产物位于 `code/src-tauri/target/x86_64-apple-darwin/release/bundle/dmg/`

### Step 4: 上传到阿里云 OSS

```bash
# 1. 等 CI 完成后，发布 Release
gh release edit vX.Y.Z --draft=false --latest

# 2. 下载 CI 产物（仓库 private，通过 artifacts 下载）
gh run download <run-id> --dir /tmp/aijia-release

# 3. 用 Python 脚本上传到 OSS（ARM DMG + Intel DMG + Windows EXE）
OSS_ACCESS_KEY_ID=xxx OSS_ACCESS_KEY_SECRET=xxx python3 code/scripts/upload-to-oss.py X.Y.Z
```

脚本上传到 `aijia/vX.Y.Z/` 版本目录，并复制到 `aijia/latest/`（Landing Page 下载链接指向 latest）。

### Step 5: 更新 Homebrew Cask

```bash
# 修改 version "X.Y.Z"
vi /opt/homebrew/Library/Taps/grant-ge/homebrew-tap/Casks/aijia.rb

cd /opt/homebrew/Library/Taps/grant-ge/homebrew-tap
git add Casks/aijia.rb && git commit -m "Update aijia to vX.Y.Z" && git push
```

### 下载地址

| 渠道 | 地址 |
|------|------|
| OSS latest（Landing Page） | `https://lotus.renlijia.com/aijia/latest/{macos-arm64,macos-x64,windows-x64}` |
| OSS 版本化 | `https://lotus.renlijia.com/aijia/vX.Y.Z/` |
| GitHub Release | `https://github.com/grant-ge/aiminjia/releases` |
| Homebrew | `brew install grant-ge/tap/aijia` |

### 注意事项

- `requirements.txt` 不能包含非 ASCII 字符（Windows pip 用 cp1252 解码会报错）
- 仓库是 private，`upload-to-oss.py` 无法直接从 GitHub Release URL 下载，需通过 `gh run download` 获取 CI artifacts
- OSS 域名 `lotus.renlijia.com` 是 CNAME 到 `lotus-releases.oss-cn-beijing.aliyuncs.com`

### 相关文件

| 文件 | 用途 |
|------|------|
| `.github/workflows/build-desktop.yml` | CI 构建 + GitHub Release |
| `code/scripts/upload-to-oss.py` | OSS 上传脚本（密钥通过环境变量传入） |
| `code/scripts/setup-python.ps1` | Windows Python runtime 打包脚本 |
| Homebrew Tap `grant-ge/homebrew-tap` | Cask 定义（下载指向 OSS） |
| lotus 项目 `code/home/index.html` | Landing Page（下载指向 OSS latest） |

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
| 云端认证 | `{base_dir}/config.json` key `cloud_auth`（JWT + session_key，AES-256-GCM 加密） |
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
| Tauri 事件 | colon-separated | `streaming:delta`, `auth:expired` |
| 存储格式 | JSON / JSONL | JSON 结构化，JSONL 追加（消息、审计） |
