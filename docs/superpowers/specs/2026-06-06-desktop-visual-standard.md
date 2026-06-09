# AIjia 桌面端全局视觉标准

> 日期: 2026-06-06  
> 范围: AIjia 桌面端全局 UI、数字员工、专家团、技能中心、聊天、设置、通道、任务、弹窗与通知  
> 状态: 视觉标准草案，配套 demo: `docs/superpowers/prototypes/2026-06-06-desktop-visual-standard-demo.html`

## 1. 设计方向

AIjia 桌面端的视觉方向统一为: **安静、密集、可信的 AI 工作台**。

它不是营销页，也不是强装饰 SaaS 首页。主界面应像现代 agent coding / AI ops 工具: 信息密度稳定、状态可追踪、操作位置可预测、视觉噪声低。竞品截图中值得吸收的是统一的窗口密度、卡片尺寸、字号层级、横向分类导航和弱装饰卡片；Codex 类产品值得吸收的是后台任务、工具执行、代码/文件/日志可追踪的工作台感。

### 核心原则

1. **低噪声**: 不用大面积品牌色、渐变、装饰背景、重阴影。
2. **高一致性**: 同类型元素在全局只有一套尺寸、圆角、阴影和字号。
3. **桌面优先**: 默认服务 1280px 以上窗口，小窗口只保证不破版。
4. **工作台密度**: 每屏展示更多可用内容，减少营销式留白。
5. **状态明确**: AI 执行、同步、错误、权限、等待用户输入都要有稳定位置。
6. **品牌克制**: 金色只作为关键动作和焦点色，不承担大面积背景。

## 2. 现状问题

当前代码里存在多套视觉实现并行:

- `src/components/ui/button.tsx` 和 `src/components/common/Button.tsx` 两套按钮。
- `src/components/ui/dialog.tsx` 和 `src/components/common/Modal.tsx` 两套弹窗。
- `Card` 默认阴影偏重，列表/市场页显得浮。
- `PageTopBar` 注释为 56px，实际实现为 40px，页面头部节奏不稳定。
- 页面宽度以 1032px 为主，对资源市场类页面偏窄。
- 页面层经常自行写圆角、阴影、padding、按钮高度，导致风格漂移。

本标准的落地目标不是换色，而是把这些重复和漂移收敛到一套产品级规则。

## 3. 全局 Token

### 3.1 颜色

| Token | 建议值 | 用途 |
|---|---:|---|
| `--background` | `#FAFAF8` | 主工作区背景 |
| `--foreground` | `#111111` | 主文字 |
| `--muted` | `#F3F3F1` | 弱背景、hover 底 |
| `--muted-foreground` | `#71717A` | 辅助文字 |
| `--card` | `#FFFFFF` | 卡片、弹窗、toast |
| `--border` | `#E6E6E3` | 默认边框 |
| `--input` | `#E1E1DE` | 输入框边框 |
| `--sidebar` | `#F4F4F1` | 侧栏背景 |
| `--sidebar-accent` | `#E9E9E5` | 侧栏选中/hover |
| `--primary` | `#D4A843` | 品牌金、主 CTA、focus |
| `--brand-primary-subtle` | `#F8F1DF` | 选中 tab、弱品牌底 |
| `--destructive` | `#D92D20` | 错误/危险 |
| `--success` | `#16A34A` | 成功 |
| `--warning` | `#D97706` | 警告 |
| `--info` | `#2563EB` | 信息 |

规则:

- 主背景、侧栏、卡片三者必须有轻微层级差。
- 品牌金不用于 sidebar 大面积背景，也不用于普通 hover。
- 语义色只表达状态，不做装饰。
- 禁止页面自行新增随机色值。新增色必须先进入 token。

### 3.2 字体

字体栈沿用现有:

```css
-apple-system, BlinkMacSystemFont, "PingFang SC", "Microsoft YaHei", Inter, "Segoe UI", system-ui, sans-serif
```

