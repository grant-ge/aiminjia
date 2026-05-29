# Composer Paste 统一 + tmpImage 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让首页和 chat 页面共用同一套粘贴附件逻辑（图片/文件路径/文件夹路径），剪贴板图片落到 `~/.renlijia/tmpImage/`。

**Architecture:** 后端把"按会话保存剪贴板图片"换成"全局 tmpImage 保存"；前端抽 `useComposerPaste` hook 承载三层粘贴逻辑（图片/文本路径/原生剪贴板）；chat 页面改造为使用 hook，首页接入 hook + `pendingFiles` 状态 + submit 时透传附件。

**Tech Stack:** Rust + Tauri 2.x（后端命令）；React + TypeScript + Vitest（前端 hook 与组件）。

**Spec:** `docs/superpowers/specs/2026-04-29-composer-paste-unified-design.md`

---

## File Structure

**新增**
- `src/hooks/useComposerPaste.ts` — 三层粘贴逻辑 hook（图片→tmpImage、文本路径→resolve、native paths→resolve），无内部状态，通过回调 `onAttachmentsResolved` 出参
- `src/hooks/useComposerPaste.test.tsx` — hook 三层分支单测

**修改**
- `src-tauri/src/commands/file.rs` — 删除 `save_clipboard_image_attachment` + `save_clipboard_image_attachment_to_home`；新增 `save_clipboard_image_to_tmp_dir` + `save_clipboard_image_to_tmp`；改写测试为 tmpImage 版本
- `src-tauri/src/lib.rs` — invoke handler 注册替换
- `src/lib/tauri.ts` — `saveClipboardImageAttachment` → `saveClipboardImageToTmp`，去掉 `conversationId` 参数
- `src/hooks/useChatAttachments.ts` — `saveClipboardImage` 签名去掉 `conversationId`
- `src/components/chat-scene/ChatBottomArea.tsx` — 删除内联 `handlePaste` / `extractAbsolutePaths` / `appendResolvedPastedPaths` / `readClipboardImageBytes`，改用 hook
- `src/components/home/HomeTaskComposerCard.tsx` — 接入 hook + `pendingFiles` state + 提交时携带附件

---

### Task 1: 后端 tmpImage 落盘 + 删除旧命令

**Files:**
- Modify: `src-tauri/src/commands/file.rs:399-442`（删旧函数与命令）、`src-tauri/src/commands/file.rs:765-783`（改写测试）
- Modify: `src-tauri/src/lib.rs:528`（替换 invoke handler 注册）

- [ ] **Step 1: 改写后端测试为 tmpImage 版本（先写失败测试）**

把 `src-tauri/src/commands/file.rs:765-783` 整段替换为：

```rust
    #[test]
    fn save_clipboard_image_writes_to_tmp_image_dir() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let home = AiJiaHome::from_path(tmp.path().to_path_buf());

        let saved = save_clipboard_image_to_tmp(&home, &[1, 2, 3, 4], "image/png")
            .expect("save clipboard image");

        assert!(saved.path.contains("/tmpImage/"));
        assert!(!saved.path.contains("/conversations/"));
        assert!(saved.file_name.starts_with("clipboard-"));
        assert!(saved.file_name.ends_with(".png"));
        assert_eq!(saved.file_size, 4);
        assert_eq!(saved.mime_type, "image/png");
        assert!(std::path::Path::new(&saved.path).exists());

        let parent = std::path::Path::new(&saved.path).parent().expect("parent");
        assert_eq!(parent, home.root().join("tmpImage").as_path());
    }
```

- [ ] **Step 2: 运行测试，确认失败**

Run: `cd src-tauri && cargo test --lib save_clipboard_image_writes_to_tmp_image_dir -- --nocapture`
Expected: 编译错误 — `save_clipboard_image_to_tmp` 未定义

- [ ] **Step 3: 用 tmpImage 版本替换 file.rs:399-442 的旧函数与命令**

把 `src-tauri/src/commands/file.rs:399-442` 整段替换为：

