# Phase 5 PR0：前端 RegistrationModal 共抽 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 抽出可复用的 `RegistrationModal` 组件，支持 `url`（dingtalk OPEN_CLAW URL+用户码模式）和 `qr_url`（wechat iLink QR URL→前端渲染二维码模式）两种 mode；把 `ChannelConfig.tsx` 里的 dingtalk inline 流程切到新组件，行为 byte-for-byte 不变。

**Architecture:** 在 `src/components/registration/` 新建 `RegistrationModal.tsx` + `QrCodeCanvas.tsx`（QR URL → canvas 渲染辅助），公开 props 包含倒计时、轮询回调、`confirmed/cancelled/expired/waiting` 状态机。`ChannelConfig.tsx` 改为薄外壳：开始注册、调 `<RegistrationModal mode="url" ...>`，把现有的内联 `QrCodePanel` + 倒计时 + 轮询 loop 全删掉。`qr_url` mode 在本 PR 内**只渲染 UI 不接业务**——Phase 5 PR3 才会有 wechat connector 调用它。

**Tech Stack:** React 18 + TypeScript + Tailwind + vitest + @testing-library/react + 现有 `qrcode` lib (^1.5.4)。无新依赖。

**Prerequisites:** 无。本 PR 可与 Phase 1-4 任何 PR 并行；Phase 5 PR3 依赖本 PR 合并。

**参考**：spec `docs/superpowers/specs/2026-05-18-im-wechat-phase5-design.md` §0。

---

## File Structure

```
src/components/registration/                    ← 新增整个目录
├── RegistrationModal.tsx                       ← 通用注册组件（url + qr_url 两 mode）
├── RegistrationModal.test.tsx                  ← vitest 单测
├── QrCodeCanvas.tsx                            ← QR URL → canvas 渲染（从 ChannelConfig 抽出）
└── QrCodeCanvas.test.tsx                       ← vitest 单测

src/features/channel/
├── ChannelConfig.tsx                           ← 改造：去 inline QR/轮询，改用 RegistrationModal
└── ChannelConfig.test.tsx                      ← 既有测试保持通过
```

**核心责任划分**：
- `RegistrationModal`：状态机 + 倒计时 + 凭证轮询，**不知道**任何平台细节（dingtalk / wechat）
- `QrCodeCanvas`：纯函数式 → 把 string URL 渲染成 QR canvas（white bg + lucide spinner overlay）
- `ChannelConfig`：dingtalk 业务壳——把 `beginRegistration('dingtalk')` 拿到的 `verificationUriComplete` 喂进 `RegistrationModal mode="url"`

---

## §0 前置准备

- [ ] **Step 0.1: 确认当前目录干净**

Run: `git status -s -- src/components src/features/channel`
Expected: 输出为空，或仅有 `src-tauri/` 等无关改动。如果有 `src/components/registration/` 残留，先 stash 或确认是上一次未完成的 PR。

- [ ] **Step 0.2: 跑现有 dingtalk 测试，记基线**

Run: `pnpm exec vitest run src/features/channel/ChannelConfig.test.tsx`
Expected: 所有 case PASS。本 PR 末尾会再跑一次确认零回归。

- [ ] **Step 0.3: 创建目录**

Run: `mkdir -p src/components/registration`
Expected: 目录存在。

---

## Task 1: 抽出 `QrCodeCanvas` 组件

**Files:**
- Create: `src/components/registration/QrCodeCanvas.tsx`
- Create: `src/components/registration/QrCodeCanvas.test.tsx`

`ChannelConfig.tsx` 当前的 `QrCodePanel` 是 dingtalk inline 实现：传 URL → useEffect 调 `QRCode.toDataURL` → `<img src={dataUrl}>`，loading 时蒙一层 spinner。把它原样抽出到 `QrCodeCanvas`，签名通用化（接收任意 URL/payload 字符串），dingtalk 占位 7x7 grid pattern 保留（offline / 错误兜底）。

- [ ] **Step 1.1: 写失败的测试**

Create `src/components/registration/QrCodeCanvas.test.tsx`:

```tsx
import '@testing-library/jest-dom'
import { render, screen, waitFor } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import { QrCodeCanvas } from './QrCodeCanvas'

describe('QrCodeCanvas', () => {
  it('renders an <img> with a data URL after qrcode lib resolves', async () => {
    render(<QrCodeCanvas value="https://example.com/qr-payload" loading={false} />)
    const img = await screen.findByRole('img', { name: /二维码/ })
    await waitFor(() => {
      expect(img.getAttribute('src')).toMatch(/^data:image\/png;base64,/)
    })
  })

  it('shows the spinner overlay when loading=true', () => {
    render(<QrCodeCanvas value="https://example.com/qr-payload" loading={true} />)
    expect(screen.getByTestId('qr-spinner-overlay')).toBeInTheDocument()
  })

  it('renders the structural placeholder grid when value is empty', () => {
    render(<QrCodeCanvas value="" loading={false} />)
    expect(screen.queryByRole('img', { name: /二维码/ })).not.toBeInTheDocument()
    expect(screen.getByTestId('qr-placeholder-grid')).toBeInTheDocument()
  })
})
```

- [ ] **Step 1.2: 运行测试确认失败**

Run: `pnpm exec vitest run src/components/registration/QrCodeCanvas.test.tsx`
Expected: FAIL —— `Cannot find module './QrCodeCanvas'`

- [ ] **Step 1.3: 实现 `QrCodeCanvas`**

Create `src/components/registration/QrCodeCanvas.tsx`:

```tsx
import { useEffect, useState } from 'react'
import QRCode from 'qrcode'
import { Loader2 } from 'lucide-react'

interface QrCodeCanvasProps {
  /** Any string payload (URL, token, etc.) to be encoded into a QR image. */
  value: string
  /** Show a spinner overlay; used while begin_registration is in-flight. */
  loading: boolean
  /** Accessible label for the generated <img>. */
  alt?: string
}

/**
 * Renders a QR code from a string payload using the `qrcode` library.
 *
 * - White background is **fixed** (not theme-driven): WeChat / DingTalk scanner
 *   apps fail to decode against dark backgrounds.
 * - When `value` is empty, shows a structural 7x7 grid placeholder so the layout
 *   doesn't jump while begin_registration is still in flight.
 * - When `loading=true`, overlays a backdrop + spinner without unmounting the
 *   QR image (avoids flash on re-render).
 */
export function QrCodeCanvas({ value, loading, alt = '注册二维码' }: QrCodeCanvasProps) {
  const [qrDataUrl, setQrDataUrl] = useState<string | null>(null)

  useEffect(() => {
    if (!value) {
      setQrDataUrl(null)
      return
    }
    let cancelled = false
    QRCode.toDataURL(value, {
      errorCorrectionLevel: 'M',
      margin: 1,
      width: 224,
      color: { dark: '#111111', light: '#ffffff' },
    })
      .then((url) => {
        if (!cancelled) setQrDataUrl(url)
      })
      .catch(() => {
        if (!cancelled) setQrDataUrl(null)
      })
    return () => {
      cancelled = true
    }
  }, [value])

  return (
    <div className="relative flex h-60 w-60 items-center justify-center rounded-3xl border border-border bg-white p-4">
      {qrDataUrl ? (
        <img src={qrDataUrl} alt={alt} className="h-full w-full" />
      ) : (
        // Structural placeholder: black dots on white grid, keeps slot height stable
        <div
          aria-label={alt}
          data-testid="qr-placeholder-grid"
          className="grid h-full w-full grid-cols-7 grid-rows-7 gap-1 rounded bg-white p-2"
        >
          {Array.from({ length: 49 }).map((_, index) => (
            <span
              key={index}
              className={`rounded-[2px] ${[0, 1, 2, 7, 14, 42, 43, 44, 48, 34, 24, 18, 12, 31, 39, 5, 10, 29, 36, 46].includes(index) ? 'bg-black' : 'bg-zinc-100'}`}
            />
          ))}
        </div>
      )}
      {loading && (
        <div
          data-testid="qr-spinner-overlay"
          className="absolute inset-4 flex items-center justify-center rounded-xl bg-background/75 backdrop-blur-[1px]"
        >
          <Loader2 className="h-8 w-8 animate-spin text-primary" />
        </div>
      )}
    </div>
  )
}
```

- [ ] **Step 1.4: 测试通过**

Run: `pnpm exec vitest run src/components/registration/QrCodeCanvas.test.tsx`
Expected: All 3 cases PASS.

- [ ] **Step 1.5: 提交**

```bash
git add src/components/registration/QrCodeCanvas.tsx src/components/registration/QrCodeCanvas.test.tsx
git commit -m "feat(registration): extract QrCodeCanvas with white-bg + placeholder + loading overlay"
```

---

## Task 2: 写 `RegistrationModal` 的类型契约 + 状态机骨架

**Files:**
- Create: `src/components/registration/RegistrationModal.tsx`
- Create: `src/components/registration/RegistrationModal.test.tsx`

定义 props + 内部状态机。本任务只做 **state machine 骨架 + 类型**，不渲染 UI。

- [ ] **Step 2.1: 写失败的测试（仅类型 + 默认状态）**

Create `src/components/registration/RegistrationModal.test.tsx`:

```tsx
import '@testing-library/jest-dom'
import { render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import { RegistrationModal } from './RegistrationModal'

const noop = async () => 'waiting' as const

describe('RegistrationModal — state machine', () => {
  it('renders the title for mode="url"', () => {
    render(
      <RegistrationModal
        mode="url"
        title="配置钉钉"
        url="https://example.com/oauth?user_code=ABCD"
        userCode="ABCD-EFGH"
        expireSeconds={7200}
        pollState={noop}
        onConfirmed={vi.fn()}
        onCancel={vi.fn()}
      />,
    )
    expect(screen.getByRole('heading', { name: /配置钉钉/ })).toBeInTheDocument()
  })

  it('renders the title for mode="qr_url"', () => {
    render(
      <RegistrationModal
        mode="qr_url"
        title="添加个人微信账号"
        qrUrl="https://ilink.weixin.qq.com/qr/abc"
        expireSeconds={120}
        pollState={noop}
        onConfirmed={vi.fn()}
        onCancel={vi.fn()}
      />,
    )
    expect(screen.getByRole('heading', { name: /添加个人微信账号/ })).toBeInTheDocument()
  })
})
```

- [ ] **Step 2.2: 运行测试确认失败**

Run: `pnpm exec vitest run src/components/registration/RegistrationModal.test.tsx`
Expected: FAIL —— `Cannot find module './RegistrationModal'`

- [ ] **Step 2.3: 实现最小骨架使两个 title 测试通过**

Create `src/components/registration/RegistrationModal.tsx`:

```tsx
import { useEffect, useRef, useState } from 'react'
import { QrCodeCanvas } from './QrCodeCanvas'

export type RegistrationPollState = 'waiting' | 'confirmed' | 'cancelled' | 'expired'

interface CommonProps {
  title: string
  /** Total time before the registration session expires, in seconds. */
  expireSeconds: number
  /** Caller-provided polling function. Called repeatedly until it returns
   *  a non-`waiting` state OR the deadline is reached. */
  pollState: () => Promise<RegistrationPollState>
  /** Interval between polls in ms. Default 2000. */
  pollIntervalMs?: number
  onConfirmed: () => void
  onCancel: () => void
}

interface UrlModeProps extends CommonProps {
  mode: 'url'
  url: string
  /** Optional user-visible code (DingTalk OPEN_CLAW shows e.g. "ABCD-EFGH"). */
  userCode?: string
  qrUrl?: never
}

interface QrUrlModeProps extends CommonProps {
  mode: 'qr_url'
  /** The raw URL string returned by the platform; will be rendered into a QR
   *  image client-side via `qrcode` lib. NOT a base64 PNG. */
  qrUrl: string
  url?: never
  userCode?: never
}

export type RegistrationModalProps = UrlModeProps | QrUrlModeProps

export function RegistrationModal(props: RegistrationModalProps) {
  return (
    <div className="flex max-h-[78vh] w-full flex-col overflow-hidden bg-background">
      <div className="flex flex-col items-center px-10 pb-5 pt-8 text-center">
        <h2 className="text-2xl font-bold tracking-tight text-foreground">{props.title}</h2>
      </div>
    </div>
  )
}
```

- [ ] **Step 2.4: 测试通过**

Run: `pnpm exec vitest run src/components/registration/RegistrationModal.test.tsx`
Expected: 2/2 PASS.

- [ ] **Step 2.5: 提交**

```bash
git add src/components/registration/RegistrationModal.tsx src/components/registration/RegistrationModal.test.tsx
git commit -m "feat(registration): RegistrationModal skeleton with url + qr_url discriminated union"
```

---

## Task 3: 倒计时显示

**Files:**
- Modify: `src/components/registration/RegistrationModal.tsx`
- Modify: `src/components/registration/RegistrationModal.test.tsx`

显示倒计时（`mm:ss` 格式）。到 0 时调 `onCancel('expired' 语义由调用方判断)`，但本 task 先只显示数字、不动 expiry side effect（task 5 一起做）。

- [ ] **Step 3.1: 加倒计时的失败测试**

在 `RegistrationModal.test.tsx` 内追加 describe block：

```tsx
import { act } from '@testing-library/react'

describe('RegistrationModal — countdown', () => {
  beforeEach(() => {
    vi.useFakeTimers()
  })
  afterEach(() => {
    vi.useRealTimers()
  })

  it('shows mm:ss formatted remaining time', () => {
    render(
      <RegistrationModal
        mode="url"
        title="配置钉钉"
        url="https://x.test"
        expireSeconds={125}
        pollState={noop}
        onConfirmed={vi.fn()}
        onCancel={vi.fn()}
      />,
    )
    // 125s = 02:05
    expect(screen.getByTestId('registration-countdown')).toHaveTextContent('02:05')

    act(() => {
      vi.advanceTimersByTime(1000)
    })
    expect(screen.getByTestId('registration-countdown')).toHaveTextContent('02:04')
  })
})
```

需要在文件顶部 imports 加 `import { beforeEach, afterEach } from 'vitest'` 并 `import { act } from '@testing-library/react'`（若已有则忽略）。

- [ ] **Step 3.2: 运行测试确认失败**

Run: `pnpm exec vitest run src/components/registration/RegistrationModal.test.tsx`
Expected: 倒计时 case FAIL（找不到 testid）；之前 2 个 title case 仍 PASS。

- [ ] **Step 3.3: 实现倒计时**

在 `RegistrationModal.tsx` 内 `RegistrationModal` 函数体加倒计时 hook + JSX：

```tsx
export function RegistrationModal(props: RegistrationModalProps) {
  const [remainingSec, setRemainingSec] = useState(props.expireSeconds)

  useEffect(() => {
    if (remainingSec <= 0) return
    const interval = window.setInterval(() => {
      setRemainingSec((s) => Math.max(0, s - 1))
    }, 1000)
    return () => window.clearInterval(interval)
  }, [remainingSec])

  const mm = String(Math.floor(remainingSec / 60)).padStart(2, '0')
  const ss = String(remainingSec % 60).padStart(2, '0')

  return (
    <div className="flex max-h-[78vh] w-full flex-col overflow-hidden bg-background">
      <div className="flex flex-col items-center px-10 pb-5 pt-8 text-center">
        <h2 className="text-2xl font-bold tracking-tight text-foreground">{props.title}</h2>
        <p data-testid="registration-countdown" className="mt-2 text-xs font-medium text-muted-foreground">
          剩余 {mm}:{ss}
        </p>
      </div>
    </div>
  )
}
```

- [ ] **Step 3.4: 测试通过**

Run: `pnpm exec vitest run src/components/registration/RegistrationModal.test.tsx`
Expected: 3/3 PASS.

- [ ] **Step 3.5: 提交**

```bash
git add src/components/registration/RegistrationModal.tsx src/components/registration/RegistrationModal.test.tsx
git commit -m "feat(registration): countdown timer in mm:ss"
```

---

## Task 4: mode-specific body rendering

**Files:**
- Modify: `src/components/registration/RegistrationModal.tsx`
- Modify: `src/components/registration/RegistrationModal.test.tsx`

`mode="url"` 渲染 URL link + 可选 userCode + 现成的 `QrCodeCanvas`（dingtalk 的旧行为是 QR + URL 链接 + 用户码，两层都给用户看）。`mode="qr_url"` 只渲染 `QrCodeCanvas`（wechat 不给 URL）。

- [ ] **Step 4.1: 加 mode-specific 渲染的失败测试**

