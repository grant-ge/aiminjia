# Tool Block 折叠 — interleaved 模式视觉弱化

**Date:** 2026-05-28
**Last updated:** 2026-05-29（实施后回写，记录与初稿的偏离）
**Scope:** 前端 — `src/components/chat-scene/` + `src/components/chat/MessageList.tsx`
**Mode 限定:** legacy 聚合卡片模式（`ToolGroupCard` / `toolDisplayMode`）已整体删除（commit `55111103`），全仓库只剩 interleaved 一条路径。本 spec 描述的就是当前唯一实现。

## 实施后偏离 (post-implementation drift, 2026-05-29)

本 spec 初稿与最终实现的几处差异：

1. **行首 `⎿` unicode 字符已删除**。原 spec §"一级展开" 描述用 `⎿ ` 字符做 vertical guide。实施时换成 CSS：父容器 `border-l` 画垂直主干，每条 `ToolStepRow` 用 `::before` 画水平短线接到主干，最后一行 `last:after` 用 `bg-background` 盖掉 stub 之下的 border 延伸，自然收口为"└"。理由：unicode 字符在不同字体下对齐不稳，且 corner 的"父子关系"语义在最初设计时是连到外层 `CompletedStepsBlock` 的 guide line，那层折叠后来被删了（见下条），unicode corner 就悬空了。
2. **`CompletedStepsBlock` turn 级折叠先实施后删除**（commit `dcd80358`）。该折叠把 turn 完成后所有 iter 文字 + 工具卡塞进"已完成 N 步"灰条，只展示"最后一条无 toolCalls 的 assistantText"作为答案。问题：LLM 经常把"最后一个文件的分析" + "综合总结"塞进同一个 `ContentComplete`，最终消息看起来像过程而不是 summary。删除折叠后所有 blocks 按时序统一展开，跟流式期间一致。
3. **`MessageList.renderInterleavedBlocks` ctx 字段简化**：删除 `isComplete` 字段（折叠分支随之删除），保留 `inlineStreamingContent` / `persistedBlockCount` / `showFinalThinkingIndicator`。
4. **`ToolTraceIO` 折叠加字符数兜底**：原 spec 未提；实施时发现 JSON 工具输出真换行少但单行超长（whitespace-pre-wrap wrap 后视觉很多行），原"5 行"判定不触发。加 `DEFAULT_VISIBLE_CHARS = 500` 作为第二条触发条件。

## 背景

当前 interleaved 模式下每个工具调用渲染为一张 `InlineToolBlock` 卡片（rounded-lg border + shadow-card）。
连续多次工具调用时（典型场景：探索代码逐文件 Read），多张卡片纵向堆叠，视觉噪音大、抢主消息正文的注意力。

参考 Codex/ChatGPT 桌面版的做法：连续工具调用折叠为一行 muted 文字摘要
（"Ran 5 commands, read 3 files ›"），点击展开后每条工具是单行文字，再点击单行才显示输入输出详情。

## 目标

1. 连续工具调用合并为**一行**摘要，视觉去掉边框/阴影
2. 单个工具调用也走同一组件，统一视觉
3. 不影响 legacy 卡片模式（`toolDisplayMode === 'grouped'`）
4. 不需要 settings 开关 — interleaved 特有行为，直接替换

## 非目标

- 不改 `useTurnRenderModel` 的契约（`RenderTurnBlock` 不新增 kind）
- 不动 `ToolGroupCard`、`ToolGroupStepRow`、`ToolTraceStep`、`ToolTraceDetails`
- 不动后端 RuntimeEvent / chatStore 状态
- 不改 streaming/persisted 拼接逻辑（`persistedBlockCount` split 行为保留）

## 数据层

**不新增 `RenderTurnBlock` 类型**。在 `MessageList.renderInterleavedBlocks` 渲染前做本地分组：

```
walkBlocks(blocks):
  pending: ToolStep[] = []
  output: ReactNode[] = []
  for b in blocks:
    if b.kind == 'toolStep':
      pending.push(b.step)
    else:
      flush(pending) → output.push(<ToolStepGroupBlock steps={pending} />)
      output.push(renderNonTool(b))
  flush(pending)
  return output
```

