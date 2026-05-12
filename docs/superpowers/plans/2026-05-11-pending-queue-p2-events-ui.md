# Pending Message Queue P2 — Events + Tauri Commands + Frontend UI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Wire `PendingQueueManager` from P1 into the Tauri transport layer (event adapter + 2 IPC commands), then build the frontend store and UI (chips above composer, per-item × button).

**Architecture:** Backend `PendingQueueManager` singleton attached to `AppHandle::manage`. Event adapter maps 4 new `RuntimeEventKind` variants to legacy Tauri events. Frontend `pendingStore` (zustand) subscribes to events; `PendingChips` component renders per-session.

**Tech Stack:** Rust + Tauri 2.x, React + TypeScript, zustand, Tailwind (theme variables only per CLAUDE.md).

**Spec reference:** §8 (event protocol), §9 (frontend implementation), §13.3 PR2

**Prerequisites:** P1 (`runtime/pending/` module) merged.

---

## File Structure

Create:

- `src-tauri/src/transport/tauri_commands/pending.rs` — 2 Tauri commands (`pending_snapshot_for_session`, `pending_remove_item`)
- `src/stores/pendingStore.ts` — zustand store
- `src/types/pending.ts` — TS types
- `src/features/chat/PendingChips.tsx` — chips container
- `src/features/chat/PendingChip.tsx` — single chip component
- `src/hooks/usePendingEventListener.ts` — top-level event subscriber
- Tests: `src/stores/pendingStore.test.ts`, `src/features/chat/PendingChips.test.tsx`, `src/features/chat/PendingChip.test.tsx`

Modify:

- `src-tauri/src/transport/tauri_event_adapter.rs` — add 4 legacy event mappings
- `src-tauri/src/transport/tauri_commands/mod.rs` — export `pending`
- `src-tauri/src/lib.rs` — `app.manage(Arc<PendingQueueManager>)`, register 2 commands in `invoke_handler!`
- `src/lib/tauri.ts` — add 4 event constants + 2 IPC wrappers + listen helpers
- `src/App.tsx` — mount `usePendingEventListener` at top
- `src/i18n/zh-CN.json` + `src/i18n/en-US.json` — add `chat.pending.*` keys
- `src/features/chat/ChatBottomArea.tsx` — render `<PendingChips>` above composer

---

## Task 1: Backend event adapter — map 4 RuntimeEventKind variants

**Files:**
- Modify: `src-tauri/src/transport/tauri_event_adapter.rs`

- [ ] **Step 1: Find the `match &event.kind` block**

Open `src-tauri/src/transport/tauri_event_adapter.rs`. Find the closing of the existing match — there's a final `_ => None,` or similar. We'll add 4 arms before it.

- [ ] **Step 2: Add 4 mapping arms**

Add (after `RuntimeEventKind::TurnCompleted { ... }` arm, before any catch-all):

```rust
        RuntimeEventKind::PendingSnapshot { items } => Some(LegacyEvent {
            name: "pending:snapshot".to_string(),
            payload: json!({
                "sessionId": conversation_id,
                "items": items,
            }),
        }),
        RuntimeEventKind::PendingQueued { item } => Some(LegacyEvent {
            name: "pending:queued".to_string(),
            payload: json!({
                "sessionId": conversation_id,
                "item": item,
            }),
        }),
        RuntimeEventKind::PendingDrained { drained_ids } => Some(LegacyEvent {
            name: "pending:drained".to_string(),
            payload: json!({
                "sessionId": conversation_id,
                "drainedIds": drained_ids,
            }),
        }),
        RuntimeEventKind::PendingRemoved { item_id } => Some(LegacyEvent {
            name: "pending:removed".to_string(),
            payload: json!({
                "sessionId": conversation_id,
                "itemId": item_id,
            }),
        }),
```

- [ ] **Step 3: Verify it compiles**

Run: `cd src-tauri && cargo check --lib`

Expected: compiles cleanly. The `PendingItem` serde already produces camelCase (per P1 Task 2).

- [ ] **Step 4: Add adapter test**

Append to existing `src-tauri/src/transport/tauri_event_adapter.rs` (or its `#[cfg(test)] mod tests` block — find where existing event mapping tests live; likely below the `pub fn map_runtime_event` function or via `tests/` dir). Add:

```rust
#[cfg(test)]
mod pending_event_tests {
    use super::*;
    use crate::runtime::ids::{RunId, SessionId};
    use crate::runtime::pending::{PendingItem, PendingSource};

    fn evt(kind: RuntimeEventKind) -> RuntimeEvent {
        RuntimeEvent::new(SessionId::new("conv-1"), RunId::new("run-1"), kind)
    }

    #[test]
    fn pending_snapshot_maps_to_legacy_event() {
        let e = evt(RuntimeEventKind::PendingSnapshot { items: vec![] });
        let m = map_runtime_event(&e).expect("mapped");
        assert_eq!(m.name, "pending:snapshot");
        assert_eq!(m.payload["sessionId"], "conv-1");
        assert!(m.payload["items"].is_array());
    }

    #[test]
    fn pending_queued_carries_item() {
        let item = PendingItem {
            id: "p-1".into(),
            source: PendingSource::App,
            text: "hi".into(),
            sender_nick: None,
            attachments: vec![],
            received_at: "2026-05-11T03:21:00Z".into(),
        };
        let e = evt(RuntimeEventKind::PendingQueued { item });
        let m = map_runtime_event(&e).unwrap();
        assert_eq!(m.name, "pending:queued");
        assert_eq!(m.payload["item"]["id"], "p-1");
    }

    #[test]
    fn pending_drained_carries_ids() {
        let e = evt(RuntimeEventKind::PendingDrained {
            drained_ids: vec!["a".into(), "b".into()],
        });
        let m = map_runtime_event(&e).unwrap();
        assert_eq!(m.name, "pending:drained");
        assert_eq!(m.payload["drainedIds"][0], "a");
    }

    #[test]
    fn pending_removed_carries_item_id() {
        let e = evt(RuntimeEventKind::PendingRemoved {
            item_id: "p-1".into(),
        });
        let m = map_runtime_event(&e).unwrap();
        assert_eq!(m.name, "pending:removed");
        assert_eq!(m.payload["itemId"], "p-1");
    }
}
```

- [ ] **Step 5: Run tests**

Run: `cd src-tauri && cargo test --lib pending_event_tests`

Expected: 4 tests PASS.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/transport/tauri_event_adapter.rs
git commit -m "feat(transport): map 4 pending RuntimeEventKind to legacy events"
```

---

## Task 2: Tauri commands — pending_snapshot_for_session + pending_remove_item

**Files:**
- Create: `src-tauri/src/transport/tauri_commands/pending.rs`
- Modify: `src-tauri/src/transport/tauri_commands/mod.rs`
- Modify: `src-tauri/src/lib.rs` (register commands)

- [ ] **Step 1: Inspect existing command pattern**

Open `src-tauri/src/transport/tauri_commands/chat.rs` and find one short command (e.g., `is_agent_busy` or `get_conversations`) to learn the pattern (`#[tauri::command]` + `app.state::<...>()`).

Run: `grep -n "fn is_agent_busy\|pub async fn is_agent_busy" src-tauri/src/transport/tauri_commands/chat.rs`

This tells us the function signature template.

- [ ] **Step 2: Create the new command file**

Create `src-tauri/src/transport/tauri_commands/pending.rs`:

```rust
//! Tauri command surface for the pending message queue.

use std::sync::Arc;

use tauri::AppHandle;
use tauri::Manager;

use crate::runtime::ids::SessionId;
use crate::runtime::pending::{PendingItem, PendingQueueManager};

#[tauri::command]
pub async fn pending_snapshot_for_session(
    app: AppHandle,
    session_id: String,
) -> Result<Vec<PendingItem>, String> {
    let mgr = app
        .try_state::<Arc<PendingQueueManager>>()
        .ok_or_else(|| "PendingQueueManager not initialised".to_string())?
        .inner()
        .clone();
    Ok(mgr.snapshot(&SessionId::new(session_id)).await)
}

#[tauri::command]
pub async fn pending_remove_item(
    app: AppHandle,
    session_id: String,
    item_id: String,
) -> Result<bool, String> {
    let mgr = app
        .try_state::<Arc<PendingQueueManager>>()
        .ok_or_else(|| "PendingQueueManager not initialised".to_string())?
        .inner()
        .clone();
    mgr.remove_item(&SessionId::new(session_id), &item_id)
        .await
        .map_err(|e| format!("{e:#}"))
}
```

- [ ] **Step 3: Export from mod.rs**

Modify `src-tauri/src/transport/tauri_commands/mod.rs` — add `pub mod pending;` next to other command modules. Verify pattern by reading existing entries:

```bash
head -30 src-tauri/src/transport/tauri_commands/mod.rs
```

Add `pub mod pending;` in alphabetical position.

- [ ] **Step 4: Register in invoke_handler**

In `src-tauri/src/lib.rs`, find the `tauri::generate_handler![` block (around line 647). Add at the end (before the closing `])`):

```rust
            // Pending queue commands
            crate::transport::tauri_commands::pending::pending_snapshot_for_session,
            crate::transport::tauri_commands::pending::pending_remove_item,
```

- [ ] **Step 5: Verify it compiles**

Run: `cd src-tauri && cargo check --lib`