追加 describe block 到 `RegistrationModal.test.tsx`：

```tsx
describe('RegistrationModal — mode rendering', () => {
  it('mode="url" renders the URL link and userCode', () => {
    render(
      <RegistrationModal
        mode="url"
        title="配置钉钉"
        url="https://example.com/oauth?user_code=ABCD-EFGH"
        userCode="ABCD-EFGH"
        expireSeconds={7200}
        pollState={noop}
        onConfirmed={vi.fn()}
        onCancel={vi.fn()}
      />,
    )
    expect(screen.getByText('ABCD-EFGH')).toBeInTheDocument()
    const link = screen.getByRole('link', { name: /继续/ })
    expect(link).toHaveAttribute('href', 'https://example.com/oauth?user_code=ABCD-EFGH')
    expect(link).toHaveAttribute('target', '_blank')
    expect(screen.getByRole('img', { name: /注册二维码/ })).toBeInTheDocument()
  })

  it('mode="qr_url" renders only the QR canvas, no URL link', () => {
    render(
      <RegistrationModal
        mode="qr_url"
        title="添加个人微信账号"
        qrUrl="https://ilink.weixin.qq.com/qr/abc123"
        expireSeconds={120}
        pollState={noop}
        onConfirmed={vi.fn()}
        onCancel={vi.fn()}
      />,
    )
    expect(screen.getByRole('img', { name: /注册二维码/ })).toBeInTheDocument()
    expect(screen.queryByRole('link', { name: /继续/ })).not.toBeInTheDocument()
  })
})
```

- [ ] **Step 4.2: 运行测试确认失败**

Run: `pnpm exec vitest run src/components/registration/RegistrationModal.test.tsx`
Expected: 新加的 2 个 case FAIL（找不到 link / userCode / QR）；前面的 case 仍 PASS。

- [ ] **Step 4.3: 实现 mode-specific 主体**

替换 `RegistrationModal` 函数 return 部分（保留倒计时 + 标题）：

```tsx
import { ExternalLink } from 'lucide-react'

// ... existing imports and types ...

export function RegistrationModal(props: RegistrationModalProps) {
  const [remainingSec, setRemainingSec] = useState(props.expireSeconds)

  useEffect(() => {
    if (remainingSec <= 0) return
    const interval = window.setInterval(() => {
      setRemainingSec((s) => Math.max(0, s - 1))
    }, 1000)
    return () => window.clearInterval(interval)
  }, [remainingSec])

  const mm = String(Math.floor(remainingSec / 60)).padStart(2, '0')
  const ss = String(remainingSec % 60).padStart(2, '0')

  const qrPayload = props.mode === 'url' ? props.url : props.qrUrl

  return (
    <div className="flex max-h-[78vh] w-full flex-col overflow-hidden bg-background">
      <div className="flex flex-col items-center px-10 pb-5 pt-8 text-center">
        <h2 className="text-2xl font-bold tracking-tight text-foreground">{props.title}</h2>
        <p data-testid="registration-countdown" className="mt-2 text-xs font-medium text-muted-foreground">
          剩余 {mm}:{ss}
        </p>
      </div>

      <div className="flex-1 overflow-y-auto px-10 pb-6">
        <div className="flex flex-col items-center gap-4">
          <QrCodeCanvas value={qrPayload} loading={false} />

          {props.mode === 'url' && (
            <>
              {props.userCode && (
                <div className="rounded-xl border border-border bg-muted/25 px-4 py-3 text-center">
                  <div className="text-xs font-bold uppercase tracking-wide text-muted-foreground">用户码</div>
                  <div className="mt-1 font-mono text-lg font-semibold text-foreground">{props.userCode}</div>
                </div>
              )}
              <a
                href={props.url}
                target="_blank"
                rel="noreferrer"
                className="inline-flex items-center gap-1 text-xs font-medium text-primary underline-offset-4 hover:underline"
              >
                页面未自动打开？点击继续 <ExternalLink className="h-3 w-3" />
              </a>
            </>
          )}
        </div>
      </div>
    </div>
  )
}
```

- [ ] **Step 4.4: 测试通过**

Run: `pnpm exec vitest run src/components/registration/RegistrationModal.test.tsx`
Expected: 5/5 PASS.

- [ ] **Step 4.5: 提交**

```bash
git add src/components/registration/RegistrationModal.tsx src/components/registration/RegistrationModal.test.tsx
git commit -m "feat(registration): mode-specific body — url shows link+userCode, qr_url shows only QR"
```

---

## Task 5: 轮询状态机 + onConfirmed/onCancel/expired

**Files:**
- Modify: `src/components/registration/RegistrationModal.tsx`
- Modify: `src/components/registration/RegistrationModal.test.tsx`

调 `pollState()` 直到收到 `confirmed`/`cancelled`/`expired`，或倒计时到 0。各分支触发对应回调或本地 state 切换。

- [ ] **Step 5.1: 写状态切换的失败测试**

追加 describe block：

