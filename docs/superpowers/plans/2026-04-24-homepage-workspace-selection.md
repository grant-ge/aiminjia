# Homepage Workspace Selection Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 首页输入框显示默认工作目录，用户可点击选择目录，发送时新建 conversation 并绑定所选目录。

**Architecture:** 新增 Rust command `get_default_folder` 返回 `~/.renlijia/defaultFolder`；前端新建 `homeStore`（localStorage persist）记住上次选择；`HomeTaskComposerCard` 改造为：初始化时读 store / 调默认目录 API，发送时先创建 conversation 再 authorize 再 sendUserMessage。

**Tech Stack:** Rust/Tauri 2.x, React, Zustand (plain create + localStorage), TypeScript

---

## File Map

| 文件 | 操作 |
|---|---|
| `src-tauri/src/commands/workspace.rs` | 新增 `get_default_folder` command |
| `src-tauri/src/lib.rs` | 注册 `workspace::get_default_folder` |
| `src/lib/tauri.ts` | 新增 `getDefaultFolder()` |
| `src/stores/homeStore.ts` | 新建，localStorage persist |
| `src/components/home/HomeTaskComposerCard.tsx` | 改造，接入 homeStore + 新发送流程 |
| `src/components/home/__tests__/HomeTaskComposerCard.test.tsx` | 新建单测 |

---

## Task 1: 后端新增 `get_default_folder` command

**Files:**
- Modify: `src-tauri/src/commands/workspace.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: 在 `src-tauri/src/commands/workspace.rs` 末尾追加 command**

在文件末尾（最后一个 `}` 前面没有更多内容之后）添加：

```rust
/// Return the default folder (`~/.renlijia/defaultFolder`) as a workspace ref.
/// The directory is guaranteed to exist because `AiJiaHome::ensure_dirs()` is
/// called at startup.
#[tauri::command]
pub async fn get_default_folder(
    aijia_home: tauri::State<'_, std::sync::Arc<crate::storage::AiJiaHome>>,
) -> Result<serde_json::Value, String> {
    let path = aijia_home.default_folder();
    let display_name = "默认项目".to_string();
    let root_path = path.to_string_lossy().to_string();
    log::info!("[workspace] get_default_folder: {}", root_path);
    Ok(serde_json::json!({
        "id": "default",
        "rootPath": root_path,
        "displayName": display_name,
    }))
}
```

- [ ] **Step 2: 在 `src-tauri/src/lib.rs` 注册该 command**

找到注册 workspace 命��的区域（当前约在第 411-419 行）：

```rust
workspace::pick_local_directory,
// ...
workspace::authorize_local_directory,
workspace::get_authorized_workspace,
workspace::revoke_authorized_workspace,
```

在 `workspace::revoke_authorized_workspace,` 之后添加一行：

```rust
workspace::get_default_folder,
```

- [ ] **Step 3: 编译验证**

```bash
cd src-tauri && cargo build 2>&1 | tail -20
```

期望：`Finished` 且无 error。

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/commands/workspace.rs src-tauri/src/lib.rs
git commit -m "feat(backend): add get_default_folder tauri command"
```

---

## Task 2: 前端 tauri.ts 新增 `getDefaultFolder`

**Files:**
- Modify: `src/lib/tauri.ts`

- [ ] **Step 1: 在 `getAuthorizedWorkspace` 函数之后添加 `getDefaultFolder`**

找到文件中 `getAuthorizedWorkspace` 函数结束后（约第 618 行），插入：

```ts
/**
 * Get the default folder (~/.renlijia/defaultFolder) as a workspace ref.
 * Always returns a value; the directory is guaranteed to exist at startup.
 */
export function getDefaultFolder(): Promise<AuthorizedWorkspaceRef> {
  return invoke<AuthorizedWorkspaceRef>('get_default_folder')
}
```

- [ ] **Step 2: 类型检查**

```bash
pnpm build 2>&1 | tail -20
```

期望：无 TypeScript 错误。

- [ ] **Step 3: Commit**

```bash
git add src/lib/tauri.ts
git commit -m "feat(frontend): add getDefaultFolder tauri wrapper"
```

---

## Task 3: 新建 `homeStore`

**Files:**
- Create: `src/stores/homeStore.ts`

- [ ] **Step 1: 新建文件**

```ts
import { create } from 'zustand'

import type { AuthorizedWorkspaceRef } from '@/lib/tauri'

const STORAGE_KEY = 'aijia-home-workspace'

interface HomeState {
  selectedWorkspace: AuthorizedWorkspaceRef | null
  setSelectedWorkspace: (ws: AuthorizedWorkspaceRef | null) => void
}

function loadFromStorage(): AuthorizedWorkspaceRef | null {
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    if (!raw) return null
    return JSON.parse(raw) as AuthorizedWorkspaceRef
  } catch {
    return null
  }
}

export const useHomeStore = create<HomeState>()((set) => ({
  selectedWorkspace: loadFromStorage(),
  setSelectedWorkspace: (ws) => {
    try {
      if (ws) {
        localStorage.setItem(STORAGE_KEY, JSON.stringify(ws))
      } else {
        localStorage.removeItem(STORAGE_KEY)
      }
    } catch {
      // ignore storage errors
    }
    set({ selectedWorkspace: ws })
  },
}))
```

