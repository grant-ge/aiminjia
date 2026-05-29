# 前端视觉重构对齐 `design.pen` — 设计规格（v2）

> 本文是 2026-04-23 视觉重构专项的正式 spec。v1 在方向上立住了「组件优先 + 设计稿唯一真源」，但在 token 映射、组件 props 与状态机、像素约束、验收清单上不够硬，导致页面实现与设计稿之间仍有"抓把柄"空间。v2 重写这些部分。

- 设计稿位置：`/Users/oayzz/project/lotus/lotus-workbench/lotus-app/design.pen`
- 导出基线 PNG：`docs/superpowers/specs/assets/design-pen-exports/`（阶段 A-0 产出，入库）
- 本次代码分支：`pzc`

---

## 1. 背景与目标

当前前端 Skill-First 改造已经建立了信息架构与功能骨架，但视觉还原度与 `design.pen` 存在明显差距：

- 页面直接堆 shadcn 原子组件，页面层自行承担视觉责任；
- Sidebar / TopBar / 首页任务卡 / 技能卡 / 设置弹窗 / 聊天输入区仍是"功能型骨架"，不是设计稿里的成套组合组件；
- 聊天页缺少设计稿的 ToolGroup 聚合卡、GeneratedFileCard、紧凑型输入区；
- 暖金色品牌 token 已接入，但组件仍停留在"替换 token 的通用 shadcn"，层次、留白、字重、边框节奏没到稿子级别。

本轮目标：以 `design.pen` 为唯一视觉真源，把前端从"页面堆原子组件"重构为"组合组件先行、页面只做拼装"的产品级实现，并**允许为承接设计稿结构做受控的交互层改造**（范围见第 6 章），其余交互/路由/事件流保持现状。

---

## 2. 设计稿盘点结论

### 2.1 顶层分区（4 个）

| 分区 | node-id | 内容 |
|---|---|---|
| [分区] 组件区 | `696e3` | 3 个子分区：布局组件 / 原子组件 / 对话场景组件 |
| [分区] 聊天区 | `H聊天` | 内含首页/技能/任务/设置/认证/聊天 6 个页面簇 |
| [分区] 首页区 | `80303` | 首页 · 新任务 |
| 参考图 | `PctBO` | 金底 logo 底图 |

### 2.2 页面清单（本轮要 1:1 还原的 10 张）

| 页面 | node-id | 设计尺寸 | 主区宽 |
|---|---|---|---|
| 首页 · 新任务 | `2cYHh` | 1280 × 820 | 1032（侧栏外） |
| 聊天 · 长对话拼装 | `ju2pU` | 1280 × 1244 | 1032 |
| 聊天 · 技能弹层 | `9qve3` | 1280 × 900 | 1032 |
| 技能中心 | `dVE8r` | 1280 × 900 | 1032 |
| 技能详情 | `cSdAy` | 1280 × 1180 | 1032 |
| 定时任务 | `s8Rc7` | 1280 × 900 | 1032 |
| 设置 · 账户 Modal | `S3D6p` | 1280 × 900，Modal 980×680 | — |
| 设置 · 关于 AI 小家 | `1MCFZ` | 1280 × 900，Modal 980×760 | — |
| 设置 · 用量 | `az6ZY` | 1280 × 900，Modal 980×760 | — |
| 登录 | `epkyz` | 1280 × 820，Card 460 宽 | — |

### 2.3 107 个 reusable 组件分类

- **布局**（7）：`PV1ln Sidebar`、`qLmzZ ChatTopBar`、`BixkY/aAO2u/tCYsE/WgoHO PageTopBar 4 variants`
- **对话场景**（12）：`uq6ga ChatComposerCompact`、`Cbtm1 ChatBottomArea`、`yNouu/ECmej toolGroup 2 态`、`Q52BT/KW8fZ toolGroupBar`、`v46uG GeneratedFileCard`、`kFmPc SuggestChipGroup`、`1JNrw bubble/adaptive-max-80`、`oYVXX/nVSBv/EAVW9/91cWy/gpR09 TypingIndicator 5 态`（Default/分析/检索/生成/整理）、`zMRnf popover`
- **原子**（88）：Button(5×2) / IconButton(5×2) / Avatar(2) / Badge(4) / Accordion(2) / Alert(2) / Input/Textarea/Select/Combobox/InputOTP 的 Default+Filled / Switch(2) / Radio(2) / Checkbox(2) / Progress / Breadcrumb(4) / Tabs / Dialog / Dropdown(含 List 系列 6 子件) / Modal(Left/Center/Icon) / Pagination(4) / Card(Plain/Action/Image) / Table+DataTable(5) / Tooltip
- **侧栏原子**：`qCCo8/jBcUh Sidebar Item Active/Default`、`24cM4 Sidebar Section Title`

---

## 3. Token 映射与皮肤策略（硬约束）

### 3.1 只做 Light + Neutral + Default

