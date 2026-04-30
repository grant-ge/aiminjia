# 前端 Skill-First 改版 · 设计系统 & 租户皮肤 · 登录 Gate · 技能中心 — 设计规格

> **状态**：设计（脑风完成，等待实施）
> **日期**：2026-04-22
> **作者**：oayzz + Claude（pzc 分支）
> **范围**：前端信息架构 + 设计规范落地 + 租户皮肤机制 + 登录 Gate + 技能中心页面骨架。**仅前端**；后端 Skill 执行链路修复、Persona / Agent 数据迁移另开 plan。
> **产物**：`design.pen`（已有）+ 本 spec。
> **关联**：
> - `docs/superpowers/specs/2026-04-21-skill-first-redesign-design.md`（本 spec 的前序视觉稿 spec，仍有效）
> - `docs/skill-system-comparison.md`（Skill 系统现状对比）
> - `docs/superpowers/plans/2026-04-21-skill-system-complete-overhaul.md`（后端 Skill 修复计划）

## 1. 背景

### 1.1 本轮要解决的问题

- 当前前端**没有一套统一的设计规范**：`styles/globals.css` 用自定义命名（`--color-primary-*` / `--color-bg-*`），和 Tailwind utility 前缀冲突；变量命名和 `design.pen` 的 shadcn 风格（`--primary` / `--background`）不一致。
- 当前**皮肤机制过度拟合** lotus 默认视觉：`brandingStore` 接受 `primaryColor` / `accentColor` / `bgColor` / `sidebarBgColor` 四个字段（后端实际返回的 `primaryColor` 已经无业务意义，`sidebarBgColor` 硬色值难以和租户的品牌主色协调）。
- 当前**专家（Persona） / Agent / Skill 三套概念并存**：用户通过 Persona 切身份、Agent 下拉约束工具池、Skill 在设置深处。信息架构碎，心智负担重。
- 当前**登录不是 Gate**：未登录也能进主界面；登录入口藏在设置 Tab；还有本地 / 云端模型双线的 `useCloud` 开关。

### 1.2 本轮目标（前端视角）

1. **设计 Token 切到 shadcn 原生命名**，和 `design.pen` 零映射对齐；**引入 shadcn/ui 组件库**，按视觉稿做组件迁移。
2. **皮肤机制收敛**：租户只提供一个 `accentColor`，其余 Token 由算法派生；design.pen 的默认视觉（米白侧栏 + 金色品牌）由算法在默认 `accentColor = #DBAA22` 下自然呈现。
3. **信息架构切到 Skill-First**：侧栏 4 入口（新任务 / 技能中心 / 定时任务 / 设置）；删除 Persona / AgentSelector / 云端-本地 toggle；技能中心承担"发现 + 详情 + 市场入口 + 上传入口"（第一期市场和上传只做页面骨架，等后端同事对齐数据后再接）。
4. **登录成为全屏 Gate**：未登录 / token 过期 / 主动退出都强制回登录页；有工作现场时登录成功回原位置，冷启动首次登录去「新任务」首页。

### 1.3 非目标

- 不改 Rust runtime / Python runtime / tool pipeline。
- 不修复 Skill 执行链路断开 bug（B1）。
- 不做 Persona → Skill 数据迁移（后端职责）。
- 不做 Agent → Skill 吸收合并（后端职责）。
- 不做 dark mode UI 切换（Token 结构预留，MVP 只出 light）。
- 不做定时任务后端。
- 不做技能市场真实数据拉取与安装功能。

## 2. 架构总览

### 2.1 分层

```
L0 Pencil 视觉稿 ── design.pen ──────────────────────── (Source of Truth, 视觉)
                         │ 变量：$--primary / $--background / $--sidebar / ...
                         │ 107 reusable 组件（shadcn 原版导入）
                         ▼
L1 Token 层   ── styles/globals.css ────────────────── (代码侧 Token 真相源)
                         │ :root { --primary: #DBAA22; --background: #FAFAFA; ... }
                         │ @theme inline { --color-primary: var(--primary); ... }
                         ▼
L2 运行时派生 ── styles/skin.ts + stores/brandingStore ─ (租户定制的唯一入口)
                         │ deriveSkin(accentColor) → 返回要 setProperty 的所有 key-value
                         │ brandingStore 调用并在 document.documentElement 上 setProperty
                         ▼
L3 组件层     ── components/ui/* (shadcn 原版) ───────── (UI 原语)
                         │ Button / Card / Sidebar / Dialog / Tabs / Dropdown ...
                         ▼
L4 业务层     ── features/chat | skills | schedules | auth | settings ── (页面/业务组件)
                         │ 组合 L3 原语实现业务
```

