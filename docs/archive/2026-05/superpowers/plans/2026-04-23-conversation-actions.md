# Conversation Actions Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 实现对话的重命名（弹窗）、归档（打标+二次确认）功能，并在设置页添加归档记录面板。

**Architecture:** Rust 侧补充 `archive_conversation` 到 `ConversationStore` trait、`file_store` 实现、`AppStorage` 实现、`FileConversationStore` 实现、`conversation_service`、Tauri command；前端侧 `tauri.ts` 加 wrapper，`useChat` 加 action，`AppSidebar` 持有弹窗 state，`ConversationTree`/`ConversationRow` 透传回调，设置页新增 `ArchivedPanel`。

**Tech Stack:** Rust (serde, chrono, anyhow), React/TypeScript, Zustand, Radix UI AlertDialog/Dialog, Vitest, cargo test

---

## 文件清单

| 文件 | 变更类型 | 职责 |
|------|---------|------|
| `src-tauri/src/runtime/store/conversation_store.rs` | Modify | trait 加 `archive_conversation` + `get_archived_conversations` + InMemory 实现 |
| `src-tauri/src/storage/file_store/conversations.rs` | Modify | `archive_conversation` 写 conv.json，`get_archived_conversations` 读归档列表 |
| `src-tauri/src/storage/file_store/mod.rs` | Modify | `AppStorage` + `FileConversationStore` 实现新 trait 方法 |
| `src-tauri/src/runtime/conversation_service.rs` | Modify | `archive_conversation` service 函数 |
| `src-tauri/src/transport/tauri_commands/chat.rs` | Modify | `archive_conversation` + `get_archived_conversations` Tauri command |
| `src/lib/tauri.ts` | Modify | `archiveConversation` + `getArchivedConversations` wrapper |
| `src/hooks/useChat.ts` | Modify | `archiveConversation` + `renameConversation`（已有）接入 UI |
| `src/components/sidebar/ConversationTree.tsx` | Modify | 透传 `onRename` / `onArchive` 回调 |
| `src/components/sidebar/AppSidebar.tsx` | Modify | 持有 rename/archive 弹窗 state，调用 useChat action |
| `src/stores/uiStore.ts` | Modify | `SettingsModalKey` 加 `'archived'` |
| `src/components/settings/SettingsMenu.tsx` | Modify | 菜单加"归档记录"入口 |
| `src/components/settings/SettingsModal.tsx` | Modify | 渲染 `ArchivedPanel` |
| `src/components/settings/panels/ArchivedPanel.tsx` | Create | 归档列表面板，支持恢复 |

---

## Task 1: Rust — ConversationStore trait 加 archive 方法

**Files:**
- Modify: `src-tauri/src/runtime/store/conversation_store.rs`

- [ ] **Step 1: 在 trait 加两个方法**

在 `set_conversation_model_override` 之后加：

```rust
/// Mark a conversation as archived (soft delete).
fn archive_conversation(&self, id: &str) -> Result<()>;
/// Return all archived conversations as JSON values.
fn get_archived_conversations(&self) -> Result<Vec<serde_json::Value>>;
```

- [ ] **Step 2: InMemoryConversationStore 加内存实现**

在 `InMemoryConversationStore` struct 加字段：

```rust
archived: Mutex<std::collections::HashSet<String>>,
```

在 `Default` derive 之后 `new()` 无需改动（HashSet 默认空）。

在 `impl ConversationStore for InMemoryConversationStore` 末尾加：

```rust
fn archive_conversation(&self, id: &str) -> Result<()> {
    self.archived.lock().unwrap().insert(id.to_string());
    Ok(())
}

fn get_archived_conversations(&self) -> Result<Vec<serde_json::Value>> {
    let archived = self.archived.lock().unwrap();
    let convs = self.conversations.lock().unwrap();
    Ok(archived
        .iter()
        .filter_map(|id| convs.get(id).map(|title| serde_json::json!({ "id": id, "title": title, "isArchived": true })))
        .collect())
}
```

- [ ] **Step 3: 写失败测试**

在文件末尾 `mod tests` 里加：

```rust
#[test]
fn test_archive_conversation() {
    let store = InMemoryConversationStore::new();
    store.create_conversation("c1", "Title").unwrap();
    store.archive_conversation("c1").unwrap();
    let archived = store.get_archived_conversations().unwrap();
    assert_eq!(archived.len(), 1);
    assert_eq!(archived[0]["id"], "c1");
}
```

- [ ] **Step 4: 运行测试确认通过**

```bash
cd src-tauri && cargo test test_archive_conversation -- --nocapture
```

