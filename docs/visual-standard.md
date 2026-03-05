# AI小家 — 视觉设计标准

**设计方向：** Quiet Professional — 极简、护眼、科学感、不刺眼

**实现：** CSS 变量定义在 `src/styles/globals.css`，TailwindCSS 4 通过 `@theme` 注册

---

## 色彩系统

### 主交互色（Carbon Black）

所有按钮、选中态、Tab 激活态使用 Carbon Black：

| Token | 色值 | 用途 |
|-------|------|------|
| `--color-primary` | `#1D1D1F` | 按钮、选中态、Tab 激活 |
| `--color-primary-hover` | `#2C2C2E` | 悬停 |
| `--color-primary-active` | `#000000` | 按下 |

### 品牌色（Gold — 仅 AI 头像和 Logo）

⚠️ Gold 不用于按钮、Tab、选中态等交互元素。

| Token | 色值 | 用途 |
|-------|------|------|
| `--color-accent` | `#D4A843` | AI 头像、Logo |
| `--color-accent-subtle` | `rgba(212,168,67,0.12)` | Tag/Badge/步骤节点背景 |
| `--color-accent-bg-light` | `rgba(212,168,67,0.04)` | 确认区块背景 |

### 中性色（暖白灰阶）

| Token | 色值 | 用途 |
|-------|------|------|
| `--color-bg-base` | `#F2F1EE` | 最深层背景 |
| `--color-bg-sidebar` | `#F5F4F1` | Sidebar |
| `--color-bg-main` | `#FAFAF8` | 主区域 |
| `--color-bg-card` | `#FFFFFF` | 卡片 |
| `--color-bg-msg-user` | `#F2F1EE` | 用户消息气泡 |
| `--color-border` | `#E2E0DC` | 默认边框 |

**文字色阶**：

| Token | 色值 | 用途 |
|-------|------|------|
| `--color-text-primary` | `#1D1D1F` | 标题、强调 |
| `--color-text-secondary` | `#555555` | 正文 |
| `--color-text-muted` | `#8C8C8C` | 辅助说明 |

### 语义色

| 语义 | 主色 | bg (0.12) | bg-light (0.04~0.06) |
|------|------|-----------|---------------------|
| 成功/正常 | `#34C759` | ✓ | ✓ |
| 警告/中等 | `#F5A623` | ✓ | — |
| 危险/严重 | `#EF4444` | ✓ | ✓ |
| 信息/蓝 | `#5B9BD5` | ✓ | ✓ |
| 辅助/紫 | `#9B7ED8` | ✓ | ✓ |

用途：`*-bg` 用于 Tag/Badge，`*-bg-light` 用于区块背景

---

## 字体系统

```css
--font-sans: -apple-system, BlinkMacSystemFont, "PingFang SC", "Microsoft YaHei", sans-serif;
--font-mono: "SF Mono", "Fira Code", Consolas, monospace;
```

| Token | 大小 | 用途 |
|-------|------|------|
| `text-xs` | 12px | Tag、时间戳、Badge |
| `text-sm` | 13px | 表头、表格数据 |
| `text-base` | 14px | 正文、表单输入 |
| `text-md` | 15px | 对话正文 |
| `text-lg` | 17px | 页面标题 |
| `text-2xl` | 24px | Metric 大数字 |

字重：400 正文 / 500 按钮 / 600 小标题 / 700 大标题

---

## 间距系统

基于 4px 栅格：

| Token | 值 | 用途 |
|-------|-----|------|
| `spacing-1` | 4px | 图标与文字间距 |
| `spacing-2` | 8px | 紧凑间距 |
| `spacing-3` | 12px | 卡片 padding、**rich-content 块统一外边距 `my-3`** |
| `spacing-4` | 16px | 标准 padding |
| `spacing-6` | 24px | 主内容区 padding |

**关键约定**：消息气泡间距 `mb-7`(28px)，步骤分割线 `my-7`(28px)

---

## 圆角与阴影

**圆角**：`rounded-xs`(4px) / `rounded-sm`(6px) / `rounded-md`(8px) / `rounded-lg`(12px) / `rounded-xl`(16px)

**阴影**：`shadow-sm`(卡片) / `shadow-md`(下拉) / `shadow-lg`(浮层) / `shadow-modal`(模态框)

---

## 关键组件模式

### 布局

- 整体：`flex h-screen`，Sidebar 260px + Main flex-1
- 聊天内容：`max-width 860px`，居中，padding `24px 24px 160px`
- 输入栏：absolute bottom，max-width 860px，居中

### 对话气泡

- AI：左侧，28px 圆形金色头像（"家"），无背景，`pl-9`
- 用户：右对齐，无头像，`bg-msg-user` 背景，`rounded-xl rounded-br-[4px]`

### 卡片类组件

所有 rich-content（表格、代码块、指标卡、洞察块等）：`bg-card + border + rounded-lg`，外边距统一 `my-3`

### 语义色区块

- 确认区块：accent-bg-light 背景 + accent-border + 3px 左边框
- 根因分析：red-bg-light + red-border
- 洞察区块：blue-bg-light + blue-border
- 搜索来源：purple-bg-light + purple-border

### 图标原则

- 内容区**不使用装饰性 SVG 图标**，通过语义颜色区分类型
- 文件类型用文字标签（HTML/XLS/PDF）+ 语义色，不用 emoji
- SVG 图标仅限操作区域（Sidebar/TopBar/InputBar）
- 所有 SVG 内联，fill 跟随 currentColor

### Flex 对齐规则

- 图标+文字 / 按钮组 / 表单行 → `items-center`
- 可变高度容器(textarea) → `items-end`
- 多行文本 → `items-start`
- **禁止**水平 flex 容器省略 `items-*`

---

## 开发约束

1. 所有字号使用 token 类名（`text-xs` ~ `text-2xl`），禁止 `text-[X.XXrem]`
2. 所有语义色使用 CSS 变量，禁止直接写 rgba 值
3. 所有圆角使用 token，禁止 `rounded-[Xpx]`
4. 所有 rich-content 组件外边距统一 `my-3`
5. 新增颜色先在 globals.css `@theme` 注册
6. 所有水平 flex 容器必须声明 `items-*`