Expected: succeeds (manager is `try_state`, so missing-state at runtime returns an error rather than panic).

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/transport/tauri_commands/pending.rs src-tauri/src/transport/tauri_commands/mod.rs src-tauri/src/lib.rs
git commit -m "feat(transport): add pending_snapshot_for_session + pending_remove_item commands"
```

---

## Task 3: Manage PendingQueueManager + production ConvDirResolver in lib.rs

**Files:**
- Modify: `src-tauri/src/lib.rs`
- Create: `src-tauri/src/runtime/pending/aijia_resolver.rs`
- Modify: `src-tauri/src/runtime/pending/mod.rs`

- [ ] **Step 1: Build production ConvDirResolver**

Create `src-tauri/src/runtime/pending/aijia_resolver.rs`:

```rust
//! Production `ConvDirResolver` backed by `AiJiaHome` + `ConversationStore`.

use std::path::PathBuf;
use std::sync::Arc;

use crate::auth::state::UserScope;
use crate::runtime::ids::SessionId;
use crate::runtime::store::ConversationStore;
use crate::storage::AiJiaHome;

use super::queue_manager::ConvDirResolver;

pub struct AiJiaPendingResolver {
    home: AiJiaHome,
    scope: UserScope,
    conv_store: Arc<dyn ConversationStore>,
}

impl AiJiaPendingResolver {
    pub fn new(home: AiJiaHome, scope: UserScope, conv_store: Arc<dyn ConversationStore>) -> Self {
        Self { home, scope, conv_store }
    }
}

impl ConvDirResolver for AiJiaPendingResolver {
    fn conversation_dir(&self, session_id: &SessionId) -> Option<PathBuf> {
        let dir = self
            .home
            .user_conversations_dir(&self.scope)
            .join(session_id.as_str());
        if dir.exists() || std::fs::create_dir_all(&dir).is_ok() {
            Some(dir)
        } else {
            None
        }
    }

    fn is_archived(&self, session_id: &SessionId) -> bool {
        match self.conv_store.get_conversation_meta(session_id.as_str()) {
            Ok(Some(meta)) => meta.is_archived,
            _ => false,
        }
    }

