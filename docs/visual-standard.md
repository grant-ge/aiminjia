# AI小家 — 视觉交互设计标准

> 基于 renlijia.png Logo 的金色/琥珀色调，定义产品全局视觉语言。
> 浅色主题，专业温暖，视觉友好，数据密集型界面优化。
> **设计方向：Quiet Professional** — 极简、护眼、科学感、不刺眼。

---

## 一、品牌色彩来源

Logo 为有机曲线构成的 "A" 形，金色渐变，传达 **温暖 / 信赖 / 专业** 的品牌气质。

从 Logo 中提取的核心色值：

| 色彩角色 | 色值 | 说明 |
|---------|------|------|
| Logo 亮金 | `#E8C86A` | 渐变高光区 |
| Logo 主金 | `#D4A843` | 视觉重心色 |
| Logo 深金 | `#C49535` | 渐变暗部 |
| Logo 底金 | `#A67C2E` | 最深投影区 |

---

## 二、色彩系统（Design Tokens）

> **实际 CSS 变量前缀为 `--color-*`**，如 `--color-primary`、`--color-bg-main`。
> TailwindCSS 4 通过 `@theme` 注册，使用时写 `bg-primary`、`text-text-primary` 等。

### 2.0 主交互色（Primary / Carbon Black）

所有按钮、选中态、Tab 激活态、选项卡边框等交互元素使用 Carbon Black：

| Token | 色值 | 用途 |
|-------|------|------|
| `--color-primary` | `#1D1D1F` | **主色 — 按钮、选中态、Tab 激活** |
| `--color-primary-hover` | `#2C2C2E` | 悬停态 |
| `--color-primary-active` | `#000000` | 按下态 |
| `--color-text-on-primary` | `#FFFFFF` | 深色背景上的文字 |
| `--color-primary-subtle` | `rgba(29, 29, 31, 0.08)` | 选中/激活背景色 |
| `--color-primary-muted` | `rgba(29, 29, 31, 0.15)` | 选中边框 |

### 2.1 品牌色（Accent / Brand — 仅用于 AI 头像和 Logo）

⚠️ **Gold 仅用于品牌标识**：AI 头像圆形背景、Logo 上下文。不用于按钮、Tab、选中态等交互元素。

| Token | 色值 | 用途 |
|-------|------|------|
| `--color-accent-50` | `#FEF9EC` | 极浅背景 |
| `--color-accent-100` | `#FBF0D1` | 浅色 Tag / Badge 背景 |
| `--color-accent-200` | `#F5DDA3` | 进度条背景 |
| `--color-accent-300` / `--color-accent-light` | `#E8C86A` | 高亮 / Logo 亮部 |
| `--color-accent-400` / `--color-accent` | `#D4A843` | **品牌色 — AI 头像、Logo** |
| `--color-accent-500` / `--color-accent-hover` | `#C49535` | 悬停态 |
| `--color-accent-600` / `--color-accent-active` | `#A67C2E` | 按下态 |
| `--color-accent-700` | `#875F1E` | 文字前景（浅色背景上） |
| `--color-accent-subtle` | `rgba(212, 168, 67, 0.12)` | Tag/Badge/步骤节点背景 |
| `--color-accent-muted` | `rgba(212, 168, 67, 0.25)` | 选中边框 |
| `--color-accent-bg-light` | `rgba(212, 168, 67, 0.04)` | 确认区块背景 |
| `--color-accent-border` | `rgba(212, 168, 67, 0.25)` | 确认区块边框 |

### 2.2 中性色（Neutral — 浅色主题）

浅色主题中性色基于暖白灰阶：

| Token | 色值 | 用途 |
|-------|------|------|
| `--color-bg-base` | `#F2F1EE` | 最深层背景 |
| `--color-bg-sidebar` | `#F5F4F1` | Sidebar 背景 |
| `--color-bg-sidebar-hover` | `#EBEAE6` | Sidebar 悬停 |
| `--color-bg-main` | `#FAFAF8` | 主区域背景 |
| `--color-bg-elevated` | `#FFFFFF` | 弹窗 / 浮层背景 |
| `--color-bg-card` | `#FFFFFF` | 卡片背景 |
| `--color-bg-card-hover` | `#F7F6F3` | 卡片悬停 |
| `--color-bg-input` | `#FFFFFF` | 输入框背景 |
| `--color-bg-msg-user` | `#F2F1EE` | 用户消息气泡 |
| `--color-bg-code` | `#F5F4F0` | 代码块背景 |
| `--color-border` | `#E2E0DC` | 默认边框 |
| `--color-border-light` | `#D0CEC9` | 悬停边框 / 分割线 |
| `--color-border-subtle` | `#EDEBE7` | 弱分割线 |

**文字色阶：**