### 2.2 四个强制原则（借鉴 qob 踩坑经验）

1. **严禁硬编码颜色**：一律通过 Tailwind class 引用 Token（`bg-primary` / `text-foreground` / `border-border` / `bg-sidebar`）；唯一例外是主色按钮上的 `text-primary-foreground`（已在 `@theme inline` 映射）。
2. **严禁 inline style 设置颜色**：`style={{ backgroundColor: ... }}` 会覆盖 Tailwind 的 `hover:` / `active:` / `focus:` 伪类，导致交互状态失效。
3. **严禁 JS hover**：`onMouseEnter` / `onMouseLeave` 手动改 style 是反模式，一律用 `hover:brightness-110` 等 Tailwind 伪类。
4. **可交互元素必须四件套**：`hover:` + `active:` + `transition-*`（`transition-all` 或 `transition-colors`）+ `disabled:opacity-50`。

### 2.3 目录结构变动

```
src/
├── styles/
│   ├── globals.css         ← 改写：shadcn 原生 Token + @theme inline + reset
│   └── skin.ts             ← 新增：租户色派生算法（mix/darken/lighten/isDarkColor）
├── components/
│   ├── ui/                 ← 新增：shadcn/ui 官方组件（Button/Card/Sidebar/...）
│   ├── chat/               ← 重构：删 AgentSelector；对齐新视觉
│   ├── sidebar/            ← 新增：主侧栏容器（对应 design.pen PV1ln）
│   ├── skill-center/       ← 新增：技能中心 / 技能详情 / 市场弹层 / 上传弹层
│   ├── schedules/          ← 新增：定时任务页面
│   ├── auth/               ← 新增：AuthGate + LoginPage + FullscreenLoader
│   └── common/             ← 缩减：保留业务特有组件（如 PermissionAskDialog、ToolCallCard）
├── features/
│   ├── auth/               ← 新增：login-page + session-restore
│   ├── home/               ← 新增：新任务首页
│   ├── skill-center/       ← 新增
│   ├── skill-detail/       ← 新增
│   └── schedules/          ← 新增
├── stores/
│   ├── brandingStore.ts    ← 改写
│   ├── authStore.ts        ← 扩展：redirectFrom / isAuthPending
│   ├── personaStore.ts     ← 删除
│   ├── skillStore.ts       ← 新增
│   └── settingsStore.ts    ← 缩减：删除 useCloud / cloudModel*
├── data/
│   └── skill-categories.ts ← 新增：前端写死的 10 个分类
└── lib/
    └── tauri.ts            ← 保持；删除 persona/agent 的前端调用（IPC 封装标注 deprecated）
```

## 3. 设计 Token 与主题

### 3.1 命名策略

**shadcn 原生命名**（`--primary` / `--background` / `--muted` / `--border` / `--sidebar-*` / `--destructive`），与 `design.pen` 的变量名一一对应。通过 `@theme inline` 映射到 Tailwind utility（`bg-primary` / `text-foreground` / `bg-sidebar`）。

注意：这里只采用 shadcn 的 token 命名，不继承 shadcn 默认 neutral 主题的黑色 `primary`。Lotus / design.pen 的默认品牌主色是金色 `#DBAA22`，黑色 `#0a0a0a` 只用于正文与前景类 token。

### 3.2 Token 清单

