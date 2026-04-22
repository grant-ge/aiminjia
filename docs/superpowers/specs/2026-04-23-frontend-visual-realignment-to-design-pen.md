# 前端视觉重构对齐 `design.pen` — 设计规格

**背景**

当前前端 Skill-First 改造已经完成信息架构与功能骨架，但视觉还原度明显低于 `/Users/oayzz/project/lotus/lotus-workbench/lotus-app/design.pen` 中定义的目标稿。现有页面过度依赖通用 shadcn 原子组件直接拼装，导致页面层自行承担了过多视觉责任，最终出现以下问题：

- 页面留白、容器宽度、分区层次与设计稿不一致。
- Sidebar / TopBar / 首页任务输入区 / 技能卡 / 设置弹窗等仍是“功能型骨架”，不是设计稿中的成套组合组件。
- 聊天页缺少设计稿中的 ToolGroup 聚合过程条、GeneratedFileCard 视觉层次与紧凑型输入区结构。
- 视觉语言虽然已经接近暖色租户皮肤，但仍停留在“默认组件换 token”层面，没有达到设计稿中的低对比、轻边框、轻阴影、稳定节奏。

本轮不再继续扩展新的前端功能，而是把现有前端重构成一个真正贴近 `design.pen` 的组件化实现。

---

## 目标

将现有前端从“页面直接堆 shadcn 原子组件”的实现方式，重构为“组合组件先行、页面仅负责拼装”的结构，并以 `design.pen` 为唯一视觉基准，覆盖以下范围：

- 首页（Home · 新任务）
- 聊天页（Chat · 长对话 / 技能弹层相关主场景）
- 技能中心页
- 技能详情页
- 定时任务页
- 设置弹窗
- 登录页
- AppShell 级布局（Sidebar / 顶栏 / 内容容器）

---

## 非目标

本轮不处理以下内容：

- 后端 runtime / Plan-U / MCP / 权限 / subagent 主链路改造
- 新增全新产品功能（例如新的技能市场流程、上传流程、任务调度后端能力）
- 重写现有前端状态管理模型（`uiStore` / `authStore` / `skillStore` 仍保留）
- 完全按 `.pen` 文件生成代码；本轮是“参考设计稿重构 React UI”，不是导出器工程

---

## 设计原则

### 1. 组件优先，页面后置

所有页面必须优先依赖组合组件层，不允许页面自己重新定义大块视觉结构。

换句话说：

- 页面可以组织内容
- 页面不能重新发明 Sidebar / TopBar / Hero Card / 列表卡片 / 输入区 / 设置容器 的视觉规则

### 2. `design.pen` 是唯一视觉真源

视觉判断优先级如下：

1. `design.pen`
2. 当前已落地的 token / 租户皮肤系统
3. 现有页面代码

若现有实现与设计稿冲突，应优先向设计稿靠拢，而不是为了复用旧代码牺牲还原度。

### 3. 保留现有状态与交互语义，重做视觉承载层

这轮优先重构 UI 承载层，不主动改变以下逻辑：

- route 切换语义
- 登录 Gate 语义
- 技能列表 / 技能详情数据来源
- 聊天消息流与消息结构
- 设置页 tab 切换逻辑

只有当现有交互结构明显阻碍设计稿落地时，才允许做最小结构调整。

### 4. 组合组件必须显式分层

本轮组件层分为三层：

- **原子层**：现有 `src/components/ui/*` 与少量 `src/components/common/*`
- **组合层**：承载视觉结构的可复用组件
- **页面层**：只负责拼装组合组件与绑定状态

目标是让页面代码重新变薄，视觉复杂度回到组件层。

---

## 设计稿到代码的映射

### 1. AppShell / 布局组件

对应 `design.pen` 中：

- `PV1ln` `Sidebar`
- `qLmzZ` `ChatTopBar`
- `BixkY` `PageTopBar/Default`
- `aAO2u` `PageTopBar/Title`
- `tCYsE` `PageTopBar/Breadcrumb`
- `WgoHO` `PageTopBar/Compact`