注意：不使用 zustand/middleware 的 persist，直接手动读写 localStorage，与项目现有的 i18n/LoginPage 模式一致。

- [ ] **Step 2: 类型检查**

```bash
pnpm build 2>&1 | tail -20
```

期望：无错误。

- [ ] **Step 3: Commit**

```bash
git add src/stores/homeStore.ts
git commit -m "feat(store): add homeStore for homepage workspace selection"
```

---

## Task 4: 改造 `HomeTaskComposerCard`

**Files:**
- Modify: `src/components/home/HomeTaskComposerCard.tsx`

- [ ] **Step 1: 完整替换文件内容**

```tsx
/**
 * @designSource design.pen#uq6ga ChatComposerCompact (home page variant)
 *
 * Flow:
 * 1. On mount: load persisted workspace from homeStore, or fetch default folder.
 * 2. On project button click: open folder picker, update homeStore.
 * 3. On submit: create conversation → authorize workspace → send message.
 */
import { useEffect, useRef, useState } from 'react'

import { SkillPopover } from '@/components/chat/SkillPopover'
import { SlashCommandPopover } from '@/components/chat/SlashCommandPopover'
import { ChatComposerCompact } from '@/components/chat-scene/ChatComposerCompact'
import { useChat } from '@/hooks/useChat'
import { useSkillComposer } from '@/hooks/useSkillComposer'
import {
  authorizeLocalDirectory,
  createConversation,
  getDefaultFolder,
  pickLocalDirectory,
  type AuthorizedWorkspaceRef,
} from '@/lib/tauri'
import { useChatStore } from '@/stores/chatStore'
import { useHomeStore } from '@/stores/homeStore'
import { useUiStore } from '@/stores/uiStore'

export function HomeTaskComposerCard() {
  const [value, setValue] = useState('')
  const textareaRef = useRef<HTMLTextAreaElement>(null)
  const { sendUserMessage } = useChat()

  const { selectedWorkspace, setSelectedWorkspace } = useHomeStore()
  const [displayWorkspace, setDisplayWorkspace] = useState<AuthorizedWorkspaceRef | null>(
    selectedWorkspace,
  )

  const {
    showSkillPopover,
    setShowSkillPopover,
    slashMatch,
    slashOpen,
    handleSkillPick,
    handleSlashSelect,
    handleSlashClose,
  } = useSkillComposer({
    input: value,
    setInput: setValue,
    textareaRef,
  })

  // Load default folder if no workspace has been selected yet
  useEffect(() => {
    if (selectedWorkspace) {
      setDisplayWorkspace(selectedWorkspace)
      return
    }
    getDefaultFolder()
      .then((ws) => setDisplayWorkspace(ws))
      .catch(() => {
        // fallback: show nothing, user can pick manually
      })
  }, [selectedWorkspace])

  const handlePickProject = async () => {
    const path = await pickLocalDirectory({
      defaultPath: displayWorkspace?.rootPath,
      title: '选择工作目录',
    })
    if (!path) return
    const name = path.split('/').pop() || path.split('\\').pop() || path
    const ws: AuthorizedWorkspaceRef = { id: name, rootPath: path, displayName: name }
    setSelectedWorkspace(ws)
    setDisplayWorkspace(ws)
  }

  const handleSubmit = async (text: string) => {
    if (!text.trim()) return
    setValue('')

    // Create conversation first so we have an ID to authorize against
    const backendId = await createConversation()
    const now = new Date().toISOString()
    const store = useChatStore.getState()
    store.setConversations([
      { id: backendId, title: 'New Conversation', createdAt: now, updatedAt: now, isArchived: false },
      ...store.conversations,
    ])
    store.setActiveConversation(backendId)
    store.setMessages([])
    useUiStore.getState().setRoute({ kind: 'chat', conversationId: backendId })

    // Authorize the selected (or default) workspace
    const workspacePath = displayWorkspace?.rootPath
    if (workspacePath) {
      try {
        await authorizeLocalDirectory(workspacePath, backendId)
      } catch (err) {
        console.error('[HomeTaskComposerCard] Failed to authorize workspace:', err)
        // Non-fatal: proceed without workspace authorization
      }
    }

    // sendUserMessage will use the already-active conversation
    await sendUserMessage(text)
  }

  return (
    <div className="relative">
      <div className="absolute bottom-full left-10 z-30 mb-3">
        <SkillPopover
          open={showSkillPopover}
          onPick={handleSkillPick}
          onClose={() => setShowSkillPopover(false)}
        />
      </div>

      {slashOpen && slashMatch ? (
        <SlashCommandPopover
          filterText={slashMatch.filter}
          onSelect={handleSlashSelect}
          onClose={handleSlashClose}
        />
      ) : null}

      <ChatComposerCompact
        value={value}
        onChange={setValue}
        onSubmit={(v) => void handleSubmit(v)}
        placeholder="描述你的任务，或输入 / 选择技能来开始..."
        onOpenSkill={() => setShowSkillPopover((prev) => !prev)}
        onPickProject={() => void handlePickProject()}
        projectLabel={displayWorkspace?.displayName ?? '默认项目'}
        textareaRef={textareaRef}
      />
    </div>
  )
}
```