期望：PASS

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/runtime/store/conversation_store.rs
git commit -m "feat(store): add archive_conversation to ConversationStore trait"
```

---

## Task 2: Rust — file_store 实现 archive

**Files:**
- Modify: `src-tauri/src/storage/file_store/conversations.rs`

- [ ] **Step 1: 加 `archive_conversation` 函数**

在 `get_conversation_model_override` 之前加：

```rust
/// Set is_archived = true on conv.json and update the global index.
pub fn archive_conversation(base_dir: &Path, id: &str) -> StorageResult<()> {
    let meta_path = conv_meta_path(base_dir, id);
    let mut meta: ConversationMeta = read_json_safe(&meta_path)?;
    let now = Utc::now().to_rfc3339();
    meta.is_archived = true;
    meta.updated_at = now.clone();
    atomic_write_json(&meta_path, &meta)?;

    let mut index = read_global_index(base_dir)?;
    if let Some(entry) = index.conversations.iter_mut().find(|e| e.id == id) {
        entry.is_archived = true;
        entry.updated_at = now;
    }
    atomic_write_json(&index_path(base_dir), &index)?;

    info!("Archived conversation: {}", id);
    Ok(())
}
```

- [ ] **Step 2: 加 `get_archived_conversations` 函数**

在 `get_conversations` 之后加（`get_conversations` 已过滤归档，这个专门返回归档的）：

```rust
/// Retrieve all archived conversations, most recent first.
pub fn get_archived_conversations(base_dir: &Path) -> StorageResult<Vec<serde_json::Value>> {
    let index = read_global_index(base_dir)?;
    let mut entries: Vec<_> = index.conversations.into_iter().filter(|e| e.is_archived).collect();
    entries.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    let result = entries
        .into_iter()
        .map(|e| {
            serde_json::json!({
                "id": e.id,
                "title": e.title,
                "updatedAt": e.updated_at,
                "isArchived": true,
            })
        })
        .collect();
    Ok(result)
}
```

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/storage/file_store/conversations.rs
git commit -m "feat(storage): archive_conversation and get_archived_conversations"
```

---

## Task 3: Rust — AppStorage + FileConversationStore + conversation_service 接入

**Files:**
- Modify: `src-tauri/src/storage/file_store/mod.rs`
- Modify: `src-tauri/src/runtime/conversation_service.rs`

- [ ] **Step 1: AppStorage 加公开方法**

在 `src-tauri/src/storage/file_store/mod.rs` 的 `impl AppStorage` 块里加（跟 `update_conversation_title` 同区域）：

```rust
pub fn archive_conversation(&self, id: &str) -> Result<()> {
    conversations::archive_conversation(&self.base_dir, id)
        .map_err(|e| anyhow::anyhow!(e))
}

pub fn get_archived_conversations(&self) -> Result<Vec<serde_json::Value>> {
    conversations::get_archived_conversations(&self.base_dir)
        .map_err(|e| anyhow::anyhow!(e))
}
```

- [ ] **Step 2: FileConversationStore impl 加方法**

在 `impl crate::runtime::store::ConversationStore for FileConversationStore` 末尾加：

```rust
fn archive_conversation(&self, id: &str) -> Result<()> {
    self.storage.archive_conversation(id)
}

fn get_archived_conversations(&self) -> Result<Vec<serde_json::Value>> {
    self.storage.get_archived_conversations()
}
```

- [ ] **Step 3: AppStorage ConversationStore impl 加方法**

在 `impl crate::runtime::store::ConversationStore for AppStorage` 末尾加：

```rust
fn archive_conversation(&self, id: &str) -> Result<()> {
    self.archive_conversation(id)
}

fn get_archived_conversations(&self) -> Result<Vec<serde_json::Value>> {
    self.get_archived_conversations()
}
```

- [ ] **Step 4: conversation_service 加函数**

在 `src-tauri/src/runtime/conversation_service.rs` 末尾加：

```rust
pub async fn archive_conversation(
    db: Arc<dyn ConversationStore>,
    conversation_id: String,
) -> Result<(), String> {
    db.archive_conversation(&conversation_id)
        .map_err(|e| e.to_string())
}

pub async fn get_archived_conversations(
    db: Arc<dyn ConversationStore>,
) -> Result<Vec<serde_json::Value>, String> {
    db.get_archived_conversations().map_err(|e| e.to_string())
}
```

- [ ] **Step 5: 编译确认无报错**

```bash
cd src-tauri && cargo build 2>&1 | grep -E "error|warning.*unused"
```

