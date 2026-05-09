# RichComposer + Tiptap 输入框重设计

日期：2026-05-07
范围：前端首页输入框与 Chat 页面输入框；Tiptap 编辑器底座；附件全量粘贴、拖放、选择与提交兼容层。

## 背景与目标

当前首页和 Chat 页面已经共用 `ChatComposerCompact`，但输入核心仍是 `textarea`：

- 附件通过外部 `pendingFiles` 队列和 `PendingAttachmentChips` 显示在输入区上方。
- 粘贴附件只能追加到队列，不能表达“插入到当前光标后”的语义。
- 未来 slash command、技能触发、模板、引用等能力缺少统一编辑器扩展点。
- 首页和 Chat 页面共享输入壳，但页面各自维护 `value`、`pendingFiles`、paste/drop/pick 连接逻辑，长期容易漂移。

本设计将输入框升级为基于 Tiptap 的共享 `RichComposer`。首期目标：

1. 首页和 Chat 页面统一接入同一个 `RichComposer`。
2. 附件作为正文里的紧凑行内 token 插入当前光标后。
3. 全量粘贴能力进入 Tiptap：文件/文件夹、本地 URI、Finder/Explorer 文件引用、截图/复制图片二进制、普通文本，以及混合粘贴。
4. 发送第一阶段保持后端协议兼容：文本中生成 `[附件: fileName]` 占位符，同时继续传现有 `attachments` 数组。
5. slash 能力本期只预留扩展接口，不实现 `/` 菜单和命令搜索 UI。

## 已确认决策

- 使用官方 Tiptap 依赖：`@tiptap/react`、`@tiptap/starter-kit`，必要时引入 `@tiptap/extension-placeholder`。
- 抽共享 `RichComposer`，首页和 Chat 页面都复用。
- 附件 token 采用紧凑行内 chip：小图标 + 文件名 + 删除按钮。
- 粘贴附件必须插入到当前光标后，而不是进入外部 pending chips 队列。
- 发送文本保留可读占位符，例如 `请分析 [附件: report.pdf]`。
- 混合粘贴优先保留剪贴板原始混合顺序；如果平台无法提供可靠顺序，需要使用可解释 fallback 并提示用户。
- 只附件提交时，发送文本使用附件占位符本身，不再额外补 `请分析附件` 默认文案。

## 非目标

- 不改后端消息协议。
- 不做 slash command 菜单、搜索和选择交互。
- 不做消息展示区的富文本回放；发送后的用户消息仍按现有消息模型展示。
- 不改变技能弹窗的数据来源和业务语义。
- 不引入复杂富文本能力，如标题、列表、表格、粗体工具栏等；输入框仍以聊天文本为主。
- 不做无关的设计系统重构。

## 当前相关代码

页面与输入组件：

- `src/features/home/HomePage.tsx`
- `src/components/home/HomeTaskComposerCard.tsx`
- `src/features/chat/ChatPage.tsx`
- `src/components/chat-scene/ChatBottomArea.tsx`
- `src/components/chat-scene/ChatComposerCompact.tsx`
- `src/components/chat/PendingAttachmentChips.tsx`

附件、粘贴、拖放和发送链路：

- `src/hooks/useComposerPaste.ts`
- `src/hooks/useChatAttachments.ts`
- `src/hooks/useDragDropListener.ts`
- `src/stores/dropInbox.ts`
- `src/hooks/useChat.ts`
- `src/lib/tauri.ts`
- `src/types/message.ts`

主要受影响测试：

- `src/hooks/useComposerPaste.test.tsx`
- `src/components/chat-scene/__tests__/ChatBottomArea.test.tsx`
- `src/components/home/__tests__/HomeTaskComposerCard.test.tsx`
- `src/components/chat-scene/__tests__/ChatComposerCompact.test.tsx`
- `src/features/chat/ChatPage.test.tsx`
- `src/features/home/HomePage.test.tsx`
- `src/hooks/useChat.test.ts`

## 架构设计

新增共享输入组件族，建议目录：

```text
src/components/rich-composer/
  RichComposer.tsx
  AttachmentTokenView.tsx
  attachmentTokenExtension.ts
  pastePipeline.ts
  serializer.ts
  types.ts
  index.ts
```

职责边界：

