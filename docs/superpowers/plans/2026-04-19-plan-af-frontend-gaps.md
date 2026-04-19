# 前端缺口修复包（Plan-AF）

> **For agentic workers:** REQUIRED SUB-SKILL: `superpowers:test-driven-development` — write failing tests first, then implement. Each task ends with `superpowers:verification-before-completion`.

**Goal:** 补 Sidebar 导出入口、清理孤儿 invoke，并把对话搜索与 `Plan-Z/Z3` 合并后一次性落地；`StreamingError toast` 保持核验型 no-op。  
**Tech Stack:** React, TypeScript, Zustand  
**Worktree branch:** pzc  
**文件关联：**
- `src/components/layout/Sidebar.tsx` — AF1、AF2 实现主体
- `src/lib/tauri.ts` — AF3 孤儿 invoke 清理
- `src/hooks/useStreaming.ts` — AF4 已有完整 toast 实现（见现状说明）
- `src/i18n/en-US.json` / `zh-CN.json` — 新增 i18n key

---

## 现状核查（执行前必读）

| Task | 现状 | 需要做什么 |
|------|------|-----------|
| AF1 | Sidebar 无搜索框；`groupConversations` 直接消费完整 `conversations` 列表 | 新增 search state + 过滤逻辑 + 高亮渲染 |
| AF2 | `exportConversation()` 已在 `src/lib/tauri.ts:282` 封装；后端 `TauriExportCommandAdapter` 完整实现；Sidebar 只有删除按钮，无导出入口 | Sidebar 对话项新增操作菜单（删除 + 导出） |
| AF3 | `showBrowseView()` 定义在 `tauri.ts:1010`；后端 `src-tauri/` 无对应 `#[tauri::command] show_browse_view`；前端全局 grep 无任何调用方 | 直接删除该函数 |
| AF4 | `useStreaming.ts:185-209` **已经有完整的 toast 实现**（`useNotificationStore.getState().push(...)` with `context: 'toast'`）| 无需修改——AF4 已是完成态，计划中仅需核实并记录 |

> **AF4 重要说明**：`streaming:error` 已在 `useStreaming.ts` 中接通 toast，包含 `errorType` 区分（chunk_timeout/agent_timeout 用 15s，其余 8s）。本计划**不需要**对 AF4 做任何代码改动，仅在 commit 中补一个 "AF4: verified, no-op" 注释。

## 对标修订 / 与 Plan-Z 边界（2026-04-19）

- `AF1` 与 `Plan-Z/Z3` 是同一个 Sidebar 搜索缺口，后续必须合并实现；若 `Z3` 已完成，`AF1` 只保留测试/高亮/空态补差，不再重复造第二套搜索。
- `AF2` 与 `AF1/Z3` 共享 `Sidebar.tsx` 写集，执行时应与 Sidebar 搜索同一次提交处理，避免来回覆盖。
- `AF3` 可独立并行；`AF4` 固定为核验型 no-op。

---

## Task AF1 — Sidebar 对话搜索

### 目标
- Sidebar 顶部（New Chat 按钮下方）新增搜索 input
- 实时过滤对话列表（按 `title` 匹配）
- 匹配词用 `<mark>` 标签高亮
- 纯前端过滤（当前历史数量不超过几百条，无需后端分页）

### 实现步骤

**Step 1：i18n key**

在 `en-US.json` 的 `sidebar` 对象中新增：
```json
"searchPlaceholder": "Search conversations...",
"noSearchResults": "No conversations match"
```
在 `zh-CN.json` 对应新增中文翻译。

**Step 2：`Sidebar.tsx` 状态与过滤逻辑**

在现有 `useState` 块附近新增：
```tsx
const [searchQuery, setSearchQuery] = useState('')
```

在 `const grouped = useMemo(...)` 之前新增过滤计算：
```tsx
const filteredConversations = useMemo(() => {
  const q = searchQuery.trim().toLowerCase()
  if (!q) return conversations
  return conversations.filter((c) =>
    c.title.toLowerCase().includes(q)
  )
}, [conversations, searchQuery])
```

将 `groupConversations(conversations)` 改为 `groupConversations(filteredConversations)`：
```tsx
const grouped = useMemo(
  () => groupConversations(filteredConversations),
  [filteredConversations],
)
```

**Step 3：搜索 input JSX**