```tsx
describe('RegistrationModal — polling state machine', () => {
  beforeEach(() => {
    vi.useFakeTimers({ shouldAdvanceTime: true })
  })
  afterEach(() => {
    vi.useRealTimers()
  })

  it('calls onConfirmed when pollState resolves "confirmed"', async () => {
    const onConfirmed = vi.fn()
    const pollState = vi.fn().mockResolvedValueOnce('confirmed')
    render(
      <RegistrationModal
        mode="qr_url"
        title="t"
        qrUrl="https://x"
        expireSeconds={60}
        pollState={pollState}
        pollIntervalMs={50}
        onConfirmed={onConfirmed}
        onCancel={vi.fn()}
      />,
    )
    await vi.waitFor(() => expect(onConfirmed).toHaveBeenCalledTimes(1), { timeout: 1000 })
  })

  it('shows "expired" state and call onCancel when pollState resolves "expired"', async () => {
    const onCancel = vi.fn()
    const pollState = vi.fn().mockResolvedValueOnce('expired')
    render(
      <RegistrationModal
        mode="qr_url"
        title="t"
        qrUrl="https://x"
        expireSeconds={60}
        pollState={pollState}
        pollIntervalMs={50}
        onConfirmed={vi.fn()}
        onCancel={onCancel}
      />,
    )
    await vi.waitFor(() => expect(onCancel).toHaveBeenCalledTimes(1), { timeout: 1000 })
    expect(screen.getByText(/二维码已过期/)).toBeInTheDocument()
  })

  it('keeps polling while pollState returns "waiting"', async () => {
    const pollState = vi
      .fn()
      .mockResolvedValueOnce('waiting')
      .mockResolvedValueOnce('waiting')
      .mockResolvedValueOnce('confirmed')
    const onConfirmed = vi.fn()
    render(
      <RegistrationModal
        mode="qr_url"
        title="t"
        qrUrl="https://x"
        expireSeconds={60}
        pollState={pollState}
        pollIntervalMs={20}
        onConfirmed={onConfirmed}
        onCancel={vi.fn()}
      />,
    )
    await vi.waitFor(() => expect(onConfirmed).toHaveBeenCalledTimes(1), { timeout: 2000 })
    expect(pollState).toHaveBeenCalledTimes(3)
  })
})
```

- [ ] **Step 5.2: 运行测试确认失败**

Run: `pnpm exec vitest run src/components/registration/RegistrationModal.test.tsx`
Expected: 新加的 3 个 case FAIL；前面 5 case 仍 PASS。

- [ ] **Step 5.3: 实现 polling state machine**

在 `RegistrationModal.tsx` 内加 polling logic。完整替换文件：

```tsx
import { useEffect, useRef, useState } from 'react'
import { ExternalLink } from 'lucide-react'
import { QrCodeCanvas } from './QrCodeCanvas'

export type RegistrationPollState = 'waiting' | 'confirmed' | 'cancelled' | 'expired'

interface CommonProps {
  title: string
  expireSeconds: number
  pollState: () => Promise<RegistrationPollState>
  pollIntervalMs?: number
  onConfirmed: () => void
  onCancel: () => void
}

interface UrlModeProps extends CommonProps {
  mode: 'url'
  url: string
  userCode?: string
  qrUrl?: never
}

interface QrUrlModeProps extends CommonProps {
  mode: 'qr_url'
  qrUrl: string
  url?: never
  userCode?: never
}

export type RegistrationModalProps = UrlModeProps | QrUrlModeProps

type LocalState = 'polling' | 'confirmed' | 'cancelled' | 'expired'

export function RegistrationModal(props: RegistrationModalProps) {
  const [remainingSec, setRemainingSec] = useState(props.expireSeconds)
  const [localState, setLocalState] = useState<LocalState>('polling')
  const pollIntervalMs = props.pollIntervalMs ?? 2000

  // Snapshot callbacks in refs so the polling loop doesn't restart on every
  // parent re-render. The loop reads the latest values via the ref.
  const pollStateRef = useRef(props.pollState)
  const onConfirmedRef = useRef(props.onConfirmed)
  const onCancelRef = useRef(props.onCancel)
  useEffect(() => {
    pollStateRef.current = props.pollState
    onConfirmedRef.current = props.onConfirmed
    onCancelRef.current = props.onCancel
  })

  // Countdown
  useEffect(() => {
    if (localState !== 'polling' || remainingSec <= 0) return
    const interval = window.setInterval(() => {
      setRemainingSec((s) => {
        if (s <= 1) {
          setLocalState('expired')
          onCancelRef.current()
          return 0
        }
        return s - 1
      })
    }, 1000)
    return () => window.clearInterval(interval)
  }, [localState, remainingSec])

  // Polling loop
  useEffect(() => {
    if (localState !== 'polling') return
    let cancelled = false
    const loop = async () => {
      while (!cancelled) {
        let result: RegistrationPollState
        try {
          result = await pollStateRef.current()
        } catch {
          result = 'waiting'  // 网络抖动等：留给倒计时兜底
        }
        if (cancelled) return
        if (result === 'confirmed') {
          setLocalState('confirmed')
          onConfirmedRef.current()
          return
        }
        if (result === 'cancelled') {
          setLocalState('cancelled')
          onCancelRef.current()
          return
        }
        if (result === 'expired') {
          setLocalState('expired')
          onCancelRef.current()
          return
        }
        // waiting → sleep then poll again
        await new Promise((r) => window.setTimeout(r, pollIntervalMs))
      }
    }
    void loop()
    return () => {
      cancelled = true
    }
  }, [localState, pollIntervalMs])

  const mm = String(Math.floor(remainingSec / 60)).padStart(2, '0')
  const ss = String(remainingSec % 60).padStart(2, '0')

  const qrPayload = props.mode === 'url' ? props.url : props.qrUrl

  return (
    <div className="flex max-h-[78vh] w-full flex-col overflow-hidden bg-background">
      <div className="flex flex-col items-center px-10 pb-5 pt-8 text-center">
        <h2 className="text-2xl font-bold tracking-tight text-foreground">{props.title}</h2>
        <p data-testid="registration-countdown" className="mt-2 text-xs font-medium text-muted-foreground">
          剩余 {mm}:{ss}
        </p>
      </div>

      <div className="flex-1 overflow-y-auto px-10 pb-6">
        <div className="flex flex-col items-center gap-4">
          {localState === 'expired' ? (
            <div className="rounded-xl bg-red-50 px-5 py-3 text-sm font-semibold text-red-500">
              二维码已过期，请重新发起
            </div>
          ) : localState === 'cancelled' ? (
            <div className="rounded-xl bg-muted px-5 py-3 text-sm font-semibold text-muted-foreground">
              扫码已取消
            </div>
          ) : localState === 'confirmed' ? (
            <div className="rounded-xl bg-emerald-50 px-5 py-3 text-sm font-semibold text-emerald-500">
              扫码成功，正在完成配置…
            </div>
          ) : (
            <>
              <QrCodeCanvas value={qrPayload} loading={false} />
              {props.mode === 'url' && (
                <>
                  {props.userCode && (
                    <div className="rounded-xl border border-border bg-muted/25 px-4 py-3 text-center">
                      <div className="text-xs font-bold uppercase tracking-wide text-muted-foreground">用户码</div>
                      <div className="mt-1 font-mono text-lg font-semibold text-foreground">{props.userCode}</div>
                    </div>
                  )}
                  <a
                    href={props.url}
                    target="_blank"
                    rel="noreferrer"
                    className="inline-flex items-center gap-1 text-xs font-medium text-primary underline-offset-4 hover:underline"
                  >
                    页面未自动打开？点击继续 <ExternalLink className="h-3 w-3" />
                  </a>
                </>
              )}
            </>
          )}
        </div>
      </div>
    </div>
  )
}
```