- **不维护** 暗色模式、不维护 Accent 多色切换、不维护 Base 多灰系。
- `branding` store 清掉 `theme`/`accent`/`base` 相关运行时字段；
- 所有 CSS 变量值在运行时**由租户接口下发单版**（即现有 branding 接口只下发一套 Light + Neutral + Default），不再有切换逻辑。
- Tailwind / CSS 层移除暗色 class 与 `.dark` 样式分支（可保留文件结构，但样式分支不再维护）。
- 这里的 **Neutral** 只表示中性色轴（背景、正文、边框、muted 等），不表示品牌主色使用 shadcn 默认黑色；`--primary` / `--ring` / `--sidebar-primary` 必须继续使用品牌金 `#DBAA22`。

### 3.2 Token 映射表

| CSS 变量 | 稿面值（Light+Neutral+Default） | 语义 | 前端使用位置 |
|---|---|---|---|
| `--background` | `#fafafa` | 页面底 | body、页面容器 |
| `--foreground` | `#0a0a0a` | 正文色 | 标题、主正文 |
| `--card` | `#fafafa` | 卡片底（与页面同色，靠 border+阴影分层） | 卡片、Alert、toolGroup 壳 |
| `--card-foreground` | `#0a0a0a` | 卡片文字 | 卡片标题正文 |
| `--border` | `#e5e5e5` | 通用边框 | 所有卡片 / 输入 / 底部 tips 分隔 |
| `--input` | `#e5e5e5` | 输入轮廓 | Input / Textarea 边框 |
| `--muted` | `#f5f5f5` | 弱背景 | 工具步骤行展开 detail、secondary hover |
| `--muted-foreground` | `#737373` | 辅助文字 | 副标 / meta / tips / 非选中侧栏行 |
| `--popover` | `#fafafa` | 浮层底 | Dropdown / Tooltip / 技能弹层 |
| `--secondary` | `#f5f5f5` | 次级底 | Tabs 外壳、设置 Modal 左侧 menu 底 |
| `--secondary-foreground` | `#0a0a0a` | 次级文字 | — |
| `--primary` | `#DBAA22` | 品牌金 | CTA 主按钮、用户气泡底、进度金高亮 |
| `--primary-foreground` | `#FFFFFF` | 品牌前景 | 金按钮文字、金气泡文字 |
| `--brand-primary-subtle` | `#FBF3DC` | 品牌弱底 | 推荐 chip、技能详情 hero icon 框、处理中步骤背景 |
| `--brand-secondary` | `#3F3F46` | 次品牌 | 登录"忘记密码"链接色 |
| `--brand-secondary-subtle` | `#F3F4F6` | 次品牌弱底 | 首页 statusList pr2Wrap 底 |
| `--ring` | `#DBAA22` | 焦点环 | 输入 focus ring、按钮 focus ring |
| `--destructive` | `#e7000b` | 危险色 | 错误 Alert、Destructive 按钮 |
| `--accent` | `#f5f5f5` | shadcn accent（与 secondary 同值） | 图片占位 / hover 背景 |
| `--accent-foreground` | `#0a0a0a` | — | — |
| `--sidebar` | `#F4F0E6` | 侧栏底（暖奶） | Sidebar 根 |
| `--sidebar-foreground` | `#0a0a0a` | 侧栏文字 | 侧栏激活项文字 |
| `--sidebar-accent` | `#E1DAC6` | 侧栏 active 底 | active nav / active 会话 |
| `--sidebar-accent-foreground` | `#18181b` | 侧栏 active 文字 | — |
| `--sidebar-border` | `#E1DAC6` | 侧栏分隔 | Sidebar 右侧 border |
| `--sidebar-primary` | `#DBAA22` | 侧栏品牌 | 品牌图标 / 激活 nav icon |
| `--sidebar-primary-foreground` | `#FFFFFF` | 侧栏品牌前景 | — |

附加固定色（不走 token，设计稿里直接用字面值的语义色）：

| 字面值 | 语义 | 使用点 |
|---|---|---|
| `#DCFCE7` | 成功浅底 | ToolGroup 完成态左上角 iconBox、status pill 底 |
| `#16A34A` | 成功前景 | status pill 文字与 check icon |
| `#107C41` | Excel 绿 | GeneratedFileCard 的 Microsoft Excel 标识 |
| `#0000004d` | Modal 遮罩 | 设置 Modal、Dialog 背后的遮罩层 |
| `#D4D4D8` | 未激活发送按钮底 | ChatComposerCompact 的 disable 发送按钮 |

### 3.3 阴影三档规则（全站仅此三档）

| 档位 | 值 | 使用点 |
|---|---|---|
| lvl-1 轻悬浮 | `0 1px 1.75px #0000000d` | Card / Tab Active / Pagination Active / Outline Button |
| lvl-2 浮层 | `0 2px 3.5px -1px #0000000f, 0 4px 5.25px -1px #0000001a` | Tooltip / Dropdown / Modal Left/Center/Icon |
| lvl-3 大浮层 | `0 20px 20px #0000001a, 0 10px 10px #0000000a` | 设置 Modal 整窗 |

