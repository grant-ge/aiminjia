# AI小家 — 组织专家，工作助手

Tauri 2.x 桌面 AI Agent，面向 HR 的薪酬分析工具。6 步分析流程 + 日常咨询，支持本地 API Key 和云端账号双模式。

## 项目结构

```
├── docs/                    # 产品设计、技术架构、视觉标准、插件扩展
└── code/
    ├── scripts/             # Python 打包、OSS 上传
    ├── src/                 # React + TS + TailwindCSS 4
    └── src-tauri/src/       # Rust 后端
        ├── commands/        # IPC 命令
        ├── plugin/          # 插件系统
        ├── llm/             # LLM 网关 + Agent 编排
        ├── auth/            # 云端认证
        ├── storage/         # 文件存储 + 加密
        └── python/          # Python 沙箱
```

**关键入口：** `lib.rs` · `commands/chat.rs` · `src/App.tsx`

## 开发命令

```bash
# 环境准备
cd code && pnpm install && bash scripts/setup-python.sh

# 开发
pnpm tauri:dev    # 完整应用
pnpm dev          # 仅前端

# 检查
cargo check && cargo test
npx tsc --noEmit && pnpm lint

# 构建
pnpm tauri:build  # 产物: target/release/bundle/
```

## 发布流程

1. **升级版本号**：`package.json` + `Cargo.toml` + `tauri.conf.json`
2. **提交 + Tag**：`git commit -m "bump to vX.Y.Z" && git tag vX.Y.Z && git push codeup main --tags`
3. **本地构建**：`pnpm tauri build` (ARM) + `pnpm tauri build --target x86_64-apple-darwin` (Intel)
4. **上传 OSS**：`python3 scripts/upload-to-oss.py X.Y.Z`（凭证从 Keychain 读取）
5. **更新 Homebrew**：编辑 `grant-ge/homebrew-tap/Casks/aijia.rb`

**下载地址：** `lotus.renlijia.com/aijia/latest/` · GitHub Release · `brew install grant-ge/tap/aijia`

**���意：** Intel DMG 打包可能失败，手动补救：`hdiutil create -volname "AIjia" -srcfolder macos/AIjia.app -ov -format UDZO dmg/AIjia_X.Y.Z_x64.dmg`

## 扩展优先原则

增加功能时优先通过插件扩展，避免修改核心引擎：

1. **声明式 Skill**（TOML + Markdown）— 新垂直场景，零 Rust 代码
2. **Python Tool**（handler.py）— 数据处理类工具
3. **Rust Tool**（ToolPlugin trait）— 需要系统 API
4. **Rust Skill**（Skill trait）— 复杂流转逻辑

详见 `docs/extension-guide.md`

## 常见开发场景

| 场景 | 操作 |
|------|------|
| 新增 Tool | Python 插件 `plugins/{id}/plugin.toml + handler.py` 或 Rust `plugin/builtin/tools/` |
| 新增 Skill | 声明式 `plugins/{id}/plugin.toml + workflow.toml + prompts/` 或 Rust `plugin/builtin/skills/` |
| 新增 LLM Provider | `llm/providers/` 新建 → `router.rs` 注册 |
| 新增 IPC 命令 | `commands/*.rs` → `lib.rs` 注册 → `src/lib/tauri.ts` wrapper |
| 新增前端组件 | 遵循 `docs/visual-standard.md` Design Tokens |
| 修改分析流程 | 编辑 `plugins/comp-analysis/prompts/` |

## 数据存储

| 数据 | 位置 |
|------|------|
| 应用数据 | `~/Library/Application Support/com.aijia.app/` (macOS) |
| 会话 + 消息 | `{base_dir}/conversations/{id}/` |
| 设置 + API Key | `{base_dir}/config.json` (AES-256-GCM 加密) |
| 云端认证 | `{base_dir}/config.json` key `cloud_auth` |
| 工作目录 | `~/.renlijia/` |
| 插件 | `{resource_dir}/plugins/` |

## 命名约定

| 类型 | 风格 | 示例 |
|------|------|------|
| Rust 模块 | snake_case | `file_manager.rs` |
| React 组件 | PascalCase | `AiBubble.tsx` |
| IPC 命令 | snake_case | `send_message` |
| 前端 IPC | camelCase | `sendMessage()` |
| JSON 字段 | camelCase | `#[serde(rename_all = "camelCase")]` |
| CSS 变量 | `--color-*` | `var(--color-primary)` |
