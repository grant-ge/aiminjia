# 对话导出 — 设计文档

> **状态**：设计稿 · 待用户 review
> **日期**：2026-06-03
> **作者**：Codex（基于与项目 owner 的头脑风暴）
> **关联工件**：
> - 前端入口：`src/features/chat/ChatPage.tsx`、`src/components/shell/ChatTopBar.tsx`
> - IPC 真相源：`src/lib/tauri.ts`
> - 消息存储：`src-tauri/src/storage/file_store/messages.rs`
> - Diagnostics 指南：`docs/harness/diagnostics-log-debugging-guide.md`
> - 本机日志目录：`~/.renlijia/logs/`

---

## 1. 背景与目标

当前用户遇到对话卡住、工具失败、网关异常、前端未刷新等问题时，研发需要同时拿到对话内容、消息原始结构、diagnostics 事件、运行日志和网关链路日志。现状这些材料分散在会话存储和 `~/.renlijia/logs/` 下，用户难以手动收集。

本功能在聊天页右上角提供一个轻量入口，让用户生成一个本地 zip。产品外层表现为“导出对话”，避免把用户吓退；zip 内核按研发排查设计，默认包含完整日志材料。

目标：

- 用户能在当前对话一键生成 zip，并在完成后打开所在文件夹。
- 用户打开 zip 后能通过 HTML 看到可读的对话过程。
- 研发拿到 zip 后能快速复盘会话、工具、事件、日志和网关链路。
- 导出过程有明确进度、失败提示和可重试路径。

---

## 2. 非目标

- 不做云端上传，不自动把 zip 发给研发。
- 不做跨会话批量导出。
- 不新增日志采集类型，只汇总当前已有本地文件和会话存储。
- 不把导出包作为正式审计归档或合规留存系统。
- 第一版不做细粒度脱敏开关；保留现有 diagnostics 脱敏逻辑，原始 `renlijia.log` / `gate.log` 按研发排查材料处理。

---

## 3. 用户体验

### 3.1 入口

在图示的聊天页顶栏右上角增加一个图标按钮。外显文案保持温和：

- 按钮 aria-label / tooltip：`导出对话`
- 弹窗标题：`导出对话`
- 不使用 `诊断包`、`研发日志` 等会降低用户点击意愿的入口文案

按钮位置复用 `ChatTopBar` 的右侧操作区，使用 `lucide-react` 图标和现有主题变量。

### 3.2 导出弹窗

点击后展示确认弹窗，文案强调“zip”和“本地文件”，不做强恐吓提示。

建议文案：

```text
导出对话

将生成一个 zip 文件，包含当前对话和相关运行信息，便于回顾过程或排查问题。

包含：对话内容、工具记录、运行日志、应用信息。
```

主按钮：`开始导出`
取消按钮：`取消`

### 3.3 进度与完成态

导出开始后弹窗进入进度态：

- `准备对话内容`
- `收集诊断事件`
- `写入运行日志`
- `生成 HTML`
- `压缩文件`

完成后显示：

- 主按钮：`打开所在文件夹`
- 次按钮：`复制路径`
- 辅助文案：`已生成 zip 文件，可在需要时发送给同事或研发。`

失败后显示错误摘要和 `重新导出`。

---

## 4. Zip 内容结构

第一版默认包含完整研发排查材料：

```text
aijia-conversation-export-{safe-title}-{yyyyMMdd-HHmmss}.zip
├── README.txt
├── conversation.html
├── diagnostics-summary.html
├── manifest.json
└── raw/
    ├── messages.jsonl
    ├── current-conversation-diagnostics.jsonl
    ├── recent-warn-error.jsonl
    ├── renlijia.log
    └── gate.log
```

### 4.1 README.txt

面向用户和研发，说明：

- 这是某次 AI 小家对话的本地导出。
- `conversation.html` 可直接打开查看对话过程。
- `raw/` 内为排查问题所需的原始材料。
- zip 不会自动上传，只有用户主动发送后他人才可看到。

### 4.2 conversation.html

面向用户可读：

- 展示会话标题、workspace、导出时间、应用版本。
- 按时间顺序展示 user / assistant / tool 消息。
- 工具调用以折叠块展示摘要、成功/失败状态和时间。
- 错误消息用明显但不过度警告的样式标出。
- 不依赖外部资源，离线可打开。