```rust
pub(crate) fn save_clipboard_image_to_tmp(
    aijia_home: &AiJiaHome,
    bytes: &[u8],
    mime_type: &str,
) -> Result<SavedClipboardAttachment, String> {
    let ext = clipboard_extension_for_mime(mime_type);
    let file_name = format!(
        "clipboard-{}-{}.{}",
        chrono::Utc::now().timestamp_millis(),
        &uuid::Uuid::new_v4().simple().to_string()[..8],
        ext
    );
    let dir = aijia_home.root().join("tmpImage");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let full_path = dir.join(&file_name);
    std::fs::write(&full_path, bytes).map_err(|e| e.to_string())?;
    Ok(SavedClipboardAttachment {
        file_name,
        path: full_path.to_string_lossy().to_string(),
        file_size: bytes.len() as u64,
        mime_type: mime_type.to_string(),
    })
}

#[tauri::command]
pub async fn save_clipboard_image_to_tmp_dir(
    aijia_home: State<'_, Arc<AiJiaHome>>,
    bytes: Vec<u8>,
    mime_type: String,
) -> Result<SavedClipboardAttachment, String> {
    save_clipboard_image_to_tmp(aijia_home.inner().as_ref(), &bytes, &mime_type)
}
```

- [ ] **Step 4: 替换 lib.rs 的 invoke handler 注册**

打开 `src-tauri/src/lib.rs`，把 `file::save_clipboard_image_attachment,` 改为 `file::save_clipboard_image_to_tmp_dir,`（行号约 528，以实际为准）。

- [ ] **Step 5: 运行新测试 + Rust 全量编译**

Run:
```
cd src-tauri && cargo test --lib save_clipboard_image_writes_to_tmp_image_dir -- --nocapture
cd src-tauri && cargo build
```
Expected: 测试 PASS；`cargo build` 成功（如果其他 Rust 模块还引用旧符号会在此暴露）。

- [ ] **Step 6: 提交**

```bash
git add src-tauri/src/commands/file.rs src-tauri/src/lib.rs
git commit -m "feat(file): save clipboard image to tmpImage instead of per-conversation dir"
```

---

### Task 2: 前端 tauri.ts 与 useChatAttachments 适配新签名

**Files:**
- Modify: `src/lib/tauri.ts:288-298`
- Modify: `src/hooks/useChatAttachments.ts:5,94-114`

- [ ] **Step 1: 替换 `src/lib/tauri.ts` 的 saveClipboardImageAttachment 为 saveClipboardImageToTmp**

把 `src/lib/tauri.ts:288-298` 替换为：

```ts
export function saveClipboardImageToTmp(
  bytes: number[],
  mimeType: string,
): Promise<SavedClipboardAttachmentPayload> {
  return invoke<SavedClipboardAttachmentPayload>('save_clipboard_image_to_tmp_dir', {
    bytes,
    mimeType,
  })
}
```

- [ ] **Step 2: 更新 useChatAttachments.ts 的 import 与 saveClipboardImage 签名**

打开 `src/hooks/useChatAttachments.ts`：

把第 5 行：
```ts
import { saveClipboardImageAttachment } from '@/lib/tauri'
```
改为：
```ts
import { saveClipboardImageToTmp } from '@/lib/tauri'
```

把 `src/hooks/useChatAttachments.ts:94-114` 整个 `saveClipboardImage` 替换为：

```ts
  const saveClipboardImage = useCallback(async (
    bytes: Uint8Array,
    mimeType: string,
  ): Promise<PendingAttachment> => {
    const saved: SavedClipboardAttachment = await saveClipboardImageToTmp(
      Array.from(bytes),
      mimeType,
    )
    return {
      id: saved.path,
      fileName: saved.fileName,
      path: saved.path,
      kind: 'image',
      fileType: 'image',
      fileSize: saved.fileSize,
      mimeType: saved.mimeType,
      source: 'clipboard-image',
    }
  }, [])
```

- [ ] **Step 3: 类型检查**

Run: `pnpm exec tsc --noEmit`
Expected: 报错指出 `ChatBottomArea.tsx` 调用 `saveClipboardImage(activeConversationId, bytes, ...)` 参数不匹配（这是预期的——将在 Task 4 修复）。仅此一处错误，没有其他报错即可继续。

- [ ] **Step 4: 提交**

```bash
git add src/lib/tauri.ts src/hooks/useChatAttachments.ts
git commit -m "refactor(attachments): drop conversationId from saveClipboardImage"
```