- [ ] **Step 5.4: 测试通过**

Run: `pnpm exec vitest run src/components/registration/RegistrationModal.test.tsx`
Expected: 8/8 PASS（之前的 5 个 + 新加的 3 个）。如果倒计时 case 因为 fake-timer + waitFor 行为冲突失败，把 Task 3 的倒计时 case 改用 `vi.useFakeTimers({ shouldAdvanceTime: true })` + `await vi.runOnlyPendingTimersAsync()` 模式，跟新加的 polling case 对齐。

- [ ] **Step 5.5: 提交**

```bash
git add src/components/registration/RegistrationModal.tsx src/components/registration/RegistrationModal.test.tsx
git commit -m "feat(registration): polling state machine with waiting/confirmed/cancelled/expired"
```

---

## Task 6: 改造 `ChannelConfig.tsx` 用 `RegistrationModal`

**Files:**
- Modify: `src/features/channel/ChannelConfig.tsx`
- Modify: `src/features/channel/ChannelConfig.test.tsx`（仅在必要时改 assertion）

去掉 `ChannelConfig` 里的 `QrCodePanel` 内联组件、`pollRegistration` 循环、`registrationStatus` 状态机；改为：调 `beginRegistration('dingtalk')` 拿 `verificationUriComplete` + `userCode` + `expiresInSeconds` 后，渲染 `<RegistrationModal mode="url" ...>`，把 `pollRegistrationAction('dingtalk', deviceCode)` 包成符合新接口的 `pollState`。

**关键约束**：UX 行为 byte-for-byte 不变。原 `ChannelConfig.test.tsx` 的现有 case 必须**不改动断言**全部通过（除非测试本身依赖了被内联组件渲染的 DOM 节点 — 如果是这样，把 testid 从内联节点移到 `RegistrationModal` 内并同步更新断言）。

- [ ] **Step 6.1: 先读 ChannelConfig 当前的全部行为**

Run: `cat src/features/channel/ChannelConfig.tsx`
Read carefully and note: ① `handleStartRegistration` 入口 ② `pollRegistration` 状态分发（waiting / success / expired / fail）③ `registrationDone` 显示 credentials block ④ 错误 / loading 文案。注意 `beginRegistration('dingtalk')` 返回的 `result.config` / `result.platformState` 处理路径。

- [ ] **Step 6.2: 先跑现有 ChannelConfig 测试拿到全部 case 名**

Run: `pnpm exec vitest run src/features/channel/ChannelConfig.test.tsx --reporter=verbose 2>&1 | head -80`
Expected: 列出所有 it() 名称。记下来 —— 重构完后这些必须全 PASS。

- [ ] **Step 6.3: 改造 `ChannelConfig.tsx`**

完整替换内容（保留 `RegisteredCredentials` 接口 + `CredentialRow` helper + `onSaved`/`onClose` props）：