- `RichComposer`
  - 渲染输入框外壳、Tiptap `EditorContent`、按钮区、tips、项目按钮、技能按钮、发送/停止按钮。
  - 管理 editor 生命周期、IME 状态、Enter/Shift+Enter 行为、禁用状态、提交清空策略。
  - 将 picker/drop/paste 的附件结果插入当前 selection。
  - 通过 serializer 对页面输出 `{ text, attachments, isEmpty }`。
- `AttachmentTokenView`
  - 渲染紧凑行内附件 chip。
  - 支持整体选中、删除按钮、按文件类型显示图标。
- `attachmentTokenExtension`
  - 定义 Tiptap inline atom node。
  - 保存附件 attrs。
  - 提供 `insertAttachmentToken(s)` command。
- `pastePipeline`
  - 解析 clipboard/drop/picker 输入。
  - 输出有序 fragments，并负责保存剪贴板图片到 tmpImage。
- `serializer`
  - 遍历 Tiptap JSON，生成兼容文本和 attachments 数组。
- 页面适配层
  - `HomeTaskComposerCard` 保留创建会话、工作区选择、授权目录、切路由逻辑。
  - `ChatBottomArea` 保留当前会话发送、流式停止、底部 tips、focus 逻辑。

`ChatComposerCompact` 迁移后应删除，或仅作为短期薄 wrapper。推荐直接由 `RichComposer` 替代，避免长期存在两个 composer 概念。

## Tiptap 文档模型

首期 schema 保持小而稳定：

- `doc`
- `paragraph`
- `text`
- `hardBreak`
- `attachmentToken`

`attachmentToken` 是 inline atom node，不允许用户编辑内部文字，只允许整体选择、删除、复制、剪切、移动。建议 attrs：

```ts
type ComposerAttachmentToken = {
  id: string
  fileName: string
  path: string
  kind: 'file' | 'folder' | 'image'
  fileType: 'image' | 'excel' | 'word' | 'pdf' | 'json' | 'csv' | 'folder'
  fileSize: number
  mimeType?: string
  source: 'picker' | 'paste' | 'drop' | 'clipboard-image'
}
```

实现要求：

- token 必须插入当前 selection 后。
- 多个 token 连续插入时保持 fragment 顺序。
- token 前后由插入命令负责补必要空格，避免文本和 chip 粘连。
- Backspace/Delete 删除 token 后，提交 payload 中也必须消失。
- token 删除按钮应调用 Tiptap command 删除对应 node，而不是只隐藏 UI。
- slash 本期只保留 extension point，例如 `composerExtensions?: Extension[]` 或内部 command registry，不注册真实菜单。

## 粘贴与附件管线

所有粘贴、拖放、选择附件的结果都进入 editor 文档，不再进入外部 pending chips 队列。

### 普通文本

- 如果剪贴板只有普通文本或 HTML 文本，不含附件候选，则不拦截，交给 Tiptap 默认粘贴。
- Tiptap 默认粘贴应被约束在聊天输入 schema 内，避免引入复杂富文本结构。

### 文件、文件夹、本地 URI

支持来源：

- Finder / Explorer 复制的文件和文件夹引用。
- `text/uri-list`。
- `public.file-url` 等平台文件 URL 类型。
- Tauri 原生 `readClipboardFilePaths()` 返回的路径。

处理要求：

- 继续使用现有路径安全过滤逻辑，拒绝磁盘根目录、系统目录、危险路径和不可支持项目。
- 继续保留最多 50 个粘贴路径上限，超出部分 toast 提示。
- 文件夹作为 `kind: 'folder'`、`fileType: 'folder'` token 插入；不在本期自动授权文件夹。

### 截图和复制图片二进制

- 必须支持 `clipboardData.items` 中的 image blob。
- 图片 blob 调用现有 `saveClipboardImageToTmp()` 落盘到 tmpImage。
- 落盘成功后生成 `source: 'clipboard-image'` 的 image token。
- 落盘失败必须 toast 提示，并且不插入损坏 token。

### 混合粘贴顺序

目标是尽力保留剪贴板原始顺序，输出 fragments：

```ts
type PasteFragment =
  | { type: 'text'; text: string }
  | { type: 'hardBreak' }
  | { type: 'attachment'; attachment: ComposerAttachmentToken }
```

优先顺序策略：

1. 如果剪贴板 HTML 或 item 列表能可靠表达文本、图片、文件引用顺序，则按该顺序生成 fragments。
2. 如果平台只提供文本和独立文件列表，无法可靠表达混合顺序，则使用 fallback：文本按原始文本插入，附件 token 插到本次粘贴范围末尾。
3. fallback 必须可解释；必要时 toast 提示“已按系统提供的剪贴板顺序插入附件”。

