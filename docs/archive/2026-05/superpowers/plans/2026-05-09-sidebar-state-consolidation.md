# Sidebar 状态合并 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 sidebar 导航/选中态收敛到 `useUiStore.route` 唯一权威，修复"IM 频道父子双高亮"和"刷新后 sidebarTab 漂移"两个 bug。

**Architecture:** `route` 已经是 discriminated union 且 localStorage 持久化。让 `chatStore.activeConversationId` / `channelStore.activeSessionId` 从外部看不可写——chatStore 通过订阅 `useUiStore` 自动 sync 内部镜像（streamingStore 的 deriveLegacy 仍依赖它），channelStore 直接删字段，所有 reader 改读 `useUiStore` 派生 selector。`AppSidebar` 删除本地 `sidebarTab` 改派生。

**Tech Stack:** React + Zustand + TypeScript + Vitest，无新依赖。

设计文档：`docs/superpowers/specs/2026-05-09-sidebar-state-consolidation-design.md`

---

## File Structure

| 文件 | 角色 | 改动 |
|---|---|---|
| `src/stores/uiStore.ts` | route 权威；新增 selector | 加 4 个 selector |
| `src/stores/sessionStore.ts` | sessionSlice | `setActiveConversation` 改私有约定（保留实现，外部不再调用） |
| `src/stores/chatStore.ts` | 组合 store；订阅 route | 加 `subscribe(useUiStore)` 桥，自动同步 `activeConversationId` |
| `src/stores/channelStore.ts` | IM 频道数据 | 删 `activeSessionId` 字段 + `setActiveSession` action |
| `src/hooks/useChat.ts` | 业务 hook | 删所有 `setActiveConversation` 调用 |
| `src/features/channel/ChannelPage.tsx` | 频道页 | 删 `setActiveSession` / `setActiveConversation`，改读 route |
| `src/components/home/HomeTaskComposerCard.tsx` | 首页 composer | 删 `setActiveConversation`（route 已经设了） |
| `src/components/sidebar/AppSidebar.tsx` | 侧边栏 | 删 `sidebarTab` 本地 state；高亮规则改 leaf-only；onClick 改 setRoute |
| `src/stores/__tests__/uiStore.derived.test.ts` | 新增 | 锚定 selector 行为 |
| `src/stores/chatStore.test.ts` | 修改 | 删 `setActiveConversation` 直接调用，改 `setRoute` 触发 sync |
| `src/stores/channelStore.test.ts` | 修改 | 删 `activeSessionId`/`setActiveSession` 相关 case |
| `src/stores/sessionStore.test.ts` | 修改 | 同上，改测 sync 路径 |
| `src/stores/streamingStore.test.ts` | 修改 | `setActiveConversation` 改 setRoute |
| `src/components/sidebar/AppSidebar.test.tsx` | 修改 | mock route 而非 chatStore.activeConversationId |
| `src/components/sidebar/__tests__/AppSidebar.test.tsx` | 修改 | 同上；新增 leaf-only / refresh 还原测试 |
| `src/features/channel/ChannelPage.test.tsx` | 修改 | mock route |
| `src/features/channel/ChannelConfig.test.tsx` | 修改 | 删 `activeSessionId` |
| `src/features/channel/ChannelConfigDetails.test.tsx` | 修改 | 同上 |
| `src/features/home/HomePage.test.tsx` | 修改 | 删 `activeConversationId` 直 setState |
| `src/features/chat/ChatPage.test.tsx` | 修改 | 同上 |
| `src/components/settings/__tests__/ArchivedPanel.test.tsx` | 修改 | 同上 |
| `src/components/settings/WorkspaceFirst.integration.test.tsx` | 修改 | 同上 |
| `src/components/chat/MessageList.layout.test.tsx` | 修改 | 同上 |
| `src/components/chat/StreamingBubble.test.tsx` | 修改 | 同上 |

---

## Task 1: 在 uiStore 暴露 4 个派生 selector

**Files:**
- Modify: `src/stores/uiStore.ts`
- Test: `src/stores/__tests__/uiStore.derived.test.ts` (create)