**禁止**出现其他阴影值；不允许用 Tailwind `shadow-md/lg/xl` 自行升档。

### 3.4 圆角常量

- `6`：Button / IconButton / Input / Dropdown List Item
- `8`：Card 原子、Table 壳、iconBox
- `10`：首页 chip / 设置 menu 行 / pr wrap icon 框
- `14`：组合卡片（首页 catRow / statusList、设置 section card、聊天 popover）
- `16`：User 气泡（`bubble/adaptive-max-80`）
- `18`：ChatComposerCompact / 登录 Card / 设置 Modal 外壳
- `22`：技能详情 heroIc
- `32`：首页 mascot 圆
- `999 / 9999`：pill / Progress / Switch / Avatar / Modal Icon Wrapper / 发送按钮

---

## 4. 组件分层契约

### 4.1 三层定义

- **原子层** `src/components/ui/*`：shadcn/ui 生成的基础原子（Button/Input/Tabs/...），只允许接收 `className` 与对应 variant props。
- **组合层** `src/components/{shell,sidebar,home,skills,schedules,settings,chat-scene,auth}/*`：本轮新增或重构的组合组件，**承接所有视觉复杂度**。每个组件：
  - 只依赖原子层 + token 变量；
  - 关键视觉常量（宽/高/padding/gap/radius）写死在组件内，不允许页面调；
  - 对外只暴露语义 props（数据 + 事件），不暴露视觉 props。
- **页面层** `src/features/*/*.tsx`：只负责「拉数据 → 拼组合组件 → 绑事件」，文件尽量 < 120 行。

### 4.2 页面层禁止清单（code-review 门）

页面层文件中禁止出现：

- `background-color` / `bg-*` 的颜色工具类（`bg-transparent` 除外）；
- 任何 `shadow-*` 工具类；
- `border-*` 颜色工具类（仅允许 `border` 结构类）；
- 数值 > 8 的 padding 工具类（`p-*/px-*/py-*/pt-*/pb-*/pl-*/pr-*` 中 `-3 ~ -96` 的数值工具类）；
- 自定义 `style={{ ... }}` 设置颜色/背景/阴影/尺寸（仅允许绑定变量值如 `style={{ width: tenantConfigWidth }}`）；
- 复制自 shadcn 原子的大段 DOM（tailwind-merge 出超过 6 个工具类的叠加即视为视觉责任未下沉）。

落地方式：在 `eslint.config.*` 里新增一条 `no-restricted-syntax` + 目录级 overrides，或至少加入 PR review checklist。

### 4.3 组合层职责宣言

每个组合组件**必须**在文件顶部 JSDoc 注明：

```
/**
 * @designSource design.pen#<node-id>
 * @sizing width <fix|fluid>, padding <x,y>, gap <n>
 */
```

便于视觉走查时快速回稿。

---

## 5. 组合组件清单

约定：所有组件位于 `src/components/<bucket>/<Name>.tsx`。「核心 props」只列必填 + 状态枚举；「视觉常量」为写死的设计稿值。

### 5.1 Shell（`src/components/shell/`、`src/components/sidebar/`）

| 组件 | design node | 核心 props | 状态 | 视觉常量 |
|---|---|---|---|---|
| `AppSidebar` | `PV1ln` | `header`/`content`/`footer` 三 slot | collapsed (本轮只实现 expanded 256 态，collapsed 视觉预留不实现) | w 256, p 8, gap 16, 右 border 1px `--sidebar-border` |
| `TenantHeader` | `6xhgh` | `name`, `logoUrl` | — | p 8, radius 6, logo 32×32 radius 10，`chevrons-up-down` icon 16 |
| `SidebarNav` | `47U5w` 内 nav1-3 | `items[] {icon, label, active}` | active/default | 每行 `qCCo8/jBcUh`，padding `6,8`，gap 2 |
| `SidebarSectionTitle` | `24cM4` | `label` | — | padding 8, fontSize 12, color `--muted-foreground` |
| `ProjectAccordion` | proj1/proj2 节点 | `name`, `expanded`, `children` | expanded/collapsed | 展开头 padding `6,8` gap 8，子项 padding `6,8,6,30`（首列缩进 30） |
| `ConversationRow` | conv 节点 | `title`, `active`, `loading`, `onClick` | active/default/loading | 激活行用 `--sidebar-accent` 底，loading 用 `lucide:loader` 替换默认圆点 |
| `SidebarFooterSettings` | 侧栏底 settings | `onClick` | — | padding `6,8` gap 8 |
| `PageTopBar` | `BixkY/aAO2u/tCYsE/WgoHO` | `variant: default \| title \| breadcrumb \| compact`, `left?`, `right?` | 4 variant | h 56，左右 padding 24，底 border 1 |
| `ChatTopBar` | `qLmzZ` | `title`, `workspace`, `onShare`, `onMore`, `onToggleSidebar` | — | h 56，左右 padding 24，左段 gap 12 + `/` 分隔，右段 gap 14 |
| `PageSectionShell` | — | `children`, `size: md \| lg`, `topBar?` | — | content max-width 1032，padding 由页面传入常量 |

