# ChatBottomArea 设计说明

日期：2026-04-23

## 背景

当前对话区底部使用的是 `InputBar`，而 `design.pen` 中对应对话区底部的可复用节点是 `Cbtm1 / ChatBottomArea`。现状存在两个问题：

- 设计稿节点与运行时代码没有一一对应，底部区域不是按 `Cbtm1` 组件化落地。
- `ChatComposerCompact` 已实现了设计稿中的紧凑输入卡片，但还没有成为可在不同页面复用的输入核心。

本次目标是让对话区底部按 `Cbtm1` 落地，同时把输入核心能力尽量收敛到 `ChatComposerCompact`，避免继续扩大 `InputBar` 的职责。

## 目标

- 新增对话区底部组件 `ChatBottomArea`，作为 `Cbtm1` 的运行时实现。
- 将 `ChatComposerCompact` 升级为可复用输入核心组件。
- 对话页底部改为使用 `ChatBottomArea`，完整替换当前 `InputBar` 的交互能力。
- 首页继续使用 `ChatComposerCompact`，但允许按页面场景传入不同显示项和行为。

## 非目标

- 不在本次改动中重构整个聊天页布局。
- 不改变发送消息、上传文件、授权目录、停止流式、slash skill 选择等既有产品行为。
- 不统一首页和对话页的所有业务文案，只抽出可复用的展示位和交互能力。

## 组件分层

### 1. ChatComposerCompact

`ChatComposerCompact` 作为底层可复用输入组件，负责承载通用 UI 结构与大部分交互入口。它不直接耦合具体页面的数据来源，而是通过 props 暴露状态、文案和事件。

它将支持以下能力：

- 文本输入值与变更回调
- 回车发送与 Shift+Enter 换行
- IME 组合输入保护
- 自动聚焦与自动增高
- 发送按钮状态
- 附件入口按钮
- 技能入口按钮
- 权限信息展示与切换入口
- 项目/工作空间展示与切换入口
- 模型信息展示与切换入口
- 底部 tips 区域
- 可选的 pending files 展示槽位

其中，按钮和信息区都通过显式 props 控制显隐，而不是把页面判断写死在组件内部。

### 2. ChatBottomArea

`ChatBottomArea` 是对话页底部的场景化封装，对应 `design.pen#Cbtm1 ChatBottomArea`。

它的职责是：

- 组合 `ChatComposerCompact`
- 连接 `useChat`、`useFileUpload`、`useWorkspaceAuthorization`、`useAuthorizedWorkspace` 等现有能力
- 保留并迁移 `InputBar` 的全部行为
- 为对话场景提供底部 tips
- 隐藏“项目/工作空间按钮”
- 保留“完全访问权限”相关入口，在对话页中可切换
- 在主卡片上方继续展示已连接工作区提示条

换句话说，`ChatBottomArea` 负责“把对话页需要的信息带出来并接上行为”，而不是重新实现一套输入框 UI。

## 页面使用方式

### 首页

首页继续使用 `ChatComposerCompact`，维持现有“发起新对话”的行为。首页可以继续显示项目/工作空间按钮，用于发起前选择目录。

### 对话页

对话页使用 `ChatBottomArea` 替换 `InputBar`。它将：

- 复用 `ChatComposerCompact` 的紧凑输入卡片 UI
- 隐藏项目/工作空间按钮
- 通过 props 注入对话态的发送、停止流式、上传附件、slash skill、权限信息等能力
- 在卡片下方追加 `Cbtm1` 的 tips 行

## 行为要求

对话页中的 `ChatBottomArea` 必须完整保留现有 `InputBar` 的行为：

- 文本消息发送
- 仅文件时的默认发送文案
- 发送中的防重入保护
- 流式输出时按钮切换为 stop
- slash command 弹层选择技能
- 输入框自动聚焦
- 输入框高度按内容自动增长
- IME 组合输入下 Enter 不误发
- 附件上传菜单与文件 chips
- 目录授权入口与已连接工作区提示
- 删除待发送文件

## 视觉对齐

对话页底部视觉上对齐 `Cbtm1`：

- 上半部分使用 `ChatComposerCompact` 的紧凑输入卡片样式
- 下半部分新增 tips 行
- tips 至少包含：
  - “内容由 AI 生成，请仔细核实回答内容”
  - “Enter 发送”
  - “Shift+Enter 换行”

如果由于运行时状态需要在卡片内部增加 pending files 或工作区提示条，以功能完整性优先，但不改变 `Cbtm1` 的主体布局关系。

## 实现步骤

1. 先为 `ChatComposerCompact` 补测试，覆盖新的可配置显示项和基础交互。
2. 在失败测试约束下扩展 `ChatComposerCompact`，使其能够承载对话页所需的显示槽位和交互入口。
3. 新增 `ChatBottomArea`，迁移 `InputBar` 的状态管理与行为接线。
4. 更新对话页，使用 `ChatBottomArea` 替换 `InputBar`。
5. 迁移或补充测试，覆盖对话页底部关键行为未回归。
6. 运行相关测试验证。

## 测试策略

- `ChatComposerCompact` 组件测试：
  - 条件显示按钮/信息区
  - Enter 触发提交，Shift+Enter 不提交
  - 提交禁用态
  - tips 渲染
- `ChatBottomArea` 组件测试：
  - 基本渲染
  - 文本发送
  - 流式中按钮切换 stop
  - 隐藏项目按钮但显示权限信息
  - 文件 chips 与 tips 存在
- 对话页接线测试：
  - 底部组件替换后页面仍可正常渲染

## 风险与处理

- `InputBar` 当前逻辑较多，迁移时容易丢失细节。
  - 处理：先补失败测试，再逐步迁移，避免一次性重写。
- `ChatComposerCompact` 从纯展示组件变成高度可配置组件后，props 可能膨胀。
  - 处理：优先按清晰的分组命名组织 props，避免直接把页面内部状态泄漏进去。
- 首页与对话页复用同一组件后，样式可能互相影响。
  - 处理：把页面差异收敛到显式 props，避免依赖隐式 class hack。

## 结果预期

完成后，代码中的组件语义与设计稿对齐关系应为：

- `design.pen#uq6ga` -> `ChatComposerCompact`
- `design.pen#Cbtm1` -> `ChatBottomArea`

并且对话区底部不再直接依赖旧的 `InputBar` 作为主实现。