```tsx
import { useEffect, useRef, useState } from 'react'
import { CheckCircle2 } from 'lucide-react'
import { type ChannelConfigView, type ChannelRegistrationBeginResult } from '@/lib/tauri'
import { useChannelStore } from '@/stores/channelStore'
import { Button } from '@/components/ui/button'
import { RegistrationModal, type RegistrationPollState } from '@/components/registration/RegistrationModal'

interface ChannelConfigProps {
  onSaved?: () => void
  onClose?: () => void
}

interface RegisteredCredentials {
  config: ChannelConfigView
}

function CredentialRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-xl border border-border bg-muted/25 px-4 py-3">
      <div className="text-xs font-bold uppercase tracking-wide text-muted-foreground">{label}</div>
      <div className="mt-1 break-all font-mono text-sm font-semibold text-foreground">{value}</div>
    </div>
  )
}

export function ChannelConfig({ onSaved, onClose }: ChannelConfigProps) {
  const [error, setError] = useState<string | null>(null)
  const [begin, setBegin] = useState<ChannelRegistrationBeginResult | null>(null)
  const [credentials, setCredentials] = useState<RegisteredCredentials | null>(null)
  const [attempt, setAttempt] = useState(0)
  const beginRegistration = useChannelStore((s) => s.beginRegistration)
  const pollRegistrationAction = useChannelStore((s) => s.pollRegistration)
  const setPlatformState = useChannelStore((s) => s.setPlatformState)

  // Snapshot poll target deviceCode so RegistrationModal's pollState closure
  // doesn't capture stale begin state across retries.
  const deviceCodeRef = useRef<string | null>(null)

  useEffect(() => {
    let cancelled = false
    setError(null)
    setBegin(null)
    setCredentials(null)
    const run = async () => {
      try {
        const result = await beginRegistration('dingtalk')
        if (cancelled) return
        deviceCodeRef.current = result.deviceCode
        setBegin(result)
      } catch (e) {
        if (cancelled) return
        setError(e instanceof Error ? e.message : '钉钉扫码开通失败，请重试')
      }
    }
    void run()
    return () => {
      cancelled = true
    }
  }, [attempt, beginRegistration])

  // Adapter: backend's pollRegistration -> RegistrationModal's RegistrationPollState
  const pollState = async (): Promise<RegistrationPollState> => {
    const deviceCode = deviceCodeRef.current
    if (!deviceCode) return 'waiting'
    const result = await pollRegistrationAction('dingtalk', deviceCode)
    if (result.state === 'success') {
      const config = result.config ?? result.platformState?.config
      if (!config) {
        setError('钉钉已授权，但未返回频道配置')
        return 'cancelled'
      }
      if (result.platformState) setPlatformState(result.platformState)
      setCredentials({ config })
      onSaved?.()
      return 'confirmed'
    }
    if (result.state === 'expired') return 'expired'
    if (result.state === 'fail') {
      setError(result.failReason || '钉钉扫码开通失败')
      return 'cancelled'
    }
    return 'waiting'
  }

  const handleRetry = () => setAttempt((n) => n + 1)

  if (credentials) {
    return (
      <div className="flex max-h-[78vh] w-full flex-col overflow-hidden bg-background">
        <div className="flex flex-col items-center px-10 pb-5 pt-8 text-center">
          <h2 className="text-2xl font-bold tracking-tight text-foreground">配置钉钉</h2>
        </div>
        <div className="flex-1 overflow-y-auto px-10 pb-6">
          <div className="flex w-full flex-col items-center gap-5">
            <div className="flex w-64 flex-col items-center rounded-xl bg-emerald-50 px-8 py-5 text-emerald-500">
              <CheckCircle2 className="h-8 w-8" />
              <div className="mt-3 text-xl font-bold">扫码开通成功</div>
              <div className="mt-1 text-sm font-semibold">应用已创建</div>
            </div>
            <div className="grid w-full gap-3 rounded-xl border border-border bg-card p-4 text-left">
              <CredentialRow label="AppKey" value={credentials.config.appKey} />
              <CredentialRow label="AppSecret" value={credentials.config.appSecretMasked} />
              <CredentialRow label="RobotCode" value={credentials.config.robotCode} />
            </div>
            <Button size="sm" variant="secondary" onClick={handleRetry}>
              重新扫码更换配置
            </Button>
          </div>
        </div>
        <div className="border-t border-border bg-background px-10 py-4">
          <Button
            className="h-10 w-full rounded-full"
            onClick={() => {
              onSaved?.()
              onClose?.()
            }}
          >
            完成
          </Button>
        </div>
      </div>
    )
  }

  if (error && !begin) {
    return (
      <div className="flex max-h-[78vh] w-full flex-col items-center justify-center bg-background p-10">
        <p className="text-sm text-red-500">{error}</p>
        <Button className="mt-4 h-10 w-64 rounded-full" onClick={handleRetry}>
          重新生成二维码
        </Button>
      </div>
    )
  }

  if (!begin) {
    return (
      <div className="flex max-h-[78vh] w-full flex-col items-center justify-center bg-background p-10 text-sm text-muted-foreground">
        正在准备扫码…
      </div>
    )
  }

  return (
    <RegistrationModal
      mode="url"
      title="配置钉钉"
      url={begin.verificationUriComplete}
      userCode={begin.userCode}
      expireSeconds={begin.expiresInSeconds || 7200}
      pollIntervalMs={Math.max(1, begin.intervalSeconds || 2) * 1000}
      pollState={pollState}
      onConfirmed={() => {
        /* credentials already set in pollState() */
      }}
      onCancel={handleRetry}
    />
  )
}
```

- [ ] **Step 6.4: 跑既有的 dingtalk 测试**

Run: `pnpm exec vitest run src/features/channel/ChannelConfig.test.tsx --reporter=verbose`
Expected: 全部 case PASS。

可能的小调整：
- 若某条 case 断言"重新生成二维码"按钮的文案 → 已保留
- 若某条 case 断言 inline `QrCodePanel` 的特定 dom 节点 → 用 `screen.getByRole('img', { name: /二维码/ })` 改成对 `QrCodeCanvas` 的断言
- 若 case 用 `fireEvent` 或 `userEvent` 触发"重新扫码更换配置"按钮 → 应仍然命中 `handleRetry` 路径

修改后再次运行直到全 PASS。

- [ ] **Step 6.5: 全前端 lint + type 检查**

Run: `pnpm exec tsc --noEmit`
Expected: 0 errors. 任何 RegistrationModal props 类型错误立刻修复。

Run: `pnpm lint src/components/registration src/features/channel`
Expected: 0 errors / warnings on touched files。

- [ ] **Step 6.6: 提交**

