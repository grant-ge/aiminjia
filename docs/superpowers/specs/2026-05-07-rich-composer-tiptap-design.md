# RichComposer + Tiptap 输入框重设计

日期：2026-05-07（2026-05-09 修订：富文本 schema + markdown 序列化 + user bubble markdown 渲染；2026-05-11 修订：Lotus Cloud Anthropic 图片通道收敛）
范围：前端首页 / Chat 页输入框；Tiptap 编辑器底座；附件粘贴-拖放-选择管线；提交 payload 升级为 markdown；user message bubble 渲染改造；Lotus Cloud Anthropic 图片理解通道。

## 背景与目标

当前首页和 Chat 页面共用 `ChatComposerCompact`，输入核心仍是 `textarea`：

- 附件通过外部 `pendingFiles` 队列和 `PendingAttachmentChips` 显示在输入区上方，不能表达"插入到当前光标后"。
- 未来 slash command / 引用 / 技能触发缺少统一编辑器扩展点。
- 首页和 Chat 页面各自维护 `value`、`pendingFiles`、paste/drop/pick 连接逻辑，容易漂移。
- 提交给后端 / LLM 的 user message 是纯文本字符串，图片附件只能以"路径 + 类型"文本提示发给模型，LLM 完全看不到像素。
- user bubble 手写 `ImageThumbnail` / `AttachmentIcon` / `whitespace-pre-wrap` 三件套，不支持富文本回显。

本次把输入框升级到基于 Tiptap 的共享 `RichComposer`，同时把提交协议、bubble 渲染、LLM 图片通道一起升级：

1. 首页和 Chat 页面统一接入同一个 `RichComposer`。
2. 附件作为正文里的紧凑行内 token 插入当前光标后。
3. 全量粘贴能力进入 Tiptap：文件/文件夹、本地 URI、Finder/Explorer 文件引用、截图/复制图片二进制、普通文本、HTML 富文本，以及混合粘贴。
4. 提交 payload 从 `text: string` 升级到 `markdown: string`；附件 token 以 `[附件: name](file://path)` / `![name](file://path)` 形式穿插在正文中。
5. user message bubble 改为 markdown 渲染，图片/附件以自定义 renderer 与正文穿插显示。
6. Lotus Cloud LLM 通道扩出 Anthropic Messages 多模态 content block；当前轮次的图片附件直接以 base64 进入 `/anthropic/v1/messages`。缺 Anthropic 路由、非云端、本地/custom/OpenAI-compatible 路径不在本期范围内。
7. slash 能力仍只预留扩展接口，本期不实现 `/` 菜单。

## 已确认决策

- 使用官方 Tiptap 依赖：`@tiptap/react`、`@tiptap/starter-kit`、`@tiptap/extension-link`、`@tiptap/extension-placeholder`。
- 抽共享 `RichComposer`，首页和 Chat 页面都复用。
- 附件 token 采用紧凑行内 chip：小图标 + 文件名 + 删除按钮。
- 粘贴附件必须插入到当前光标后，而不是进入外部 pending chips 队列。
- 提交 payload 字段为 `markdown`；附件 token 序列化为 `[附件: fileName](file://abs/path)`（非图片）或 `![fileName](file://abs/path)`（图片）。
- 混合粘贴优先保留剪贴板原始混合顺序；如果平台无法提供可靠顺序，使用可解释 fallback 并 toast 提示用户。
- 只附件提交时，markdown 就是一个或多个附件占位符本身，不再额外补"请分析附件"默认文案。
- user bubble 渲染只读 markdown 字符串，不再读 `StoredMessage.content.files` 数组；`files` 数组退给后端链路（build_llm_content / workspace 授权）用。
- user bubble 中附件 chip 的"视觉语义"放在 react-markdown 自定义 link renderer 里（`href.startsWith('file://')` → chip；否则普通链接）。
- 图片传给 AI 走真 multimodal：仅 Lotus Cloud 新桌面端路径生效，按 Anthropic `content[]` 的 `image.source.base64` 结构发送；缺 Anthropic 路由的模型对新桌面端不可用，不在客户端做 OpenAI 协议回退。
- 历史消息不重新塞 multimodal：仅当前轮的图进视觉通道；历史回放继续走文本提示。
- base64 不持久化：一次性进当轮 LLM 请求，结束即丢。
- 服务端请求体全局上限 10MB；客户端图片 guard 收敛为单图原始 bytes ≤ 3MB、单请求图片原始总量 ≤ 6MB、最多 4 张，格式白名单 `png/jpeg/webp/gif`；超限该图降级回 path + toast，不阻断整条消息。
- 输入框富文本 schema 采用"中档"：允许 bold / italic / inline code / strike / link / bulletList / orderedList / blockquote / codeBlock；不允许标题、表格、图片粘贴直贴（统一走附件 token）、颜色、行内 style。
- 不做工具栏 UI，富文本靠快捷键和跨 app 粘贴获得。
- lightbox / 全屏图片预览本期不做；点击图片沿用现有 `openLocalFile`。