### 5.2 Home（`src/components/home/`）

| 组件 | design node | 核心 props | 视觉常量 |
|---|---|---|---|
| `HomeMascotHero` | `PqcAk > canvas > mascot+hello+subHello` | `mascotUrl`, `title`, `subtitle` | mascot 64×64 radius 32，title fontSize 30 / 700，subtitle fontSize 14 `--muted-foreground`，内部 gap 16 |
| `HomeTaskComposerCard` | `MBdnN` 包 `uq6ga` | 透传 Composer props | width 820，Composer 全宽 |
| `HomeCategoryChipRow` | `Mk2H9 catRow` | `items[] {key,label}`, `activeKey` | 壳 padding `8,12` radius 14，active chip 底 `--brand-primary-subtle` / 前景 `--primary` + `sparkles` icon；inactive 壳 padding `8,12` radius 10 fontSize 13 / 500 `--muted-foreground` |
| `HomeStatusList` | `ORsy4` | `items[] {icon, variant: empty\|loading\|success, title, desc}` | 壳 padding 8 gap 8 radius 14；行内左 iconBox 34×34 radius 10，底色按 variant（empty: `--brand-primary-subtle`, loading: `--brand-secondary-subtle`, success: `#DCFCE7`） |
| `HomeSkillCenterPill` | `M2pKg` | `onClick` | pill padding `10,16` radius 999 底 `--secondary`，右 `arrow-right` icon 14 |

页面容器 `canvas` padding `32,40,28,40`，gap 16，所有区块 width 820 居中。

### 5.3 Skills（`src/components/skills/`）

| 组件 | design node | 核心 props | 视觉常量 |
|---|---|---|---|
| `SkillHotSection` | `znwZc` | `items[] SkillCardData` | 标题 fontSize 15/600，grid gap 16 |
| `SkillCategoryBar` | `ueSct` | `items[] {key,label}`, `activeKey` | 行 gap 8 |
| `SkillOfficeSection` | `CoiX7` | `category`, `items[]` | 外 gap 14 |
| `SkillCard` | 技能卡（Card Action 衍生） | `title, desc, iconNode, onRequest` | radius 8，padding 16，border 1 `--border`，阴影 lvl-1 |
| `SkillDetailHero` | `UDRR3` | `iconNode, title, subtitle, primaryCta, secondaryCta` | heroIc 88×88 radius 22 底 `--brand-primary-subtle`；右上按钮组用 `Button/Default`（金色）+ `Button/Outline`（"禁用"） |
| `SkillMetaRow` | `DWw8D` | `items[] {label, value}` | 行内 gap 48 |
| `SkillTryGrid` | `ZQLFS` | `cards[]` | 标题 fontSize 15/600 gap 14 |
| `SkillUsageBlock` | `MTvV8` | `text` | gap 8 |
| `SkillActionBar` | heroAct | `primaryLabel, secondaryLabel, primaryDisabled?` | gap 10 |

### 5.4 Schedules（`src/components/schedules/`）

| 组件 | design node | 核心 props | 视觉常量 |
|---|---|---|---|
| `ScheduleTemplateCard` | `YQ44C/CWIDc/8wrsn` | `title, desc, cta` | radius 14 border 1 padding 18 gap 10 |
| `ScheduleListCard` | `jhWGa` | `header, table, empty` 三 slot | radius 14 border 1 |
| `ScheduleTableHeader` | `j4hWs` | `columns[]` | padding `10,20` 底 border 1 |
| `ScheduleEmptyState` | `Ifs8C` | `icon, title, desc, cta?` | h 280 center |

### 5.5 Settings（`src/components/settings/`）

| 组件 | design node | 核心 props | 视觉常量 |
|---|---|---|---|
| `SettingsShell` | `giMe2/kFHCj/vHMr4` | `menu, content`, 本轮只实现 single modal | 980×680（关于/用量 760），radius 18，阴影 lvl-3，遮罩 `#0000004d` |
| `SettingsMenu` | `YboA7/Z9asD/r95Aa` | `items[] {key, label, active}`, `onSelect` | w 220，底 `--secondary`，左上圆角 18，行 radius 10 padding `10,12` fontSize 14 |
| `SettingsContentTop` | `5aczK/dQk75/YuBIQ` | `title`, `onClose` | h 56 padding `0,28` 底 border 1 |
| `SettingsContentBody` | `7wrps/fRV7f/0M01f` | `children` | padding `24,32` gap 24 |
| `AccountPanel` | `IIzfj + nKzUU` | `user {name, tenantName, avatarUrl}`, `onLogout` | 账户卡用 `--secondary` 底 radius 14 padding 18；通知区同底 padding 24 |
| `AboutPanel` | `MQLyd + 7s18f + lcRrf` | `appMeta, helpLinks, devInfo` | 三段 gap 16 |
| `UsagePanel` | `mbeKY + BtLe0 + SAOik` | `plan, quota[], detail[]` | 计划卡 padding `16,0` 底 border 1；quota gap 18；detail gap 16 |

