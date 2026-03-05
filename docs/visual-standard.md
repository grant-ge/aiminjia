# AI小家 — 视觉设计标准

**设计方向：** Quiet Professional — 极简、护眼、科学感、不刺眼

**实现：** CSS 变量定义在 `src/styles/globals.css`，TailwindCSS 4 通过 `@theme` 注册

> 此文档是**唯一权威标准**，所有新增组件必须遵循。

---

## 1. 色彩系统

### 1.1 Primary / Carbon Black（交互元素）

| Token | 值 | 用途 |
|---|---|---|
| `--color-primary` | `#1D1D1F` | 按钮、活跃状态、强调文字 |
| `--color-primary-hover` | `#2C2C2E` | Primary hover 状态 |
| `--color-primary-active` | `#000000` | Primary active 状态 |
| `--color-text-on-primary` | `#FFFFFF` | Primary 背景上的文字 |
| `--color-primary-subtle` | `rgba(29,29,31,0.08)` | 选中/活跃背景色 |
| `--color-primary-muted` | `rgba(29,29,31,0.15)` | 较深的 subtle 变体 |

### 1.2 Accent / Brand（仅用于品牌元素：AI 头像、Logo）

| Token | 值 | 用途 |
|---|---|---|
| `--color-accent` | `#D4A843` | 品牌金色 |
| `--color-accent-hover` | `#C49535` | Accent hover |
| `--color-accent-light` | `#E8C86A` | 浅金色 |
| `--color-accent-subtle` | `rgba(212,168,67,0.12)` | Accent 背景色 |

> **重要**：Accent 颜色仅用于品牌标识，交互元素一律使用 Primary。

### 1.3 背景色

| Token | 值 | 用途 |
|---|---|---|
| `--color-bg-base` | `#F2F1EE` | 全局基底色 |
| `--color-bg-sidebar` | `#F5F4F1` | 侧边栏 |
| `--color-bg-sidebar-hover` | `#EBEAE6` | 侧边栏 hover |
| `--color-bg-main` | `#FAFAF8` | 主内容区、输入框内部 |
| `--color-bg-elevated` | `#FFFFFF` | 悬浮层 |
| `--color-bg-card` | `#FFFFFF` | 卡片、弹窗 |
| `--color-bg-card-hover` | `#F7F6F3` | 卡片 hover |
| `--color-bg-input` | `#FFFFFF` | 输入框容器 |
| `--color-bg-msg-user` | `#F2F1EE` | 用户消息气泡 |

### 1.4 边框色

| Token | 值 | 用途 |
|---|---|---|
| `--color-border` | `#E2E0DC` | 默认边框 |
| `--color-border-light` | `#D0CEC9` | 较深边框 |
| `--color-border-subtle` | `#EDEBE7` | 分隔线、次要边框 |

### 1.5 文字色

| Token | 值 | 用途 |
|---|---|---|
| `--color-text-primary` | `#1D1D1F` | 标题、主要文字 |
| `--color-text-secondary` | `#555555` | 正文、说明文字 |
| `--color-text-muted` | `#8C8C8C` | 辅助信息、占位符 |
| `--color-text-disabled` | `#C0C0C0` | 禁用状态 |

### 1.6 语义色

| Token | 值 | 用途 |
|---|---|---|
| `--color-semantic-green` | `#34C759` | 成功、正常 |
| `--color-semantic-orange` | `#F5A623` | 警告 |
| `--color-semantic-red` | `#EF4444` | 错误、危险 |
| `--color-semantic-blue` | `#5B9BD5` | 信息、洞察 |
| `--color-semantic-purple` | `#9B7ED8` | 搜索来源 |

---

## 2. 字体

### 2.1 字体族

| Token | 值 |
|---|---|
| `--font-sans` | `-apple-system, BlinkMacSystemFont, "PingFang SC", "Microsoft YaHei", "Segoe UI", Roboto, sans-serif` |
| `--font-mono` | `"SF Mono", "Fira Code", "JetBrains Mono", Consolas, "Courier New", monospace` |

### 2.2 字号（基于 Apple HIG，14px base）