- [ ] **Step 1: 写失败测试**

创建 `src/stores/__tests__/uiStore.derived.test.ts`:

```ts
import { beforeEach, describe, expect, it } from 'vitest'
import {
  useUiStore,
  getActiveConversationId,
  getActiveChannelSessionId,
} from '@/stores/uiStore'

beforeEach(() => {
  useUiStore.setState({ route: { kind: 'home' } })
})

describe('uiStore derived selectors', () => {
  it('getActiveConversationId returns conversationId when route is chat', () => {
    useUiStore.getState().setRoute({ kind: 'chat', conversationId: 'c1' })
    expect(getActiveConversationId()).toBe('c1')
  })

  it('getActiveConversationId returns null for non-chat routes', () => {
    useUiStore.getState().setRoute({ kind: 'channel', sessionId: 's1' })
    expect(getActiveConversationId()).toBeNull()
    useUiStore.getState().setRoute({ kind: 'home' })
    expect(getActiveConversationId()).toBeNull()
  })

  it('getActiveChannelSessionId returns sessionId when route is channel with sessionId', () => {
    useUiStore.getState().setRoute({ kind: 'channel', sessionId: 's1' })
    expect(getActiveChannelSessionId()).toBe('s1')
  })

  it('getActiveChannelSessionId returns null for channel without sessionId', () => {
    useUiStore.getState().setRoute({ kind: 'channel' })
    expect(getActiveChannelSessionId()).toBeNull()
  })

  it('getActiveChannelSessionId returns null for non-channel routes', () => {
    useUiStore.getState().setRoute({ kind: 'chat', conversationId: 'c1' })
    expect(getActiveChannelSessionId()).toBeNull()
  })
})
```

- [ ] **Step 2: 跑测试确认失败**

```bash
pnpm exec vitest run src/stores/__tests__/uiStore.derived.test.ts
```
预期：FAIL（`getActiveConversationId is not a function`）。

- [ ] **Step 3: 加 selector**

编辑 `src/stores/uiStore.ts` 末尾追加：

```ts
export const getActiveConversationId = (): string | null => {
  const r = useUiStore.getState().route
  return r.kind === 'chat' ? r.conversationId : null
}

export const getActiveChannelSessionId = (): string | null => {
  const r = useUiStore.getState().route
  return r.kind === 'channel' ? r.sessionId ?? null : null
}

export const useActiveConversationId = (): string | null =>
  useUiStore((s) => (s.route.kind === 'chat' ? s.route.conversationId : null))

export const useActiveChannelSessionId = (): string | null =>
  useUiStore((s) => (s.route.kind === 'channel' ? s.route.sessionId ?? null : null))
```

- [ ] **Step 4: 跑测试确认通过**

```bash
pnpm exec vitest run src/stores/__tests__/uiStore.derived.test.ts
```
预期：PASS（5 个 case 全过）。

- [ ] **Step 5: Commit**

```bash
git add src/stores/uiStore.ts src/stores/__tests__/uiStore.derived.test.ts
git commit -m "feat(ui-store): expose route-derived selectors for active conv/session"
```

---

## Task 2: chatStore 订阅 useUiStore 自动同步 activeConversationId

**Files:**
- Modify: `src/stores/chatStore.ts`
- Test: `src/stores/chatStore.test.ts`

**背景**：`sessionSlice.activeConversationId` 是 streamingStore.deriveLegacy 的依赖，不能删除。我们让它从外部看不可写——通过订阅 `useUiStore.route` 自动同步。

- [ ] **Step 1: 写失败测试**

在 `src/stores/chatStore.test.ts` 末尾追加：

