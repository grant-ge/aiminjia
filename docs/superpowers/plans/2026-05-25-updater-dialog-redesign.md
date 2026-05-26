# Updater Dialog 重设计 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将自动更新流程从"后台静默下载"改为"用户主导的弹窗式下载"，带进度条、三阶段 UI。

**Architecture:** 改造现有 updaterStore 状态机（新增 `checking`/`available` phase，拆分 `bootstrap()` 和 `startDownload()`），重写 UpdaterPanel 为 Dialog 三阶段 UI，扩展 UpdateAvailableLink 支持更多 phase。

**Tech Stack:** React/TypeScript, Zustand, @tauri-apps/plugin-updater, @/components/ui/dialog

**Spec:** `docs/superpowers/specs/2026-05-25-updater-dialog-redesign.md`

---

## 文件结构

| 文件 | 操作 | 职责 |
|---|---|---|
| `src/lib/updaterStore.ts` | 修改 | 状态机重构：新增 phase、拆分方法 |
| `src/components/common/UpdaterPanel.tsx` | 重写 | Dialog 三阶段 UI |
| `src/components/layout/UpdateAvailableLink.tsx` | 修改 | 支持 available/downloading/ready/failed 显示 |
| `src/components/layout/TitleBar.tsx` | 修改 | 扩大 link 显示条件 |
| `src/components/settings/SettingsModal.tsx` | 修改 | onCheckUpdate 适配新 phase |
| `src/i18n/zh-CN.json` | 修改 | 新增/调整翻译 key |
| `src/i18n/en-US.json` | 修改 | 新增/调整翻译 key |
| `src/lib/updaterStore.test.ts` | 重写 | 适配新状态机的测试 |

---

### Task 1: i18n — 新增/调整翻译 key

**Files:**
- Modify: `src/i18n/zh-CN.json:592-618`
- Modify: `src/i18n/en-US.json:592-618`

- [ ] **Step 1: 更新 zh-CN.json updater section**

替换整个 `"updater": { ... }` 块：

```json
"updater": {
  "linkAvailable": "v{{version}} 可用",
  "linkAvailableTooltip": "有新版本，点击查看",
  "linkDownloading": "v{{version}} 下载中 {{pct}}%",
  "linkDownloadingTooltip": "正在下载更新，点击查看进度",
  "linkReady": "v{{version}} 可安装",
  "linkReadyTooltip": "下载完成，点击安装",
  "linkFailed": "更新失败，点击重试",
  "linkFailedTooltip": "下载更新时出错，点击重新尝试",
  "dialogTitleAvailable": "新版本可用 v{{version}}",
  "dialogTitleDownloading": "正在下载更新 v{{version}}",
  "dialogTitleReady": "更新下载完成 v{{version}}",
  "dialogTitleFailed": "更新失败",
  "versionLine": "当前 {{current}} → 新版本 {{next}}",
  "releaseNotesHeader": "本次更新内容",
  "updateAvailableDesc": "有新版本可用，是否现在更新？",
  "updateNow": "立即更新",
  "updateLater": "稍后再说",
  "downloadProgress": "已下载 {{downloaded}} / {{total}}",
  "downloadComplete": "✓ 下载完成 ({{size}})",
  "installAndRestart": "立即安装并重启",
  "installing": "正在安装...",
  "retry": "重试",
  "downloadFailedMessage": "下载失败：{{error}}",
  "installFailedTitle": "更新安装失败",
  "installSuccessTitle": "更新安装成功",
  "relaunchFailedHint": "安装已完成，但自动重启失败。请手动关闭并重新打开应用以使用新版本。",
  "notReadyMessage": "更新尚未就绪，请稍候再试或重启应用",
  "offlineHint": "网络不可用，无法安装更新，请稍后重试",
  "downloadFailed": "更新下载失败",
  "downloadFailedDesc": "请稍后重试或手动下载新版本"
}
```

- [ ] **Step 2: 更新 en-US.json updater section**

替换整个 `"updater": { ... }` 块：