### 4.3 diagnostics-summary.html

面向研发快速扫描：

- 会话 ID、相关 runId 列表、消息数量。
- 当前会话 diagnostics 事件统计。
- warn/error/ok=false 列表。
- 工具失败、权限交互、streaming error、turn failed 等关键事件摘要。
- 指向 `raw/*.jsonl` 的文件名和用途说明。

### 4.4 manifest.json

机器可读元信息：

```json
{
  "schemaVersion": 1,
  "exportedAt": "2026-06-03T12:00:00+08:00",
  "app": {
    "name": "AI小家",
    "version": "x.y.z",
    "platform": "darwin",
    "arch": "arm64"
  },
  "conversation": {
    "id": "conversation-id",
    "title": "会话标题",
    "workspaceName": "lotus-app",
    "updatedAt": "..."
  },
  "logs": [
    {
      "name": "raw/renlijia.log",
      "sourcePath": "~/.renlijia/logs/renlijia.log",
      "sizeBytes": 123,
      "modifiedAt": "...",
      "included": true
    }
  ]
}
```

### 4.5 raw/messages.jsonl

来自当前会话 `messages.jsonl` 的完整消息记录，使用当前存储读路径得到的有序消息，便于研发复现消息结构、tool call、错误字段和 runId。

### 4.6 raw/current-conversation-diagnostics.jsonl

从所有 `~/.renlijia/logs/metrics*.jsonl` 中读取，过滤：

```jq
select(.category=="diagnostics" and .conversationId==$conversationId)
```

保留原始 JSONL 行，兼容历史 `\t✓` marker。

### 4.7 raw/recent-warn-error.jsonl

从所有 `~/.renlijia/logs/metrics*.jsonl` 中读取，过滤最近 24 小时内：

```jq
select(.category=="diagnostics" and (.level=="warn" or .level=="error" or .ok==false))
```

这份文件不只限当前会话，用于发现全局 auth、network、runtime、updater、diagnostics 自身异常。

### 4.8 raw/renlijia.log 与 raw/gate.log

默认完整打包：

- `renlijia.log`：Tauri/Rust 运行时日志、panic、runtime、工具、授权、事件转发等信息。
- `gate.log`：网关请求、provider/model、requestId、traceId、response chunk、stream close 等链路信息。

如果未来体积成为问题，再新增“截断策略”或“高级导出完整日志”选项；第一版按用户确认选择 A：默认完整包含。

---

## 5. 架构设计

### 5.1 前端组件

新增或改造：

- `ChatTopBar`：支持一个 `onExportConversation` 或通过 `trailing` 注入导出按钮。
- `ChatPage`：绑定当前 `conversationId`，调用导出 IPC，处理进度和完成态。
- `ConversationExportDialog`：负责确认、进度、完成、失败状态。

前端约束：

- 使用 `@/components/ui/button`、`@/components/ui/dialog`、toast 等现有组件。
- 图标使用 `lucide-react`。
- 颜色使用主题变量，不硬编码具体色值。

### 5.2 IPC

在 `src/lib/tauri.ts` 新增类型化封装：

```ts
export interface ExportConversationResult {
  zipPath: string
  fileName: string
  sizeBytes: number
}

export function exportConversation(conversationId: string): Promise<ExportConversationResult>
export function revealExportInFolder(path: string): Promise<void>
```

如果实现期希望展示细粒度进度，可增加事件：

```ts
TAURI_EVENTS.CONVERSATION_EXPORT_PROGRESS = 'conversation-export:progress'
```

事件 payload：

```ts
interface ConversationExportProgressPayload {
  conversationId: string
  stage: 'prepare' | 'diagnostics' | 'logs' | 'html' | 'zip'
  label: string
  current: number
  total: number
}
```

### 5.3 后端分层

新增 runtime/service 层而不是把业务写在 Tauri command：

```text
transport/tauri_commands/conversation_export.rs
  -> runtime/export/conversation_exporter.rs
      -> storage/file_store/messages.rs
      -> telemetry / logs reader
      -> html renderer
      -> zip writer
```

职责：