字号层级:

| 层级 | 字号 / 行高 / 字重 | 用途 |
|---|---|---|
| Display | 24 / 32 / 700 | 少量首屏标题 |
| Page Title | 22 / 28 / 700 | 页面标题 |
| Section Title | 18 / 24 / 700 | 分区标题 |
| Card Title | 15 / 22 / 650 | 卡片标题 |
| Body | 14 / 22 / 400 | 正文 |
| Dense Body | 13 / 20 / 400 | 卡片描述、表格 |
| Meta | 12 / 18 / 500 | 辅助信息、标签 |
| Caption | 11 / 16 / 500 | 极少量计数/状态 |

规则:

- 卡片内标题不超过 16px。
- 弹窗标题默认 18px，复杂详情弹窗可 20px。
- 不使用负 letter-spacing。
- 不用 viewport width 动态缩放字号。

### 3.3 间距

采用 4px 基线。

| Token | 值 | 用途 |
|---|---:|---|
| `space-1` | 4 | 极小间隔 |
| `space-2` | 8 | icon 与文字、tag gap |
| `space-3` | 12 | 紧凑控件内边距 |
| `space-4` | 16 | 卡片基础 padding |
| `space-5` | 20 | 卡片宽松 padding |
| `space-6` | 24 | 弹窗、section 间距 |
| `space-8` | 32 | 页面 padding |

页面标准:

- 普通页面: `px-32 pt-28 pb-40`。
- 市场/目录页面: `px-32 pt-28 pb-40`, max-width 1280。
- 聊天页面: 自适应全宽，消息区使用单独宽度规则。
- 设置弹窗: 980 x 680/760，内部 padding 24。

### 3.4 圆角

| Token | 值 | 用途 |
|---|---:|---|
| `radius-sm` | 4 | 小图标按钮、代码片段 |
| `radius` | 6 | 输入框、按钮 |
| `radius-md` | 8 | 资源卡、普通卡 |
| `radius-lg` | 10 | 弹窗内 section |
| `radius-xl` | 12 | 弹窗外壳、复杂面板 |
| `radius-pill` | 999 | tab、tag、主 CTA |

规则:

- 卡片默认 8px，不再使用 12px 以上大圆角。
- 大圆角只给弹窗、登录卡、主 CTA、pill。

### 3.5 阴影

| Token | 值 | 用途 |
|---|---|---|
| `--shadow-card` | `0 1px 2px rgba(0,0,0,.04)` | 默认卡片 |
| `--shadow-card-hover` | `0 6px 18px rgba(0,0,0,.07)` | 卡片 hover |
| `--shadow-popover` | `0 8px 24px rgba(0,0,0,.10)` | dropdown/popover |
| `--shadow-modal` | `0 20px 60px rgba(0,0,0,.16)` | modal |

规则:

- 普通卡片以 border 分层，阴影只做轻微深度。
- 页面 section 不使用阴影。
- 禁止在页面层直接写 `shadow-lg/xl/2xl`。

### 3.6 动效

| Token | 值 | 用途 |
|---|---|---|
| `--transition-fast` | `120ms ease-out` | hover、active |
| `--transition-normal` | `180ms ease-out` | tab、popover |
| `--transition-slow` | `240ms ease-out` | modal、sheet |

规则:

- 动效只服务反馈，不做装饰性动画。
- loading spinner 使用已有 lucide `RefreshCw/Loader2`。
- 大面积 layout transition 禁止默认启用。

## 4. 全局布局

### 4.1 应用窗口

- 默认设计基准: 1440 x 900。
- 最小可用宽度: 1024。
- Sidebar: 256px 固定。
- 主工作区: `min-width: 0`, 背景 `--background`。
- 标题栏: 保持 Tauri drag region，视觉弱化。

### 4.2 Sidebar

尺寸:

- 宽度 256px。
- 顶部 padding 8px。
- 一级菜单行高 32px。
- icon 16px，文字 14px。
- 会话行高 30-34px。