位置：`<button ... onClick={createNewConversation}>` 之后（`</div>` 关闭 header 区域之前）：
```tsx
{/* Conversation search */}
<div className="relative mt-2">
  <input
    type="search"
    className="w-full rounded-md border py-1.5 pl-8 pr-3 text-sm outline-none transition-colors"
    style={{
      background: 'var(--color-bg-main)',
      borderColor: 'var(--color-border)',
      color: 'var(--color-text-primary)',
    }}
    placeholder={t('sidebar.searchPlaceholder')}
    value={searchQuery}
    onChange={(e) => setSearchQuery(e.target.value)}
  />
  {/* Search icon */}
  <svg
    className="pointer-events-none absolute top-1/2 left-2 h-3.5 w-3.5 -translate-y-1/2 opacity-40"
    viewBox="0 0 24 24"
    fill="currentColor"
    style={{ color: 'var(--color-text-muted)' }}
  >
    <path d="M15.5 14h-.79l-.28-.27A6.471 6.471 0 0 0 16 9.5 6.5 6.5 0 1 0 9.5 16c1.61 0 3.09-.59 4.23-1.57l.27.28v.79l5 4.99L20.49 19l-4.99-5zm-6 0C7.01 14 5 11.99 5 9.5S7.01 5 9.5 5 14 7.01 14 9.5 11.99 14 9.5 14z" />
  </svg>
</div>
```

**Step 4：高亮渲染**

新增工具函数（可放在 Sidebar.tsx 文件顶部，组件外）：
```tsx
/** Wrap matching substrings in <mark> for highlighting. */
function highlightMatch(text: string, query: string): React.ReactNode {
  if (!query.trim()) return text
  const q = query.trim().toLowerCase()
  const idx = text.toLowerCase().indexOf(q)
  if (idx === -1) return text
  return (
    <>
      {text.slice(0, idx)}
      <mark
        style={{
          background: 'var(--color-accent-muted)',
          color: 'var(--color-accent-700)',
          borderRadius: '2px',
          padding: '0 1px',
        }}
      >
        {text.slice(idx, idx + q.length)}
      </mark>
      {text.slice(idx + q.length)}
    </>
  )
}
```

在对话标题渲染处（`<span className="flex-1 truncate text-sm">`）将 `{conv.title}` 替换为：
```tsx
{highlightMatch(conv.title, searchQuery)}
```

**Step 5：空结果提示**

在 `conversations.length === 0` 分支之后，新增 `filteredConversations.length === 0 && searchQuery` 分支：
```tsx
) : filteredConversations.length === 0 && searchQuery.trim() ? (
  <p
    className="px-3 py-8 text-center text-sm"
    style={{ color: 'var(--color-text-muted)' }}
  >
    {t('sidebar.noSearchResults')}
  </p>
) : (
  // existing grouped.map(...)
```

### 测试

**文件：** `src/components/layout/Sidebar.test.tsx`（新建）

测试场景（Vitest + @testing-library/react）：
1. 渲染时搜索框默认为空，显示所有对话
2. 输入 "foo" 后仅显示 title 包含 "foo" 的对话
3. 输入内容无匹配时显示 `sidebar.noSearchResults` 文本
4. 清空搜索框后恢复全量列表
5. 匹配词被 `<mark>` 标签包裹

Mock 依赖：`vi.mock('@/hooks/useChat')` 返回固定 conversations 列表；`vi.mock('@/stores/chatStore')` 返回空 busyConversations。

### Commit
```
feat(sidebar): add real-time conversation search with match highlighting - AF1
```

---

## Task AF2 — export_conversation 前端入口

### 目标
- 将现有 Sidebar 对话项的单按钮（删除）升级为操作菜单（删除 + 导出 HTML/PDF）
- 点击导出触发 `exportConversation(id, format)`
- 结果用 `notificationStore.push(...)` 通知（成功 `success` / 失败 `error`）

### 实现步骤

**Step 1：i18n key**

在 `sidebar` 对象新增（`en-US.json` / `zh-CN.json`）：
```json
"exportConversation": "Export",
"exportAsHtml": "Export as HTML",
"exportAsPdf": "Export as PDF",
"exportSuccess": "Exported: {{fileName}}",
"exportFailed": "Export failed"
```
> 注：`topBar` 已有同名 key，但 Sidebar 语义相同可复用；若要复用，直接引用 `topBar.exportAsHtml` 等；若语义有差，在 `sidebar` 命名空间独立定义。**推荐直接复用 `topBar.*` 已有 key，不新增冗余。**

**Step 2：操作菜单状态**

```tsx
const [menuOpenId, setMenuOpenId] = useState<string | null>(null)
```