| 类别 | Token | 默认值 | 租户可改 | 说明 |
|---|---|---|---|---|
| 品牌 | `--primary` | `#DBAA22` | ✅ 来自 `accentColor` | 按钮、焦点、聚光元素 |
| 品牌 | `--primary-foreground` | `#FFFFFF` | 派生 | primary 上的文字（根据 primary 亮度选黑/白） |
| 品牌 | `--ring` | = `--primary` | 派生 | 焦点环 |
| 品牌 | `--sidebar-primary` / `-foreground` | = `--primary` / `#FFFFFF` | 派生 | 预留槽；design.pen 已在全 8 个 Accent 轴固定为 `#DBAA22` |
| 页面 | `--background` | `#FAFAFA` | ❌ 固定 | 主区底色 |
| 页面 | `--foreground` | `#0a0a0a` | ❌ 固定 | 主区文字 |
| 中性 | `--muted` / `-foreground` | `#f5f5f5` / `#737373` | ❌ 固定 | |
| 中性 | `--card` / `-foreground` | `#fafafa` / `#0a0a0a` | ❌ 固定 | |
| 中性 | `--popover` / `-foreground` | 同 card | ❌ 固定 | 下拉、弹出层 |
| 中性 | `--secondary` / `-foreground` | `#f5f5f5` / `#0a0a0a` | ❌ 固定 | 次要按钮 |
| 中性 | `--accent` / `-foreground` | `#f5f5f5` / `#0a0a0a` | ❌ 固定 | **shadcn 语义**：悬浮高亮（不是品牌色） |
| 边框 | `--border` | `#e5e5e5` | ❌ 固定 | |
| 边框 | `--input` | `#e5e5e5` | ❌ 固定 | 输入框边框 |
| 侧栏 | `--sidebar` | `mix(--primary, #fff, 93%)` | ✅ 派生 | 侧栏底色；默认主色下 ≈ `#FBF6E6` |
| 侧栏 | `--sidebar-accent` | `darken(--sidebar, 8%)` | ✅ 派生 | 侧栏选中态；默认主色下 ≈ `#E9DEB2` |
| 侧栏 | `--sidebar-accent-foreground` | `#0a0a0a` | ❌ 固定 | |
| 侧栏 | `--sidebar-foreground` | `#0a0a0a` | ❌ 固定 | 侧栏文字 |
| 侧栏 | `--sidebar-border` | `#E1DAC6` | ❌ 固定 | 侧栏右侧分隔线（极暗主色兜底） |
| 侧栏 | `--sidebar-ring` | `#71717a` | ❌ 固定 | |
| 语义 | `--destructive` / `-foreground` | `#e7000b` / `#FFFFFF` | ❌ 固定 | |
| 字体 | `--font-sans` | 苹方 + Inter | ✅ 来自 `fontFamily` | |
| 字体 | `--font-mono` | SF Mono / Fira Code / Menlo | ❌ 固定 | |
| 圆角 | `--radius-sm` / `--radius` / `--radius-md` / `--radius-lg` / `--radius-xl` | 4 / 6 / 8 / 12 / 16 | ❌ 固定 | 对齐 design.pen |

### 3.3 派生算法

`src/styles/skin.ts`：

```ts
export function deriveSkin(accentColor: string): Record<string, string> {
  const fg = isDarkColor(accentColor) ? '#FFFFFF' : '#1A1A1A'
  const sidebar = mix(accentColor, '#FFFFFF', 0.93)
  return {
    '--primary': accentColor,
    '--primary-foreground': fg,
    '--ring': accentColor,
    '--sidebar-primary': accentColor,
    '--sidebar-primary-foreground': fg,
    '--sidebar': sidebar,
    '--sidebar-accent': darken(sidebar, 0.08),
  }
}

// 工具函数（已在现有 themeUtils.ts 里）
export function mix(hex: string, other: string, weightOfOther: number): string
export function darken(hex: string, amount: number): string
export function lighten(hex: string, amount: number): string
export function isDarkColor(hex: string): boolean
```

**极暗主色兜底**：侧栏右侧保留 1px `--sidebar-border` 竖线（写在 Sidebar 组件里，不可移除），保证 `accentColor = #1A2E22` 这类深墨绿主色下，侧栏和主区仍能视觉分区。

### 3.4 brandingStore 职责

```ts
interface BrandingState {
  productName: string
  logoUrl: string
  accentColor: string        // 唯一的租户色输入
  fontFamily: string
  isCustom: boolean

  applyBranding(tenant: TenantBranding | null): void
  reset(): void
}

interface TenantBranding {
  productName?: string
  logoUrl?: string
  accentColor?: string        // 被使用
  fontFamily?: string
  // 以下字段后端仍会返回，前端忽略
  primaryColor?: string       // 忽略
  bgColor?: string            // 忽略
  sidebarBgColor?: string     // 忽略
}
```

**行为**：

- `applyBranding(tenant)`：
  - 非色值部分（productName / logoUrl / fontFamily）：沿用现有处理
  - 色值：取 `tenant.accentColor`（如缺省回退到 `#DBAA22`）→ 调 `deriveSkin()` → 在 `document.documentElement` 上 `setProperty` 每一条
  - 设置 `window.title` 和 `productName` 组合
- `reset()`：
  - `removeProperty` 所有派生 key（全部 7 条），fallback 到 `globals.css` `:root` 默认值
  - 恢复 `productName` 等到 DEFAULTS
  - 设置 `window.title`