```ts
import { useUiStore } from '@/stores/uiStore'

describe('chatStore syncs activeConversationId from route', () => {
  beforeEach(() => {
    useUiStore.setState({ route: { kind: 'home' } })
    useChatStore.setState({ activeConversationId: null, messages: [] })
  })

  it('updates activeConversationId when route changes to chat', () => {
    useUiStore.getState().setRoute({ kind: 'chat', conversationId: 'r-c1' })
    expect(useChatStore.getState().activeConversationId).toBe('r-c1')
  })

  it('updates activeConversationId when route changes to channel with sessionId', () => {
    useUiStore.getState().setRoute({ kind: 'channel', sessionId: 'r-s1' })
    expect(useChatStore.getState().activeConversationId).toBe('r-s1')
  })

  it('clears activeConversationId when route is home', () => {
    useUiStore.getState().setRoute({ kind: 'chat', conversationId: 'r-c1' })
    useUiStore.getState().setRoute({ kind: 'home' })
    expect(useChatStore.getState().activeConversationId).toBeNull()
  })

  it('clears activeConversationId when route is channel without sessionId', () => {
    useUiStore.getState().setRoute({ kind: 'chat', conversationId: 'r-c1' })
    useUiStore.getState().setRoute({ kind: 'channel' })
    expect(useChatStore.getState().activeConversationId).toBeNull()
  })
})
```

- [ ] **Step 2: 跑测试确认失败**

```bash
pnpm exec vitest run src/stores/chatStore.test.ts -t "syncs activeConversationId from route"
```
预期：FAIL（route 改了但 chatStore 不响应）。

- [ ] **Step 3: 在 chatStore.ts 末尾加订阅桥**

在 `src/stores/chatStore.ts` 文件末尾（`bindStreamingStore(useChatStore)` 之后；如不存在则放在 `useChatStore` 创建之后）追加：

```ts
import { useUiStore } from './uiStore'

function deriveActiveIdFromRoute(route: ReturnType<typeof useUiStore.getState>['route']): string | null {
  if (route.kind === 'chat') return route.conversationId
  if (route.kind === 'channel') return route.sessionId ?? null
  return null
}

useChatStore.setState({ activeConversationId: deriveActiveIdFromRoute(useUiStore.getState().route) })

useUiStore.subscribe((state, prev) => {
  if (state.route === prev.route) return
  const nextId = deriveActiveIdFromRoute(state.route)
  const prevId = deriveActiveIdFromRoute(prev.route)
  if (nextId === prevId) return
  useChatStore.getState().setActiveConversation(nextId)
})
```

如 `chatStore.ts` 末尾尚无 `bindSessionStore(useChatStore)` / `bindStreamingStore(useChatStore)`，先 grep 找到这两个调用的位置，把上面订阅代码追加在它们之后。

注意：不要 import `useUiStore` 在文件顶端形成循环——`uiStore.ts` 不依赖 chatStore，所以从 chatStore 引入 uiStore 安全。

- [ ] **Step 4: 跑测试确认通过**

```bash
pnpm exec vitest run src/stores/chatStore.test.ts -t "syncs activeConversationId from route"
```
预期：PASS（4 个 case 全过）。

- [ ] **Step 5: 跑全套 chatStore 测试确认未回归**

```bash
pnpm exec vitest run src/stores/chatStore.test.ts
```
预期：所有现存 case 仍通过。

- [ ] **Step 6: Commit**

```bash
git add src/stores/chatStore.ts src/stores/chatStore.test.ts
git commit -m "feat(chat-store): sync activeConversationId from route subscription"
```

---

## Task 3: useChat.ts 删除所有 setActiveConversation 调用

**Files:**
- Modify: `src/hooks/useChat.ts`
- Test: 既有测试

`useChat.ts` 当前在 5 处同时调 `store.setActiveConversation(id)` 和 `setRoute({kind:'chat',conversationId:id})`。Task 2 后 setRoute 会自动触发 sync，所以前者全删。

- [ ] **Step 1: 找出所有调用点**

```bash
grep -n "setActiveConversation\b" src/hooks/useChat.ts
```
预期：4-5 个匹配（约在 line 118 / 130 / 185 / 280 / 442 附近）。

- [ ] **Step 2: 编辑文件，逐处删调用**

对每个匹配处：保留紧随其后的 `setRoute({kind:'chat',conversationId:...})` 调用，删除 `store.setActiveConversation(...)` / `useChatStore.getState().setActiveConversation(...)` 那一行。