- [ ] **Step 2: 类型检查**

```bash
pnpm build 2>&1 | tail -30
```

期望：无 TypeScript 错误。

- [ ] **Step 3: Commit**

```bash
git add src/components/home/HomeTaskComposerCard.tsx
git commit -m "feat(home): wire workspace selection to homepage composer"
```

---

## Task 5: 单测 `HomeTaskComposerCard`

**Files:**
- Create: `src/components/home/__tests__/HomeTaskComposerCard.test.tsx`

- [ ] **Step 1: 检查现有 mock 模式**

```bash
cat src/components/home/__tests__/HomeMascotHero.test.tsx | head -20
```

了解项目 vi.mock 的写法规范。

- [ ] **Step 2: 新建测试文件**

```tsx
import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { HomeTaskComposerCard } from '../HomeTaskComposerCard'

// Mock tauri calls
vi.mock('@/lib/tauri', () => ({
  getDefaultFolder: vi.fn().mockResolvedValue({
    id: 'default',
    rootPath: '/Users/test/.renlijia/defaultFolder',
    displayName: '默认项目',
  }),
  pickLocalDirectory: vi.fn(),
  authorizeLocalDirectory: vi.fn().mockResolvedValue({ id: 'ws1', rootPath: '/tmp/proj', displayName: 'proj' }),
  createConversation: vi.fn().mockResolvedValue('conv-123'),
}))

vi.mock('@/hooks/useChat', () => ({
  useChat: () => ({ sendUserMessage: vi.fn().mockResolvedValue(undefined) }),
}))

vi.mock('@/hooks/useSkillComposer', () => ({
  useSkillComposer: () => ({
    showSkillPopover: false,
    setShowSkillPopover: vi.fn(),
    slashMatch: null,
    slashOpen: false,
    handleSkillPick: vi.fn(),
    handleSlashSelect: vi.fn(),
    handleSlashClose: vi.fn(),
  }),
}))

vi.mock('@/stores/chatStore', () => ({
  useChatStore: {
    getState: () => ({
      conversations: [],
      setConversations: vi.fn(),
      setActiveConversation: vi.fn(),
      setMessages: vi.fn(),
    }),
  },
}))

vi.mock('@/stores/uiStore', () => ({
  useUiStore: {
    getState: () => ({ setRoute: vi.fn() }),
  },
}))

vi.mock('@/stores/homeStore', () => ({
  useHomeStore: () => ({
    selectedWorkspace: null,
    setSelectedWorkspace: vi.fn(),
  }),
}))

vi.mock('@/components/chat/SkillPopover', () => ({ SkillPopover: () => null }))
vi.mock('@/components/chat/SlashCommandPopover', () => ({ SlashCommandPopover: () => null }))

describe('HomeTaskComposerCard', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('shows 默认项目 after loading default folder', async () => {
    render(<HomeTaskComposerCard />)
    await waitFor(() => {
      expect(screen.getByText('默认项目')).toBeTruthy()
    })
  })

  it('updates project label after user picks a directory', async () => {
    const { pickLocalDirectory } = await import('@/lib/tauri')
    vi.mocked(pickLocalDirectory).mockResolvedValueOnce('/Users/test/myproject')

    render(<HomeTaskComposerCard />)
    await waitFor(() => screen.getByText('默认项目'))

    await userEvent.click(screen.getByText('默认项目'))
    await waitFor(() => {
      expect(screen.getByText('myproject')).toBeTruthy()
    })
  })
})
```

- [ ] **Step 3: 运行测试**

```bash
pnpm exec vitest run src/components/home/__tests__/HomeTaskComposerCard.test.tsx
```

期望：2 tests pass。

- [ ] **Step 4: Commit**

```bash
git add src/components/home/__tests__/HomeTaskComposerCard.test.tsx
git commit -m "test(home): HomeTaskComposerCard workspace selection unit tests"
```

---

## Task 6: 手动验证

- [ ] **Step 1: 启动开发服务器**

```bash
pnpm tauri:dev
```

- [ ] **Step 2: 黄金路径验证**

1. 进入首页，输入框底部项目按钮显示「默认项目」
2. 点击「默认项目」，弹出系统文件夹选择器
3. 选择一个目录（如桌面），按钮文字更新为目录名
4. 输入一段文字，点击发送
5. 跳转到聊天页，确认正常进入对话
6. 回到首页，项目按钮仍显示刚才选的目录名（持久化验证）

- [ ] **Step 3: 取消选择验证**

点击项目按钮后在文件夹选择器中取消，确认 label 不变。

- [ ] **Step 4: 全量测试**

```bash
pnpm test
```

期望：所有测试通过。