### 3.5 `globals.css` 骨架

```css
@import "tailwindcss";

:root {
  /* 品牌 */
  --primary: #DBAA22;
  --primary-foreground: #FFFFFF;
  --ring: var(--primary);
  --sidebar-primary: var(--primary);
  --sidebar-primary-foreground: #FFFFFF;

  /* 页面 */
  --background: #FAFAFA;
  --foreground: #0a0a0a;

  /* 中性 */
  --muted: #f5f5f5;
  --muted-foreground: #737373;
  --card: #fafafa;
  --card-foreground: #0a0a0a;
  --popover: #fafafa;
  --popover-foreground: #0a0a0a;
  --secondary: #f5f5f5;
  --secondary-foreground: #0a0a0a;
  --accent: #f5f5f5;
  --accent-foreground: #0a0a0a;

  /* 边框 */
  --border: #e5e5e5;
  --input: #e5e5e5;

  /* 侧栏 */
  --sidebar: #FBF6E6;           /* mix(--primary, #fff, 93%) */
  --sidebar-accent: #E9DEB2;    /* darken(--sidebar, 8%) */
  --sidebar-accent-foreground: #0a0a0a;
  --sidebar-foreground: #0a0a0a;
  --sidebar-border: #E1DAC6;
  --sidebar-ring: #71717a;

  /* 语义 */
  --destructive: #e7000b;
  --destructive-foreground: #FFFFFF;

  /* 字体 */
  --font-sans: -apple-system, BlinkMacSystemFont, "PingFang SC",
               "Microsoft YaHei", Inter, "Segoe UI", system-ui, sans-serif;
  --font-mono: "SF Mono", "Fira Code", "JetBrains Mono", Menlo, monospace;

  /* 圆角 */
  --radius-sm: 4px;
  --radius: 6px;
  --radius-md: 8px;
  --radius-lg: 12px;
  --radius-xl: 16px;
}

@theme inline {
  --color-primary: var(--primary);
  --color-primary-foreground: var(--primary-foreground);
  --color-background: var(--background);
  --color-foreground: var(--foreground);
  --color-muted: var(--muted);
  --color-muted-foreground: var(--muted-foreground);
  --color-card: var(--card);
  --color-card-foreground: var(--card-foreground);
  --color-popover: var(--popover);
  --color-popover-foreground: var(--popover-foreground);
  --color-secondary: var(--secondary);
  --color-secondary-foreground: var(--secondary-foreground);
  --color-accent: var(--accent);
  --color-accent-foreground: var(--accent-foreground);
  --color-border: var(--border);
  --color-input: var(--input);
  --color-ring: var(--ring);
  --color-sidebar: var(--sidebar);
  --color-sidebar-accent: var(--sidebar-accent);
  --color-sidebar-accent-foreground: var(--sidebar-accent-foreground);
  --color-sidebar-foreground: var(--sidebar-foreground);
  --color-sidebar-border: var(--sidebar-border);
  --color-sidebar-primary: var(--sidebar-primary);
  --color-sidebar-primary-foreground: var(--sidebar-primary-foreground);
  --color-sidebar-ring: var(--sidebar-ring);
  --color-destructive: var(--destructive);
  --color-destructive-foreground: var(--destructive-foreground);
  --font-sans: var(--font-sans);
  --font-mono: var(--font-mono);
  --radius-sm: var(--radius-sm);
  --radius: var(--radius);
  --radius-md: var(--radius-md);
  --radius-lg: var(--radius-lg);
  --radius-xl: var(--radius-xl);
}

* { box-sizing: border-box; }
html { overscroll-behavior: none; }

body, #root {
  background: var(--background);
  color: var(--foreground);
  font-family: var(--font-sans);
  font-size: 13px;
  line-height: 1.6;
  -webkit-font-smoothing: antialiased;
}

/* ...scrollbar / selection / titlebar drag ... */
```

## 4. 信息架构与路由

### 4.1 全局布局

```
┌──────────────────────────────────────────────────┐
│  macOS titlebar drag region（透明）              │
├───────────────┬──────────────────────────────────┤
│               │                                  │
│   Sidebar     │         Main Panel              │
│   (256px)     │        (fill_container)          │
│               │                                  │
└───────────────┴──────────────────────────────────┘
```

### 4.2 侧栏结构（对齐 design.pen `PV1ln`）