## 非目标

- 不做 slash command 菜单、搜索和选择交互。
- 不做消息展示区的富文本回放 diff / 编辑历史；发送后的 user bubble 只读 markdown。
- 不改变技能弹窗的数据来源和业务语义。
- 不引入 heading / table / 行内颜色 / 工具栏。
- 不做图片 lightbox / 全屏预览。
- 不做 OCR / 视觉工具链 fallback（ChatGPT 式降级只做到"path 文本提示"这一级）。
- 不改后端消息存储 schema（仍存 `text` + `files[]`；`text` 字段内容从纯文本变成 markdown）。

## 当前相关代码

页面与输入组件：

- `src/features/home/HomePage.tsx`
- `src/components/home/HomeTaskComposerCard.tsx`
- `src/features/chat/ChatPage.tsx`
- `src/components/chat-scene/ChatBottomArea.tsx`
- `src/components/chat-scene/ChatComposerCompact.tsx`
- `src/components/chat/PendingAttachmentChips.tsx`

附件、粘贴、拖放、发送链路：

- `src/hooks/useComposerPaste.ts`
- `src/hooks/useChatAttachments.ts`
- `src/hooks/useDragDropListener.ts`
- `src/stores/dropInbox.ts`
- `src/hooks/useChat.ts`
- `src/lib/tauri.ts`
- `src/types/message.ts`

Bubble 与 markdown：

- `src/components/chat-scene/UserMessageBubble.tsx`（改造重点）
- `src/components/chat-scene/AssistantMarkdown.tsx`
- `src/components/chat-scene/markdown/`

后端 LLM 链路（multimodal 改造点）：

- `src-tauri/src/llm/streaming.rs`（ChatMessage 当前仍是文本，P7 只做 Lotus/Claude 发送前的 Anthropic 多模态扩展；不做 provider-wide content enum）
- `src-tauri/src/llm/providers/claude.rs` / `src-tauri/src/llm/providers/lotus.rs`（Lotus 复用 ClaudeProvider，序列化 Anthropic content blocks）
- `src-tauri/src/llm/max_tokens.rs` 旁边新增 `vision_support.rs`（只描述 Lotus Cloud + Anthropic 路由可用模型的视觉 allowlist）
- `src-tauri/src/runtime/chat/chat_turn_driver.rs`（构造 user message）
- `src-tauri/src/runtime/chat/history.rs`
- `src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs::build_llm_content`

主要受影响测试：

- `src/hooks/useComposerPaste.test.tsx`
- `src/components/chat-scene/__tests__/ChatBottomArea.test.tsx`
- `src/components/home/__tests__/HomeTaskComposerCard.test.tsx`
- `src/components/chat-scene/__tests__/ChatComposerCompact.test.tsx`
- `src/features/chat/ChatPage.test.tsx`
- `src/features/home/HomePage.test.tsx`
- `src/hooks/useChat.test.ts`

## 架构设计

新增共享输入组件族：

```text
src/components/rich-composer/
  RichComposer.tsx
  AttachmentTokenView.tsx
  attachmentTokenExtension.ts
  pastePipeline.ts
  serializer.ts              # Tiptap JSON → markdown
  types.ts
  index.ts
```

职责边界：

- `RichComposer`
  - 渲染输入外壳、Tiptap `EditorContent`、按钮区、tips、项目按钮、技能按钮、发送/停止按钮。
  - 管理 editor 生命周期、IME 状态、Enter/Shift+Enter 行为、禁用状态、提交清空策略。
  - 将 picker/drop/paste 的附件结果插入当前 selection。
  - 通过 serializer 对页面输出 `{ markdown, attachments, isEmpty }`。