- Transport command：接收 `conversationId`，调用 exporter，返回结果，转发进度事件。
- Exporter：收集材料、渲染 HTML、生成 manifest、写临时目录、压缩 zip。
- Log reader：枚举 `~/.renlijia/logs/metrics*.jsonl`、`renlijia.log`、`gate.log`。
- HTML renderer：输出离线 HTML，做基本 HTML escape。

### 5.4 导出路径

建议输出到用户可访问的本地目录：

```text
~/.renlijia/exports/conversations/
```

每次导出创建临时目录：

```text
~/.renlijia/exports/conversations/tmp/{export-id}/
```

成功压缩后移动到：

```text
~/.renlijia/exports/conversations/aijia-conversation-export-{safe-title}-{yyyyMMdd-HHmmss}.zip
```

失败时清理临时目录。

---

## 6. 数据流

```text
用户点击导出
  -> 前端打开确认弹窗
  -> invoke('export_conversation', { conversationId })
  -> Tauri command 调 runtime exporter
  -> exporter 读取会话消息
  -> exporter 读取 metrics diagnostics
  -> exporter 复制 renlijia.log / gate.log
  -> exporter 渲染 README / HTML / manifest
  -> exporter 压缩 zip
  -> 返回 zipPath / sizeBytes
  -> 前端展示完成态
  -> 用户点击“打开所在文件夹”
```

---

## 7. 隐私与安全边界

本功能默认包含完整 `renlijia.log` 和 `gate.log`，因此它不是纯分享功能。产品文案不在入口过度强调，但 README 和导出弹窗应保持最低限度透明：

- 弹窗使用“相关运行信息/日志”描述。
- README 明确 `raw/` 为排查问题材料。
- 不自动上传，不自动发送。
- `manifest.json` 记录打包了哪些文件。
- HTML 渲染必须 escape 用户文本和模型输出，避免打开本地 HTML 时执行脚本。

---

## 8. 错误处理

- 会话不存在：返回清晰错误，前端提示“当前对话不存在或已被删除”。
- 消息读取失败：导出失败，不生成半成品 zip。
- metrics 不存在：继续导出，manifest 标注缺失。
- `renlijia.log` / `gate.log` 不存在：继续导出，manifest 标注缺失。
- 单个日志文件读取失败：继续导出其他文件，并在 `diagnostics-summary.html` 和 manifest 中标注。
- zip 写入失败：清理临时目录，前端显示错误并允许重试。
- 打开文件夹失败：zip 仍保留，前端显示路径并允许复制。

---

## 9. 测试计划

### 9.1 Rust

- exporter 在临时 storage 下能生成 zip，包含固定文件清单。
- messages.jsonl 顺序和字段保留。
- metrics shard 读取顺序正确，兼容 `\t✓` marker。
- `current-conversation-diagnostics.jsonl` 只包含当前 conversationId。
- `recent-warn-error.jsonl` 包含最近 24h 的 warn/error/ok=false。
- `renlijia.log` 和 `gate.log` 存在时被完整写入 raw。
- 缺失日志时导出成功且 manifest 标注缺失。
- HTML renderer 对 `<script>` 等内容做 escape。

### 9.2 前端

- ChatTopBar 展示导出按钮。
- 点击后弹窗展示确认内容。
- 成功后显示打开文件夹和复制路径按钮。
- 失败后显示错误和重试。
- 进度事件能更新进度态。

### 9.3 验证命令

实现后建议运行：

```bash
pnpm exec vitest run src/features/chat/ChatPage.test.tsx src/components/shell/ChatTopBar.test.tsx
pnpm build
cd src-tauri && cargo test conversation_export --tests --no-fail-fast
cd src-tauri && cargo check
```

---

## 10. 待实现决策

已确认：

- 外层入口叫“导出对话”，不显式叫诊断包。
- zip 默认完整包含 `renlijia.log` 和 `gate.log`。
- zip 需包含可读 HTML，同时保留 raw 研发材料。

实现期仍需确认：

- 是否新增独立 `ConversationExportDialog` 组件，还是直接放在 `ChatPage` 内。
- 导出进度使用后端事件还是前端本地阶段提示。
- zip writer 选择 Rust crate 现有依赖或新增依赖。