| Token | 色值 | 用途 | 对比度(on #FAFAF8) |
|-------|------|------|-------------------|
| `--color-text-primary` | `#1D1D1F` | 标题、强调文字 | 17.2:1 |
| `--color-text-secondary` | `#555555` | 正文内容 | 7.5:1 |
| `--color-text-muted` | `#8C8C8C` | 辅助说明、时间戳 | 3.4:1 |
| `--color-text-disabled` | `#C0C0C0` | 禁用态文字 | 1.8:1 |
| `--color-text-on-accent` | `#1A1A1A` | 金色背景上的文字 | 7.2:1 |
| `--color-text-code` | `#383838` | 代码块文字 | — |

**其他 UI 色：**

| Token | 色值 | 用途 |
|-------|------|------|
| `--color-overlay` | `rgba(0, 0, 0, 0.5)` | 模态框遮罩层 |
| `--color-user-avatar` | `#6366f1` | 用户头像紫色背景 |

### 2.3 语义色（Semantic）

用于数据分析中的状态指示、异常标记、图表区分：

| 语义 | 主色 Token | 背景 Token | 浅背景 Token | 边框 Token |
|------|-----------|-----------|-------------|-----------|
| 成功/正常 | `--color-semantic-green` `#34C759` | `--color-semantic-green-bg` `rgba(52,199,89,0.12)` | `--color-semantic-green-bg-light` `rgba(52,199,89,0.04)` | — |
| 警告/中等 | `--color-semantic-orange` `#F5A623` | `--color-semantic-orange-bg` `rgba(245,166,35,0.12)` | — | — |
| 危险/严重 | `--color-semantic-red` `#EF4444` | `--color-semantic-red-bg` `rgba(239,68,68,0.12)` | `--color-semantic-red-bg-light` `rgba(239,68,68,0.04)` | `--color-semantic-red-border` `rgba(239,68,68,0.18)` |
| 信息/蓝 | `--color-semantic-blue` `#5B9BD5` | `--color-semantic-blue-bg` `rgba(91,155,213,0.12)` | `--color-semantic-blue-bg-light` `rgba(91,155,213,0.06)` | `--color-semantic-blue-border` `rgba(91,155,213,0.2)` |
| 辅助/紫 | `--color-semantic-purple` `#9B7ED8` | `--color-semantic-purple-bg` `rgba(155,126,216,0.12)` | `--color-semantic-purple-bg-light` `rgba(155,126,216,0.06)` | `--color-semantic-purple-border` `rgba(155,126,216,0.2)` |
| 中性/低 | `--color-semantic-yellow` `#E8C86A` | — | — | — |

**语义色用途说明：**

- `*-bg`（opacity 0.12）：Tag/Badge 背景、进度步骤节点
- `*-bg-light`（opacity 0.04~0.06）：整块区域背景（根因分析块、洞察块、代码执行结果）
- `*-border`（opacity 0.18~0.2）：语义色区块边框

### 2.4 数据可视化色板

用于图表中的多系列区分：

```
图表色序列（最多 8 色，按使用顺序）：
1. #D4A843  — 金色（主系列）
2. #5B9BD5  — 蓝色（对比系列）
3. #34C759  — 绿色（正向指标）
4. #EF4444  — 红色（负向指标）
5. #9B7ED8  — 紫色
6. #F5A623  — 橙色
7. #E8C86A  — 浅金
8. #A8A8A8  — 灰色（基线/参考）
```

### 2.5 文件类型色

用于文件附件图标和标签的背景色：

| 文件类型 | 背景色 | 前景色 |
|---------|--------|--------|
| Excel/CSV | `rgba(52,199,89,0.15)` | `--color-semantic-green` |
| Word/DOC | `rgba(91,155,213,0.15)` | `--color-semantic-blue` |
| PDF | `rgba(239,68,68,0.15)` | `--color-semantic-red` |
| JSON | `rgba(212,168,67,0.15)` | `--color-accent` |
| PNG | `rgba(155,126,216,0.15)` | `--color-semantic-purple` |
| Python | `rgba(245,166,35,0.15)` | `--color-semantic-orange` |
| 其他/灰 | `rgba(168,168,168,0.15)` | `--color-text-muted` |

---

## 三、字体系统

### 3.1 字体栈

```css
--font-sans: -apple-system, BlinkMacSystemFont, "PingFang SC",
             "Microsoft YaHei", "Segoe UI", Roboto, sans-serif;

--font-mono: "SF Mono", "Fira Code", "JetBrains Mono",
             Consolas, "Courier New", monospace;
```

### 3.2 字号梯度

基于 `16px` 基础字号（Apple HIG 标准），使用 `rem` 单位，通过 TailwindCSS 4 `@theme` 注册：

| Token | 大小 | 行高 | 用途 |
|-------|------|------|------|
| `--text-xs` | `0.75rem` (12px) | 1.4 | 最小标注、版本号、Tag、时间戳、Badge |
| `--text-sm` | `0.8125rem` (13px) | 1.5 | 辅助信息、表头、表格数据、描述文字 |
| `--text-base` | `0.875rem` (14px) | 1.6 | 正文、表单输入、卡片标题 |
| `--text-md` | `0.9375rem` (15px) | 1.6 | 对话正文、用户消息、主内容 |
| `--text-lg` | `1.0625rem` (17px) | 1.5 | 页面标题、区域标题 |
| `--text-xl` | `1.25rem` (20px) | 1.4 | 大数字、核心指标 |
| `--text-2xl` | `1.5rem` (24px) | 1.3 | Metric 大数字 |

**使用规范：**

- 所有字号必须使用上述 token 类名（`text-xs`、`text-sm`、`text-base`、`text-md`、`text-lg`、`text-xl`、`text-2xl`）
- 禁止使用 Tailwind 任意值 `text-[0.XXrem]`
- 最大视觉偏移不超过 2px

### 3.3 字重

| 值 | 用途 |
|-----|------|
| `400` (normal) | 正文 |
| `500` (medium) | 按钮文字、Tab 标签、辅助信息 |
| `600` (semibold) | 小标题、表头、强调文字 |
| `700` (bold) | 大标题、核心数据、Logo 文字 |

---

## 四、间距系统

基于 4px 栅格，8 的倍数为主要节奏：

| Token | 值 | 用途 |
|-------|-----|------|
| `--spacing-1` | `4px` | 图标与文字间距、最小间距 |
| `--spacing-2` | `8px` | 元素内紧凑间距 |
| `--spacing-3` | `12px` | 卡片内 padding、列表项间距、**rich-content 块垂直间距** |
| `--spacing-4` | `16px` | 标准 padding、区块间距 |
| `--spacing-5` | `20px` | 模态框 padding |
| `--spacing-6` | `24px` | 主内容区 padding |
| `--spacing-8` | `32px` | 大区域分割 |
| `--spacing-10` | `40px` | 页面级间距 |

**关键约定：**

- 所有 rich-content 组件（代码块、表格、指标卡、洞察块等）外边距统一为 `my-3`（12px）
- 消息气泡间距为 `mb-7`（28px）
- 步骤分割线间距为 `my-7`（28px）

---

## 五、圆角与阴影

### 5.1 圆角

| Token | 值 | 用途 |
|-------|-----|------|
| `--radius-xs` (rounded-xs) | `4px` | 小按钮、内联 Tag |
| `--radius-sm` (rounded-sm) | `6px` | 代码块、输入框 |
| `--radius-md` (rounded-md) | `8px` | 卡片、下拉菜单 |
| `--radius-lg` (rounded-lg) | `12px` | 对话气泡、模态框、大卡片、文件图标 |
| `--radius-xl` (rounded-xl) | `16px` | 浮层、Step Badge |
| `--radius-full` (rounded-full) | `9999px` | 头像、圆形按钮、Pill Tag |

**关键约定：**

- 禁止使用 `rounded-[Xpx]` 任意值
- 文件类型图标统一 `rounded-lg`（12px）

### 5.2 阴影

| Token | 值 | 用途 |
|-------|-----|------|
| `--shadow-sm` | `0 1px 3px rgba(0,0,0,0.06)` | 卡片微阴影 |
| `--shadow-md` | `0 4px 12px rgba(0,0,0,0.08)` | 下拉菜单、Toast |
| `--shadow-lg` | `0 12px 40px rgba(0,0,0,0.12)` | 浮层 |
| `--shadow-accent` | `0 0 0 3px rgba(212,168,67,0.2)` | 聚焦环（Focus Ring） |
| `--shadow-input` | `0 0 0 1px var(--color-border-subtle), 0 2px 8px rgba(0,0,0,0.04)` | 输入栏外框 |
| `--shadow-modal` | `0 20px 60px rgba(0,0,0,0.15)` | 模态框 |

---

## 六、动效规范

### 6.1 过渡

| 场景 | duration | easing |
|------|----------|--------|
| 按钮悬停/状态变化 | `150ms` | `ease` |
| 面板展开/折叠 | `250ms` | `ease-out` |
| 消息出现 | `300ms` | `ease` |
| 模态框出现 | `200ms` | `ease-out` |
| 侧边栏删除动画 | `200ms` | `ease` |

```css
--transition-fast: 150ms ease;
--transition-normal: 250ms ease-out;
--transition-slow: 300ms ease;
```

### 6.2 关键帧动画

```css
/* 消息淡入上滑 */
@keyframes fadeUp {
  from { opacity: 0; transform: translateY(8px); }
  to   { opacity: 1; transform: none; }
}

/* 打字指示器 */
@keyframes blink {
  0%, 80%, 100% { opacity: 0.3; }
  40% { opacity: 1; }
}

/* 进度条脉冲 */
@keyframes pulse {
  0%, 100% { opacity: 1; }
  50% { opacity: 0.6; }
}
```

---

## 七、核心组件规范

### 7.1 按钮

**主按钮（Primary）**

```
背景: var(--color-primary)             #1D1D1F (Carbon Black)
文字: var(--color-text-on-primary)     #FFFFFF
圆角: rounded-sm                       6px
内距: 8px 18px
字号: text-base                        14px
字重: 500
悬停: background → var(--color-primary-hover) #2C2C2E
按下: background → var(--color-primary-active) #000000
```

**次要按钮（Secondary）**

```
背景: var(--color-bg-card)         #FFFFFF
文字: var(--color-text-primary)     #1D1D1F
边框: 1px solid var(--color-border) #E2E0DC
悬停: background → var(--color-bg-card-hover); border-color → var(--color-border-light)
```

**幽灵按钮（Ghost）**

```
背景: transparent
文字: var(--color-text-muted)       #8C8C8C
悬停: text → var(--color-text-secondary); background → rgba(0,0,0,0.03)
```

### 7.2 输入框

**主输入栏（ChatArea 底部）**

```
背景: var(--color-bg-input)         #FFFFFF
圆角: rounded-xl                    12px
阴影: var(--shadow-input)
字号: text-md                       15px
内距: 12px 16px
占位: var(--color-text-muted)
最大宽度: 860px, 居中
```

**表单输入框（Settings 内）**

```
背景: var(--color-bg-main)          #FAFAF8
边框: 1px solid var(--color-border)
圆角: rounded-md                    8px
字号: text-base                     14px
内距: 8px 12px
聚焦: border-color → var(--color-accent); box-shadow → var(--shadow-accent)
```

### 7.3 卡片

```
背景: var(--color-bg-card)          #FFFFFF
边框: 1px solid var(--color-border) #E2E0DC
圆角: rounded-lg                    12px
内距: 14px 16px
悬停: border-color → var(--color-border-light); background → var(--color-bg-card-hover)
选中: border-color → var(--color-primary); background → var(--color-primary-subtle)
选中标记: 右上角 18px Carbon Black 圆形 + 白色勾
```

### 7.4 对话气泡

**AI 消息（左侧）**

```
头像: 28px 圆形, 背景 var(--color-accent), 文字 "家", color var(--color-text-on-accent)
名称: text-sm, font-weight 600, color var(--color-text-primary)
消息体: 无背景, padding-left 36px
文字: text-md, leading-relaxed (1.625), 渲染 markdown
间距: mb-7 (28px)
```

**用户消息（右侧展示）**

```
布局: flex flex-col items-end（右对齐，无头像）
气泡背景: var(--color-bg-msg-user) #F2F1EE
圆角: rounded-xl rounded-br-[4px] (右下角小圆角表示发送方向)
内距: 10px 16px
文字: text-base, leading-relaxed
最大宽度: 88%
间距: mb-7 (28px)
```

**流式消息**

```
同 AI 消息布局
文字: text-md, leading-relaxed
工具执行指示: text-xs, color var(--color-text-muted), 带旋转图标
打字指示器: 3 个圆点，blink 动画
```

### 7.5 数据表格

```
外容器: bg-card + border + rounded-lg, overflow hidden
外边距: my-3 (12px)
标题行:
  文字: text-sm, font-weight 600, color var(--color-text-primary)
  Badge: 右侧可选
  下边框: 1px solid var(--color-border)
  内距: 16px 16px 12px
表头:
  文字: text-xs, font-weight 600, uppercase, tracking-wide
  颜色: var(--color-text-muted)
  内距: 10px 16px
  下边框: 1px solid var(--color-border)
单元格:
  文字: text-sm, color var(--color-text-secondary)
  内距: 8px 16px
  下边框: 1px solid rgba(0,0,0,0.04)
最后一行: 无下边框
```

### 7.6 Tag / Badge

| 类型 | 背景 | 文字色 | 示例 |
|------|------|--------|------|
| 金色/品牌 | `bg-accent-subtle` | `color-accent` | 推荐、已确认 |
| 绿色/正常 | `bg-semantic-green-bg` | `color-semantic-green` | 正常、已完成 |
| 红色/异常 | `bg-semantic-red-bg` | `color-semantic-red` | 严重偏低、高风险 |
| 橙色/警告 | `bg-semantic-orange-bg` | `color-semantic-orange` | 中优先级 |
| 蓝色/信息 | `bg-semantic-blue-bg` | `color-semantic-blue` | 国际、信息 |
| 紫色/搜索 | `bg-semantic-purple-bg` | `color-semantic-purple` | 搜索来源 |
| 灰色/中性 | `rgba(168,168,168,0.12)` | `color-text-muted` | 默认、无标签 |

```
字号: text-xs (12px)
内距: 3px 10px
圆角: rounded-xl (Pill 形)
字重: 500
```

### 7.7 确认区块（Checkpoint）

```
背景: var(--color-accent-bg-light)       rgba(212,168,67,0.04)
边框: 1px solid var(--color-accent-border) rgba(212,168,67,0.25)
左边框: 3px solid var(--color-accent) (强调)
圆角: rounded-lg
内距: 16px
外边距: my-3
标题: text-base, font-weight 600, color var(--color-accent)（纯色区分，无图标）
已确认状态: text-sm, color var(--color-semantic-green)
已拒绝状态: text-sm, color var(--color-semantic-red)
非活跃态: 背景 var(--color-bg-card), 边框 var(--color-border)
```

### 7.8 代码块

```
外容器:
  背景: var(--color-bg-code)          #F5F4F0
  边框: 1px solid var(--color-border)
  圆角: rounded-lg
  外边距: my-3
头部栏:
  背景: rgba(0,0,0,0.02)
  下边框: 1px solid var(--color-border)
  内距: 8px 14px
  左侧: text-xs, font-weight 600, color var(--color-text-muted)
  右侧: text-xs, font-weight 500, 状态色 + 复制按钮
代码区:
  字体: var(--font-mono)
  字号: text-sm (13px)
  行高: 1.65
  内距: 12px 14px
  文字: var(--color-text-code) #383838
执行结果区:
  成功背景: var(--color-semantic-green-bg-light) rgba(52,199,89,0.04)
  错误背景: var(--color-semantic-red-bg-light) rgba(239,68,68,0.04)
  字体: var(--font-mono)
  字号: text-sm
  行高: 1.6
  错误文字: var(--color-semantic-red)
  成功文字: var(--color-text-secondary)
```

### 7.9 文件附件卡

```
布局: inline-flex, align-items center, gap 10px
背景: var(--color-bg-card)
边框: 1px solid var(--color-border)
圆角: rounded-lg
内距: 10px 14px
最大宽度: 360px
外边距: mb-1.5

文件图标 (36x36px, rounded-lg):
  背景色和前景色参见 §2.5 文件类型色
  字号: text-xs, font-weight 700

文件名: text-sm, font-weight 600, 单行溢出省略
元信息: text-xs, color var(--color-text-muted) (大小 · 状态)
```

### 7.10 进度指示器（分析步骤）

```
容器: bg-card + border + rounded-lg, padding 14px
外边距: my-3
标题: text-sm, font-weight 600, color var(--color-accent)（纯色区分，无图标）

步骤节点:
  已完成: background var(--color-accent-subtle), color var(--color-accent), 文字 ✓ 前缀
  进行中: background var(--color-semantic-blue-bg), color var(--color-semantic-blue), 文字 ● 前缀
  待执行: background rgba(168,168,168,0.1), color var(--color-text-muted)
  字号: text-xs, padding 4px 10px, rounded-xl (Pill)
  字重: 500

布局: flex-wrap, gap 6px
```

### 7.11 指标卡（Metric Card）

```
布局: grid, 4 列（≤2 项时 2 列）
外边距: my-3
背景: var(--color-bg-card) + border + rounded-lg
内距: 14px, text-align center

标签: text-xs, color var(--color-text-muted)
数值: text-2xl, font-weight 700
副文字: text-xs, color var(--color-text-muted)

状态色:
  good → 数值色 var(--color-semantic-green)
  warn → 数值色 var(--color-semantic-orange)
  bad  → 数值色 var(--color-semantic-red)
  neutral → 数值色 var(--color-text-primary)
```

### 7.12 根因分析区块

```
背景: var(--color-semantic-red-bg-light)      rgba(239,68,68,0.04)
边框: 1px solid var(--color-semantic-red-border) rgba(239,68,68,0.18)
圆角: rounded-lg
内距: 16px
外边距: my-3

标题: text-base, font-weight 600, color var(--color-semantic-red)（纯色区分，无图标）
根因条目: padding 10px 0, 下划线分隔 rgba(0,0,0,0.06)
计数 Badge: text-xs, font-weight 700, bg rgba(239,68,68,0.12) + var(--color-semantic-red)
标签: text-sm, font-weight 600
详情: text-sm, color var(--color-text-muted), leading-snug
行动建议: text-sm, font-weight 500, color var(--color-accent)
```

### 7.13 洞察区块（Insight）

```
背景: var(--color-semantic-blue-bg-light)      rgba(91,155,213,0.06)
边框: 1px solid var(--color-semantic-blue-border) rgba(91,155,213,0.2)
圆角: rounded-lg
内距: 14px
外边距: my-3

标题: text-base, font-weight 600, color var(--color-semantic-blue)
正文: text-sm, color var(--color-text-secondary), line-height 1.65
```

### 7.14 搜索来源区块

```
背景: var(--color-semantic-purple-bg-light)      rgba(155,126,216,0.06)
边框: 1px solid var(--color-semantic-purple-border) rgba(155,126,216,0.2)
圆角: rounded-lg
内距: 14px
外边距: my-3

标题: text-sm (继承), font-weight 600, color var(--color-semantic-purple)（纯色区分，无图标）
条目: text-sm (继承), color var(--color-text-muted)
来源名: font-weight 500, color var(--color-text-secondary)
提示: text-xs, italic, color var(--color-semantic-orange)
链接: underline, color var(--color-semantic-purple)
```

### 7.15 选项卡（Option Cards）

```
布局: grid, 3 列（可配 2 列）, gap 12px
外边距: my-3

单个选项:
  背景: var(--color-bg-card)
  边框: 1.5px solid var(--color-border)
  圆角: rounded-lg
  内距: 14px
  过渡: 200ms

  标签 Tag: text-xs, font-weight 700, uppercase, tracking-wide, color var(--color-accent)
  标题: text-base, font-weight 600, color var(--color-text-primary)
  描述: text-sm, leading-snug, color var(--color-text-muted)

选中态:
  背景: var(--color-accent-bg-light)
  边框: var(--color-accent)
  右上角: 18px 金色圆形 + 白色勾
```

### 7.16 异常列表（Anomaly List）

```
外容器: bg-card + border + rounded-lg, overflow hidden
外边距: my-3

每项:
  布局: flex, gap 10px, padding 12px 16px
  下边框: 1px solid rgba(0,0,0,0.06)
  优先级圆点: 10x10px rounded-full

  标题: text-sm, font-weight 600
    high: color var(--color-semantic-red)
    medium: color var(--color-semantic-orange)
    low: color var(--color-text-secondary)
  描述: text-sm, leading-snug, color var(--color-text-muted)
```

### 7.17 执行摘要卡（Executive Summary）

```
外容器: bg-card + border + rounded-lg
内距: 16px
外边距: my-3

标题: text-base, font-weight 700, color var(--color-accent)（纯色区分，无图标）
指标格子: grid, 4 列(lg) / 2 列(sm), gap 12px

单个指标:
  背景: var(--color-bg-main)
  边框: 1px solid var(--color-border)
  圆角: rounded-lg
  内距: 12px
  标签: text-xs, uppercase, tracking-wide, color var(--color-text-muted)
  数值: text-lg, font-weight 700
  副文字: text-xs, color var(--color-text-muted)
  数值色:
    danger → var(--color-semantic-red)
    money → var(--color-accent)
    good → var(--color-semantic-green)
    neutral → var(--color-text-primary)
```

### 7.18 报告卡片（Report Cards）

```
布局: grid, 2 列, gap 12px
外边距: my-3

单个报告:
  布局: flex, gap 14px, padding 16px
  背景: var(--color-bg-card)
  边框: 1px solid var(--color-border)
  圆角: rounded-lg
  悬停: border-color → var(--color-accent), background → var(--color-bg-card-hover)

  文件类型标签: 42x42px, rounded-lg, text-xs, font-weight 700
    使用文字标签(HTML/XLS/PDF) + 语义色区分，不使用 emoji
    背景色/前景色参见 §2.5 文件类型色
  标题: text-base, font-weight 600
  描述: text-sm, color var(--color-text-muted)
```

### 7.19 生成文件卡（Generated File Card）

```
布局: flex, gap 14px, padding 14px
背景: var(--color-bg-card)
边框: 1px solid var(--color-border)
圆角: rounded-lg
外边距: my-2
旧版本: opacity 0.6

文件图标: 40x40px, rounded-lg
  字号: text-xs, font-weight 700
  背景色/前景色参见 §2.5 文件类型色

文件名: text-base, font-weight 600, truncate
版本号 Badge: text-xs, font-weight 500, bg rgba(168,168,168,0.12), color var(--color-text-muted)
元信息: text-sm, color var(--color-text-muted)
操作按钮: Ghost Button, text-xs
```

### 7.20 步骤分割线（Step Divider）

```
布局: flex, items-center, gap 12px
外边距: my-7 (28px)

左右线: h-px, flex-1, background var(--color-border)
标签: text-xs, font-weight 700, tracking-wide
  背景: var(--color-bg-card)
  边框: 1px solid var(--color-border)
  圆角: rounded-2xl (Pill)
  内距: 2px 10px
  颜色: var(--color-text-muted)
```

### 7.21 模态框（Modal）

```
遮罩层:
  背景: var(--color-overlay) rgba(0,0,0,0.5)
  定位: fixed inset-0, z-50
  关闭: 点击遮罩关闭

尺寸系统:
  sm: 400px (确认对话框)
  md: 520px (设置弹窗，默认)
  lg: 640px (复杂表单)

弹窗容器:
  布局: flex flex-col（头部/底部固定，内容区弹性填充）
  固定高度: 70vh（切换 Tab 时高度不跳动）
  最大高度: 80vh
  背景: var(--color-bg-card)
  边框: 1px solid var(--color-border)
  圆角: rounded-lg
  阴影: var(--shadow-modal) 0 20px 60px rgba(0,0,0,0.15)
  入场动画: modalIn 0.2s ease-out (scale 0.97 + translateY 4px → none)

头部:
  内距: 14px 20px
  下边框: 1px solid var(--color-border)
  标题: text-lg, font-weight 600
  关闭按钮: 右侧 × 图标, text-lg, color var(--color-text-muted), hover → var(--color-text-secondary)
  收缩: shrink-0（固定不缩放）

内容区:
  内距: 20px
  弹性: flex-1 min-h-0
  滚动: overflow-y auto（仅内容区滚动，头部/底部固定）

底部按钮栏:
  内距: 12px 20px
  上边框: 1px solid var(--color-border)
  布局: flex, justify-end, gap 8px
  收缩: shrink-0（固定不缩放）
```

### 7.22 Toast 通知

```
定位: fixed bottom-4 right-4, z-[9999]
最大宽度: 380px
动画: fadeUp 0.25s ease

单个 Toast:
  背景: var(--color-bg-card) 白色
  边框: 1px solid var(--color-border)
  左边框: 3px solid 语义色 (red/orange/green/blue)
  圆角: rounded-lg
  阴影: shadow-md
  内距: 12px 14px
  间距: mb-2

  图标: 20px 圆形, 白色文字, 语义色背景
  标题: text-sm, font-weight 600
  内容: text-xs, color var(--color-text-secondary)
  关闭: 右上角 × 按钮
  自动消失: 可配置秒数
```

### 7.23 设置表单

```
表单组:
  间距: mb-4.5
  标签: text-sm, font-weight 600, color var(--color-text-secondary)
  描述: text-xs, color var(--color-text-muted), mb-2

下拉框:
  背景: var(--color-bg-main)
  边框: 1px solid var(--color-border)
  圆角: rounded-md
  字号: text-base
  内距: 8px 12px

文本输入:
  同下拉框规格
  密码框: 右侧 显示/隐藏 切换按钮, text-xs

验证状态:
  有效: text-sm, color var(--color-semantic-green)
  无效: text-sm, color var(--color-semantic-red)

复选框标签: text-sm, color var(--color-text-secondary)
```

### 7.24 头像（Avatar）

```
尺寸: 28x28px
圆角: rounded-full
字号: text-sm, font-weight 700

AI 头像:
  背景: var(--color-accent) #D4A843
  文字: var(--color-text-on-accent) #1A1A1A
  内容: "家"

用户头像（仅备用，聊天气泡不再显示）:
  背景: var(--color-user-avatar) #6366f1
  文字: #FFFFFF
  内容: "我" 或自定义
```

### 7.25 空状态

```
Sidebar 无对话:
  文字: text-sm, color var(--color-text-muted)
  对齐: text-center
  内距: px-3 py-8

ChatArea 欢迎消息:
  同 AI 消息布局
  标题文字: text-md, leading-relaxed, color var(--color-text-secondary)
  品牌名强调: font-weight 700, color var(--color-text-primary)
```

---

## 八、布局规范

### 8.1 整体布局

```
┌──────────────────────────────────────────────────────┐
│  body: flex, height 100vh, min-width 1080px          │
├──────────┬───────────────────────────────────────────┤
│ Sidebar  │  Main                                     │
│ 260px    │  flex: 1                                  │
│ fixed    │  ┌─────────────────────────────────────┐  │
│          │  │ Top Bar (52px)                      │  │
│          │  ├─────────────────────────────────────┤  │
│          │  │ Chat Area (flex: 1, overflow-y)     │  │
│          │  │  ┌───────────────────────────┐      │  │
│          │  │  │ Chat Inner               │      │  │
│          │  │  │ max-width: 860px          │      │  │
│          │  │  │ margin: 0 auto            │      │  │
│          │  │  │ padding: 24px 24px 160px  │      │  │
│          │  │  └───────────────────────────┘      │  │
│          │  ├─────────────────────────────────────┤  │
│          │  │ Input Bar (absolute bottom)         │  │
│          │  │ gradient background                 │  │
│          │  │ max-width: 860px, centered          │  │
│          │  └─────────────────────────────────────┘  │
├──────────┴───────────────────────────────────────────┤
│ 响应式: @media (max-width: 1200px) sidebar → 240px   │
└──────────────────────────────────────────────────────┘
```

### 8.2 Sidebar 布局

```
宽度: 260px
背景: var(--color-bg-sidebar)
右边框: 1px solid var(--color-border)
overflow-x: hidden（禁止水平滚动）

结构:
  Header (logo + tagline + 新建按钮) — px-4 pt-4 pb-3
  Nav (聊天记录列表, flex:1, overflow-y auto, overflow-x hidden) — p-2
  Footer (版本号 + 设置) — px-4 py-3

对话项:
  布局: flex, items-center, rounded-md
  活跃态: 背景 var(--color-bg-sidebar-hover), 左侧 3px accent 竖线
  悬停态: 背景 var(--color-bg-sidebar-hover)
  文字: text-sm, truncate
    活跃: font-weight 500, color var(--color-text-primary)
    非活跃: font-weight 400, color var(--color-text-secondary)
  删除按钮: hover 时显示, opacity 过渡
```

### 8.3 对话消息间距

```
消息间距: mb-7 (28px)
消息头与内容间距: mb-2 (8px)
Rich-content 块垂直间距: my-3 (12px) — 统一
步骤分割线间距: my-7 (28px)
按钮组: gap 8px, margin 16px 0
```

### 8.4 Flex 对齐规范

**规则 1：图标 + 文字 → `items-center`**
```
所有「图标 + 文字标签」组合必须 flex + items-center。
图标和文字在交叉轴垂直居中。
示例: 按钮内图标+文字, 标题栏图标+标题, Badge 内圆点+文字
```

**规则 2：按钮组 → `items-center`**
```
所有水平排列的按钮组必须 flex + items-center。
包括: Modal footer, ConfirmBlock 操作栏, 表单行内按钮, 验证按钮+状态文字。
```

**规则 3：表单行（输入框 + 按钮并排）→ `items-center`**
```
输入框与旁边的操作按钮必须 flex + items-center。
两者高度应一致（统一 h-9），确保垂直居中。
示例: 工作目录输入框 + 选择目录按钮
```

**规则 4：可变高度容器（textarea）→ `items-end`**
```
当容器内有可变高度元素（如 textarea 多行输入）时，使用 items-end。
固定高度按钮始终对齐容器底部。
textarea 的 minHeight 必须等于相邻按钮高度（h-8 = 32px），
确保单行时与按钮等高，视觉居中。
```

**规则 5：多行文本内容 → `items-start`**
```
当子元素包含可能换行的段落文本时，使用 items-start。
图标/标记与首行文字顶部对齐。
示例: Toast 通知, 异常列表项
```

**禁止事项：**
```
- 禁止水平 flex 容器省略 items-* 对齐类
- 禁止同一行内子元素高度不一致且无 items-center 补偿
- 禁止按钮组使用 items-end / items-start（必须 items-center）
```

---

## 九、图标规范

### 9.1 图标使用原则

**内容区域（聊天消息 / rich-content 卡片）图标极简化：**

- 卡片标题**不使用装饰性 SVG 图标**，通过**语义颜色 + 背景色 + 边框色**区分类型
- 各卡片已有明确的颜色语义：红色=根因、蓝色=洞察、紫色=搜索来源、金色=确认/进度/摘要
- 步骤完成状态使用文字符号（`✓` `●`）而非 SVG 图标
- 报告/文件类型使用文字标签（HTML/XLS/PDF/CSV）+ 语义色，不使用 emoji
- SVG 图标仅限于**操作区域**（Sidebar 导航、TopBar 按钮、InputBar 按钮、Toast 关闭）

### 9.2 操作区域图标尺寸

| 场景 | 尺寸 |
|------|------|
| Sidebar Logo | 24×24 |
| 聊天项图标 | 18×18 |
| 按钮内图标 | 16×16 |
| 删除按钮 | 14×14 |

### 9.3 图标风格

- 使用 **内联 SVG**，fill 跟随 `currentColor`
- 风格：Material Design 填充型（Filled）
- 颜色跟随父级文字色或语义色
- 所有 SVG 内联，不依赖外部图标库

---

## 十、滚动条

```css
/* Webkit */
::-webkit-scrollbar { width: 6px; }
::-webkit-scrollbar-track { background: transparent; }
::-webkit-scrollbar-thumb { background: var(--color-border); border-radius: 3px; }
::-webkit-scrollbar-thumb:hover { background: var(--color-border-light); }

/* Firefox */
* {
  scrollbar-width: thin;
  scrollbar-color: var(--color-border) transparent;
}
```

---

## 十一、无障碍 / 可访问性

- 所有交互元素支持键盘聚焦，使用 `var(--shadow-accent)` 金色聚焦环
- 文字对比度符合 WCAG AA 标准（正文 ≥ 4.5:1，大文字 ≥ 3:1）
- 语义色不依赖纯色彩区分，搭配图标/文字标签
- 按钮和链接有明确的 hover/focus/active 三态

---

## 十二、Logo 使用规范

- **主 Logo 文件**：`renlijia.png`（项目根目录），金色渐变 A 形
- **产品图标**：所有需要应用图标的场景统一使用 `renlijia.png`
- **最小尺寸**：24×24px（Sidebar），不低于 16×16px
- **安全区域**：Logo 周围留白不小于 Logo 高度的 25%
- **禁止**：拉伸变形、旋转、加描边、改变渐变方向

---

## 十三、开发约束（AI 必须遵守）

1. **所有字号必须使用 token 类名**（`text-xs` ~ `text-2xl`），禁止 `text-[X.XXrem]`
2. **所有语义色必须使用 CSS 变量**，禁止直接写 `rgba(R,G,B,A)` 语义色值
3. **所有圆角必须使用 token**（`rounded-xs` ~ `rounded-full`），禁止 `rounded-[Xpx]`
4. **所有 rich-content 组件外边距统一 `my-3`**
5. **模态框/弹窗必须使用 `--color-overlay`、`--shadow-modal`**
6. **新增颜色必须先在 globals.css `@theme` 注册为 token**，再在组件中引用
7. **所有水平 flex 容器必须声明 `items-*` 对齐类**，严格遵守 §8.4 对齐规范（图标+文字→center，按钮组→center，表单行→center，可变高度→end，多行文本→start）
