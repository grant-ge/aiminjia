# 前端视觉重构 · plan-E：Auth & Verification 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 完成登录页视觉重构，建立可重复执行的 UI 截图脚本，按 spec 第 9.2 节 30 个检查点完成 10 页人工对照走查，达到本轮 DoD。

**Architecture:** 4 个 Auth 组合组件 + 重写 LoginPage；新增 `scripts/capture-ui.mjs` 用 Playwright 在 1280×fixed 窗口逐页截图；新增 `docs/superpowers/specs/assets/verification-checklist.md` 作为人工走查模板。

**Tech Stack:** 同前。新增 dev-dependency `playwright` 与 `@playwright/test`（若未装）。

**对应 spec：** `docs/superpowers/specs/2026-04-23-frontend-visual-realignment-to-design-pen.md` 第 5.7、7.8、9.1、9.2、11 章。

**前置：** plan-A、B、C、D 已完成。分支 `pzc`。

---

## 文件结构

### 新建

| 路径 | 责任 |
|---|---|
| `src/components/auth/LoginLogoStack.tsx` | logo 56 圆 + brand name 22/600 + gap 10 |
| `src/components/auth/LoginCard.tsx` | r-18 padding [40,40,32,40] gap 20 width 460 border 1 bg card |
| `src/components/auth/LoginOptionsRow.tsx` | "记住我 + 忘记密码" 行 |
| `src/components/auth/LoginFooter.tsx` | 12px muted 版本/版权脚注 |
| `src/components/auth/__tests__/LoginCard.test.tsx` | 渲染 + 子节点 slot |
| `scripts/capture-ui.mjs` | Playwright 截图脚本：1280×900 viewport 截 10 页 |
| `docs/superpowers/specs/assets/verification-checklist.md` | 30 检查点对照模板 |

### 修改

| 路径 | 修改内容 |
|---|---|
| `src/components/auth/LoginPage.tsx` | 完全重写为 4 组件拼装 + tocWrap + footer |
| `package.json` | 新增 `"capture:ui": "node scripts/capture-ui.mjs"`；若未装 playwright，`devDependencies` 加 `playwright` |

---

## Task E-1.1：Auth 四组合组件

**Files:**
- Create: `src/components/auth/LoginLogoStack.tsx`
- Create: `src/components/auth/LoginCard.tsx`
- Create: `src/components/auth/LoginOptionsRow.tsx`
- Create: `src/components/auth/LoginFooter.tsx`
- Create: `src/components/auth/__tests__/LoginCard.test.tsx`

- [ ] **Step 1：写失败测试**

```tsx
// src/components/auth/__tests__/LoginCard.test.tsx
import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import { LoginCard } from '../LoginCard'

describe('LoginCard', () => {
  it('renders children inside', () => {
    render(
      <LoginCard>
        <div>form-slot</div>
      </LoginCard>,
    )
    expect(screen.getByText('form-slot')).toBeInTheDocument()
  })

  it('uses width 460 r-18 border 1 bg-card', () => {
    const { container } = render(
      <LoginCard>
        <div />
      </LoginCard>,
    )
    const card = container.querySelector('[data-testid="login-card"]')
    expect(card?.className).toMatch(/w-\[460px\]/)
    expect(card?.className).toMatch(/rounded-\[18px\]/)
    expect(card?.className).toMatch(/border/)
    expect(card?.className).toMatch(/bg-card/)
  })
})
```

- [ ] **Step 2：运行确认失败**

```bash
pnpm exec vitest run src/components/auth/__tests__/LoginCard.test.tsx
```

Expected: FAIL（组件不存在）。

- [ ] **Step 3：实现四个组件**

```tsx
// src/components/auth/LoginLogoStack.tsx
/**
 * @designSource design.pen#TSZyx
 * @sizing logo 56×56 r-28; brand 22/600; gap 10
 */
interface LoginLogoStackProps {
  logoUrl: string
  brandName: string
}

export function LoginLogoStack({ logoUrl, brandName }: LoginLogoStackProps) {
  return (
    <div className="flex flex-col items-center gap-2.5">
      <div className="h-14 w-14 overflow-hidden rounded-full">
        <img src={logoUrl} alt="" className="h-full w-full object-cover" />
      </div>
      <div className="text-[22px] font-semibold text-foreground">{brandName}</div>
    </div>
  )
}
```

