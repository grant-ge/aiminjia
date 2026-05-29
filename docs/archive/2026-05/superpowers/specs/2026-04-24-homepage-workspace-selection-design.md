# 首页工作目录选择设计

**日期：** 2026-04-24
**状态：** 待实施

## 背景

首页输入框（`HomeTaskComposerCard`）目前项目按钮没有实际功能——`useAuthorizedWorkspace(null)` 因为没有 sessionId 永远返回 `null`，`projectLabel` 始终显示硬编码的 `'Desktop'`。发送时也没有把工作目录绑定到新建的 conversation。

## 目标

1. 首页输入框默认显示「默认项目」（`~/.renlijia/defaultFolder`）
2. 点击项目按钮弹出系统文件夹选择器，选择后显示名更新
3. 发送时新建 conversation 并绑定选择的工作目录
4. 用户选择持久化，下次进首页仍是上次选的目录

## 设计方案

### 后端：新增 `get_default_folder` command

在 `src-tauri/src/transport/tauri_commands/` 适当位置新增：

```rust
#[tauri::command]
async fn get_default_folder(/* state */) -> Result<AuthorizedWorkspaceRef, String>
```

返回：
```json
{ "id": "default", "rootPath": "/Users/xx/.renlijia/defaultFolder", "displayName": "默认项目" }
```

`~/.renlijia/defaultFolder` 已由 `AiJiaHome::default_folder()` + `ensure_dirs()` 保证存在，无需额外创建。

对应在 `src/lib/tauri.ts` 新增：

```ts
export function getDefaultFolder(): Promise<AuthorizedWorkspaceRef>
```

### 前端：`homeStore`

新建 `src/stores/homeStore.ts`：

```ts
interface HomeState {
  selectedWorkspace: AuthorizedWorkspaceRef | null
  setSelectedWorkspace: (ws: AuthorizedWorkspaceRef | null) => void
}
```

- 使用 Zustand `persist` 中间件，key `aijia-home`，持久化到 `localStorage`
- 初始值 `null` 语义：「还未选择，使用默认项目」
- 不在 store 里存默认值，保留 null 以便未来可以区分「用户主动选过」和「从未选过」

### 前端：`HomeTaskComposerCard` 改造

**初始化：**
- 读 `homeStore.selectedWorkspace`
- 若为 `null`，在 `useEffect` 里调 `getDefaultFolder()` 并存入本地 `displayWorkspace` state（不写回 store）
- 若有值，直接用 store 里的值初始化 `displayWorkspace`

**点击项目按钮：**
```
pickLocalDirectory() → 用户选择路径
→ 构造 AuthorizedWorkspaceRef { rootPath, displayName: basename(path) }
→ homeStore.setSelectedWorkspace(ws)
→ 更新本地 displayWorkspace state
```
此时不调 `authorizeLocalDirectory`（还没有 conversationId）。

**发送流程：**
```
createConversation() → conversationId
→ authorizeLocalDirectory(displayWorkspace.rootPath, conversationId)
→ setActiveConversation(conversationId)（chatStore）
→ sendUserMessage(text)
```

`ChatComposerCompact` 的 `projectLabel` 绑定 `displayWorkspace?.displayName ?? '默认项目'`。

## 数据流

```
homeStore (persist) ──────────────────────────────────┐
                                                       ↓
进首页 → useEffect → getDefaultFolder() → displayWorkspace state → projectLabel 显示
                                                       ↑
点击项目按钮 → pickLocalDirectory() → 构造 ref → homeStore + displayWorkspace
                                                       
发送 → createConversation → authorizeLocalDirectory(displayWorkspace.rootPath, id)
     → sendUserMessage(text)
```

## 改动范围

| 文件 | 改动 |
|---|---|
| `src-tauri/src/transport/tauri_commands/` (适当文件) | 新增 `get_default_folder` command |
| `src-tauri/src/lib.rs` | 注册新 command |
| `src/lib/tauri.ts` | 新增 `getDefaultFolder()` |
| `src/stores/homeStore.ts` | 新建，Zustand persist store |
| `src/components/home/HomeTaskComposerCard.tsx` | 改造主逻辑 |

## 不在本次范围内

- 「最近使用目录」列表
- 首页以外的 composer 的项目选择
- 默认项目的用户自定义（改名、改路径）
