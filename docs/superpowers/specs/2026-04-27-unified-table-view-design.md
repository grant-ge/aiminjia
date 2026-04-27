# 统一表格视图（TableView）设计

- 日期：2026-04-27
- 范围：前端表格渲染统一
- 现状：仓库存在两套表格——`src/lib/markdown.ts::renderTable` 和 `src/components/rich-content/RichDataTable.tsx`——视觉、能力、维护链路均不一致
- 目标：建立单一表格组件 `TableView`，作为 markdown 表格与 chat DataTable 的共同出口；视觉对齐参考项目 `qob` 的极简数据风

## 1. 架构与文件布局

```
src/
├── components/
│   ├── data-table/                    # 新增，表格领域唯一入口
│   │   ├── TableView.tsx              # 核心组件
│   │   ├── TableHeader.tsx            # 表头 + 排序
│   │   ├── TableBody.tsx              # 行渲染 + 截断/展开 + 胶囊
│   │   ├── TableToolbar.tsx           # 标题 + badge + 复制
│   │   ├── tableSchema.ts             # 类型 (TableColumn / TableRow / TableCellSpec)
│   │   ├── tableUtils.ts              # 排序、CSV/TSV 序列化
│   │   ├── mapDataTable.ts            # types/message.ts::DataTable → TableView schema
│   │   ├── __tests__/TableView.test.tsx
│   │   ├── __tests__/mapDataTable.test.ts
│   │   └── index.ts                   # 导出 TableView + 类型
│   ├── chat-scene/
│   │   └── AssistantMarkdown.tsx      # 改：调 react-markdown，table override → TableView
│   ├── chat/
│   │   └── AiBubble.tsx               # 改：RichDataTable → TableView
│   └── rich-content/
│       ├── RichDataTable.tsx          # 删除
│       └── index.ts                   # 移除 RichDataTable 导出
├── lib/
│   ├── markdown.ts                    # 重写：改为 react-markdown + remark-gfm 桥接
│   └── markdown.test.ts               # 改：保留普通 markdown 行为测试，新增表格集成测试
└── styles/
    └── globals.css                    # 新增 --table-* 设计 token
```

关键点：
- `data-table/` 不放在 `rich-content/` 下；markdown 也用它，语义上不与"富内容"绑死
- `markdown.ts` 由"返回 HTML 字符串"改为"返回 React Element 树"；所有调用 `markdownToHtml` 的地方需要切换到新 API
- 设计 token 落在 `globals.css`，TableView 内部仅用 className，不写行内 style
- `RichDataTable` 当前只有 `AiBubble.tsx` 一个调用者，迁移面积可控

## 2. 数据结构与 props

### 2.1 类型定义（`tableSchema.ts`）

```ts
export type CellAlign = 'left' | 'center' | 'right'
export type CellTone =
  | 'neutral' | 'success' | 'warning' | 'danger' | 'info' | 'accent'

export interface TableCellSpec {
  text: string
  tone?: CellTone
  variant?: 'pill' | 'plain' | 'bold'
}

export type TableCellValue = string | number | null | TableCellSpec

export interface TableColumn {
  key: string
  label: string
  align?: CellAlign            // 默认 left
  width?: number | string      // 数字按 px；字符串原样（min-content / 1fr）
  wrap?: 'truncate' | 'wrap'   // 默认 truncate
  sortable?: boolean           // 仅在 enableSort 开启时生效
  sortType?: 'string' | 'number' | 'date'  // 默认 string
  tabularNums?: boolean
}

export type TableRow = Record<string, TableCellValue>

export interface TableMeta {
  title?: string
  badge?: string
  footnote?: string
}
```

### 2.2 TableView props

```ts
interface TableViewProps {
  columns: TableColumn[]
  rows: TableRow[]
  meta?: TableMeta

  // 行为开关，默认全关
  enableSort?: boolean
  enableCopy?: boolean
  stickyHeader?: boolean
  maxHeight?: number | string
  truncateRows?: number   // 超过则默认显示前 N 行；undefined = 不截断

  className?: string      // 仅外层包装可定制
}
```

设计约束：
- `TableCellSpec` 不允许任意 className，避免样式碎片
- 行为开关默认全关——markdown 表格不传任何开关，AiBubble 按需开启
- `stickyHeader` 必须配合 `maxHeight`，否则开发模式 `console.warn`

### 2.3 调用方接入