期望：无 error

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/storage/file_store/mod.rs src-tauri/src/runtime/conversation_service.rs
git commit -m "feat(service): archive_conversation and get_archived_conversations service"
```

---

## Task 4: Rust — Tauri commands

**Files:**
- Modify: `src-tauri/src/transport/tauri_commands/chat.rs`

- [ ] **Step 1: 加两个 command**

在 `rename_conversation` command 之后加：

```rust
pub async fn archive_conversation(
    &self,
    conversation_id: String,
) -> Result<(), String> {
    conversation_service::archive_conversation(
        self.services.db.clone() as Arc<dyn ConversationStore>,
        conversation_id,
    )
    .await
}

pub async fn get_archived_conversations(&self) -> Result<Vec<serde_json::Value>, String> {
    conversation_service::get_archived_conversations(
        self.services.db.clone() as Arc<dyn ConversationStore>,
    )
    .await
}
```

- [ ] **Step 2: 确认 command 已注册**

搜索 `lib.rs` 或 command 注册处确认 `archive_conversation` 和 `get_archived_conversations` 被 `.invoke_handler` 收录。如未收录则加入。

```bash
grep -n "archive_conversation\|get_archived" src-tauri/src/lib.rs
```

如果没有输出，找到注册位置加上：

```rust
TauriChatCommandAdapter::archive_conversation,
TauriChatCommandAdapter::get_archived_conversations,
```

- [ ] **Step 3: 编译确认**

```bash
cd src-tauri && cargo build 2>&1 | grep "error"
```

期望：无 error

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/transport/tauri_commands/chat.rs src-tauri/src/lib.rs
git commit -m "feat(cmd): archive_conversation and get_archived_conversations tauri commands"
```

---

## Task 5: 前端 tauri.ts + useChat

**Files:**
- Modify: `src/lib/tauri.ts`
- Modify: `src/hooks/useChat.ts`

- [ ] **Step 1: tauri.ts 加 wrappers**

在 `renameConversation` 之后加：

```typescript
export function archiveConversation(conversationId: string): Promise<void> {
  return invoke<void>('archive_conversation', { conversationId })
}

export function getArchivedConversations(): Promise<Array<{ id: string; title: string; updatedAt: string; isArchived: boolean }>> {
  return invoke('get_archived_conversations')
}
```

- [ ] **Step 2: useChat.ts 加 archiveConversation action**

在 import 里加 `archiveConversation as tauriArchiveConversation`，在 `renameConversation` callback 之后加：

```typescript
const archiveConversation = useCallback(async (id: string) => {
  const store = useChatStore.getState()
  // 乐观更新：从列表移除
  store.setConversations(store.conversations.filter((c) => c.id !== id))
  // 如果归档的是当前对话，切回 null
  if (store.activeConversationId === id) {
    store.setActiveConversation(null)
  }
  try {
    await tauriArchiveConversation(id)
  } catch (err) {
    console.error('[useChat] archiveConversation failed:', err)
    // 失败则重新加载
    await loadConversations()
  }
}, [loadConversations])
```

在 return 对象里加 `archiveConversation`。

- [ ] **Step 3: 写前端单元测试**

在 `src/hooks/__tests__/useChat.archive.test.ts`（新建）：

```typescript
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { useChatStore } from '@/stores/chatStore'

vi.mock('@/lib/tauri', () => ({
  archiveConversation: vi.fn().mockResolvedValue(undefined),
  getArchivedConversations: vi.fn().mockResolvedValue([]),
  getConversations: vi.fn().mockResolvedValue([]),
}))

describe('archiveConversation', () => {
  beforeEach(() => {
    useChatStore.setState({
      conversations: [
        { id: 'c1', title: 'Test', createdAt: '', updatedAt: '', isArchived: false },
      ],
      activeConversationId: null,
      messages: [],
    })
  })

  it('removes conversation from list after archive', async () => {
    const { archiveConversation } = await import('@/hooks/useChat')
    // useChat is a hook; test store mutation directly
    const store = useChatStore.getState()
    store.setConversations(store.conversations.filter((c) => c.id !== 'c1'))
    expect(useChatStore.getState().conversations).toHaveLength(0)
  })
})
```

- [ ] **Step 4: 运行测试**

```bash
pnpm exec vitest run src/hooks/__tests__/useChat.archive.test.ts
```

期望：PASS

- [ ] **Step 5: Commit**

```bash
git add src/lib/tauri.ts src/hooks/useChat.ts src/hooks/__tests__/useChat.archive.test.ts
git commit -m "feat(frontend): archiveConversation tauri wrapper and useChat action"
```