单个 toolStep 也走同一 `ToolStepGroupBlock`（steps.length === 1 时，文案规则一致）。

`persistedBlockCount` 的 split：保持现在按 index 切分 persisted vs live 的语义。分组发生在切分之后各自的子数组内（即 persisted 末尾的 toolStep 和 live 开头的 toolStep **不跨界合并**，避免持久化分界对组的状态界定造成歧义）。

## UI 行为

### 折叠态（默认）

```
[icon] 读取了 3 个文件、运行了 2 个命令 ›
```

- 整行可点击 toggle
- 无 border / 无 bg / 无 shadow
- `text-sm text-muted-foreground`，hover 时整行 `text-foreground`
- 右侧 `ChevronRight` (折叠) / `ChevronDown` (展开)，`h-3.5 w-3.5`
- 左侧 icon 由状态决定：
  - 全部完成 → 无 icon（保持轻量）
  - 至少一个 running → `Loader2 animate-spin text-primary h-3.5 w-3.5`
  - 至少一个 error → `AlertCircle text-destructive h-3.5 w-3.5`
- 行高紧凑：`py-1.5 px-0`（不缩进，跟在 AiBubble 段落后视觉自然连贯）

### 一级展开

```
[icon] 读取了 3 个文件、运行了 2 个命令 ↓
│
├── ⊙ Read base_prompt.rs ›
│
├── ⊙ Read chat_turn_driver.rs ›
│
├── ⊙ Read post_process.rs ›
│
├── ⊙ Bash ls src-tauri/... ›
│
└── ⊙ Bash cargo test --test ... ›
```

- 每条用 `<ToolStepRow>` 渲染（新拆出的轻量行组件）
- 进度连线用 CSS 画（**不再用 unicode `⎿` 字符**）：
  - 父容器 `ToolStepGroupBlock` 展开部分 `border-l border-border/60`，`ml-[7px]` 对齐 summary leading icon 中心
  - `ToolStepRow` `::before` 画水平短线（`left-[-12px] top-3 w-3 h-px`）接到主干
  - 最后一行 `last:after` 用 `bg-background` 盖掉 stub 之下那段 border-l 延伸，自然收口为"└"
- 状态 icon：lucide spinner / check / alert，`-translate-y-px` 上移 1px 让 icon 中心刚好压在水平 stub 上
- 整行可点击 → 二级展开

### 二级展开（每行点开）

复用 `ToolTraceIO`：在 `<ToolStepRow>` 下方插入一个无边框 panel，padding 与对齐跟 `⎿` 缩进保持一致。
不再有外层 `<div className="rounded-lg border bg-card shadow-...">`。

二级展开互斥与否：**不互斥**（用户可同时展开多条），与现有 `InlineToolBlock` 行为对齐。

## 文案规则

### 摘要行（一级折叠态）

按"tool name → 动词桶"分类，按出现顺序拼接，逗号分隔，最末项无逗号。

| Tool 名匹配（前缀/相等，忽略大小写） | 桶 key | 中文动词 | 英文动词 |
|---|---|---|---|
| `Bash`, `shell`, `shell_run` | `command` | 运行了 | ran |
| `Read`, `read_file` | `file_read` | 读取了 | read |
| `Write`, `Edit`, `MultiEdit`, `write_file`, `edit_file` | `file_edit` | 编辑了 | edited |
| `Grep`, `grep`, `Glob` | `search` | 搜索了 | searched |
| `mcp__*` | `mcp` | 调用了 | called |
| 其他 | `other` | 使用了 | used |

模板：
- 中文：`{动词} {N} 个{量词}` — `命令` / `文件` / `次` / `MCP 工具` / `工具`
- 英文：`{verb} {N} {noun}` — `commands` / `files` / `times` / `MCP tools` / `tools`

例：
- 5 个 Read + 3 个 Bash → `读取了 5 个文件、运行了 3 个命令`
- 1 个 Read → `读取了 1 个文件`
- 1 个 Bash + 1 个 Grep → `运行了 1 个命令、搜索了 1 次`

