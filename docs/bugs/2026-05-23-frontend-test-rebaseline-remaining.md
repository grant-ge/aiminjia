# TODO：前端单测重对齐 — 剩余 25 个失败(2026-05-23)

## 背景

`pnpm test` 有一批既有失败的前端单测,根因是 oayzz 一轮 UI 大改(IM 多平台频道矩阵、技能页、产物预览面板、设置页等)后**测试腐烂**:组件已随 v0.5.29 上线、是事实上的预期行为,但测试断言停留在旧设计。

本轮已修 **43 → 25**(4 个 commit:`13d3e11f` / `f2f54527` / `e26b9bc3` / `c3aa8c61`),包括:
- 新增 `src/test/setup-tauri.ts` 全局 stub `window.__TAURI_INTERNALS__`(消除组件 mount 时 IPC 同步崩溃);
- **真实 a11y 回归修复**:`ExecutionTraceCard` 可折叠头部按钮补 `aria-expanded`;
- 文案/尺寸/class/store-mock 重对齐 + 删除 2 个过时重复文件。

剩下 25 个失败集中在**改版幅度大、需对照新组件重写交互断言**的文件,不是简单换字符串,故单独记录。

## 复现

```bash
cd code
pnpm exec vitest run   # 看 FAIL 列表
# 或单文件:pnpm exec vitest run src/components/chat/RightPanel.test.tsx
```

## 剩余清单(按文件)

### 1. `src/components/chat/RightPanel.test.tsx`(10)— 产物预览面板改版
预览/打开按钮的 aria-name、产物列表项结构、`data-testid="right-panel"` 都变了。需对照当前 `RightPanel` + 产物行组件重写。失败用例:
- renders the default narrow panel without empty preview
- filters the artifact list by conversation
- switches the preview target when clicking a previewable / image artifact
- previews image artifacts when legacy actions omit preview
- keeps non-previewable artifacts disabled when no default-app opener is available
- opens non-previewable artifacts with the default app instead of disabling them
- previews previewable / json+csv artifacts even when preview action is disabled
- keeps preview-disabled markdown artifacts previewable by type

### 2. `src/components/sidebar/__tests__/AppSidebar.test.tsx`(5)— 多平台频道矩阵改版
单平台(dingtalk)频道模型 → 多平台(dingtalk/feishu/wecom/wechat/telegram)。频道 tab、频道列表渲染、nav 项数都变了。失败用例:
- renders the main nav items, the section title 项目, and footer 设置(`expected 1 to be greater than 1`)
- renders a top drag-region spacer on macOS(spacer 选择器失效)
- separates expert team conversations from the project tab into an expert team tab
- switches the sidebar body between 项目 and 频道 tabs without changing route(找不到「频道」按钮)
- (route-derived sidebarTab)shows channel list after fresh mount when channel tab persisted(频道会话渲染)
> 另:已删除的 `sidebar/AppSidebar.test.tsx`(非 __tests__)曾覆盖频道区「未配置/折叠」状态,这部分覆盖应在本文件按新多平台设计**重新补写**。

### 3. `src/features/skill-center/SkillCenterPage.integration.test.tsx`(4)— 技能导入流程
directory picker 调用 / 校验结果对话框文案("技能目录不符合规范")变了。失败用例:
- 点击「+ 导入技能」走 directory picker(picker mock 未被调用)
- upload 抛出 SkillValidationError / parseFailed 时弹校验结果对话框(文案不匹配)
- upload 抛出 alreadyExists 时走覆盖确认

### 4. `src/components/chat/MessageList.generatedFiles.test.tsx`(3)— 生成文件预览交互改版
- opens generated files using the file id and owning conversation id
- previews markdown generated files from the primary action
- previews image generated files from the primary action without opening externally(`Cannot read properties of undefined (reading 'has')` — 疑似某 Set/Map 未初始化,需确认是测试 setup 还是组件)

### 5. 单个失败
- `src/components/chat/MessageList.test.tsx` > pushes an error toast when open external fails — toast title 文案变了(`无法打开文件` → 现文案待查)
- `src/components/chat/AiBubble.subagent.test.tsx` > renders SubAgentResultCard when message has subagentEnvelope — 找不到 `data-testid="transcript-viewer-stub"`(stub/结构变了)
- `src/features/skill-detail/SkillDetailPage.test.tsx` > renders try items without click-to-run behavior — 找不到 `data-testid="skill-card"`(改版)

## 注意事项(本轮总结的判断准则)

- **组件已上线 = 事实预期**,默认以组件为准重对齐测试断言;
- 但凡看着像**真实回归**(测试对、组件错)的,**修组件**而不是弱化测试 —— 例如本轮的 `ExecutionTraceCard` `aria-expanded`。`MessageList.generatedFiles` 的 `reading 'has'` 需要按此甄别;
- 弹窗确认按钮可能与列表行同名(如 ArchivedPanel 的「恢复」),用 `within(screen.getByRole('alertdialog'))` 作用域消歧。