---

### Task 3: 新增 useComposerPaste hook + 单测

**Files:**
- Create: `src/hooks/useComposerPaste.ts`
- Create: `src/hooks/useComposerPaste.test.tsx`

- [ ] **Step 1: 写失败测试 useComposerPaste.test.tsx**

新建 `src/hooks/useComposerPaste.test.tsx`：

```tsx
import { describe, expect, it, vi, beforeEach } from 'vitest'
import { renderHook } from '@testing-library/react'

import { useComposerPaste } from './useComposerPaste'
import type { PendingAttachment } from './useChatAttachments'

const saveClipboardImageMock = vi.fn()
const resolvePastedPathsMock = vi.fn()

vi.mock('./useChatAttachments', () => ({
  useChatAttachments: () => ({
    saveClipboardImage: saveClipboardImageMock,
    resolvePastedPaths: resolvePastedPathsMock,
    pickAttachments: vi.fn(),
    isPickingAttachments: false,
  }),
}))

const readClipboardFilePathsMock = vi.fn()
vi.mock('@/lib/tauri', () => ({
  readClipboardFilePaths: () => readClipboardFilePathsMock(),
}))

function makeImagePasteEvent(file: File) {
  const item = {
    kind: 'file',
    type: file.type,
    getAsFile: () => file,
  } as unknown as DataTransferItem

  return {
    preventDefault: vi.fn(),
    clipboardData: {
      items: [item] as unknown as DataTransferItemList,
      getData: () => '',
    },
  } as unknown as React.ClipboardEvent<HTMLTextAreaElement>
}

function makeTextPasteEvent(text: string) {
  return {
    preventDefault: vi.fn(),
    clipboardData: {
      items: [] as unknown as DataTransferItemList,
      getData: (kind: string) => (kind === 'text/plain' ? text : ''),
    },
  } as unknown as React.ClipboardEvent<HTMLTextAreaElement>
}

const samplePending: PendingAttachment = {
  id: '/tmp/x.png',
  fileName: 'x.png',
  path: '/tmp/x.png',
  kind: 'image',
  fileType: 'image',
  fileSize: 1,
  mimeType: 'image/png',
  source: 'clipboard-image',
}

describe('useComposerPaste', () => {
  beforeEach(() => {
    saveClipboardImageMock.mockReset()
    resolvePastedPathsMock.mockReset()
    readClipboardFilePathsMock.mockReset()
  })

  it('saves clipboard image and emits attachment', async () => {
    saveClipboardImageMock.mockResolvedValue(samplePending)
    const onResolved = vi.fn()
    const { result } = renderHook(() => useComposerPaste({ onAttachmentsResolved: onResolved }))

    const file = new File([new Uint8Array([1, 2, 3])], 'paste.png', { type: 'image/png' })
    const event = makeImagePasteEvent(file)
    result.current.handlePaste(event)

    await new Promise((r) => setTimeout(r, 0))
    expect(event.preventDefault).toHaveBeenCalled()
    expect(saveClipboardImageMock).toHaveBeenCalledTimes(1)
    expect(onResolved).toHaveBeenCalledWith([samplePending])
  })

  it('resolves absolute paths in pasted text', async () => {
    resolvePastedPathsMock.mockResolvedValue([samplePending])
    const onResolved = vi.fn()
    const { result } = renderHook(() => useComposerPaste({ onAttachmentsResolved: onResolved }))

    const event = makeTextPasteEvent('/Users/me/a.png\n/Users/me/dir')
    result.current.handlePaste(event)

    await new Promise((r) => setTimeout(r, 0))
    expect(event.preventDefault).toHaveBeenCalled()
    expect(resolvePastedPathsMock).toHaveBeenCalledWith(['/Users/me/a.png', '/Users/me/dir'])
    expect(onResolved).toHaveBeenCalledWith([samplePending])
  })

  it('falls back to native clipboard file paths', async () => {
    readClipboardFilePathsMock.mockResolvedValue(['/Users/me/native.png'])
    resolvePastedPathsMock.mockResolvedValue([samplePending])
    const onResolved = vi.fn()
    const { result } = renderHook(() => useComposerPaste({ onAttachmentsResolved: onResolved }))

    const event = makeTextPasteEvent('')
    result.current.handlePaste(event)

    await new Promise((r) => setTimeout(r, 0))
    expect(readClipboardFilePathsMock).toHaveBeenCalled()
    expect(resolvePastedPathsMock).toHaveBeenCalledWith(['/Users/me/native.png'])
    expect(onResolved).toHaveBeenCalledWith([samplePending])
  })
})
```