i18n key：`chat.toolGroup.summary.bucket.{command|file_read|file_edit|search|mcp|other}`
（用 `count` 插值 + `i18next` 的 plural 规则；中文 plural 等同单数）

### 进行中状态附加

若 `running > 0`：在摘要末尾追加 `…`（不再追加单独"X 个进行中"，spinner 已表达）

### 错误状态附加

若 `error > 0`：摘要末尾追加 `，{N} 个失败`（英文 `, {N} failed`）

### 单行（二级折叠态内容）

`<ToolStepRow>` 文案规则：

- `Bash` / shell → `Bash <command 头部 80 字符截断>`
- `Read` → `Read <basename(path)>`
- `Write` / `Edit` → `Edit <basename(path)>` / `Write <basename(path)>`
- `Grep` → `Grep <pattern 截断>`
- `Glob` → `Glob <pattern>`
- 其他 / MCP → `<tool name>`（保留完整名）

参数从 `step.inputJson` 解析；解析失败 fallback 为单独 tool name。

## 组件清单

### 新增

- `src/components/chat-scene/ToolStepGroupBlock.tsx`
  - props: `{ steps: RenderToolStep[] }`
  - 内部用 `useState<boolean>(false)` 控制一级展开
  - 渲染：折叠态摘要行 / 展开态摘要行 + steps.map(ToolStepRow)

- `src/components/chat-scene/ToolStepRow.tsx`
  - props: `{ step: RenderToolStep }`
  - 内部用 `useState` 控制二级展开（与 `InlineToolBlock` 现有 auto-expand 规则一致：running + progressTail 自动展开）
  - 渲染：单行 + 可选 `ToolTraceIO` panel

- `src/components/chat-scene/__tests__/ToolStepGroupBlock.test.tsx`

### 改

- `src/components/chat/MessageList.tsx`
  - `renderInterleavedBlocks` 里 walk `blocks` 时把连续 `toolStep` 收拢到 `ToolStepGroupBlock`
  - 移除直接 `<InlineToolBlock>` 引用
  - persisted/live split 在分组前先拆，分别 walk（防止跨界合并）

- `src/i18n/zh-CN.json` / `src/i18n/en-US.json`
  - 加 `chat.toolGroup.summary.bucket.*` 六个 key + `chat.toolGroup.summary.failedSuffix`

### 删除 / 保留

- `InlineToolBlock.tsx`：删除（被 ToolStepRow 取代）
- `MessageList.tsx` 里 `InlineToolBlock` 的 import：删除

## 测试

`ToolStepGroupBlock.test.tsx` 覆盖：

1. 单个 Read → 摘要行显示"读取了 1 个文件"，无 icon，chevron 折叠态
2. 3 Read + 2 Bash → 摘要"读取了 3 个文件、运行了 2 个命令"
3. 包含 running → 左侧 spinner，末尾 `…`
4. 包含 error → 左侧 `AlertCircle`，末尾 "1 个失败"
5. 点击摘要行 → 展开渲染 N 个 ToolStepRow
6. 点击 ToolStepRow → 展开渲染 ToolTraceIO（用 mock step.output）
7. 单个工具名 → ToolStepRow 显示 `Read <basename>`

不动 `ToolGroupCard.test.tsx`（legacy 模式）。

## 风险与约束自检

- **memo**：`ToolStepGroupBlock` 不套 React.memo（reuse 现有路径，AiBubble 是套 memo 的，但 toolStep 本身不套）。`RenderToolStep` 在 streaming 时确实会原地 mutate `step.progressTail`、`step.status`、`step.output`（见 `useStreaming.ts` 注释），UI 靠每次 store update 触发 MessageList 整体重渲。新组件同样**不能依赖引用变化**做 memo 优化，且**不要往 step 上写新字段**。
- **颜色**：全程用主题变量（`text-muted-foreground` / `text-foreground` / `text-primary` / `text-destructive`）—— 跟 CLAUDE.md 规范一致。
- **图标**：lucide `Loader2` / `CheckCircle2` / `AlertCircle` / `ChevronRight` / `ChevronDown`，全部 `currentColor`。
- **i18n**：英文 plural 用 i18next 内建复数；中文走 zero/other 等价。