如果某处是 `store.setActiveConversation(id)` 后没有 `setRoute`：检查是否漏 setRoute。例如 `switchConversation` (`useChat.ts:185-187`) 当前是：
```ts
const store = useChatStore.getState()
store.setActiveConversation(id)
store.setMessages([])
useUiStore.getState().setRoute({ kind: 'chat', conversationId: id })
```
改为：
```ts
const store = useChatStore.getState()
store.setMessages([])
useUiStore.getState().setRoute({ kind: 'chat', conversationId: id })
```

- [ ] **Step 3: 验证文件中已无该调用**

```bash
grep -n "setActiveConversation\b" src/hooks/useChat.ts
```
预期：0 行输出。

- [ ] **Step 4: 跑相关单测**

```bash
pnpm exec vitest run src/lib/tauri.events.test.ts src/hooks/useStreaming.integration.test.tsx src/stores/chatStore.test.ts
```
预期：全部 PASS。

- [ ] **Step 5: Commit**

```bash
git add src/hooks/useChat.ts
git commit -m "refactor(use-chat): drop setActiveConversation calls (synced via route)"
```

---

## Task 4: HomeTaskComposerCard / ChannelPage 删除 setActiveConversation

**Files:**
- Modify: `src/components/home/HomeTaskComposerCard.tsx`
- Modify: `src/features/channel/ChannelPage.tsx`

- [ ] **Step 1: 改 HomeTaskComposerCard**

打开 `src/components/home/HomeTaskComposerCard.tsx:138`，删除该行：

```ts
store.setActiveConversation(backendId)
```

紧随其后的 `useUiStore.getState().setRoute({ kind: 'chat', conversationId: backendId })`（line 140）保留——它会触发 Task 2 的 sync。

- [ ] **Step 2: 改 ChannelPage（删 chatStore.setActiveConversation 调用）**

打开 `src/features/channel/ChannelPage.tsx:272-285`。当前 effect：

```ts
useEffect(() => {
  const store = useChatStore.getState()

  if (!activeSessionId) {
    if (store.activeConversationId !== null) {
      store.setActiveConversation(null)
      store.setMessages([])
    }
    return
  }

  let cancelled = false
  store.setActiveConversation(activeSessionId)
  store.setMessages([])
  // ...
```

改为：删除 `store.setActiveConversation(...)` 两处调用，保留 `store.setMessages([])`。Task 2 的订阅会通过 route 自动 sync chatStore.activeConversationId。

```ts
useEffect(() => {
  const store = useChatStore.getState()

  if (!activeSessionId) {
    if (store.activeConversationId !== null) {
      store.setMessages([])
    }
    return
  }

  let cancelled = false
  store.setMessages([])
  // ...
```

- [ ] **Step 3: 跑测试**

```bash
pnpm exec vitest run src/features/channel/ChannelPage.test.tsx src/features/home/HomePage.test.tsx
```
预期：PASS（如失败先看是否依赖 setActiveConversation mock，有则进 Task 7 一起改）。

- [ ] **Step 4: Commit**

```bash
git add src/components/home/HomeTaskComposerCard.tsx src/features/channel/ChannelPage.tsx
git commit -m "refactor(channel,home): drop chatStore.setActiveConversation calls"
```

---

## Task 5: channelStore 删除 activeSessionId / setActiveSession

**Files:**
- Modify: `src/stores/channelStore.ts`
- Test: `src/stores/channelStore.test.ts`

- [ ] **Step 1: 改 channelStore.test.ts 新增取代测试**

把 `src/stores/channelStore.test.ts` 中 `activeSessionId / setActiveSession` 相关 case 改成"通过 setRoute 间接控制"或直接删。具体：

找到 `expect(useChannelStore.getState().activeSessionId).toBeNull()` 和 `useChannelStore.setState({ ..., activeSessionId: 'dingtalk_session_1' })` 两处。删除 activeSessionId 字段相关 setup/assert（保留 conversations / platforms 部分）。

新增一个 case 锚定 unread 行为不再依赖 activeSessionId（incrementUnread / clearUnread 仍按 sessionId 直接操作 conversations）：

