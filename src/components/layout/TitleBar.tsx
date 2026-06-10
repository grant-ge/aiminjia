import React from 'react'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { UpdateAvailableLink } from './UpdateAvailableLink'
import { TitleBarEnvSwitcher } from './TitleBarEnvSwitcher'
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
  // Keep window controls readable on the sidebar-colored title bar; close
  // button hover routes to --destructive instead of hardcoded red.
  const btnClass =
    'flex h-7 w-11 items-center justify-center text-sidebar-foreground/70 transition-colors hover:bg-sidebar-accent/70 hover:text-sidebar-foreground'
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
 * The native drag strip uses the same surface color as the left sidebar so the
 * window chrome and app navigation read as one continuous shell.
 */
const TITLE_BAR_STYLE: React.CSSProperties = {
  backgroundColor: 'var(--sidebar)',
}

// "DEV" or "DEV 5174" when a vite dev port is detectable.  Including the port
// makes multi-instance dev (two vite servers side by side) visually
// distinguishable.  Pass an explicit `port` for tests; otherwise reads
// `window.location.port` at call time.
export function getDevBadgeLabel(port?: string): string {
  const detected = port ?? (typeof window !== 'undefined' ? window.location.port : '')
  return detected ? `DEV ${detected}` : 'DEV'
}

// DEV badge: not tenant-themed by design (it's a build-mode marker, not UI).
// Color picked from semantic blue so it stays distinct from any tenant accent.
function DevBadge() {
  return (
    <span
      className="pointer-events-none mr-2 rounded-md bg-[var(--color-semantic-purple)] px-1.5 py-0.5 text-[11px] font-semibold tracking-widest text-primary-foreground shadow-[var(--shadow-sm)]"
    >
      {getDevBadgeLabel()}
    </span>
  )
}

/**
 * Both macOS (Overlay titleBarStyle) and Windows render a 28px shell strip at
 * the top. macOS draws native traffic lights over this strip.
 */
export function TitleBar() {
  const showUpdateLink = useUpdaterStore((s) =>
    s.phase === 'available' || s.phase === 'downloading' || s.phase === 'ready' || s.phase === 'failed'
  )
  const isWindows = navigator.userAgent.includes('Windows')
  const isDev = import.meta.env.DEV

  const barClass = 'flex h-8 w-full shrink-0 items-center text-sidebar-foreground'
  const barStyle = TITLE_BAR_STYLE

  if (!isWindows) {
    return (
      <div
        data-tauri-drag-region
        className={`${barClass} justify-end`}
        style={barStyle}
      >
        {showUpdateLink ? (
          <div className="pr-3" onMouseDown={(e) => e.stopPropagation()}>
            <UpdateAvailableLink />
          </div>
        ) : null}
        {isDev ? <TitleBarEnvSwitcher /> : null}
        {isDev ? <DevBadge /> : null}
      </div>
    )
  }

  return (
    <div
      data-tauri-drag-region
      className={`${barClass} ${isDev ? '' : 'border-b border-sidebar-border'}`}
      style={barStyle}
      onMouseDown={handleDragStart}
    >
      <div className="flex-1" data-tauri-drag-region />
      <div onMouseDown={(e) => e.stopPropagation()}>
        <UpdateAvailableLink />
      </div>
      {isDev ? <TitleBarEnvSwitcher /> : null}
      {isDev ? <DevBadge /> : null}
      <WindowControls />
    </div>
  )
}
