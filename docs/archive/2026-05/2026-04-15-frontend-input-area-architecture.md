# lotus-app 前端输入区域架构速查

## 组件结构

| 模块 | 文件 | 关键函数/导出 |
|------|------|--------------|
| 输入框 UI | `src/components/layout/InputBar.tsx` | `handleSend()`, `handleSendButtonClick()` |
| 发送逻辑 Hook | `src/hooks/useChat.ts` | `sendMessage(conversationId, text, fileIds)` |
| 文件上传 Hook | `src/hooks/useFileUpload.ts` | `useFileUpload()` |
| 附件 UI | `src/components/chat/FileAttachmentChip.tsx` | 文件附件展示芯片 |
| 状态管理 | `src/stores/chatStore.ts` | streaming actions, per-conversation state |
| IPC 层 | `src/lib/tauri.ts` | `sendMessage` Tauri invoke |

## 数据流

```
用户输入文字 / 上传文件
  ↓
InputBar.tsx
  ├── handleSend()            — 文字发送入口
  └── useFileUpload()         — 文件上传入口
        ↓
useChat.ts
  └── sendMessage(conversationId, text, fileIds)
        ↓
src/lib/tauri.ts
  └── invoke('send_message', { ... })   — Tauri IPC
        ↓
Rust 后端
  └── transport/tauri_commands/chat.rs → SessionRuntime
```

## 输入框状态

- placeholder 通过 i18n 切换：
  - 普通状态：`t('inputBar.placeholder')` → "随时提问，或上传文件让我分析..."
  - 有文件时：`t('inputBar.placeholderWithFile')`
- `attachmentBusy`：`isUploading || isAuthorizingDirectory` 时禁用附件按钮
- 发送按钮：输入非空或有附件时可点击

## chatStore 分层

```
chatStore (src/stores/chatStore.ts)
  ├── Legacy actions          — delegate to per-conversation with activeConversationId
  ├── Per-conversation streaming actions — 流式消息更新
  └── Legacy actions          — 兼容层
```

chatStore 本身不直接定义 `sendMessage`，由 `useChat.ts` hook 封装 Tauri IPC 调用。