```ts
it('incrementUnread bumps unreadCount regardless of route', () => {
  useChannelStore.setState({
    conversations: [
      { sessionId: 's1', unreadCount: 0, /* ... 必填字段，参考既有 fixture */ } as any,
    ],
  })
  useChannelStore.getState().incrementUnread('s1')
  expect(useChannelStore.getState().conversations[0].unreadCount).toBe(1)
})
```

- [ ] **Step 2: 跑测试确认失败**

```bash
pnpm exec vitest run src/stores/channelStore.test.ts
```
预期：FAIL（可能是字段引用 / 旧 case 残留）。

- [ ] **Step 3: 改 channelStore.ts**

编辑 `src/stores/channelStore.ts`：

1. 删 interface 字段（line 23）：`activeSessionId: string | null`
2. 删 interface action（line 27）：`setActiveSession: (sessionId: string | null) => void`
3. 删初始值（line 46）：`activeSessionId: null,`
4. 删 action 实现（line 56-59）：整个 `setActiveSession: (sessionId) => { ... }` 块
5. 改 `removePlatform` 中对 `activeSessionId` 的处理（line 119-126）。原代码：

```ts
removePlatform: async (platform) => {
  const platformState = await channelRemovePlatform(platform)
  get().setPlatformState(platformState)
  set((s) => {
    const removedActiveSession = s.conversations.some(
      (c) => c.platform === platform && c.sessionId === s.activeSessionId,
    )
    return {
      conversations: s.conversations.filter((c) => c.platform !== platform),
      activeSessionId: removedActiveSession ? null : s.activeSessionId,
    }
  })
  return platformState
},
```

改为（删平台时如果当前选中的会话恰好属于这个平台，把 route 重置到 channel overview）：

```ts
removePlatform: async (platform) => {
  const platformState = await channelRemovePlatform(platform)
  get().setPlatformState(platformState)

  const route = useUiStore.getState().route
  const activeId = route.kind === 'channel' ? route.sessionId ?? null : null
  const willRemoveActive = activeId
    ? get().conversations.some((c) => c.platform === platform && c.sessionId === activeId)
    : false

  set((s) => ({
    conversations: s.conversations.filter((c) => c.platform !== platform),
  }))

  if (willRemoveActive) {
    useUiStore.getState().setRoute({ kind: 'channel' })
  }
  return platformState
},
```

需要 `import { useUiStore } from './uiStore'`（在文件顶部 import 区添加）。

6. 改 `onChannelMessage` 中对 `activeSessionId` 的引用（line 159-170）：

```ts
await onChannelMessage(({ sessionId }) => {
  const { conversations } = useChannelStore.getState()
  const isKnownSession = conversations.some((c) => c.sessionId === sessionId)
  if (!isKnownSession) {
    void useChannelStore.getState().loadConversations()
    return
  }
  const route = useUiStore.getState().route
  const activeId = route.kind === 'channel' ? route.sessionId ?? null : null
  if (sessionId !== activeId) {
    useChannelStore.getState().incrementUnread(sessionId)
  }
})
```

- [ ] **Step 4: 跑测试**

```bash
pnpm exec vitest run src/stores/channelStore.test.ts
```
预期：PASS。

- [ ] **Step 5: Commit**

```bash
git add src/stores/channelStore.ts src/stores/channelStore.test.ts
git commit -m "refactor(channel-store): drop activeSessionId; derive from route"
```

---

## Task 6: ChannelPage 改读 route 而非 channelStore.activeSessionId

**Files:**
- Modify: `src/features/channel/ChannelPage.tsx`
- Test: `src/features/channel/ChannelPage.test.tsx`

ChannelPage `line 258-270` 现在通过 `setActiveSession(sessionId ?? null)` 把 route 携带的 sessionId 投影回 channelStore。Task 5 删了 setActiveSession，ChannelPage 不再需要这个投影——直接用 route prop（或 useActiveChannelSessionId）。

- [ ] **Step 1: 改 ChannelPage.tsx**

编辑 `src/features/channel/ChannelPage.tsx`:

1. 删 `const setActiveSession = useChannelStore((s) => s.setActiveSession)`（line 258）。
2. 删 `const activeSessionId = useChannelStore((s) => s.activeSessionId)`（line 259）。
3. 改用 prop（`sessionId` 是该页的 prop，已在用）：把 effect 里的 `activeSessionId` 都改成 `sessionId ?? null`。
4. 删 line 268-270 的 `useEffect(() => { setActiveSession(sessionId ?? null) }, ...)`——已经无意义。
5. 改 line 386-388：`activeSessionId` → `sessionId`。

- [ ] **Step 2: 改 ChannelPage.test.tsx**

`src/features/channel/ChannelPage.test.tsx:87, 187, 214` 处现有 mock 依赖 `activeSessionId`。把 fixture 改成：测试通过路由（`useUiStore` 的 mock）传 sessionId，而非 channelStore.activeSessionId。

具体改法：找到每个 `useChannelStore.setState({ activeSessionId: '...' })` 调用，删除该字段。如该 case 需要 ChannelPage 看到具体 sessionId，改为传 prop 或 mock useUiStore：

```ts
useUiStore.setState({ route: { kind: 'channel', sessionId: 'sess-cur' } })
```

(具体 ChannelPage 接 sessionId 的方式依赖于路由层，看 ChannelPage 是直接接 prop 还是从 useUiStore 读——按现状用 prop 就传 prop。)

- [ ] **Step 3: 跑测试**

```bash
pnpm exec vitest run src/features/channel/
```
预期：PASS。

- [ ] **Step 4: Commit**

```bash
git add src/features/channel/ChannelPage.tsx src/features/channel/ChannelPage.test.tsx
git commit -m "refactor(channel-page): read sessionId from prop instead of channelStore"
```

---

## Task 7: AppSidebar 派生 sidebarTab + leaf-only 高亮 + onClick→setRoute

**Files:**
- Modify: `src/components/sidebar/AppSidebar.tsx`
- Test: `src/components/sidebar/__tests__/AppSidebar.test.tsx` (改+新增) `src/components/sidebar/AppSidebar.test.tsx` (改)

- [ ] **Step 1: 写新测试（refresh-restore + leaf-only）**

在 `src/components/sidebar/__tests__/AppSidebar.test.tsx` 末尾追加：

```ts
describe('AppSidebar route-derived sidebarTab', () => {
  it('shows channel list when route.kind === channel even after fresh mount', () => {
    useUiStore.setState({ route: { kind: 'channel', sessionId: 'sess-X' } })
    useChannelStore.setState({
      conversations: [
        {
          sessionId: 'sess-X',
          platform: 'dingtalk',
          robotCode: 'r1',
          displayName: 'Test Session',
          unreadCount: 0,
          isActiveRobot: true,
          /* ... 其它必填字段按既有 fixture */
        } as any,
      ],
    })
    render(<AppSidebar />)
    // 钉钉会话 X 应被高亮（带 bg-sidebar-accent class）
    const sessionBtn = screen.getByText('Test Session').closest('button')!
    expect(sessionBtn.className).toContain('bg-sidebar-accent')
    // 顶部 IM 频道入口不高亮（leaf-only）
    const channelNavBtn = screen.getByRole('button', { name: /IM 频道/ })
    expect(channelNavBtn.className).not.toContain('bg-sidebar-accent')
  })

  it('clicking IM 频道 nav while a session is selected resets route to channel overview', () => {
    const setRouteSpy = vi.fn()
    useUiStore.setState({ route: { kind: 'channel', sessionId: 'sess-X' }, setRoute: setRouteSpy })
    render(<AppSidebar />)
    fireEvent.click(screen.getByRole('button', { name: /IM 频道/ }))
    expect(setRouteSpy).toHaveBeenCalledWith({ kind: 'channel' })
  })

  it('highlights skill-center nav when route is skill-detail', () => {
    useUiStore.setState({ route: { kind: 'skill-detail', skillId: 'sk-1' } })
    render(<AppSidebar />)
    const navBtn = screen.getByRole('button', { name: /技能中心/ })
    expect(navBtn.className).toContain('bg-sidebar-accent')
  })
})
```