| Token | 值 | 行高 | 用途 |
|---|---|---|---|
| `--text-xs` | 0.75rem (12px) | 1.4 | 标签、辅助信息 |
| `--text-sm` | 0.8125rem (13px) | 1.5 | 次要文字、描述 |
| `--text-base` | 0.875rem (14px) | 1.6 | 正文、按钮文字 |
| `--text-md` | 0.9375rem (15px) | 1.6 | AI 回复正文 |
| `--text-lg` | 1.0625rem (17px) | 1.5 | 小标题 |
| `--text-xl` | 1.25rem (20px) | 1.4 | 标题 |
| `--text-2xl` | 1.5rem (24px) | 1.3 | 大标题、指标数字 |

---

## 3. 圆角系统（4 级）

> **核心规则**：圆角按元素层级分 4 档，禁止随意使用中间值。

| 级别 | Token | TailwindCSS | 值 | 适用元素 |
|---|---|---|---|---|
| **Controls** | `--radius-md` | `rounded-md` | 8px | 按钮、输入框、Tab 切换、下拉选项 |
| **Containers** | `--radius-lg` | `rounded-lg` | 12px | 卡片、对话框、下拉菜单、代码块、表格容器、Modal |
| **Surfaces** | `--radius-xl` | `rounded-xl` | 16px | 聊天气泡、输入区域外壳 |
| **Pills** | `--radius-full` | `rounded-full` | 9999px | Badge、标签、头像、进度药丸、StepDivider 标签 |

### 3.1 具体元素映射

| 元素 | 圆角 | 说明 |
|---|---|---|
| Button (primary/secondary/ghost) | `rounded-md` | 所有按钮统一 8px |
| Input / Textarea | `rounded-md` | 与按钮对齐 |
| Tab 切换按钮 (Settings) | `rounded-md` | 选中态带 subtle 背景 |
| Sub-tab 切换按钮 | `rounded-md` | 同上 |
| Modal 容器 | `rounded-lg` | 12px |
| 卡片 (MetricCard, OptionCard 等) | `rounded-lg` | 12px |
| 代码块 | `rounded-lg` | 12px |
| 数据表格容器 | `rounded-lg` | 12px |
| Toast 通知 | `rounded-lg` | 12px |
| 下拉菜单 | `rounded-lg` | 12px |
| 用户消息气泡 | `rounded-xl rounded-br-[4px]` | 16px + 右下角小圆角 |
| InputBar 输入区域 | `rounded-xl` | 16px |
| Badge / 标签 | `rounded-full` | 胶囊形 |
| 头像 | `rounded-full` | 正圆形 |
| Progress 步骤药丸 | `rounded-full` | 胶囊形 |
| StepDivider 标签 | `rounded-full` | 胶囊形 |
| 版本号标签 | `rounded-full` | 胶囊形 |

### 3.2 禁止使用的圆角值

- `rounded-xs` (4px) — 除非有特殊小元素需求
- `rounded-sm` (6px) — 已废弃，统一用 `rounded-md`
- `rounded-2xl` (24px) — 用 `rounded-full` 替代
- 任何硬编码的 `border-radius` 数值（除聊天气泡右下角 4px）

---

## 4. 间距系统（4px 网格）

| Token | 值 | 用途 |
|---|---|---|
| `--spacing-1` | 4px | 最小间距 |
| `--spacing-2` | 8px | 紧凑间距 |
| `--spacing-3` | 12px | 默认组内间距 |
| `--spacing-4` | 16px | 标准间距 |
| `--spacing-5` | 20px | 区块间距 |
| `--spacing-6` | 24px | 大区块间距 |
| `--spacing-8` | 32px | 大区域间距 |
| `--spacing-10` | 40px | 页面级间距 |

---

## 5. 阴影系统

| Token | 值 | 用途 |
|---|---|---|
| `--shadow-sm` | `0 1px 3px rgba(0,0,0,0.06)` | 微阴影 |
| `--shadow-md` | `0 4px 12px rgba(0,0,0,0.08)` | 卡片、Toast |
| `--shadow-lg` | `0 12px 40px rgba(0,0,0,0.12)` | 悬浮面板 |
| `--shadow-modal` | `0 20px 60px rgba(0,0,0,0.15)` | Modal、下拉 |
| `--shadow-input` | `0 0 0 1px border-subtle, 0 2px 8px rgba(0,0,0,0.04)` | 输入区域 |
| `--shadow-accent` | `0 0 0 3px rgba(212,168,67,0.2)` | Accent focus |

---

## 6. 动画与过渡

| Token | 值 | 用途 |
|---|---|---|
| `--transition-fast` | `150ms ease` | 按钮 hover、颜色变化 |
| `--transition-normal` | `250ms ease-out` | 面板展开 |
| `--transition-slow` | `300ms ease` | 页面切换 |