**Markdown 端：** `AssistantMarkdown.tsx` 通过 react-markdown `components.table` override：
```tsx
const components = {
  table: ({ children }) => {
    const { columns, rows } = extractTableFromGfm(children)
    if (columns.length === 0) return <FallbackTable>{children}</FallbackTable>
    return <TableView columns={columns} rows={rows} className="my-3" />
  },
}
```

`extractTableFromGfm`：
- 第一行 `<th>` → `columns[i].label`，`columns[i].key = String(i)`
- 数据格 → `TableRow`，cell 是 `string`（GFM 不带 tone）
- 失败返回空数组，外层降级原生 `<table>`

**AiBubble 端：**
```tsx
<TableView
  columns={mapDataTableColumns(table.columns)}
  rows={mapDataTableRows(table.rows)}
  meta={{ title: table.title, badge: table.badge }}
  enableCopy
  truncateRows={50}
/>
```

`types/message.ts::DataTable` 类型保持不动；`mapDataTable.ts` 负责把 `CELL_COLOR_MAP` 等现有语义映射到 `TableCellSpec.tone`。

## 3. 视觉与样式 token

### 3.1 设计 token（`src/styles/globals.css`）

```css
:root {
  /* 表格基础 — 不强制 font-family，继承父级 */
  --table-font-size: 14px;
  --table-line-height: 1.5;

  /* 容器 */
  --table-bg: var(--color-bg-card);
  --table-radius: 8px;
  --table-border: var(--color-border);
  --table-divider: var(--color-border-subtle);

  /* 表头 */
  --table-header-bg: var(--color-bg-base);
  --table-header-fg: var(--color-text-secondary);
  --table-header-weight: 600;
  --table-header-pad-y: 8px;
  --table-header-pad-x: 12px;

  /* 表体 */
  --table-cell-fg: var(--color-text-primary);
  --table-cell-pad-y: 8px;
  --table-cell-pad-x: 12px;
  --table-row-zebra: color-mix(in srgb, var(--color-bg-base) 40%, transparent);
  --table-row-hover: color-mix(in srgb, var(--color-bg-base) 60%, transparent);

  /* 单元格 tone（pill 与 plain 共用） */
  --table-tone-neutral-bg: color-mix(in srgb, var(--color-text-secondary) 10%, transparent);
  --table-tone-neutral-fg: var(--color-text-primary);
  --table-tone-success-bg: color-mix(in srgb, var(--color-accent-success) 14%, transparent);
  --table-tone-success-fg: var(--color-accent-success);
  --table-tone-warning-bg: color-mix(in srgb, var(--color-accent-warning) 14%, transparent);
  --table-tone-warning-fg: var(--color-accent-warning);
  --table-tone-danger-bg:  color-mix(in srgb, var(--color-accent-danger) 14%, transparent);
  --table-tone-danger-fg:  var(--color-accent-danger);
  --table-tone-info-bg:    color-mix(in srgb, var(--color-accent-info) 14%, transparent);
  --table-tone-info-fg:    var(--color-accent-info);
  --table-tone-accent-bg:  color-mix(in srgb, var(--color-accent) 14%, transparent);
  --table-tone-accent-fg:  var(--color-accent);
}

[data-theme='dark'] {
  /* 大多 token 通过 color-mix 自动跟随；zebra 在深色下需削弱 */
  --table-row-zebra: color-mix(in srgb, var(--color-bg-base) 25%, transparent);
}
```

注：所有最终颜色都来自现有 `--color-*`；如项目现有变量名不存在（如 `--color-accent-success`），实施期需先核对并对齐。

### 3.2 视觉规范

| 元素 | 规范 |
|---|---|
| 字体 | 继承父级，不主动声明 `font-family` |
| 字号 | 14px（统一 token） |
| 外框 | 1px solid `--table-border`，8px 圆角，`overflow:hidden` |
| 表头 | 背景 `--table-header-bg`；底边 1px solid `--table-border`（用主边框，不用 divider） |
| 表头排序态 | `enableSort` + `sortable` 才显示三角图标；激活态高亮 |
| 表体行高 | 8px 12px 内边距，总行高约 32px |
| 斑马纹 | 偶数行 `--table-row-zebra`；奇数行透明 |
| 行 hover | `--table-row-hover`，整行高亮 |
| 列分隔 | 不画（qob 风格无竖线） |
| 行分隔 | 每行底边 1px solid `--table-divider` |
| Sticky header | `position: sticky; top: 0; z-index: 1`；底边 `box-shadow: 0 1px 0 var(--table-border)` |
| 单元格截断 | 默认 truncate（`overflow:hidden + ellipsis`），title 属性兜底 |
| `tone='pill'` | 4px 圆角胶囊，`padding: 0 6px`，tone bg+fg |
| `tone='plain'` | 仅 tone fg，无背景 |
| `variant='bold'` | font-weight 600，颜色随 tone |
| `tabularNums=true` | `font-variant-numeric: tabular-nums` |