使用 `useEffect + ref.contains(e.target)` 注册点击外部关闭菜单，避免“点击菜单项时先关闭再卸载导致事件丢失”的竞态：
```tsx
const menuRef = useRef<HTMLDivElement | null>(null)

useEffect(() => {
  if (!menuOpenId) return
  const close = (event: MouseEvent) => {
    if (!menuRef.current?.contains(event.target as Node)) {
      setMenuOpenId(null)
    }
  }
  document.addEventListener('mousedown', close)
  return () => document.removeEventListener('mousedown', close)
}, [menuOpenId])
```

**Step 3：导出处理函数**

```tsx
const handleExport = useCallback(
  async (convId: string, format: 'html' | 'pdf') => {
    setMenuOpenId(null)
    try {
      const result = await exportConversation(convId, format)
      useNotificationStore.getState().push({
        level: 'success',
        title: t('topBar.exportSuccess'),
        message: result.fileName,
        actions: [],
        dismissible: true,
        autoHide: 5,
        context: 'toast',
      })
    } catch (err) {
      useNotificationStore.getState().push({
        level: 'error',
        title: t('topBar.exportFailed'),
        message: String(err),
        actions: [],
        dismissible: true,
        autoHide: 8,
        context: 'toast',
      })
    }
  },
  [t],
)
```

需要在文件顶部新增 import：
```tsx
import { updateSettings, getSettings, exportConversation } from '@/lib/tauri'
import { useNotificationStore } from '@/stores/notificationStore'
import { useCallback } from 'react'
```
（`useCallback` 加入现有 `{ useEffect, useMemo, useState, useRef }` 解构）

**Step 4：操作菜单 JSX 替换现有删除按钮**

将现有单个删除 `<button>` 替换为三点菜单按钮 + 下拉：
```tsx
{/* Actions menu (replaces standalone delete button) */}
<div className="relative mr-1" onClick={(e) => e.stopPropagation()}>
  <button
    className="flex h-6 w-6 shrink-0 cursor-pointer items-center justify-center rounded border-none opacity-0 transition-opacity duration-150 group-hover:opacity-100"
    style={{ background: 'transparent', color: 'var(--color-text-muted)' }}
    title={t('sidebar.deleteConversation')}
    onClick={(e) => {
      e.stopPropagation()
      setMenuOpenId(menuOpenId === conv.id ? null : conv.id)
    }}
  >
    {/* ⋯ icon (three horizontal dots) */}
    <svg className="h-3.5 w-3.5" viewBox="0 0 24 24" fill="currentColor">
      <path d="M6 10c-1.1 0-2 .9-2 2s.9 2 2 2 2-.9 2-2-.9-2-2-2zm6 0c-1.1 0-2 .9-2 2s.9 2 2 2 2-.9 2-2-.9-2-2-2zm6 0c-1.1 0-2 .9-2 2s.9 2 2 2 2-.9 2-2-.9-2-2-2z" />
    </svg>
  </button>

  {menuOpenId === conv.id && (
    <div
      className="absolute right-0 z-50 mt-1 w-40 rounded-md border py-1 shadow-lg"
      style={{
        background: 'var(--color-bg-main)',
        borderColor: 'var(--color-border)',
        top: '100%',
      }}
    >
      <button
        className="flex w-full cursor-pointer items-center gap-2 px-3 py-1.5 text-left text-sm transition-colors"
        style={{ background: 'transparent', color: 'var(--color-text-secondary)' }}
        onMouseEnter={(e) => { e.currentTarget.style.background = 'var(--color-bg-sidebar-hover)' }}
        onMouseLeave={(e) => { e.currentTarget.style.background = 'transparent' }}
        onClick={() => handleExport(conv.id, 'html')}
      >
        {t('topBar.exportAsHtml')}
      </button>
      <button
        className="flex w-full cursor-pointer items-center gap-2 px-3 py-1.5 text-left text-sm transition-colors"
        style={{ background: 'transparent', color: 'var(--color-text-secondary)' }}
        onMouseEnter={(e) => { e.currentTarget.style.background = 'var(--color-bg-sidebar-hover)' }}
        onMouseLeave={(e) => { e.currentTarget.style.background = 'transparent' }}
        onClick={() => handleExport(conv.id, 'pdf')}
      >
        {t('topBar.exportAsPdf')}
      </button>
      <div className="my-1 border-t" style={{ borderColor: 'var(--color-border)' }} />
      <button
        className="flex w-full cursor-pointer items-center gap-2 px-3 py-1.5 text-left text-sm transition-colors"
        style={{ background: 'transparent', color: 'var(--color-semantic-red)' }}
        onMouseEnter={(e) => { e.currentTarget.style.background = 'var(--color-bg-sidebar-hover)' }}
        onMouseLeave={(e) => { e.currentTarget.style.background = 'transparent' }}
        onClick={() => {
          setMenuOpenId(null)
          deleteConversation(conv.id)
        }}
      >
        {t('sidebar.deleteConversation')}
      </button>
    </div>
  )}
</div>
```