"系统权限 / MCP 服务 / SSO 集成 / 快捷键"四个 tab 若对应后端能力未就绪（见第 6 章），本轮只实现稿子里的静态外观 + "敬请期待"占位 body，不阻塞 spec 完成。

### 5.6 Chat Scene（`src/components/chat-scene/`）

| 组件 | design node | 核心 props | 状态 | 视觉常量 |
|---|---|---|---|---|
| `ChatComposerCompact` | `uq6ga` | `value, onChange, onSubmit, leftSlots (add, skillBtn, accessChip, projectChip), rightSlots (modelChip, micBtn)`, `submitDisabled` | default/disabled | radius 18 padding `16,18,14,18` gap 12 底 `--card` border 1；submit 圆 底 `#D4D4D8` padding 8 |
| `ChatBottomArea` | `Cbtm1` | `composerNode`, `tips?` | — | gap 10；tips 行 padding `0,12`，文字 fontSize 11 `--muted-foreground` |
| `ToolGroupCard` | `yNouu/ECmej` | `steps[]`, `status: running\|done`, `durationMs`, `expanded`, `onToggle` | 4 态：折叠-执行中（`ECmej`）/ 折叠-已完成（`yNouu` 顶部 bar）/ 展开步骤列表 / 单步展开代码 | radius 12 border 1 底 `--background`；顶 bar padding `12,14` 底 border 1；步骤行 padding `10,14`；展开 detail 底 `--muted` padding `14,16,16,46` |
| `ToolGroupStepRow` | 步骤行 | `name, status, durationMs, expanded, onToggle` | collapsed/expanded | 见上 |
| `ToolGroupCodeBlock` | 展开 detail | `inputJson, outputNode` | — | 代码字体 monospace，行高 1.5；输出部分用 `GeneratedFileCard` 嵌入 |
| `GeneratedFileCard` | `v46uG` | `title, sub, appName, appIcon, onOpen` | — | radius 14 border 1 底 `--card` padding `14,16` gap 16；左 fileIcon 44×52 radius 6 底 `--muted`；右 openBtn pill radius 999 |
| `SuggestChipGroup` | `kFmPc` | `items[] {icon, label, onClick}` | — | gap 8；chip radius 999 底 `--background` border 1 padding `6,12` gap 8，icon 14 `--primary` |
| `UserMessageBubble` | `1JNrw` | `text` | — | radius 16 padding `12,16` 底 `--primary` 前景 `--primary-foreground`；右对齐；max-width 80% |
| `AiSegmentText` | `TtxTY/HSE9l/ZK6ey` | `text` | — | fontSize 14 color `#0a0a0a` lineHeight ~1.5；段间 gap 18（父 flow 给） |
| `TypingIndicator` | `oYVXX/nVSBv/EAVW9/91cWy/gpR09` | `variant: default\|analyze\|retrieve\|generate\|organize` | 5 态 | 按稿复刻 5 套图标/文字 |
| `SkillPopover` | `ip8MF popover` | `anchorEl, items[] SkillEntry`, `onPick, onClose` | open/closed | 壳 w 560 radius 14 底 `--popover` border 1 阴影 lvl-2；popHead padding `12,16` 底 border 1 fontSize 12 / 600 |

### 5.7 Auth（`src/components/auth/`）

| 组件 | design node | 核心 props | 视觉常量 |
|---|---|---|---|
| `LoginLogoStack` | `TSZyx` | `logoUrl, brandName` | logo 56×56 radius 28，brandName fontSize 22 / 600，gap 10 |
| `LoginCard` | `PFEwh` | `children` | radius 18 padding `40,40,32,40` gap 20 width 460 border 1 底 `--card` |
| `LoginOptionsRow` | `hfGT2` | `rememberSlot, forgetSlot` | space-between；"忘记密码" fontSize 13 / 500 color `--brand-secondary` |
| `LoginFooter` | `wJSL6` | `text` | fontSize 12 `--muted-foreground` |

登录按钮用 `Button/Default` 但 radius 强制 999，padding `12,16`，fontSize 15 / 600。

---

## 6. 交互层受控改造清单

本轮允许动的**仅限以下清单**；清单外的交互、路由、事件流、消息语义**禁止修改**。

### 6.1 Chat · ToolGroup 聚合（必做）

**现状**：后端按 `tool:executing/completed` 发事件，前端 `chatStore`/`MessageList` 把每次调用渲染成单独条目。

**目标**：把一轮 agent turn 里的多次工具调用在**前端层**聚合成单张 `ToolGroupCard`。

**允许的最小后端改造（若需要）**：
- 给 `ToolCallExecuting/Completed` 事件 payload 增加 `toolGroupId`（同一轮同一 tool 名连续调用共用，否则按轮 ID fallback）、`stepIndex`、`aggregateStatus`；
- 若不改后端，前端按 `(runId, agentId)` 分组并按事件顺序编号 stepIndex。优先使用前端纯聚合方案，**不做后端事件结构破坏性变更**。