### 3.3 容器结构

```
<div className="table-frame">          # 圆角 + 边框 + overflow:hidden
  <TableToolbar />                     # 仅 meta/copy 触发时渲染
  <div className="table-scroll" style={{ maxHeight }}>
    <table>
      <thead className="sticky top-0">
      <tbody>...</tbody>
    </table>
  </div>
  <TableFooter />                      # 截断态 / footnote 触发时渲染
</div>
```

布局示意：

```
┌─────────────────────────────────────────────┐
│  Title         [BADGE]          [📋 复制]   │  TableToolbar
├─────────────────────────────────────────────┤
│  Header  Header  Header                     │
├─────────────────────────────────────────────┤
│  ...                                        │
└─────────────────────────────────────────────┘
   共 128 行 · 显示前 50 行 · [展开全部]        TableFooter
```

- toolbar 字号 14px，背景与表头一致（`--table-header-bg`），底边沿 divider
- 复制按钮：默认 tooltip "复制为 CSV"；按住 Shift 切换为 "复制为 TSV"
- footer：截断态强制显示「共 X 行 · 显示前 N 行 · [展开全部]」；非截断态仅在 `meta.footnote` 存在时显示

### 3.4 集成点

- 表格在 chat 气泡内默认 `my-3` 上下间距
- 气泡受限宽度时，表格自动 `overflow-x-auto`
- markdown 表格不带 toolbar、不开 sticky/copy（最轻量）
- AiBubble DataTable：`enableCopy + truncateRows=50`；title/badge 由 schema 提供

## 4. 行为细节

### 4.1 排序

- 列 `sortable: true` 才显示排序图标
- 默认 `null`（不排序）；点击表头循环 `null → asc → desc → null`
- 比较器按 `column.sortType`：
  - `string`（默认）：`Intl.Collator` 比较 `cell.text ?? String(cell)`
  - `number`：`Number(cell.text ?? cell)`，NaN 排末尾
  - `date`：`Date.parse`，无效值排末尾
- 排序状态在 TableView 内部 `useState`
- 可访问性：`<th>` 加 `aria-sort="ascending|descending|none"`，按钮可 Tab/Enter 触发

### 4.2 截断 + 展开

- `truncateRows` 未传 → 全量
- 传值且 `rows.length > truncateRows`：默认渲染前 N 行，footer 显示「共 X 行 · 显示前 N 行 · 展开全部」
- 点击「展开全部」切换为「折叠」，再次点击恢复截断
- **截断与排序联动：先排序整表 → 再切片**，确保截断态显示有序数据集的前 N 行
- 截断状态在 TableView 内部 `useState`，初始为 true

### 4.3 Sticky header

- `stickyHeader=true` 必须配合 `maxHeight`
- 开发模式 `stickyHeader=true && !maxHeight` → `console.warn`
- markdown 表格、AiBubble 默认都不开

### 4.4 复制 CSV / TSV

- `enableCopy=true` 才在 toolbar 出按钮
- 序列化：
  - 表头：`columns.map(c => c.label)`
  - 行：按 `columns.key` 顺序读 cell（`cell.text ?? cell`）
  - CSV 转义：含 `,` `"` `\n` 字段双引号包裹，内部 `"` → `""`
  - TSV：将字段中 `\t` `\n` 替换为单空格，不引号包裹
  - 行分隔统一 `\r\n`
- 默认 CSV；按住 Shift 点击切 TSV（按下 Shift 时 tooltip 文案变 "复制为 TSV"）
- `navigator.clipboard.writeText`；失败 toast `复制失败`，成功 toast `已复制`
- **始终复制全量 rows**，不受截断态影响

### 4.5 单元格渲染规则

- `cell == null` → 渲染 `—`，色用 `--color-text-tertiary`
- `cell` 是 `string | number` → 直接文本，按列 `align` 对齐
- `cell` 是 `TableCellSpec`：
  - `variant='pill'`：胶囊，tone bg/fg
  - `variant='bold'`：加粗，颜色随 tone
  - `variant='plain'` 或缺省：仅 tone 文字色
