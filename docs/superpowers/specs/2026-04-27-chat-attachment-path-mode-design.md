# Chat 附件路径化模式设计规格

> **状态**：设计中（用于 TDD 落地）
> **日期**：2026-04-27
> **作者**：oayzz + Codex
> **范围**：聊天输入区附件交互、前端附件数据结构、Tauri IPC、Turn 请求、剪贴板图片落盘。
> **关联**：
> - `docs/superpowers/specs/2026-04-23-chat-bottom-area-design.md`
> - `/Users/oayzz/project/qob/docs/superpowers/specs/2026-03-30-attachment-alignment-design.md`
> - `/Users/oayzz/project/qob/packages/renderer/src/features/chat/ChatInput.tsx`

## 1. 背景

当前 lotus 的聊天附件链路是“上传模型”：

- 前端通过 `useFileUpload` 调系统文件选择器
- 选中的文件被复制到 workspace `uploads/`
- 前端拿到 `fileId`
- `send_message` 只透传 `fileIds`
- 后端在构建用户消息上下文时，从 file store 反查 uploaded file metadata

这和当前产品目标不一致。新的需求不是“上传文件到工作区”，而是“把用户明确选择/粘贴的本地路径作为当前消息附件附加给运行时”。

## 2. 本轮目标

1. `+` 按钮点击后只允许选择**文件**，不出现二次菜单。
2. 文件夹不通过按钮选择，只通过**复制/黏贴路径**进入附件池。
3. 文件、文件夹、已带本地路径的图片，都统一作为 **path-based attachment** 进入当前消息。
4. 只有“剪贴板图片没有现成文件路径”时，才落盘到 `~/.renlijia/conversations/<conversationId>/attachments/clipboard/`，再作为普通路径附件处理。
5. 输入区不再显示“已连接本地目录”状态条。
6. `WorkspaceFirst.integration.test.tsx` 不再把聊天输入区的目录状态文案当成测试目标。

## 3. 非目标

- 不在本轮重做完整附件预览系统。
- 不在本轮实现拖拽文件/文件夹（可后续补）。
- 不在本轮重做授权工作目录（workspace-first）模型。
- 不在本轮移除旧 `upload_file` 后端命令；仅聊天输入链路不再依赖它。
- 不在本轮修改 report block / markdown 表格之外的渲染体系。

## 4. 目标交互

### 4.1 `+` 按钮

- 点击 `+` 按钮，只打开系统文件选择器。
- 允许多选文件。
- 不允许通过此入口选目录。
- 选择完成后，在输入框上方显示附件 chips。

### 4.2 黏贴

聊天输入框黏贴时按如下优先级处理：

1. 若剪贴板内存在 `File`/blob：
   - 若该项有可解析的本地路径，按普通路径附件处理。
   - 若该项是图片 blob 且无本地路径，落盘到会话目录，再作为路径附件处理。
2. 若剪贴板文本可解析出绝对路径：
   - 文件路径 -> file attachment
   - 文件夹路径 -> folder attachment
3. 否则回退为普通文本黏贴。

### 4.3 附件展示

- 统一显示在输入框上方。
- 只显示 chip：图标 + 文件名/目录名 + 删除按钮。
- 不显示“已连接本地目录”提示卡。

## 5. 数据模型

前端把当前待发送附件从 `PendingFileInfo` 改成 `PendingAttachment`：

```ts
interface PendingAttachment {
  id: string
  fileName: string
  path: string
  kind: 'file' | 'folder' | 'image'
  fileType: 'image' | 'document' | 'code' | 'data' | 'folder' | 'other'
  fileSize?: number
  mimeType?: string
  source: 'picker' | 'paste' | 'drop' | 'clipboard-image'
}
```

设计原则：

- `path` 是主键语义，不再依赖 `fileId`
- `kind` 用于行为判断（如 folder）
- `fileType` 用于展示图标/文案
- `source` 仅用于调试和后续埋点，不影响发送协议

## 6. IPC 与运行时协议

### 6.1 前端 → Tauri

`sendMessage()` 从：

```ts
sendMessage(conversationId, content, fileIds)
```

变成：

```ts
sendMessage(conversationId, content, attachments)
```

其中 `attachments` 是结构化 path descriptors。

### 6.2 ChatTurnRequest

`ChatTurnRequest` 新增：

```rust
pub attachments: Vec<ChatAttachmentRef>
```

其中：

```rust
pub struct ChatAttachmentRef {
    pub name: String,
    pub path: String,
    pub kind: ChatAttachmentKind,
    pub mime_type: Option<String>,
    pub size: Option<u64>,
}
```

`file_ids` 保留一段兼容期，但聊天输入链路不再写入它。

## 7. 运行时语义

路径型附件不是“上传文件”，而是“本轮用户显式授权给 agent 读取的路径”。

因此运行时要区分两类来源：

- 会话级授权目录（authorized workspace）
- 当前消息显式附加路径（attachments）

本轮最小实现要求：

1. 持久化 user message 时保存附件元信息，用于 transcript 回显。
2. 构建用户消息内容时把附件路径提示进 LLM content。
3. Query/Tool runtime 能拿到本轮 attachment paths，作为只读允许路径的一部分。

## 8. 剪贴板图片落盘

### 8.1 保存位置

```text
~/.renlijia/conversations/<conversationId>/attachments/clipboard/
```

文件名规则：

```text
clipboard-<timestamp>-<shortid>.png
```

### 8.2 新 IPC

新增类似命令：

```rust
save_clipboard_image_attachment(conversation_id, bytes, mime_type) -> SavedAttachment
```

返回：

```ts
{
  fileName: string,
  path: string,
  fileSize: number,
  mimeType: string,
}
```

## 9. 测试边界

### 9.1 前端

- `ChatBottomArea.test.tsx`
  - `+` 按钮只触发文件选择
  - 不显示“已连接本地目录”状态条
  - 黏贴绝对文件路径 -> 出现 file chip
  - 黏贴目录路径 -> 出现 folder chip
  - 黏贴图片 blob -> 调保存 IPC -> 出现 image chip
  - 发送消息时把 attachments 传给 `sendUserMessage`

- `WorkspaceFirst.integration.test.tsx`
  - 只验证授权/撤销链路
  - 不验证聊天输入区状态文案

### 9.2 后端

- `ChatTurnRequest` / `build_user_content_json` / transport send_message：
  - 能透传 structured attachments
- 用户消息持久化：
  - transcript 中带附件元信息
- 会话目录落盘：
  - clipboard image 会写入 conversation attachments 子目录

## 10. 对 qob 的借鉴边界

借鉴：

- `ChatInput.tsx` 的 `handlePaste` / 路径解析 / 附件 chip 交互
- 附件交互统一入口的思路

不直接照搬：

- qob 里对文件内容/base64 的依赖
- qob 的 file upload / file_id 语义

lotus 本轮采用的是：**事件入口参考 qob，协议语义改为 path-based attachment**。