```
┌─── Sidebar ───┐
│ [租户 Header] │   品牌 logo + 租户名 + 下拉
├───────────────┤
│ ✨ 新任务      │   主导航 1（默认选中）
│ 🧩 技能中心    │   主导航 2
│ ⏰ 定时任务    │   主导航 3
├───────────────┤
│ "任务"（小标题） │
│ ▾ 默认项目     │   项目折叠组
│    · 对话 1    │
│ ▾ Desktop      │
│    · 对话 A    │
├───────────────┤
│      (spacer)  │
│ ⚙ 设置         │   固定底部
└───────────────┘
```

### 4.3 路由表

路由采用 **Jotai atom 派生**的简单路由状态（不引入 react-router），状态保存在 `uiStore.route` 上：

```ts
type Route =
  | { kind: 'home' }
  | { kind: 'skill-center' }
  | { kind: 'skill-detail', skillId: string }
  | { kind: 'schedules' }
  | { kind: 'chat', conversationId: string }

// 设置 Modal 是独立的 atom（和路由正交）
type SettingsModalState = null | 'account' | 'general' | 'about' | 'usage'
```

| 路由 | 主面板组件 | 入口 |
|---|---|---|
| `home` | `<HomePage />` | 侧栏"新任务"、"+ 新对话" 按钮 |
| `skill-center` | `<SkillCenterPage />` | 侧栏"技能中心" |
| `skill-detail` | `<SkillDetailPage />` | 点击技能卡 |
| `schedules` | `<SchedulesPage />` | 侧栏"定时任务" |
| `chat` | `<ChatPage />` | 侧栏对话项 |
| settings（Modal） | `<SettingsModal />` | 侧栏"设置"按钮 |

### 4.4 顶层渲染

```tsx
function App() {
  return (
    <AuthGate>                       {/* 未登录 → <LoginPage /> */}
      <AppShell>
        <Sidebar />
        <MainPanel>
          {/* 根据 route atom 派生主面板内容 */}
          <RouteSwitch />
        </MainPanel>
        <SettingsModal />           {/* 全局 overlay */}
      </AppShell>
    </AuthGate>
  )
}
```

### 4.5 第一期页面清单

| 页面 | design.pen node | 备注 |
|---|---|---|
| 登录页 | `epkyz` | 全屏 gate |
| 首页（新任务） | `2cYHh` | 大输入框 + 分类 Tab + 推荐提示词 |
| 技能中心 | `dVE8r` | 分类网格 + 技能卡 + 市场/上传按钮 |
| 技能详情 | `cSdAy` | 详情 + workflow + CTA |
| 定时任务 | `s8Rc7` | 推荐模板 + 任务表（第一期可先做静态） |
| 对话页 | `9qve3` + `ju2pU` | 保留现有对话能力，迁移视觉 |
| 设置 Modal | `S3D6p` + `1MCFZ` + `az6ZY` | 三屏（设置 / 关于 / 用量） |

## 5. 登录 Gate 与状态流

### 5.1 状态机

```
          启动 App
             │
             ▼
      getCloudAuth() ──fail/expired──┐
             │                       │
         success                     │
             │                       │
             ▼                       ▼
      isLoggedIn=true          isLoggedIn=false
        applyBranding()              │
        fetchCloudModels()           │
             │                       │
             ▼                       ▼
        AppShell                LoginPage
             │                       │
             │                    login()
             │                       │
             │                  success ──┐
             │                             │
             │         ┌───────────────────┘
             │         ▼
             │    restoreRoute() ← 从 authStore.redirectFrom 读
             │         │
             ▼         ▼
         continue work
             │
        onAuthExpired (后端推送)
             │
             ▼
      capture current route → authStore.redirectFrom
      clearAuth()
             │
             ▼
         LoginPage
```

### 5.2 `authStore` 扩展

```ts
interface AuthState {
  isLoggedIn: boolean
  user: User | null
  tenant: Tenant | null
  cloudModels: CloudModel[]
  selectedCloudModel: string | null
  // 新增：
  redirectFrom: Route | null      // 登录失效时记录上次位置
  isAuthPending: boolean          // 启动恢复中 / 登录中

  login(payload): Promise<void>
  logout(): Promise<void>
  restoreFromStorage(): Promise<void>
  clearAndRedirect(route?: Route): void
}
```

### 5.3 LoginPage 要素（按 design.pen `epkyz`）