### Picker 和 Drop

- picker 选中文件后，直接调用 editor command 在当前 selection 插入 token。
- drop inbox 被首页或 Chat 页消费后，直接插入当前 editor selection。
- 如果 editor 未 focus 或没有有效 selection，插入文档末尾。
- 保留当前 `useDragDropListener` + `dropInbox` 的全局接收模型，只改变 consumer：由 append pending files 改为 insert editor tokens。

## 提交序列化

`serializer` 遍历 Tiptap JSON，输出：

```ts
type RichComposerSubmitPayload = {
  text: string
  attachments: PendingFileInfo[]
  isEmpty: boolean
}
```

规则：

- `text` 节点按原样拼接。
- `hardBreak` 转 `\n`。
- 段落之间转 `\n`。
- `attachmentToken` 转 `[附件: fileName]`。
- attachments 数组按 token 在文档中的出现顺序收集。
- 如果同一个附件 token 被复制多次，文本中保留多处占位符；attachments 是否去重由实现阶段根据后端能力决定。默认建议按 `id` 去重传输，避免重复处理同一路径。
- `isEmpty` 只在没有文本且没有 token 时为 true。
- 只附件提交时，文本就是一个或多个附件占位符，例如 `[附件: a.pdf] [附件: b.png]`。

页面层不再拼接默认附件文案。`RichComposer` 应在 payload 为空时阻止提交。

## `RichComposer` API

建议 props：

```ts
type RichComposerSubmitPayload = {
  text: string
  attachments: PendingFileInfo[]
  isEmpty: boolean
}

type ComposerSkillCommand = {
  command: string
  label: string
  id?: string
}

type RichComposerProps = {
  placeholder: string
  disabled?: boolean
  isStreaming?: boolean
  autoFocus?: boolean
  initialText?: string
  clearOnSubmit?: boolean

  onSubmit: (payload: RichComposerSubmitPayload) => void | Promise<void>
  onStop?: () => void

  topSlot?: React.ReactNode
  tips?: React.ReactNode

  onOpenSkill?: () => void
  skillCommand?: ComposerSkillCommand | null
  onClearSkillCommand?: () => void

  projectLabel?: string
  onPickProject?: () => void
  showProjectButton?: boolean
}
```

行为：

- Enter 提交。
- Shift+Enter 插入换行。
- IME composition 期间 Enter 不提交。
- `isStreaming` 时发送按钮变停止按钮，点击调用 `onStop`。
- `disabled` 或内部提交中时不允许重复提交。
- `clearOnSubmit` 为 true 时，提交成功后清空 editor；提交失败保留内容。
- `initialText` 用于 prefill，插入后 focus 到末尾。

## 页面接入

### `ChatBottomArea`

保留：

- `activeConversationId`
- `isSending`
- `isStreaming`
- `stopCurrentStream`
- `sendUserMessage`
- bottom tips
- conversation/focus 语义
- `SkillPopover` 的 open/pick/close 状态

删除或迁移：

- 本地 `input` 状态。
- 本地 `pendingFiles` 状态。
- textarea ref 和高度自适应逻辑。
- `useComposerPaste` 旧 hook 接入。
- append pending files 的 drop inbox consumer。

提交：

```ts
await sendUserMessage(payload.text, payload.attachments)
```

### `HomeTaskComposerCard`

保留：

- `displayWorkspace`
- `selectedWorkspace`
- `pickLocalDirectory`
- `getDefaultFolder`
- `authorizeLocalDirectory`
- `createConversation`
- 路由切换到新 chat
- `SkillPopover` 的 open/pick/close 状态

删除或迁移：

- 本地 `value` 状态。
- 本地 `pendingFiles` 状态。
- 旧 paste/drop/pick append 队列。

提交顺序不变：

1. 阻止空 payload 和重复提交。
2. `createConversation()`。
3. 写入乐观 conversation，切 active conversation，切路由。
4. 对非默认工作区执行 `authorizeLocalDirectory()`。
5. 调用 `sendUserMessage(payload.text, payload.attachments)`。

## 兼容行为

必须保留：

- 文本 + attachments 数组双轨发送。
- 只附件也能发送。
- 首页项目选择和工作目录授权流程。
- Chat 页流式停止按钮和发送提示。
- 纯文本粘贴直接进入正文，不误判为附件。
- 拖放附件体验。
- 附件不预上传，发送时继续按本地路径交给现有后端链路处理。
- 危险路径过滤和 toast 提示。
- 技能按钮和现有 `SkillPopover`。