- `AttachmentTokenView`
  - 渲染紧凑行内附件 chip。
  - 支持整体选中、删除按钮、按文件类型显示图标。
- `attachmentTokenExtension`
  - 定义 Tiptap inline atom node。
  - 保存附件 attrs。
  - 提供 `insertAttachmentToken(s)` command。
- `pastePipeline`
  - 解析 clipboard/drop/picker 输入。
  - 输出有序 fragments（文本 / HTML 片段 / hardBreak / attachment）。
  - 负责保存剪贴板图片到 tmpImage。
- `serializer`
  - 遍历 Tiptap JSON，生成 markdown 字符串和 attachments 数组。
- 页面适配层
  - `HomeTaskComposerCard`：保留创建会话、工作区选择、授权目录、切路由逻辑。
  - `ChatBottomArea`：保留当前会话发送、流式停止、底部 tips、focus 逻辑。

`ChatComposerCompact` 迁移完成后删除，不保留 wrapper。

## Tiptap 文档模型（中档富文本）

Schema：

- `doc`
- `paragraph`
- `text`
- `hardBreak`
- `blockquote`
- `codeBlock`（保留 `language` attr）
- `bulletList` / `orderedList` / `listItem`
- `attachmentToken`（inline atom，不允许编辑内部文字，只允许整体选择、删除、复制、剪切、移动）

Marks：

- `bold` / `italic` / `code`(inline) / `strike` / `link`

依赖：`@tiptap/starter-kit` 自带 paragraph / text / hardBreak / blockquote / codeBlock / list / bold / italic / code / strike；额外 `@tiptap/extension-link` 启用 link mark；`@tiptap/extension-placeholder` 做 placeholder。

不启用：heading / horizontalRule / table / image（图片只通过 attachmentToken 走附件通道，不允许 HTML `<img>` 直接进 schema）/ 任意行内 style / 颜色。

`attachmentToken` attrs：

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
- slash 本期只保留 extension point（例如 `composerExtensions?: Extension[]`），不注册真实菜单。
- 工具栏 UI 不做；富文本通过快捷键（Cmd+B / Cmd+I / Cmd+K link 等 StarterKit 默认）和跨 app 粘贴获得。

## 粘贴与附件管线

所有粘贴、拖放、选择附件的结果都进入 editor 文档，不再进入外部 pending chips 队列。

### PasteFragment 类型

```ts
type PasteFragment =
  | { type: 'html'; html: string }
  | { type: 'text'; text: string }
  | { type: 'hardBreak' }
  | { type: 'attachment'; attachment: ComposerAttachmentToken }
```

### 普通文本与 HTML 富文本

- 剪贴板只有普通文本时，不拦截，交给 Tiptap 默认粘贴。
- 剪贴板含 `text/html` 但不含文件附件候选时，交给 Tiptap schema filter 吃下：支持的 mark / 节点保留，其它（heading / table / inline style / color）降级成纯文本或对应可支持节点。
- HTML 粘贴必须被 schema 约束在聊天输入 schema 内，避免引入复杂富文本结构。
- 实测主流来源（飞书 / 钉钉 / Notion / 微信网页 / VS Code / 网页）并在 paste rule 里补必要适配。

### 文件、文件夹、本地 URI

支持来源（与现状一致）：

- Finder / Explorer 复制的文件和文件夹引用。
- `text/uri-list`。
- `public.file-url` 等平台文件 URL 类型。
- Tauri 原生 `readClipboardFilePaths()` 返回的路径。

处理要求：

- 继续使用现有路径安全过滤逻辑，拒绝磁盘根目录、系统目录、危险路径和不可支持项目。
- 继续保留最多 50 个粘贴路径上限，超出部分 toast 提示。
- 文件夹作为 `kind: 'folder'`、`fileType: 'folder'` token 插入；不在本期自动授权文件夹。

### 截图和复制图片二进制

- 支持 `clipboardData.items` 中的 image blob。
- 图片 blob 调用现有 `saveClipboardImageToTmp()` 落盘到 tmpImage。
- 落盘成功后生成 `source: 'clipboard-image'` 的 image token。
- 落盘失败 toast 提示，不插入损坏 token。