- 居中单卡片：品牌 logo（`logoUrl`）+ 产品名（`productName`）+ 账号输入 + 密码输入 + 登录按钮
- 卡片宽度固定 400px，居中，背景 `--background`
- 错误反馈：卡内 `<Alert variant="destructive">`；保留已填内容；密码字段清空
- 无注册入口、无忘记密码、无第三方登录（企业租户由管理员开账号）

### 5.4 session-restore 时序

```
1. 渲染时 isAuthPending=true → <FullscreenLoader />（< 500ms）
2. 并发：
   - authStore.restoreFromStorage() → getCloudAuth()
   - skillStore.reload() → listSkills() （和登录无关，可提前）
3. getCloudAuth 返回：
   3.1 成功：setUser + setTenant + applyBranding + fetchCloudModels
   3.2 失败：保持 isLoggedIn=false
4. isAuthPending=false，渲染正常组件树
5. AuthGate 根据 isLoggedIn 决定显示 LoginPage 还是 AppShell
```

### 5.5 退出登录

- 设置 Modal 里 `<Button variant="destructive">退出登录</Button>`
- 点击 → `<AlertDialog>` 二次确认
- 确认 → `logout()` → `clearAuth()` → `redirectFrom = null`（主动退出不保留现场）→ LoginPage

### 5.6 删除的内容

| 模块 | 处理 |
|---|---|
| `settingsStore.useCloud` | 删除 |
| `settingsStore.cloudModel` / `cloudModelType` | 保留（登录后生效）|
| `LoginSection.tsx` 中的云端-本地 toggle UI | 删除 |
| `SettingsModal` 的 Models / Search Tab 的"未登录特殊分支" | 删除（改为：未登录根本进不来设置） |

## 6. 技能中心

### 6.1 分类数据（前端写死）

`src/data/skill-categories.ts`：

```ts
export const SKILL_CATEGORIES: SkillCategory[] = [
  { id: 'general',    name: '通用工具',     icon: 'wrench' },
  { id: 'ecommerce',  name: '电商',         icon: 'shopping-cart' },
  { id: 'finance',    name: '门店与财务',   icon: 'store' },
  { id: 'design',     name: '设计与制造',   icon: 'pencil-ruler' },
  { id: 'dev',        name: '开发',         icon: 'code' },
  { id: 'legal',      name: '律所',         icon: 'scale' },
  { id: 'media',      name: '媒介',         icon: 'megaphone' },
  { id: 'health',     name: '健康与学习',   icon: 'heart-pulse' },
  { id: 'ops',        name: '运营',         icon: 'trending-up' },
  { id: 'content',    name: '内容创作',     icon: 'feather' },
]

// UI 层在 CATEGORIES 之前额外插入一个虚拟分类"为你推荐"（id: 'recommended'）
```

### 6.2 Skill 数据结构（和后端 SkillInfo 一致，第一期不新增字段）

```ts
interface SkillInfo {
  id: string
  displayName: string
  description: string
  source: 'builtin' | 'user'
  hasWorkflow: boolean
  icon?: string
  category?: CategoryId         // 后端已有，用于分类
  triggerText?: string
  shortDescription?: string
  displayNameEn?: string
  shortDescriptionEn?: string
}
```

### 6.3 skillStore

```ts
interface SkillState {
  skills: SkillInfo[]
  recommendedIds: string[]      // 第一期可以先写死 5 个内置技能的 id
  isLoading: boolean

  listByCategory(id: CategoryId | 'recommended'): SkillInfo[]
  getById(id: string): SkillInfo | null

  reload(): Promise<void>       // 调 tauri invoke('list_skills')
  install(id: string): Promise<void>    // 第一期仅调桩函数，UI 存在
  uninstall(id: string): Promise<void>
  upload(file: File): Promise<void>
}
```

### 6.4 关键组件

| 组件 | 位置 | 说明 |
|---|---|---|
| `SkillCenterPage` | `features/skill-center` | 顶部分类 Tab + 右上角"技能市场"和"上传"按钮；中间技能卡网格 |
| `SkillCard` | `components/skill/SkillCard.tsx` | Icon + name + short description + badge（已安装 / 内置） |
| `SkillDetailPage` | `features/skill-detail` | 详细描述 + workflow 步骤预览 + "开始使用"CTA + "卸载"或"上传新版本" |
| `SkillMarketModal` | `features/skill-center/market` | 第一期**骨架**：打开后显示"即将开放"占位，保留布局 |
| `SkillUploadModal` | `features/skill-center/upload` | 第一期**骨架**：`<input type="file">` 可选文件，点提交后 toast "即将开放" |
| `SkillPopover` | `features/chat/composer` | 对话页内的技能快捷弹层，展示已安装 + "去技能中心"入口 |

