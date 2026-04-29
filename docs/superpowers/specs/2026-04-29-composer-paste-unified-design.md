# 输入框粘贴逻辑统一 + tmpImage 设计

日期：2026-04-29
范围：前端 composer（首页 + chat 页面）粘贴附件能力统一；后端剪贴板图片落盘改到全局 tmpImage 目录。

## 背景与问题

当前两个输入框使用同一个 `ChatComposerCompact` 组件，但 chat 页面通过 `onPaste` 注入了图片/路径附件解析逻辑，首页则完全没接：

- `src/components/chat-scene/ChatBottomArea.tsx:220-252` 内联了 200 行级别的 `handlePaste`，覆盖三层：clipboard 图片 → 落盘到 `~/.renlijia/conversations/{id}/attachments/clipboard/`、文本里的绝对路径 → `resolvePastedPaths`、原生剪贴板文件 → Tauri `readClipboardFilePaths`。
- `src/components/home/HomeTaskComposerCard.tsx:146-156` 没有传 `onPaste`，只能走 textarea 默认文本粘贴。
- 后端 `save_clipboard_image_attachment_to_home`（`src-tauri/src/commands/file.rs:399`）写入路径绑定 `conversation_id`，导致首页"还没有会话"的场景天然接不上。

期望：
1. 首页和 chat 页面统一支持图片、文件路径、文件夹路径粘贴。
2. 文件夹粘贴只取路径作为附件传给 LLM，不做沙箱授权。
3. 剪贴板图片统一保存到 `~/.renlijia/tmpImage/`，与会话解耦。

## 设计原则

- **抽 hook 而非下沉到组件**：`ChatComposerCompact` 是纯展示组件，附件状态属于业务层；下沉会让组件承担多余职责。
- **tmpImage 是终点路径**：图片落盘后直接把绝对路径作为附件传给 LLM，**不再迁移**到会话目录。历史会话里的 `attachments/clipboard/` 路径快照不受影响。
- **删旧不留兼容层**：`save_clipboard_image_attachment` 命令前端只有一处调用，本期直接替换；不保留 deprecated 兼容入口。

## 范围（本期做 / 不做）

**做**
- 后端新增 tmpImage 落盘 command，删除旧 per-conversation 命令
- 抽 `useComposerPaste` hook，承载三层粘贴逻辑
- chat 页面改用 hook
- 首页接入 hook + `pendingFiles` 状态 + submit 时透传附件

**不做（明确延后）**
- tmpImage 清理机制（按时间或容量）
- 粘贴文件夹时自动 `authorizeLocalDirectory`（沙箱实际读写若失败另行处理）
- 把图片从 tmpImage 迁移到会话目录
- 首页加附件按钮（本期仅支持粘贴入口）

## 后端改动

### `src-tauri/src/commands/file.rs`

- 新增 `save_clipboard_image_to_tmp(aijia_home: &AiJiaHome, bytes: &[u8], mime_type: &str) -> Result<SavedClipboardAttachment, String>`
  - 目录：`aijia_home.root().join("tmpImage")`，`create_dir_all`
  - 文件名沿用现有模式 `clipboard-{ts_millis}-{uuid8}.{ext}`，`ext` 复用 `clipboard_extension_for_mime`
- 新增 Tauri command：
  ```rust
  #[tauri::command]
  pub async fn save_clipboard_image_to_tmp_dir(
      aijia_home: State<'_, Arc<AiJiaHome>>,
      bytes: Vec<u8>,
      mime_type: String,
  ) -> Result<SavedClipboardAttachment, String>
  ```
- 删除 `save_clipboard_image_attachment` command 和 `save_clipboard_image_attachment_to_home` 函数
- 测试 `save_clipboard_image_attachment_writes_to_conversation_clipboard_dir` 改写为 `save_clipboard_image_writes_to_tmp_image_dir`，断言文件落在 `~/.renlijia/tmpImage/clipboard-*.{png,jpg,...}`