- [ ] **Step 2: 运行测试，确认失败**

Run: `pnpm exec vitest run src/hooks/useComposerPaste.test.tsx`
Expected: FAIL — `Cannot find module './useComposerPaste'`

- [ ] **Step 3: 实现 useComposerPaste.ts**

新建 `src/hooks/useComposerPaste.ts`：

```ts
import { useCallback, type ClipboardEvent } from 'react'

import { readClipboardFilePaths } from '@/lib/tauri'

import { useChatAttachments, type PendingAttachment } from './useChatAttachments'

function extractAbsolutePaths(text: string): string[] {
  return text
    .split(/[\n\r]+/)
    .map((line) => line.trim())
    .filter((line) => line.startsWith('/'))
}

async function readClipboardImageBytes(file: File): Promise<Uint8Array> {
  const buffer = await file.arrayBuffer()
  return new Uint8Array(buffer)
}

export interface UseComposerPasteParams {
  onAttachmentsResolved: (attachments: PendingAttachment[]) => void
}

export function useComposerPaste({ onAttachmentsResolved }: UseComposerPasteParams) {
  const { saveClipboardImage, resolvePastedPaths } = useChatAttachments()

  const handlePaste = useCallback((event: ClipboardEvent<HTMLTextAreaElement>) => {
    const items = Array.from(event.clipboardData?.items ?? [])
    const imageItem = items.find((item) => item.kind === 'file' && item.type.startsWith('image/'))
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

    void (async () => {
      const nativePaths = await readClipboardFilePaths().catch(() => [] as string[])
      if (nativePaths.length === 0) return
      const resolved = await resolvePastedPaths(nativePaths)
      if (resolved.length > 0) onAttachmentsResolved(resolved)
    })()
  }, [saveClipboardImage, resolvePastedPaths, onAttachmentsResolved])

  return { handlePaste }
}

export type { PendingAttachment }
```

- [ ] **Step 4: 运行测试，确认通过**

Run: `pnpm exec vitest run src/hooks/useComposerPaste.test.tsx`
Expected: PASS — 三个用例全过

- [ ] **Step 5: 提交**

```bash
git add src/hooks/useComposerPaste.ts src/hooks/useComposerPaste.test.tsx
git commit -m "feat(hook): add useComposerPaste for unified clipboard paste handling"
```

---

### Task 4: ChatBottomArea 改用 useComposerPaste

**Files:**
- Modify: `src/components/chat-scene/ChatBottomArea.tsx:4,11,13,70-95,109-118,220-252`

- [ ] **Step 1: 删掉 ChatBottomArea.tsx 的内联辅助函数与 paste 逻辑**

执行以下编辑：

1. 修改 import 行 4，去掉 `ClipboardEvent`：
```ts
import { useCallback, useEffect, useRef, useState, type KeyboardEvent } from 'react'
```

2. 修改 import 行 11（保留 `PendingAttachment` 给 `PendingFiles` 类型用）：
```ts
import { useChatAttachments, type PendingAttachment } from '@/hooks/useChatAttachments'
```

3. 修改 import 行 13，删掉 `readClipboardFilePaths`：
```ts
// 整行删除
```
（如果文件其他位置还用，保留；当前 grep 仅 paste 链路用）

4. 在 `useChatAttachments` import 之后追加：
```ts
import { useComposerPaste } from '@/hooks/useComposerPaste'
```

5. 删除 `src/components/chat-scene/ChatBottomArea.tsx:70-95` 的三个函数 `extractAbsolutePaths` / `appendResolvedPastedPaths` / `readClipboardImageBytes`

