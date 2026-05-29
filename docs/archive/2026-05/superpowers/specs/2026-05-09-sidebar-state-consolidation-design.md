# Sidebar 状态合并：让 `route` 成为唯一权威

**日期**：2026-05-09
**作者**：oayzz + Claude
**状态**：设计稿，待实施

## 背景

`AppSidebar` 与相关 store 当前共有 5 个变量在控制"导航/选中态"：

| 变量 | 来源 | 持久化 | 角色 |
|---|---|---|---|
| `route` | `useUiStore` | localStorage | 顶部 nav 选中、当前页、当前会话/IM 会话 id |
| `chatStore.activeConversationId` | `useChatStore` | 否 | 项目树中高亮的会话 |
| `channelStore.activeSessionId` | `useChannelStore` | 否 | 频道列表中高亮的 IM 会话 |
| `sidebarTab` | `AppSidebar` 本地 `useState` | 否 | 项目 / 频道 tab 切换 |
| `legacyExpanded` | `AppSidebar` 本地 `useState` | 否 | 历史会话区域折叠状态 |

`route` 已经是 discriminated union，能完整表达全部导航状态（包括会话 id、sessionId）；其它三个"选中态"字段是 `route` 的影子副本，未与 `route` 严格联动。这种"多份真相"导致两个具体 bug：

### Bug 1：IM 频道入口与子会话同时高亮

`activeKey` 推导规则把 `route.kind === 'channel'` 一律映射到 IM 频道入口高亮，而 `route.sessionId` 又驱动列表中具体钉钉会话的高亮，于是出现"父级入口 + 子会话"双亮，违反常见的 leaf-only 高亮模型。

### Bug 2：选中钉钉会话后刷新回到"项目" tab

刷新时 `route` 从 localStorage 恢复为 `{kind:'channel', sessionId:'X'}`，顶部 IM 频道入口因此恢复高亮；但 `sidebarTab` 是 React 本地 state，重新挂载后默认回到 `'project'`，`channelStore.activeSessionId` 也是内存态，重启后丢失。结果：顶部说"在 IM 频道"，下方却显示项目树，状态错位。

**根因**：`sidebarTab` / `chatStore.activeConversationId` / `channelStore.activeSessionId` 都是 `route` 的影子，但它们没有真正从 `route` 派生，而是与 `route` 并行被各自的 setter 写入。一旦其中任一写路径漏调用，状态就漂移。

## 目标

1. 修掉上述两个 bug。
2. 把"导航 / 选中态"收敛到单一权威：`useUiStore.route`。其它消费方一律派生，不再持有副本。
3. 写入路径只剩 `setRoute` 一条；不再要求调用方"两边都写"。
4. 不引入新 store；`chatStore` / `channelStore` 仍然是会话/消息**数据**的家，只是不再持有"哪个是当前"的**身份**。

## 非目标

- 不重构 `chatStore` / `channelStore` 的数据结构、网络/事件订阅、消息分页逻辑。
- 不动 settings modal、notification、prefillText 等其他 ui state。
- 不调整 `Route` 类型定义本身（已经够用）。
- 不重构 `legacyExpanded` 与重命名/归档弹窗本地 state（它们是纯交互态，与本次 bug 无关）。

## 设计

### §1 状态模型

**保留**
- `useUiStore.route`（`src/stores/uiStore.ts`，已 localStorage 持久化）

**删除**
- `useChatStore.activeConversationId` 字段
- `useChatStore.setActiveConversation` action
- `useChannelStore.activeSessionId` 字段
- `useChannelStore.setActiveSession` action

**新增 selector（在 `uiStore.ts` 暴露）**

```ts
// 给非 React 代码（store action 内部 peek）
export const getActiveConversationId = (): string | null => {
  const r = useUiStore.getState().route
  return r.kind === 'chat' ? r.conversationId : null
}
export const getActiveChannelSessionId = (): string | null => {
  const r = useUiStore.getState().route
  return r.kind === 'channel' ? r.sessionId ?? null : null
}

// 给 React 组件（订阅式，配合 zustand 的 referential equality）
export const useActiveConversationId = (): string | null =>
  useUiStore((s) => (s.route.kind === 'chat' ? s.route.conversationId : null))
export const useActiveChannelSessionId = (): string | null =>
  useUiStore((s) => (s.route.kind === 'channel' ? s.route.sessionId ?? null : null))
```

`chatStore` / `channelStore` 内部所有原本读 `state.activeConversationId` / `state.activeSessionId` 的位置改为调 `getActiveConversationId()` / `getActiveChannelSessionId()`。

### §2 Sidebar UI 派生

`AppSidebar.tsx`：

- 删 `useState<'project'|'channel'>('project')`。
- 派生：`const sidebarTab = route.kind === 'channel' ? 'channel' : 'project'`。
- "项目" tab 按钮 onClick 改为 `setRoute({ kind: 'home' })`。
- "频道" tab 按钮 onClick 改为 `setRoute({ kind: 'channel' })`。
- 项目树中"当前会话"高亮：`useActiveConversationId()` 取代 `useChat()` 解构出的 `activeConversationId`。
- 频道列表中"当前 IM 会话"高亮：`useActiveChannelSessionId()` 取代 `channelActiveSessionId`。

**边界 / 副作用说明**
- 用户身处 `chat/conv-1` 然后点"频道" tab：route 直接切到 `{kind:'channel'}`，右侧从聊天页切到频道总览页。这是用户的"清选中、跳总览"选择的自然后果。
- 频道总览页本身是 `route.kind === 'channel' && !sessionId` 的渲染目标——已存在，本设计不新增页面。