### `src-tauri/src/lib.rs`

- `invoke_handler` 中把 `file::save_clipboard_image_attachment` 替换为 `file::save_clipboard_image_to_tmp_dir`

## 前端改动

### `src/lib/tauri.ts`

把：
```ts
export function saveClipboardImageAttachment(conversationId, bytes, mimeType)
```
改为：
```ts
export function saveClipboardImageToTmp(bytes: number[], mimeType: string): Promise<SavedClipboardAttachmentPayload>
```
对应 invoke 名 `save_clipboard_image_to_tmp_dir`。`SavedClipboardAttachmentPayload` 字段不变。

### `src/hooks/useChatAttachments.ts`

`saveClipboardImage` 去掉 `conversationId` 参数：
```ts
const saveClipboardImage = useCallback(async (
  bytes: Uint8Array,
  mimeType: string,
): Promise<PendingAttachment> => {
  const saved = await saveClipboardImageToTmp(Array.from(bytes), mimeType)
  return { id: saved.path, fileName: saved.fileName, path: saved.path,
           kind: 'image', fileType: 'image', fileSize: saved.fileSize,
           mimeType: saved.mimeType, source: 'clipboard-image' }
}, [])
```
其余 `pickAttachments` / `resolvePastedPaths` 不变。

### `src/hooks/useComposerPaste.ts`（新文件）

```ts
interface UseComposerPasteParams {
  onAttachmentsResolved: (attachments: PendingAttachment[]) => void
}

export function useComposerPaste({ onAttachmentsResolved }: UseComposerPasteParams) {
  const { saveClipboardImage, resolvePastedPaths } = useChatAttachments()

  const handlePaste = useCallback((event: ClipboardEvent<HTMLTextAreaElement>) => {
    // 1. 图片
    const items = Array.from(event.clipboardData?.items ?? [])
    const imageItem = items.find(i => i.kind === 'file' && i.type.startsWith('image/'))
    if (imageItem) {
      const file = imageItem.getAsFile()
      if (file) {
        event.preventDefault()
        void (async () => {
          const bytes = await readClipboardImageBytes(file)
          const pending = await saveClipboardImage(bytes, file.type || 'image/png')
          onAttachmentsResolved([pending])
        })()
      }
      return
    }
    // 2. 文本里的绝对路径
    const text = event.clipboardData?.getData('text/plain') ?? ''
    const paths = extractAbsolutePaths(text)
    if (paths.length > 0) {
      event.preventDefault()
      void (async () => {
        const resolved = await resolvePastedPaths(paths)
        if (resolved.length > 0) onAttachmentsResolved(resolved)
      })()
      return
    }
    // 3. 原生剪贴板文件 (Tauri)
    void (async () => {
      const nativePaths = await readClipboardFilePaths().catch(() => [] as string[])
      if (nativePaths.length === 0) return
      const resolved = await resolvePastedPaths(nativePaths)
      if (resolved.length > 0) onAttachmentsResolved(resolved)
    })()
  }, [saveClipboardImage, resolvePastedPaths, onAttachmentsResolved])

  return { handlePaste }
}
```

辅助函数 `extractAbsolutePaths` / `readClipboardImageBytes` 从 `ChatBottomArea.tsx` 搬到本文件顶部（同文件 `function`，不导出，本期没有跨文件复用需求）。

**hook 不持有附件状态、不做去重**：附件去重逻辑依赖调用方维护的 `pendingFiles` 列表，所以放在调用方。

### `src/components/chat-scene/ChatBottomArea.tsx`

- 删除 `handlePaste` 内联实现、`extractAbsolutePaths`、`appendResolvedPastedPaths` 三段
- 删除对 `activeConversationId` 在 paste 链路的依赖
- 接入 hook：
  ```ts
  const { handlePaste } = useComposerPaste({
    onAttachmentsResolved: (resolved) => {
      setPendingFiles((prev) => {
        const seen = new Set(prev.map(f => f.id))
        const deduped = resolved.filter(f => !seen.has(f.id))
        return deduped.length > 0 ? [...prev, ...deduped] : prev
      })
    },
  })
  ```