### 6.5 CTA 行为

- 技能中心点"使用" → 新建对话（`createConversation(skillId)`）→ 切到 `chat:<新id>`
- 不再有 Persona / AgentSelector

### 6.6 删除的前端模块（本期）

| 模块 | 处理 |
|---|---|
| `stores/personaStore.ts` | **删除** |
| `components/chat/AgentSelector.tsx` | **删除** |
| `SettingsModal` 的 Persona Tab | **删除** |
| `lib/tauri.ts` 的 `listAgents` / `getActivePersona` / `setActivePersona` | 前端停止引用；IPC 封装保留（后端一期不动，不破兼容） |

## 7. 组件库：shadcn/ui

### 7.1 引入方式

- 通过 shadcn CLI 在仓库内生成组件：`npx shadcn@latest add button card dialog ...`
- 组件文件落在 `src/components/ui/*`，受仓库管理，可按需改 variant（CVA）对齐 design.pen
- 不引入 shadcn 作为 npm 依赖；但会引入 Radix UI / CVA / lucide-react 等底层依赖

### 7.2 第一期要引入的组件清单

| 类别 | shadcn 组件 | 替代现有 |
|---|---|---|
| 基础 | Button / Input / Textarea / Label / Select / Combobox / Switch / Checkbox / Radio-group | `components/common/Button.tsx` 等 |
| 布局 | Sidebar / Separator / Scroll-area | — |
| 反馈 | Alert / Alert-dialog / Sonner（Toast） / Progress / Skeleton | — |
| 容器 | Card / Dialog / Sheet / Popover / Tooltip | — |
| 导航 | Tabs / Dropdown-menu / Context-menu / Breadcrumb / Pagination | — |
| 数据 | Table / Data-table / Badge / Avatar / Accordion / Collapsible | `common/Badge.tsx` 等 |
| 表单 | Form（react-hook-form + zod） | — |

### 7.3 迁移节奏

1. **Phase A**：引入 shadcn 基础设施 + 核心组件（Button、Card、Dialog、Sidebar、Tabs、Input、Tooltip、Dropdown、Badge、Separator、ScrollArea、Alert、Sonner）
2. **Phase B**：新建页面（`features/home`、`skill-center`、`skill-detail`、`schedules`、`auth/login-page`）**直接用 shadcn**
3. **Phase C**：老页面（对话页、设置 Modal）**逐块替换**，不新建混用
4. **Phase D**：`common/` 清理被替代的组件，保留业务特有（`PermissionAskDialog`、`ToolCallCard` 等）

### 7.4 与 design.pen 对齐

- Pencil reusable ID（如 `VSnC2` = Button/Default）和 shadcn `<Button variant="default">` 一一对应
- 视觉属性（padding / gap / radius / shadow）通过 Pencil MCP `batch_get(readDepth >= 3)` 取出，若 shadcn 默认值不一致，改 `cva()` variants 匹配 Pencil（不改 Token）
- 圆角：`--radius` = 6px（对齐 Pencil 的 Button 默认），`--radius-lg` = 12px（对齐 Card）

## 8. 测试与验收

### 8.1 测试策略

1. **前端单测 Vitest**：
   - `stores/brandingStore.test.ts` — 覆盖：租户切换、深/浅 accentColor、派生算法正确、reset、登录后 setProperty 的 key 数量
   - `styles/skin.test.ts` — 纯函数：mix/darken/lighten/isDarkColor 边界
   - `stores/authStore.test.ts` — login/logout/expired/redirectFrom 恢复
   - `stores/skillStore.test.ts` — listByCategory / getById / reload 路径
2. **组件集成**：
   - `AuthGate.integration.test.tsx` — 未登录渲染 LoginPage；成功后切回 redirectFrom；冷启动去 home
   - `SkillCenterPage.integration.test.tsx` — 分类 Tab 切换、卡片 CTA 路由跳转
3. **视觉对比**（qob 规则）：
   - 关键页面 Playwright 截图 + Pencil MCP `get_screenshot(nodeId)` 并排比对
   - 覆盖：登录页 / 首页 / 技能中心 / 技能详情 / 对话页空态 + 技能弹层 / 设置 Modal 三屏 / 定时任务

### 8.2 DoD（Definition of Done）

