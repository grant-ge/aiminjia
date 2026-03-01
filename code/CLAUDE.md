# AI小家 — 项目上下文

> 组织专家，工作助手 — 随叫随到的 HR 智能顾问。Tauri 2.x 桌面应用。

## 项目结构

```
code/
├── scripts/                     # 构建辅助脚本
│   ├── setup-python.sh          # macOS/Linux: 下载 Python standalone + 安装 pip 依赖
│   └── setup-python.ps1         # Windows: 同上 PowerShell 版
├── src/                          # React + TypeScript 前端
│   ├── App.tsx                   # 入口，挂载 useStreaming + ToastContainer
│   ├── components/
│   │   ├── layout/               # Sidebar, TopBar, ChatArea, InputBar
│   │   ├── chat/                 # MessageList, MessageItem, UserBubble, AiBubble,
│   │   │                         # StreamingBubble, TypingIndicator, StepDivider
│   │   ├── rich-content/         # CodeBlock, DataTable, MetricCards, OptionCards 等
│   │   ├── settings/             # SettingsModal
│   │   └── common/               # Avatar, ToastContainer
│   ├── stores/                   # Zustand: chatStore, settingsStore, analysisStore, notificationStore
│   ├── hooks/                    # useChat, useStreaming, useFileUpload, useTauriEvent
│   ├── lib/                      # tauri.ts (IPC), markdown.ts (Markdown→HTML 渲染), format.ts
│   ├── types/                    # message.ts, analysis.ts, settings.ts
│   └── styles/                   # globals.css (TailwindCSS 4 Design Tokens)
├── src-tauri/                    # Rust 后端
│   └── src/
│       ├── lib.rs                # Tauri 应用构建 + 命令注册
│       ├── commands/             # chat, file, settings (IPC 命令)
│       ├── llm/                  # gateway, router, providers/, tools, masking, streaming,
│       │                         # prompts (系统提示词库), orchestrator (6步分析编排器)
│       ├── search/               # tavily, searxng
│       ├── storage/              # file_store (JSON/JSONL), crypto, workspace, file_manager
│       ├── python/               # runner, parser, sandbox
│       └── models/               # conversation, message, analysis, settings
└── python/                       # Python 脚本（预留）
```

## 开发命令

```bash
pnpm install              # 安装前端依赖
pnpm dev                  # 启动 Vite dev server（仅前端）
pnpm tauri dev            # 启动完整 Tauri 开发环境（前端 + Rust 后端）
pnpm tauri build          # 构建生产包
pnpm test                 # 运行前端测试 (vitest)
pnpm test:watch           # 前端测试监听模式
cargo test --manifest-path src-tauri/Cargo.toml  # 运行 Rust 测试
bash scripts/setup-python.sh  # 下载打包 Python 运行时（构建前执行一次）
```

## 数据存储位置

| 数据 | 路径 |
|------|------|
| 文件存储根目录 | Tauri `app_data_dir()` (macOS: `~/Library/Application Support/com.aijia.app/`) |
| 加密密钥 | OS Keychain (`com.aijia.app.secure_storage`) |
| 用户工作目录 | `~/.renlijia/` (默认) |
| 应用设置 | `{base_dir}/config.json` |
| 各 Provider API Key | `{base_dir}/config.json`，键 `apiKey:{provider}`，AES-256-GCM 加密 |

## 命名约定

- **Tauri 事件名**: `streaming:delta`, `streaming:done`, `streaming:error`, `streaming:step-reset`, `message:updated`, `conversation:title-updated`, `tool:executing`, `tool:completed`, `analysis:step-transition`, `agent:idle`（所有事件均含 `conversationId` 字段）
- **Tauri 命令名**: snake_case (`send_message`, `create_conversation`, `stop_streaming`, `is_agent_busy`, `switch_provider`, `get_configured_providers`, `get_all_provider_keys`, `update_all_provider_keys`, `reveal_file_in_folder`)
- **前端 Store**: camelCase (`useChatStore`, `activeConversationId`)
- **CSS Token 前缀**: `--color-*`, `--spacing-*`, `--radius-*`, `--shadow-*`

## 视觉标准强制规则

> 完整规范见 `docs/visual-standard.md`，以下为开发时必须遵守的硬性约束。

**字号 — 只能用 Token 类名：**

| Token | 大小 | 用途 |
|-------|------|------|
| `text-xs` | 12px | 标注、版本号、Tag、时间戳 |
| `text-sm` | 13px | 辅助信息、表头、表格数据 |
| `text-base` | 14px | 正文、表单输入、卡片标题 |
| `text-md` | 15px | 对话正文、主内容 |
| `text-lg` | 17px | 页面标题 |
| `text-xl` | 20px | 大数字 |
| `text-2xl` | 24px | Metric 大数字 |

- 根字号 `16px`（Apple HIG 标准），禁止修改
- 禁止 `text-[X.XXrem]` 任意值

**颜色 — 只能用 CSS 变量：**

- 交互色（按钮/选中/Tab）：`var(--color-primary)` 及其 `-hover` / `-active` / `-subtle` / `-muted` 变体（Carbon Black）
- 品牌色（仅 AI 头像/Logo）：`var(--color-accent)` 及其变体（Gold）
- 语义色：`var(--color-semantic-red)` / `var(--color-semantic-blue)` / ... 及其 `-bg` / `-bg-light` / `-border` 变体
- 新增颜色必须先在 `globals.css` `@theme` 中注册
- 禁止在组件中直接写 `rgba(R,G,B,A)` 语义色值

**圆角 — 只能用 Token：**

- `rounded-xs`(4px) / `rounded-sm`(6px) / `rounded-md`(8px) / `rounded-lg`(12px) / `rounded-xl`(16px) / `rounded-full`
- 禁止 `rounded-[Xpx]` 任意值

**间距 — 统一规则：**

- rich-content 组件外边距：`my-3`（12px）
- 消息气泡间距：`mb-7`（28px）
- 步骤分割线间距：`my-7`（28px）

**阴影/遮罩 — 必须用 Token：**

- 模态框遮罩：`var(--color-overlay)`
- 模态框阴影：`var(--shadow-modal)`
- 输入栏阴影：`var(--shadow-input)`

**图标 — 内容区极简：**

- rich-content 卡片标题**不使用装饰性 SVG 图标**，通过语义颜色 + 背景色区分类型
- 报告/文件类型使用文字标签（HTML/XLS/PDF）+ 语义色，不使用 emoji
- SVG 图标仅限操作区域（Sidebar/TopBar/InputBar 按钮、Toast 关闭）