注意：原文件可能没引入 `useUiStore` / `useChannelStore` 真实 hook。看顶部 mock 风格——如果 mock 用 `vi.mock`，就在 mock 里相应方法支持上述 setState 调用，或换成既有 mock 的等价 API。如果原文件用 `setUiState({ route: ... })` 之类辅助函数，照风格写。

- [ ] **Step 2: 跑测试确认失败**

```bash
pnpm exec vitest run src/components/sidebar/__tests__/AppSidebar.test.tsx
```
预期：FAIL（sidebarTab 还是 useState；leaf-only 高亮规则未实现）。

- [ ] **Step 3: 改 AppSidebar.tsx**

编辑 `src/components/sidebar/AppSidebar.tsx`：

1. 删除 `useState` import（如不再用到）和：
```ts
const [sidebarTab, setSidebarTab] = useState<'project' | 'channel'>('project')
```

2. 删除：
```ts
const channelActiveSessionId = useChannelStore((s) => s.activeSessionId)
const channelSetActiveSession = useChannelStore((s) => s.setActiveSession)
```

3. 添加（在 route 取出之后）：
```ts
import { useActiveConversationId, useActiveChannelSessionId } from '@/stores/uiStore'
// 顶部 import 区改 useUiStore 引入
const activeConversationId = useActiveConversationId()
const channelActiveSessionId = useActiveChannelSessionId()
const sidebarTab: 'project' | 'channel' = route.kind === 'channel' ? 'channel' : 'project'
```

并把原本 `useChat()` 解构出来的 `activeConversationId` 改名（避免冲突），如：
```ts
const { conversations, switchConversation, renameConversation, archiveConversation } = useChat()
// 移除原来从 useChat 取的 activeConversationId
```

4. 改 `activeKey` 推导（line 85-98）为：

```ts
const activeKey: SidebarNavKey | null =
  route.kind === 'channel'
    ? route.sessionId ? null : 'channel'
    : route.kind === 'employees' ? 'employees'
    : route.kind === 'skill-center' || route.kind === 'skill-detail' ? 'skill-center'
    : route.kind === 'schedules' ? 'schedules'
    : route.kind === 'inbox' ? 'inbox'
    : route.kind === 'home' ? 'home'
    : null
```

5. 改 tab 切换按钮（line 141-164）。原代码 onClick 是 `setSidebarTab('project')` / `setSidebarTab('channel')`，改为：

```ts
onClick={() => setRoute({ kind: 'home' })}      // 项目按钮
// ...
onClick={() => setRoute({ kind: 'channel' })}   // 频道按钮
```

6. 改 `openChannelOverview` 和 `selectChannelSession`（line 112-120）：

```ts
const openChannelOverview = () => {
  setRoute({ kind: 'channel' })
}

const selectChannelSession = (sessionId: string) => {
  setRoute({ kind: 'channel', sessionId })
}
```

（删去 `channelSetActiveSession(...)` 行；这两个函数现在每个只剩一行 setRoute。）

- [ ] **Step 4: 跑新测试确认通过**

```bash
pnpm exec vitest run src/components/sidebar/__tests__/AppSidebar.test.tsx
```
预期：PASS。

- [ ] **Step 5: 改既有 sidebar 测试 mock**

`src/components/sidebar/AppSidebar.test.tsx` 和 `src/components/sidebar/__tests__/AppSidebar.test.tsx` 中：

- 删除任何 `setActiveSession: vi.fn()` / `activeSessionId: null` 在 channelStore mock 里的字段。
- 删除任何 `chatState.activeConversationId = ...` 直写——改为 `useUiStore.setState({ route: { kind: 'chat', conversationId: ... } })`。

跑：
```bash
pnpm exec vitest run src/components/sidebar/
```
预期：全 PASS。

- [ ] **Step 6: Commit**

```bash
git add src/components/sidebar/
git commit -m "feat(sidebar): derive tab/highlight from route (fix bug 1+2)"
```

---