    fn conversations_root(&self) -> PathBuf {
        self.home.user_conversations_dir(&self.scope)
    }
}
```

Note: Verify `ConversationStore::get_conversation_meta` exists. If not, use whatever existing method returns the meta or check archive state. Inspect with:

```bash
grep -n "fn get_conversation_meta\|is_archived" src-tauri/src/runtime/store/conversation_store.rs
```

If the method is named differently, adapt. If unavailable, fall back to reading `conv.json` directly via `crate::storage::file_store::conversations::read_meta` (verify that exists — `grep -n "pub fn read_meta\|pub fn get_meta" src-tauri/src/storage/file_store/conversations.rs`). Use whichever is the public API.

- [ ] **Step 2: Re-export resolver**

Add to `src-tauri/src/runtime/pending/mod.rs`:

```rust
pub mod aijia_resolver;
pub use aijia_resolver::AiJiaPendingResolver;
```

- [ ] **Step 3: Wire manager into lib.rs setup**

In `src-tauri/src/lib.rs`, find the `.setup(|app| { ... })` block where other Arc-managed state (e.g., `ConnectorEngine`, `AgentRuntime`) is created. Add after those (search for `app.manage(` to locate insertion):

```rust
            // PendingQueueManager — must be after RuntimeRunRegistry + ConversationStore
            // are available, and after RuntimeEventBus is constructed.
            let pending_resolver = std::sync::Arc::new(
                crate::runtime::pending::AiJiaPendingResolver::new(
                    aijia_home.clone(),
                    user_scope.clone(),
                    conversation_store.clone() as std::sync::Arc<dyn crate::runtime::store::ConversationStore>,
                ),
            );
            let pending_manager = crate::runtime::pending::PendingQueueManager::new(
                run_registry.clone(),
                runtime_event_bus.clone(),
                pending_resolver,
                crate::runtime::pending::PendingConfig::default(),
            );
            // Restore from disk (best-effort; warn but never panic)
            if let Err(e) = tokio::runtime::Handle::current().block_on(pending_manager.restore_from_disk()) {
                log::warn!("[pending] restore_from_disk failed: {:#}", e);
            }
            app.manage(pending_manager);
```

**Adjust variable names** to match what's in scope at the actual setup location. If `aijia_home` / `user_scope` / `conversation_store` / `run_registry` / `runtime_event_bus` are named differently, search the existing setup block:

```bash
grep -n "app.manage\|let runtime_event_bus\|let run_registry\|let conversation_store\|let aijia_home" src-tauri/src/lib.rs | head -20
```

Use the names that are actually defined.

- [ ] **Step 4: Verify it compiles**

Run: `cd src-tauri && cargo check --lib`

Expected: compiles cleanly. If it errors on `block_on` inside async context — switch to using `tauri::async_runtime::block_on` or move the restore into `tauri::async_runtime::spawn` if `setup` is non-async.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/runtime/pending/aijia_resolver.rs src-tauri/src/runtime/pending/mod.rs src-tauri/src/lib.rs
git commit -m "feat(pending): wire PendingQueueManager into Tauri app state"
```

---

## Task 4: Frontend — TS types

**Files:**
- Create: `src/types/pending.ts`

- [ ] **Step 1: Write the types**

Create `src/types/pending.ts`:

```typescript
export type PendingSource = 'app' | 'im-dingtalk'

export interface PendingAttachment {
  id: string
  filePath: string
  mime?: string | null
  sizeBytes?: number | null
}

export interface PendingItem {
  id: string
  source: PendingSource
  text: string
  senderNick?: string | null
  attachments: PendingAttachment[]
  receivedAt: string
}

export interface PendingSnapshotPayload {
  sessionId: string
  items: PendingItem[]
}

export interface PendingQueuedPayload {
  sessionId: string
  item: PendingItem
}

export interface PendingDrainedPayload {
  sessionId: string
  drainedIds: string[]
}

export interface PendingRemovedPayload {
  sessionId: string
  itemId: string
}
```

- [ ] **Step 2: Type-check**

Run: `pnpm exec tsc --noEmit`

Expected: 0 errors.

- [ ] **Step 3: Commit**

```bash
git add src/types/pending.ts
git commit -m "feat(pending): TS types"
```

---

## Task 5: Frontend — tauri.ts event constants + IPC wrappers + listen helpers

**Files:**
- Modify: `src/lib/tauri.ts`

- [ ] **Step 1: Add event constants**

In `src/lib/tauri.ts`, find the `TAURI_EVENTS` const (~line 27). Add 4 entries:

```typescript
  PENDING_SNAPSHOT: 'pending:snapshot',
  PENDING_QUEUED: 'pending:queued',
  PENDING_DRAINED: 'pending:drained',
  PENDING_REMOVED: 'pending:removed',
```

- [ ] **Step 2: Add IPC wrappers**

Add at the bottom of `src/lib/tauri.ts` (after existing exported functions):

```typescript
import type {
  PendingItem,
  PendingSnapshotPayload,
  PendingQueuedPayload,
  PendingDrainedPayload,
  PendingRemovedPayload,
} from '@/types/pending'

export async function pendingSnapshotForSession(sessionId: string): Promise<PendingItem[]> {
  return invoke<PendingItem[]>('pending_snapshot_for_session', { sessionId })
}

export async function pendingRemoveItem(sessionId: string, itemId: string): Promise<boolean> {
  return invoke<boolean>('pending_remove_item', { sessionId, itemId })
}
```

(Move `import type` lines to the file's existing import block at the top of the file.)

- [ ] **Step 3: Add listen helpers**

Add (alongside `listenStreamingDelta` etc.):

```typescript
export function listenPendingSnapshot(
  handler: (payload: PendingSnapshotPayload) => void,
): Promise<UnlistenFn> {
  return listen<PendingSnapshotPayload>(TAURI_EVENTS.PENDING_SNAPSHOT, (event) =>
    handler(event.payload),
  )
}

export function listenPendingQueued(
  handler: (payload: PendingQueuedPayload) => void,
): Promise<UnlistenFn> {
  return listen<PendingQueuedPayload>(TAURI_EVENTS.PENDING_QUEUED, (event) =>
    handler(event.payload),
  )
}

export function listenPendingDrained(
  handler: (payload: PendingDrainedPayload) => void,
): Promise<UnlistenFn> {
  return listen<PendingDrainedPayload>(TAURI_EVENTS.PENDING_DRAINED, (event) =>
    handler(event.payload),
  )
}

export function listenPendingRemoved(
  handler: (payload: PendingRemovedPayload) => void,
): Promise<UnlistenFn> {
  return listen<PendingRemovedPayload>(TAURI_EVENTS.PENDING_REMOVED, (event) =>
    handler(event.payload),
  )
}
```

- [ ] **Step 4: Type-check**

Run: `pnpm exec tsc --noEmit`

Expected: 0 errors.

- [ ] **Step 5: Commit**

```bash
git add src/lib/tauri.ts
git commit -m "feat(pending): tauri.ts event constants + IPC + listeners"
```

---

## Task 6: pendingStore (zustand) with reducer tests

**Files:**
- Create: `src/stores/pendingStore.ts`
- Create: `src/stores/pendingStore.test.ts`

- [ ] **Step 1: Write the failing test**

Create `src/stores/pendingStore.test.ts`:

```typescript
import { describe, it, expect, beforeEach, vi } from 'vitest'

vi.mock('@/lib/tauri', () => ({
  pendingRemoveItem: vi.fn(async () => true),
}))

import { usePendingStore } from './pendingStore'
import { pendingRemoveItem } from '@/lib/tauri'
import type { PendingItem } from '@/types/pending'

const itemA: PendingItem = {
  id: 'a',
  source: 'app',
  text: 'hello',
  senderNick: null,
  attachments: [],
  receivedAt: '2026-05-11T03:21:00Z',
}
const itemB: PendingItem = { ...itemA, id: 'b', text: 'world' }

describe('pendingStore', () => {
  beforeEach(() => {
    usePendingStore.setState({ bySession: {} })
    vi.clearAllMocks()
  })

  it('applySnapshot replaces items per session', () => {
    usePendingStore.getState().applySnapshot('s1', [itemA, itemB])
    expect(usePendingStore.getState().bySession.s1).toHaveLength(2)
    usePendingStore.getState().applySnapshot('s1', [itemA])
    expect(usePendingStore.getState().bySession.s1).toHaveLength(1)
  })

  it('applyQueued appends if not present, ignores duplicates', () => {
    usePendingStore.getState().applyQueued('s1', itemA)
    usePendingStore.getState().applyQueued('s1', itemA)
    expect(usePendingStore.getState().bySession.s1).toHaveLength(1)
  })

  it('applyDrained clears all items in drainedIds', () => {
    usePendingStore.getState().applySnapshot('s1', [itemA, itemB])
    usePendingStore.getState().applyDrained('s1', ['a'])
    expect(usePendingStore.getState().bySession.s1).toHaveLength(1)
    expect(usePendingStore.getState().bySession.s1[0].id).toBe('b')
  })

  it('applyRemoved removes single item', () => {
    usePendingStore.getState().applySnapshot('s1', [itemA, itemB])
    usePendingStore.getState().applyRemoved('s1', 'a')
    expect(usePendingStore.getState().bySession.s1).toHaveLength(1)
  })

  it('removeItem invokes IPC + waits for event (does not mutate locally)', async () => {
    usePendingStore.getState().applySnapshot('s1', [itemA])
    await usePendingStore.getState().removeItem('s1', 'a')
    expect(pendingRemoveItem).toHaveBeenCalledWith('s1', 'a')
    // Locally still there until event lands
    expect(usePendingStore.getState().bySession.s1).toHaveLength(1)
  })
})
```

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm test src/stores/pendingStore.test.ts`

Expected: FAIL — module not found.

- [ ] **Step 3: Implement the store**

Create `src/stores/pendingStore.ts`:

```typescript
import { create } from 'zustand'

import { pendingRemoveItem } from '@/lib/tauri'
import type { PendingItem } from '@/types/pending'

export interface PendingState {
  bySession: Record<string, PendingItem[]>
  applySnapshot: (sessionId: string, items: PendingItem[]) => void
  applyQueued: (sessionId: string, item: PendingItem) => void
  applyDrained: (sessionId: string, drainedIds: string[]) => void
  applyRemoved: (sessionId: string, itemId: string) => void
  removeItem: (sessionId: string, itemId: string) => Promise<void>
}

export const usePendingStore = create<PendingState>((set) => ({
  bySession: {},

  applySnapshot: (sessionId, items) =>
    set((state) => ({
      bySession: { ...state.bySession, [sessionId]: items },
    })),

  applyQueued: (sessionId, item) =>
    set((state) => {
      const list = state.bySession[sessionId] ?? []
      if (list.some((i) => i.id === item.id)) {
        return state
      }
      return {
        bySession: { ...state.bySession, [sessionId]: [...list, item] },
      }
    }),

  applyDrained: (sessionId, drainedIds) =>
    set((state) => {
      const list = state.bySession[sessionId] ?? []
      const drainedSet = new Set(drainedIds)
      return {
        bySession: {
          ...state.bySession,
          [sessionId]: list.filter((i) => !drainedSet.has(i.id)),
        },
      }
    }),

  applyRemoved: (sessionId, itemId) =>
    set((state) => {
      const list = state.bySession[sessionId] ?? []
      return {
        bySession: {
          ...state.bySession,
          [sessionId]: list.filter((i) => i.id !== itemId),
        },
      }
    }),

  removeItem: async (sessionId, itemId) => {
    // Single source of truth: backend emits pending:removed, applyRemoved fires from the event.
    await pendingRemoveItem(sessionId, itemId)
  },
}))
```

- [ ] **Step 4: Run tests**

Run: `pnpm test src/stores/pendingStore.test.ts`

Expected: 5 tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/stores/pendingStore.ts src/stores/pendingStore.test.ts
git commit -m "feat(pending): pendingStore (zustand) with reducer tests"
```

---

## Task 7: usePendingEventListener hook

**Files:**
- Create: `src/hooks/usePendingEventListener.ts`

- [ ] **Step 1: Write the hook**

Create `src/hooks/usePendingEventListener.ts`:

```typescript
import { useEffect } from 'react'

import {
  listenPendingDrained,
  listenPendingQueued,
  listenPendingRemoved,
  listenPendingSnapshot,
} from '@/lib/tauri'
import { usePendingStore } from '@/stores/pendingStore'

/** Mount once at App level. Subscribes to all 4 pending events and forwards to the store. */
export function usePendingEventListener(): void {
  useEffect(() => {
    const unlisteners: Array<Promise<() => void>> = []

    unlisteners.push(
      listenPendingSnapshot((p) => usePendingStore.getState().applySnapshot(p.sessionId, p.items)),
    )
    unlisteners.push(
      listenPendingQueued((p) => usePendingStore.getState().applyQueued(p.sessionId, p.item)),
    )
    unlisteners.push(
      listenPendingDrained((p) =>
        usePendingStore.getState().applyDrained(p.sessionId, p.drainedIds),
      ),
    )
    unlisteners.push(
      listenPendingRemoved((p) => usePendingStore.getState().applyRemoved(p.sessionId, p.itemId)),
    )

    return () => {
      unlisteners.forEach((p) => {
        p.then((fn) => fn()).catch(() => {})
      })
    }
  }, [])
}
```

- [ ] **Step 2: Type-check**

Run: `pnpm exec tsc --noEmit`

Expected: 0 errors.

- [ ] **Step 3: Mount in App.tsx**

In `src/App.tsx`, find where similar app-wide listeners are mounted (e.g., `useDragDropListener()`). Add:

```tsx
import { usePendingEventListener } from '@/hooks/usePendingEventListener'

// inside the App component, near other top-level hooks:
usePendingEventListener()
```

- [ ] **Step 4: Verify**

Run: `pnpm exec tsc --noEmit && pnpm exec vitest run --passWithNoTests`

Expected: 0 errors.

- [ ] **Step 5: Commit**

```bash
git add src/hooks/usePendingEventListener.ts src/App.tsx
git commit -m "feat(pending): top-level event listener mounted in App.tsx"
```

---

## Task 8: i18n keys

**Files:**
- Modify: `src/i18n/zh-CN.json`
- Modify: `src/i18n/en-US.json`

- [ ] **Step 1: Inspect existing chat.* namespace**

Run: `grep -n '"chat"' src/i18n/zh-CN.json | head -3`

Locate the `chat` namespace block.

- [ ] **Step 2: Add `chat.pending.*` keys to zh-CN**

Inside the `chat` object in `src/i18n/zh-CN.json`, add (preserve sort order):

```json
    "pending": {
      "singleHint": "1 条待处理消息",
      "batchHint": "{{count}} 条待处理消息",
      "removeAria": "移除这条待处理消息",
      "attachmentsCount": "{{count}} 个附件"
    },
```

- [ ] **Step 3: Add to en-US**

Inside the `chat` object in `src/i18n/en-US.json`:

```json
    "pending": {
      "singleHint": "1 message pending",
      "batchHint": "{{count}} messages pending",
      "removeAria": "Remove this pending message",
      "attachmentsCount": "{{count}} attachment(s)"
    },
```

- [ ] **Step 4: Verify JSON parses**

Run: `pnpm exec node -e "require('./src/i18n/zh-CN.json'); require('./src/i18n/en-US.json'); console.log('ok')"`

Expected: prints `ok`.

- [ ] **Step 5: Commit**

```bash
git add src/i18n/zh-CN.json src/i18n/en-US.json
git commit -m "feat(pending): i18n keys for chat.pending.*"
```

---

## Task 9: PendingChip component (single chip)

**Files:**
- Create: `src/features/chat/PendingChip.tsx`
- Create: `src/features/chat/PendingChip.test.tsx`

- [ ] **Step 1: Write the failing test**

Create `src/features/chat/PendingChip.test.tsx`:

```tsx
import { render, screen, fireEvent } from '@testing-library/react'
import { describe, it, expect, vi } from 'vitest'

import { PendingChip } from './PendingChip'
import type { PendingItem } from '@/types/pending'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (k: string) => k }),
}))

const baseItem: PendingItem = {
  id: 'p-1',
  source: 'app',
  text: 'hello world',
  senderNick: null,
  attachments: [],
  receivedAt: '2026-05-11T03:21:00Z',
}

describe('PendingChip', () => {
  it('renders text content', () => {
    render(<PendingChip item={baseItem} onRemove={() => {}} />)
    expect(screen.getByText(/hello world/)).toBeInTheDocument()
  })

  it('renders sender prefix when senderNick is set', () => {
    const item = { ...baseItem, senderNick: '张三' }
    render(<PendingChip item={item} onRemove={() => {}} />)
    expect(screen.getByText(/张三:/)).toBeInTheDocument()
  })

  it('shows attachment icon when attachments present', () => {
    const item: PendingItem = {
      ...baseItem,
      attachments: [{ id: 'a-1', filePath: '/tmp/x.png' }],
    }
    render(<PendingChip item={item} onRemove={() => {}} />)
    expect(screen.getByTestId('pending-chip-attachment-icon')).toBeInTheDocument()
  })

  it('calls onRemove when × clicked', () => {
    const onRemove = vi.fn()
    render(<PendingChip item={baseItem} onRemove={onRemove} />)
    fireEvent.click(screen.getByLabelText('chat.pending.removeAria'))
    expect(onRemove).toHaveBeenCalledTimes(1)
  })

  it('truncates text longer than 30 chars', () => {
    const long = 'a'.repeat(50)
    const item = { ...baseItem, text: long }
    render(<PendingChip item={item} onRemove={() => {}} />)
    const node = screen.getByText(/a+…|a+\.\.\.|a{30}/)
    expect(node).toBeInTheDocument()
  })
})
```

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm test src/features/chat/PendingChip.test.tsx`

Expected: FAIL.

- [ ] **Step 3: Implement the component**

Create `src/features/chat/PendingChip.tsx`:

```tsx
import { Paperclip, X } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import type { PendingItem } from '@/types/pending'

const PREVIEW_MAX = 30

interface Props {
  item: PendingItem
  onRemove: () => void
}

function truncate(text: string, max: number): string {
  if (text.length <= max) return text
  return text.slice(0, max) + '…'
}

export function PendingChip({ item, onRemove }: Props) {
  const { t } = useTranslation()

  return (
    <div
      className="
        inline-flex items-center gap-1.5 max-w-xs
        px-2 py-1 rounded-md
        bg-muted text-muted-foreground text-xs
        border border-border
      "
    >
      {item.senderNick && (
        <span className="font-medium text-foreground shrink-0">
          {item.senderNick}:
        </span>
      )}
      <span className="truncate">{truncate(item.text, PREVIEW_MAX)}</span>
      {item.attachments.length > 0 && (
        <Paperclip
          className="w-3 h-3 shrink-0"
          data-testid="pending-chip-attachment-icon"
        />
      )}
      <button
        type="button"
        onClick={onRemove}
        aria-label={t('chat.pending.removeAria')}
        className="
          ml-0.5 shrink-0
          hover:bg-destructive/10 hover:text-destructive
          rounded p-0.5
          transition-colors
        "
      >
        <X className="w-3 h-3" />
      </button>
    </div>
  )
}
```

**Theme variable check**: only `bg-muted`, `text-muted-foreground`, `border-border`, `text-foreground`, `text-destructive`, `bg-destructive/10` are used (no `bg-white` / `bg-black` / `text-[#xxx]`). Per CLAUDE.md hard rule.

- [ ] **Step 4: Run tests**

Run: `pnpm test src/features/chat/PendingChip.test.tsx`

Expected: 5 tests PASS.

- [ ] **Step 5: Commit**

```bash
git add src/features/chat/PendingChip.tsx src/features/chat/PendingChip.test.tsx
git commit -m "feat(pending): PendingChip component"
```

---

## Task 10: PendingChips container + ChatBottomArea integration

**Files:**
- Create: `src/features/chat/PendingChips.tsx`
- Create: `src/features/chat/PendingChips.test.tsx`
- Modify: `src/features/chat/ChatBottomArea.tsx`

- [ ] **Step 1: Write the failing test**

Create `src/features/chat/PendingChips.test.tsx`:

```tsx
import { render, screen } from '@testing-library/react'
import { describe, it, expect, beforeEach, vi } from 'vitest'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (k: string, opts?: { count?: number }) =>
      opts?.count != null ? `${k}:${opts.count}` : k,
  }),
}))

import { PendingChips } from './PendingChips'
import { usePendingStore } from '@/stores/pendingStore'
import type { PendingItem } from '@/types/pending'

const item = (id: string): PendingItem => ({
  id,
  source: 'app',
  text: `text-${id}`,
  senderNick: null,
  attachments: [],
  receivedAt: '2026-05-11T03:21:00Z',
})

describe('PendingChips', () => {
  beforeEach(() => {
    usePendingStore.setState({ bySession: {} })
  })

  it('renders nothing when no items', () => {
    const { container } = render(<PendingChips sessionId="s1" />)
    expect(container.firstChild).toBeNull()
  })

  it('renders single hint with 1 item', () => {
    usePendingStore.setState({ bySession: { s1: [item('a')] } })
    render(<PendingChips sessionId="s1" />)
    expect(screen.getByText('chat.pending.singleHint')).toBeInTheDocument()
  })

  it('renders batch hint with N items', () => {
    usePendingStore.setState({
      bySession: { s1: [item('a'), item('b'), item('c')] },
    })
    render(<PendingChips sessionId="s1" />)
    expect(screen.getByText('chat.pending.batchHint:3')).toBeInTheDocument()
  })

  it('renders one chip per item', () => {
    usePendingStore.setState({
      bySession: { s1: [item('a'), item('b')] },
    })
    render(<PendingChips sessionId="s1" />)
    expect(screen.getByText(/text-a/)).toBeInTheDocument()
    expect(screen.getByText(/text-b/)).toBeInTheDocument()
  })
})
```

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm test src/features/chat/PendingChips.test.tsx`

Expected: FAIL.

- [ ] **Step 3: Implement the container**

Create `src/features/chat/PendingChips.tsx`:

```tsx
import { useTranslation } from 'react-i18next'

import { usePendingStore } from '@/stores/pendingStore'

import { PendingChip } from './PendingChip'

interface Props {
  sessionId: string
}

export function PendingChips({ sessionId }: Props) {
  const { t } = useTranslation()
  const items = usePendingStore((s) => s.bySession[sessionId] ?? [])
  const removeItem = usePendingStore((s) => s.removeItem)

  if (items.length === 0) return null

  const hint =
    items.length === 1
      ? t('chat.pending.singleHint')
      : t('chat.pending.batchHint', { count: items.length })

  return (
    <div className="flex flex-wrap gap-1.5 px-3 py-2 border-t border-border bg-muted/30">
      <span className="text-xs text-muted-foreground self-center">{hint}</span>
      {items.map((item) => (
        <PendingChip
          key={item.id}
          item={item}
          onRemove={() => removeItem(sessionId, item.id)}
        />
      ))}
    </div>
  )
}
```

- [ ] **Step 4: Run tests**

Run: `pnpm test src/features/chat/PendingChips.test.tsx`

Expected: 4 tests PASS.

- [ ] **Step 5: Mount in ChatBottomArea + snapshot fetch**

Open `src/features/chat/ChatBottomArea.tsx`. Find the JSX root (likely a `<div>` wrapping the composer). Add `<PendingChips sessionId={...} />` immediately above the actual composer textarea.

```tsx
import { PendingChips } from './PendingChips'
import { useEffect } from 'react'
import { pendingSnapshotForSession } from '@/lib/tauri'
import { usePendingStore } from '@/stores/pendingStore'

// ... inside the component, where sessionId is in scope:
useEffect(() => {
  if (!sessionId) return
  pendingSnapshotForSession(sessionId)
    .then((items) => usePendingStore.getState().applySnapshot(sessionId, items))
    .catch((e) => console.warn('[pending] snapshot fetch failed', e))
}, [sessionId])

// ... in JSX, just above the composer input row:
<PendingChips sessionId={sessionId} />
```

The exact wrapper class for the parent depends on the existing ChatBottomArea layout — keep PendingChips at the top of the bottom-area block, above any input controls.

- [ ] **Step 6: Verify**

Run: `pnpm exec tsc --noEmit && pnpm test src/features/chat/`

Expected: 0 type errors; chip tests pass.

- [ ] **Step 7: Commit**

```bash
git add src/features/chat/PendingChips.tsx src/features/chat/PendingChips.test.tsx src/features/chat/ChatBottomArea.tsx
git commit -m "feat(pending): PendingChips container + mount in ChatBottomArea"
```

---

## Task 11: Snapshot push from backend on session switch

In addition to the `pendingSnapshotForSession` IPC pull (Task 10 step 5), it's nice for the backend to also push a snapshot when a session enters memory. We delay this concern: the IPC pull is sufficient for v1. Note in spec §11 follow-up.

Skip — not implementing in P2.

---

## Task 12: End-to-end smoke test (manual)

**Files:** none (manual verification)

- [ ] **Step 1: Start the app**

Run: `pnpm tauri:dev`

- [ ] **Step 2: Verify init logs**

Look for `[pending] restore_from_disk` in the log (no warn line means clean).

- [ ] **Step 3: Trigger a busy session**

Open any conversation, send a long-running message. Before it finishes, send another message. Expected:
- Second message shows up as a chip above the composer
- Once the first turn completes + 1.2s, the chip disappears and the message appears as a user bubble in history

(End-to-end IM channel test is in P3.)

- [ ] **Step 4: Verify × removal**

While a chip is visible, click ×. Expected: chip disappears immediately (after backend round-trip).

- [ ] **Step 5: Commit nothing**

This is verification only.

---

## Self-Review

Spec coverage check:

1. **§8.1 RuntimeEvent → Tauri event mapping** → Task 1 ✓
2. **§8.2 2 IPC commands** → Task 2 ✓
3. **§9.1 pendingStore** → Task 6 ✓
4. **§9.2 event subscription pattern** → Task 7 ✓
5. **§9.3 PendingChips component** → Task 9 + 10 ✓
6. **§9.4 ChatBottomArea integration** → Task 10 ✓
7. **AiJiaPendingResolver wiring** → Task 3 ✓
8. **i18n keys** → Task 8 ✓

Type consistency:
- `PendingItem` field names match between Rust serde camelCase, TS `pending.ts`, and store reducers
- Event names match between Rust adapter strings and `TAURI_EVENTS` constants

Theme variables (CLAUDE.md hard rule):
- All colors used: `bg-muted`, `text-muted-foreground`, `border-border`, `text-foreground`, `text-destructive`, `bg-destructive/10`, `bg-muted/30` — all theme variables ✓

Not in P2 (deferred):
- IM worker integration → P3
- App composer integration → P4
- Multimodal cross-message budget → P5
- Drain hooks into `messages.jsonl` (P3 wires the drain dispatcher to `TauriChatCommandAdapter::send_chat_request` so messages get persisted as N user messages)