6. 在 `ChatBottomArea` 函数内部、`handlePickAttachments` 之后（约原第 218 行后），新增：
```ts
  const appendPendingFiles = useCallback((resolved: PendingAttachment[]) => {
    setPendingFiles((prev) => {
      const seen = new Set(prev.map((file) => file.id))
      const next = resolved.filter((file) => !seen.has(file.id))
      return next.length > 0 ? [...prev, ...next] : prev
    })
  }, [])
  const { handlePaste } = useComposerPaste({ onAttachmentsResolved: appendPendingFiles })
```

7. 删除 `src/components/chat-scene/ChatBottomArea.tsx:220-252` 整段旧 `handlePaste`

8. 检查 `useChatAttachments` 解构（约第 109-118 行），移除现在未使用的 `saveClipboardImage`，仅保留 `pickAttachments`、`isPickingAttachments`、`resolvePastedPaths`（如果 `resolvePastedPaths` 也不再被组件直接用，一并去掉）。最简实现：
```ts
  const { isPickingAttachments, pickAttachments } = useChatAttachments()
```

9. 确认 `activeConversationId` 是否还有其他用途；如仅用于旧 paste 链路，删掉对应解构与 import。

- [ ] **Step 2: 类型检查 + 单测**

Run:
```
pnpm exec tsc --noEmit
pnpm exec vitest run src/hooks/useComposerPaste.test.tsx src/hooks/useChatAttachments.ts
```
Expected: tsc 无错误；hook 测试通过。

- [ ] **Step 3: 跑现有 chat 相关回归**

Run: `pnpm exec vitest run src/lib/tauri.events.test.ts src/hooks/useStreaming.integration.test.tsx src/stores/chatStore.test.ts`
Expected: 全部 PASS（这套关键集成测试在 CLAUDE.md 中标注为事件联调回归基线）。

- [ ] **Step 4: 提交**

```bash
git add src/components/chat-scene/ChatBottomArea.tsx
git commit -m "refactor(chat): ChatBottomArea use useComposerPaste hook"
```

---

### Task 5: HomeTaskComposerCard 接入粘贴附件 + submit 透传

**Files:**
- Modify: `src/components/home/HomeTaskComposerCard.tsx`

- [ ] **Step 1: 在 HomeTaskComposerCard 引入 hook、state 与 PendingFiles 渲染**

修改 `src/components/home/HomeTaskComposerCard.tsx`：

1. import 区追加（参考 chat 端）：
```ts
import { useCallback, useEffect, useRef, useState } from 'react'
// ...
import { useChat, type PendingFileInfo } from '@/hooks/useChat'
import { useComposerPaste } from '@/hooks/useComposerPaste'
import type { PendingAttachment } from '@/hooks/useChatAttachments'
```

2. 把 `PendingFiles` 组件 + `FILE_TYPE_CONFIG` 从 `ChatBottomArea.tsx:17-68` 复制粘贴到 `HomeTaskComposerCard.tsx` 文件顶部（`HomeTaskComposerCard` 函数定义之前）。**不要尝试抽取为共享组件**——这是本期 YAGNI 范围之外的事，spec 明确未列入。

3. 在 `HomeTaskComposerCard` 函数体内、`useState(value)` 之后追加：

```ts
  const [pendingFiles, setPendingFiles] = useState<PendingAttachment[]>([])

  const appendPendingFiles = useCallback((resolved: PendingAttachment[]) => {
    setPendingFiles((prev) => {
      const seen = new Set(prev.map((file) => file.id))
      const next = resolved.filter((file) => !seen.has(file.id))
      return next.length > 0 ? [...prev, ...next] : prev
    })
  }, [])

  const { handlePaste } = useComposerPaste({ onAttachmentsResolved: appendPendingFiles })
```

4. 修改 `handleSubmit` —— 在 `await sendUserMessage(text)` 之前先把 `pendingFiles` 映射成 `PendingFileInfo[]`，并把这一行替换掉：

把：
```ts
      // sendUserMessage will use the already-active conversation
      await sendUserMessage(text)
```
改为：
```ts
      // sendUserMessage will use the already-active conversation
      const fileInfos: PendingFileInfo[] = pendingFiles.map((f) => ({
        id: f.id,
        fileName: f.fileName,
        filePath: f.path,
        kind: f.kind,
        fileSize: f.fileSize,
        fileType: f.fileType,
        mimeType: f.mimeType,
      }))
      await sendUserMessage(text, fileInfos)
      setPendingFiles([])
```