## Task 8: 修复其它依赖 activeConversationId / activeSessionId 的测试

**Files:**
- Modify: `src/features/home/HomePage.test.tsx`
- Modify: `src/features/chat/ChatPage.test.tsx`
- Modify: `src/components/settings/__tests__/ArchivedPanel.test.tsx`
- Modify: `src/components/settings/WorkspaceFirst.integration.test.tsx`
- Modify: `src/components/chat/MessageList.layout.test.tsx`
- Modify: `src/components/chat/StreamingBubble.test.tsx`
- Modify: `src/stores/streamingStore.test.ts`
- Modify: `src/stores/sessionStore.test.ts`
- Modify: `src/features/channel/ChannelConfig.test.tsx`
- Modify: `src/features/channel/ChannelConfigDetails.test.tsx`

这些测试在 mock 时直接读写 `activeConversationId` / `activeSessionId`。Task 2 改成由 route 同步后，最干净的办法是：在测试 setup 里改用 `useUiStore.setState({ route: { kind: 'chat', conversationId: 'X' } })`。

- [ ] **Step 1: 跑全套，看哪些挂了**

```bash
pnpm test 2>&1 | tail -100
```
记下失败文件清单。

- [ ] **Step 2: 逐文件修复**

对每个失败文件，按以下规则改：

1. `useChatStore.setState({ activeConversationId: 'X' })` → `useUiStore.setState({ route: { kind: 'chat', conversationId: 'X' } })`
2. `useChannelStore.setState({ activeSessionId: 'X' })` → `useUiStore.setState({ route: { kind: 'channel', sessionId: 'X' } })`
3. mock store 工厂函数（如 `sel({ activeConversationId, ... })`）里的 activeConversationId 字段——保留（chatStore 仍有该字段，由 route 同步），但 setup 时改用 setRoute 触发同步。
4. `setActiveConversation` / `setActiveSession` 直接调用：删，改用 setRoute。

`src/stores/sessionStore.test.ts:57` 处 `setActiveConversation('c1')` 是 sessionSlice 的单元测试，验证 slice 内部行为——这是合法的内部测试，保留即可（slice 实现没变）。

- [ ] **Step 3: 跑全套确认绿**

```bash
pnpm test
```
预期：全 PASS。

- [ ] **Step 4: 跑 lint**

```bash
pnpm lint
```
预期：无错误。

- [ ] **Step 5: Commit**

```bash
git add -u
git commit -m "test: migrate active-id mocks to route-driven setup"
```

---

## Task 9: 跑 Rust review 测试 + 端到端冒烟

**Files:** 无改动

- [ ] **Step 1: 跑前端全套**

```bash
pnpm test
pnpm lint
```
预期：全 PASS。

- [ ] **Step 2: 跑 Rust 架构约束回归**

```bash
cd src-tauri && cargo test review_ --tests --no-fail-fast
```
预期：全 PASS（架构约束跟前端 store 重构无关，理应不动；但作为闭环检查跑一遍）。

- [ ] **Step 3: 启动 dev 验证两个 bug 已修**

```bash
pnpm tauri:dev
```

手动验证：
1. **Bug 1**：登录 → 进 IM 频道 → 选中一个钉钉会话 → 顶部 IM 频道入口**不高亮**，左侧具体会话**高亮**。
2. **Bug 2**：在选中的钉钉会话状态下刷新页面（DevTools → Cmd+R 或重启 Tauri 窗口）→ 还原后 sidebar 仍在频道 tab 且同一钉钉会话仍高亮。
3. **Bug 1 反向**：点 IM 频道总览页（任意 nav 进入未选会话）→ 顶部 IM 频道入口**高亮**。
4. **顺手验证**：进入 skill-detail 页面 → 顶部"技能中心"入口仍高亮。
5. **回归**：项目会话切换、新建会话、归档、重命名 4 条核心交互正常。

如手动验证有 regression：回看相关 task 的实现步骤排查；不要直接绕过。

- [ ] **Step 4: 提交 final commit（如有 stage 修复）或直接结束**

```bash
git status
# 如有 unstaged 修改，按 case 提交
```
