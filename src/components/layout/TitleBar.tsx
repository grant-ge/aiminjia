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
    'flex h-7 w-11 items-center justify-center text-primary-foreground/70 transition-colors hover:bg-black/15 hover:text-primary-foreground'
  return (
    <div className="flex shrink-0 items-center" onMouseDown={(e) => e.stopPropagation()}>
      <button className={btnClass} onClick={() => getCurrentWindow().minimize()} aria-label="Minimize">
        <svg width="10" height="1" viewBox="0 0 10 1"><rect fill="currentColor" width="10" height="1"/></svg>
      </button>
      <button className={btnClass} onClick={() => getCurrentWindow().toggleMaximize()} aria-label="Maximize">
        <svg width="10" height="10" viewBox="0 0 10 10"><rect fill="none" stroke="currentColor" strokeWidth="1" x="0.5" y="0.5" width="9" height="9"/></svg>
      </button>
      <button
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
 * Both macOS (Overlay titleBarStyle) and Windows render a 28px accent strip
 * at the top so tenant-branded accent color is visible at the most prominent
 * area of the window. macOS draws native traffic lights over this strip.
 */
export function TitleBar() {
  const updateReady = useUpdaterStore((s) => s.phase === 'ready')
  const isWindows = navigator.userAgent.includes('Windows')

  if (!isWindows) {
    return (
      <div
        data-tauri-drag-region
        className="flex h-8 w-full shrink-0 items-center justify-end bg-primary text-primary-foreground"
      >
        {updateReady ? (
          <div className="pr-3" onMouseDown={(e) => e.stopPropagation()}>
            <UpdateAvailableLink />
          </div>
        ) : null}
      </div>
    )
  }

  return (
    <div
      data-tauri-drag-region
      className="flex h-8 w-full shrink-0 items-center border-b border-primary-foreground/15 bg-primary text-primary-foreground"
      onMouseDown={handleDragStart}
    >
      <div className="flex-1" data-tauri-drag-region />
      <div onMouseDown={(e) => e.stopPropagation()}>
        <UpdateAvailableLink />
      </div>
      <WindowControls />
    </div>
  )
}