5. 修改 `<ChatComposerCompact>`（约第 146 行起），追加两个 prop：

```tsx
      <ChatComposerCompact
        value={value}
        onChange={setValue}
        onSubmit={(v) => void handleSubmit(v)}
        placeholder="描述你的任务，或输入 / 选择技能来开始..."
        onOpenSkill={() => setShowSkillPopover((prev) => !prev)}
        onPickProject={() => void handlePickProject()}
        projectLabel={displayWorkspace?.displayName ?? '默认项目'}
        textareaRef={textareaRef}
        submitDisabled={isSubmitting}
        onPaste={handlePaste}
        pendingFilesSlot={pendingFiles.length > 0 ? (
          <PendingFiles
            pendingFiles={pendingFiles}
            onRemove={(id) => setPendingFiles((prev) => prev.filter((f) => f.id !== id))}
          />
        ) : null}
      />
```

- [ ] **Step 2: 类型检查**

Run: `pnpm exec tsc --noEmit`
Expected: 无错误

- [ ] **Step 3: 跑前端单测**

Run: `pnpm test`
Expected: 全部通过

- [ ] **Step 4: 跑 Rust 全量测试**

Run: `cd src-tauri && cargo test`
Expected: 全部通过

- [ ] **Step 5: 启动 dev 模式手动验证（重要 — UI 改动必须实际试一次）**

Run: `pnpm tauri:dev`

手动测试清单：
1. 在首页输入框：截图复制、Cmd+V → 应在输入框上方出现 "IMG xxx" 卡片，点 X 可移除
2. 在首页输入框：在 Finder 选一个文件 Cmd+C，在输入框 Cmd+V → 应识别为附件
3. 在首页输入框：在 Finder 选一个**文件夹** Cmd+C，在输入框 Cmd+V → 应出现 "DIR" 卡片
4. 首页粘贴附件后写一段文字 → 点发送 → 应进入 chat 页面，消息带附件
5. chat 页面输入框：重复 1-3 项验证不回归
6. 校验文件落盘：`ls ~/.renlijia/tmpImage/` 应能看到 `clipboard-*.png` 文件
7. 校验旧目录不再写入：`ls ~/.renlijia/conversations/<新会话 id>/attachments/clipboard/ 2>/dev/null` 应不存在或为空

如果 6、7 任一不符合，停下来排查。

- [ ] **Step 6: 提交**

```bash
git add src/components/home/HomeTaskComposerCard.tsx
git commit -m "feat(home): support clipboard paste of images, file paths and folder paths"
```

---

## Self-Review

- **Spec coverage**：
  - 后端 tmpImage 命令 → Task 1 ✓
  - 删旧命令 → Task 1 ✓
  - 前端 tauri.ts 适配 → Task 2 ✓
  - useChatAttachments 适配 → Task 2 ✓
  - useComposerPaste hook → Task 3 ✓
  - ChatBottomArea 改造 → Task 4 ✓
  - HomeTaskComposerCard 接入 + submit 透传 → Task 5 ✓
  - 后端测试 → Task 1 Step 1 ✓
  - 前端 hook 测试 → Task 3 Step 1 ✓
  - 手动 UI 验证 → Task 5 Step 5 ✓

- **Placeholder scan**：每步给出实际代码或命令；无 TBD/TODO/「类似 X」。

- **Type consistency**：
  - `saveClipboardImageToTmp(bytes, mimeType)` — Task 2 / 3 一致
  - `useComposerPaste({ onAttachmentsResolved })` — Task 3/4/5 一致
  - `PendingFileInfo`（来自 `useChat`）字段：`{id, fileName, filePath, kind, fileSize, fileType, mimeType}` — 与 chat 端 `ChatBottomArea.tsx:176` 既有映射保持一致

---

## 执行选择

**Plan complete and saved to `docs/superpowers/plans/2026-04-29-composer-paste-unified-plan.md`. Two execution options:**

**1. Subagent-Driven (recommended)** — 每个 task 派一个新 subagent，task 间复审，迭代快

**2. Inline Execution** — 在当前会话用 executing-plans 批量执行，关键节点暂停 review

哪种？