### 混合粘贴顺序

目标是尽力保留剪贴板原始顺序：

1. 如果剪贴板 HTML 或 item 列表能可靠表达文本、图片、文件引用顺序，按该顺序生成 fragments。
2. 如果平台只提供文本和独立文件列表，使用 fallback：文本按原始文本插入，附件 token 插到本次粘贴范围末尾。
3. fallback 必须可解释；必要时 toast 提示"已按系统提供的剪贴板顺序插入附件"。

### Picker 和 Drop

- picker 选中文件后，直接调用 editor command 在当前 selection 插入 token。
- drop inbox 被首页或 Chat 页消费后，直接插入当前 editor selection。
- 如果 editor 未 focus 或没有有效 selection，插入文档末尾。
- 保留当前 `useDragDropListener` + `dropInbox` 的全局接收模型，只改变 consumer：由 append pending files 改为 insert editor tokens。

## 提交序列化：Tiptap → Markdown

`serializer` 遍历 Tiptap JSON，输出：

```ts
type RichComposerSubmitPayload = {
  markdown: string
  attachments: PendingFileInfo[]
  isEmpty: boolean
}
```

节点 → markdown 规则：

| Tiptap | Markdown |
|---|---|
| `paragraph` | 段落，段间 `\n\n` |
| `hardBreak` | 行尾两空格 + `\n` |
| `bold` | `**...**` |
| `italic` | `*...*` |
| `code`（inline） | `` `...` `` |
| `strike` | `~~...~~` |
| `link` | `[text](url)` |
| `bulletList` + `listItem` | `- item` |
| `orderedList` + `listItem` | `1. item` |
| `blockquote` | 行首 `> ` |
| `codeBlock` | ` ``` ` 围栏，含 `language` attr |
| `attachmentToken` kind≠image | `[附件: fileName](file://abs/path)` |
| `attachmentToken` kind=image | `![fileName](file://abs/path)` |

其它规则：

- `text` 节点里出现 `*` `_` `[` `` ` `` 等 markdown 特殊字符需 escape。
- attachments 数组按 token 在文档中的出现顺序收集。
- 同一 id 的附件 token 去重，避免同路径重复传后端。
- `isEmpty` 仅在没有非空文本节点且没有任何 attachmentToken 时为 true。
- 只附件提交时，markdown 就是一个或多个附件占位符的串联。
- 页面层不再拼接默认附件文案。`RichComposer` 在 payload 为空时阻止提交。

## `RichComposer` API

```ts
type RichComposerSubmitPayload = {
  markdown: string
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
  initialMarkdown?: string
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
- `initialMarkdown` 用于 prefill：将 markdown 转回 Tiptap JSON 插入后 focus 到末尾（prefill 能力本期仅支持纯文本 + 换行 + attachmentToken，不支持 bold/link 等复杂 mark 的反向解析——有使用场景再扩）。

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
await sendUserMessage(payload.markdown, payload.attachments)
```

`sendUserMessage` 入参语义从"纯文本"改为"markdown"；内部字段名（`text` → `markdown`）是否随之更名不是硬要求，以不破坏既有命名一致性为前提。

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
5. 调用 `sendUserMessage(payload.markdown, payload.attachments)`。

## User Message Bubble 渲染改造

### 总方针

`UserMessageBubble.tsx` 现有的手写 `ImageThumbnail` / `AttachmentIcon` / `files.map(...)` / `whitespace-pre-wrap` 文本四件套全部替换为 markdown 渲染。`StoredMessage.content.text` 从"纯文本"变成"markdown 字符串"。

### 渲染器

新增 `src/components/chat-scene/markdown/UserBubbleMarkdown.tsx`（或给现有 `AssistantMarkdown` 加 `variant="user"`）。底座用 react-markdown + remark-gfm（与 assistant 保持一致）。

### 主题映射（深底浅字）

Bubble 是 `bg-primary text-primary-foreground`，所有内嵌样式只使用主题变量，不新增硬编码颜色：

| 节点 | 样式 |
|---|---|
| 段落 / 文本 | `leading-relaxed`；段间 `mt-2` |
| 内联 `code` | `bg-primary-foreground/15 rounded px-1` |
| 链接（非 `file://`） | `underline underline-offset-2`；点击走外部浏览器 |
| 链接（`file://`） | 自定义 renderer，渲染为附件 chip：图标 + 文件名；点击走 `openLocalFile` / 可预览类型走 `openPreview` |
| 图片（`file://`） | 缩略图 `max-h-40 max-w-[200px] rounded-lg`；仍走 `useLocalImageDataUrl` 把 `file://` 路径换 data URL；点击走 `openLocalFile` |
| 图片（`http(s)://`） | 直接 `<img src>`；点击走外部浏览器 |
| `> quote` | `border-l-2 border-primary-foreground/40 pl-3 opacity-90` |
| `- / 1.` 列表 | `pl-5 list-{disc\|decimal}` |
| ` ``` ` 围栏 | `bg-primary-foreground/10 rounded p-2 overflow-x-auto`；不上 syntax highlight |

### 附件 chip 渲染

自定义 react-markdown `a` renderer：

- `href.startsWith('file://')` → 渲染为 chip（图标 + 文件名 + 点击行为等价于当前 `UserMessageBubble.AttachmentIcon` + button 组合）。
- 其它 → 普通外链。

图片同理通过自定义 `img` renderer 实现：

- `src` 以 `file://` 开头 → 走 `useLocalImageDataUrl`。
- 其它 → 原生 `<img>`。

### 不再读 `files` 数组

bubble 渲染层只吃 markdown 字符串；`StoredMessage.content.files` 仍保留，但只给后端 `build_llm_content` 和 workspace 授权使用，不给前端 bubble 展示用。

### Skill token

bubble 左上角的 skill chip（`skillCommand` / `tokenLabel`）渲染逻辑保留，独立于 markdown 内容之外。

### 旧消息兼容

历史消息的 `content.text` 可能是纯文本（含未 escape 的 `*` `_` 等）。react-markdown 对孤立特殊字符多数场景无害，直接用同一渲染器。不做数据迁移。

## Lotus Cloud Anthropic 图片理解通道

### 目标

让新桌面端在 Lotus Cloud 路径下把**当前轮次**发送的图片像素交给支持视觉的云端模型。服务端已经明确：新桌面端走 `/anthropic/v1/messages`，只匹配 `provider.protocol = "anthropic"` 的活动路由；缺 Anthropic 路由的模型对新桌面端不可用。因此本期不再做 OpenAI/Qwen/custom/非云端多协议适配。

### 已验证事实

- `LotusProvider` 固定调用 `https://ai-tenant.renlijia.com/anthropic/v1/messages`，复用 `ClaudeProvider` 的 Anthropic Messages 请求构造。
- 服务端 `/anthropic/v1/messages` 是 Anthropic 原生透传：只筛 `provider.protocol = "anthropic"`，只重写 `model` 为 `upstream_model_name`，不做 OpenAI ↔ Anthropic 请求体转换。
- 已用真实 session key + `claude-sonnet-4-5` 向 `/anthropic/v1/messages` 发送 1x1 红色 PNG 的 Anthropic base64 image block，返回 200 且模型能识别红色图片。
- OPS 协议覆盖矩阵是模型可用性的准线：`qwen-plus` 这类 openai-only 模型不属于新桌面端当前路径；`glm5.1` 已确认有 Anthropic 路由且可接收 image block；实际识别质量由上游模型决定。

### Wire Format

当前 App 只发送 Anthropic Messages content blocks：

```json
{
  "model": "claude-sonnet-4-5",
  "max_tokens": 4096,
  "messages": [
    {
      "role": "user",
      "content": [
        { "type": "text", "text": "请描述这张图片" },
        {
          "type": "image",
          "source": {
            "type": "base64",
            "media_type": "image/png",
            "data": "纯 base64，不带 data:image/png;base64, 前缀"
          }
        }
      ]
    }
  ]
}
```

明确不发送 OpenAI `image_url` data URL：

```json
{ "type": "image_url", "image_url": { "url": "data:image/png;base64,..." } }
```

### 实现边界

- 只做 Lotus Cloud 路径；非云端配置、本地 provider、自定义 endpoint 不改。
- 只做 Anthropic Messages 协议；不改 `openai.rs`、`qwen.rs`、`deepseek_*`、`volcano.rs`、`custom.rs`。
- 不引入 provider-wide `ChatMessageContent::Parts` 大改；优先在 Lotus/Claude 请求体构造前，用当前 turn 的附件 sidecar/enrichment 生成 Anthropic content blocks。
- 历史消息不重放图片 base64；base64 不持久化，只在当前 LLM 请求内存中存在。
- 非图片附件继续保留现有 `[当前消息附件]` path 文本提示。
- 图片成功进入 Anthropic image block 后，不再重复出现在 `[当前消息附件]` path 文本列表里，避免模型同时看到像素和路径造成混淆。

### 构造 user message 决策

```text
当前 turn 的 attachments 里有 image 吗？
  否 → 沿用 Text(build_llm_content(markdown, attachments, ...))
  是 → 当前 cloud model 在 Lotus Anthropic vision allowlist 内吗？
        是 → 读取符合约束的 image bytes；
             构造 Anthropic content = [Text(build_llm_content(markdown, non_image + degraded_image_attachments, ...)),
                                      Image(base64_1), Image(base64_2), ...]
        否 → 沿用 Text(build_llm_content(markdown, attachments, ...))（完整列 image path）
```

allowlist 第一版保守：

- 已实测支持：`claude-sonnet-4-5`。
- 可列入支持：`claude-ops`、其它明确走 Anthropic 路由且上游为 Claude 3/4/Opus/Sonnet 视觉能力的模型。
- 尝试启用：`glm5.1`（已测可接收 Anthropic image block；识别质量不稳定，先按“能理解就理解”传图）。
- 明确不期望：`deepseek-*`、openai-only `qwen-plus`。

### 约束与降级

服务端有 10MB 全局 body limit；base64 约放大 33%，还要容纳 system prompt、历史消息和工具 schema。因此客户端 guard 必须保守：

- 单图原始 bytes ≤ 3MB。
- 单请求图片原始 bytes 总量 ≤ 6MB。
- 单轮最多 4 张图片。
- 格式白名单：`image/png`、`image/jpeg`、`image/webp`、`image/gif`。
- 超限 / 格式不白 / 读盘失败 / 模型不在 vision allowlist → 该图降级回原路径提示，不阻断整条消息；用户可见 toast 留到后续体验增强。
- Provider 端 image decoding 错误 → 整轮按现有 LLM 错误路径展示。

### 历史消息

- `StoredMessage.content` schema 不动：仍存 markdown `text` 和 `files[]`。
- `build_chat_history` / 历史 user message 一律走 text 路径，不重新读盘 + base64。
- 用户追问历史图片时，模型只能依赖上一轮视觉上下文和 path 文本；本期不做历史图片重发策略。

### Telemetry 与日志

结构体保留以下安全元数据；日志/telemetry 接入可后续补充，但不得打印 base64：

- `image_part_count`
- `image_part_bytes_total`
- `image_degraded_count`
- `vision_model` / `vision_enabled`

日志 preview 严禁包含 base64；只允许文件名、mime、原始 bytes、降级原因。

## 兼容行为

必须保留：

- 文本 + attachments 数组双轨发送（`markdown` 字段替换 `text` 字段内容）。
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

- 文本 + attachmentToken + 文本 → `...[附件: name](file://...)...`。
- image token → `![name](file://...)`。
- 多 token 按文档顺序输出，且 attachments 数组同序。
- `hardBreak` → 行尾两空格换行；段落 → `\n\n`。
- mark 序列化：bold / italic / inline code / strike / link。
- 节点序列化：bulletList / orderedList / blockquote / codeBlock（含 language）。
- text 节点里 markdown 特殊字符被 escape。
- 同 id token 去重：attachments 输出一份，markdown 保留多处占位符。
- 删除 token 后 payload 不再包含该附件。

`pastePipeline`：

- 普通文本不拦截。
- HTML 富文本被 schema filter 吃下，保留支持的 mark / 节点。
- 文件路径粘贴插入 token。
- 文件夹粘贴插入 folder token。
- 图片 blob 粘贴调用 `saveClipboardImageToTmp()` 后插入 image token。
- 混合文本 / 附件按可用顺序生成 fragments。
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
- 只附件提交输出占位符 markdown 和 attachments。
- `isStreaming` 时发送按钮变停止按钮。
- `disabled` / 提交中防重复提交。
- `clearOnSubmit` 成功清空、失败保留。

### User bubble 测试

`UserBubbleMarkdown`：

- 纯文本渲染等价 `whitespace-pre-wrap` 旧行为（回归旧消息不破）。
- bold / italic / inline code / link / list / blockquote / codeBlock 正确渲染。
- `file://` 链接渲染为 chip，`http(s)://` 渲染为外链。
- `file://` 图片渲染为 data URL 缩略图；`http(s)://` 图片直出。
- 深底主题下对比度与变量引用不越界（无硬编码颜色）。

### 页面测试

`ChatBottomArea`：

- 当前会话直接发送 payload（markdown + attachments）。
- 流式停止行为保留。
- prefill 插入 editor 并 focus。
- drop inbox 被消费后插入 token，而不是 pending chips。

`HomeTaskComposerCard`：

- 创建会话、切路由、授权工作区顺序不变。
- payload attachments 继续传给 `sendUserMessage`。
- 项目按钮仍能选择工作区。
- 只附件提交不再额外补"请分析附件"。

### 后端测试

`vision_support`：

- `claude-sonnet-4-5` 返回 Supported（已实测）。
- `claude-ops` 和明确 Anthropic Claude Sonnet/Opus 路由返回 Supported。
- `deepseek-*`、openai-only `qwen-plus` 返回 Unsupported。
- `glm5.1` 返回 Supported（已测可接收 Anthropic image block；识别质量不稳定，先允许传图）。

`chat_turn_driver` multimodal 构造：

- 无 image 附件 → 沿用普通 Text 请求。
- Lotus Cloud + allowlist 支持 + 有 image 附件 → 请求 sidecar 包含 Anthropic `Text + Image` blocks；text 里不含已转 image 的 path 列表。
- 非 Lotus Cloud / 不在 allowlist / Unknown → 沿用 Text 请求，含完整 image path 列表（原行为）。
- 超 3MB 单图 / 超 6MB 图片总量 / 非白名单格式 / 读盘失败 → 该图降级，其它图正常；telemetry `image_degraded_count++`。
- 超 4 张 → 前 4 张尝试进 Anthropic image blocks，其余降级 + toast。

Provider 序列化：

- Claude/Lotus：sidecar → Anthropic `content` 数组含 `image.source.base64`。
- 明确不测 OpenAI/Qwen/DeepSeek/Volcano/custom provider；这些路径不属于本期范围。

`history`：

- 历史 user message 一律走 Text 路径，不重新读盘。
- 历史回放含 image 附件时仍走原有 `[当前消息附件]` 文本提示。

### 旧测试迁移

- `useComposerPaste.test.tsx` 迁移为新 paste pipeline 测试；旧 hook 删除后删除旧测试。
- `ChatComposerCompact.test.tsx` 迁移为 `RichComposer` 测试；旧组件删除后删除旧测试。
- `UserMessageBubble` 现有图片 thumbnail 测试迁移到 `UserBubbleMarkdown`。

## 实施分期

1. **P0 序列化纯函数**：Tiptap JSON → markdown serializer + 完整单测（不依赖 React / Tiptap 运行时）。
2. **P1 Tiptap 底座与 schema**：装依赖；新建 `rich-composer/`；StarterKit + Link + Placeholder + attachmentToken extension。
3. **P2 RichComposer 组件**：基础输入、IME、Enter/Shift+Enter、disabled、clearOnSubmit、SkillPopover 接入。
4. **P3 Picker + Drop 直插 token**：替换原 pending chips 队列消费方。
5. **P4 全量 paste pipeline**：HTML / 文件 / URI / 图片 blob / 混合顺序 / fallback toast；实测主流来源（飞书 / 钉钉 / Notion / 微信网页 / VS Code / 网页）。
6. **P5 页面接入**：`ChatBottomArea` + `HomeTaskComposerCard` 本地状态退场，测试迁移。
7. **P6 User bubble markdown 渲染**：新建 `UserBubbleMarkdown`，自定义 link / img renderer，删 `UserMessageBubble.tsx` 的手写 thumbnail / chip / files.map。
8. **P7 Lotus Cloud Anthropic multimodal**（独立 PR）：当前 turn 图片附件转 Anthropic `image.source.base64` blocks；只改 Lotus/Claude 云端发送路径、附件降级与 telemetry；不做 OpenAI/Qwen/custom/非云端 provider-wide 改造。可与 P0–P6 并行。
9. **P8 收敛**：删 `ChatComposerCompact`、旧 `useComposerPaste`、不再使用的 `PendingAttachmentChips`。

## 风险与应对

- **Tiptap 在 jsdom 下测试 selection 行为有限**：把 serializer 和 paste pipeline 抽纯函数，组件层只测关键交互。
- **剪贴板混合顺序跨平台不稳定**：实现"尽力保留 + fallback + toast"，不虚假承诺所有平台都能精确还原。
- **macOS WebKit 文件剪贴板曾有读取 `clipboardData.items/types` 卡顿风险**：保留 capture snapshot 思路，并在 Tiptap paste handler 中避免不必要读取。
- **图片 blob 落盘异步**：插入 token 前先等待保存完成；不做 optimistic loading token。
- **跨 app 粘贴 HTML 兼容性**：飞书 / 钉钉 / Notion 可能用自定义标签（如 `<pre class="code-block">`），Tiptap 默认解析不到。P4 里按主流来源实测并补 paste rule；覆盖不到的格式降级为纯文本。
- **Tiptap 官方 `prosemirror-markdown` 桥接**：与 attachmentToken custom node 不兼容，**自写 serializer** 保留可控性。
- **react-markdown 深底主题**：不直接复用 assistant 的浅底主题；独立样式组合，只用主题变量。
- **base64 读盘阻塞 send 路径**：本期最多 4 张、原始总量 6MB，阻塞风险可控；不上后台预编码，保持实现线性。
- **服务端协议覆盖漂移**：新桌面端只走 Anthropic 入口；以 `/anthropic/v1/models` 和 OPS 协议覆盖矩阵为准。客户端 allowlist 只作为图片发送 guard，缺 Anthropic 路由时不做 OpenAI 回退。
- **页面业务逻辑被共享组件吞掉**：`RichComposer` 只输出 payload，不创建会话、不授权工作区、不访问 chat store。

## 验收标准

- 首页和 Chat 页面都使用同一个 `RichComposer`。
- 在任意光标位置粘贴 / 选择 / 拖放附件，附件 token 插入该位置。
- 粘贴截图或复制图片后，图片保存到 tmpImage 并插入 image token。
- 普通文本粘贴不受附件逻辑影响。
- 混合粘贴按可用顺序插入；无法保证顺序时有 fallback 提示。
- 输入框支持 bold / italic / inline code / strike / link / bulletList / orderedList / blockquote / codeBlock 的输入和粘贴回显。
- 从飞书 / 钉钉 / 网页粘贴富文本，常见格式（粗体、链接、列表、代码块）能保留。
- 提交 payload 的 `markdown` 字段是合法 markdown；附件 token 以 `[附件: name](file://...)` / `![name](file://...)` 形式与正文穿插。
- user bubble 用 markdown 渲染：粗体 / 链接 / 列表 / 引用 / 代码块正确显示；`file://` 链接渲染为附件 chip；`file://` 图片渲染为缩略图；旧纯文本消息兼容渲染。
- 当前轮次发送图片给 `claude-sonnet-4-5` 等已配置 Anthropic 路由且支持视觉的 Lotus Cloud 模型时，模型能"看见"图（视觉测试 prompt 有视觉感知输出）。
- 模型未在客户端视觉 allowlist、图片超限、读盘失败或格式不支持时：用户可见 toast，模型按 path 提示工作，不报错；缺 Anthropic 路由的模型由云端模型列表/服务端 404 处理，不在本功能内做协议回退。
- 历史轮次的图不重新塞 multimodal 给 LLM。
- 超 3MB 单图 / 超 6MB 图片总量 / 非白名单格式 / 读盘失败 / 超 4 张 → 该图降级 + toast；整条消息照常发送。
- Enter、Shift+Enter、IME、流式停止、首页创建会话和工作区授权行为保持正确。
- 本期没有实现 slash 菜单，但代码中有明确扩展点。
- 本期没有实现图片 lightbox / 全屏预览。
- UI 全部使用主题变量，无硬编码颜色。