状态:

- 默认: 透明/无边框，文字 `sidebar-foreground/80`。
- Hover: `--sidebar-accent` 40-60%。
- Active: `--sidebar-accent` 实底，文字 `--foreground`。
- 禁止使用品牌金作为整行 active 背景。

内容结构:

1. 租户/产品头。
2. 一级入口: 新任务、数字员工、专家团、技能、定时任务、IM。
3. 二级 tab: 项目、员工、专家团、通道。
4. 会话/资源列表。
5. 设置入口。

### 4.3 Page Top Bar

统一高度: 48px。若页面需要更强工具栏，可扩展为 56px，但同一路由域内必须一致。

变体:

- `default`: 空白拖拽栏。
- `title`: 左标题，右工具。
- `breadcrumb`: 返回/面包屑。
- `compact`: 聊天/详情使用。

右侧工具顺序:

1. 搜索。
2. 筛选/排序。
3. 刷新/更新。
4. 主动作。

### 4.4 页面宽度

| 页面类型 | max-width | 示例 |
|---|---:|---|
| 目录/市场 | 1280 | 数字员工、专家团、技能中心 |
| 普通设置/详情 | 1032 | 技能详情、任务详情 |
| 表单配置 | 960 | 通道配置、员工配置 |
| 聊天 | 全宽 | ChatPage |

### 4.5 网格

资源市场:

- >= 1360 窗口: 4 列。
- 1100-1359: 3 列。
- < 1100: 2 列。
- 卡片 gap 12-16px。

卡片高度:

- 资源卡: 168-190px，按内容类型固定。
- 复杂卡: 220px 上限。

## 5. 基础组件标准

### 5.1 Button

统一使用 `src/components/ui/button.tsx`，逐步废弃 `src/components/common/Button.tsx`。

尺寸:

| Size | 高度 | padding | 字号 | 用途 |
|---|---:|---|---:|---|
| `xs` | 24 | 8 | 12 | 紧凑 icon/text |
| `sm` | 28 | 10 | 12 | 工具栏 |
| `md` | 36 | 14 | 13 | 常规 |
| `lg` | 40 | 18 | 14 | 主 CTA |

变体:

- `primary`: 当前区域唯一主动作。
- `secondary`: 普通次动作。
- `outline`: 工具栏、刷新、筛选。
- `ghost`: icon、关闭、返回。
- `destructive`: 删除、退出、危险确认。
- `link`: 文本链接。

规则:

- 图标按钮使用熟悉图标，不写“关闭/返回/刷新”的长按钮，除非需要明确主动作。
- 主按钮每个独立区域最多一个。
- 文案使用用户语言: “更新内容”，不要“同步服务端”。

### 5.2 Icon Button

- 默认 28 x 28。
- 常规 32 x 32。
- 图标 16px。
- hover 用 `--muted`。
- 必须有 `aria-label` 和 tooltip。

### 5.3 Input / Search / Textarea

Input:

- 高度 36px。
- radius 6px。
- border `--input`。
- focus: border `--primary`, ring `primary/20`。

Search:

- 高度 36px。
- 宽度 240-320px。
- 左侧 search icon 16px。
- placeholder 13px。

Textarea:

- 最小高度 84px。
- 行高 21px。
- 禁止在普通表单里超过 5 行默认高度。

### 5.4 Tabs / Category Bar

用于技能/数字员工/专家团分类。

- 高度 32px。
- gap 8px。
- 选中态: `--muted` 或 `--brand-primary-subtle`，文字 `--foreground`。
- 未选中: 透明，文字 `--muted-foreground`。
- 不使用强边框。
- 一行放不下时横向滚动，不换成多行大面积按钮墙。

### 5.5 Badge / Tag

- 高度 22px。
- padding x 8px。
- 字号 12px。
- 默认灰底。
- 语义 badge 才允许使用语义色。
- 一张卡最多外显 3 个 tag，其余显示 `+N`。