## 测试策略

### 核心单测

`serializer`：

- 文本 + token + 文本输出 `[附件: fileName]`。
- 多 token 按文档顺序输出。
- `hardBreak` 和段落换行正确。
- attachments 数组按 token 顺序收集。
- 删除 token 后 payload 不再包含该附件。

`pastePipeline`：

- 普通文本不拦截。
- 文件路径粘贴插入 token。
- 文件夹粘贴插入 folder token。
- 图片 blob 粘贴调用 `saveClipboardImageToTmp()` 后插入 image token。
- 混合文本/附件按可用顺序生成 fragments。
- 平台无法表达顺序时 fallback，并 toast 提示。
- 危险路径和不可支持项目不会插入 token。

editor commands：

- `insertAttachmentTokensAtSelection()` 插入当前光标后。
- editor 未 focus 时插入末尾。
- Backspace/Delete 删除 token 后 serializer 输出更新。

### 共享组件测试

`RichComposer`：

- 渲染 placeholder、技能按钮、项目按钮、tips。
- Enter 提交，Shift+Enter 换行。
- IME 期间 Enter 不提交。
- 只附件提交输出占位符文本和 attachments。
- `isStreaming` 时发送按钮变停止按钮。
- `disabled` / 提交中防重复提交。
- `clearOnSubmit` 成功清空、失败保留。

### 页面测试

`ChatBottomArea`：

- 当前会话直接发送 payload。
- 流式停止行为保留。
- prefill 插入 editor 并 focus。
- drop inbox 被消费后插入 token，而不是 pending chips。

`HomeTaskComposerCard`：

- 创建会话、切路由、授权工作区顺序不变。
- payload attachments 继续传给 `sendUserMessage`。
- 项目按钮仍能选择工作区。
- 只附件提交不再额外补 `请分析附件`。

旧测试迁移：

- `useComposerPaste.test.tsx` 迁移为新 paste pipeline 测试；如果旧 hook 删除，则同步删除旧测试。
- `ChatComposerCompact.test.tsx` 迁移为 `RichComposer` 测试；如果旧组件删除，则同步删除旧测试。

## 实施分期建议

1. 引入 Tiptap 依赖，新增 `rich-composer` 目录和纯函数 serializer 测试。
2. 实现 `attachmentToken` extension 和 `RichComposer` 基础输入/提交/IME/Enter 行为。
3. 实现 picker/drop 直接插入 token，替代 pending chips 队列。
4. 实现全量 paste pipeline：文件/URI/路径/图片 blob/混合顺序/fallback/toast。
5. 接入 `ChatBottomArea`，迁移测试。
6. 接入 `HomeTaskComposerCard`，迁移测试。
7. 删除或收敛 `ChatComposerCompact`、旧 `useComposerPaste` 和不再使用的 `PendingAttachmentChips` 调用点。

## 风险与应对

- Tiptap 在 jsdom 下测试 selection 行为有限：把序列化和 paste pipeline 抽纯函数，组件层只测关键交互。
- 剪贴板混合顺序跨平台不稳定：实现“尽力保留 + fallback + toast”，不要虚假承诺所有平台都能精确还原。
- macOS WebKit 文件剪贴板曾有读取 `clipboardData.items/types` 卡顿风险：保留 capture snapshot 思路，并在 Tiptap paste handler 中避免不必要读取。
- 图片 blob 落盘是异步：插入 token 前可以先等待保存完成；首期不做 optimistic loading token，避免提交到不存在路径。
- 页面业务逻辑容易被共享组件吞掉：`RichComposer` 只输出 payload，不创建会话、不授权工作区、不访问 chat store。

## 验收标准

- 首页和 Chat 页面都使用同一个 `RichComposer`。
- 在任意光标位置粘贴/选择/拖放附件，附件 token 插入该位置。
- 粘贴截图或复制图片后，图片保存到 tmpImage 并插入 image token。
- 普通文本粘贴不受附件逻辑影响。
- 混合粘贴按可用顺序插入；无法保证顺序时有 fallback 提示。
- 提交 payload 的 `text` 含 `[附件: fileName]` 占位符，`attachments` 继续传现有数组。
- Enter、Shift+Enter、IME、流式停止、首页创建会话和工作区授权行为保持正确。
- 本期没有实现 slash 菜单，但代码中有明确扩展点。