代码中应形成以下组件：

- `AppSidebar`
- `PageTopBar`
- `ChatTopBar`
- `PageSectionShell`（可选，用于统一页面容器宽度、上下留白、区块间距）

### 2. 首页组件

对应 `design.pen`：

- `80303 / [首页] Home · 新任务`

代码中应拆为：

- `HomeTaskComposerCard`
- `HomePromptTabs`
- `HomeSuggestionList`

其中首页页面本身只负责组装 Sidebar + PageTopBar + 中央任务卡。

### 3. 技能中心 / 技能详情组件

对应 `design.pen`：

- `H技能 / [技能] Skills · 技能中心`
- `H技能 / [技能] Skill Detail · 技能详情`

代码中应拆为：

- `SkillHeroHeader`
- `SkillCategoryTabs`
- `SkillCard`
- `SkillDetailHeader`
- `SkillWorkflowPreviewCard`
- `SkillActionBar`

### 4. 定时任务组件

对应 `design.pen`：

- `a4338 / [任务] Schedules · 定时任务`

代码中应拆为：

- `ScheduleTemplateCard`
- `ScheduleListShell`
- `ScheduleTableHeader`

### 5. 设置弹窗组件

对应 `design.pen`：

- `H设置 / Settings · 设置 Modal`
- `H设置 / Settings · 关于 AI 小家`
- `H设置 / Settings · 用量`

代码中应拆为：

- `SettingsShell`
- `SettingsSidebarNav`
- `SettingsSectionCard`
- `AboutPanel`
- `UsagePanel`

### 6. 聊天场景组件

对应 `design.pen`：

- `H聊天 / [聊天] Chat · 长对话拼装`
- `H聊天 / [聊天] Chat · 技能弹层`
- 组件区中的 `toolGroupBar` / `toolGroup` / `GeneratedFileCard` / `ChatComposerCompact`

代码中应拆为：

- `ChatComposerCompact`
- `ToolGroupBar`
- `ToolGroupPanel`
- `GeneratedFileCard`
- `ChatEmptyState`（用于 WelcomeScreen 风格收敛）

聊天页是本轮最重要的视觉修正区域，但仍保留现有消息语义与 store 数据流。

### 7. 登录页

对应 `design.pen`：

- `H认证 / Login · 登录`

代码中应把现有 `LoginPage` 改造成与整体系统统一的居中卡片与品牌语义，而不是通用 shadcn 登录表单。

---

## 组件实施顺序

本轮严格使用方案 A：**先组件，再页面**。

### 阶段 A：组合组件层重建

优先顺序：

1. `AppSidebar` / `TenantHeader` / `SidebarNav` / `ConversationTree` 视觉重构
2. `PageTopBar` / `ChatTopBar` 抽象
3. 首页任务卡与建议区组件
4. 技能卡、技能详情头部与工作流卡片
5. 设置弹窗外壳与分栏结构
6. 聊天页的 `ChatComposerCompact`、`ToolGroupBar`、`ToolGroupPanel`、`GeneratedFileCard`

### 阶段 B：页面拼装替换

按以下顺序替换页面：

1. `HomePage`
2. `SkillCenterPage`
3. `SkillDetailPage`
4. `SchedulesPage`
5. `SettingsModal`
6. `ChatPage` / 聊天页相关入口
7. `LoginPage`

这样做的原因是：

- 首页 / 技能 / 定时任务对消息流依赖较少，适合先建立统一视觉节奏
- 设置页与聊天页更复杂，放到后段可以直接吃前面沉淀下来的组合组件

---

## 代码结构建议

建议在现有目录结构中新增或重组以下文件族群：

### `src/components/shell/`

承载 AppShell 级别组合组件：

- `PageTopBar.tsx`
- `PageSectionShell.tsx`
- `ChatTopBar.tsx`（也可保留在现有目录，但职责需更清晰）

### `src/components/home/`

