# 技能中心页面 B6iNU 设计对齐

**日期：** 2026-04-24
**状态：** 待实施
**设计源：** design.pen 节点 B6iNU（Frame 8）

---

## 背景

当前 `SkillCenterPage` 与 design.pen B6iNU 稿子有以下差距：

1. **顶栏**：现在只有「技能市场」「上传技能」两个按钮，稿子有 `技能中心` 标题 + 技能数量 badge + 搜索框 + 「上传技能资料」「+ 创建技能」
2. **热门推荐卡片**：稿子为行布局（图标 + 标题/元信息 在左，描述文字在右下），高度 140px，没有底部按钮区
3. **全部技能卡片**：稿子为行布局，高度 120px，没有底部按钮区
4. **分类 bar**：稿子是圆角 chip 样式，激活态为品牌主色填充；现有实现用 `rounded-md bg-secondary`
5. **卡片交互**：稿子整卡可点击进入详情页，无常驻「详情/使用」按钮
6. **分类数据**：稿子的分类 chip 为「全部 / HR / 财务 / 法务 / 销售 / 运营 / 通用」，与现有 `SKILL_CATEGORIES` 不符

---

## 目标

按 B6iNU 全面还原技能中心页面视觉与交互，数据仍从 `listSkills()` / `skillStore` 读取。

---

## 方案

### 1. 顶栏 (`SkillCenterPage` + `PageTopBar`)

- 左侧：`技能中心` 标题（18px/700）+ 技能数量 badge（次要文字 + `bg-secondary` 圆角 pill）
- 右侧：搜索框（`bg-secondary` 圆角 pill，宽 220px，搜索 icon + 占位文字）、「上传技能资料」（Outline 按钮）、「+ 创建技能」（Primary 按钮）
- 搜索框本次只做静态样式，不做实际筛选逻辑（范围管控，避免扩大）
- 上传、创建按钮沿用现有弹窗入口

### 2. 技能卡片重构 (`SkillCard`)

设计规格：
- 热门卡（大）：`h-[140px]` + `rounded-[14px]` + `p-4` + `gap-[10px]` + `border border-border`
- 普通卡（小）：`h-[120px]`，其余同上
- 头部：`36×36`（热门）/ `34×34`（普通）圆角图标容器 + 右侧竖排标题/元信息
- 图标容器颜色：来自 `SkillInfo.icon` 字段扩展（见第 4 条），fallback 用 `bg-brand-primary-subtle`
- 元信息格式：`内置 · {category显示名}` 或 `自定义 · {category显示名}`，颜色 `text-brand-secondary`
- 描述文字：12px，`text-muted-foreground`，字数超出截断（line-clamp-2）
- 无底部按钮；整卡点击 → 进入详情页（`setRoute({ kind: 'skill-detail', skillId })`）
- hover 效果：`hover:-translate-y-0.5 transition-all duration-150`（稿子里的轻微上浮）

拆分两个组件：
- `SkillCardHot` — 热门推荐大卡，接受 `variant="hot"`
- `SkillCardOffice` — 全部技能小卡，接受 `variant="office"`

> 实现时可以用一个 `SkillCard` 组件加 `size` prop 区分，不强制拆文件，具体由实施决定。

### 3. 分类 Chip Bar (`SkillCategoryBar`)

设计规格（来自节点 `Kkinf` / `rnhH6`）：
- chip 为圆角 pill（`rounded-full`），padding `[8px, 14px]`，font 13px
- 激活态：`bg-brand-primary-subtle text-primary font-semibold`
- 非激活态：无背景/border，`text-muted-foreground font-medium`，hover `bg-muted`

### 4. 分类数据 (`skill-categories.ts`)

稿子的分类 chip 顺序：全部 / HR / 财务 / 法务 / 销售 / 运营 / 通用

新的 `SKILL_CATEGORIES`：

```ts
[
  { id: 'hr',      name: 'HR',   icon: 'users' },
  { id: 'finance', name: '财务', icon: 'bar-chart-2' },
  { id: 'legal',   name: '法务', icon: 'scale' },
  { id: 'sales',   name: '销售', icon: 'trending-up' },
  { id: 'ops',     name: '运营', icon: 'settings' },
  { id: 'general', name: '通用', icon: 'wrench' },
]
```

`SkillCategoryId` 同步更新，`recommended` 保留。

> `SkillCategoryBar` 的 `items` 包含「全部」chip（key = `recommended`），在 `SkillCenterPage` 里拼到头部即可，不入 `SKILL_CATEGORIES`。

### 5. SkillHotSection / SkillOfficeSection

- `SkillHotSection`：标题「热门推荐」，grid `grid-cols-3 gap-4`，固定显示前 3 个 `recommended` 技能
- `SkillOfficeSection`：标题「全部技能」，grid `grid-cols-3 gap-2.5`（稿子 gap 比热门稍小）

### 6. 图标颜色扩展（可选，不阻塞主流程）

稿子中每张卡的图标容器有各自的颜色（primary-subtle / secondary-subtle / 橙色 / 等）。本次可先用 `source === 'builtin' ? 'bg-brand-primary-subtle' : 'bg-secondary'` 简单处理，后续在 SkillInfo 扩展 `iconBgColor` 字段时跟进。

---

## 不在范围内

- 搜索功能（本次只做 UI 静态）
- 技能创建/上传实际后端功能
- 技能市场

---

## 涉及文件

| 文件 | 变更类型 |
|---|---|
| `src/features/skill-center/SkillCenterPage.tsx` | 重构顶栏、传入新 category items |
| `src/components/skills/SkillCard.tsx` | 重构为无底部按钮 + 整卡点击 + size prop |
| `src/components/skills/SkillCategoryBar.tsx` | 更新 chip 样式至稿子规格 |
| `src/components/skills/SkillHotSection.tsx` | 调整 grid gap |
| `src/components/skills/SkillOfficeSection.tsx` | 标题改「全部技能」，调整 gap |
| `src/data/skill-categories.ts` | 更新分类列表 + `SkillCategoryId` 类型 |
| 对应 `__tests__/` 文件 | 同步更新测试断言 |