```json
"updater": {
  "linkAvailable": "v{{version}} available",
  "linkAvailableTooltip": "New version available — click to view",
  "linkDownloading": "v{{version}} downloading {{pct}}%",
  "linkDownloadingTooltip": "Downloading update — click to view progress",
  "linkReady": "v{{version}} ready to install",
  "linkReadyTooltip": "Download complete — click to install",
  "linkFailed": "Update failed — tap to retry",
  "linkFailedTooltip": "Download failed — click to retry",
  "dialogTitleAvailable": "Update available v{{version}}",
  "dialogTitleDownloading": "Downloading update v{{version}}",
  "dialogTitleReady": "Update downloaded v{{version}}",
  "dialogTitleFailed": "Update failed",
  "versionLine": "{{current}} → {{next}}",
  "releaseNotesHeader": "What's new",
  "updateAvailableDesc": "A new version is available. Would you like to update now?",
  "updateNow": "Update Now",
  "updateLater": "Later",
  "downloadProgress": "{{downloaded}} / {{total}} downloaded",
  "downloadComplete": "✓ Download complete ({{size}})",
  "installAndRestart": "Install & Restart",
  "installing": "Installing...",
  "retry": "Retry",
  "downloadFailedMessage": "Download failed: {{error}}",
  "installFailedTitle": "Update install failed",
  "installSuccessTitle": "Update installed",
  "relaunchFailedHint": "Installation complete, but automatic restart failed. Please close and reopen the app to use the new version.",
  "notReadyMessage": "Update not ready yet. Please wait or restart the app.",
  "offlineHint": "Network unavailable, cannot install update. Please try again later.",
  "downloadFailed": "Update download failed",
  "downloadFailedDesc": "Please try again later or download the new version manually"
}
```

- [ ] **Step 3: 验证**

Run: `pnpm build 2>&1 | tail -5`
Expected: 构建成功（i18n key 在构建时不做静态检查，但确保 JSON 合法）

- [ ] **Step 4: Commit**

```bash
git add src/i18n/zh-CN.json src/i18n/en-US.json
git commit -m "feat(updater): add i18n keys for dialog redesign"
```

---

### Task 2: updaterStore 状态机重构

**Files:**
- Modify: `src/lib/updaterStore.ts`

- [ ] **Step 1: 更新 Phase 类型和 State 接口**

将 `src/lib/updaterStore.ts` 的 Phase 和 interface 替换为：

```typescript
type Phase = 'idle' | 'checking' | 'available' | 'downloading' | 'ready' | 'failed' | 'installing'

interface UpdaterState {
  phase: Phase
  version: string | null
  notes: string
  progress: { downloaded: number; total: number } | null
  panelOpen: boolean
  online: boolean
  error: string | null
  _update: Update | null
  _downloaded: boolean
  _bootstrapPromise: Promise<void> | null

  bootstrap(): Promise<void>
  startDownload(): Promise<void>
  openPanel(): void
  closePanel(): void
  installNow(): Promise<void>
}
```

- [ ] **Step 2: 重写 bootstrap() — 只做 check，不做 download**

替换现有 `bootstrap()` 方法体（从 `async bootstrap()` 到对应的闭合 `}`）：

```typescript
  async bootstrap() {
    const inFlight = get()._bootstrapPromise
    if (inFlight) return inFlight

    let resolveHolder!: () => void
    const holder = new Promise<void>((r) => { resolveHolder = r })
    set({ _bootstrapPromise: holder })

    const run = (async () => {
      if (typeof navigator !== 'undefined' && !networkListenersInstalled) {
        networkListenersInstalled = true
        window.addEventListener('online', () => set({ online: true }))
        window.addEventListener('offline', () => set({ online: false }))
      }

      set({ phase: 'checking', error: null })

      let update: Update | null = null
      try {
        update = await check()
      } catch (e) {
        console.warn('[updater] check failed:', e)
        set({ phase: 'idle' })
        return
      }

      if (!update) {
        set({ phase: 'idle', version: null, notes: '', progress: null, _update: null, _downloaded: false })
        return
      }

      const currentVersion = await getVersion()
      if (update.version === currentVersion) {
        set({ phase: 'idle', version: null, notes: '', progress: null, _update: null, _downloaded: false })
        return
      }

      set({
        _update: update,
        version: update.version,
        notes: update.body ?? '',
        phase: 'available',
        progress: null,
        _downloaded: false,
        error: null,
      })
    })()

    try { await run } finally {
      resolveHolder()
      if (get()._bootstrapPromise === holder) set({ _bootstrapPromise: null })
    }
  },
```