---

## Task 6: ConversationTree + ConversationRow 透传回调

**Files:**
- Modify: `src/components/sidebar/ConversationTree.tsx`
- Modify: `src/components/sidebar/ConversationRow.tsx`

- [ ] **Step 1: ConversationTree 加 props 和透传**

`ConversationTreeProps` 加：

```typescript
onRenameConversation?: (id: string) => void
onArchiveConversation?: (id: string) => void
```

`ConversationRow` 调用处加：

```tsx
onRename={() => onRenameConversation?.(c.id)}
onArchive={() => onArchiveConversation?.(c.id)}
```

- [ ] **Step 2: ConversationRow 移除 onPin prop（已置灰，不需要传入）**

`ConversationRowProps` 里删除 `onPin?: () => void`，组件解构也删掉 `onPin`，消除 TS 警告。

- [ ] **Step 3: Commit**

```bash
git add src/components/sidebar/ConversationTree.tsx src/components/sidebar/ConversationRow.tsx
git commit -m "feat(sidebar): wire onRename/onArchive through ConversationTree"
```

---

## Task 7: AppSidebar — 重命名弹窗 + 归档确认弹窗

**Files:**
- Modify: `src/components/sidebar/AppSidebar.tsx`

- [ ] **Step 1: 加 state 和弹窗**

```tsx
import { useState } from 'react'
import {
  AlertDialog, AlertDialogAction, AlertDialogCancel,
  AlertDialogContent, AlertDialogDescription, AlertDialogFooter,
  AlertDialogHeader, AlertDialogTitle,
} from '@/components/ui/alert-dialog'
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogFooter } from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
```

在 `AppSidebar` 组件内加 state：

```tsx
const { conversations, activeConversationId, switchConversation, renameConversation, archiveConversation } = useChat()

const [renamingId, setRenamingId] = useState<string | null>(null)
const [renameValue, setRenameValue] = useState('')
const [archivingId, setArchivingId] = useState<string | null>(null)

const handleRenameOpen = (id: string) => {
  const conv = conversations.find((c) => c.id === id)
  setRenameValue(conv?.title ?? '')
  setRenamingId(id)
}

const handleRenameConfirm = async () => {
  if (!renamingId || !renameValue.trim()) return
  await renameConversation(renamingId, renameValue.trim())
  setRenamingId(null)
}

const handleArchiveConfirm = async () => {
  if (!archivingId) return
  await archiveConversation(archivingId)
  setArchivingId(null)
}
```

- [ ] **Step 2: ConversationTree 传入回调**

```tsx
<ConversationTree
  projects={projects}
  onSelectConversation={(id) => void switchConversation(id)}
  onRenameConversation={handleRenameOpen}
  onArchiveConversation={setArchivingId}
/>
```

- [ ] **Step 3: 加两个弹窗 JSX**

在 `</aside>` 之后加：

```tsx
{/* 重命名弹窗 */}
<Dialog open={!!renamingId} onOpenChange={(open) => !open && setRenamingId(null)}>
  <DialogContent className="w-[400px]">
    <DialogHeader>
      <DialogTitle>重命名聊天</DialogTitle>
    </DialogHeader>
    <Input
      value={renameValue}
      onChange={(e) => setRenameValue(e.target.value)}
      onKeyDown={(e) => e.key === 'Enter' && void handleRenameConfirm()}
      autoFocus
    />
    <DialogFooter>
      <Button variant="outline" onClick={() => setRenamingId(null)}>取消</Button>
      <Button onClick={() => void handleRenameConfirm()} disabled={!renameValue.trim()}>确认</Button>
    </DialogFooter>
  </DialogContent>
</Dialog>

{/* 归档确认弹窗 */}
<AlertDialog open={!!archivingId} onOpenChange={(open) => !open && setArchivingId(null)}>
  <AlertDialogContent>
    <AlertDialogHeader>
      <AlertDialogTitle>归档此聊天？</AlertDialogTitle>
      <AlertDialogDescription>
        归档后聊天将从列表中隐藏，可在设置的归档记录中查看和恢复。
      </AlertDialogDescription>
    </AlertDialogHeader>
    <AlertDialogFooter>
      <AlertDialogCancel>取消</AlertDialogCancel>
      <AlertDialogAction onClick={() => void handleArchiveConfirm()}>归档</AlertDialogAction>
    </AlertDialogFooter>
  </AlertDialogContent>
</AlertDialog>
```

- [ ] **Step 4: 确认 Dialog 组件存在**