- `HomeTaskComposerCard.tsx`
- `HomeSuggestionList.tsx`

### `src/components/skills/`

- `SkillHeroHeader.tsx`
- `SkillCategoryTabs.tsx`
- `SkillWorkflowPreviewCard.tsx`
- `SkillActionBar.tsx`

### `src/components/schedules/`

- `ScheduleTemplateCard.tsx`
- `ScheduleListShell.tsx`

### `src/components/chat-scene/`

- `ChatComposerCompact.tsx`
- `ToolGroupBar.tsx`
- `ToolGroupPanel.tsx`

如果某些目录拆分对现有代码移动成本过高，也允许保留在原目录下，但必须保证职责收敛，而不是继续把大块视觉直接塞回 `features/*` 页面文件。

---

## 视觉约束

### 配色

沿用现有 token / skin 系统，但视觉使用规则向设计稿收敛：

- 背景以暖白 / 米白为主
- 侧栏背景和页面背景有轻微区分
- 强调色使用租户金色主色，但只出现在 CTA、选中态、关键标签
- 边框必须轻，不能重新回到高对比灰边框堆叠感

### 圆角与阴影

- 卡片与浮层使用中小圆角
- 阴影极轻，只用于浮层、重点卡片、顶部轻悬浮感
- 普通容器优先用边框和背景层次，不滥用阴影

### 字体层级

- 页面标题、分区标题、辅助说明、列表 meta 必须形成稳定 4 级层级
- 不允许页面各自用任意字号凑视觉

### 间距

所有页面必须统一遵守：

- 页面外边距
- 区块间距
- 卡片内部 padding
- 列表项高度与垂直 rhythm

不能继续出现某一页 24px、另一页 32px、第三页 20px 的自由散落。

---

## 测试与验收策略

### 1. 组件级验证优先

每个关键组合组件至少要有最小渲染测试，确保：

- 主结构存在
- 关键 CTA / 文案 / 状态能正确显示
- 关键 props 能驱动不同视觉状态

### 2. 页面级验证保留现有核心集成测试

保留并适配当前已有：

- `AuthGate.integration.test.tsx`
- `SkillCenterPage.integration.test.tsx`
- `Sidebar.test.tsx`
- `InputBar.agent-selector.test.tsx`

必要时新增页面快照/结构断言，但不做脆弱的像素级测试。

### 3. 手工验收清单

最终手工验收必须覆盖：

- 首页与设计稿的主视觉对齐
- 侧栏导航层级与选中态
- 技能中心与技能详情的密度和卡片层次
- 设置弹窗分栏与面板层次
- 聊天页顶部、消息区、底部输入区、ToolGroup 样式
- 登录页是否与整体品牌风格统一

---

## 风险与约束

### 风险 1：页面直接样式残留

如果组合组件没有先抽出来，页面会继续绕过设计系统自己写样式，导致这轮重构失败。

**应对：** 先组件后页面，页面层只允许消费组合组件。

### 风险 2：聊天页复杂度高

聊天页同时承载消息语义、工具过程、文件卡片、输入区，是最容易“做着做着退回骨架”的页面。

**应对：** 先完成聊天场景组合组件，再改页面，不在 `ChatPage` 内直接堆视觉细节。

### 风险 3：现有测试与结构重构冲突

组件目录重组后，现有测试可能需要跟随迁移。

**应对：** 保留旧测试意图，只更新结构与选择器，不推倒重写行为测试。

---

## 完成定义

当以下条件全部成立时，这轮视觉重构可视为完成：

- 首页、聊天页、技能中心、技能详情、定时任务、设置、登录页都已对齐 `design.pen` 的主视觉结构。
- 关键视觉结构不再由页面层临时实现，而是沉淀为组合组件。
- 页面文件显著变薄，主要承担数据绑定与拼装职责。
- 现有前端 lint / 类型检查 / 测试通过。
- 手工打开应用时，整体观感从“shadcn 骨架页”提升为“贴近设计稿的产品界面”。