```tsx
// src/components/auth/LoginCard.tsx
/**
 * @designSource design.pen#PFEwh
 * @sizing w 460 r-18 padding [40,40,32,40] gap 20 border 1 bg card
 */
import type { PropsWithChildren } from 'react'

export function LoginCard({ children }: PropsWithChildren) {
  return (
    <div
      data-testid="login-card"
      className="flex w-[460px] flex-col gap-5 rounded-[18px] border border-border bg-card px-10 pb-8 pt-10"
    >
      {children}
    </div>
  )
}
```

```tsx
// src/components/auth/LoginOptionsRow.tsx
/**
 * @designSource design.pen#hfGT2
 * @sizing space-between; "忘记密码" fontSize 13 / 500 color brand-secondary
 */
import type { ReactNode } from 'react'

interface LoginOptionsRowProps {
  rememberSlot: ReactNode
  onForget: () => void
}

export function LoginOptionsRow({ rememberSlot, onForget }: LoginOptionsRowProps) {
  return (
    <div className="flex w-full items-center justify-between">
      {rememberSlot}
      <button
        type="button"
        onClick={onForget}
        className="text-[13px] font-medium text-brand-secondary transition-colors hover:opacity-80"
      >
        忘记密码？
      </button>
    </div>
  )
}
```

```tsx
// src/components/auth/LoginFooter.tsx
/**
 * @designSource design.pen#wJSL6
 * @sizing fontSize 12 muted
 */
interface LoginFooterProps {
  text: string
}

export function LoginFooter({ text }: LoginFooterProps) {
  return <div className="text-[12px] text-muted-foreground">{text}</div>
}
```

- [ ] **Step 4：测试通过**

```bash
pnpm exec vitest run src/components/auth/__tests__/LoginCard.test.tsx
```

Expected: PASS。

- [ ] **Step 5：commit**

```bash
git add src/components/auth/LoginLogoStack.tsx src/components/auth/LoginCard.tsx src/components/auth/LoginOptionsRow.tsx src/components/auth/LoginFooter.tsx src/components/auth/__tests__/LoginCard.test.tsx
git commit -m "feat(frontend): add auth composite components"
```

---

## Task E-1.2：重写 LoginPage

**Files:**
- Modify: `src/components/auth/LoginPage.tsx`

- [ ] **Step 1：替换实现**

```tsx
/**
 * @designSource design.pen#epkyz
 * @sizing page bg --background, gap 24, centered; card 460
 */
import { type FormEvent, useState } from 'react'

import { LoginCard } from '@/components/auth/LoginCard'
import { LoginFooter } from '@/components/auth/LoginFooter'
import { LoginLogoStack } from '@/components/auth/LoginLogoStack'
import { LoginOptionsRow } from '@/components/auth/LoginOptionsRow'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { useAuthStore } from '@/stores/authStore'
import { useBrandingStore } from '@/stores/brandingStore'

export function LoginPage() {
  const login = useAuthStore((s) => s.login)
  const isAuthPending = useAuthStore((s) => s.isAuthPending)
  const productName = useBrandingStore((s) => s.productName)
  const logoUrl = useBrandingStore((s) => s.logoUrl)
  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')
  const [remember, setRemember] = useState(true)
  const [error, setError] = useState('')

  async function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    try {
      setError('')
      await login(username, password)
    } catch (err) {
      setPassword('')
      setError(err instanceof Error ? err.message : '登录失败，请重试')
    }
  }

  return (
    <div className="flex min-h-screen w-full flex-col items-center justify-center gap-6 bg-background px-6">
      <LoginLogoStack logoUrl={logoUrl} brandName={productName} />
      <LoginCard>
        <div className="flex flex-col gap-1.5">
          <div className="text-[20px] font-semibold text-foreground">登录到 {productName}</div>
          <div className="text-[13px] text-muted-foreground">使用企业账号继续</div>
        </div>
        <form className="flex flex-col gap-5" onSubmit={handleSubmit}>
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="username">账号</Label>
            <Input
              id="username"
              placeholder="请输入企业账号"
              value={username}
              onChange={(e) => setUsername(e.target.value)}
            />
          </div>
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="password">密码</Label>
            <Input
              id="password"
              type="password"
              placeholder="请输入密码"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
            />
          </div>
          <LoginOptionsRow
            rememberSlot={
              <label className="flex items-center gap-2 text-[13px] text-foreground">
                <Checkbox
                  checked={remember}
                  onCheckedChange={(v) => setRemember(Boolean(v))}
                />
                记住我
              </label>
            }
            onForget={() => {}}
          />
          {error ? (
            <div className="text-[13px] text-destructive">{error}</div>
          ) : null}
          <Button
            type="submit"
            disabled={isAuthPending}
            className="w-full rounded-full py-3 text-[15px] font-semibold"
          >
            登录
          </Button>
          <div className="text-center text-[12px] text-muted-foreground">
            登录即代表同意《服务条款》与《隐私政策》
          </div>
        </form>
      </LoginCard>
      <LoginFooter text="AI 小家 v0.9.30 · © 仁励家网络科技(杭州)有限公司" />
    </div>
  )
}
```