### 关键动画

| 动画名 | 效果 | 用途 |
|---|---|---|
| `fadeUp` | 透明度 0→1 + 上移 8px | 消息气泡入场 |
| `modalIn` | 透明度 0→1 + 缩放 0.97→1 | Modal 入场 |
| `blink` | 闪烁 | 打字指示器 |
| `pulse` | 呼吸 | 加载状态 |

---

## 7. 组件规范

### 7.1 Button

- 三种变体：`primary`（深色填充）、`secondary`（白底边框）、`ghost`（透明无边框）
- 两种尺寸：
  - `md`（默认）：`h-9` (36px)，`px-3.5`，`text-sm` (13px) — 与 Input 等高
  - `sm`：`h-7` (28px)，`px-2.5`，`text-xs` (12px) — 紧凑场景（文件卡片等）
- 圆角：`rounded-md` (8px)
- 禁用态：`opacity: 0.5`，`cursor: not-allowed`

### 7.2 Input

- 高度：`h-9` (36px) — 与 Button md 等高
- 圆角：`rounded-md` (8px)
- 字号：`text-sm` (13px) — 与 Button 一致
- 背景：`--color-bg-main`
- 边框：`--color-border`
- 不使用 `py-*`，高度由 `h-9` 控制垂直居中

### 7.3 Modal

- 圆角：`rounded-lg` (12px)
- 阴影：`--shadow-modal`
- 背景：`--color-bg-card`
- 遮罩：`--color-overlay`

### 7.4 Toast

- 圆角：`rounded-lg` (12px)
- 左侧 3px 语义色边框
- 阴影：`--shadow-md`

### 7.5 Tab 切换

- 圆角：`rounded-md` (8px)
- 选中态：`--color-primary-subtle` 背景 + `--color-primary` 文字
- 未选中：透明背景 + `--color-text-muted` 文字

### 7.6 Badge

- 圆角：`rounded-full` (胶囊形)
- `px-2.5 py-0.5`，`text-xs`
- 语义色背景 + 对应文字色

### 7.7 Avatar

- 尺寸：`h-7 w-7` (28px)
- 圆角：`rounded-full`
- AI 头像：产品图标
- 用户头像：深色背景 + 白色人物图标

### 7.8 聊天气泡

- 用户消息：`rounded-xl rounded-br-[4px]`，`--color-bg-msg-user` 背景，右对齐
- AI 消息：无气泡背景，左对齐，`pl-9` 偏移

---

## 8. 响应式规范

- 最小宽度：`1080px`
- 侧边栏固定宽度：`260px`
- 聊天内容最大宽度：`860px`，居中
- 输入栏最大宽度：`860px`，居中

---

## 9. 滚动条

- 宽度：`6px`
- 滑块颜色：`--color-border`
- 滑块圆角：`3px`
- 轨道：透明

---

## 10. 图标原则

- 内容区**不使用装饰性 SVG 图标**，通过语义颜色区分类型
- 文件类型用文字标签（HTML/XLS/PDF）+ 语义色，不用 emoji
- SVG 图标仅限操作区域（Sidebar/TopBar/InputBar）
- 所有 SVG 内联，fill 跟随 `currentColor`

---

## 11. Flex 对齐规则

- 图标+文字 / 按钮组 / 表单行 → `items-center`
- 可变高度容器(textarea) → `items-end`
- 多行文本 → `items-start`
- **禁止**水平 flex 容器省略 `items-*`

---

## 12. 关键间距约定

- 消息气泡间距：`mb-7` (28px)
- 步骤分割线：`my-7` (28px)
- 所有 rich-content 块外边距：`my-3` (12px)
- 卡片内 padding：`p-3.5` 或 `p-4`
- 字重：400 正文 / 500 按钮 / 600 小标题 / 700 大标题

---

## 13. 开发约束

1. 所有字号使用 token 类名（`text-xs` ~ `text-2xl`），禁止 `text-[X.XXrem]`
2. 所有语义色使用 CSS 变量，禁止直接写 rgba 值
3. 所有圆角使用 token（4 级体系），禁止 `rounded-[Xpx]`
4. 所有 rich-content 组件外边距统一 `my-3`
5. 新增颜色先在 globals.css `@theme` 注册
6. 所有水平 flex 容器必须声明 `items-*`
