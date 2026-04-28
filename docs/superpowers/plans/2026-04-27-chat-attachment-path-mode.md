# Chat 附件路径化模式实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use verification-before-completion before claiming completion or committing.

**Goal:** 将聊天输入区附件从 `upload + fileId` 模式迁移到 `path-based attachment` 模式；`+` 按钮只选文件，文件夹通过黏贴路径进入，剪贴板图片在无本地路径时落盘到会话目录后作为普通路径附件处理。

**Architecture:** 前端 `ChatBottomArea` 不再走 `useFileUpload` 的上传语义，而是维护 `PendingAttachment[]`。`sendMessage` IPC 改为透传 structured attachments。后端 `ChatTurnRequest` 持有附件路径描述，持久化 user transcript 并把路径提示注入 turn 上下文；剪贴板图片通过新 Tauri 命令落盘到 `~/.renlijia/conversations/<id>/attachments/clipboard/`。

**Tech Stack:** React 19, Zustand, Vitest, Tauri IPC, Rust runtime, per-conversation storage.

---

## Scope

- Do remove ChatBottomArea 中“已连接本地目录”状态卡。
- Do change `WorkspaceFirst.integration.test.tsx` so it no longer asserts chat-input workspace copy.
- Do make `+` button open file picker only.
- Do support pasted file paths / folder paths / clipboard images.
- Do not keep the two-option attachment popup.
- Do not rely on `upload_file` for chat composer attachments.

## Task 1: 收缩现有测试边界并去掉无用 UI

**Files:**
- Modify: `src/components/settings/WorkspaceFirst.integration.test.tsx`
- Modify: `src/components/chat-scene/ChatBottomArea.tsx`
- Modify: `src/components/chat-scene/__tests__/ChatBottomArea.test.tsx`

**Acceptance:** 聊天输入区不再渲染“已连接本地目录”状态卡；workspace-first 测试只验证授权链路，不再要求输入区状态文案。

## Task 2: 前端附件模型改为 PendingAttachment

**Files:**
- Modify: `src/hooks/useChat.ts`
- Modify: `src/components/chat-scene/ChatBottomArea.tsx`
- Add/Modify: `src/hooks/useChatAttachments.ts`（如需要）
- Test: `src/components/chat-scene/__tests__/ChatBottomArea.test.tsx`
- Test: `src/hooks/useChat.skill.test.ts`（若涉及 send payload）

**Acceptance:** `ChatBottomArea` 维护 path-based attachments；发送时 `sendUserMessage(text, attachments)` 不再使用 uploaded file ids。

## Task 3: 黏贴路径 / 黏贴图片 TDD

**Files:**
- Modify: `src/components/chat-scene/ChatBottomArea.tsx`
- Modify: `src/lib/tauri.ts`
- Test: `src/components/chat-scene/__tests__/ChatBottomArea.test.tsx`

**Acceptance:**
- 黏贴绝对文件路径会生成 file chip
- 黏贴目录路径会生成 folder chip
- 黏贴无路径图片 blob 会调用保存 IPC，成功后生成 image chip

## Task 4: Tauri IPC 与 ChatTurnRequest 改造

**Files:**
- Modify: `src/lib/tauri.ts`
- Modify: `src-tauri/src/commands/chat.rs`
- Modify: `src-tauri/src/transport/tauri_commands/chat.rs`
- Modify: `src-tauri/src/runtime/chat/chat_turn_driver.rs`
- Test: Rust 单元测试就近补在 `chat_turn_driver.rs` / `chat.rs`

**Acceptance:** `send_message` 能接收 structured attachments，`ChatTurnRequest` 保存这些附件描述，旧 file-id-only 发送路径不再是聊天输入主路径。

## Task 5: 剪贴板图片落盘到会话目录

**Files:**
- Modify: `src/lib/tauri.ts`
- Add/Modify: `src-tauri/src/commands/file.rs` 或新附件命令文件
- Modify: `src-tauri/src/lib.rs`（命令注册）
- Test: Rust 单元测试覆盖 conversation attachments 路径落盘

**Acceptance:** 剪贴板图片可写入 `~/.renlijia/conversations/<conversationId>/attachments/clipboard/`，返回路径元信息给前端。

## Task 6: 用户消息 transcript 与 turn context 对齐

**Files:**
- Modify: `src/types/message.ts`
- Modify: `src/hooks/useChat.ts`
- Modify: `src-tauri/src/runtime/chat/chat_turn_driver.rs`
- Modify: `src-tauri/src/transport/tauri_commands/chat.rs`
- Test: 相关前端/后端测试

**Acceptance:** 用户消息可见 transcript 正确显示附件 chip 信息；turn 上下文能看到本轮 attachment path 提示。

## Verification

```bash
pnpm test src/components/settings/WorkspaceFirst.integration.test.tsx src/components/chat-scene/__tests__/ChatBottomArea.test.tsx src/hooks/useChat.skill.test.ts
pnpm test src/features/auth/AuthGate.integration.test.tsx src/lib/markdown.test.ts src/components/chat-scene/__tests__/AssistantMarkdown.test.tsx
cargo test --manifest-path src-tauri/Cargo.toml chat_turn_driver -- --nocapture
```

如 Cargo 定向测试范围过大，优先改用更小的 test filter 或 `cargo check` + 就近 unit tests 组合验证。
