# Generated File Card Open Menu Design

日期：2026-04-28

## 背景

聊天流里的 generated file card 已经有接近 split button 的视觉：左侧文件信息，右侧 `Open` 胶囊按钮带分隔线和 chevron。但当前行为没有实现：

- `/src/components/chat/MessageList.tsx` 传入 `onOpen={() => {}}`，点击不会打开文件。
- `/src/components/chat-scene/GeneratedFileCard.tsx` 是纯展示组件，只有 `title/sub/appName/appIcon/onOpen`。
- `/src/hooks/useTurnRenderModel.ts` 的 `RenderGeneratedFile` 只保留 `id/title/sub/appName`，丢失 `conversationId/fileType/actions` 等动作决策信息。

用户希望先做简单方案：md/html 等可以内部预览，其他文件点击后用系统默认软件打开；同时把下拉逻辑补上。

## 对标说明

项目长期要求后端架构设计对标 `/Users/a20250311/github/claude-code-best`。本轮检查时该路径不存在，因此无法进行代码级对标。本设计按 lotus-app 现有 Tauri 命令、file store、message render model 结构收敛，并把后端新增能力限定在当前 repository/facade/file-manager 边界内。

## 目标

第一版交付一个可用、可扩展、低风险的文件动作体验：

1. `Open` 不再是空逻辑，能按 `fileId + conversationId` 打开真实文件。
2. 卡片右侧变成真实 split button：主动作 + chevron 下拉。
3. 下拉菜单显式展示 `Preview inside`、`Open with default app`、`Show in folder`。
4. 对可预览文件显示 `Preview` 主动作；对不可预览文件显示 `Open` 主动作。
5. 内部预览坐在现有任务监控面板扩展出来的 `RightPanel Workspace Preview` 中，不使用临时弹窗作为主路径。
6. 内部预览先支持小文本类文件，复杂 Office/PDF 不在第一版范围内。

## 非目标

- 不做 xlsx/docx/pptx/pdf 的复杂内嵌预览。
- 不新建独立 artifact 中心；第一版只在现有 `RightPanel` 内扩展工作区预览模式。
- 不把前端改成通过裸 `filePath` 读取文件。
- 不使用 `openFileByName` 作为文件卡片主路径，因为它不按 conversation 隔离。
- 不在第一版实现 generated file 删除；现有 `deleteFile` 语义与 generated 支持不完整。

## 交互设计

### 卡片结构

右侧按钮从单按钮改成 split action：

- 左半：主动作按钮。
- 右半：chevron-only 按钮，打开下拉菜单。

主动作按文件类型决定：

| 文件类型 | 主按钮文案 | 主按钮行为 |
| --- | --- | --- |
| `md`, `markdown`, `html`, `txt`, `json`, `csv` | `Preview` | 打开内部预览 |
| `xlsx`, `excel`, `pdf`, `png`, `jpg`, `jpeg`, `py` 或未知类型 | `Open` | 系统默认应用打开 |

下拉菜单始终展示三个动作：

| 菜单项 | 行为 | 禁用策略 |
| --- | --- | --- |
| `Preview inside` | 内部预览 | 文件类型不可预览时禁用，显示 `Preview unavailable` |
| `Open with default app` | 调用系统默认应用打开 | 文件记录缺失时禁用或显示错误 toast |
| `Show in folder` | 在 Finder/Explorer/file manager 里定位 | 文件记录缺失时禁用或显示错误 toast |

### 错误反馈

所有动作失败时复用现有 notification toast 模式：

- 打开失败：`无法打开文件`
- 显示位置失败：`无法在文件夹中显示`
- 预览失败：`无法预览文件`

错误详情放在 toast 描述里，不吞掉异常。

## 前端设计

### 数据投影

扩展 `RenderGeneratedFile`，至少保留：

- `id`
- `title`
- `sub`
- `appName`
- `fileType`
- `actions`

`conversationId` 可以从 message/turn 层透传，也可以在 `MessageList` 中读取 `activeConversationId` 后作为动作参数传入。优先采用能明确绑定当前 turn 的方案；如果当前 render model 已天然按 active conversation 渲染，可先使用 active conversation，并在测试中覆盖。

### 组件边界

`GeneratedFileCard` 继续保持展示/事件组件，不直接 import Tauri API：

- 输入：文件展示信息、动作可用性、主动作类型。
- 输出：`onPreview`、`onOpenExternal`、`onReveal`。
- 内部：只负责 split button、DropdownMenu、禁用态、可访问语义。

`MessageList` 或其附近容器负责：

- 调用 `openGeneratedFile(fileId, conversationId)`。
- 调用 `revealFileInFolder(fileId, conversationId)`。
- 请求右侧面板进入 preview mode。
- 处理失败 toast。

### 右侧工作区预览

内部预览复用现有任务监控面板，而不是新增主路径 dialog：

- `RightPanel` 默认保持当前窄面板：`w-[260px]`，显示任务监控、产物、技能与 MCP。
- 用户点击可预览文件的 `Preview` 后，`RightPanel` 进入 workspace preview mode，宽度扩展到约 `620-760px`。
- preview mode 内部左右分栏：
  - 左侧 `FilePreviewPane`：显示当前文件预览，占主要宽度。
  - 右侧 `RightPanelSidebar`：保留任务监控、产物列表、技能与 MCP，宽约 `220-260px`。
- 产物列表变成文件切换器：点击某个产物后，左侧切换到对应文件预览或 unsupported 状态。
- 当前预览文件在产物列表里高亮。
- preview mode 顶部提供关闭/收起入口，关闭后回到窄面板，但保留产物列表状态。
- 空间不足或移动端不硬塞宽面板，退回全屏 drawer/dialog；这是响应式 fallback，不是桌面主路径。