- `<ChatComposerCompact onPaste={handlePaste} ... />` 不变

### `src/components/home/HomeTaskComposerCard.tsx`

新增：
```ts
const [pendingFiles, setPendingFiles] = useState<PendingAttachment[]>([])

const { handlePaste } = useComposerPaste({
  onAttachmentsResolved: (resolved) => {
    setPendingFiles((prev) => {
      const seen = new Set(prev.map(f => f.id))
      const deduped = resolved.filter(f => !seen.has(f.id))
      return deduped.length > 0 ? [...prev, ...deduped] : prev
    })
  },
})
```

`handleSubmit` 末尾调 `sendUserMessage` 时传附件：
```ts
const fileInfos: PendingFileInfo[] = pendingFiles.map((f) => ({
  id: f.id, fileName: f.fileName, filePath: f.path,
  kind: f.kind, fileSize: f.fileSize, fileType: f.fileType, mimeType: f.mimeType,
}))
await sendUserMessage(text, fileInfos)
setPendingFiles([])
```

`<ChatComposerCompact>` 增加：
```tsx
onPaste={handlePaste}
pendingFilesSlot={pendingFiles.length > 0 ? (
  <PendingFiles
    pendingFiles={pendingFiles}
    onRemove={(id) => setPendingFiles((prev) => prev.filter(f => f.id !== id))}
  />
) : null}
```

## 数据流

首页粘贴图片 →
`useComposerPaste.handlePaste` →
`saveClipboardImage(bytes, mime)` →
invoke `save_clipboard_image_to_tmp_dir` →
`~/.renlijia/tmpImage/clipboard-{ts}-{uuid8}.png` →
`PendingAttachment{ path, source: 'clipboard-image' }` →
`onAttachmentsResolved` → `setPendingFiles` →
用户点发送 → `createConversation` → `authorizeWorkspace` → `sendUserMessage(text, fileInfos)` → LLM 收到 tmpImage 绝对路径

Chat 页面同结构，只是 submit 时使用已有 `activeConversationId`。

## 测试

**后端**（`src-tauri/src/commands/file.rs` tests）
- `save_clipboard_image_writes_to_tmp_image_dir`：传字节 + mime，断言文件落在 `aijia_home.root().join("tmpImage")` 下，且文件名匹配 `clipboard-*.{ext}`

**前端**
- `src/hooks/useComposerPaste.test.tsx`（新建）：
  - mock `useChatAttachments` 的 `saveClipboardImage` / `resolvePastedPaths`
  - mock `readClipboardFilePaths`
  - 三个用例：clipboardData 有 image item / 有绝对路径文本 / 都没有但 native paths 非空
  - 断言 `onAttachmentsResolved` 被调用、参数正确
- 首页集成测（已有 home-related 测试如有，否则新建轻量回归）：粘贴图片 → `pendingFiles` 增长一个 → submit 后 `sendUserMessage` 收到对应 `PendingFileInfo`

## 风险与回滚

| 风险 | 处理 |
|------|------|
| 历史会话 `attachments/clipboard/` 路径快照仍指向旧目录 | 不影响（消息存的是字符串路径，不再写新文件即可） |
| tmpImage 长期累积 | 本期不处理，docs 中标注待办 |
| `extractAbsolutePaths` 移位后旧测试 import 路径失效 | 搬移时同步更新 import；如有 export 兼容需求，从新文件 re-export |
| 删除 `save_clipboard_image_attachment` 后，若有外部调用方（插件、脚本）使用 invoke 名 | grep 确认仅前端 `useChatAttachments` 一处；本期允许 break |

回滚：单 commit 即可 revert（hook 抽取与命令重命名是独立改动，但建议同 PR 一起合并避免中间态）。