### §3 SidebarNav 高亮：leaf-only

`AppSidebar.tsx` 中 `activeKey` 推导改写：

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

变化点：
1. `route.kind === 'channel'` 且带 `sessionId` 时，`activeKey = null`——顶部 IM 频道入口不再高亮，仅左侧具体会话高亮。
2. `route.kind === 'skill-detail'` 顺便归到 `skill-center` 入口高亮（修复"进入 skill 详情时顶部 nav 啥都不亮"的同类小漂移）。

### §4 写入路径收敛

| 现存位置 | 现在 | 改后 |
|---|---|---|
| `useChat.switchConversation`（`src/hooks/useChat.ts:180`） | 同时调 `chatStore.setActiveConversation(id)` 和 `setRoute({kind:'chat',conversationId:id})` | 仅 `setRoute({kind:'chat',conversationId:id})` |
| `useChat` 创建会话路径 4 处（约 `useChat.ts:118 / 130 / 280 / 442`） | 同上双写 | 同上单写 |
| `AppSidebar.openChannelOverview` / `selectChannelSession` | 同时调 `channelSetActiveSession` 和 `setRoute({kind:'channel', ...})` | 仅 `setRoute({kind:'channel', ...})`，sessionId 由 route 携带 |

写入只剩 `setRoute` 一条；读取走 selector；不可能再漂移。

### §5 测试

**新增（锚定本次修复的两条不变量）**
1. `AppSidebar` 单测：localStorage 预置 `{kind:'channel', sessionId:'X'}` → 渲染 → 断言 sidebarTab 显示频道列表 **且** 钉钉会话 X 高亮 **且** SidebarNav 中 channel 入口不带 active class。
2. `AppSidebar` 单测：route 为 `{kind:'channel', sessionId:'X'}` 时点击顶部 nav "IM 频道" → 断言调用 `setRoute({kind:'channel'})`（不带 sessionId）。

**修改**
- `AppSidebar.test.tsx` / `AppSidebar.__tests__/AppSidebar.test.tsx`：现有 mock 假定 `chatStore.activeConversationId` / `channelStore.activeSessionId` 字段，改为 mock `useUiStore.route`。
- 涉及 `setActiveConversation` / `setActiveSession` 的其它单测：删除调用，改成 mock route。

**保留**
- `uiStore.settingsModal.test.ts` 已经全程 `setRoute` ✓。
- `chatStore.test.ts` 中 `setActiveConversation` 相关 case：随该 action 一并删除。

### §6 迁移步骤（建议单 PR 内顺序）

1. `uiStore.ts` 暴露 4 个 selector（不破坏既有 API）。
2. `chatStore` 内部 reader 切换到 `getActiveConversationId()`；删 `activeConversationId` 字段 + `setActiveConversation` action；改 `chatStore.test.ts`。
3. `useChat` 中所有 `setActiveConversation` 调用删除；保留 `setRoute`。
4. `channelStore` 同 §2 处理；改 `channelStore.test.ts`。
5. `AppSidebar` 删 `sidebarTab` 本地 state、改派生、改 onClick、改 selector、改 `activeKey` 规则。
6. 增 §5 中两个新单测；改既有 sidebar 单测的 mock。
7. 跑：`pnpm lint` / `pnpm test` / `cd src-tauri && cargo test review_ --tests --no-fail-fast`（架构约束类，预期不受影响但跑一下保险）。

## 影响面评估

- 文件改动量：约 6–8 个文件
  - `src/stores/uiStore.ts`（加 selector）
  - `src/stores/chatStore.ts`（删字段/action）
  - `src/stores/channelStore.ts`（删字段/action）
  - `src/hooks/useChat.ts`（删伴随双写）
  - `src/components/sidebar/AppSidebar.tsx`（派生 + 高亮规则）
  - 对应 3–4 个 test 文件
- 行为变化（用户可见）
  - 修复 bug 1：选中钉钉会话时顶部 IM 频道入口不再亮。
  - 修复 bug 2：刷新后位置完全还原。
  - 新增（顺手）：进入 skill-detail 时顶部 skill-center 入口保持高亮。
  - "项目"tab 按钮点击行为：从"只切左侧 tab"变为"切到 home 页"——与"频道"tab 对称（也是切到 channel 总览）。这是从隐式（视觉切换）变为显式（导航）。如认为有回归风险可在实施 PR 中再单独评估。

## 风险与缓解

| 风险 | 缓解 |
|---|---|
| `chatStore.activeConversationId` 有未发现的 reader | 实施前 `grep -rn "activeConversationId"` 全仓清点；TypeScript 编译会报出剩余 reader |
| 同上对 `activeSessionId` | 同样 grep 清点 |
| `useChat` 双写删除可能影响生命周期顺序 | `setActiveConversation` 主要用于本地立即响应，删后所有消费方改读 selector，立即性由 zustand 订阅保证 |
| 测试 mock 改动遗漏 | 实施 PR 中跑全套 vitest，红了再补 |

## 不做的事（明确划界）

- 不引入 `useNavigationStore`（C 方案）。
- 不在 setRoute 中加同步副作用（B 方案）。
- 不改 `Route` 类型形状。
- 不改 settings modal / prefillText / 重命名 / 归档 / legacyExpanded 等无关状态。