- [ ] **Step 2：lint + tsc + 全测**

```bash
pnpm exec tsc --noEmit
pnpm test
pnpm lint
```

Expected: 0 error / 全 PASS（含 `AuthGate.integration.test.tsx`）。如果集成测试因为新结构里多了"使用企业账号继续"等文字而需更新选择器，按最小修改原则更新。

- [ ] **Step 3：commit**

```bash
git add src/components/auth/LoginPage.tsx
git commit -m "refactor(frontend): rebuild LoginPage to design.pen layout"
```

---

## Task E-2：Playwright 截图脚本

**Files:**
- Create: `scripts/capture-ui.mjs`
- Modify: `package.json`

- [ ] **Step 1：确认 Playwright ��否已安装**

```bash
pnpm list playwright 2>/dev/null
```

如果未安装：

```bash
pnpm add -D playwright
pnpm exec playwright install chromium
```

- [ ] **Step 2：写脚本**

```js
// scripts/capture-ui.mjs
import { mkdir } from 'node:fs/promises'
import path from 'node:path'

import { chromium } from 'playwright'

const OUT_DIR = path.resolve(process.cwd(), 'tmp/ui-capture')
const BASE_URL = process.env.UI_CAPTURE_BASE || 'http://localhost:1420'
const VIEWPORT = { width: 1280, height: 900 }

const PAGES = [
  { name: 'home', path: '/?ui=home' },
  { name: 'chat-long', path: '/?ui=chat-long' },
  { name: 'chat-skill-popover', path: '/?ui=chat-skill-popover' },
  { name: 'skill-center', path: '/?ui=skill-center' },
  { name: 'skill-detail', path: '/?ui=skill-detail' },
  { name: 'schedules', path: '/?ui=schedules' },
  { name: 'settings-account', path: '/?ui=settings-account' },
  { name: 'settings-about', path: '/?ui=settings-about' },
  { name: 'settings-usage', path: '/?ui=settings-usage' },
  { name: 'login', path: '/?ui=login' },
]

async function main() {
  await mkdir(OUT_DIR, { recursive: true })
  const browser = await chromium.launch()
  const ctx = await browser.newContext({ viewport: VIEWPORT })
  const page = await ctx.newPage()
  for (const p of PAGES) {
    const url = BASE_URL + p.path
    console.log(`[capture] ${p.name} → ${url}`)
    try {
      await page.goto(url, { waitUntil: 'networkidle', timeout: 15_000 })
    } catch (err) {
      console.warn(`[capture] navigation issue for ${p.name}, continuing: ${err.message}`)
    }
    await page.waitForTimeout(800)
    const file = path.join(OUT_DIR, `${p.name}.png`)
    await page.screenshot({ path: file, fullPage: false })
    console.log(`[capture] saved ${file}`)
  }
  await browser.close()
}

main().catch((err) => {
  console.error(err)
  process.exit(1)
})
```

> 注：`?ui=...` 是预留的硬编码路由钩子，用于在 dev server 直接落到对应页面/状态。如果应用尚未支持这个 query param，本步保留脚本结构，验收阶段也可以用 dev 启动后**手工**导航到对应页面再触发 `await page.screenshot(...)`。脚本上方注释里加一行说明：