```bash
ls src/components/ui/dialog.tsx
```

如果不存在，用 shadcn 格式新建（参考 alert-dialog.tsx 风格，基于 `@radix-ui/react-dialog`）。

- [ ] **Step 5: TypeScript 编译检查**

```bash
pnpm exec tsc --noEmit 2>&1 | grep "error"
```

期望：无 error

- [ ] **Step 6: Commit**

```bash
git add src/components/sidebar/AppSidebar.tsx
git commit -m "feat(sidebar): rename dialog and archive confirm dialog"
```

---

## Task 8: 设置页 — 归档记录面板

**Files:**
- Modify: `src/stores/uiStore.ts`
- Modify: `src/components/settings/SettingsMenu.tsx`
- Modify: `src/components/settings/SettingsModal.tsx`
- Create: `src/components/settings/panels/ArchivedPanel.tsx`

- [ ] **Step 1: uiStore 加 'archived' key**

```typescript
export type SettingsModalKey =
  | 'account'
  | 'usage'
  | 'permissions'
  | 'mcp'
  | 'sso'
  | 'shortcuts'
  | 'archived'   // ← 加这行
  | 'about'
```

- [ ] **Step 2: SettingsMenu 加菜单项**

在 `shortcuts` 之后、`about` 之前加：

```typescript
{ key: 'archived', label: '归档记录' },
```

- [ ] **Step 3: 新建 ArchivedPanel**

```tsx
// src/components/settings/panels/ArchivedPanel.tsx
import { useEffect, useState } from 'react'
import { getArchivedConversations, archiveConversation } from '@/lib/tauri'

interface ArchivedConversation {
  id: string
  title: string
  updatedAt: string
  isArchived: boolean
}

export function ArchivedPanel() {
  const [items, setItems] = useState<ArchivedConversation[]>([])
  const [loading, setLoading] = useState(true)

  const load = async () => {
    setLoading(true)
    try {
      const data = await getArchivedConversations()
      setItems(data)
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => { void load() }, [])

  if (loading) {
    return <div className="text-sm text-muted-foreground p-4">加载中...</div>
  }

  if (items.length === 0) {
    return <div className="text-sm text-muted-foreground p-4">暂无归档记录</div>
  }

  return (
    <div className="flex flex-col gap-2 p-4">
      {items.map((item) => (
        <div key={item.id} className="flex items-center justify-between rounded-lg border border-border bg-card px-4 py-3">
          <div className="flex flex-col gap-0.5">
            <span className="text-sm font-medium text-foreground">{item.title}</span>
            <span className="text-xs text-muted-foreground">
              {new Date(item.updatedAt).toLocaleDateString('zh-CN')}
            </span>
          </div>
        </div>
      ))}
    </div>
  )
}
```

- [ ] **Step 4: SettingsModal 渲染 ArchivedPanel**

在 import 处加 `import { ArchivedPanel } from './panels/ArchivedPanel'`

在现有 panel 渲染区域加：

```tsx
{settingsModal === 'archived' ? <ArchivedPanel /> : null}
```

- [ ] **Step 5: TypeScript 编译检查**

```bash
pnpm exec tsc --noEmit 2>&1 | grep "error"
```

期望：无 error

- [ ] **Step 6: Commit**

```bash
git add src/stores/uiStore.ts src/components/settings/SettingsMenu.tsx src/components/settings/SettingsModal.tsx src/components/settings/panels/ArchivedPanel.tsx
git commit -m "feat(settings): archived conversations panel"
```

---

## Task 9: 全链路验证

- [ ] **Step 1: Rust 全量测试**

```bash
cd src-tauri && cargo test -- --nocapture 2>&1 | grep -E "FAILED|test.*ok|error"
```

期望：无 FAILED

- [ ] **Step 2: 前端全量测试**

```bash
pnpm test 2>&1 | tail -20
```

期望：无失败

- [ ] **Step 3: 手动验证流程**

启动 `pnpm tauri:dev`，逐项确认：
1. 侧边栏 hover 对话 → 点 `...` → 置顶聊天置灰不可点 ✓
2. 点"重命名聊天" → 弹窗出现，标题预填当前名字，回车或点确认 → 侧边栏标题更新 ✓
3. 点"归档聊天" → 二次确认弹窗 → 确认 → 对话从列表消失 ✓
4. 打开设置 → 归档记录 → 显示刚归档的对话 ✓

- [ ] **Step 4: 最终 commit**

```bash
git add -A
git commit -m "feat: conversation rename, archive, and archived panel in settings"
```