- `column.wrap='truncate'`（默认）：单元格 `truncate`，`title` 属性放完整文本
- `column.wrap='wrap'`：允许自动换行
- `column.tabularNums=true`：加 `font-variant-numeric: tabular-nums`

### 4.6 空状态

- `rows.length === 0`：表头照常渲染，表体显示一行 `colSpan = columns.length` 的占位 `暂无数据`，`text-secondary`，padding 与正常行一致
- 不暴露 prop，所有调用方一致

### 4.7 错误兜底

- markdown `extractTableFromGfm` 失败：`console.warn`，渲染原生 GFM HTML（react-markdown 默认 table），不阻塞消息显示
- AiBubble schema 转换失败：渲染一行错误占位 `表格数据格式异常`

## 5. 测试策略

### 5.1 TableView 单测（`src/components/data-table/__tests__/TableView.test.tsx`）

| 用例分组 | 关键断言 |
|---|---|
| 渲染基础 | columns + rows 渲染出 `<th>/<td>`；空 rows 渲染 `暂无数据` 占位 |
| TableCellSpec | `pill` 渲染胶囊（tone class）；`bold` font-weight 600；`null` cell → `—` |
| 列对齐 | `align='right'` `<td>` 有 right class；`tabularNums=true` 加 numeric class |
| 排序 | `enableSort=false` 表头无图标；`sortable` 列点击循环 null→asc→desc→null；string/number/date 排序结果正确 |
| 截断 + 展开 | `truncateRows=2` rows=5 时只渲染 2 行 + footer；点击展开 → 5 行；再次点击折叠 → 2 行 |
| 截断与排序联动 | 截断态点击表头排序 → 截断后是排序后的前 N 行 |
| Toolbar | 无 meta/copy 时不渲染 toolbar；有 title 时渲染；`enableCopy` 渲染复制按钮 |
| 复制 CSV/TSV | mock `navigator.clipboard.writeText`；点击调用 writeText 内容为 CSV；按 Shift 点击 → TSV；CSV 转义规则正确 |
| 空态/兜底 | rows=[] 渲染占位行；cell=null 渲染 `—` 且 tertiary 色 |
| Sticky 警告 | `stickyHeader=true && !maxHeight` 触发 `console.warn` |

### 5.2 Markdown 集成测试（`src/lib/markdown.test.ts`）

| 用例 | 断言 |
|---|---|
| 基础 GFM 表格 | 渲染含 `data-testid="table-view"`，表头/行内容正确 |
| 空表格降级 | 仅表头无 body 时走 TableView，渲染 `暂无数据` 占位行（与 §4.6 空态一致） |
| 解析异常降级 | mock `extractTableFromGfm` 返回 `{columns:[], rows:[]}` → 渲染 fallback `<table>` 而非 TableView，且 `console.warn` 被调用 |
| 非表格 markdown 不动 | 行内代码、链接、列表照常渲染 |

### 5.3 schema 转换（`src/components/data-table/__tests__/mapDataTable.test.ts`）

- `mapDataTableColumns` / `mapDataTableRows` 正确转换 `types/message.ts::DataTable`
- `CELL_COLOR_MAP` 颜色语义准确映射到 `TableCellSpec.tone`（避免颜色漂移）

### 5.4 不做的事

- 不做视觉回归 / 截图测试
- 不测样式精确像素（测 className / data-attr）
- 不测 react-markdown 自身的转换正确性

### 5.5 验收命令

```bash
# 表格相关单测
pnpm test src/components/data-table src/lib/markdown.test.ts

# 既有相关回归（chat 渲染链路）
pnpm exec vitest run \
  src/components/chat/AiBubble.subagent.test.tsx \
  src/components/chat/StreamingBubble.test.tsx
```

## 6. 迁移注意事项

- `markdown.ts` 输出由 HTML 字符串 → React Element：所有 `dangerouslySetInnerHTML={{ __html: markdownToHtml(...) }}` 调用点需切换到新 API（输出 ReactNode）。实施期需先盘点调用点
- `RichDataTable` 删除前确认无其他被忽略的调用方（grep 全仓再次核对）
- `globals.css` 引用的 `--color-accent-success` 等变量名需先核对项目当前实际命名，必要时调整 token 引用
- 引入 `react-markdown` 与 `remark-gfm` 依赖（package.json），在 plan 阶段确认版本与现有生态兼容