**前端状态机**：
```
ToolGroupState = { groupId, status: 'running'|'done', steps: Step[], startedAt, endedAt? }
Step           = { index, name, status: 'running'|'done', inputJson?, outputRef?, durationMs?, expanded }
```

事件到状态的映射写在 `chatStore` 的 selector 或 `useToolGroup` hook 中，不污染消息持久化结构。

### 6.2 SkillPopover 接入历史技能（必做）

把现有 `expert-mode` / `skill-store` / `skill-center` 中已注册的技能**全量**注入聊天底部"技能"按钮的弹层。数据源复用现有 `skillStore`，**不新建后端接口**。弹层条目规范按 `ip8MF popList > sp1..sp4` 结构：左 icon + 标题 + 右 toggle/badge。

### 6.3 Settings 分栏外壳（必做）

新增 `SettingsShell` 外壳 + 7 个菜单项路由（账户 / 用量 / 系统权限 / MCP 服务 / SSO 集成 / 快捷键 / 关于 AI 小家）。其中：
- 账户：接现有 auth store / user；
- 关于：读 `package.json version` + 硬编码企业信息；
- 用量、系统权限、MCP 服务、SSO 集成、快捷键：本轮**只做页面与占位文案**，等后端能力接入后再填数据。

### 6.4 消息流渲染重组（必做）

把 `MessageList` 从"扁平消息列表"改为"按 turn 分段 + 段内结构化渲染"：

```
Turn = [
  UserMessage,
  AiIntroText?,
  ToolGroupCard?,
  AiResultText?,
  GeneratedFileCard[],
  SuggestChipGroup?,
  TypingIndicator?
]
```

保留 `chatStore` 的消息存储结构，仅改渲染层做聚合。

### 6.5 Home 推荐 chip（必做）

Home 的"为你推荐 / 文案有意 / 行业研究 / 文件智能 / 电商运营 / 玩转钉钉" chips 点击后把对应 prompt 填入 composer 或跳转到技能中心对应分类。本轮只做"填入 composer"语义。

### 6.6 明确**不做**

- 不改后端 runtime / Plan-U / MCP / 权限链路；
- 不改消息持久化结构；
- 不做暗色模式 / 多 Accent 切换；
- 不重写 `authStore / uiStore / chatStore` 的 slice 形状；
- 不做 pixel-diff 自动视觉回归（见第 9 章）。

---

## 7. 页面拼装规格

每页只列「结构树 + 关键尺寸/padding」，代码实现时作为硬约束。

### 7.1 首页 · 新任务（`2cYHh`）

```
AppShell
└─ AppSidebar (w256)
└─ <main>
   └─ PageSectionShell (maxW 1032, padding [32,40,28,40], gap 16, centerX)
      ├─ HomeMascotHero (gap 16)
      ├─ HomeTaskComposerCard (w 820)
      ├─ HomeCategoryChipRow (w 820, radius 14)
      ├─ HomeStatusList (w 820)
      └─ HomeSkillCenterPill (centerX)
```

### 7.2 聊天 · 长对话（`ju2pU`）

```
AppShell
└─ AppSidebar
└─ <main>
   ├─ ChatTopBar
   ├─ MessageList/<flow> (padding [24,40], gap 18)
   │   └─ Turn[]
   │       ├─ UserMessageBubble
   │       ├─ AiSegmentText
   │       ├─ ToolGroupCard (展开态含步骤列表 + CodeBlock + 输出卡)
   │       ├─ AiSegmentText
   │       ├─ GeneratedFileCard
   │       ├─ SuggestChipGroup
   │       └─ TypingIndicator?
   └─ ChatBottomArea (padding [0,40,24,40])
       └─ ChatComposerCompact
```

### 7.3 聊天 · 技能弹层（`9qve3`）

同上，但 `ChatBottomArea` 上方挂一层 `SkillPopover`（w 560，锚点为 composer 的 "技能" 按钮）。

### 7.4 技能中心（`dVE8r`）

```
AppShell
└─ AppSidebar
└─ <main>
   ├─ PageTopBar (variant: default)
   └─ <canvas3> (padding [24,28,32,28], gap 20, bg #ffffff)
      ├─ SkillHotSection (title "热门推荐", gap 12)
      └─ SkillOfficeSection
          ├─ title "办公效率"
          ├─ SkillCategoryBar
          └─ grid gap 16
```

### 7.5 技能详情（`cSdAy`）

```
AppShell
└─ AppSidebar
└─ <main>
   └─ <canvas4> (padding [28,40,32,40], gap 24)
      ├─ SkillDetailHero (+ SkillActionBar)
      ├─ SkillMetaRow (gap 48)
      ├─ SkillTryGrid
      └─ SkillUsageBlock
```

### 7.6 定时任务（`s8Rc7`）