- [ ] `pnpm lint` / `pnpm typecheck` / `pnpm test` 全通过
- [ ] `pnpm exec vitest run src/lib/tauri.events.test.ts src/hooks/useStreaming.integration.test.tsx src/stores/chatStore.test.ts` 不回归
- [ ] 金色默认租户视觉和 design.pen `2cYHh` 截图肉眼对比 ≥ 95% 接近
- [ ] 切极端租户（`accentColor = #960505` / `#1A2E22`）应用不崩溃、侧栏色派生生效、极暗主色下侧栏分隔线可见
- [ ] 未登录启动 → 强制显示登录页
- [ ] 登录成功 → 进首页（冷启动）或原位置（有 redirectFrom）
- [ ] 模拟 token 过期 → 回登录页且保留 redirectFrom；重新登录 → 回原位置
- [ ] 技能中心显示 10 个分类 + 分类下技能卡；技能市场 / 上传弹层骨架存在
- [ ] 全仓搜索零硬编码颜色、零 inline style 色值、零 JS hover
- [ ] `personaStore.ts` / `AgentSelector.tsx` / `useCloud` 相关 UI 已删
- [ ] 本 spec 末尾的 Review Findings / Open Questions / Decision Log 区块维护

### 8.3 实施节奏（建议，交给 writing-plans 时细化）

1. **Phase 1 · Token & 主题**：`globals.css` 重写 + `skin.ts` + `brandingStore` 重写 + 单测（约 1.5 天）
2. **Phase 2 · shadcn 基础设施 + AppShell 骨架**：引入 shadcn、Sidebar、AppShell、路由、AuthGate + LoginPage（约 3 天）
3. **Phase 3 · 新页面**：首页 / 技能中心 / 技能详情 / 定时任务（约 4-5 天）
4. **Phase 4 · 对话页 + 设置 Modal 迁移**：功能保留、组件替换、视觉对齐（约 3-4 天）
5. **Phase 5 · 清理 & DoD 验收**：删 persona/agent/useCloud、视觉走查、补单测（约 1.5 天）

**总估**：前端 **13-15 天**（单人），不含后端协调、code review、设计稿微调。

### 8.4 Out-of-Scope（本 spec 不管）

- 后端 Skill 执行链路打通（B1 问题 — 另起 plan）
- Persona → Skill 数据迁移（后端）
- Agent 体系吸收合并（后端）
- 定时任务后端接口
- 技能市场下载 / 安装 / 上传的后端实装
- dark mode UI 切换（Token 预留，不做）

## 9. Review Findings

（待 review 后填入）

## 10. Open Questions

- Q1：技能中心"为你推荐"分类的数据来源——第一期写死 5 个技能 id？按什么规则挑选？（提议：挑最常用的 5 个内置技能，由 `src/data/recommended-skills.ts` 写死，可后续接后端）
- Q2：`fontFamily` 字段是否保留租户定制？现有 FONT_MAP 有 system/kai/mono 三档，设计稿只用 system（苹方 + Inter）。是否允许租户下发非预设字体？（提议：保留现有白名单 FONT_MAP，租户只能选这三档之一）
- Q3：定时任务页第一期实装范围？design.pen 有 `s8Rc7` 视觉稿但后端无接口。是否做"纯静态页面 + 占位数据"？（提议：是，和技能市场一样做骨架，后端就位再接）

## 11. Decision Log

- 2026-04-22 · Token 命名从"qob 风格分命名空间"改为"shadcn 原生命名"（和 design.pen 零映射对齐；引入 shadcn/ui 组件库）
- 2026-04-22 · 皮肤机制从"primaryColor + accentColor + bgColor + sidebarBgColor 四字段"收敛到"仅 accentColor"，其他派生
- 2026-04-22 · 侧栏色从"硬编码 #F4F0E6"改为"`mix(primary, white, 93%)` 派生"
- 2026-04-22 · `design.pen` 里 `--sidebar-primary` 已被用户把全 8 个 Accent 轴固定为 `#DBAA22`，深蓝 `#143290` 全局消失
- 2026-04-22 · Token 分层预留 dark 值结构，MVP 只发 light，不暴露切换入口
- 2026-04-22 · Persona + AgentSelector 本期从前端删除；前端概念统一到 Skill
- 2026-04-22 · 分类前端写死 10 个（与 2026-04-21 spec 一致）
- 2026-04-22 · 技能市场 / 上传弹层第一期只做 UI 骨架
- 2026-04-22 · 登录成为全屏 Gate；失效保留现场、主动退出不保留
