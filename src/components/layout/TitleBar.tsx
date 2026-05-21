import React from 'react'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { UpdateAvailableLink } from './UpdateAvailableLink'
import { useUpdaterStore } from '@/lib/updaterStore'

function handleDragStart(e: React.MouseEvent) {
  if (e.buttons === 1 && e.detail === 2) {
    void getCurrentWindow().toggleMaximize()
    return
  }
  if (e.buttons === 1 && e.detail === 1) {
    getCurrentWindow().startDragging()
  }
}

function WindowControls() {
  // hover bg uses primary-foreground/15 so it follows tenant theme; close
  // button hover routes to --destructive instead of hardcoded red.
  const btnClass =
    'flex h-7 w-11 items-center justify-center text-primary-foreground/70 transition-colors hover:bg-primary-foreground/15 hover:text-primary-foreground'
  return (
    <div className="flex shrink-0 items-center" onMouseDown={(e) => e.stopPropagation()}>
      <button className={btnClass} onClick={() => getCurrentWindow().minimize()} aria-label="Minimize">
        <svg width="10" height="1" viewBox="0 0 10 1"><rect fill="currentColor" width="10" height="1"/></svg>
      </button>
      <button className={btnClass} onClick={() => getCurrentWindow().toggleMaximize()} aria-label="Maximize">
        <svg width="10" height="10" viewBox="0 0 10 10"><rect fill="none" stroke="currentColor" strokeWidth="1" x="0.5" y="0.5" width="9" height="9"/></svg>
      </button>
      <button
        className={`${btnClass} hover:!bg-destructive hover:!text-destructive-foreground`}
        onClick={() => getCurrentWindow().close()}
        aria-label="Close"
      >
        <svg width="10" height="10" viewBox="0 0 10 10"><path fill="currentColor" d="M1.7.3.3 1.7 3.6 5 .3 8.3l1.4 1.4L5 6.4l3.3 3.3 1.4-1.4L6.4 5l3.3-3.3L8.3.3 5 3.6 1.7.3z"/></svg>
      </button>
    </div>
  )
}

/**
 * dev 模式下：底色 --primary，斜纹另一档用 --primary 派生的"淡 60%"实色
 * （color-mix 朝白色混 60%），两条都是实色（不透明），不论租户 accent
 * 是什么颜色都自动协调。45° 斜纹 + 中央 DEV 标签。
 */
const DEV_STRIPE_STYLE: React.CSSProperties = {
  backgroundColor: 'var(--primary)',
  backgroundImage:
    'repeating-linear-gradient(45deg, var(--primary) 0 10px, color-mix(in srgb, var(--primary), #000 10%) 10px 20px)',
}

// DEV badge: not tenant-themed by design (it's a build-mode marker, not UI).
// Color picked from semantic blue so it stays distinct from any tenant accent.
function DevBadge() {
  return (
    <span
      className="pointer-events-none mr-2 rounded-sm bg-[var(--color-semantic-purple)] px-1.5 py-0.5 text-[11px] font-semibold tracking-widest text-primary-foreground shadow-[var(--shadow-sm)]"
    >
      DEV
    </span>
  )
}

/**
 * Both macOS (Overlay titleBarStyle) and Windows render a 28px accent strip
 * at the top so tenant-branded accent color is visible at the most prominent
 * area of the window. macOS draws native traffic lights over this strip.
 */
export function TitleBar() {
  const updateReady = useUpdaterStore((s) => s.phase === 'ready')
  const isWindows = navigator.userAgent.includes('Windows')
  const isDev = import.meta.env.DEV

  const barClass = isDev
    ? 'flex h-8 w-full shrink-0 items-center'
    : 'flex h-8 w-full shrink-0 items-center bg-primary text-primary-foreground'
  const barStyle = isDev ? DEV_STRIPE_STYLE : undefined

  if (!isWindows) {
    return (
      <div
        data-tauri-drag-region
        className={`${barClass} justify-end`}
        style={barStyle}
      >
        {updateReady ? (
          <div className="pr-3" onMouseDown={(e) => e.stopPropagation()}>
            <UpdateAvailableLink />
          </div>
        ) : null}
        {isDev ? <DevBadge /> : null}
      </div>
    )
  }

  return (
    <div
      data-tauri-drag-region
      className={`${barClass} ${isDev ? '' : 'border-b border-primary-foreground/15'}`}
      style={barStyle}
      onMouseDown={handleDragStart}
    >
      <div className="flex-1" data-tauri-drag-region />
      <div onMouseDown={(e) => e.stopPropagation()}>
        <UpdateAvailableLink />
      </div>
      {isDev ? <DevBadge /> : null}
      <WindowControls />
    </div>
  )
}