- [ ] **Step 3: 新增 startDownload() 方法**

在 `closePanel()` 之后、`installNow()` 之前，添加：

```typescript
  async startDownload() {
    const { _update, phase } = get()
    if (!_update || (phase !== 'available' && phase !== 'failed')) return

    set({ phase: 'downloading', progress: { downloaded: 0, total: 0 }, error: null, _downloaded: false })

    let total = 0
    let downloaded = 0
    try {
      await _update.download((event) => {
        if (event.event === 'Started') {
          total = event.data.contentLength ?? 0
        } else if (event.event === 'Progress') {
          downloaded += event.data.chunkLength
          set({ progress: { downloaded, total } })
        }
      })
      set({ phase: 'ready', progress: { downloaded, total }, _downloaded: true })
    } catch (e) {
      console.warn('[updater] download failed:', e)
      set({ phase: 'failed', _downloaded: false, error: String((e as Error)?.message ?? e) })
    }
  },
```

- [ ] **Step 4: 更新 store 初始状态**

在 store 初始状态中新增 `error: null`：

```typescript
export const useUpdaterStore = create<UpdaterState>()((set, get) => ({
  phase: 'idle',
  version: null,
  notes: '',
  progress: null,
  panelOpen: false,
  online: typeof navigator !== 'undefined' ? navigator.onLine : true,
  error: null,
  _update: null,
  _downloaded: false,
  _bootstrapPromise: null,
  // ... methods
```

- [ ] **Step 5: 验证编译**

Run: `pnpm build 2>&1 | tail -5`
Expected: 构建成功

- [ ] **Step 6: Commit**

```bash
git add src/lib/updaterStore.ts
git commit -m "feat(updater): refactor state machine — check-only bootstrap + user-triggered download"
```

---

### Task 3: UpdateAvailableLink — 支持多 phase 显示

**Files:**
- Modify: `src/components/layout/UpdateAvailableLink.tsx`

- [ ] **Step 1: 重写组件**

替换整个文件内容：

```tsx
import { useTranslation } from 'react-i18next'
import { useUpdaterStore } from '@/lib/updaterStore'

const DOT_COLORS: Record<string, string> = {
  available: '#ef4444',
  downloading: '#3b82f6',
  ready: '#22c55e',
  failed: '#f59e0b',
}

export function UpdateAvailableLink() {
  const { t } = useTranslation()
  const phase = useUpdaterStore((s) => s.phase)
  const version = useUpdaterStore((s) => s.version)
  const progress = useUpdaterStore((s) => s.progress)
  const openPanel = useUpdaterStore((s) => s.openPanel)
  const bootstrap = useUpdaterStore((s) => s.bootstrap)

  const dotColor = DOT_COLORS[phase]
  if (!dotColor || !version) return null

  const pct = progress && progress.total > 0
    ? Math.round((progress.downloaded / progress.total) * 100)
    : 0

  let label: string
  let tooltip: string
  let onClick: () => void

  if (phase === 'failed') {
    label = t('updater.linkFailed')
    tooltip = t('updater.linkFailedTooltip')
    onClick = () => void bootstrap()
  } else if (phase === 'downloading') {
    label = t('updater.linkDownloading', { version, pct })
    tooltip = t('updater.linkDownloadingTooltip')
    onClick = openPanel
  } else if (phase === 'ready') {
    label = t('updater.linkReady', { version })
    tooltip = t('updater.linkReadyTooltip')
    onClick = openPanel
  } else {
    label = t('updater.linkAvailable', { version })
    tooltip = t('updater.linkAvailableTooltip')
    onClick = openPanel
  }

  return (
    <button
      type="button"
      onClick={onClick}
      onMouseDown={(e) => e.stopPropagation()}
      title={tooltip}
      className="mr-2 flex h-6 shrink-0 items-center gap-1.5 rounded-md px-2 text-xs font-medium text-primary-foreground/95 transition-colors hover:bg-white/10"
    >
      <span
        className="inline-block h-1.5 w-1.5 rounded-full ring-1 ring-white/75"
        style={{ background: dotColor }}
        aria-hidden
      />
      <span>{label}</span>
    </button>
  )
}
```

- [ ] **Step 2: 验证编译**

Run: `pnpm build 2>&1 | tail -5`
Expected: 构建成功