### 5.6 Card

普通卡:

- radius 8px。
- border 1px。
- padding 16px。
- default shadow: none 或 `--shadow-card`。
- hover: border 加深 + `--shadow-card-hover`。

资源卡结构:

1. 头像/图标 48 x 48。
2. 标题 + tag 行。
3. 描述 2 行。
4. 来源/成员/技能摘要。
5. 使用量/状态右下。

规则:

- 数字员工、专家团、技能中心统一使用资源卡规范。
- 卡片内部不放大按钮，整卡可点，主动作进入详情或在详情底部执行。

### 5.7 Table

- 字号 13px。
- header 高 36px，灰底。
- row 高 38-42px。
- hover 极弱灰。
- 状态 chip 高 22px。
- 表格工具栏在上方右侧。

### 5.8 Dialog / Modal

统一使用 `src/components/ui/dialog.tsx`，逐步废弃 `src/components/common/Modal.tsx`。

尺寸:

| 类型 | 宽度 | 高度 |
|---|---:|---|
| Confirm | 400-440 | 内容自适应 |
| Form | 560-720 | max 80vh |
| Detail | 720-840 | max 86vh |
| Settings | 980 | 680/760 |

结构:

1. Header: 标题/说明/关闭。
2. Body: 可滚动。
3. Footer: 固定底部，右侧主操作。

规则:

- 关闭按钮永远右上。
- 主操作永远底部右侧。
- 不在 header 右侧放大 CTA，避免与关闭按钮重合。

### 5.9 Popover / Dropdown

- radius 8px。
- shadow `--shadow-popover`。
- item 高度 32px。
- item 字号 13px。
- destructive item 使用红色文字，hover 浅红底。

### 5.10 Toast / Notification

Toast:

- 位置右下。
- 宽度 360-400px。
- radius 8px。
- 左侧 3px 语义色条。
- 成功 4s 自动消失，错误 6-8s。

Inline alert:

- 表单和页面内错误必须 inline 展示。
- toast 只做提醒，不承担唯一错误解释。

### 5.11 Empty / Loading

Empty:

- 小 icon 32px。
- 一句话 + 一个动作。
- 不使用大插画。

Loading:

- 列表用 skeleton，尺寸等同最终卡片。
- 页面级 loading 只用于启动/登录恢复。

## 6. 业务域视觉标准

### 6.1 数字员工

页面类型: 目录/市场页。

布局:

- max-width 1280。
- 顶部: 标题、搜索、排序、更新内容。
- 分类: 横向 tab。
- 主体: 3-4 列资源卡。

卡片:

- 真实姓名头像。
- 标题为姓名，副标题为角色。
- 展示 2-3 个能力 tag。
- 描述最多 2 行。
- 点击卡片打开详情。

详情:

- header: 头像、姓名、角色、描述。
- body: 能力侧重、适合任务、技能、触发方式。
- footer: “召唤/派活/进入会话”。

### 6.2 专家团

页面类型: 目录/市场页。

布局与数字员工一致。

卡片:

- 团队 logo/专家头像组。
- 标题为专家团名。
- 标签为行业/能力方向。
- 描述最多 2 行。
- 底部展示成员头像组和使用量/来源。

详情:

- header: 团队 logo、名称、tagline。
- body: 成员、协作方式、适合议题。
- footer: “召唤专家团”。

### 6.3 技能中心

与数字员工/专家团使用同一资源市场框架。

差异:

- 技能卡以 icon 为主。
- 强调触发词、能力范围、安装/启用状态。
- 导入按钮作为右上主动作。

### 6.4 聊天

目标: Codex-style 工作台，强调执行过程可追踪。

结构:

- 顶部: 会话标题、workspace、分享/更多。
- 中部: 消息流。
- 右侧可选: 任务/专家团/工具详情 drawer。
- 底部: composer。

消息:

- 用户气泡右侧，最大宽度 80%。
- AI 内容左侧，正文宽度 760-860px。
- 工具执行默认折叠摘要，可展开 I/O。
- 文件结果用 compact file card。

状态:

- streaming indicator 稳定放在消息尾。
- 工具执行显示“读取/运行/生成”的聚合摘要。
- 权限/用户输入请求使用 inline blocking card。

### 6.5 设置

设置是 modal app。

- 宽 980px。
- 左侧 menu 220px。
- 右侧内容区。
- menu item 高 36px。
- section card radius 8px，轻 border。
- 保存/退出等操作固定在对应 section，不漂浮。

### 6.6 通道 / IM

通道配置偏 operational。

- 使用表单 card，不使用营销式 hero。
- 连接状态用状态 chip。
- QR/注册码区域固定尺寸，避免加载后跳动。
- 危险提示用 inline alert + confirm dialog。

### 6.7 定时任务 / 日程

- 列表优先表格/紧凑卡片。
- 状态 chip: 启用、暂停、失败。
- 操作按钮使用 icon button + tooltip。
- 新建任务可用 modal/drawer，非独立大页面。

## 7. 文案标准

技术词替换:

| 避免 | 推荐 |
|---|---|
| 同步服务端 | 更新内容 |
| 模板缓存 | 内容 |
| IPC 失败 | 操作失败 |
| registry | 技能列表 |
| manifest | 资源文件 |
| tenant | 企业 |

按钮文案:

- 动词优先: 更新、打开、导入、删除、派活。
- 主动作不超过 6 个汉字。
- loading 态用“更新中.../处理中.../启动中...”。

错误:

- 第一行说结果: “更新失败”。
- 第二行说原因和下一步: “网络恢复后再试”。

## 8. 代码落地规则

### 8.1 组件收敛

优先迁移:

1. `components/common/Button` -> `components/ui/button`。
2. `components/common/Modal` -> `components/ui/dialog`。
3. 页面自写卡片 -> `ResourceCard` / `Card`。
4. 页面自写 tab -> `SkillCategoryBar` 升级为通用 `CategoryBar`。

### 8.2 页面层限制

页面层允许:

- 布局 grid/flex。
- 数据映射。
- 业务状态。

页面层不允许:

- 自定义阴影。
- 自定义色值。
- 大段按钮/弹窗原子 DOM。
- 对同类元素写不同高度和圆角。

### 8.3 新增组件要求

每个视觉组件顶部写:

```ts
/**
 * @visualStandard 2026-06-06 Desktop Visual Standard
 * @sizing ...
 */
```

组件 props 只暴露语义，不暴露颜色/阴影/尺寸，除非确实是布局组件。

## 9. 迁移计划

### P0: Token 和基础组件

- 降低 `--shadow-card`。
- 补齐 `xs` button。
- 统一 Button/Modal 使用路径。
- PageTopBar 高度统一为 48px。

### P1: Shell

- Sidebar 行高、选中态、tab 密度统一。
- PageSectionShell 增加 `variant: market | default | form | chat`。
- Toast 样式收敛。

### P2: 资源市场页

- 数字员工、专家团、技能中心统一布局。
- 搜索、分类、排序统一。
- 资源卡统一。

### P3: Chat Workbench

- 工具执行块、文件卡、权限卡、composer 密度统一。
- 右侧 drawer 标准化。

### P4: 设置/通道/任务

- Settings modal 统一。
- 通道配置表单统一。
- 任务列表和表格统一。

## 10. 验收清单

- 同一页面内按钮高度不超过两种。
- 同一资源市场内卡片高度一致。
- 页面层无自定义色值和阴影。
- 弹窗主操作在底部，关闭在右上。
- Toast 不作为唯一错误提示。
- 数字员工、专家团、技能中心使用同一套分类导航。
- 1280px、1440px、1728px 宽度下布局稳定。
- 长中文、长英文、长数字不会挤爆按钮或卡片。
- 所有 icon-only 操作有 tooltip 和 aria-label。