```bash
git add src/components/registration src/features/channel/ChannelConfig.tsx src/features/channel/ChannelConfig.test.tsx
git commit -m "feat(channel): migrate dingtalk registration to shared RegistrationModal"
```

---

## Task 7: 在浏览器里冒烟测试

`tsc` / `vitest` 验证不了真实 UI 行为；CLAUDE.md 强制规定 UI 改动必须在浏览器跑过。

- [ ] **Step 7.1: 启动 dev server**

Run: `pnpm tauri:dev`（in a separate terminal session via `! pnpm tauri:dev`，让用户自己起；或者直接 background）

等待"App ready"提示。

- [ ] **Step 7.2: 手动验证 dingtalk OPEN_CLAW 流程**

在 AIjia 应用里打开 设置 → 频道 → 钉钉。验证：

1. 自动弹出二维码（白底，跟主题切换无关）
2. 二维码下方显示倒计时（`剩余 02:00:00` 格式）
3. 用户码（如 `ABCD-EFGH`）显示在二维码下面
4. "页面未自动打开？点击继续" 链接可点击，打开 dingtalk 注册页
5. 点"重新扫码更换配置"（如果之前已注册）→ 二维码刷新 + 倒计时重置

**如果有真的 OpenClaw 账号且能跑通注册**：扫码 + 用户码确认 → 预期看到 "扫码开通成功" + AppKey/AppSecret/RobotCode 三行 + "完成"按钮。

- [ ] **Step 7.3: 验证主题切换（dark/light）**

切换系统 dark mode → 二维码盒子背景**仍是白色**（不是 dark bg），dingtalk 注册按钮等元素颜色跟主题切换。

- [ ] **Step 7.4: 记一笔结果**

在 commit message 里写一句"manual smoke: dingtalk QR rendered ✓ countdown ✓ link works ✓ theme switch leaves QR white ✓"。这不需要单独 commit，可以 amend 到 Task 6 的 commit 里：

```bash
git commit --amend --no-edit
```

（如果你只是测试通过、没有改动，amend 不需要 — 跳过即可。）

---

## Task 8: 完成自检 + 最终 commit

- [ ] **Step 8.1: 跑全部受影响的测试**

Run: `pnpm exec vitest run src/components/registration src/features/channel`
Expected: 全部 PASS。

- [ ] **Step 8.2: 检查没有 spec 跑题**

确认下列 spec 要点都在 PR0 里实现了：
- ✅ `RegistrationModal` 公开 props 覆盖 url + qr_url
- ✅ 倒计时显示
- ✅ 轮询 4 状态全覆盖（waiting/confirmed/cancelled/expired）
- ✅ dingtalk 切换到新组件
- ✅ vitest 渲染两种 mode + 倒计时 + 状态切换 fixture

- [ ] **Step 8.3: 把 Phase 5 wechat 部分留为后续 PR 的 todo**

确认 `RegistrationModal` 的 `qr_url` mode 已经能渲染，但本 PR 没有任何代码调用它 —— 这是预期的。Phase 5 PR3 (login) 才会把 `<RegistrationModal mode="qr_url" qrUrl={wechatBegin.qrUrl} ... />` 接进来。

- [ ] **Step 8.4: 若有未提交改动，最终 commit**

Run: `git status -s`
如果还有 modified 文件，commit 它们。

```bash
git add -A src/components/registration src/features/channel
git status -s   # 确认 clean
```

- [ ] **Step 8.5: 准备 PR description**

Title: `feat(registration): extract RegistrationModal supporting url + qr_url modes (Phase 5 PR0)`

Body:
```
Phase 5 PR0 — 前端 RegistrationModal 共抽。

引入 `src/components/registration/RegistrationModal` 通用注册组件，支持：
- `mode="url"`：URL + 用户码（dingtalk OPEN_CLAW 现有流程）
- `mode="qr_url"`：纯 QR URL（Phase 5 PR3 wechat 会用）

`ChannelConfig.tsx` 改用新组件，dingtalk 流程 byte-for-byte 不变。
QR 容器固定白底（避免 dark mode 影响微信/钉钉扫码器识别）。

Tests: vitest 覆盖两种 mode + 倒计时 + 4 个 polling 状态。

Spec: docs/superpowers/specs/2026-05-18-im-wechat-phase5-design.md §0
Plan: docs/superpowers/plans/2026-05-18-im-wechat-phase5-pr0-registration-modal.md

Manual smoke: dingtalk QR renders correctly, countdown ticks, link works,
QR stays white across theme switches.
```

---

## §End — 自检 checklist

实施完成后，回答以下问题（如果有 No，回到对应 task 补）：

- [ ] `src/components/registration/RegistrationModal.tsx` 存在并 export `RegistrationModal` + `RegistrationPollState`
- [ ] `src/components/registration/QrCodeCanvas.tsx` 存在
- [ ] `RegistrationModal.test.tsx` 覆盖 8 个 case（2 title + 1 countdown + 2 mode + 3 polling）
- [ ] `QrCodeCanvas.test.tsx` 覆盖 3 个 case（img / spinner / placeholder）
- [ ] `ChannelConfig.tsx` 不再有 `QRCode.toDataURL` 调用（已迁到 `QrCodeCanvas`）
- [ ] `ChannelConfig.tsx` 不再有 inline `pollRegistration` while 循环（已迁到 `RegistrationModal`）
- [ ] `pnpm exec vitest run src/features/channel/ChannelConfig.test.tsx` 全 PASS
- [ ] `pnpm exec tsc --noEmit` 0 errors
- [ ] dingtalk 浏览器冒烟通过

完成。Phase 5 PR3 可以开始（依赖本 PR 合并）。