- [ ] **Step 3: Commit**

```bash
git add src/components/layout/UpdateAvailableLink.tsx
git commit -m "feat(updater): UpdateAvailableLink supports available/downloading/ready/failed phases"
```

---

### Task 4: TitleBar — 扩大 link 显示条件

**Files:**
- Modify: `src/components/layout/TitleBar.tsx`

- [ ] **Step 1: 修改 TitleBar 组件**

将 `TitleBar` 中：

```typescript
const updateReady = useUpdaterStore((s) => s.phase === 'ready')
```

替换为：

```typescript
const showUpdateLink = useUpdaterStore((s) =>
  s.phase === 'available' || s.phase === 'downloading' || s.phase === 'ready' || s.phase === 'failed'
)
```

然后将模板中所有 `updateReady` 替换为 `showUpdateLink`（共 2 处：macOS 分支和 Windows 分支的条件渲染）。

- [ ] **Step 2: 验证编译**

Run: `pnpm build 2>&1 | tail -5`
Expected: 构建成功

- [ ] **Step 3: Commit**

```bash
git add src/components/layout/TitleBar.tsx
git commit -m "feat(updater): TitleBar shows update link for available/downloading/ready/failed phases"
```

---

### Task 5: UpdaterPanel — 重写为 Dialog 三阶段 UI

**Files:**
- Modify: `src/components/common/UpdaterPanel.tsx`

- [ ] **Step 1: 重写组件**

替换整个文件内容：

```tsx
import { useTranslation } from 'react-i18next'
import { getVersion } from '@tauri-apps/api/app'
import { useEffect, useState } from 'react'
import { useUpdaterStore } from '@/lib/updaterStore'
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
} from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'

function formatBytes(n: number): string {
  if (n <= 0) return '0 B'
  const units = ['B', 'KB', 'MB', 'GB']
  let i = 0
  let v = n
  while (v >= 1024 && i < units.length - 1) { v /= 1024; i++ }
  return `${v.toFixed(v >= 10 || i === 0 ? 0 : 1)} ${units[i]}`
}

export function UpdaterPanel() {
  const { t } = useTranslation()
  const open = useUpdaterStore((s) => s.panelOpen)
  const phase = useUpdaterStore((s) => s.phase)
  const version = useUpdaterStore((s) => s.version)
  const notes = useUpdaterStore((s) => s.notes)
  const progress = useUpdaterStore((s) => s.progress)
  const error = useUpdaterStore((s) => s.error)
  const online = useUpdaterStore((s) => s.online)
  const closePanel = useUpdaterStore((s) => s.closePanel)
  const startDownload = useUpdaterStore((s) => s.startDownload)
  const installNow = useUpdaterStore((s) => s.installNow)

  const [currentVersion, setCurrentVersion] = useState('')
  useEffect(() => {
    if (open) void getVersion().then(setCurrentVersion)
  }, [open])

  if (!version) return null

  const pct = progress && progress.total > 0
    ? Math.round((progress.downloaded / progress.total) * 100)
    : 0

  const bullets = notes
    .split(/\r?\n/)
    .map((line) => line.replace(/^[-•·]\s*/, '').trim())
    .filter(Boolean)

  const dialogTitle =
    phase === 'downloading' ? t('updater.dialogTitleDownloading', { version })
    : phase === 'ready' ? t('updater.dialogTitleReady', { version })
    : phase === 'failed' ? t('updater.dialogTitleFailed')
    : t('updater.dialogTitleAvailable', { version })

  return (
    <Dialog open={open} onOpenChange={(v) => { if (!v) closePanel() }}>
      <DialogContent className="max-w-md overflow-hidden">
        <DialogHeader>
          <DialogTitle>{dialogTitle}</DialogTitle>
          {currentVersion && phase !== 'failed' && (
            <p className="text-sm text-muted-foreground">
              {t('updater.versionLine', { current: currentVersion, next: version })}
            </p>
          )}
        </DialogHeader>

        {/* Phase: available — release notes */}
        {phase === 'available' && (
          <div className="space-y-3">
            {bullets.length > 0 ? (
              <>
                <p className="text-xs font-medium uppercase tracking-wide text-muted-foreground">
                  {t('updater.releaseNotesHeader')}
                </p>
                <ul className="space-y-1.5 pl-5 list-disc text-sm text-foreground/80">
                  {bullets.map((line, i) => <li key={i}>{line}</li>)}
                </ul>
              </>
            ) : (
              <p className="text-sm text-muted-foreground">{t('updater.updateAvailableDesc')}</p>
            )}
          </div>
        )}

        {/* Phase: downloading — progress bar */}
        {phase === 'downloading' && (
          <div className="space-y-3 py-2">
            <div className="h-2 w-full overflow-hidden rounded-full bg-muted">
              <div
                className="h-full rounded-full bg-primary transition-all duration-300"
                style={{ width: `${pct}%` }}
              />
            </div>
            <p className="text-center text-sm text-muted-foreground">
              {t('updater.downloadProgress', {
                downloaded: formatBytes(progress?.downloaded ?? 0),
                total: formatBytes(progress?.total ?? 0),
              })}
            </p>
          </div>
        )}

        {/* Phase: ready — download complete */}
        {phase === 'ready' && (
          <div className="py-4 text-center">
            <p className="text-sm text-foreground">
              {t('updater.downloadComplete', { size: formatBytes(progress?.total ?? 0) })}
            </p>
          </div>
        )}

        {/* Phase: failed — error message */}
        {phase === 'failed' && (
          <div className="py-4 text-center">
            <p className="text-sm text-destructive">
              {t('updater.downloadFailedMessage', { error: error ?? '' })}
            </p>
          </div>
        )}

        {/* Phase: installing — spinner */}
        {phase === 'installing' && (
          <div className="flex items-center justify-center py-6">
            <p className="text-sm text-muted-foreground">{t('updater.installing')}</p>
          </div>
        )}

        <DialogFooter>
          {phase === 'available' && (
            <>
              <Button variant="outline" onClick={closePanel}>{t('updater.updateLater')}</Button>
              <Button onClick={() => void startDownload()}>{t('updater.updateNow')}</Button>
            </>
          )}
          {phase === 'ready' && (
            <Button
              onClick={() => void installNow()}
              disabled={!online}
              title={!online ? t('updater.offlineHint') : undefined}
            >
              {t('updater.installAndRestart')}
            </Button>
          )}
          {phase === 'failed' && (
            <Button onClick={() => void startDownload()}>{t('updater.retry')}</Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
```