### 测试

**文件：** `src/components/layout/Sidebar.test.tsx`（在 AF1 同一文件内追加）

测试场景：
1. 对话项 hover 显示三点菜单按钮
2. 点击菜单按钮打开下拉，包含 "Export as HTML"、"Export as PDF"、"Delete conversation"
3. 点击 "Export as HTML" 调用 `exportConversation(id, 'html')`，成功后 push success toast
4. 点击 "Export as HTML" 时若 `exportConversation` 抛出错误，push error toast
5. 点击菜单外区域关闭下拉
6. 点击 "Delete conversation" 调用 `deleteConversation(id)`

Mock：`vi.mock('@/lib/tauri', () => ({ exportConversation: vi.fn() }))`；`vi.mock('@/stores/notificationStore', () => ({ useNotificationStore: { getState: vi.fn() } }))`。

### Commit
```
feat(sidebar): add export conversation menu with HTML/PDF options and toast feedback - AF2
```

---

## Task AF3 — 清理 show_browse_view 孤儿 invoke

### 现状确认
- `src/lib/tauri.ts:1010-1012`：`showBrowseView()` 定义，调用 `invoke('show_browse_view')`
- `src-tauri/` 全局搜索无 `show_browse_view` 注册
- `src/` 全局搜索无任何 `showBrowseView` 调用方（仅有函数定义本身）

### 实现步骤

**Step 1：删除孤儿函数**

从 `src/lib/tauri.ts` 中删除以下代码块（第 1009-1012 行）：
```ts
/** Show the CDP browser window (bring active tab to front). */
export function showBrowseView(): Promise<void> {
  return invoke<void>('show_browse_view')
}
```

注意：保留其前后的注释节（`// Browser Events` 节头注释保持不动）。

**Step 2：验证**
```bash
cd /Users/a20250311/IdeaProjects/lotus-app
grep -r "showBrowseView\|show_browse_view" src/ src-tauri/
```
期望：无任何结果。

### 测试

无需专门测试。通过 `pnpm build`（TypeScript 编译通过）即视为验证完成。

### Commit
```
chore(tauri): remove orphan showBrowseView invoke with no backend handler - AF3
```

---

## Task AF4 — StreamingError toast（已完成，无需修改）

### 核实结论

`src/hooks/useStreaming.ts:184-209` 中 `streaming:error` 的处理逻辑已完整接通 toast：

```ts
useNotificationStore.getState().push({
  level: 'error',
  title: i18n.t('errors.streamingError'),
  message: (error ?? i18n.t('errors.unknownRetry')) + suffix,
  actions: [],
  dismissible: true,
  autoHide: autoHideSecs,
  context: 'toast',
})
```

- `errorType` 区分已实现（`chunk_timeout` / `agent_timeout` → 15s，其余 → 8s）
- `partialContent` suffix 处理已实现
- `context: 'toast'` 已正确设置

**本 task 无代码改动。** 仅在最终 PR 描述中说明 AF4 在代码审查阶段发现已完成，保留核实记录。

---

## 执行顺序与 Checklist

```
[ ] AF3 — 最小改动先行（删除死代码），验证 build 通过
[ ] AF1 — 搜索过滤 + 高亮（纯前端，无后端依赖）
[ ] AF2 — 操作菜单（依赖 AF1 文件，在同一 Sidebar.tsx 改动中追加）
[ ] AF4 — 核实已完成，添加 no-op commit 注释（可选）
[ ] pnpm lint && pnpm test — 全量通过
[ ] pnpm build — TypeScript 编译通过
```

## 关键约束

1. **不修改后端**：AF1 纯前端过滤，AF2 使用已有 `exportConversation()` 封装，不新增 Tauri command。
2. **不改 useStreaming.ts**：AF4 已完成，禁止画蛇添足。
3. **Sidebar.tsx 改动集中**：AF1 + AF2 在同一文件，建议单次 checkout 完成，减少冲突风险。
4. **i18n 双语同步**：每次新增 i18n key 必须同时更新 `en-US.json` 和 `zh-CN.json`。
5. **操作菜单不阻断 click 冒泡至 `switchConversation`**：菜单容器加 `onClick={(e) => e.stopPropagation()}`。