`FilePreviewPane` 按 `kind` 渲染：

- `markdown`：复用现有 `AssistantMarkdown` 或 markdown 渲染链路。
- `text/json/csv`：代码/纯文本区域，保留换行并可滚动。
- `html`：优先 sandbox iframe；如果后端未能提供安全 HTML，则显示“当前 HTML 预览不可用，请用默认应用打开”。
- `unsupported`：显示不可预览原因，并提供 `Open with default app` 兜底。

## 后端设计

### 已有能力

可直接复用：

- `openGeneratedFile(fileId, conversationId)` -> `open_generated_file`
- `revealFileInFolder(fileId, conversationId)` -> `reveal_file_in_folder`

这两个命令后端通过 `resolve_stored_path` 查 uploaded 和 generated，并以 `fileId + conversationId` 做隔离。

### 新增或扩展预览接口

不要依赖现有 `previewFile` 直接完成第一版，因为当前 `preview_file` 只查 uploaded file，并且返回的是 `{ type, path, name }` 字符串，不是可展示内容。

新增或调整为安全接口：

```ts
type FilePreview =
  | {
      kind: 'markdown' | 'text' | 'json' | 'csv'
      fileName: string
      mimeType: string
      content: string
    }
  | {
      kind: 'html'
      fileName: string
      mimeType: 'text/html'
      content: string
      sandbox: true
    }
  | {
      kind: 'unsupported'
      fileName: string
      reason: string
    }
```

命令建议命名为 `get_file_preview(file_id, conversation_id)` 或把现有 `preview_file` 升级成该结构。实现要求：

- 统一使用 uploaded/generated resolver。
- 仅允许 workspace 内文件。
- 限制最大读取大小，建议第一版 1 MB。
- 仅按 allowlist 类型返回内容。
- 编码失败时返回 unsupported 或明确错误。
- HTML 不执行任意脚本；前端必须使用 sandbox 或显示不可用。

### 路径安全

需要额外审视 `FileManager::full_path` 对 path traversal 的 fallback 行为。当前风险是异常 stored path 可能退回 workspace root。打开/预览类命令更理想的边界是显式返回错误，而不是打开目录。第一版至少在 preview 新接口里使用显式错误；是否同步修复 open/reveal 的 fallback 可作为实现阶段的安全检查项。

## 测试计划

### 前端单元/组件测试

- `GeneratedFileCard`：
  - 渲染 split button。
  - 点击主动作触发 `onPreview` 或 `onOpenExternal`。
  - 点击 chevron 展开 dropdown。
  - `Preview inside` 在不可预览文件时禁用。
  - `Open with default app` 和 `Show in folder` 分别触发正确回调。

- `useTurnRenderModel`：
  - 保留 `fileType/actions`。
  - 兼容缺失 `fileType/actions` 的旧消息。

- `MessageList` / `ChatPage` / `RightPanel` 集成：
  - 点击 open 调用 `openGeneratedFile(fileId, conversationId)`。
  - 点击 reveal 调用 `revealFileInFolder(fileId, conversationId)`。
  - 失败时 push notification toast。
  - 可预览文件使 `RightPanel` 进入 workspace preview mode。
  - 产物列表点击可切换当前预览文件。
  - 关闭 preview mode 后恢复窄任务监控面板。

### 后端验证

- `cargo check` 覆盖新增命令编译。
- 小范围命令测试或 repository-level 测试覆盖：
  - generated file 可以 preview。
  - uploaded file 可以 preview。
  - 错误 conversationId 不能 preview。
  - 超过大小限制返回 unsupported/error。
  - 不支持类型返回 unsupported，不读取为任意内容。

按项目约束，避免默认使用耗时的 `cargo test <filter>` 方式启动全量 test binary；优先使用 `cargo check` 和小范围独立验证。

## 分阶段落地

### Phase 1: 可用动作与右侧面板入口

- 扩展 render model。
- 接通 `openGeneratedFile`。
- 接通 `revealFileInFolder`。
- 改造 split button + dropdown。
- 为 `RightPanel` 增加 preview mode 状态、宽面板布局和产物高亮/切换入口。
- 对尚未能读取内容的文件，在 `FilePreviewPane` 显示 loading/unsupported 占位和外部打开兜底。
- 补前端测试。

Phase 1 完成后，xlsx 这类文件已经能用默认应用打开；可预览文件点击后会打开右侧工作区预览框架，用户不会再点击无反应。

### Phase 2: 简单内部预览内容

- 新增或升级 preview Tauri 命令。
- `FilePreviewPane` 支持 md/txt/json/csv，小心处理 html。
- 产物列表点击真实加载并切换不同文件内容。
- 补后端与前端测试。

### Phase 3: 后续增强

- 根据用户反馈决定是否支持 PDF/图片内嵌预览。
- 增强产物版本、文件搜索、预览历史或 pin 住某个产物。

## 设计决策

推荐采用“显式菜单 + 类型感知主动作 + RightPanel Workspace Preview”：

- 比单纯 `Open` 更符合用户对 md/html 内部预览的预期。
- 比 modal 更适合长 markdown/html，用户可以边看右侧预览边继续聊天。
- 复用现有任务监控面板和产物列表，使产物天然成为文件切换器。
- 下拉菜单能清楚区分内部预览和系统默认应用打开，减少误解。
- 范围比临时 dialog 大，但仍小于重建完整 artifact 中心。