- [ ] **Step 2: 验证编译**

Run: `pnpm build 2>&1 | tail -5`
Expected: 构建成功

- [ ] **Step 3: Commit**

```bash
git add src/components/common/UpdaterPanel.tsx
git commit -m "feat(updater): rewrite UpdaterPanel as Dialog with 3-phase UI"
```

---

### Task 6: SettingsModal — 适配新 phase

**Files:**
- Modify: `src/components/settings/SettingsModal.tsx`

- [ ] **Step 1: 更新 onCheckUpdate**

将 `onCheckUpdate` 函数中：

```typescript
      if (phase === 'idle') {
        await message(t('settings.about.alreadyLatestVersion'), { title: productName, kind: 'info' })
      } else if (phase !== 'failed') {
        // downloading / ready / installing → show the panel
        store.openPanel()
      }
      // 'failed' → download-failure toast already shown by bootstrap()
```

替换为：

```typescript
      if (phase === 'idle' || phase === 'checking') {
        await message(t('settings.about.alreadyLatestVersion'), { title: productName, kind: 'info' })
      } else {
        store.openPanel()
      }
```

- [ ] **Step 2: 验证编译**

Run: `pnpm build 2>&1 | tail -5`
Expected: 构建成功

- [ ] **Step 3: Commit**

```bash
git add src/components/settings/SettingsModal.tsx
git commit -m "feat(updater): SettingsModal opens dialog for all non-idle phases"
```

---

### Task 7: 测试 — 适配新状态机

**Files:**
- Modify: `src/lib/updaterStore.test.ts`

- [ ] **Step 1: 重写测试文件**

替换整个 `src/lib/updaterStore.test.ts`：