```
AppShell
└─ AppSidebar
└─ <main>
   ├─ PageTopBar (variant: title)
   └─ <canvas5> (padding [24,28,32,28], gap 24)
      ├─ ScheduleTemplateGrid (3 card, gap 16)
      └─ ScheduleListCard
          ├─ listHeadWrap (padding [16,20])
          ├─ ScheduleTableHeader
          └─ ScheduleEmptyState (h 280)
```

### 7.7 设置 · 三弹窗（`S3D6p / 1MCFZ / az6ZY`）

```
Overlay (#0000004d)
└─ SettingsShell (980 × 680|760, radius 18, 阴影 lvl-3)
   ├─ SettingsMenu (w 220, 底 --secondary, left radius [18,0,0,18])
   └─ <content>
       ├─ SettingsContentTop (h 56, padding [0,28], 底 border 1)
       └─ SettingsContentBody (padding [24,32], gap 24)
           └─ AccountPanel | UsagePanel | AboutPanel | <占位面板>
```

### 7.8 登录（`epkyz`）

```
<page> (bg --background, centerXY, gap 24)
├─ LoginLogoStack (logo 56 + brandName 22/600, gap 10)
├─ LoginCard (w 460, radius 18, padding [40,40,32,40], gap 20)
│   ├─ titleBlock ("登录到 AI 小家" 20/600 + sub 13 muted)
│   ├─ InputGroup "账号"
│   ├─ InputGroup "密码"
│   ├─ LoginOptionsRow (Remember + 忘记密码)
│   ├─ LoginSubmitButton (w-full, radius 999, padding [12,16], fontSize 15/600)
│   └─ LoginToc ("登录即代表同意《服务条款》与《隐私政策》", 12 muted)
└─ LoginFooter ("AI 小家 v0.9.30 · © 仁励家网络科技(杭州)有限公司", 12 muted)
```

---

## 8. 实施顺序

本 spec 只列阶段门；具体任务拆分由后续 writing-plans 输出。

| 阶段 | 交付物 | 截图对照点 |
|---|---|---|
| **A-0 基线导出** | `docs/superpowers/specs/assets/design-pen-exports/*.png`（10 页面 + 组件分区） | — |
| **A-1 Token 硬对齐** | 移除暗色/Accent 分支，`--sidebar / --brand-*` 等缺失 token 补齐，branding store 清瘦 | HomePage 截图的底色层次与稿对得上 |
| **A-2 Shell 层** | `AppSidebar`（Header/Nav/Section/Project/Conv/Footer 完整复刻）+ `PageTopBar` 4 variant + `ChatTopBar` + `PageSectionShell` | 侧栏暖金色皮肤、active `--sidebar-accent` 底 |
| **B-1 Home** | 5 个 Home 组合组件 + HomePage 拼装 | 截图对照首页 |
| **B-2 Skills** | 9 个 Skill 组合组件 + 技能中心 + 技能详情 | 截图对照技能中心 + 详情 |
| **B-3 Schedules** | 4 个 Schedule 组合组件 + SchedulesPage | 截图对照定时任务 |
| **B-4 Settings** | `SettingsShell/Menu/Top/Body` + Account/About/Usage 三面板 + 其余 4 tab 占位 | 截图对照 3 设置弹窗 |
| **C-1 Chat Scene 组件** | Composer/BottomArea/ToolGroupCard(4 态)/StepRow/CodeBlock/GeneratedFileCard/SuggestChipGroup/UserBubble/AiSegmentText/TypingIndicator(5 态)/SkillPopover | — |
| **C-2 Chat 消息流重组** | MessageList 改 turn 型渲染，ToolGroup 聚合 hook 上线 | 截图对照长对话页 |
| **C-3 SkillPopover 接入** | 全量技能接入，composer 技能按钮开弹层 | 截图对照技能弹层 |
| **D Auth** | `LoginLogoStack/LoginCard/LoginOptionsRow/LoginFooter` + LoginPage | 截图对照登录页 |
| **E 视觉收尾** | 按第 9 章清单逐页走查 + 修复遗留问题 | 10 页全部过 |

---

## 9. 验收：截图对照清单

### 9.1 脚本与产物

- 新增脚本 `scripts/capture-ui.mjs`（Playwright）：在 1280px 固定宽度窗口逐页截图到 `tmp/ui-capture/<page>.png`。该产物目录加入 `.gitignore`，不入库。
- 新增脚本 `scripts/export-design-pen.mjs` 或手工用 `mcp__pencil__export_nodes` 一次性导出 10 张稿图到 `docs/superpowers/specs/assets/design-pen-exports/`（入库）。
- **不做**像素级 diff；只产图做人工/review 对照。

### 9.2 10 页对照清单（每页 3 检查点）