```js
// 备选：如果应用不支持 ?ui= 路由钩子，可注释 PAGES 数组改为单页捕获，
// 并在 dev server 中手工切换到目标页后调 `node -e "..."` 单独截图。
```

- [ ] **Step 3：在 package.json 加脚本**

```json
{
  "scripts": {
    "capture:ui": "node scripts/capture-ui.mjs"
  }
}
```

- [ ] **Step 4：commit**

```bash
git add scripts/capture-ui.mjs package.json
git commit -m "chore(frontend): add capture-ui playwright script for visual review"
```

---

## Task E-3：30 检查点对照清单

**Files:**
- Create: `docs/superpowers/specs/assets/verification-checklist.md`

- [ ] **Step 1：写清单（人工填写用，本任务只产模板）**

```markdown
# Plan-E 视觉对照清单

执行方式：
1. `pnpm tauri:dev` 启动应用，依次切换到下方 10 个页面
2. 每页用 Cmd+Shift+S 或 `pnpm capture:ui` 截图到 `tmp/ui-capture/<name>.png`
3. 与 `docs/superpowers/specs/assets/design-pen-exports/<name>.png` 对比
4. 在每页 3 个检查点上打 ✓ / ✗，✗ 必须附记问题与修复 commit hash

| # | 页面 | 稿 | 实现 | 检查点 1 | 检查点 2 | 检查点 3 | 备注 |
|---|---|---|---|---|---|---|---|
| 1 | 首页 | home.png | home.png | ☐ mascot 64 圆居中标题上 | ☐ "为你推荐" chip 金底激活含 sparkles | ☐ 三态行卡 iconBox 底色按 variant | |
| 2 | 聊天长对话 | chat-long.png | chat-long.png | ☐ 用户气泡金底右对齐 max 80% | ☐ ToolGroup 顶栏绿 check + 已完成 N 步 + 时长 | ☐ GeneratedFileCard 右侧 Microsoft Excel pill | |
| 3 | 聊天技能弹层 | chat-skill-popover.png | chat-skill-popover.png | ☐ popover 锚在 composer 上方 | ☐ 头部 "管理已安装的技能" | ☐ 行 padding [10,16] 左右标题/标签 | |
| 4 | 技能中心 | skill-center.png | skill-center.png | ☐ TopBar 右上 "技能市场 / 上传技能" | ☐ "热门推荐" 15/600 + 网格 gap 16 | ☐ "办公效率" 分类条 + 网格 | |
| 5 | 技能详情 | skill-detail.png | skill-detail.png | ☐ heroIc 88×88 底 brand-primary-subtle | ☐ meta 行 gap 48（来源/更新时间） | ☐ 右上"禁用 outline" + "使用 primary" 按钮组 | |
| 6 | 定时任务 | schedules.png | schedules.png | ☐ 3 张模板卡 padding 18 gap 16 | ☐ 列表卡 header padding [16,20] | ☐ 空态居中 h 280 | |
| 7 | 设置账户 | settings-account.png | settings-account.png | ☐ Modal 980×680 居中 + 半透明遮罩 | ☐ 左 220 menu "账户" 激活白底 | ☐ 账户卡 secondary 底 r-14 退出按钮 outline | |
| 8 | 设置关于 | settings-about.png | settings-about.png | ☐ "关于 AI 小家" 激活 | ☐ appCard 平铺 padding 20 | ☐ 帮助/开发者两段 gap 16 | |
| 9 | 设置用量 | settings-usage.png | settings-usage.png | ☐ "用量" 激活 | ☐ planCard 底 border 1 | ☐ quota 进度条 + detail 列 | |
| 10 | 登录 | login.png | login.png | ☐ logo 56 圆 + brand 22/600 | ☐ Card 460 r-18 padding [40,40,32,40] | ☐ 登录按钮 r-999 金底 fontSize 15/600 | |

完成后：
- 所有 30 项 ✓ 视为本轮 plan-E DoD 通过；
- 任意 ✗ 必须先修复并补 commit hash，再回到本表打 ✓。
```

- [ ] **Step 2：commit**

```bash
git add docs/superpowers/specs/assets/verification-checklist.md
git commit -m "docs(frontend): add 30-point visual verification checklist"
```

