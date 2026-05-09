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
  const btnClass =
    'flex h-7 w-11 items-center justify-center text-sidebar-foreground/40 transition-colors hover:bg-sidebar-accent hover:text-sidebar-foreground/80'
  return (
    <div className="flex shrink-0" onMouseDown={e => e.stopPropagation()}>
      <button className={btnClass} onClick={() => getCurrentWindow().minimize()} aria-label="Minimize">
        <svg width="10" height="1" viewBox="0 0 10 1"><rect fill="currentColor" width="10" height="1"/></svg>
      </button>
      <button className={btnClass} onClick={() => getCurrentWindow().toggleMaximize()} aria-label="Maximize">
        <svg width="10" height="10" viewBox="0 0 10 10"><rect fill="none" stroke="currentColor" strokeWidth="1" x="0.5" y="0.5" width="9" height="9"/></svg>
      </button>
      <button
        // Windows 关闭按钮固定红底 + 白字（对齐 Win 系统外观，不随主题）
        className={`${btnClass} hover:!bg-red-600 hover:!text-white`}
        onClick={() => getCurrentWindow().close()}
        aria-label="Close"
      >
        <svg width="10" height="10" viewBox="0 0 10 10"><path fill="currentColor" d="M1.7.3.3 1.7 3.6 5 .3 8.3l1.4 1.4L5 6.4l3.3 3.3 1.4-1.4L6.4 5l3.3-3.3L8.3.3 5 3.6 1.7.3z"/></svg>
      </button>
    </div>
  )
}

/**
 * Windows: full title bar with drag region + window controls.
 * macOS: thin overlay strip (transparent, no border) carrying only the
 * UpdateAvailableLink — visible only when an update is ready, otherwise null.
 */
export function TitleBar() {
  const updateReady = useUpdaterStore((s) => s.phase === 'ready')
  const isWindows = navigator.userAgent.includes('Windows')

  if (!isWindows) {
    if (!updateReady) return null
    return (
      <div className="pointer-events-none fixed right-3 top-2 z-40 flex items-center">
        <div className="pointer-events-auto" onMouseDown={(e) => e.stopPropagation()}>
          <UpdateAvailableLink />
        </div>
      </div>
    )
  }

  return (
    <div
      data-tauri-drag-region
      className="flex h-7 w-full shrink-0 items-center border-b border-border bg-sidebar"
      onMouseDown={handleDragStart}
    >
      <div className="flex-1" data-tauri-drag-region />
      <UpdateAvailableLink />
      <WindowControls />
    </div>
  )
}