| # | 页面 | 稿 PNG | 实现 PNG | 检查点 |
|---|---|---|---|---|
| 1 | 首页 | `2cYHh.png` | `home.png` | ① mascot 在标题上居中；② 推荐 chip 金色激活；③ 统态 3 行卡的 iconBox 底色按 variant |
| 2 | 聊天长对话 | `ju2pU.png` | `chat-long.png` | ① 用户气泡金底右对齐 max-80%；② ToolGroup 顶栏绿 check + 已完成 3 步 + 时长；③ GeneratedFileCard 右侧 Microsoft Excel pill |
| 3 | 聊天技能弹层 | `9qve3.png` | `chat-skill-popover.png` | ① popover 锚在 composer 上方；② 头 "管理已安装的技能"；③ 4 行技能项 padding `10,16` |
| 4 | 技能中心 | `dVE8r.png` | `skill-center.png` | ① TopBar 变体对；② 热门推荐 title 15/600；③ 办公效率分类条 + 网格 gap 16 |
| 5 | 技能详情 | `cSdAy.png` | `skill-detail.png` | ① heroIc 88×88 底 `--brand-primary-subtle`；② meta 行 gap 48；③ 右上金按钮 + 禁用 outline 按钮 |
| 6 | 定时任务 | `s8Rc7.png` | `schedules.png` | ① 3 张模板卡 padding 18 gap 16；② 列表卡 header padding `16,20`；③ 空态区 h 280 居中 |
| 7 | 设置账户 | `S3D6p.png` | `settings-account.png` | ① Modal 980×680 居中 + 遮罩；② 左 220 menu "账户" 激活白底；③ 账户卡 `--secondary` 底 radius 14 |
| 8 | 设置关于 | `1MCFZ.png` | `settings-about.png` | ① "关于 AI 小家" 激活；② appCard 平铺 padding 20；③ 帮助/开发者两段 gap 16 |
| 9 | 设置用量 | `az6ZY.png` | `settings-usage.png` | ① "用量" 激活；② planCard 底 border 1；③ quota/detail 两段 gap 18/16 |
| 10 | 登录 | `epkyz.png` | `login.png` | ① logo 56 圆 + brand 22/600；② Card 460 radius 18 padding `40,40,32,40`；③ 登录按钮 radius 999 金底 |

### 9.3 Unit test 约束

每个组合组件新增 1 条 render test（`@testing-library/react`），断言：
- 主 DOM 结构存在（关键 data-testid 或 role）；
- 所有必填 props 能驱动显示；
- 不测具体像素值。

保留并适配以下现有测试：`AuthGate.integration.test.tsx`、`SkillCenterPage.integration.test.tsx`、`Sidebar.test.tsx`、`HomePage.test.tsx`、`InputBar.agent-selector.test.tsx`、`chatStore.test.ts`。

---

## 10. 非目标与风险

### 10.1 非目标

- 不改后端 runtime / Plan-U / MCP / 权限 / subagent 链路；
- 不新增产品功能（除 ToolGroup 聚合语义与技能弹层接入外，无新能力）；
- 不实现暗色 / 多 Accent / 多 Base 的运行时切换；
- 未就绪后端能力的设置 tab 仅做占位，不阻塞验收；
- 不引入像素级视觉回归测试。

### 10.2 风险

| 风险 | 影响 | 应对 |
|---|---|---|
| ToolGroup 聚合改消息流，可能影响现有 AiBubble / StreamingBubble 测试 | 中 | 先前端纯聚合不碰后端；老测试意图保留，选择器跟随更新 |
| `--card` 与 `--background` 同值，弱边框卡片在某些显示器上对比不足 | 低 | lvl-1 阴影 + 1px border 双保险；走查时重点盯首页 statusList |
| 设置 Modal 中 4 个占位 tab 被视为"未完成" | 中 | 在 spec 与 plan 中明确"只做稿子里已有外观 + '即将上线'文案即视为满足本轮 DoD"；UI 上显式写"即将上线"提示 |
| 全量技能接入弹层后条目过多 | 低 | 弹层内实现最小滚动 + 搜索，超过 6 条启用滚动 |
| Playwright 截图受字体渲染影响与稿子细微不一致 | 低 | 验收只做人工对照，不做像素 diff |

---

## 11. 完成定义（DoD）

全部成立视为本轮完成：

1. `docs/superpowers/specs/assets/design-pen-exports/` 下 10 张基线 PNG 齐全并入库；
2. `tmp/ui-capture/` 脚本可产出 10 张实现 PNG，并与基线逐条过第 9.2 节 30 个检查点；
3. 所有组合组件（Shell / Home / Skills / Schedules / Settings / Chat Scene / Auth 共 ~50 个）按第 5 章的 props、状态、视觉常量实现，且文件顶部带 `@designSource` JSDoc；
4. 页面层文件全部符合 4.2 禁止清单，单文件 ≤ 120 行为软目标，> 150 行需在 PR 中解释；
5. Token 层只剩 Light + Neutral + Default 一套，`branding` store 无暗色/Accent/Base 字段；
6. `pnpm lint` / `tsc` / `pnpm test` 通过；保留的 6 条核心集成测试通过；
7. 手工打开应用：首页 / 聊天长对话 / 聊天技能弹层 / 技能中心 / 技能详情 / 定时任务 / 设置三弹窗 / 登录，均能与 design.pen 对应页面逐项对齐，视觉与交互走查无明显"把柄"。