```typescript
import { afterEach, describe, expect, it, vi } from 'vitest'

const checkMock = vi.fn()
const relaunchMock = vi.fn()
const getVersionMock = vi.fn()

vi.mock('@tauri-apps/plugin-updater', () => ({ check: (...a: unknown[]) => checkMock(...a) }))
vi.mock('@tauri-apps/plugin-process', () => ({ relaunch: (...a: unknown[]) => relaunchMock(...a) }))
vi.mock('@tauri-apps/api/app', () => ({ getVersion: (...a: unknown[]) => getVersionMock(...a) }))

async function loadModules() {
  vi.resetModules()
  const storeMod = await import('./updaterStore')
  const notifMod = await import('@/stores/notificationStore')
  return { useUpdaterStore: storeMod.useUpdaterStore, useNotificationStore: notifMod.useNotificationStore }
}

afterEach(() => { vi.clearAllMocks() })

describe('updaterStore.bootstrap', () => {
  it('stays idle when no update is available', async () => {
    checkMock.mockResolvedValue(null)
    getVersionMock.mockResolvedValue('0.5.29')
    const { useUpdaterStore: useStore } = await loadModules()
    await useStore.getState().bootstrap()
    expect(useStore.getState().phase).toBe('idle')
    expect(useStore.getState().version).toBeNull()
  })

  it('transitions to checking then available when update exists', async () => {
    const fakeUpdate = { version: '0.5.30', body: 'Fixes' }
    checkMock.mockResolvedValue(fakeUpdate)
    getVersionMock.mockResolvedValue('0.5.29')
    const { useUpdaterStore: useStore } = await loadModules()
    await useStore.getState().bootstrap()
    expect(useStore.getState().phase).toBe('available')
    expect(useStore.getState().version).toBe('0.5.30')
    expect(useStore.getState().notes).toBe('Fixes')
  })

  it('stays idle when server version equals current', async () => {
    checkMock.mockResolvedValue({ version: '0.5.29', body: '' })
    getVersionMock.mockResolvedValue('0.5.29')
    const { useUpdaterStore: useStore } = await loadModules()
    await useStore.getState().bootstrap()
    expect(useStore.getState().phase).toBe('idle')
  })

  it('deduplicates concurrent bootstrap calls', async () => {
    checkMock.mockResolvedValue({ version: '0.5.30', body: '' })
    getVersionMock.mockResolvedValue('0.5.29')
    const { useUpdaterStore: useStore } = await loadModules()
    const p1 = useStore.getState().bootstrap()
    const p2 = useStore.getState().bootstrap()
    await p1
    await p2
    expect(checkMock).toHaveBeenCalledTimes(1)
  })
})

describe('updaterStore.startDownload', () => {
  it('downloads and transitions to ready', async () => {
    const fakeUpdate = {
      version: '0.5.30',
      body: '',
      download: vi.fn(async (cb: (e: { event: string; data: { contentLength?: number; chunkLength?: number } }) => void) => {
        cb({ event: 'Started', data: { contentLength: 200 } })
        cb({ event: 'Progress', data: { chunkLength: 200 } })
      }),
    }
    checkMock.mockResolvedValue(fakeUpdate)
    getVersionMock.mockResolvedValue('0.5.29')
    const { useUpdaterStore: useStore } = await loadModules()
    await useStore.getState().bootstrap()
    expect(useStore.getState().phase).toBe('available')

    await useStore.getState().startDownload()
    expect(useStore.getState().phase).toBe('ready')
    expect(useStore.getState().progress).toEqual({ downloaded: 200, total: 200 })
  })

  it('transitions to failed on download error', async () => {
    const fakeUpdate = {
      version: '0.5.30',
      body: '',
      download: vi.fn().mockRejectedValue(new Error('network timeout')),
    }
    checkMock.mockResolvedValue(fakeUpdate)
    getVersionMock.mockResolvedValue('0.5.29')
    const { useUpdaterStore: useStore } = await loadModules()
    await useStore.getState().bootstrap()
    await useStore.getState().startDownload()
    expect(useStore.getState().phase).toBe('failed')
    expect(useStore.getState().error).toBe('network timeout')
  })

  it('does nothing when phase is not available or failed', async () => {
    const { useUpdaterStore: useStore } = await loadModules()
    await useStore.getState().startDownload()
    expect(useStore.getState().phase).toBe('idle')
  })

  it('can retry from failed state', async () => {
    let callCount = 0
    const fakeUpdate = {
      version: '0.5.30',
      body: '',
      download: vi.fn(async (cb: (e: { event: string; data: { contentLength?: number; chunkLength?: number } }) => void) => {
        callCount++
        if (callCount === 1) throw new Error('transient')
        cb({ event: 'Started', data: { contentLength: 100 } })
        cb({ event: 'Progress', data: { chunkLength: 100 } })
      }),
    }
    checkMock.mockResolvedValue(fakeUpdate)
    getVersionMock.mockResolvedValue('0.5.29')
    const { useUpdaterStore: useStore } = await loadModules()
    await useStore.getState().bootstrap()

    await useStore.getState().startDownload()
    expect(useStore.getState().phase).toBe('failed')

    await useStore.getState().startDownload()
    expect(useStore.getState().phase).toBe('ready')
  })
})

describe('updaterStore.installNow', () => {
  it('installs and relaunches when ready', async () => {
    const fakeUpdate = {
      version: '0.5.30',
      body: '',
      download: vi.fn(async (cb: (e: { event: string; data: { contentLength?: number; chunkLength?: number } }) => void) => {
        cb({ event: 'Started', data: { contentLength: 1 } })
        cb({ event: 'Progress', data: { chunkLength: 1 } })
      }),
      install: vi.fn().mockResolvedValue(undefined),
    }
    checkMock.mockResolvedValue(fakeUpdate)
    getVersionMock.mockResolvedValue('0.5.29')
    relaunchMock.mockResolvedValue(undefined)
    const { useUpdaterStore: useStore } = await loadModules()
    await useStore.getState().bootstrap()
    await useStore.getState().startDownload()
    await useStore.getState().installNow()
    expect(fakeUpdate.install).toHaveBeenCalled()
    expect(relaunchMock).toHaveBeenCalled()
    expect(useStore.getState().phase).toBe('idle')
  })

  it('shows toast when not ready', async () => {
    const { useUpdaterStore: useStore, useNotificationStore } = await loadModules()
    useNotificationStore.getState().dismissAll()
    await useStore.getState().installNow()
    const notes = useNotificationStore.getState().notifications
    expect(notes.length).toBe(1)
    expect(notes[0].level).toBe('error')
  })

  it('blocks install when offline', async () => {
    const fakeUpdate = {
      version: '0.5.30',
      body: '',
      download: vi.fn(async (cb: (e: { event: string; data: { contentLength?: number; chunkLength?: number } }) => void) => {
        cb({ event: 'Started', data: { contentLength: 1 } })
        cb({ event: 'Progress', data: { chunkLength: 1 } })
      }),
      install: vi.fn(),
    }
    checkMock.mockResolvedValue(fakeUpdate)
    getVersionMock.mockResolvedValue('0.5.29')
    const { useUpdaterStore: useStore, useNotificationStore } = await loadModules()
    useNotificationStore.getState().dismissAll()
    await useStore.getState().bootstrap()
    await useStore.getState().startDownload()
    useStore.setState({ online: false })
    await useStore.getState().installNow()
    expect(fakeUpdate.install).not.toHaveBeenCalled()
  })
})
```