---

## Task E-4：执行 30 检查点走查并修复

**Files:** 由走查结果决定

- [ ] **Step 1：拉起 dev**

```bash
pnpm tauri:dev
```

- [ ] **Step 2：跑截图脚本**

```bash
pnpm capture:ui
```

或手工逐页截图到 `tmp/ui-capture/`。

- [ ] **Step 3：逐页对照、填表、修复**

对每一行 30 个检查点，发现 ✗ 时：
1. 打开对应组件文件（搜索 `@designSource design.pen#<node-id>` 找回稿子参考）；
2. 按设计稿改样式常量；
3. 重新截图、目视确认；
4. 在 `verification-checklist.md` 把 ☐ 改成 ✓ 并写 commit hash；
5. commit 信息形如：`fix(frontend): align <component> <检查点> per checklist`。

- [ ] **Step 4：当全部 ✓ 后，commit checklist 最终版**

```bash
git add docs/superpowers/specs/assets/verification-checklist.md
git commit -m "docs(frontend): complete 30-point visual checklist (all green)"
```

---

## Task E-Final：阶段 E 验收 + 整体 DoD

- [ ] **Step 1：跑全套测试 + lint + tsc**

```bash
pnpm test
pnpm lint
pnpm exec tsc --noEmit
```

Expected: 全 PASS / 0 error。

- [ ] **Step 2：核对 DoD（spec 第 11 章）**

逐条核对：

1. ☐ `docs/superpowers/specs/assets/design-pen-exports/` 下 10 张基线 PNG 入库（plan-A Task A-0）
2. ☐ `tmp/ui-capture/` 脚本可产 10 张实现 PNG（plan-E Task E-2）
3. ☐ 30 检查点全部 ✓（plan-E Task E-4）
4. ☐ 所有组合组件文件顶部带 `@designSource` JSDoc（plan-A/B/C/D 各任务已加）
5. ☐ 页面层文件 ≤ 120 行（plan-B/C/D 已规约；超出文件必须在最终 PR 描述里解释）
6. ☐ Token 层只剩 Light + Neutral + Default（plan-A Task A-1）
7. ☐ `pnpm lint` / `tsc` / `pnpm test` 通过（本步验证）

- [ ] **Step 3：阶段 commit + plan 完工**

```bash
git commit --allow-empty -m "chore(frontend): plan-E milestone — auth + verification done"
git commit --allow-empty -m "feat(frontend): visual realignment to design.pen — done"
```

- [ ] **Step 4：发起 PR（可选，由用户决定时机）**

PR 标题：`feat(frontend): visual realignment to design.pen v1`

PR body 引用：
- spec：`docs/superpowers/specs/2026-04-23-frontend-visual-realignment-to-design-pen.md`
- 5 plan：`docs/superpowers/plans/2026-04-23-frontend-visual-realignment-plan-{A..E}.md`
- checklist：`docs/superpowers/specs/assets/verification-checklist.md`

---

## 自审

**Spec coverage：** 第 5.7 章 Auth 组件 ✓；第 7.8 章登录页装配 ✓；第 9.1 截图脚本与基线目录约定（基线由 plan-A Task A-0 落地，本 plan 加截图脚本）✓；第 9.2 章 30 检查点（Task E-3 模板 + Task E-4 执行）✓；第 11 章 DoD 核对 ✓。

**Placeholder scan：** 已扫；30 检查点是用户必须执行的人工动作，不算 placeholder（`☐` 是模板符号）。

**Type consistency：** Auth 4 组件 props 单一来源；`LoginPage` 内通过 `useBrandingStore.productName` 拼"登录到 X"，与设计稿"登录到 AI 小家"一致；`Checkbox` 用现有 shadcn 原子。

---

## 5 份 plan 索引

为方便后续执行，按推荐顺序：

1. `plan-A-tokens-and-shell.md` — token 清瘦 + AppShell
2. `plan-B-static-pages.md` — Home / Skills / Schedules
3. `plan-C-settings.md` — Settings Modal
4. `plan-D-chat-scene.md` — 聊天场景与交互
5. `plan-E-auth-and-verify.md` — 登录 + 视觉走查（本份）