- [ ] **Step 2: 运行测试**

Run: `pnpm exec vitest run src/lib/updaterStore.test.ts`
Expected: 所有测试通过

- [ ] **Step 3: Commit**

```bash
git add src/lib/updaterStore.test.ts
git commit -m "test(updater): rewrite tests for new state machine with check/download/install phases"
```

---

### Task 8: 端到端验证

- [ ] **Step 1: 全量测试**

Run: `pnpm test 2>&1 | tail -10`
Expected: 全部通过

- [ ] **Step 2: Lint**

Run: `pnpm lint 2>&1 | grep updater`
Expected: 无 updater 相关的新 lint 错误

- [ ] **Step 3: 构建**

Run: `pnpm build 2>&1 | tail -5`
Expected: 构建成功

- [ ] **Step 4: 开发模式验证**

Run: `pnpm tauri:dev`
验证：
1. 启动后如果有新版本，右上角出现 "v? 可用"（蓝色圆点变红色）
2. 点击右上角 → 弹出 Dialog 显示 release notes
3. 点击"立即更新" → 进度条出现
4. 关闭弹窗 → 后台继续下载，右上角显示进度百分比
5. 下载完成 → 右上角变"可安装"，弹窗显示安装按钮
6. 设置 → 关于 → 检查更新 → 同样的弹窗流程
